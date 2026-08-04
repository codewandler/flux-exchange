//! One value-free entry point shared by native and hosted FXLM transports.

use std::sync::Arc;

use exchange_host::{GrantReceiptId, StoreRevision, Tenant};
use serde::Serialize;

use super::codec::{Direction, Frame, FrameError, Opcode, StreamDecoder};
use super::grant::{GrantAudit, GrantAuditUnavailable, GrantCeremony};
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

/// Shared local-management authority after transport authentication.
#[derive(Clone)]
pub(crate) struct Dispatcher {
    state: AppState,
    coordinator: Arc<TransactionCoordinator>,
    grant: Option<GrantCeremony>,
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
        Self {
            state,
            coordinator,
            grant,
        }
    }

    pub(crate) fn state(&self) -> &AppState {
        &self.state
    }

    /// Decode exactly one complete hosted message and return one complete server frame.
    pub(crate) async fn dispatch_message(
        &self,
        transport: Transport,
        tenant: &Tenant,
        bytes: &[u8],
    ) -> Vec<u8> {
        let request = match exact_frame(bytes) {
            Ok(frame) => frame,
            Err(error) => return error_frame(frame_error(error)),
        };
        self.dispatch_frame(transport, tenant, request)
            .await
            .encode()
    }

    pub(super) async fn dispatch_frame(
        &self,
        transport: Transport,
        tenant: &Tenant,
        request: Frame,
    ) -> Frame {
        let _state = &self.state;
        let _coordinator = &self.coordinator;
        if !admitted(transport, request.opcode()) {
            return refusal("unexpected_frame", 422, "never");
        }
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
            let response = grant.handle(tenant, request.opcode() as u16, payload);
            let opcode = Opcode::try_from(response.opcode())
                .expect("grant ceremonies return only closed FXLM opcodes");
            return Frame::control(
                Direction::ServerToClient,
                opcode,
                response.payload().to_vec(),
            )
            .expect("grant ceremony responses satisfy the shared control bound");
        }
        // Individual ceremonies are wired as their value-free projections and atomic stores land.
        // Keeping this refusal inside the shared authenticated dispatcher is deliberate: neither
        // transport may substitute a point-write path while one ceremony is unavailable.
        refusal("local_management_unavailable", 503, "operator")
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

enum DecodeRefusal {
    Frame(FrameError),
    Truncated,
    Surplus,
}

fn frame_error(error: DecodeRefusal) -> ErrorBody {
    match error {
        DecodeRefusal::Frame(FrameError::UnsupportedVersion(_)) => {
            body("unsupported_version", 422, "never")
        }
        DecodeRefusal::Frame(FrameError::WrongDirection { .. }) => {
            body("wrong_direction", 422, "never")
        }
        DecodeRefusal::Frame(FrameError::FrameTooLarge { .. }) => {
            body("frame_too_large", 413, "never")
        }
        DecodeRefusal::Frame(FrameError::TruncatedFrame { .. }) | DecodeRefusal::Truncated => {
            body("truncated_frame", 422, "never")
        }
        DecodeRefusal::Surplus => body("surplus_data", 422, "never"),
        DecodeRefusal::Frame(_) => body("invalid_frame", 422, "never"),
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

fn error_frame(error: ErrorBody) -> Vec<u8> {
    refusal(error.code, error.status, error.retry).encode()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
