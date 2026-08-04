//! Native Service Account mint and receipt-query ceremony for `exchange.local-management.v1`.
//!
//! This module owns the closed control and receipt objects and the one-shot FXSA writer boundary.
//! The durable port deliberately includes the terminal receipt in the same mutation authority as
//! the verifier: composing [`ServiceAccountStore`] with an unrelated receipt file would create a
//! crash window in which either a principal or a receipt exists alone. The retained v0.17 store has
//! no such transaction seam, so [`ServiceAccountCeremony::bind_retained`] reports that precise
//! integration requirement instead of manufacturing a second ledger.

use std::path::PathBuf;
use std::sync::Arc;

use exchange_host::{Principal, Tenant};
use flux_exchange::service_account::{ServiceAccountStore, ServiceAccountToken};
use serde::de::{self, DeserializeOwned};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::service_account_handoff::HandoffFrame;

pub(crate) const MINT_OPCODE: u16 = 0x0020;
pub(crate) const QUERY_OPCODE: u16 = 0x0021;
pub(crate) const RECEIPT_OPCODE: u16 = 0x0022;
pub(crate) const ERROR_OPCODE: u16 = 0x7fff;

const MAX_CONTROL_BYTES: usize = 65_536;
const RECEIPT_SCHEMA: &str = "exchange.service-account-mint-receipt.v1";
const ERROR_SCHEMA: &str = "exchange.local-management-error.v1";

/// Why the retained Service Account store cannot yet bind this ceremony.
#[derive(Debug, thiserror::Error)]
pub(crate) enum BindingRefusal {
    /// The verifier store cannot atomically publish/query the terminal proposal receipt.
    #[error(
        "Service Account store {path} has no atomic verifier/proposal/receipt transaction seam"
    )]
    AtomicReceiptStoreRequired { path: PathBuf },
}

/// One server-to-client Service Account control frame before its shared FXLM header.
pub(crate) struct ServiceAccountFrame {
    opcode: u16,
    payload: Vec<u8>,
}

impl ServiceAccountFrame {
    pub(crate) const fn opcode(&self) -> u16 {
        self.opcode
    }

    pub(crate) fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// An opaque nonzero 256-bit terminal receipt identity.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ReceiptId([u8; 32]);

impl ReceiptId {
    fn from_bytes(bytes: [u8; 32]) -> Option<Self> {
        (bytes != [0; 32]).then_some(Self(bytes))
    }

    fn parse(value: &str) -> Result<Self, ()> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(());
        }
        let mut bytes = [0; 32];
        for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
            bytes[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
        }
        Self::from_bytes(bytes).ok_or(())
    }

    fn encoded(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut encoded = String::with_capacity(64);
        for byte in self.0 {
            encoded.push(HEX[usize::from(byte >> 4)] as char);
            encoded.push(HEX[usize::from(byte & 0x0f)] as char);
        }
        encoded
    }
}

impl Serialize for ReceiptId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.encoded())
    }
}

impl<'de> Deserialize<'de> for ReceiptId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(|()| de::Error::custom("invalid receipt identity"))
    }
}

fn nibble(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        _ => Err(()),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct MintControl {
    expires_at: ExpiresAt,
    id: ServiceAccountId,
}

/// One validated canonical MINT proposal, containing no token or writer identity.
pub(crate) struct MintRequest {
    expires_at: i64,
    id: String,
}

impl MintRequest {
    pub(crate) fn id(&self) -> &str {
        &self.id
    }

    pub(crate) const fn expires_at(&self) -> i64 {
        self.expires_at
    }
}

struct ServiceAccountId(String);

impl Serialize for ServiceAccountId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ServiceAccountId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || value.len() > 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            return Err(de::Error::custom("invalid Service Account identifier"));
        }
        Ok(Self(value))
    }
}

struct ExpiresAt(i64);

impl Serialize for ExpiresAt {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for ExpiresAt {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || value.len() > 19
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(de::Error::custom("noncanonical expiry"));
        }
        let parsed = value
            .parse::<i64>()
            .map_err(|_| de::Error::custom("expiry is outside the signed 64-bit range"))?;
        if parsed <= 0 {
            return Err(de::Error::custom("expiry must be positive"));
        }
        Ok(Self(parsed))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct QueryControl {
    receipt_id: ReceiptId,
}

/// The sole writer capability associated with one admitted native MINT.
pub(crate) trait OneShotWriter: Send {
    /// Consume the capability while writing the complete FXSA frame once, then close for EOF.
    fn write_once(self: Box<Self>, frame: &[u8]) -> Result<(), WriterRefusal>;
}

/// Value-free classification from capability validation or the one write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriterRefusal {
    Invalid,
    Closed,
}

