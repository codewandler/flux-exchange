//! Revisioned whole-tenant grant replacement for owner-bound local management.
//!
//! The ordinary [`crate::Grants`] port answers the invocation gate. This module adds the narrower
//! mutation contract X-134 needs: project exactly one expressible connector grant, bind the
//! candidate to a durable whole-tenant revision, and replace that one vector position without
//! reconstructing any unrelated row.

use std::collections::BTreeSet;
use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use crate::{Effect, Idempotency, Risk, Tenant};

const GRANT_PROPOSAL_DOMAIN: &[u8] = b"exchange.local-management.v1.grant-proposal";

/// A nonzero monotonic whole-tenant grant-set revision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StoreRevision(u64);

impl StoreRevision {
    /// Construct a nonzero revision.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 {
            None
        } else {
            Some(Self(value))
        }
    }

    /// The checked successor used by one successful whole-set mutation.
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// The numeric high-water mark held by the store.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for StoreRevision {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}

impl Serialize for StoreRevision {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for StoreRevision {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        if encoded.is_empty()
            || (encoded.len() > 1 && encoded.starts_with('0'))
            || !encoded.bytes().all(|byte| byte.is_ascii_digit())
        {
            return Err(serde::de::Error::custom("noncanonical store revision"));
        }
        let value = encoded
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom("store revision is outside u64"))?;
        Self::new(value).ok_or_else(|| serde::de::Error::custom("store revision must be nonzero"))
    }
}

/// The three metadata selector axes expressible by local-management v1.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantSelector {
    /// Admit only operations whose effects are a subset of this set.
    pub effects_within: Option<BTreeSet<Effect>>,
    /// Admit only operations with this idempotency.
    pub idempotency: Option<Idempotency>,
    /// Admit only operations at or below this risk.
    pub max_risk: Option<Risk>,
}

impl Serialize for GrantSelector {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        struct Wire<'a> {
            effects_within: Option<Vec<&'static str>>,
            idempotency: &'a Option<Idempotency>,
            max_risk: &'a Option<Risk>,
        }

        let effects_within = self.effects_within.as_ref().map(|effects| {
            let mut words = effects.iter().copied().map(effect_word).collect::<Vec<_>>();
            words.sort_unstable();
            words
        });
        Wire {
            effects_within,
            idempotency: &self.idempotency,
            max_risk: &self.max_risk,
        }
        .serialize(serializer)
    }
}

fn effect_word(effect: Effect) -> &'static str {
    match effect {
        Effect::Network => "network",
        Effect::Process => "process",
        Effect::WorkspaceWrite => "workspace_write",
    }
}

/// One inbound binding retained in a grant candidate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantCandidateInbound {
    /// Released connector binding id.
    pub binding: String,
    /// The nonempty released event set, in typed lexical order.
    pub events: BTreeSet<String>,
}

/// The complete connector-scoped grant projection local management can replace losslessly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantCandidate {
    /// Released connector id.
    pub connector: String,
    /// Existing inbound authority, retaining the stored vector order.
    pub inbound: Vec<GrantCandidateInbound>,
    /// Proposed selector axes.
    pub selector: GrantSelector,
}

impl GrantCandidate {
    /// RFC 8785 bytes of the exact grant proposal input object.
    ///
    /// This closed value contains only strings, nulls and arrays. Its struct members are declared
    /// in lexical order and every set serializer emits lexical order, so `serde_json`'s compact
    /// UTF-8 representation is the RFC 8785 representation for this vocabulary.
    pub fn proposal_input(
        &self,
        revision: &StoreRevision,
    ) -> Result<Vec<u8>, GrantTransactionRefusal> {
        #[derive(Serialize)]
        struct Input<'a> {
            candidate: &'a GrantCandidate,
            revision: &'a StoreRevision,
        }

        serde_json::to_vec(&Input {
            candidate: self,
            revision,
        })
        .map_err(|error| GrantTransactionRefusal::Store {
            reason: format!("the grant proposal could not be serialized: {error}"),
        })
    }

    /// SHA-256 of the exact domain, one zero byte and [`proposal_input`](Self::proposal_input).
    pub fn proposal_digest(
        &self,
        revision: &StoreRevision,
    ) -> Result<GrantProposalDigest, GrantTransactionRefusal> {
        let input = self.proposal_input(revision)?;
        let mut hasher = Sha256::new();
        hasher.update(GRANT_PROPOSAL_DOMAIN);
        hasher.update([0]);
        hasher.update(input);
        Ok(GrantProposalDigest(hasher.finalize().into()))
    }
}

macro_rules! opaque_256 {
    ($name:ident, $doc:literal, $reject_zero:expr) => {
        #[doc = $doc]
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name([u8; 32]);

        impl $name {
            /// Construct from the protocol's 32 bytes.
            pub const fn from_protocol_bytes(bytes: [u8; 32]) -> Option<Self> {
                if $reject_zero && is_zero(&bytes) {
                    None
                } else {
                    Some(Self(bytes))
                }
            }

            /// Parse exactly 64 lowercase hexadecimal characters.
            pub fn parse(encoded: &str) -> Option<Self> {
                decode_lowerhex(encoded).and_then(Self::from_protocol_bytes)
            }

            /// The protocol bytes without assigning them numeric meaning.
            pub const fn protocol_bytes(self) -> [u8; 32] {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&encode_lowerhex(&self.0))
            }
        }

        impl Serialize for $name {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: Serializer,
            {
                serializer.serialize_str(&self.to_string())
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let encoded = String::deserialize(deserializer)?;
                Self::parse(&encoded).ok_or_else(|| {
                    serde::de::Error::custom(concat!(
                        stringify!($name),
                        " is not canonical lowerhex"
                    ))
                })
            }
        }
    };
}

