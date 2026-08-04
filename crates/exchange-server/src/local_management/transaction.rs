//! Value-free durable ownership of prepared credential transactions.
//!
//! The provider owns staged secret bytes and their state machine. Exchange owns only opaque ids,
//! proposal identity, the commit decision and receipt lookup needed to recover that state machine.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use exchange_host::{
    PreparedSecretError, PreparedSecretStore, SecretBatch, SecretProposalDigest,
    SecretTransactionGeneration, SecretTransactionId, SecretTransactionState,
};
use rusqlite::{params, Connection, OptionalExtension as _, TransactionBehavior};

const SCHEMA_VERSION: &str = "exchange.transaction-journal.v1";

/// One admitted value-free proposal kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransactionKind {
    Connect,
    Credential,
}

impl TransactionKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Connect => "connect",
            Self::Credential => "credential",
        }
    }
}

/// The opaque identities returned before any secret bytes are accepted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Allocation {
    id: SecretTransactionId,
    generation: SecretTransactionGeneration,
    proposal: SecretProposalDigest,
    receipt: ReceiptId,
}

impl Allocation {
    pub const fn transaction_id(&self) -> SecretTransactionId {
        self.id
    }

    pub const fn receipt_id(&self) -> ReceiptId {
        self.receipt
    }

    /// Opaque provider identity for the FXLM control object. It must never enter a URL or log.
    pub fn transaction_protocol_bytes(&self) -> [u8; 32] {
        self.id.protocol_bytes()
    }

    /// Exact proposal identity associated with this allocation.
    pub fn proposal_protocol_bytes(&self) -> [u8; 32] {
        self.proposal.protocol_bytes()
    }
}

/// A separate opaque receipt identity. The all-zero value is reserved and never emitted.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct ReceiptId([u8; 32]);

impl std::fmt::Debug for ReceiptId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("ReceiptId(<opaque>)")
    }
}

impl ReceiptId {
    pub fn from_protocol_bytes(bytes: [u8; 32]) -> Option<Self> {
        bytes.iter().any(|byte| *byte != 0).then_some(Self(bytes))
    }

    pub const fn protocol_bytes(self) -> [u8; 32] {
        self.0
    }
}

/// Whether an exact proposal is unresolved or already has its terminal receipt.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProposalState {
    Active,
    Committed(ReceiptId),
}

/// One durable, value-free publication still owed after provider recovery.
pub struct PendingPublication {
    receipt: ReceiptId,
    bytes: Vec<u8>,
}

impl PendingPublication {
    pub const fn receipt_id(&self) -> ReceiptId {
        self.receipt
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// Durable coordinator around one process-lifetime prepared-store port.
pub struct TransactionCoordinator {
    provider: Arc<dyn PreparedSecretStore>,
    journal: Mutex<Connection>,
    path: PathBuf,
}

impl TransactionCoordinator {
    /// Bind one owner-only journal without opening the credential store a second time.
    pub fn bind(
        path: impl AsRef<Path>,
        provider: Arc<dyn PreparedSecretStore>,
    ) -> Result<Self, CoordinatorError> {
        let path = path.as_ref();
        let parent = path
            .parent()
            .ok_or_else(|| CoordinatorError::UnsafeJournal {
                path: path.to_path_buf(),
                reason: "the transaction journal path has no parent directory".to_owned(),
            })?;
        exchange_host::ensure_private_state_directory(parent).map_err(|error| {
            CoordinatorError::UnsafeJournal {
                path: parent.to_path_buf(),
                reason: error.to_string(),
            }
        })?;
        exchange_host::ensure_private_state_file(path).map_err(|error| {
            CoordinatorError::UnsafeJournal {
                path: path.to_path_buf(),
                reason: error.to_string(),
            }
        })?;
        let sqlite_journal = sqlite_journal_path(path);
        exchange_host::ensure_private_state_file(&sqlite_journal).map_err(|error| {
            CoordinatorError::UnsafeJournal {
                path: sqlite_journal,
                reason: error.to_string(),
            }
        })?;
        let connection = Connection::open(path).map_err(|source| CoordinatorError::Journal {
            path: path.to_path_buf(),
            source,
        })?;
        initialise(&connection, path)?;
        Ok(Self {
            provider,
            journal: Mutex::new(connection),
            path: path.to_path_buf(),
        })
    }

    /// Allocate both provider transaction components and a distinct receipt id durably.
    pub fn allocate(
        &self,
        kind: TransactionKind,
        connector: &str,
        label: &str,
        proposal: SecretProposalDigest,
    ) -> Result<Allocation, CoordinatorError> {
        self.allocate_for_tenant(kind, "", connector, label, proposal)
    }

    /// Allocate inside one server-derived tenant boundary.
    pub fn allocate_for_tenant(
        &self,
        kind: TransactionKind,
        tenant: &str,
        connector: &str,
        label: &str,
        proposal: SecretProposalDigest,
    ) -> Result<Allocation, CoordinatorError> {
        let nonce = nonzero_nonce()?;
        let receipt = nonzero_receipt()?;
        self.allocate_with(kind, tenant, connector, label, proposal, nonce, receipt)
    }

    fn allocate_with(
        &self,
        kind: TransactionKind,
        tenant: &str,
        connector: &str,
        label: &str,
        proposal: SecretProposalDigest,
        nonce: [u8; 24],
        receipt: ReceiptId,
    ) -> Result<Allocation, CoordinatorError> {
        if nonce.iter().all(|byte| *byte == 0) {
            return Err(CoordinatorError::IdentityCollision);
        }
        let mut journal = self.lock()?;
        let transaction = journal
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| self.database(source))?;
        let current: String = transaction
            .query_row(
                "SELECT value FROM coordinator_metadata WHERE key = 'next_generation'",
                [],
                |row| row.get(0),
            )
            .map_err(|source| self.database(source))?;
        let generation_value = current
            .parse::<u64>()
            .ok()
            .filter(|value| *value != 0)
            .ok_or(CoordinatorError::GenerationCorrupt)?;
        let generation =
            SecretTransactionGeneration::from_protocol_bytes(generation_value.to_be_bytes())
                .ok_or(CoordinatorError::GenerationCorrupt)?;
        let next = generation
            .checked_next()
            .ok_or(CoordinatorError::GenerationExhausted)?;
        let id = SecretTransactionId::new(generation, nonce);
        let inserted = transaction.execute(
            "INSERT INTO transactions (
                 transaction_id, generation, nonce, proposal_digest, receipt_id, kind,
                 tenant, connector, label, phase, decided
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, 'allocated', 0)",
            params![
                id.protocol_bytes().as_slice(),
                generation_value.to_string(),
                nonce.as_slice(),
                proposal.protocol_bytes().as_slice(),
                receipt.protocol_bytes().as_slice(),
                kind.as_str(),
                tenant,
                connector,
                label,
            ],
        );
        match inserted {
            Ok(1) => {}
            Ok(_) => return Err(CoordinatorError::Invariant("allocation inserted no row")),
            Err(source) if is_constraint(&source) => {
                return Err(CoordinatorError::IdentityCollision)
            }
            Err(source) => return Err(self.database(source)),
        }
        transaction
            .execute(
                "UPDATE coordinator_metadata SET value = ?1 WHERE key = 'next_generation'",
                [u64::from_be_bytes(next.protocol_bytes()).to_string()],
            )
            .map_err(|source| self.database(source))?;
        transaction
            .commit()
            .map_err(|source| self.database(source))?;
        Ok(Allocation {
            id,
            generation,
            proposal,
            receipt,
        })
    }

