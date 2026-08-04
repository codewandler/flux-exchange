mod service_account_handoff {
    pub(super) struct HandoffFrame {
        token: Vec<u8>,
    }

    impl HandoffFrame {
        pub(super) fn new(token: Vec<u8>) -> Result<Self, ()> {
            (1..=512)
                .contains(&token.len())
                .then_some(Self { token })
                .ok_or(())
        }

        pub(super) fn encode(&self) -> Vec<u8> {
            let mut frame = b"FXSA\x01\x01\x00\x00".to_vec();
            frame.extend_from_slice(&(self.token.len() as u32).to_be_bytes());
            frame.extend_from_slice(&self.token);
            frame
        }
    }
}
#[path = "../src/local_management/service_account.rs"]
mod service_account;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use exchange_host::{Principal, PrincipalKind, Tenant};
use flux_exchange::service_account::{Expiry, ServiceAccountStore};
use service_account::{
    BindingRefusal, MintOutcome, MintPort, MintPortRefusal, MintRequest, OneShotWriter, ReceiptId,
    ServiceAccountCeremony, TokenHandoff, WriterRefusal, ERROR_OPCODE, MINT_OPCODE, QUERY_OPCODE,
    RECEIPT_OPCODE,
};

const RECEIPT: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const MINT: &[u8] = br#"{"expires_at":"4102444800","id":"runtime"}"#;

struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "flux-exchange-service-account-fxlm-{name}-{}-{}",
            std::process::id(),
            std::thread::current().name().unwrap_or("test")
        ));
        let _ = std::fs::remove_dir_all(&path);
        private_dir(&path);
        Self(path)
    }
}

#[cfg(unix)]
fn private_dir(path: &std::path::Path) {
    use std::os::unix::fs::DirBuilderExt as _;

    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .expect("owner-only scratch directory");
}