opaque_256!(
    GrantProposalDigest,
    "The exact value-free digest binding a candidate and precondition revision.",
    false
);
opaque_256!(
    GrantReceiptId,
    "A nonzero opaque terminal grant receipt identity.",
    true
);

const fn is_zero(bytes: &[u8; 32]) -> bool {
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != 0 {
            return false;
        }
        index += 1;
    }
    true
}

fn encode_lowerhex(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(64);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn decode_lowerhex(encoded: &str) -> Option<[u8; 32]> {
    if encoded.len() != 64 {
        return None;
    }
    let mut decoded = [0; 32];
    for (index, pair) in encoded.as_bytes().chunks_exact(2).enumerate() {
        let high = lowerhex_nibble(pair[0])?;
        let low = lowerhex_nibble(pair[1])?;
        decoded[index] = (high << 4) | low;
    }
    Some(decoded)
}

const fn lowerhex_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

/// A preview candidate and the exact CAS/digest facts APPLY must echo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GrantPreview {
    /// Complete connector-scoped replacement.
    pub candidate: GrantCandidate,
    /// Digest of the exact candidate and revision.
    pub proposal_digest: GrantProposalDigest,
    /// Precondition whole-tenant revision.
    pub revision: StoreRevision,
}

/// One terminal whole-set commit record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantApplyReceipt {
    /// Connector whose one row was replaced or created.
    pub connector: String,
    /// Durable opaque receipt id.
    pub receipt_id: GrantReceiptId,
    /// True for same-proposal replay and query.
    pub replayed: bool,
    /// Post-commit whole-tenant revision.
    pub revision: StoreRevision,
}

/// Why preview, CAS apply or receipt query refused. Every variant is value-free.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GrantTransactionRefusal {
    /// The selected stored authority cannot be represented without dropping meaning.
    #[error("the selected connector grant is not expressible by local management")]
    Unexpressible,
    /// The whole-tenant set changed after preview.
    #[error("grant revision is stale: expected {expected}, current {current}")]
    Stale {
        /// Revision carried by APPLY.
        expected: StoreRevision,
        /// Current durable revision.
        current: StoreRevision,
    },
    /// Candidate or revision bytes do not match the carried proposal digest.
    #[error("the grant proposal digest does not match the candidate")]
    DigestMismatch,
    /// The monotonic u64 high-water mark cannot advance.
    #[error("the grant revision space is exhausted")]
    RevisionExhausted,
    /// A caller-supplied receipt id is already occupied by another proposal.
    #[error("the grant receipt identity is already occupied")]
    ReceiptConflict,
    /// The durable store could not be read or atomically replaced.
    #[error("the grant transaction store is unavailable: {reason}")]
    Store {
        /// Value-free operator diagnostic.
        reason: String,
    },
}

/// Revisioned whole-tenant grant replacement, separate from the invocation read port.
pub trait GrantTransactions: Send + Sync {
    /// Project one connector change after atomically initializing legacy revision state.
    fn preview(
        &self,
        tenant: &Tenant,
        connector: &str,
        selector: GrantSelector,
    ) -> Result<GrantPreview, GrantTransactionRefusal>;

    /// Atomically replace the selected row, increment once and retain the terminal receipt.
    fn apply(
        &self,
        tenant: &Tenant,
        candidate: &GrantCandidate,
        revision: StoreRevision,
        proposal_digest: GrantProposalDigest,
        receipt_id: GrantReceiptId,
    ) -> Result<GrantApplyReceipt, GrantTransactionRefusal>;

    /// Apply while exposing the first instant at which this receipt is durably queryable.
    ///
    /// The default preserves compatibility for non-file implementations. Durable production
    /// stores override this method and call `decided` immediately after fsync (or immediately on a
    /// byte-identical replay), before publishing later in-memory or audit projections.
    fn apply_observed(
        &self,
        tenant: &Tenant,
        candidate: &GrantCandidate,
        revision: StoreRevision,
        proposal_digest: GrantProposalDigest,
        receipt_id: GrantReceiptId,
        decided: &mut dyn FnMut(GrantReceiptId),
    ) -> Result<GrantApplyReceipt, GrantTransactionRefusal> {
        let receipt = self.apply(tenant, candidate, revision, proposal_digest, receipt_id)?;
        decided(receipt.receipt_id);
        Ok(receipt)
    }

    /// Query a receipt in the resolved tenant only.
    fn query(
        &self,
        tenant: &Tenant,
        receipt_id: GrantReceiptId,
    ) -> Result<Option<GrantApplyReceipt>, GrantTransactionRefusal>;
}