    /// Return exact same-proposal state without allocating or prompting again.
    pub fn proposal_state(
        &self,
        kind: TransactionKind,
        connector: &str,
        label: &str,
        proposal: SecretProposalDigest,
    ) -> Result<Option<ProposalState>, CoordinatorError> {
        self.proposal_state_for_tenant(kind, "", connector, label, proposal)
    }

    /// Return same-proposal state only inside the server-derived tenant boundary.
    pub fn proposal_state_for_tenant(
        &self,
        kind: TransactionKind,
        tenant: &str,
        connector: &str,
        label: &str,
        proposal: SecretProposalDigest,
    ) -> Result<Option<ProposalState>, CoordinatorError> {
        let journal = self.lock()?;
        let row: Option<(Vec<u8>, String)> = journal
            .query_row(
                "SELECT receipt_id, phase FROM transactions
                 WHERE kind = ?1 AND tenant = ?2 AND connector = ?3 AND label = ?4
                   AND proposal_digest = ?5
                 ORDER BY rowid DESC LIMIT 1",
                params![
                    kind.as_str(),
                    tenant,
                    connector,
                    label,
                    proposal.protocol_bytes().as_slice()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|source| self.database(source))?;
        row.map(|(receipt, phase)| {
            if phase == "terminal" {
                Ok(ProposalState::Committed(decode_receipt(&receipt)?))
            } else {
                Ok(ProposalState::Active)
            }
        })
        .transpose()
    }

    /// Attach the complete non-secret roll-forward image before any provider prepare.
    pub fn attach_publication(
        &self,
        allocation: Allocation,
        canonical: &[u8],
    ) -> Result<(), CoordinatorError> {
        let journal = self.lock()?;
        let changed = journal
            .execute(
                "UPDATE transactions SET publication = ?1, published = 0
                 WHERE transaction_id = ?2 AND proposal_digest = ?3
                   AND phase = 'allocated' AND publication IS NULL",
                params![
                    canonical,
                    allocation.id.protocol_bytes().as_slice(),
                    allocation.proposal.protocol_bytes().as_slice(),
                ],
            )
            .map_err(|source| self.database(source))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(CoordinatorError::InvalidPhase)
        }
    }

    /// Every provider-terminal row whose value-free metadata/audit image is not yet published.
    pub fn pending_publications(&self) -> Result<Vec<PendingPublication>, CoordinatorError> {
        let journal = self.lock()?;
        let mut statement = journal
            .prepare(
                "SELECT receipt_id, publication FROM transactions
                 WHERE phase = 'terminal' AND published = 0
                 ORDER BY rowid",
            )
            .map_err(|source| self.database(source))?;
        let publications = statement
            .query_map([], |row| {
                Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
            })
            .map_err(|source| self.database(source))?
            .map(|row| {
                let (receipt, bytes) = row.map_err(|source| self.database(source))?;
                Ok(PendingPublication {
                    receipt: decode_receipt(&receipt)?,
                    bytes,
                })
            })
            .collect();
        publications
    }

