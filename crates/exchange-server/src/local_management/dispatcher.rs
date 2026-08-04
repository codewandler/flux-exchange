//! One value-free entry point shared by native and hosted FXLM transports.

use std::sync::Arc;

use exchange_host::{GrantReceiptId, Principal, PrincipalKind, StoreRevision, Tenant};
use serde::Serialize;

use super::codec::{Direction, Frame, FrameError, Opcode, StreamDecoder};
use super::connection::{
    ActiveCeremony, AdvanceOutcome as ConnectionAdvance, BeginOutcome as ConnectionBegin,
    Ceremony as ConnectionCeremony, PublicationRefusal,
};
use super::deadline::{DeadlineController, Expired, ReceiptIdentity, Unresolved};
use super::grant::{GrantAudit, GrantAuditUnavailable, GrantCeremony};
use super::service_account::{OneShotWriter, ServiceAccountCeremony};
use super::TransactionCoordinator;
use crate::audit::{Action, AuditJournal, Outcome, RequestId, Target};
use crate::state::AppState;

impl GrantAudit for AuditJournal {
    fn commit(
        &self,
        tenant: &Tenant,
        _connector: &str,
        _revision: StoreRevision,
        receipt_id: GrantReceiptId,
    ) -> Result<(), GrantAuditUnavailable> {
        let request_id = RequestId::generate().map_err(|_| GrantAuditUnavailable)?;
        self.record_terminal_once(
            &receipt_id.to_string(),
            &request_id,
            Action::GrantsReplaced,
            Outcome::Succeeded,
            None,
            Target::Grants {
                tenant: tenant.as_str().to_owned(),
            },
        )
        .map(|_| ())
        .map_err(|_| GrantAuditUnavailable)
    }
}

/// Which authenticated transport admitted an exact FXLM operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Transport {
    Native,
    Hosted,
}

/// One hosted FXLM response and the exact empty-reason WebSocket close code that follows it.
pub(crate) struct HostedReply {
    bytes: Vec<u8>,
    close_code: u16,
}

/// First response for one exact authenticated logical operation.
pub(crate) enum SessionBegin {
    Terminal(HostedReply),
    Active {
        response: Vec<u8>,
        session: Box<ActiveSession>,
    },
}

/// One live interactive ceremony retained by its sole native/WebSocket connection.
pub(crate) struct ActiveSession {
    ceremony: ActiveCeremony,
    deadline: DeadlineController,
}

/// Result of one subsequent client frame.
pub(crate) enum SessionAdvance {
    Awaiting,
    Terminal(HostedReply),
}

impl ActiveSession {
    pub(crate) async fn accept_message(&mut self, bytes: &[u8]) -> SessionAdvance {
        let request = match exact_frame(bytes) {
            Ok(frame) => frame,
            Err(error) => {
                self.abort().await;
                return SessionAdvance::Terminal(decode_reply(error));
            }
        };
        self.accept_frame(request).await
    }

    pub(super) async fn accept_frame(&mut self, request: Frame) -> SessionAdvance {
        match self
            .deadline
            .race(self.ceremony.accept(request, &self.deadline))
            .await
        {
            Ok(ConnectionAdvance::Awaiting) => SessionAdvance::Awaiting,
            Ok(ConnectionAdvance::Terminal(frame)) => SessionAdvance::Terminal(hosted_frame(frame)),
            Err(expired) => SessionAdvance::Terminal(expired_reply(expired)),
        }
    }

    pub(crate) async fn abort(&mut self) {
        if self.deadline.may_abort() {
            self.ceremony.abort().await;
        }
    }
}

impl HostedReply {
    pub(crate) fn into_parts(self) -> (Vec<u8>, u16) {
        (self.bytes, self.close_code)
    }
}

/// Canonical pre-decision absolute-deadline outcome shared by native and hosted transports.
#[cfg(test)]
pub(crate) fn deadline_frame() -> Vec<u8> {
    error_frame(body("deadline_exceeded", 408, "refresh"))
}

/// Terminal deadline outcome selected from the durable phase at the exact expiry boundary.
pub(crate) fn expired_reply(expired: Expired) -> HostedReply {
    hosted_frame(expired_frame(expired))
}

fn expired_frame(expired: Expired) -> Frame {
    match expired {
        Expired::PreDecision => refusal("deadline_exceeded", 408, "refresh"),
        Expired::PostDecision {
            receipt,
            unresolved,
        } => postdeadline_refusal(receipt, unresolved),
    }
}