/// Token disclosure callback owned by the atomic verifier/receipt transaction port.
pub(crate) trait TokenHandoff {
    fn write_token(&mut self, token: &ServiceAccountToken) -> Result<(), MintPortRefusal>;
}

/// A terminal mint record returned by the atomic verifier/proposal/receipt authority.
pub(crate) enum MintOutcome {
    Committed { id: String, receipt_id: ReceiptId },
    Replay { id: String, receipt_id: ReceiptId },
}

/// Closed value-free outcomes from that atomic authority.
#[derive(Clone, Copy)]
pub(crate) enum MintPortRefusal {
    Conflict,
    InvalidRequest,
    WriterInvalid,
    WriterClosed,
    StoreUnavailable,
    Internal,
}

/// The narrow durable seam required to make verifier, proposal and receipt one authority.
pub(crate) trait MintPort: Send + Sync {
    fn mint(
        &self,
        actor: &Principal,
        request: &MintRequest,
        receipt_id: ReceiptId,
        handoff: &mut dyn TokenHandoff,
    ) -> Result<MintOutcome, MintPortRefusal>;

    fn query(
        &self,
        tenant: &Tenant,
        receipt_id: &ReceiptId,
    ) -> Result<Option<MintOutcome>, MintPortRefusal>;
}

trait ReceiptIds: Send + Sync {
    fn next(&self) -> Result<ReceiptId, ()>;
}

struct OsReceiptIds;

impl ReceiptIds for OsReceiptIds {
    fn next(&self) -> Result<ReceiptId, ()> {
        for _ in 0..4 {
            let bytes = flux_exchange::entropy::bytes::<32>().map_err(|_| ())?;
            if let Some(receipt) = ReceiptId::from_bytes(bytes) {
                return Ok(receipt);
            }
        }
        Err(())
    }
}

#[cfg(test)]
struct FixedReceiptId([u8; 32]);

#[cfg(test)]
impl ReceiptIds for FixedReceiptId {
    fn next(&self) -> Result<ReceiptId, ()> {
        ReceiptId::from_bytes(self.0).ok_or(())
    }
}

/// One exact native Service Account MINT or QUERY operation.
pub(crate) struct ServiceAccountCeremony {
    port: Arc<dyn MintPort>,
    receipts: Arc<dyn ReceiptIds>,
}

impl ServiceAccountCeremony {
    pub(crate) fn new(port: Arc<dyn MintPort>) -> Self {
        Self {
            port,
            receipts: Arc::new(OsReceiptIds),
        }
    }

    /// The retained store cannot be adapted safely until it owns the terminal receipt atomically.
    pub(crate) fn bind_retained(store: Arc<ServiceAccountStore>) -> Result<Self, BindingRefusal> {
        Err(BindingRefusal::AtomicReceiptStoreRequired {
            path: store.path().to_path_buf(),
        })
    }

    #[cfg(test)]
    pub(crate) fn with_receipts(port: Arc<dyn MintPort>, receipt: [u8; 32]) -> Self {
        Self {
            port,
            receipts: Arc::new(FixedReceiptId(receipt)),
        }
    }

    /// Handle one admitted native opcode as this connection's sole logical operation.
    pub(crate) fn handle(
        &self,
        actor: &Principal,
        opcode: u16,
        payload: &[u8],
        writer: Option<Box<dyn OneShotWriter>>,
    ) -> ServiceAccountFrame {
        match opcode {
            MINT_OPCODE => self.mint(actor, payload, writer),
            QUERY_OPCODE if writer.is_none() => self.query(actor.tenant(), payload),
            QUERY_OPCODE => refusal(Refusal::UnexpectedFrame),
            _ => refusal(Refusal::UnexpectedFrame),
        }
    }

    fn mint(
        &self,
        actor: &Principal,
        payload: &[u8],
        writer: Option<Box<dyn OneShotWriter>>,
    ) -> ServiceAccountFrame {
        let Some(writer) = writer else {
            return refusal(Refusal::WriterInvalid);
        };
        let request: MintControl = match canonical(payload) {
            Ok(request) => request,
            Err(()) => return refusal(Refusal::InvalidRequest),
        };
        let request = MintRequest {
            expires_at: request.expires_at.0,
            id: request.id.0,
        };
        let receipt_id = match self.receipts.next() {
            Ok(receipt) => receipt,
            Err(()) => return refusal(Refusal::Internal),
        };
        let mut handoff = FxsaHandoff {
            writer: Some(writer),
            written: false,
        };
        match self.port.mint(actor, &request, receipt_id, &mut handoff) {
            Ok(MintOutcome::Committed { id, receipt_id }) if handoff.written => {
                receipt(id, receipt_id, false)
            }
            Ok(MintOutcome::Replay { id, receipt_id }) if !handoff.written => {
                receipt(id, receipt_id, true)
            }
            Ok(_) => refusal(Refusal::Internal),
            Err(error) => refusal(Refusal::from(error)),
        }
    }