    /// Whether one tenant/connector still has a provider-committed public image to roll forward.
    ///
    /// A same-proposal replay may resolve this row, but a different mutation must not observe the
    /// intermediate durable steps or race them after the original in-process claim is released.
    pub fn publication_pending_for(
        &self,
        tenant: &str,
        connector: &str,
    ) -> Result<bool, CoordinatorError> {
        self.lock()?
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transactions
                 WHERE tenant = ?1 AND connector = ?2
                   AND phase = 'terminal' AND published = 0)",
                params![tenant, connector],
                |row| row.get(0),
            )
            .map_err(|source| self.database(source))
    }

    /// Close one exact roll-forward row only after metadata and audit are both durable.
    pub fn mark_published(&self, receipt: ReceiptId) -> Result<(), CoordinatorError> {
        let journal = self.lock()?;
        let changed = journal
            .execute(
                "UPDATE transactions SET published = 1
                 WHERE receipt_id = ?1 AND phase = 'terminal' AND published = 0",
                [receipt.protocol_bytes().as_slice()],
            )
            .map_err(|source| self.database(source))?;
        let terminal = journal
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transactions WHERE receipt_id = ?1 AND phase = 'terminal' AND published = 1)",
                [receipt.protocol_bytes().as_slice()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|source| self.database(source))?;
        if changed == 1 || terminal {
            Ok(())
        } else {
            Err(CoordinatorError::UnknownAllocation)
        }
    }

    /// Value-free publication image for receipt query or same-proposal replay.
    pub fn publication(&self, receipt: ReceiptId) -> Result<Option<Vec<u8>>, CoordinatorError> {
        let journal = self.lock()?;
        journal
            .query_row(
                "SELECT publication FROM transactions WHERE receipt_id = ?1 AND phase = 'terminal'",
                [receipt.protocol_bytes().as_slice()],
                |row| row.get(0),
            )
            .optional()
            .map_err(|source| self.database(source))
    }

    /// Whether public metadata and canonical audit have already crossed their durable boundary.
    pub fn publication_is_complete(&self, receipt: ReceiptId) -> Result<bool, CoordinatorError> {
        let journal = self.lock()?;
        journal
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transactions
                 WHERE receipt_id = ?1 AND phase = 'terminal' AND published = 1)",
                [receipt.protocol_bytes().as_slice()],
                |row| row.get(0),
            )
            .map_err(|source| self.database(source))
    }

    /// Prepare the complete provider-owned candidate. Exchange never inspects its mutations.
    pub async fn prepare(
        &self,
        allocation: Allocation,
        batch: &SecretBatch,
    ) -> Result<SecretTransactionState, CoordinatorRefusal> {
        self.require_phase(allocation, &["allocated", "prepared"])
            .map_err(CoordinatorRefusal::internal_predecision)?;
        let state = match self
            .provider
            .prepare(allocation.id, allocation.proposal, batch)
            .await
        {
            Ok(state) => state,
            Err(PreparedSecretError::Backend) => match self.provider.state(allocation.id).await {
                Ok(SecretTransactionState::Absent) => self
                    .provider
                    .prepare(allocation.id, allocation.proposal, batch)
                    .await
                    .map_err(|error| CoordinatorRefusal::provider(error, DecisionPhase::Before))?,
                Ok(state) => state,
                Err(error) => {
                    return Err(CoordinatorRefusal::provider(error, DecisionPhase::Before));
                }
            },
            Err(error) => {
                return Err(CoordinatorRefusal::provider(error, DecisionPhase::Before));
            }
        };
        match state {
            SecretTransactionState::Absent => Err(CoordinatorRefusal::internal_predecision(
                CoordinatorError::Invariant("prepare returned Absent"),
            )),
            SecretTransactionState::Prepared => {
                self.set_phase(allocation, "prepared", false)
                    .map_err(CoordinatorRefusal::internal_predecision)?;
                Ok(state)
            }
            SecretTransactionState::Committed => Err(CoordinatorRefusal::internal_predecision(
                CoordinatorError::Invariant("prepare returned Committed before a decision"),
            )),
        }
    }

    /// Persist the one-way commit decision before asking the provider to publish anything.
    pub fn decide_commit(&self, allocation: Allocation) -> Result<(), CoordinatorRefusal> {
        self.set_phase(allocation, "decided", true)
            .map_err(CoordinatorRefusal::internal_predecision)
    }

    /// Roll the provider forward after the durable decision and publish the receipt lookup.
    pub async fn commit(&self, allocation: Allocation) -> Result<ReceiptId, CoordinatorRefusal> {
        self.require_decided(allocation).map_err(|error| {
            CoordinatorRefusal::internal_postdecision(error, allocation.receipt)
        })?;
        let state = match self.provider.commit(allocation.id).await {
            Ok(state) => state,
            Err(PreparedSecretError::Backend) => match self.provider.state(allocation.id).await {
                Ok(SecretTransactionState::Prepared) => {
                    self.provider.commit(allocation.id).await.map_err(|error| {
                        CoordinatorRefusal::provider_postdecision(error, allocation.receipt)
                    })?
                }
                Ok(state) => state,
                Err(error) => {
                    return Err(CoordinatorRefusal::provider_postdecision(
                        error,
                        allocation.receipt,
                    ));
                }
            },
            Err(error) => {
                return Err(CoordinatorRefusal::provider_postdecision(
                    error,
                    allocation.receipt,
                ))
            }
        };
        if state != SecretTransactionState::Committed {
            return Err(CoordinatorRefusal::internal_postdecision(
                CoordinatorError::Invariant("commit did not reach Committed"),
                allocation.receipt,
            ));
        }
        self.set_phase(allocation, "terminal", true)
            .map_err(|error| {
                CoordinatorRefusal::internal_postdecision(error, allocation.receipt)
            })?;
        Ok(allocation.receipt)
    }

    /// Abort/tombstone an allocated id only while no durable decision exists.
    pub async fn abort_before_decision(
        &self,
        allocation: Allocation,
    ) -> Result<(), CoordinatorRefusal> {
        self.require_undecided(allocation)
            .map_err(CoordinatorRefusal::internal_predecision)?;
        let state = self
            .provider
            .abort(allocation.id)
            .await
            .map_err(|error| CoordinatorRefusal::provider(error, DecisionPhase::Before))?;
        if state != SecretTransactionState::Absent {
            return Err(CoordinatorRefusal::internal_predecision(
                CoordinatorError::Invariant("abort did not reach Absent"),
            ));
        }
        let journal = self
            .lock()
            .map_err(CoordinatorRefusal::internal_predecision)?;
        journal
            .execute(
                "DELETE FROM transactions WHERE transaction_id = ?1 AND decided = 0",
                [allocation.id.protocol_bytes().as_slice()],
            )
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        Ok(())
    }

    /// Recover every unresolved row before readiness. Pre-decision work aborts; decisions commit.
    pub async fn recover(&self) -> Result<(), CoordinatorRefusal> {
        let rows = self
            .rows()
            .map_err(CoordinatorRefusal::internal_predecision)?;
        for (allocation, decided, phase) in rows {
            if phase == "terminal" {
                continue;
            }
            if !decided {
                self.abort_before_decision(allocation).await?;
                continue;
            }
            match self.provider.state(allocation.id).await {
                Ok(SecretTransactionState::Committed) => {
                    self.set_phase(allocation, "terminal", true)
                        .map_err(|error| {
                            CoordinatorRefusal::internal_postdecision(error, allocation.receipt)
                        })?;
                }
                Ok(SecretTransactionState::Prepared) => {
                    self.commit(allocation).await?;
                }
                Ok(SecretTransactionState::Absent) => {
                    return Err(CoordinatorRefusal::internal_postdecision(
                        CoordinatorError::Invariant(
                            "provider transaction is Absent after the commit decision",
                        ),
                        allocation.receipt,
                    ));
                }
                Err(error) => {
                    return Err(CoordinatorRefusal::provider_postdecision(
                        error,
                        allocation.receipt,
                    ));
                }
            }
        }
        if let Some(retired) = self
            .retired_generation()
            .map_err(CoordinatorRefusal::internal_predecision)?
        {
            self.provider
                .reclaim(retired)
                .await
                .map_err(|error| CoordinatorRefusal::provider(error, DecisionPhase::Before))?;
        }
        Ok(())
    }

    /// Query a terminal receipt without exposing transaction identity or proposal digest.
    pub fn receipt(&self, receipt: ReceiptId) -> Result<bool, CoordinatorError> {
        let journal = self.lock()?;
        journal
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM transactions WHERE receipt_id = ?1 AND phase = 'terminal')",
                [receipt.protocol_bytes().as_slice()],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|source| self.database(source))
    }

    /// Close every terminal query/replay row through one generation, then retire it at the provider.
    ///
    /// The journal boundary is committed before the provider call. A provider failure is therefore
    /// safely retryable, while receipt and same-proposal lookup can no longer ask about an id that
    /// the provider may already have retired. No clock, count or opaque-id ordering triggers this.
    pub async fn reclaim(
        &self,
        through: SecretTransactionGeneration,
    ) -> Result<(), CoordinatorRefusal> {
        let maximum = u64::from_be_bytes(through.protocol_bytes());
        if !self.close_reclaim_boundary(maximum)? {
            return Ok(());
        }
        self.provider
            .reclaim(through)
            .await
            .map_err(|error| CoordinatorRefusal::provider(error, DecisionPhase::Before))
    }

    fn close_reclaim_boundary(&self, maximum: u64) -> Result<bool, CoordinatorRefusal> {
        let mut journal = self
            .lock()
            .map_err(CoordinatorRefusal::internal_predecision)?;
        let transaction = journal
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        let next: String = transaction
            .query_row(
                "SELECT value FROM coordinator_metadata WHERE key = 'next_generation'",
                [],
                |row| row.get(0),
            )
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        let next = next
            .parse::<u64>()
            .ok()
            .filter(|value| *value != 0)
            .ok_or(CoordinatorError::GenerationCorrupt)
            .map_err(CoordinatorRefusal::internal_predecision)?;
        if maximum >= next {
            return Err(CoordinatorRefusal::internal_predecision(
                CoordinatorError::ReclaimNotAllocated,
            ));
        }
        let retired: String = transaction
            .query_row(
                "SELECT value FROM coordinator_metadata WHERE key = 'retired_generation'",
                [],
                |row| row.get(0),
            )
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        let retired = retired
            .parse::<u64>()
            .map_err(|_| CoordinatorError::GenerationCorrupt)
            .map_err(CoordinatorRefusal::internal_predecision)?;
        if maximum < retired {
            return Ok(false);
        }
        let rows = transaction
            .prepare("SELECT transaction_id, generation, phase, published FROM transactions")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok((
                            row.get::<_, Vec<u8>>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                            row.get::<_, bool>(3)?,
                        ))
                    })?
                    .collect::<Result<Vec<_>, _>>()
            })
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        let mut retiring = Vec::new();
        for (id, generation, phase, published) in rows {
            let generation = generation
                .parse::<u64>()
                .map_err(|_| CoordinatorError::GenerationCorrupt)
                .map_err(CoordinatorRefusal::internal_predecision)?;
            if generation <= maximum {
                if phase != "terminal" || !published {
                    return Err(CoordinatorRefusal::internal_predecision(
                        CoordinatorError::ReclaimStillReferenced,
                    ));
                }
                retiring.push(id);
            }
        }
        for id in retiring {
            transaction
                .execute(
                    "DELETE FROM transactions WHERE transaction_id = ?1 AND phase = 'terminal'",
                    [id],
                )
                .map_err(|source| {
                    CoordinatorRefusal::internal_predecision(self.database(source))
                })?;
        }
        transaction
            .execute(
                "UPDATE coordinator_metadata SET value = ?1 WHERE key = 'retired_generation'",
                [maximum.to_string()],
            )
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        transaction
            .commit()
            .map_err(|source| CoordinatorRefusal::internal_predecision(self.database(source)))?;
        Ok(true)
    }

    fn retired_generation(&self) -> Result<Option<SecretTransactionGeneration>, CoordinatorError> {
        let journal = self.lock()?;
        let value: String = journal
            .query_row(
                "SELECT value FROM coordinator_metadata WHERE key = 'retired_generation'",
                [],
                |row| row.get(0),
            )
            .map_err(|source| self.database(source))?;
        let value = value
            .parse::<u64>()
            .map_err(|_| CoordinatorError::GenerationCorrupt)?;
        if value == 0 {
            return Ok(None);
        }
        SecretTransactionGeneration::from_protocol_bytes(value.to_be_bytes())
            .map(Some)
            .ok_or(CoordinatorError::GenerationCorrupt)
    }

    fn set_phase(
        &self,
        allocation: Allocation,
        phase: &'static str,
        decided: bool,
    ) -> Result<(), CoordinatorError> {
        let journal = self.lock()?;
        let changed = journal
            .execute(
                "UPDATE transactions SET phase = ?1, decided = ?2
                 WHERE transaction_id = ?3 AND proposal_digest = ?4",
                params![
                    phase,
                    decided,
                    allocation.id.protocol_bytes().as_slice(),
                    allocation.proposal.protocol_bytes().as_slice(),
                ],
            )
            .map_err(|source| self.database(source))?;
        if changed == 1 {
            Ok(())
        } else {
            Err(CoordinatorError::UnknownAllocation)
        }
    }

    fn require_phase(
        &self,
        allocation: Allocation,
        admitted: &[&str],
    ) -> Result<(), CoordinatorError> {
        let (phase, _) = self.load_phase(allocation)?;
        if admitted.contains(&phase.as_str()) {
            Ok(())
        } else {
            Err(CoordinatorError::InvalidPhase)
        }
    }

    fn require_decided(&self, allocation: Allocation) -> Result<(), CoordinatorError> {
        let (phase, decided) = self.load_phase(allocation)?;
        if decided && matches!(phase.as_str(), "decided" | "terminal") {
            Ok(())
        } else {
            Err(CoordinatorError::InvalidPhase)
        }
    }

    fn require_undecided(&self, allocation: Allocation) -> Result<(), CoordinatorError> {
        let (_, decided) = self.load_phase(allocation)?;
        if decided {
            Err(CoordinatorError::InvalidPhase)
        } else {
            Ok(())
        }
    }

    fn load_phase(&self, allocation: Allocation) -> Result<(String, bool), CoordinatorError> {
        let journal = self.lock()?;
        journal
            .query_row(
                "SELECT phase, decided FROM transactions
                 WHERE transaction_id = ?1 AND proposal_digest = ?2",
                params![
                    allocation.id.protocol_bytes().as_slice(),
                    allocation.proposal.protocol_bytes().as_slice()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|source| self.database(source))?
            .ok_or(CoordinatorError::UnknownAllocation)
    }

    fn rows(&self) -> Result<Vec<(Allocation, bool, String)>, CoordinatorError> {
        let journal = self.lock()?;
        let mut statement = journal
            .prepare(
                "SELECT transaction_id, generation, proposal_digest, receipt_id, decided, phase
                 FROM transactions ORDER BY decided DESC, rowid",
            )
            .map_err(|source| self.database(source))?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, Vec<u8>>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, Vec<u8>>(3)?,
                    row.get::<_, bool>(4)?,
                    row.get::<_, String>(5)?,
                ))
            })
            .map_err(|source| self.database(source))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|source| self.database(source))?;
        rows.into_iter()
            .map(|(id, generation, proposal, receipt, decided, phase)| {
                let id = decode_transaction(&id)?;
                let generation_value = generation
                    .parse::<u64>()
                    .ok()
                    .filter(|value| *value != 0)
                    .ok_or(CoordinatorError::GenerationCorrupt)?;
                let generation = SecretTransactionGeneration::from_protocol_bytes(
                    generation_value.to_be_bytes(),
                )
                .ok_or(CoordinatorError::GenerationCorrupt)?;
                let proposal = SecretProposalDigest::from_protocol_bytes(array(&proposal)?);
                let receipt = decode_receipt(&receipt)?;
                Ok((
                    Allocation {
                        id,
                        generation,
                        proposal,
                        receipt,
                    },
                    decided,
                    phase,
                ))
            })
            .collect()
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, Connection>, CoordinatorError> {
        self.journal.lock().map_err(|_| CoordinatorError::Poisoned)
    }

    fn database(&self, source: rusqlite::Error) -> CoordinatorError {
        CoordinatorError::Journal {
            path: self.path.clone(),
            source,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DecisionPhase {
    Before,
    After(ReceiptId),
}

/// One exact value-free local-management refusal tuple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CoordinatorRefusal {
    pub code: &'static str,
    pub status: u16,
    pub retry: &'static str,
    pub commit: &'static str,
    pub receipt: Option<ReceiptId>,
}

impl CoordinatorRefusal {
    fn provider(error: PreparedSecretError, phase: DecisionPhase) -> Self {
        match phase {
            DecisionPhase::Before => match error {
                PreparedSecretError::Unsupported => {
                    Self::before("local_management_unavailable", 503, "operator")
                }
                PreparedSecretError::Busy => Self::before("connect_busy", 409, "refresh"),
                PreparedSecretError::DigestMismatch => {
                    Self::before("proposal_conflict", 409, "refresh")
                }
                PreparedSecretError::Capacity | PreparedSecretError::Backend => {
                    Self::before("store_unavailable", 503, "operator")
                }
                PreparedSecretError::TransactionIdReused
                | PreparedSecretError::NotPrepared
                | PreparedSecretError::AlreadyCommitted
                | PreparedSecretError::Retired
                | PreparedSecretError::InvalidBatch => {
                    Self::before("internal_refusal", 500, "operator")
                }
            },
            DecisionPhase::After(receipt) => match error {
                PreparedSecretError::Backend => Self::after("store_unavailable", 503, receipt),
                PreparedSecretError::Unsupported
                | PreparedSecretError::Busy
                | PreparedSecretError::DigestMismatch
                | PreparedSecretError::TransactionIdReused
                | PreparedSecretError::NotPrepared
                | PreparedSecretError::AlreadyCommitted
                | PreparedSecretError::Retired
                | PreparedSecretError::Capacity
                | PreparedSecretError::InvalidBatch => {
                    Self::after("internal_refusal", 500, receipt)
                }
            },
        }
    }

    fn provider_postdecision(error: PreparedSecretError, receipt: ReceiptId) -> Self {
        Self::provider(error, DecisionPhase::After(receipt))
    }

    const fn before(code: &'static str, status: u16, retry: &'static str) -> Self {
        Self {
            code,
            status,
            retry,
            commit: "none",
            receipt: None,
        }
    }

    const fn after(code: &'static str, status: u16, receipt: ReceiptId) -> Self {
        Self {
            code,
            status,
            retry: "same_proposal",
            commit: "query_receipt",
            receipt: Some(receipt),
        }
    }

    fn internal_predecision(_error: CoordinatorError) -> Self {
        Self::before("internal_refusal", 500, "operator")
    }

    fn internal_postdecision(_error: CoordinatorError, receipt: ReceiptId) -> Self {
        Self::after("internal_refusal", 500, receipt)
    }
}

/// Binding/allocation failures never carry secret or proposal bytes in their Display output.
#[derive(Debug, thiserror::Error)]
pub enum CoordinatorError {
    #[error("refusing transaction journal `{path}`: {reason}")]
    UnsafeJournal { path: PathBuf, reason: String },
    #[error("transaction journal `{path}` is unavailable: {source}")]
    Journal {
        path: PathBuf,
        #[source]
        source: rusqlite::Error,
    },
    #[error("transaction identity entropy is unavailable: {0}")]
    Entropy(std::io::Error),
    #[error("transaction generation state is corrupt")]
    GenerationCorrupt,
    #[error("transaction generation is exhausted")]
    GenerationExhausted,
    #[error("transaction or receipt identity collided")]
    IdentityCollision,
    #[error("the transaction allocation is unknown")]
    UnknownAllocation,
    #[error("the transaction is in the wrong coordinator phase")]
    InvalidPhase,
    #[error("transaction journal lock is poisoned")]
    Poisoned,
    #[error("provider reclamation is still referenced by replay or receipt state")]
    ReclaimStillReferenced,
    #[error("provider reclamation cannot pass the latest allocated generation")]
    ReclaimNotAllocated,
    #[error("transaction coordinator invariant failed: {0}")]
    Invariant(&'static str),
}

fn initialise(connection: &Connection, path: &Path) -> Result<(), CoordinatorError> {
    connection
        .execute_batch(
            "PRAGMA journal_mode = PERSIST;
             PRAGMA synchronous = FULL;
             CREATE TABLE IF NOT EXISTS coordinator_metadata (
                 key TEXT PRIMARY KEY,
                 value TEXT NOT NULL
             );
             CREATE TABLE IF NOT EXISTS transactions (
                 transaction_id BLOB PRIMARY KEY CHECK(length(transaction_id) = 32),
                 generation TEXT NOT NULL,
                 nonce BLOB NOT NULL UNIQUE CHECK(length(nonce) = 24),
                 proposal_digest BLOB NOT NULL CHECK(length(proposal_digest) = 32),
                 receipt_id BLOB NOT NULL UNIQUE CHECK(length(receipt_id) = 32),
                 kind TEXT NOT NULL CHECK(kind IN ('connect', 'credential')),
                 tenant TEXT NOT NULL,
                 connector TEXT NOT NULL,
                 label TEXT NOT NULL,
                 phase TEXT NOT NULL CHECK(phase IN ('allocated', 'prepared', 'decided', 'terminal')),
                 decided INTEGER NOT NULL CHECK(decided IN (0, 1)),
                 publication BLOB,
                 published INTEGER NOT NULL DEFAULT 1 CHECK(published IN (0, 1))
             );
             INSERT OR IGNORE INTO coordinator_metadata(key, value)
                 VALUES ('schema', 'exchange.transaction-journal.v1');
             INSERT OR IGNORE INTO coordinator_metadata(key, value)
                 VALUES ('next_generation', '1');
             INSERT OR IGNORE INTO coordinator_metadata(key, value)
                 VALUES ('retired_generation', '0');",
        )
        .map_err(|source| CoordinatorError::Journal {
            path: path.to_path_buf(),
            source,
        })?;
    let columns = connection
        .prepare("PRAGMA table_info(transactions)")
        .and_then(|mut statement| {
            statement
                .query_map([], |row| row.get::<_, String>(1))?
                .collect::<Result<Vec<_>, _>>()
        })
        .map_err(|source| CoordinatorError::Journal {
            path: path.to_path_buf(),
            source,
        })?;
    if !columns.iter().any(|column| column == "tenant") {
        connection
            .execute(
                "ALTER TABLE transactions ADD COLUMN tenant TEXT NOT NULL DEFAULT ''",
                [],
            )
            .map_err(|source| CoordinatorError::Journal {
                path: path.to_path_buf(),
                source,
            })?;
    }
    if !columns.iter().any(|column| column == "publication") {
        connection
            .execute("ALTER TABLE transactions ADD COLUMN publication BLOB", [])
            .map_err(|source| CoordinatorError::Journal {
                path: path.to_path_buf(),
                source,
            })?;
    }
    if !columns.iter().any(|column| column == "published") {
        connection
            .execute(
                "ALTER TABLE transactions ADD COLUMN published INTEGER NOT NULL DEFAULT 1 CHECK(published IN (0, 1))",
                [],
            )
            .map_err(|source| CoordinatorError::Journal {
                path: path.to_path_buf(),
                source,
            })?;
    }
    connection
        .execute_batch(
            "DROP INDEX IF EXISTS transaction_proposal;
             CREATE UNIQUE INDEX IF NOT EXISTS transaction_proposal_v2
                 ON transactions(kind, tenant, connector, label, proposal_digest);",
        )
        .map_err(|source| CoordinatorError::Journal {
            path: path.to_path_buf(),
            source,
        })?;
    let schema: String = connection
        .query_row(
            "SELECT value FROM coordinator_metadata WHERE key = 'schema'",
            [],
            |row| row.get(0),
        )
        .map_err(|source| CoordinatorError::Journal {
            path: path.to_path_buf(),
            source,
        })?;
    if schema != SCHEMA_VERSION {
        return Err(CoordinatorError::GenerationCorrupt);
    }
    Ok(())
}

fn nonzero_receipt() -> Result<ReceiptId, CoordinatorError> {
    for _ in 0..4 {
        let bytes = crate::entropy::bytes::<32>().map_err(CoordinatorError::Entropy)?;
        if let Some(receipt) = ReceiptId::from_protocol_bytes(bytes) {
            return Ok(receipt);
        }
    }
    Err(CoordinatorError::IdentityCollision)
}

fn nonzero_nonce() -> Result<[u8; 24], CoordinatorError> {
    for _ in 0..4 {
        let bytes = crate::entropy::bytes::<24>().map_err(CoordinatorError::Entropy)?;
        if bytes.iter().any(|byte| *byte != 0) {
            return Ok(bytes);
        }
    }
    Err(CoordinatorError::IdentityCollision)
}

fn decode_transaction(bytes: &[u8]) -> Result<SecretTransactionId, CoordinatorError> {
    SecretTransactionId::from_protocol_bytes(array(bytes)?)
        .ok_or(CoordinatorError::GenerationCorrupt)
}

fn decode_receipt(bytes: &[u8]) -> Result<ReceiptId, CoordinatorError> {
    ReceiptId::from_protocol_bytes(array(bytes)?).ok_or(CoordinatorError::IdentityCollision)
}

fn array<const N: usize>(bytes: &[u8]) -> Result<[u8; N], CoordinatorError> {
    bytes
        .try_into()
        .map_err(|_| CoordinatorError::GenerationCorrupt)
}

fn is_constraint(error: &rusqlite::Error) -> bool {
    matches!(
        error,
        rusqlite::Error::SqliteFailure(problem, _)
            if problem.code == rusqlite::ErrorCode::ConstraintViolation
    )
}

fn sqlite_journal_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy())
        .unwrap_or_default();
    path.with_file_name(format!("{name}-journal"))
}