fn worker_internal_refusal(deadline: &DeadlineController) -> Frame {
    if let Some(receipt) = deadline.decision_receipt() {
        deadline.unresolved(Unresolved::Internal);
        postdeadline_refusal(receipt, Unresolved::Internal)
    } else {
        deadline.terminal();
        refusal("internal_refusal", 500, "operator")
    }
}

/// Shared local-management authority after transport authentication.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    state: AppState,
    coordinator: Arc<TransactionCoordinator>,
    connection: ConnectionCeremony,
    grant: Option<GrantCeremony>,
    service_account: Option<ServiceAccountCeremony>,
}

impl Dispatcher {
    pub(crate) fn from_state(state: AppState) -> Result<Self, &'static str> {
        let coordinator = state
            .transaction_coordinator()
            .cloned()
            .ok_or("the local-management transaction coordinator is unavailable")?;
        Ok(Self::new(state, coordinator))
    }

    pub(crate) fn new(state: AppState, coordinator: Arc<TransactionCoordinator>) -> Self {
        let grant = state.grant_transactions().and_then(|grants| {
            state
                .audit()
                .map(|audit| GrantCeremony::new(grants.clone(), audit.clone()))
        });
        let service_account = state.service_accounts().map(|store| {
            ServiceAccountCeremony::bind_retained(store.clone())
                .expect("binding the retained Service Account store is infallible")
        });
        let connection = ConnectionCeremony::new(state.clone(), coordinator.clone());
        Self {
            state,
            coordinator,
            connection,
            grant,
            service_account,
        }
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    pub(crate) async fn begin_message(
        &self,
        transport: Transport,
        tenant: &Tenant,
        bytes: &[u8],
        deadline: &DeadlineController,
    ) -> SessionBegin {
        let request = match exact_frame(bytes) {
            Ok(frame) => frame,
            Err(error) => return SessionBegin::Terminal(decode_reply(error)),
        };
        self.begin_frame(transport, tenant, request, deadline).await
    }

    pub(super) async fn begin_frame(
        &self,
        transport: Transport,
        tenant: &Tenant,
        request: Frame,
        deadline: &DeadlineController,
    ) -> SessionBegin {
        self.begin_frame_with_writer(transport, tenant, request, None, deadline)
            .await
    }

    /// Begin one native operation with its separately authenticated one-shot writer capability.
    ///
    /// Only Service Account MINT consumes this port. Keeping it out-of-band from FXLM ensures the
    /// token can never be redirected by a path or handle spelling in caller-controlled bytes.
    pub(super) async fn begin_frame_with_writer(
        &self,
        transport: Transport,
        tenant: &Tenant,
        request: Frame,
        writer: Option<Box<dyn OneShotWriter>>,
        deadline: &DeadlineController,
    ) -> SessionBegin {
        if !admitted(transport, request.opcode()) {
            return SessionBegin::Terminal(hosted_frame(refusal("unexpected_frame", 409, "never")));
        }
        if writer.is_some() && request.opcode() != Opcode::ServiceAccountMint {
            return SessionBegin::Terminal(hosted_frame(refusal("unexpected_frame", 409, "never")));
        }
        if matches!(
            request.opcode(),
            Opcode::ConnectBegin
                | Opcode::ConnectQuery
                | Opcode::CredentialBegin
                | Opcode::CredentialQuery
        ) {
            return match self.connection.begin(tenant, request, deadline).await {
                ConnectionBegin::Terminal(frame) => SessionBegin::Terminal(hosted_frame(frame)),
                ConnectionBegin::Active { response, active } => SessionBegin::Active {
                    response: response.encode(),
                    session: Box::new(ActiveSession {
                        ceremony: *active,
                        deadline: deadline.clone(),
                    }),
                },
            };
        }
        SessionBegin::Terminal(hosted_frame(
            self.dispatch_terminal(transport, tenant, request, writer, deadline)
                .await,
        ))
    }

    pub(crate) fn recover_publications(&self) -> Result<(), PublicationRefusal> {
        self.connection.recover()
    }

    async fn dispatch_terminal(
        &self,
        transport: Transport,
        tenant: &Tenant,
        request: Frame,
        writer: Option<Box<dyn OneShotWriter>>,
        deadline: &DeadlineController,
    ) -> Frame {
        let _state = &self.state;
        let _coordinator = &self.coordinator;
        if matches!(
            request.opcode(),
            Opcode::GrantPreview | Opcode::GrantApply | Opcode::GrantQuery
        ) {
            let Some(grant) = &self.grant else {
                return refusal("local_management_unavailable", 503, "operator");
            };
            let Some(payload) = request.control_payload() else {
                return refusal("unexpected_frame", 422, "never");
            };
            let grant = grant.clone();
            let tenant = tenant.clone();
            let payload = payload.to_vec();
            let opcode = request.opcode() as u16;
            let worker_deadline = deadline.clone();
            let worker = tokio::task::spawn_blocking(move || {
                grant.handle_with_deadline(&tenant, opcode, &payload, &worker_deadline)
            });
            let response = match deadline.race(worker).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => return worker_internal_refusal(deadline),
                Err(expired) => return expired_frame(expired),
            };
            let opcode = Opcode::try_from(response.opcode())
                .expect("grant ceremonies return only closed FXLM opcodes");
            return Frame::control(
                Direction::ServerToClient,
                opcode,
                response.payload().to_vec(),
            )
            .expect("grant ceremony responses satisfy the shared control bound");
        }
        if request.opcode() == Opcode::PlanQuery {
            let Some(payload) = request.control_payload() else {
                return refusal("invalid_request", 400, "never");
            };
            return match crate::routes::native_plan_query(&self.state, tenant, payload) {
                Ok(plan) => Frame::control(Direction::ServerToClient, Opcode::PlanResponse, plan)
                    .expect("the validated connection plan satisfies the control-frame bound"),
                Err(crate::routes::NativePlanRefusal {
                    code,
                    status,
                    retry,
                }) => refusal(code, status, retry),
            };
        }
        if matches!(
            request.opcode(),
            Opcode::ServiceAccountMint | Opcode::ServiceAccountQuery
        ) {
            if transport != Transport::Native {
                return refusal("unexpected_frame", 409, "never");
            }
            let Some(ceremony) = &self.service_account else {
                return refusal("local_management_unavailable", 503, "operator");
            };
            let Some(payload) = request.control_payload() else {
                return refusal("unexpected_frame", 422, "never");
            };
            let ceremony = ceremony.clone();
            let tenant = tenant.clone();
            let payload = payload.to_vec();
            let opcode = request.opcode() as u16;
            let worker_deadline = deadline.clone();
            let worker = tokio::task::spawn_blocking(move || {
                let actor = Principal::new(PrincipalKind::User, "local-owner", tenant);
                ceremony.handle_with_deadline(&actor, opcode, &payload, writer, &worker_deadline)
            });
            let response = match deadline.race(worker).await {
                Ok(Ok(response)) => response,
                Ok(Err(_)) => return worker_internal_refusal(deadline),
                Err(expired) => return expired_frame(expired),
            };
            let opcode = Opcode::try_from(response.opcode())
                .expect("Service Account ceremonies return only closed FXLM opcodes");
            return Frame::control(
                Direction::ServerToClient,
                opcode,
                response.payload().to_vec(),
            )
            .expect("Service Account ceremony responses satisfy the shared control bound");
        }
        // Individual ceremonies are wired as their value-free projections and atomic stores land.
        // Keeping this refusal inside the shared authenticated dispatcher is deliberate: neither
        // transport may substitute a point-write path while one ceremony is unavailable.
        refusal("local_management_unavailable", 503, "operator")
    }
}