    fn query(&self, tenant: &Tenant, payload: &[u8]) -> ServiceAccountFrame {
        let request: QueryControl = match canonical(payload) {
            Ok(request) => request,
            Err(()) => return refusal(Refusal::InvalidRequest),
        };
        match self.port.query(tenant, &request.receipt_id) {
            Ok(Some(MintOutcome::Committed { id, receipt_id }))
            | Ok(Some(MintOutcome::Replay { id, receipt_id })) => receipt(id, receipt_id, true),
            Ok(None) => refusal(Refusal::InvalidRequest),
            Err(error) => refusal(Refusal::from(error)),
        }
    }
}

struct FxsaHandoff {
    writer: Option<Box<dyn OneShotWriter>>,
    written: bool,
}

impl TokenHandoff for FxsaHandoff {
    fn write_token(&mut self, token: &ServiceAccountToken) -> Result<(), MintPortRefusal> {
        let writer = self.writer.take().ok_or(MintPortRefusal::WriterClosed)?;
        let frame = HandoffFrame::new(token.as_str().as_bytes().to_vec())
            .map_err(|_| MintPortRefusal::Internal)?;
        writer
            .write_once(&frame.encode())
            .map_err(|refusal| match refusal {
                WriterRefusal::Invalid => MintPortRefusal::WriterInvalid,
                WriterRefusal::Closed => MintPortRefusal::WriterClosed,
            })?;
        self.written = true;
        Ok(())
    }
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

#[derive(Serialize)]
struct ReceiptCommit {
    frame_written: bool,
    verifier: &'static str,
}

#[derive(Serialize)]
struct ReceiptBody {
    commit: ReceiptCommit,
    id: String,
    receipt_id: ReceiptId,
    replayed: bool,
    schema: &'static str,
}

fn receipt(id: String, receipt_id: ReceiptId, replayed: bool) -> ServiceAccountFrame {
    response(
        RECEIPT_OPCODE,
        &ReceiptBody {
            commit: ReceiptCommit {
                frame_written: true,
                verifier: "committed",
            },
            id,
            receipt_id,
            replayed,
            schema: RECEIPT_SCHEMA,
        },
    )
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    commit: &'static str,
    retry: &'static str,
    schema: &'static str,
    status: u16,
}

#[derive(Clone, Copy)]
enum Refusal {
    InvalidRequest,
    UnexpectedFrame,
    Conflict,
    WriterInvalid,
    WriterClosed,
    Store,
    Internal,
}

impl From<MintPortRefusal> for Refusal {
    fn from(refusal: MintPortRefusal) -> Self {
        match refusal {
            MintPortRefusal::Conflict => Self::Conflict,
            MintPortRefusal::InvalidRequest => Self::InvalidRequest,
            MintPortRefusal::WriterInvalid => Self::WriterInvalid,
            MintPortRefusal::WriterClosed => Self::WriterClosed,
            MintPortRefusal::StoreUnavailable => Self::Store,
            MintPortRefusal::Internal => Self::Internal,
        }
    }
}

impl Refusal {
    const fn tuple(self) -> (&'static str, u16, &'static str) {
        match self {
            Self::InvalidRequest => ("invalid_request", 400, "never"),
            Self::UnexpectedFrame => ("unexpected_frame", 409, "never"),
            Self::Conflict => ("service_account_conflict", 409, "refresh"),
            Self::WriterInvalid => ("writer_invalid", 400, "never"),
            Self::WriterClosed => ("writer_closed", 409, "operator"),
            Self::Store => ("store_unavailable", 503, "operator"),
            Self::Internal => ("internal_refusal", 500, "operator"),
        }
    }
}

fn refusal(refusal: Refusal) -> ServiceAccountFrame {
    let (code, status, retry) = refusal.tuple();
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

fn response(opcode: u16, body: &impl Serialize) -> ServiceAccountFrame {
    match serde_json::to_vec(body) {
        Ok(payload) if payload.len() <= MAX_CONTROL_BYTES => ServiceAccountFrame { opcode, payload },
        Ok(_) | Err(_) => ServiceAccountFrame {
            opcode: ERROR_OPCODE,
            payload: br#"{"code":"internal_refusal","commit":"none","retry":"operator","schema":"exchange.local-management-error.v1","status":500}"#.to_vec(),
        },
    }
}