#[cfg(test)]
mod tests {
    use super::*;

    use exchange_host::{
        async_trait, CredentialRef, CredentialScope, CredentialStore, Secret, SecretStore,
        StoreError,
    };
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{Duration, Instant};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "flux-exchange-x134-coordinator-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            exchange_host::ensure_private_state_directory(&path).expect("private test root");
            Self(path)
        }

        fn journal(&self) -> PathBuf {
            self.0.join("coordinator/transactions.sqlite3")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn digest(byte: u8) -> SecretProposalDigest {
        SecretProposalDigest::from_protocol_bytes([byte; 32])
    }

    fn reference() -> CredentialRef {
        CredentialRef::new("local", "com.example.api", "primary", "token").expect("reference")
    }

    fn batch(value: &str) -> SecretBatch {
        let reference = reference();
        let mut batch = SecretBatch::new(
            CredentialScope::new(reference.tenant(), reference.authority()).expect("scope"),
        );
        batch
            .put(reference, Secret::new(value))
            .expect("scoped mutation");
        batch
    }

    fn fixed_allocation(
        coordinator: &TransactionCoordinator,
        proposal: SecretProposalDigest,
        nonce: u8,
        receipt: u8,
    ) -> Allocation {
        coordinator
            .allocate_with(
                TransactionKind::Connect,
                "",
                "example",
                "primary",
                proposal,
                [nonce; 24],
                ReceiptId::from_protocol_bytes([receipt; 32]).expect("nonzero receipt"),
            )
            .expect("allocation")
    }

    fn stores(
        scratch: &Scratch,
    ) -> (
        Arc<dyn SecretStore>,
        Arc<dyn PreparedSecretStore>,
        CredentialStore,
    ) {
        let bound = CredentialStore::bind(scratch.0.join("credentials/store"))
            .expect("one concrete credential store");
        (bound.secrets(), bound.prepared_secrets(), bound)
    }

    struct BackendOnce {
        ordinary: Arc<dyn SecretStore>,
        prepared: Arc<dyn PreparedSecretStore>,
        prepare_failures: AtomicU64,
        commit_failures: AtomicU64,
    }

    impl BackendOnce {
        fn new(
            ordinary: Arc<dyn SecretStore>,
            prepared: Arc<dyn PreparedSecretStore>,
            prepare_failures: u64,
            commit_failures: u64,
        ) -> Self {
            Self {
                ordinary,
                prepared,
                prepare_failures: AtomicU64::new(prepare_failures),
                commit_failures: AtomicU64::new(commit_failures),
            }
        }

        fn fail_once(counter: &AtomicU64) -> bool {
            counter
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                    value.checked_sub(1)
                })
                .is_ok()
        }
    }

    #[async_trait]
    impl SecretStore for BackendOnce {
        async fn get(&self, reference: &CredentialRef) -> Result<Secret, StoreError> {
            self.ordinary.get(reference).await
        }

        async fn put(&self, reference: &CredentialRef, secret: &Secret) -> Result<(), StoreError> {
            self.ordinary.put(reference, secret).await
        }

        async fn delete(&self, reference: &CredentialRef) -> Result<(), StoreError> {
            self.ordinary.delete(reference).await
        }

        async fn references(
            &self,
            scope: &CredentialScope,
        ) -> Result<Vec<CredentialRef>, StoreError> {
            self.ordinary.references(scope).await
        }

        async fn apply(&self, batch: &SecretBatch) -> Result<(), StoreError> {
            self.ordinary.apply(batch).await
        }
    }

    #[async_trait]
    impl PreparedSecretStore for BackendOnce {
        async fn prepare(
            &self,
            id: SecretTransactionId,
            proposal: SecretProposalDigest,
            batch: &SecretBatch,
        ) -> Result<SecretTransactionState, PreparedSecretError> {
            if Self::fail_once(&self.prepare_failures) {
                Err(PreparedSecretError::Backend)
            } else {
                self.prepared.prepare(id, proposal, batch).await
            }
        }

        async fn state(
            &self,
            id: SecretTransactionId,
        ) -> Result<SecretTransactionState, PreparedSecretError> {
            self.prepared.state(id).await
        }

        async fn commit(
            &self,
            id: SecretTransactionId,
        ) -> Result<SecretTransactionState, PreparedSecretError> {
            if Self::fail_once(&self.commit_failures) {
                Err(PreparedSecretError::Backend)
            } else {
                self.prepared.commit(id).await
            }
        }

        async fn abort(
            &self,
            id: SecretTransactionId,
        ) -> Result<SecretTransactionState, PreparedSecretError> {
            self.prepared.abort(id).await
        }

        async fn reclaim(
            &self,
            through: SecretTransactionGeneration,
        ) -> Result<(), PreparedSecretError> {
            self.prepared.reclaim(through).await
        }
    }

    #[tokio::test]
    async fn commit_decision_recovers_to_one_provider_commit_and_queryable_receipt() {
        let scratch = Scratch::new();
        let (ordinary, prepared, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), prepared.clone()).expect("coordinator");
        let allocation = fixed_allocation(&coordinator, digest(1), 1, 2);
        assert_eq!(
            coordinator
                .prepare(allocation, &batch("coordinator-test-sentinel"))
                .await
                .expect("prepare"),
            SecretTransactionState::Prepared
        );
        coordinator.decide_commit(allocation).expect("decision");
        drop(coordinator);

        let restarted = TransactionCoordinator::bind(scratch.journal(), prepared.clone())
            .expect("restarted coordinator");
        restarted.recover().await.expect("roll forward");
        assert!(restarted.receipt(allocation.receipt_id()).expect("receipt"));
        assert!(ordinary.get(&reference()).await.is_ok());
        assert_eq!(
            restarted
                .proposal_state(TransactionKind::Connect, "example", "primary", digest(1))
                .expect("proposal lookup"),
            Some(ProposalState::Committed(allocation.receipt_id()))
        );
        assert_journal_excludes(&scratch, b"coordinator-test-sentinel");
    }

    #[tokio::test]
    async fn recovery_rolls_decisions_forward_before_aborting_older_allocations() {
        let scratch = Scratch::new();
        let (ordinary, prepared, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), prepared.clone()).expect("coordinator");
        let older = fixed_allocation(&coordinator, digest(21), 21, 22);
        let decided = fixed_allocation(&coordinator, digest(23), 23, 24);
        coordinator
            .prepare(decided, &batch("decision-first-sentinel"))
            .await
            .expect("newer provider prepare");
        coordinator.decide_commit(decided).expect("newer decision");
        drop(coordinator);

        let restarted = TransactionCoordinator::bind(scratch.journal(), prepared.clone())
            .expect("restarted coordinator");
        restarted
            .recover()
            .await
            .expect("decision before cross-id abort");
        assert_eq!(
            prepared.state(decided.transaction_id()).await,
            Ok(SecretTransactionState::Committed)
        );
        assert_eq!(
            prepared.state(older.transaction_id()).await,
            Ok(SecretTransactionState::Absent)
        );
        assert!(ordinary.get(&reference()).await.is_ok());
    }

    #[tokio::test]
    async fn backend_resolution_retries_absent_prepare_and_prepared_commit() {
        let scratch = Scratch::new();
        let (ordinary, prepared, _bound) = stores(&scratch);
        let faulting: Arc<dyn PreparedSecretStore> =
            Arc::new(BackendOnce::new(ordinary.clone(), prepared.clone(), 1, 1));
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), faulting).expect("coordinator");
        let allocation = fixed_allocation(&coordinator, digest(25), 25, 26);
        assert_eq!(
            coordinator
                .prepare(allocation, &batch("backend-retry-sentinel"))
                .await,
            Ok(SecretTransactionState::Prepared)
        );
        coordinator.decide_commit(allocation).expect("decision");
        assert_eq!(
            coordinator.commit(allocation).await,
            Ok(allocation.receipt_id())
        );
        assert_eq!(
            prepared.state(allocation.transaction_id()).await,
            Ok(SecretTransactionState::Committed)
        );
    }

    #[tokio::test]
    async fn terminal_publication_gates_its_tenant_connector_until_closed() {
        let scratch = Scratch::new();
        let (_ordinary, prepared, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), prepared).expect("coordinator");
        let allocation = coordinator
            .allocate_with(
                TransactionKind::Connect,
                "acme",
                "example",
                "primary",
                digest(27),
                [27; 24],
                ReceiptId::from_protocol_bytes([28; 32]).expect("receipt"),
            )
            .expect("allocation");
        coordinator
            .attach_publication(allocation, br#"{"schema":"test"}"#)
            .expect("publication image");
        coordinator
            .prepare(allocation, &batch("pending-publication-sentinel"))
            .await
            .expect("provider prepare");
        coordinator.decide_commit(allocation).expect("decision");
        let receipt = coordinator
            .commit(allocation)
            .await
            .expect("provider commit");

        assert!(coordinator
            .publication_pending_for("acme", "example")
            .expect("pending query"));
        assert!(!coordinator
            .publication_pending_for("other", "example")
            .expect("tenant isolation"));
        assert!(!coordinator
            .publication_pending_for("acme", "other")
            .expect("connector isolation"));
        coordinator
            .mark_published(receipt)
            .expect("close publication");
        assert!(!coordinator
            .publication_pending_for("acme", "example")
            .expect("closed query"));
    }

    #[tokio::test]
    async fn predecision_recovery_aborts_provider_staging_and_removes_the_journal_row() {
        let scratch = Scratch::new();
        let (ordinary, prepared, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), prepared.clone()).expect("coordinator");
        let allocation = fixed_allocation(&coordinator, digest(2), 3, 4);
        coordinator
            .prepare(allocation, &batch("never-published-sentinel"))
            .await
            .expect("prepare");
        drop(coordinator);

        let restarted = TransactionCoordinator::bind(scratch.journal(), prepared.clone())
            .expect("restarted coordinator");
        restarted.recover().await.expect("abort recovery");
        assert_eq!(
            prepared.state(allocation.transaction_id()).await,
            Ok(SecretTransactionState::Absent)
        );
        assert_eq!(
            restarted
                .proposal_state(TransactionKind::Connect, "example", "primary", digest(2))
                .expect("proposal lookup"),
            None
        );
        assert!(ordinary.get(&reference()).await.is_err());
    }

    #[test]
    fn generation_and_nonce_allocation_are_nonzero_unique_and_restart_stable() {
        let scratch = Scratch::new();
        let (_ordinary, provider, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), provider.clone()).expect("coordinator");
        let first = fixed_allocation(&coordinator, digest(3), 5, 6);
        drop(coordinator);
        let restarted = TransactionCoordinator::bind(scratch.journal(), provider)
            .expect("restarted coordinator");
        let second = fixed_allocation(&restarted, digest(4), 7, 8);
        assert_ne!(
            first.transaction_id().protocol_bytes(),
            second.transaction_id().protocol_bytes()
        );
        assert_eq!(
            &first.transaction_id().protocol_bytes()[..8],
            &1_u64.to_be_bytes()
        );
        assert_eq!(
            &second.transaction_id().protocol_bytes()[..8],
            &2_u64.to_be_bytes()
        );
        assert!(matches!(
            restarted.allocate_with(
                TransactionKind::Connect,
                "",
                "example",
                "zero-nonce",
                digest(31),
                [0; 24],
                ReceiptId::from_protocol_bytes([31; 32]).expect("receipt"),
            ),
            Err(CoordinatorError::IdentityCollision)
        ));
    }

    #[test]
    fn provider_errors_map_exhaustively_to_closed_pre_and_post_decision_tuples() {
        let receipt = ReceiptId::from_protocol_bytes([9; 32]).expect("receipt");
        let all = [
            PreparedSecretError::Unsupported,
            PreparedSecretError::Busy,
            PreparedSecretError::DigestMismatch,
            PreparedSecretError::TransactionIdReused,
            PreparedSecretError::NotPrepared,
            PreparedSecretError::AlreadyCommitted,
            PreparedSecretError::Retired,
            PreparedSecretError::Capacity,
            PreparedSecretError::InvalidBatch,
            PreparedSecretError::Backend,
        ];
        for error in all {
            let before = CoordinatorRefusal::provider(error, DecisionPhase::Before);
            assert_eq!(before.commit, "none");
            assert!(before.receipt.is_none());
            assert!(matches!(before.retry, "operator" | "refresh"));
            let after = CoordinatorRefusal::provider(error, DecisionPhase::After(receipt));
            assert_eq!(after.commit, "query_receipt");
            assert_eq!(after.retry, "same_proposal");
            assert_eq!(after.receipt, Some(receipt));
        }
        for error in [
            PreparedSecretError::NotPrepared,
            PreparedSecretError::Retired,
        ] {
            let after = CoordinatorRefusal::provider(error, DecisionPhase::After(receipt));
            assert_eq!(after.code, "internal_refusal");
            assert_eq!(after.status, 500);
            assert_eq!(after.commit, "query_receipt");
            assert_eq!(after.retry, "same_proposal");
        }
    }

    #[tokio::test]
    async fn an_empty_batch_is_prepared_and_committed_without_secret_presence_logic() {
        let scratch = Scratch::new();
        let (_ordinary, provider, _bound) = stores(&scratch);
        let coordinator =
            TransactionCoordinator::bind(scratch.journal(), provider.clone()).expect("coordinator");
        let allocation = fixed_allocation(&coordinator, digest(5), 10, 11);
        let empty =
            SecretBatch::new(CredentialScope::new("local", "com.example.api").expect("scope"));
        let premature = coordinator
            .reclaim(allocation.generation)
            .await
            .expect_err("an allocated replay row is not a retirement boundary");
        assert_eq!(premature.code, "internal_refusal");
        assert_eq!(premature.commit, "none");
        assert_eq!(
            coordinator
                .prepare(allocation, &empty)
                .await
                .expect("empty prepare"),
            SecretTransactionState::Prepared
        );
        coordinator.decide_commit(allocation).expect("decision");
        assert_eq!(
            coordinator.commit(allocation).await.expect("commit"),
            allocation.receipt_id()
        );
        coordinator
            .reclaim(allocation.generation)
            .await
            .expect("explicit terminal closure and provider reclaim");
        assert!(!coordinator
            .receipt(allocation.receipt_id())
            .expect("closed receipt"));
        assert_eq!(
            coordinator
                .proposal_state(TransactionKind::Connect, "example", "primary", digest(5))
                .expect("closed replay lookup"),
            None
        );
        assert_eq!(
            provider.state(allocation.transaction_id()).await,
            Err(PreparedSecretError::Retired)
        );
    }

    fn assert_journal_excludes(scratch: &Scratch, needle: &[u8]) {
        let directory = scratch.0.join("coordinator");
        for entry in std::fs::read_dir(&directory).expect("coordinator directory") {
            let path = entry.expect("journal entry").path();
            if path.is_file() {
                let bytes = std::fs::read(&path).expect("read value-free journal artifact");
                assert!(
                    !bytes.windows(needle.len()).any(|window| window == needle),
                    "credential bytes reached coordinator artifact {}",
                    path.display()
                );
            }
        }
    }

    #[test]
    fn legacy_writer_child() {
        if std::env::var_os("FLUX_EXCHANGE_LEGACY_WRITER_CHILD").is_none() {
            return;
        }
        let store = PathBuf::from(
            std::env::var_os("FLUX_EXCHANGE_LEGACY_WRITER_STORE").expect("legacy store"),
        );
        let ready = PathBuf::from(
            std::env::var_os("FLUX_EXCHANGE_LEGACY_WRITER_READY").expect("ready path"),
        );
        let release = PathBuf::from(
            std::env::var_os("FLUX_EXCHANGE_LEGACY_WRITER_RELEASE").expect("release path"),
        );
        let opened = std::fs::read(&store).expect("released 0.19 writer opened v1 bytes");
        std::fs::write(&ready, b"ready").expect("signal legacy open");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !release.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(release.exists(), "legacy writer was not released");
        std::fs::write(store, opened).expect("released 0.19 whole-image rewrite");
    }

    #[test]
    fn released_019_writer_exits_before_the_first_020_store_open() {
        let scratch = Scratch::new();
        let store = scratch.0.join("legacy/credentials");
        exchange_host::ensure_private_state_file(&store).expect("private legacy store");
        std::fs::write(&store, b"# codewandler-connector-secrets file store, v1\n")
            .expect("released v1 fixture");
        let ready = scratch.0.join("legacy.ready");
        let release = scratch.0.join("legacy.release");
        let mut legacy = Command::new(std::env::current_exe().expect("test executable"))
            .arg("--exact")
            .arg("local_management::transaction::tests::legacy_writer_child")
            .arg("--nocapture")
            .env("FLUX_EXCHANGE_LEGACY_WRITER_CHILD", "1")
            .env("FLUX_EXCHANGE_LEGACY_WRITER_STORE", &store)
            .env("FLUX_EXCHANGE_LEGACY_WRITER_READY", &ready)
            .env("FLUX_EXCHANGE_LEGACY_WRITER_RELEASE", &release)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn released 0.19 writer fixture");
        let deadline = Instant::now() + Duration::from_secs(10);
        while !ready.exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(ready.exists(), "legacy writer did not open v1");

        std::fs::write(&release, b"quiesce").expect("release legacy writer");
        assert!(
            legacy.wait().expect("wait for legacy writer").success(),
            "legacy writer must be reaped before 0.20 opens"
        );
        let current = crate::credential_store(Some(&store))
            .expect("production credential binding after quiescence")
            .expect("configured production credential binding");
        let generation =
            SecretTransactionGeneration::from_protocol_bytes(1_u64.to_be_bytes()).expect("gen");
        let id = SecretTransactionId::new(generation, [1; 24]);
        assert_eq!(
            block_on(current.prepared.abort(id)),
            Ok(SecretTransactionState::Absent)
        );
        let contending = CredentialStore::bind(&store)
            .expect_err("the production ordinary/prepared ports retain the 0.20 lease");
        assert!(contending.to_string().contains("lease"), "{contending}");
        assert!(block_on(current.ordinary.get(&reference())).is_err());
        let migrated = std::fs::read_to_string(&store).expect("migrated store");
        assert!(migrated.starts_with("# codewandler-connector-secrets file store, v2\n"));
        drop(current);
        CredentialStore::bind(&store).expect("the final production port releases the lease");
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime")
            .block_on(future)
    }
}