fn decode_reply(error: DecodeRefusal) -> HostedReply {
    let close_code = match error {
        DecodeRefusal::Frame(
            FrameError::FrameTooLarge { .. } | FrameError::InvalidSecretLength { .. },
        ) => 1009,
        _ => 1002,
    };
    HostedReply {
        bytes: error_frame(frame_error(error)),
        close_code,
    }
}

fn hosted_frame(frame: Frame) -> HostedReply {
    let close_code = if frame.opcode() == Opcode::Error {
        frame
            .control_payload()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(bytes).ok())
            .and_then(|value| {
                value
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned)
            })
            .map_or(1000, |code| match code.as_str() {
                "invalid_frame"
                | "unsupported_version"
                | "wrong_direction"
                | "unexpected_frame"
                | "truncated_frame"
                | "surplus_data" => 1002,
                "frame_too_large" => 1009,
                "deadline_exceeded" => 1008,
                _ => 1000,
            })
    } else {
        1000
    };
    HostedReply {
        bytes: frame.encode(),
        close_code,
    }
}

fn exact_frame(bytes: &[u8]) -> Result<Frame, DecodeRefusal> {
    let mut decoder = StreamDecoder::new(Direction::ClientToServer);
    decoder.push(bytes).map_err(DecodeRefusal::Frame)?;
    let frame = decoder
        .next_frame()
        .map_err(DecodeRefusal::Frame)?
        .ok_or(DecodeRefusal::Truncated)?;
    if decoder
        .next_frame()
        .map_err(DecodeRefusal::Frame)?
        .is_some()
    {
        return Err(DecodeRefusal::Surplus);
    }
    decoder.finish().map_err(DecodeRefusal::Frame)?;
    Ok(frame)
}

