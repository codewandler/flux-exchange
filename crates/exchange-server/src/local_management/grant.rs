//! Grant preview and whole-set CAS ceremonies for `exchange.local-management.v1`.
//!
//! Framing and transport admission live beside this module. This file owns only the three client
//! grant control objects, their terminal server objects and the exact value-free mapping from the
//! revisioned host port. Keeping that boundary explicit lets native and hosted dispatchers share
//! one mutation path without either transport acquiring a second grant representation.

use std::cell::Cell;
use std::sync::Arc;

use connector_catalog::{provider, ProviderKey};
use exchange_host::{
    GrantApplyReceipt, GrantPreview, GrantReceiptId, GrantSelector, GrantTransactionRefusal,
    GrantTransactions, StoreRevision, Tenant,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::deadline::{DeadlineController, ReceiptIdentity, Unresolved};

pub(crate) const PREVIEW_OPCODE: u16 = 0x0010;
pub(crate) const CANDIDATE_OPCODE: u16 = 0x0011;
pub(crate) const APPLY_OPCODE: u16 = 0x0012;
pub(crate) const QUERY_OPCODE: u16 = 0x0013;
pub(crate) const RECEIPT_OPCODE: u16 = 0x0014;
pub(crate) const ERROR_OPCODE: u16 = 0x7fff;

const MAX_CONTROL_BYTES: usize = 65_536;
const ERROR_SCHEMA: &str = "exchange.local-management-error.v1";
const RECEIPT_SCHEMA: &str = "exchange.grant-apply-receipt.v1";
const INTERNAL_ERROR: &[u8] = br#"{"code":"internal_refusal","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":500}"#;

/// One server-to-client grant control frame, before the shared FXLM header is applied.
pub(crate) struct GrantFrame {
    opcode: u16,
    payload: Vec<u8>,
}

impl GrantFrame {
    pub(crate) const fn opcode(&self) -> u16 {
        self.opcode
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

trait ReceiptIds: Send + Sync {
    fn next(&self) -> Result<GrantReceiptId, ()>;
}

/// Idempotent canonical audit delivery required before a committed receipt is returned.
///
/// APPLY replay and QUERY may call this again with the same receipt id, which is the stable event
/// identity. An implementation reports success when that event was already delivered.
pub(crate) trait GrantAudit: Send + Sync {
    fn commit(
        &self,
        tenant: &Tenant,
        connector: &str,
        revision: StoreRevision,
        receipt_id: GrantReceiptId,
    ) -> Result<(), GrantAuditUnavailable>;
}

/// Value-free audit refusal; sink diagnostics never enter local-management bytes.
pub(crate) struct GrantAuditUnavailable;

struct OsReceiptIds;

impl ReceiptIds for OsReceiptIds {
    fn next(&self) -> Result<GrantReceiptId, ()> {
        for _ in 0..4 {
            let bytes = flux_exchange::entropy::bytes::<32>().map_err(|_| ())?;
            if let Some(receipt) = GrantReceiptId::from_protocol_bytes(bytes) {
                return Ok(receipt);
            }
        }
        Err(())
    }
}

/// One shared grant ceremony over the exact grant port used by invocation.
#[derive(Clone)]
pub(crate) struct GrantCeremony {
    audit: Arc<dyn GrantAudit>,
    grants: Arc<dyn GrantTransactions>,
    receipts: Arc<dyn ReceiptIds>,
}

impl GrantCeremony {
    /// Bind production OS entropy and the retained revisioned grant port.
    pub(crate) fn new(grants: Arc<dyn GrantTransactions>, audit: Arc<dyn GrantAudit>) -> Self {
        Self {
            audit,
            grants,
            receipts: Arc::new(OsReceiptIds),
        }
    }

    #[cfg(test)]
    fn with_receipts(
        grants: Arc<dyn GrantTransactions>,
        audit: Arc<dyn GrantAudit>,
        receipts: Arc<dyn ReceiptIds>,
    ) -> Self {
        Self {
            audit,
            grants,
            receipts,
        }
    }

    /// Handle one admitted client grant opcode as the connection's sole logical operation.
    pub(crate) fn handle_with_deadline(
        &self,
        tenant: &Tenant,
        opcode: u16,
        payload: &[u8],
        deadline: &DeadlineController,
    ) -> GrantFrame {
        match opcode {
            PREVIEW_OPCODE => {
                let frame = self.preview(tenant, payload);
                deadline.terminal();
                frame
            }
            APPLY_OPCODE => self.apply(tenant, payload, deadline),
            QUERY_OPCODE => self.query(tenant, payload, deadline),
            // The shared codec has already rejected unknown opcodes. A client sending either
            // server response opcode, or a known opcode in this grant state, is a state refusal.
            _ => {
                deadline.terminal();
                refusal(Refusal::UnexpectedFrame)
            }
        }
    }

    #[cfg(test)]
    fn handle(&self, tenant: &Tenant, opcode: u16, payload: &[u8]) -> GrantFrame {
        self.handle_with_deadline(tenant, opcode, payload, &DeadlineController::start())
    }

    fn preview(&self, tenant: &Tenant, payload: &[u8]) -> GrantFrame {
        let request: PreviewRequest = match canonical(payload) {
            Ok(request) => request,
            Err(()) => return refusal(Refusal::InvalidRequest),
        };
        if !released(&request.connector) {
            return refusal(Refusal::UnknownConnector);
        }
        match self
            .grants
            .preview(tenant, &request.connector, request.selector)
        {
            Ok(candidate) => response(CANDIDATE_OPCODE, &candidate),
            Err(error) => refusal(Refusal::from(error)),
        }
    }

    fn apply(&self, tenant: &Tenant, payload: &[u8], deadline: &DeadlineController) -> GrantFrame {
        let request: GrantPreview = match canonical(payload) {
            Ok(request) => request,
            Err(()) => return refusal(Refusal::InvalidRequest),
        };
        if !released(&request.candidate.connector) {
            return refusal(Refusal::UnknownConnector);
        }
        let receipt = match self.receipts.next() {
            Ok(receipt) => receipt,
            Err(()) => return refusal(Refusal::Internal),
        };
        let invariant = Cell::new(false);
        let mut decided = |receipt: GrantReceiptId| {
            if deadline
                .decided(grant_receipt(receipt), Unresolved::Audit)
                .is_err()
            {
                invariant.set(true);
            }
        };
        match self.grants.apply_observed(
            tenant,
            &request.candidate,
            request.revision,
            request.proposal_digest,
            receipt,
            &mut decided,
        ) {
            Ok(receipt) if invariant.get() => {
                postdecision_refusal("internal_refusal", 500, receipt.receipt_id)
            }
            Ok(receipt) => self.audited_receipt(tenant, receipt, deadline),
            Err(error) => {
                deadline.terminal();
                refusal(Refusal::from(error))
            }
        }
    }

    fn query(&self, tenant: &Tenant, payload: &[u8], deadline: &DeadlineController) -> GrantFrame {
        let request: QueryRequest = match canonical(payload) {
            Ok(request) => request,
            Err(()) => return refusal(Refusal::InvalidRequest),
        };
        match self.grants.query(tenant, request.receipt_id) {
            Ok(Some(receipt)) => {
                if deadline
                    .decided(grant_receipt(receipt.receipt_id), Unresolved::Audit)
                    .is_err()
                {
                    return postdecision_refusal("internal_refusal", 500, receipt.receipt_id);
                }
                self.audited_receipt(tenant, receipt, deadline)
            }
            Ok(None) => {
                deadline.terminal();
                refusal(Refusal::InvalidRequest)
            }
            Err(error) => {
                deadline.terminal();
                refusal(Refusal::from(error))
            }
        }
    }

    fn audited_receipt(
        &self,
        tenant: &Tenant,
        receipt: GrantApplyReceipt,
        deadline: &DeadlineController,
    ) -> GrantFrame {
        match self.audit.commit(
            tenant,
            &receipt.connector,
            receipt.revision,
            receipt.receipt_id,
        ) {
            Ok(()) => {
                deadline.terminal();
                receipt_frame(receipt)
            }
            Err(GrantAuditUnavailable) => {
                deadline.unresolved(Unresolved::Audit);
                postdecision_refusal("audit_unavailable", 503, receipt.receipt_id)
            }
        }
    }
}

fn grant_receipt(receipt: GrantReceiptId) -> ReceiptIdentity {
    ReceiptIdentity::from_protocol_bytes(receipt.protocol_bytes())
        .expect("grant receipt identities are always nonzero")
}

fn released(connector: &str) -> bool {
    provider(ProviderKey::id(connector)).is_some()
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRequest {
    connector: String,
    selector: GrantSelector,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryRequest {
    receipt_id: GrantReceiptId,
}

#[derive(Serialize)]
struct ReceiptCommit {
    audit: &'static str,
    resource: &'static str,
}

#[derive(Serialize)]
struct ReceiptBody {
    commit: ReceiptCommit,
    connector: String,
    receipt_id: GrantReceiptId,
    replayed: bool,
    revision: StoreRevision,
    schema: &'static str,
}

fn receipt_frame(receipt: GrantApplyReceipt) -> GrantFrame {
    response(
        RECEIPT_OPCODE,
        &ReceiptBody {
            commit: ReceiptCommit {
                audit: "committed",
                resource: "committed",
            },
            connector: receipt.connector,
            receipt_id: receipt.receipt_id,
            replayed: receipt.replayed,
            revision: receipt.revision,
            schema: RECEIPT_SCHEMA,
        },
    )
}

fn canonical<T>(payload: &[u8]) -> Result<T, ()>
where
    T: DeserializeOwned + Serialize,
{
    if payload.len() > MAX_CONTROL_BYTES {
        return Err(());
    }
    let decoded: T = serde_json::from_slice(payload).map_err(|_| ())?;
    let encoded = serde_json::to_vec(&decoded).map_err(|_| ())?;
    (encoded == payload).then_some(decoded).ok_or(())
}

fn response(opcode: u16, body: &impl Serialize) -> GrantFrame {
    match serde_json::to_vec(body) {
        Ok(payload) if payload.len() <= MAX_CONTROL_BYTES => GrantFrame { opcode, payload },
        Ok(_) | Err(_) => GrantFrame {
            opcode: ERROR_OPCODE,
            payload: INTERNAL_ERROR.to_vec(),
        },
    }
}

#[derive(Clone, Copy)]
enum Refusal {
    InvalidRequest,
    UnknownConnector,
    UnexpectedFrame,
    Unexpressible,
    Stale,
    DigestMismatch,
    Store,
    Internal,
}

impl From<GrantTransactionRefusal> for Refusal {
    fn from(refusal: GrantTransactionRefusal) -> Self {
        match refusal {
            GrantTransactionRefusal::Unexpressible => Self::Unexpressible,
            GrantTransactionRefusal::Stale { .. } => Self::Stale,
            GrantTransactionRefusal::DigestMismatch => Self::DigestMismatch,
            GrantTransactionRefusal::RevisionExhausted | GrantTransactionRefusal::Store { .. } => {
                Self::Store
            }
            GrantTransactionRefusal::ReceiptConflict => Self::Internal,
        }
    }
}

impl Refusal {
    const fn tuple(self) -> (&'static str, u16, &'static str) {
        match self {
            Self::InvalidRequest => ("invalid_request", 400, "never"),
            Self::UnknownConnector => ("unknown_connector", 404, "refresh"),
            Self::UnexpectedFrame => ("unexpected_frame", 409, "never"),
            Self::Unexpressible => ("grant_unexpressible", 409, "operator"),
            Self::Stale => ("grant_stale", 409, "refresh"),
            Self::DigestMismatch => ("grant_digest_mismatch", 409, "refresh"),
            Self::Store => ("store_unavailable", 503, "operator"),
            Self::Internal => ("internal_refusal", 500, "operator"),
        }
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
    receipt_id: GrantReceiptId,
    retry: &'static str,
    schema: &'static str,
    status: u16,
}

fn refusal(kind: Refusal) -> GrantFrame {
    let (code, status, retry) = kind.tuple();
    response(
        ERROR_OPCODE,
        &ErrorBody {
            code,
            commit: "none",
            retry,
            schema: ERROR_SCHEMA,
            status,
        },
    )
}

fn postdecision_refusal(code: &'static str, status: u16, receipt_id: GrantReceiptId) -> GrantFrame {
    response(
        ERROR_OPCODE,
        &PostDecisionErrorBody {
            code,
            commit: "query_receipt",
            receipt_id,
            retry: "same_proposal",
            schema: ERROR_SCHEMA,
            status,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::{BTreeSet, VecDeque};
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Mutex;

    use exchange_host::{Grant, GrantStore, Grants, Selector};

    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-x134-grant-fxlm-{name}-{}-{:?}",
                std::process::id(),
                std::thread::current().id(),
            ));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir(&path).expect("owner scratch");
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt as _;
                std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                    .expect("owner-only scratch");
            }
            Self(path)
        }

        fn store(&self) -> PathBuf {
            self.0.join("grants")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct FixedReceipts(Mutex<VecDeque<GrantReceiptId>>);

    impl FixedReceipts {
        fn new(bytes: impl IntoIterator<Item = u8>) -> Self {
            Self(Mutex::new(
                bytes
                    .into_iter()
                    .map(|byte| {
                        GrantReceiptId::from_protocol_bytes([byte; 32]).expect("nonzero receipt")
                    })
                    .collect(),
            ))
        }
    }

    impl ReceiptIds for FixedReceipts {
        fn next(&self) -> Result<GrantReceiptId, ()> {
            self.0.lock().map_err(|_| ())?.pop_front().ok_or(())
        }
    }

    struct CommittedAudit;

    impl GrantAudit for CommittedAudit {
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

    struct FailsOnceAudit(AtomicBool);

    impl GrantAudit for FailsOnceAudit {
        fn commit(
            &self,
            _tenant: &Tenant,
            _connector: &str,
            _revision: StoreRevision,
            _receipt_id: GrantReceiptId,
        ) -> Result<(), GrantAuditUnavailable> {
            if self.0.swap(false, Ordering::SeqCst) {
                Err(GrantAuditUnavailable)
            } else {
                Ok(())
            }
        }
    }

    fn tenant() -> Tenant {
        Tenant::new("local").expect("tenant")
    }

    fn ceremony(store: Arc<GrantStore>) -> GrantCeremony {
        let grants: Arc<dyn GrantTransactions> = store;
        GrantCeremony::with_receipts(
            grants,
            Arc::new(CommittedAudit),
            Arc::new(FixedReceipts::new(1..=32)),
        )
    }

    fn assert_error(frame: &GrantFrame, expected: &[u8]) {
        assert_eq!(frame.opcode(), ERROR_OPCODE);
        assert_eq!(frame.payload(), expected);
    }

    fn owner_file(path: &Path, bytes: &[u8]) {
        std::fs::write(path, bytes).expect("grant file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
                .expect("owner-only grant file");
        }
    }

    #[test]
    fn preview_candidate_apply_replay_and_query_are_exact_canonical_controls() {
        let scratch = Scratch::new("lifecycle");
        let store = Arc::new(GrantStore::bind(scratch.store()).expect("store"));
        let ceremony = ceremony(store);
        let preview_request = br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#;

        let candidate = ceremony.handle(&tenant(), PREVIEW_OPCODE, preview_request);
        assert_eq!(candidate.opcode(), CANDIDATE_OPCODE);
        assert_eq!(
            candidate.payload(),
            br#"{"candidate":{"connector":"github","inbound":[],"selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}},"proposal_digest":"2ab0284e934adf48c3b97a9e1fd08feb95ff4e62561d8f26339fa9df1626d770","revision":"1"}"#
        );

        let applied = ceremony.handle(&tenant(), APPLY_OPCODE, candidate.payload());
        assert_eq!(applied.opcode(), RECEIPT_OPCODE);
        let receipt_id = "01".repeat(32);
        assert_eq!(
            applied.payload(),
            format!(
                "{{\"commit\":{{\"audit\":\"committed\",\"resource\":\"committed\"}},\"connector\":\"github\",\"receipt_id\":\"{receipt_id}\",\"replayed\":false,\"revision\":\"2\",\"schema\":\"exchange.grant-apply-receipt.v1\"}}"
            )
            .as_bytes()
        );

        let replay = ceremony.handle(&tenant(), APPLY_OPCODE, candidate.payload());
        assert_eq!(replay.opcode(), RECEIPT_OPCODE);
        assert_eq!(
            replay.payload(),
            format!(
                "{{\"commit\":{{\"audit\":\"committed\",\"resource\":\"committed\"}},\"connector\":\"github\",\"receipt_id\":\"{receipt_id}\",\"replayed\":true,\"revision\":\"2\",\"schema\":\"exchange.grant-apply-receipt.v1\"}}"
            )
            .as_bytes()
        );

        let query = format!("{{\"receipt_id\":\"{receipt_id}\"}}");
        let queried = ceremony.handle(&tenant(), QUERY_OPCODE, query.as_bytes());
        assert_eq!(queried.opcode(), RECEIPT_OPCODE);
        assert_eq!(queried.payload(), replay.payload());
    }

    #[test]
    fn stale_and_digest_conflicts_refuse_before_another_whole_set_write() {
        let scratch = Scratch::new("conflicts");
        let store = Arc::new(GrantStore::bind(scratch.store()).expect("store"));
        let ceremony = ceremony(store.clone());
        let low = ceremony.handle(
            &tenant(),
            PREVIEW_OPCODE,
            br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#,
        );
        let high = ceremony.handle(
            &tenant(),
            PREVIEW_OPCODE,
            br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":"high"}}"#,
        );
        assert_eq!(
            ceremony
                .handle(&tenant(), APPLY_OPCODE, low.payload())
                .opcode(),
            RECEIPT_OPCODE
        );
        assert_error(
            &ceremony.handle(&tenant(), APPLY_OPCODE, high.payload()),
            br#"{"code":"grant_stale","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":409}"#,
        );

        let mut changed: serde_json::Value =
            serde_json::from_slice(high.payload()).expect("candidate");
        changed["proposal_digest"] = serde_json::Value::String("00".repeat(32));
        let changed = serde_json::to_vec(&changed).expect("canonical changed candidate");
        assert_error(
            &ceremony.handle(&tenant(), APPLY_OPCODE, &changed),
            br#"{"code":"grant_digest_mismatch","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":409}"#,
        );
        assert_eq!(store.held(&tenant()).len(), 1);
    }

    #[test]
    fn malformed_unknown_and_wrong_state_controls_have_closed_value_free_refusals() {
        let scratch = Scratch::new("control-refusals");
        let path = scratch.store();
        let store = Arc::new(GrantStore::bind(&path).expect("store"));
        let ceremony = ceremony(store);
        for invalid in [
            br#"{"selector":{"effects_within":null,"idempotency":null,"max_risk":"low"},"connector":"github"}"#.as_slice(),
            br#"{"connector":"github","connector":"slack","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#.as_slice(),
            br#"{"connector":"github","extra":null,"selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#.as_slice(),
            br#"{"connector":"github","selector":{"effects_within":["workspace_write","network"],"idempotency":null,"max_risk":"low"}}"#.as_slice(),
            b"{\"connector\":\"github\",\"selector\":{\"effects_within\":null,\"idempotency\":null,\"max_risk\":\"low\"}}\n".as_slice(),
        ] {
            assert_error(
                &ceremony.handle(&tenant(), PREVIEW_OPCODE, invalid),
                br#"{"code":"invalid_request","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
            );
        }
        assert_error(
            &ceremony.handle(
                &tenant(),
                PREVIEW_OPCODE,
                br#"{"connector":"not-released","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#,
            ),
            br#"{"code":"unknown_connector","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":404}"#,
        );
        assert!(
            !path.exists(),
            "unknown connector is refused before migration"
        );
        assert_error(
            &ceremony.handle(&tenant(), CANDIDATE_OPCODE, b"{}"),
            br#"{"code":"unexpected_frame","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":409}"#,
        );
        assert_error(
            &ceremony.handle(
                &tenant(),
                QUERY_OPCODE,
                format!("{{\"receipt_id\":\"{}\"}}", "55".repeat(32)).as_bytes(),
            ),
            br#"{"code":"invalid_request","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#,
        );
    }

    #[test]
    fn selected_unexpressible_authority_maps_without_rewriting_it() {
        let scratch = Scratch::new("unexpressible");
        let path = scratch.store();
        let legacy = br#"{"local":[{"connector":"github","selector":{"allow_ids":["manual-id"],"deny_ids":[],"effects_within":null,"idempotency":null,"max_risk":null},"inbound":[]}]}"#;
        owner_file(&path, legacy);
        let store = Arc::new(GrantStore::bind(&path).expect("typed legacy store"));
        let ceremony = ceremony(store.clone());
        assert_error(
            &ceremony.handle(
                &tenant(),
                PREVIEW_OPCODE,
                br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#,
            ),
            br#"{"code":"grant_unexpressible","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":409}"#,
        );
        assert!(store.held(&tenant())[0]
            .selector
            .allow_ids
            .contains("manual-id"));
    }

    #[test]
    fn every_host_refusal_has_one_exhaustive_protocol_tuple() {
        let one = StoreRevision::new(1).expect("revision");
        let cases = [
            (
                GrantTransactionRefusal::Unexpressible,
                br#"{"code":"grant_unexpressible","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":409}"#.as_slice(),
            ),
            (
                GrantTransactionRefusal::Stale {
                    expected: one,
                    current: one.checked_next().expect("next"),
                },
                br#"{"code":"grant_stale","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":409}"#.as_slice(),
            ),
            (
                GrantTransactionRefusal::DigestMismatch,
                br#"{"code":"grant_digest_mismatch","commit":"none","retry":"refresh","schema":"exchange.local-management-error.v1","status":409}"#.as_slice(),
            ),
            (
                GrantTransactionRefusal::RevisionExhausted,
                br#"{"code":"store_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#.as_slice(),
            ),
            (
                GrantTransactionRefusal::Store {
                    reason: "sentinel must not cross".into(),
                },
                br#"{"code":"store_unavailable","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":503}"#.as_slice(),
            ),
            (
                GrantTransactionRefusal::ReceiptConflict,
                br#"{"code":"internal_refusal","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":500}"#.as_slice(),
            ),
        ];
        for (host, expected) in cases {
            assert_error(&refusal(Refusal::from(host)), expected);
        }
    }

    #[test]
    fn audit_failure_after_cas_returns_query_receipt_and_replay_drains_it() {
        let scratch = Scratch::new("audit-recovery");
        let store = Arc::new(GrantStore::bind(scratch.store()).expect("store"));
        let grants: Arc<dyn GrantTransactions> = store;
        let ceremony = GrantCeremony::with_receipts(
            grants,
            Arc::new(FailsOnceAudit(AtomicBool::new(true))),
            Arc::new(FixedReceipts::new([0x44, 0x45])),
        );
        let candidate = ceremony.handle(
            &tenant(),
            PREVIEW_OPCODE,
            br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":"low"}}"#,
        );
        let receipt_id = "44".repeat(32);
        assert_error(
            &ceremony.handle(&tenant(), APPLY_OPCODE, candidate.payload()),
            format!(
                "{{\"code\":\"audit_unavailable\",\"commit\":\"query_receipt\",\"receipt_id\":\"{receipt_id}\",\"retry\":\"same_proposal\",\"schema\":\"exchange.local-management-error.v1\",\"status\":503}}"
            )
            .as_bytes(),
        );

        let replay = ceremony.handle(&tenant(), APPLY_OPCODE, candidate.payload());
        assert_eq!(replay.opcode(), RECEIPT_OPCODE);
        let replay = std::str::from_utf8(replay.payload()).expect("receipt");
        assert!(replay.contains(&format!("\"receipt_id\":\"{receipt_id}\"")));
        assert!(replay.contains("\"replayed\":true"));
    }

    #[test]
    fn the_production_constructor_draws_nonzero_opaque_receipts() {
        let scratch = Scratch::new("os-receipt");
        let store = Arc::new(GrantStore::bind(scratch.store()).expect("store"));
        let grants: Arc<dyn GrantTransactions> = store;
        let ceremony = GrantCeremony::new(grants, Arc::new(CommittedAudit));
        let candidate = ceremony.handle(
            &tenant(),
            PREVIEW_OPCODE,
            br#"{"connector":"github","selector":{"effects_within":null,"idempotency":null,"max_risk":null}}"#,
        );
        let receipt = ceremony.handle(&tenant(), APPLY_OPCODE, candidate.payload());
        assert_eq!(receipt.opcode(), RECEIPT_OPCODE);
        let value: serde_json::Value = serde_json::from_slice(receipt.payload()).expect("receipt");
        let id = value["receipt_id"].as_str().expect("receipt id");
        assert_eq!(id.len(), 64);
        assert_ne!(id, "00".repeat(32));
    }

    #[test]
    fn typed_event_sets_remain_lexical_in_candidate_bytes() {
        let scratch = Scratch::new("event-order");
        let store = Arc::new(GrantStore::bind(scratch.store()).expect("store"));
        let mut grant = Grant::for_connector("slack", Selector::default());
        grant.inbound.push(exchange_host::InboundGrant {
            connector: "slack".into(),
            binding: "socket".into(),
            events: BTreeSet::from(["message".into(), "app_mention".into()]),
        });
        store.set(&tenant(), &[grant]).expect("stored authority");
        let ceremony = ceremony(store);
        let candidate = ceremony.handle(
            &tenant(),
            PREVIEW_OPCODE,
            br#"{"connector":"slack","selector":{"effects_within":null,"idempotency":null,"max_risk":null}}"#,
        );
        let text = std::str::from_utf8(candidate.payload()).expect("candidate UTF-8");
        assert!(
            text.contains("\"events\":[\"app_mention\",\"message\"]"),
            "{text}"
        );
    }
}