#[cfg(windows)]
fn private_dir(path: &std::path::Path) {
    std::fs::create_dir(path).expect("owner-only scratch directory");
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[derive(Default)]
struct CapturedWriter {
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
}

struct RefusingWriter(WriterRefusal);

impl OneShotWriter for RefusingWriter {
    fn write_once(self: Box<Self>, _frame: &[u8]) -> Result<(), WriterRefusal> {
        Err(self.0)
    }
}

struct RefusingPort(MintPortRefusal);

impl MintPort for RefusingPort {
    fn mint(
        &self,
        _actor: &Principal,
        _request: &MintRequest,
        _receipt_id: ReceiptId,
        _handoff: &mut dyn TokenHandoff,
    ) -> Result<MintOutcome, MintPortRefusal> {
        Err(self.0)
    }

    fn query(
        &self,
        _tenant: &Tenant,
        _receipt_id: &ReceiptId,
    ) -> Result<Option<MintOutcome>, MintPortRefusal> {
        Err(self.0)
    }
}

impl OneShotWriter for CapturedWriter {
    fn write_once(self: Box<Self>, frame: &[u8]) -> Result<(), WriterRefusal> {
        self.frames
            .lock()
            .expect("writer lock")
            .push(frame.to_vec());
        Ok(())
    }
}

struct FixturePort {
    store: Arc<ServiceAccountStore>,
    receipts: Mutex<BTreeMap<String, (String, ReceiptId)>>,
}

impl MintPort for FixturePort {
    fn mint(
        &self,
        actor: &Principal,
        request: &MintRequest,
        receipt_id: ReceiptId,
        handoff: &mut dyn TokenHandoff,
    ) -> Result<MintOutcome, MintPortRefusal> {
        let key = format!(
            "{}\0{}\0{}",
            actor.tenant().as_str(),
            request.id(),
            request.expires_at()
        );
        if let Some((_, existing)) = self.receipts.lock().expect("receipt lock").get(&key) {
            return Ok(MintOutcome::Replay {
                id: request.id().to_owned(),
                receipt_id: existing.clone(),
            });
        }

        let minted = self
            .store
            .mint(
                actor,
                request.id(),
                Expiry {
                    expires_at: request.expires_at(),
                    as_of: 4_071_000_000,
                },
            )
            .map_err(|_| MintPortRefusal::StoreUnavailable)?;
        handoff.write_token(&minted.token)?;
        self.receipts
            .lock()
            .expect("receipt lock")
            .insert(key, (request.id().to_owned(), receipt_id.clone()));
        Ok(MintOutcome::Committed {
            id: request.id().to_owned(),
            receipt_id,
        })
    }

    fn query(
        &self,
        tenant: &Tenant,
        receipt_id: &ReceiptId,
    ) -> Result<Option<MintOutcome>, MintPortRefusal> {
        Ok(self
            .receipts
            .lock()
            .expect("receipt lock")
            .iter()
            .find(|(key, (_, stored))| {
                key.starts_with(&format!("{}\0", tenant.as_str())) && stored == receipt_id
            })
            .map(|(_, (id, stored))| MintOutcome::Replay {
                id: id.clone(),
                receipt_id: stored.clone(),
            }))
    }
}

struct Fixture {
    ceremony: ServiceAccountCeremony,
    actor: Principal,
    frames: Arc<Mutex<Vec<Vec<u8>>>>,
    _scratch: Scratch,
}

fn fixture() -> Fixture {
    let scratch = Scratch::new("ceremony");
    let store = Arc::new(
        ServiceAccountStore::open(scratch.0.join("service-accounts.json")).expect("private store"),
    );
    let port = Arc::new(FixturePort {
        store,
        receipts: Mutex::new(BTreeMap::new()),
    });
    let frames = Arc::new(Mutex::new(Vec::new()));
    let _production_entropy = ServiceAccountCeremony::new(port.clone());
    let ceremony = ServiceAccountCeremony::with_receipts(port, [0x11; 32]);
    let actor = Principal::new(
        PrincipalKind::User,
        "local-owner",
        Tenant::new("local").expect("tenant"),
    );
    Fixture {
        ceremony,
        actor,
        frames,
        _scratch: scratch,
    }
}

#[test]
fn mint_writes_one_exact_fxsa_frame_and_returns_the_closed_value_free_receipt() {
    let Fixture {
        ceremony,
        actor,
        frames,
        _scratch,
    } = fixture();
    let response = ceremony.handle(
        &actor,
        MINT_OPCODE,
        MINT,
        Some(Box::new(CapturedWriter {
            frames: frames.clone(),
        })),
    );

    assert_eq!(response.opcode(), RECEIPT_OPCODE);
    assert_eq!(
        response.payload(),
        format!(
            "{{\"commit\":{{\"frame_written\":true,\"verifier\":\"committed\"}},\"id\":\"runtime\",\"receipt_id\":\"{RECEIPT}\",\"replayed\":false,\"schema\":\"exchange.service-account-mint-receipt.v1\"}}"
        )
        .as_bytes()
    );
    let frames = frames.lock().expect("frames");
    assert_eq!(frames.len(), 1);
    assert_eq!(&frames[0][..12], b"FXSA\x01\x01\x00\x00\x00\x00\x00E");
    assert_eq!(frames[0].len(), 81);
    assert!(std::str::from_utf8(&frames[0][12..])
        .expect("current opaque token happens to be UTF-8")
        .starts_with("fxsa_"));
    assert!(!std::str::from_utf8(response.payload())
        .expect("receipt JSON")
        .contains("expires"));
}

#[test]
fn same_proposal_replay_and_query_return_one_receipt_without_a_second_write() {
    let Fixture {
        ceremony,
        actor,
        frames,
        _scratch,
    } = fixture();
    let first = ceremony.handle(
        &actor,
        MINT_OPCODE,
        MINT,
        Some(Box::new(CapturedWriter {
            frames: frames.clone(),
        })),
    );
    assert_eq!(first.opcode(), RECEIPT_OPCODE);

    let replay = ceremony.handle(
        &actor,
        MINT_OPCODE,
        MINT,
        Some(Box::new(CapturedWriter {
            frames: frames.clone(),
        })),
    );
    assert_eq!(replay.opcode(), RECEIPT_OPCODE);
    assert!(std::str::from_utf8(replay.payload())
        .expect("receipt")
        .contains("\"replayed\":true"));

    let query = ceremony.handle(
        &actor,
        QUERY_OPCODE,
        format!("{{\"receipt_id\":\"{RECEIPT}\"}}").as_bytes(),
        None,
    );
    assert_eq!(query.payload(), replay.payload());
    assert_eq!(frames.lock().expect("frames").len(), 1);
}

#[test]
fn mint_and_query_controls_are_canonical_closed_and_refuse_before_writer_or_store_mutation() {
    let Fixture {
        ceremony,
        actor,
        frames,
        _scratch,
    } = fixture();
    for payload in [
        br#"{"id":"runtime","expires_at":"4102444800"}"#.as_slice(),
        br#"{"expires_at":4102444800,"id":"runtime"}"#,
        br#"{"expires_at":"04102444800","id":"runtime"}"#,
        br#"{"expires_at":"4102444800","id":"runtime","token":"sentinel"}"#,
        br#"{"expires_at":"4102444800","id":"runtime","id":"again"}"#,
    ] {
        let response = ceremony.handle(
            &actor,
            MINT_OPCODE,
            payload,
            Some(Box::new(CapturedWriter {
                frames: frames.clone(),
            })),
        );
        assert_eq!(response.opcode(), ERROR_OPCODE);
        assert_eq!(
            response.payload(),
            br#"{"code":"invalid_request","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#
        );
    }
    assert!(frames.lock().expect("frames").is_empty());

    for query in [
        br#"{"receipt_id":"0000000000000000000000000000000000000000000000000000000000000000"}"#.as_slice(),
        br#"{"receipt_id":"111111111111111111111111111111111111111111111111111111111111111A"}"#,
        br#"{"receipt_id":"1111111111111111111111111111111111111111111111111111111111111111","id":"runtime"}"#,
    ] {
        assert_eq!(
            ceremony
                .handle(&actor, QUERY_OPCODE, query, None)
                .opcode(),
            ERROR_OPCODE
        );
    }
}

#[test]
fn missing_or_misplaced_writer_is_an_exact_value_free_refusal() {
    let Fixture {
        ceremony,
        actor,
        _scratch,
        ..
    } = fixture();
    let missing = ceremony.handle(&actor, MINT_OPCODE, MINT, None);
    assert_eq!(missing.opcode(), ERROR_OPCODE);
    assert_eq!(
        missing.payload(),
        br#"{"code":"writer_invalid","commit":"none","retry":"never","schema":"exchange.local-management-error.v1","status":400}"#
    );

    let unexpected = ceremony.handle(
        &actor,
        QUERY_OPCODE,
        format!("{{\"receipt_id\":\"{RECEIPT}\"}}").as_bytes(),
        Some(Box::new(CapturedWriter::default())),
    );
    assert_eq!(unexpected.opcode(), ERROR_OPCODE);
    assert!(std::str::from_utf8(unexpected.payload())
        .expect("error")
        .contains("\"code\":\"unexpected_frame\""));
}

#[test]
fn writer_and_atomic_port_refusals_use_only_the_closed_value_free_tuples() {
    for (writer, code, status, retry) in [
        (WriterRefusal::Invalid, "writer_invalid", 400, "never"),
        (WriterRefusal::Closed, "writer_closed", 409, "operator"),
    ] {
        let Fixture {
            ceremony,
            actor,
            _scratch,
            ..
        } = fixture();
        let response = ceremony.handle(
            &actor,
            MINT_OPCODE,
            MINT,
            Some(Box::new(RefusingWriter(writer))),
        );
        assert_eq!(response.opcode(), ERROR_OPCODE);
        assert_eq!(
            response.payload(),
            format!(
                "{{\"code\":\"{code}\",\"commit\":\"none\",\"retry\":\"{retry}\",\"schema\":\"exchange.local-management-error.v1\",\"status\":{status}}}"
            )
            .as_bytes()
        );
    }

    let Fixture {
        actor, _scratch, ..
    } = fixture();
    for (refusal, code) in [
        (MintPortRefusal::Conflict, "service_account_conflict"),
        (MintPortRefusal::InvalidRequest, "invalid_request"),
    ] {
        let ceremony =
            ServiceAccountCeremony::with_receipts(Arc::new(RefusingPort(refusal)), [0x22; 32]);
        let response = ceremony.handle(
            &actor,
            MINT_OPCODE,
            MINT,
            Some(Box::new(CapturedWriter::default())),
        );
        assert_eq!(response.opcode(), ERROR_OPCODE);
        assert!(std::str::from_utf8(response.payload())
            .expect("error")
            .contains(&format!("\"code\":\"{code}\"")));
    }
}

#[test]
fn current_retained_store_surfaces_the_atomic_receipt_integration_need() {
    let scratch = Scratch::new("binding");
    let store = Arc::new(
        ServiceAccountStore::open(scratch.0.join("service-accounts.json")).expect("private store"),
    );
    assert!(matches!(
        ServiceAccountCeremony::bind_retained(store),
        Err(BindingRefusal::AtomicReceiptStoreRequired { .. })
    ));
}