#[derive(Debug)]
enum DecodeRefusal {
    Frame(FrameError),
    Truncated,
    Surplus,
}

fn frame_error(error: DecodeRefusal) -> ErrorBody {
    match error {
        DecodeRefusal::Frame(FrameError::UnsupportedVersion(_)) => {
            body("unsupported_version", 426, "never")
        }
        DecodeRefusal::Frame(FrameError::WrongDirection { .. }) => {
            body("wrong_direction", 400, "never")
        }
        DecodeRefusal::Frame(
            FrameError::FrameTooLarge { .. } | FrameError::InvalidSecretLength { .. },
        ) => body("frame_too_large", 413, "never"),
        DecodeRefusal::Frame(FrameError::TruncatedFrame { .. }) | DecodeRefusal::Truncated => {
            body("truncated_frame", 400, "never")
        }
        DecodeRefusal::Surplus => body("surplus_data", 400, "never"),
        DecodeRefusal::Frame(_) => body("invalid_frame", 400, "never"),
    }
}

fn admitted(transport: Transport, opcode: Opcode) -> bool {
    match transport {
        Transport::Native => !matches!(opcode, Opcode::NeedSecrets | Opcode::PlanResponse),
        Transport::Hosted => matches!(
            opcode,
            Opcode::ConnectBegin
                | Opcode::ConnectCommit
                | Opcode::ConnectQuery
                | Opcode::GrantPreview
                | Opcode::GrantApply
                | Opcode::GrantQuery
                | Opcode::CredentialBegin
                | Opcode::CredentialCommit
                | Opcode::CredentialQuery
                | Opcode::Secret
        ),
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    commit: &'static str,
    retry: &'static str,
    schema: &'static str,
    status: u16,
}

#[derive(Serialize)]
struct PostDecisionErrorBody {
    code: &'static str,
    commit: &'static str,
    receipt_id: String,
    retry: &'static str,
    schema: &'static str,
    status: u16,
}

const fn body(code: &'static str, status: u16, retry: &'static str) -> ErrorBody {
    ErrorBody {
        code,
        commit: "none",
        retry,
        schema: "exchange.local-management-error.v1",
        status,
    }
}

fn refusal(code: &'static str, status: u16, retry: &'static str) -> Frame {
    let encoded = serde_json::to_vec(&body(code, status, retry))
        .expect("the closed value-free FXLM refusal serializes");
    Frame::control(Direction::ServerToClient, Opcode::Error, encoded)
        .expect("the fixed FXLM refusal is bounded")
}

fn postdeadline_refusal(receipt: ReceiptIdentity, unresolved: Unresolved) -> Frame {
    let (code, status) = match unresolved {
        Unresolved::Store => ("store_unavailable", 503),
        Unresolved::Audit => ("audit_unavailable", 503),
        Unresolved::Internal => ("internal_refusal", 500),
    };
    let encoded = serde_json::to_vec(&PostDecisionErrorBody {
        code,
        commit: "query_receipt",
        receipt_id: receipt.encoded(),
        retry: "same_proposal",
        schema: "exchange.local-management-error.v1",
        status,
    })
    .expect("the closed value-free post-decision refusal serializes");
    Frame::control(Direction::ServerToClient, Opcode::Error, encoded)
        .expect("the fixed post-decision refusal is bounded")
}

fn error_frame(error: ErrorBody) -> Vec<u8> {
    refusal(error.code, error.status, error.retry).encode()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::{Barrier, Mutex};

    use exchange_host::{
        CredentialScope, CredentialStore, GrantApplyReceipt, GrantCandidate, GrantDecisionObserver,
        GrantPreview, GrantProposalDigest, GrantSelector, GrantStore, GrantTransactionRefusal,
        GrantTransactions, SecretBatch, SecretProposalDigest,
    };

    use super::*;
    use crate::local_management::service_account::{
        MintOutcome, MintPort, MintPortRefusal, MintRequest, ReceiptId as MintReceiptId,
        TokenHandoff, WriterRefusal,
    };
    use crate::local_management::transaction::TransactionKind;

    struct BlockingGrantPort {
        inner: Arc<GrantStore>,
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
        decision_first: bool,
    }

    impl GrantTransactions for BlockingGrantPort {
        fn preview(
            &self,
            tenant: &Tenant,
            connector: &str,
            selector: GrantSelector,
        ) -> Result<GrantPreview, GrantTransactionRefusal> {
            self.inner.preview(tenant, connector, selector)
        }

        fn apply(
            &self,
            tenant: &Tenant,
            candidate: &GrantCandidate,
            revision: StoreRevision,
            proposal_digest: GrantProposalDigest,
            receipt_id: GrantReceiptId,
        ) -> Result<GrantApplyReceipt, GrantTransactionRefusal> {
            self.inner
                .apply(tenant, candidate, revision, proposal_digest, receipt_id)
        }

        fn apply_observed(
            &self,
            tenant: &Tenant,
            candidate: &GrantCandidate,
            revision: StoreRevision,
            proposal_digest: GrantProposalDigest,
            receipt_id: GrantReceiptId,
            observer: &mut dyn GrantDecisionObserver,
        ) -> Result<GrantApplyReceipt, GrantTransactionRefusal> {
            if !self.decision_first {
                self.entered.wait();
                self.release.wait();
            }
            if !observer.starting(receipt_id) {
                return Err(GrantTransactionRefusal::DecisionExpired);
            }
            if self.decision_first {
                self.entered.wait();
                self.release.wait();
            }
            let receipt =
                self.inner
                    .apply(tenant, candidate, revision, proposal_digest, receipt_id)?;
            observer.decided(receipt.receipt_id);
            Ok(receipt)
        }

        fn query(
            &self,
            tenant: &Tenant,
            receipt_id: GrantReceiptId,
        ) -> Result<Option<GrantApplyReceipt>, GrantTransactionRefusal> {
            self.inner.query(tenant, receipt_id)
        }
    }

    struct CommittedGrantAudit;

    impl GrantAudit for CommittedGrantAudit {
        fn commit(
            &self,
            _tenant: &Tenant,
            _connector: &str,
            _revision: StoreRevision,
            _receipt_id: GrantReceiptId,
        ) -> Result<(), GrantAuditUnavailable> {
            Ok(())
        }
    }

    struct BlockingMintPort {
        entered: Arc<Barrier>,
        release: Arc<Barrier>,
        decision_first: bool,
        committed: Mutex<Option<(String, MintReceiptId)>>,
    }

    impl MintPort for BlockingMintPort {
        fn mint(
            &self,
            _actor: &Principal,
            _request: &MintRequest,
            _receipt_id: MintReceiptId,
            _handoff: &mut dyn TokenHandoff,
        ) -> Result<MintOutcome, MintPortRefusal> {
            Err(MintPortRefusal::Internal)
        }

        fn mint_observed(
            &self,
            _actor: &Principal,
            request: &MintRequest,
            receipt_id: MintReceiptId,
            _handoff: &mut dyn TokenHandoff,
            starting: &mut dyn FnMut(MintReceiptId) -> bool,
            decided: &mut dyn FnMut(MintReceiptId),
        ) -> Result<MintOutcome, MintPortRefusal> {
            if !self.decision_first {
                self.entered.wait();
                self.release.wait();
            }
            if !starting(receipt_id.clone()) {
                return Err(MintPortRefusal::DecisionExpired);
            }
            if self.decision_first {
                self.entered.wait();
                self.release.wait();
            }
            let id = request.id().to_owned();
            *self.committed.lock().expect("mint result") = Some((id.clone(), receipt_id.clone()));
            decided(receipt_id.clone());
            Ok(MintOutcome::Committed { id, receipt_id })
        }

        fn query(
            &self,
            _tenant: &Tenant,
            receipt_id: &MintReceiptId,
        ) -> Result<Option<MintOutcome>, MintPortRefusal> {
            Ok(self
                .committed
                .lock()
                .expect("mint result")
                .as_ref()
                .filter(|(_, held)| held == receipt_id)
                .map(|(id, held)| MintOutcome::Replay {
                    id: id.clone(),
                    receipt_id: held.clone(),
                }))
        }
    }

    struct UnusedWriter;

    impl OneShotWriter for UnusedWriter {
        fn write_once(self: Box<Self>, _frame: &[u8]) -> Result<(), WriterRefusal> {
            Err(WriterRefusal::Closed)
        }
    }

    fn request(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
        Frame::control(Direction::ClientToServer, opcode, payload.to_vec())
            .expect("request")
            .encode()
    }

    #[test]
    fn hosted_transport_matrix_excludes_plan_and_service_account_operations() {
        for opcode in [
            Opcode::PlanQuery,
            Opcode::ServiceAccountMint,
            Opcode::ServiceAccountQuery,
        ] {
            assert!(!admitted(Transport::Hosted, opcode));
            assert!(admitted(Transport::Native, opcode));
        }
        assert!(admitted(Transport::Hosted, Opcode::ConnectBegin));
        assert!(admitted(Transport::Hosted, Opcode::CredentialQuery));
    }

    #[test]
    fn one_hosted_message_is_exactly_one_frame() {
        let first = request(
            Opcode::ConnectQuery,
            br#"{"receipt_id":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}"#,
        );
        assert!(exact_frame(&first).is_ok());
        let mut surplus = first.clone();
        surplus.extend_from_slice(&first);
        assert!(matches!(exact_frame(&surplus), Err(DecodeRefusal::Surplus)));
        assert!(matches!(
            exact_frame(&first[..first.len() - 1]),
            Err(DecodeRefusal::Frame(FrameError::TruncatedFrame { .. }))
                | Err(DecodeRefusal::Truncated)
        ));
    }

    #[tokio::test]
    async fn active_session_abort_tombstones_only_before_the_durable_decision() {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x134-session-abort-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("private test root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only test root");
        }
        let store = CredentialStore::bind(root.join("credentials/store"))
            .expect("retained credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        let proposal = SecretProposalDigest::from_protocol_bytes([9; 32]);

        let before = coordinator
            .allocate_for_tenant(
                TransactionKind::Connect,
                "local",
                "test",
                "before",
                proposal,
            )
            .expect("pre-decision allocation");
        let deadline = DeadlineController::start();
        let mut before_session = ActiveSession {
            ceremony: ActiveCeremony::abort_probe(
                before,
                coordinator.clone(),
                AppState::without_identity(),
                deadline.clone(),
            ),
            deadline,
        };
        before_session.abort().await;
        assert!(
            coordinator
                .proposal_state_for_tenant(
                    TransactionKind::Connect,
                    "local",
                    "test",
                    "before",
                    proposal,
                )
                .expect("pre-decision state")
                .is_none(),
            "pre-decision disconnect must tombstone the allocation"
        );

        let after = coordinator
            .allocate_for_tenant(TransactionKind::Connect, "local", "test", "after", proposal)
            .expect("post-decision allocation");
        let batch = SecretBatch::new(
            CredentialScope::new("local", "example.test").expect("test credential scope"),
        );
        coordinator
            .prepare(after, &batch)
            .await
            .expect("prepared provider row");
        coordinator.decide_commit(after).expect("durable decision");
        let deadline = DeadlineController::start();
        deadline
            .decided(
                ReceiptIdentity::from_protocol_bytes(after.receipt_id().protocol_bytes())
                    .expect("nonzero receipt"),
                Unresolved::Store,
            )
            .expect("deadline decision");
        let mut after_session = ActiveSession {
            ceremony: ActiveCeremony::abort_probe(
                after,
                coordinator.clone(),
                AppState::without_identity(),
                deadline.clone(),
            ),
            deadline,
        };
        after_session.abort().await;
        assert!(matches!(
            coordinator
                .proposal_state_for_tenant(
                    TransactionKind::Connect,
                    "local",
                    "test",
                    "after",
                    proposal,
                )
                .expect("post-decision state"),
            Some(crate::local_management::transaction::ProposalState::Active)
        ));
        coordinator
            .commit(after)
            .await
            .expect("post-decision row remains recoverable");

        drop(coordinator);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(start_paused = true)]
    async fn real_store_decisions_at_299_and_300_select_the_only_safe_phase() {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x135-real-decision-boundary-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("private test root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only test root");
        }
        let store = CredentialStore::bind(root.join("credentials/store"))
            .expect("retained credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        let proposal = SecretProposalDigest::from_protocol_bytes([13; 32]);
        let batch = SecretBatch::new(
            CredentialScope::new("local", "example.test").expect("test credential scope"),
        );

        let inflight = coordinator
            .allocate_for_tenant(
                TransactionKind::Connect,
                "local",
                "test",
                "inflight-299",
                proposal,
            )
            .expect("in-flight allocation");
        coordinator
            .prepare(inflight, &batch)
            .await
            .expect("in-flight prepare");
        let inflight_deadline = DeadlineController::start();
        tokio::time::advance(std::time::Duration::from_secs(299)).await;
        inflight_deadline
            .begin_decision(
                ReceiptIdentity::from_protocol_bytes(inflight.receipt_id().protocol_bytes())
                    .expect("nonzero receipt"),
                Unresolved::Store,
            )
            .expect("durable write starts before the boundary");
        tokio::time::advance(std::time::Duration::from_secs(1)).await;
        coordinator
            .decide_commit(inflight)
            .expect("in-flight decision reaches the journal at the boundary");
        inflight_deadline
            .decided(
                ReceiptIdentity::from_protocol_bytes(inflight.receipt_id().protocol_bytes())
                    .expect("nonzero receipt"),
                Unresolved::Store,
            )
            .expect("outcome-uncertain write rolls forward");
        coordinator
            .commit(inflight)
            .await
            .expect("in-flight decision commits");

        let late = coordinator
            .allocate_for_tenant(
                TransactionKind::Connect,
                "local",
                "test",
                "not-started-300",
                proposal,
            )
            .expect("late allocation");
        coordinator
            .prepare(late, &batch)
            .await
            .expect("late prepare");
        let late_deadline = DeadlineController::start();
        tokio::time::advance(std::time::Duration::from_secs(300)).await;
        assert!(
            late_deadline
                .begin_decision(
                    ReceiptIdentity::from_protocol_bytes(late.receipt_id().protocol_bytes())
                        .expect("nonzero receipt"),
                    Unresolved::Store,
                )
                .is_err(),
            "a durable write not started by 300 seconds is refused"
        );
        coordinator
            .abort_before_decision(late)
            .await
            .expect("late operation remains safely abortable");

        drop(coordinator);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test]
    async fn allocated_ceremony_drop_tombstones_only_until_the_decision_guard_disarms() {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x135-cancellation-guard-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("private test root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only test root");
        }
        let store = CredentialStore::bind(root.join("credentials/store"))
            .expect("retained credential store");
        let coordinator = Arc::new(
            TransactionCoordinator::bind(
                root.join("transactions/journal.sqlite3"),
                store.prepared_secrets(),
            )
            .expect("transaction coordinator"),
        );
        let proposal = SecretProposalDigest::from_protocol_bytes([17; 32]);
        let batch = SecretBatch::new(
            CredentialScope::new("local", "example.test").expect("test credential scope"),
        );

        for (label, prepared) in [
            ("begin-drop", false),
            ("prepare-drop", true),
            ("secret-drop", false),
        ] {
            let allocation = coordinator
                .allocate_for_tenant(TransactionKind::Connect, "local", "test", label, proposal)
                .expect("guarded allocation");
            if prepared {
                coordinator
                    .prepare(allocation, &batch)
                    .await
                    .expect("prepared row");
            }
            let deadline = DeadlineController::start();
            let ceremony = ActiveCeremony::abort_probe(
                allocation,
                coordinator.clone(),
                AppState::without_identity(),
                deadline,
            );
            drop(ceremony);
            for _ in 0..32 {
                if coordinator
                    .proposal_state_for_tenant(
                        TransactionKind::Connect,
                        "local",
                        "test",
                        label,
                        proposal,
                    )
                    .expect("guard state")
                    .is_none()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
            assert!(
                coordinator
                    .proposal_state_for_tenant(
                        TransactionKind::Connect,
                        "local",
                        "test",
                        label,
                        proposal,
                    )
                    .expect("tombstoned state")
                    .is_none(),
                "dropping {label} must tombstone the pre-decision allocation"
            );
        }

        let decided = coordinator
            .allocate_for_tenant(
                TransactionKind::Connect,
                "local",
                "test",
                "commit-drop",
                proposal,
            )
            .expect("decision allocation");
        coordinator
            .prepare(decided, &batch)
            .await
            .expect("decision prepare");
        let deadline = DeadlineController::start();
        let mut ceremony = ActiveCeremony::abort_probe(
            decided,
            coordinator.clone(),
            AppState::without_identity(),
            deadline.clone(),
        );
        deadline
            .begin_decision(
                ReceiptIdentity::from_protocol_bytes(decided.receipt_id().protocol_bytes())
                    .expect("nonzero receipt"),
                Unresolved::Store,
            )
            .expect("decision starts");
        coordinator
            .decide_commit(decided)
            .expect("durable decision");
        ceremony.disarm_abort_probe();
        deadline
            .decided(
                ReceiptIdentity::from_protocol_bytes(decided.receipt_id().protocol_bytes())
                    .expect("nonzero receipt"),
                Unresolved::Store,
            )
            .expect("decision observed");
        drop(ceremony);
        assert!(matches!(
            coordinator
                .proposal_state_for_tenant(
                    TransactionKind::Connect,
                    "local",
                    "test",
                    "commit-drop",
                    proposal,
                )
                .expect("post-decision state"),
            Some(crate::local_management::transaction::ProposalState::Active)
        ));
        coordinator
            .commit(decided)
            .await
            .expect("post-decision drop retains roll-forward");

        drop(coordinator);
        drop(store);
        let _ = std::fs::remove_dir_all(root);
    }

    #[tokio::test(start_paused = true)]
    async fn blocked_grant_and_mint_ports_do_not_block_the_deadline_runtime() {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        let root = std::env::temp_dir().join(format!(
            "flux-exchange-x135-blocked-ports-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).expect("private test root");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))
                .expect("owner-only test root");
        }
        let tenant = Tenant::new("local").expect("tenant");
        let grants = Arc::new(GrantStore::bind(root.join("grants.json")).expect("grant store"));
        let selector: GrantSelector = serde_json::from_slice(
            br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
        )
        .expect("selector");
        let candidate = grants
            .preview(&tenant, "github", selector)
            .expect("grant preview");
        let candidate_bytes = serde_json::to_vec(&candidate).expect("candidate bytes");

        let grant_entered = Arc::new(Barrier::new(2));
        let grant_release = Arc::new(Barrier::new(2));
        let grant_port = Arc::new(BlockingGrantPort {
            inner: grants.clone(),
            entered: grant_entered.clone(),
            release: grant_release.clone(),
            decision_first: false,
        });
        let grant = GrantCeremony::new(grant_port, Arc::new(CommittedGrantAudit));
        let deadline = DeadlineController::start();
        let worker_deadline = deadline.clone();
        let worker_tenant = tenant.clone();
        let worker = tokio::task::spawn_blocking(move || {
            grant.handle_with_deadline(
                &worker_tenant,
                super::super::grant::APPLY_OPCODE,
                &candidate_bytes,
                &worker_deadline,
            )
        });
        let raced_deadline = deadline.clone();
        let raced = tokio::spawn(async move { raced_deadline.race(worker).await });
        tokio::task::yield_now().await;
        grant_entered.wait();
        tokio::time::advance(std::time::Duration::from_secs(300)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            raced.await.expect("grant runtime"),
            Err(Expired::PreDecision)
        ));
        grant_release.wait();
        for _ in 0..32 {
            if grants
                .preview(
                    &tenant,
                    "github",
                    serde_json::from_slice(
                        br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
                    )
                    .expect("selector"),
                )
                .expect("unchanged preview")
                .revision
                == StoreRevision::new(1).expect("revision")
            {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert_eq!(
            grants
                .preview(
                    &tenant,
                    "github",
                    serde_json::from_slice(
                        br#"{"effects_within":null,"idempotency":null,"max_risk":"low"}"#,
                    )
                    .expect("selector"),
                )
                .expect("unchanged preview")
                .revision,
            StoreRevision::new(1).expect("revision"),
            "the released worker must observe the expired start and skip the grant write"
        );

        let mint_entered = Arc::new(Barrier::new(2));
        let mint_release = Arc::new(Barrier::new(2));
        let mint_port = Arc::new(BlockingMintPort {
            entered: mint_entered.clone(),
            release: mint_release.clone(),
            decision_first: true,
            committed: Mutex::new(None),
        });
        let mint = ServiceAccountCeremony::with_receipts(mint_port.clone(), [0x55; 32]);
        let actor = Principal::new(PrincipalKind::User, "local-owner", tenant.clone());
        let deadline = DeadlineController::start();
        let worker_deadline = deadline.clone();
        let worker = tokio::task::spawn_blocking(move || {
            mint.handle_with_deadline(
                &actor,
                crate::local_management::service_account::MINT_OPCODE,
                br#"{"expires_at":"4070908800","id":"runtime"}"#,
                Some(Box::new(UnusedWriter)),
                &worker_deadline,
            )
        });
        let raced_deadline = deadline.clone();
        let raced = tokio::spawn(async move { raced_deadline.race(worker).await });
        tokio::task::yield_now().await;
        mint_entered.wait();
        tokio::time::advance(std::time::Duration::from_secs(300)).await;
        tokio::task::yield_now().await;
        assert!(matches!(
            raced.await.expect("mint runtime"),
            Err(Expired::PostDecision {
                unresolved: Unresolved::Store,
                ..
            })
        ));
        mint_release.wait();
        for _ in 0..32 {
            if mint_port.committed.lock().expect("mint result").is_some() {
                break;
            }
            tokio::task::yield_now().await;
        }
        assert!(
            mint_port.committed.lock().expect("mint result").is_some(),
            "post-decision mint work must detach and become queryable"
        );

        drop(grants);
        let _ = std::fs::remove_dir_all(root);
    }
}
