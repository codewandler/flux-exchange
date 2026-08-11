//! Transaction-backed connect and credential FXLM ceremonies.
//!
//! Secret bytes live only in the active ceremony and the provider-owned [`SecretBatch`]. Durable
//! Exchange state contains the opaque provider/proposal identities plus a public metadata
//! roll-forward image; transaction ids, ordinals and credential bytes never enter [`AppState`].

use std::sync::Arc;

use exchange_host::{
    ConnectionLabel, ConnectorDeclaration, CredentialRef, CredentialScope, DeclaredCredential,
    DeclaredSetting, InstanceId, Secret, SecretBatch, SecretProposalDigest, Tenant,
    TenantInstances,
};
use serde::{Deserialize, Serialize};

use super::codec::{Direction, Frame, Opcode};
use super::deadline::{DeadlineController, ReceiptIdentity, Unresolved};
use super::proposal::{ConnectBegin, CredentialAction, CredentialBegin, ProposalError, TargetFact};
use super::transaction::{
    Allocation, CoordinatorRefusal, ProposalState, ReceiptId, TransactionCoordinator,
    TransactionKind,
};
use crate::audit::{Action, Outcome, RequestId, Target};
use crate::connection_guard::Claim;
use crate::credential_head::{CredentialHead, CredentialHeadKey};
use crate::state::AppState;

const PUBLICATION_SCHEMA: &str = "exchange.local-management-publication.v1";
#[cfg(feature = "native-root-test-seam")]
const PUBLICATION_CRASH_AFTER_ENV: &str = "FLUX_EXCHANGE_TEST_PUBLICATION_CRASH_AFTER";
#[cfg(feature = "native-root-test-seam")]
const PUBLICATION_FAIL_AFTER_ENV: &str = "FLUX_EXCHANGE_TEST_PUBLICATION_FAIL_AFTER";
#[cfg(feature = "native-root-test-seam")]
static PUBLICATION_FAILURE_INJECTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Shared production entry point for a single connection or credential operation.
#[derive(Clone)]
pub(super) struct Ceremony {
    state: AppState,
    coordinator: Arc<TransactionCoordinator>,
    #[cfg(test)]
    cancellation_pause: Option<Arc<CancellationPause>>,
}

impl Ceremony {
    pub(super) const fn new(state: AppState, coordinator: Arc<TransactionCoordinator>) -> Self {
        Self {
            state,
            coordinator,
            #[cfg(test)]
            cancellation_pause: None,
        }
    }

    #[cfg(test)]
    fn with_cancellation_pause(
        state: AppState,
        coordinator: Arc<TransactionCoordinator>,
        pause: Arc<CancellationPause>,
    ) -> Self {
        Self {
            state,
            coordinator,
            cancellation_pause: Some(pause),
        }
    }

    /// Begin one mutation after transport authentication, or return a terminal replay/refusal.
    pub(super) async fn begin(
        &self,
        tenant: &Tenant,
        request: Frame,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        match request.opcode() {
            Opcode::ConnectBegin => self.begin_connect(tenant, request, deadline).await,
            Opcode::CredentialBegin => self.begin_credential(tenant, request, deadline).await,
            Opcode::ConnectQuery => {
                self.query(tenant, request, Opcode::ConnectReceipt, deadline)
                    .await
            }
            Opcode::CredentialQuery => {
                self.query(tenant, request, Opcode::CredentialReceipt, deadline)
                    .await
            }
            _ => BeginOutcome::Terminal(refusal("unexpected_frame", 409, "never")),
        }
    }

    /// Drain every provider-terminal public image before readiness/routes.
    pub(super) fn recover(&self) -> Result<(), PublicationRefusal> {
        for pending in self
            .coordinator
            .pending_publications()
            .map_err(|_| PublicationRefusal::Store)?
        {
            let publication = parse_publication(pending.bytes())?;
            apply_publication(&self.state, pending.receipt_id(), &publication)?;
            self.coordinator
                .mark_published(pending.receipt_id())
                .map_err(|_| PublicationRefusal::Store)?;
        }
        Ok(())
    }

    async fn begin_connect(
        &self,
        tenant: &Tenant,
        request: Frame,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        let Some(payload) = request.control_payload() else {
            return BeginOutcome::Terminal(refusal("invalid_request", 400, "never"));
        };
        let begin = match ConnectBegin::parse_canonical(payload) {
            Ok(begin) => begin,
            Err(_) => return BeginOutcome::Terminal(refusal("invalid_request", 400, "never")),
        };
        let digest = begin.proposal_digest();
        let provider_digest = protocol_digest(&digest);
        match self.coordinator.proposal_state_for_tenant(
            TransactionKind::Connect,
            tenant.as_str(),
            begin.connector(),
            begin.label(),
            provider_digest,
        ) {
            Ok(Some(ProposalState::Committed(receipt))) => {
                return self.replay(receipt, true, deadline).await;
            }
            Ok(Some(ProposalState::Active)) => {
                return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
            }
            Ok(None) => {}
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }
        match self
            .coordinator
            .publication_pending_for(tenant.as_str(), begin.connector())
        {
            Ok(false) => {}
            Ok(true) => {
                return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
            }
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }

        // An exact committed replay returned above. Any other proposal for a label that already
        // exists is a value-free conflict regardless of whether its embedded plan revision has
        // since become stale; validating current plan bytes first would make the same durable
        // conflict depend on unrelated later catalogue state.
        let label = match ConnectionLabel::new(begin.label()) {
            Ok(label) => label,
            Err(_) => return BeginOutcome::Terminal(refusal("invalid_label", 422, "never")),
        };
        let Some(registry) = self.state.connection_registry() else {
            return BeginOutcome::Terminal(refusal(
                "local_management_unavailable",
                503,
                "operator",
            ));
        };
        match registry.resolve(tenant, begin.connector(), &label) {
            Ok(Some(_)) => {
                return BeginOutcome::Terminal(refusal("proposal_conflict", 409, "refresh"));
            }
            Ok(None) => {}
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }

        let snapshot =
            match crate::routes::native_plan_snapshot(&self.state, tenant, begin.connector(), None)
            {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    return BeginOutcome::Terminal(refusal(error.code, error.status, error.retry));
                }
            };
        if begin.plan_revision() != snapshot.plan_revision {
            return BeginOutcome::Terminal(refusal("stale_plan", 409, "refresh"));
        }
        if let Err(error) = begin.validate_target_closure(&target_facts(&snapshot)) {
            return BeginOutcome::Terminal(proposal_refusal(&begin, &snapshot, error));
        }

        let Some(claim) = self.state.claim_connection(tenant, begin.connector()) else {
            return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
        };
        let Some(tenant_claim) = self.state.connections().claim_tenant(tenant) else {
            return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
        };
        let instance = match mint_instance() {
            Ok(instance) => instance,
            Err(()) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        };
        let key = match CredentialHeadKey::new(tenant.as_str(), begin.connector(), begin.label()) {
            Ok(key) => key,
            Err(_) => return BeginOutcome::Terminal(refusal("invalid_request", 400, "never")),
        };
        let Some(heads) = self.state.credential_heads() else {
            return BeginOutcome::Terminal(refusal(
                "local_management_unavailable",
                503,
                "operator",
            ));
        };
        let next_head = match heads.allocate_new(&key) {
            Ok(head) => head,
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        };
        let targets = match secret_targets(
            &self.state,
            tenant,
            begin.connector(),
            Some(&instance),
            true,
            begin.targets().iter().map(|target| target.target()),
        )
        .await
        {
            Ok(targets) => targets,
            Err(frame) => return BeginOutcome::Terminal(frame),
        };
        let publication = Publication {
            action: PublicationAction::Connect,
            connector: begin.connector().to_owned(),
            expected_head: None,
            instance: instance.to_string(),
            label: begin.label().to_owned(),
            next_head: next_head.as_str().to_owned(),
            schema: PUBLICATION_SCHEMA.to_owned(),
            settings: begin
                .settings()
                .iter()
                .map(|setting| PublishedSetting {
                    authority: begin
                        .authorities()
                        .iter()
                        .any(|authority| authority.target() == setting.target()),
                    target: setting.target().to_owned(),
                    value: setting.value().to_owned(),
                })
                .collect(),
            tenant: tenant.as_str().to_owned(),
        };
        self.allocate(
            tenant,
            TransactionKind::Connect,
            provider_digest,
            publication,
            targets,
            Some(claim),
            Some(tenant_claim),
            deadline,
        )
        .await
    }

    async fn begin_credential(
        &self,
        tenant: &Tenant,
        request: Frame,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        let Some(payload) = request.control_payload() else {
            return BeginOutcome::Terminal(refusal("invalid_request", 400, "never"));
        };
        let begin = match CredentialBegin::parse_canonical(payload) {
            Ok(begin) => begin,
            Err(_) => return BeginOutcome::Terminal(refusal("invalid_request", 400, "never")),
        };
        let digest = begin.proposal_digest();
        let provider_digest = protocol_digest(&digest);
        match self.coordinator.proposal_state_for_tenant(
            TransactionKind::Credential,
            tenant.as_str(),
            begin.connector(),
            begin.label(),
            provider_digest,
        ) {
            Ok(Some(ProposalState::Committed(receipt))) => {
                return self.replay(receipt, true, deadline).await;
            }
            Ok(Some(ProposalState::Active)) => {
                return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
            }
            Ok(None) => {}
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }
        match self
            .coordinator
            .publication_pending_for(tenant.as_str(), begin.connector())
        {
            Ok(false) => {}
            Ok(true) => {
                return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
            }
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }
        let snapshot = match crate::routes::native_plan_snapshot(
            &self.state,
            tenant,
            begin.connector(),
            Some(begin.label()),
        ) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return BeginOutcome::Terminal(refusal(error.code, error.status, error.retry));
            }
        };
        if begin.plan_revision() != snapshot.plan_revision {
            return BeginOutcome::Terminal(refusal("stale_plan", 409, "refresh"));
        }
        if let Err(error) = begin.validate_target_closure(&target_facts(&snapshot)) {
            return BeginOutcome::Terminal(proposal_refusal_credential(&begin, &snapshot, error));
        }
        if snapshot.credential_revision.as_deref() != Some(begin.credential_revision()) {
            return BeginOutcome::Terminal(refusal("stale_credential_revision", 409, "refresh"));
        }
        let Some(claim) = self.state.claim_connection(tenant, begin.connector()) else {
            return BeginOutcome::Terminal(refusal("connect_busy", 409, "refresh"));
        };
        let key = match CredentialHeadKey::new(tenant.as_str(), begin.connector(), begin.label()) {
            Ok(key) => key,
            Err(_) => return BeginOutcome::Terminal(refusal("invalid_request", 400, "never")),
        };
        let Some(heads) = self.state.credential_heads() else {
            return BeginOutcome::Terminal(refusal(
                "local_management_unavailable",
                503,
                "operator",
            ));
        };
        let expected = match heads.current(&key) {
            Ok(head) => head,
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        };
        let next = match heads.allocate_next(&key) {
            Ok(head) => head,
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        };
        let selected_instance =
            match resolved_instance(&self.state, tenant, begin.connector(), begin.label()) {
                Some(instance) => instance,
                None => return BeginOutcome::Terminal(refusal("unknown_label", 404, "refresh")),
            };
        let targets = match secret_targets(
            &self.state,
            tenant,
            begin.connector(),
            Some(&selected_instance),
            false,
            begin.targets().iter().map(|target| target.target()),
        )
        .await
        {
            Ok(targets) => targets,
            Err(frame) => return BeginOutcome::Terminal(frame),
        };
        match credential_state_admits(&self.state, &targets, begin.action()).await {
            Ok(true) => {}
            Ok(false) => {
                return BeginOutcome::Terminal(refusal(
                    "credential_state_conflict",
                    409,
                    "refresh",
                ));
            }
            Err(()) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        }
        let publication = Publication {
            action: match begin.action() {
                CredentialAction::Acquire => PublicationAction::Acquire,
                CredentialAction::Rotate => PublicationAction::Rotate,
            },
            connector: begin.connector().to_owned(),
            expected_head: Some(expected.as_str().to_owned()),
            instance: selected_instance.to_string(),
            label: begin.label().to_owned(),
            next_head: next.as_str().to_owned(),
            schema: PUBLICATION_SCHEMA.to_owned(),
            settings: Vec::new(),
            tenant: tenant.as_str().to_owned(),
        };
        self.allocate(
            tenant,
            TransactionKind::Credential,
            provider_digest,
            publication,
            targets,
            Some(claim),
            None,
            deadline,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn allocate(
        &self,
        tenant: &Tenant,
        kind: TransactionKind,
        proposal: SecretProposalDigest,
        publication: Publication,
        targets: SecretTargets,
        claim: Option<Claim>,
        tenant_claim: Option<Claim>,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        let allocation = match self.coordinator.allocate_for_tenant(
            kind,
            tenant.as_str(),
            &publication.connector,
            &publication.label,
            proposal,
        ) {
            Ok(allocation) => allocation,
            Err(_) => {
                return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
            }
        };
        let mut cancellation =
            CeremonyCancellation::new(allocation, self.coordinator.clone(), deadline.clone());
        let publication_bytes = match serde_json::to_vec(&publication) {
            Ok(bytes) => bytes,
            Err(_) => {
                cancellation.abort().await;
                return BeginOutcome::Terminal(refusal("internal_refusal", 500, "operator"));
            }
        };
        if self
            .coordinator
            .attach_publication(allocation, &publication_bytes)
            .is_err()
        {
            cancellation.abort().await;
            return BeginOutcome::Terminal(refusal("store_unavailable", 503, "operator"));
        }
        #[cfg(test)]
        if let Some(pause) = &self.cancellation_pause {
            pause.at(CancellationPoint::Begin).await;
        }
        let mut active = ActiveCeremony {
            allocation,
            batch: targets.batch,
            coordinator: self.coordinator.clone(),
            expected: targets.needs,
            next_ordinal: 1,
            prepared: false,
            publication,
            state: self.state.clone(),
            cancellation,
            #[cfg(test)]
            cancellation_pause: self.cancellation_pause.clone(),
            _claim: claim,
            _tenant_claim: tenant_claim,
        };
        if active.expected.is_empty() {
            if let Err(error) = active
                .coordinator
                .prepare(active.allocation, &active.batch)
                .await
            {
                active.abort().await;
                return BeginOutcome::Terminal(coordinator_refusal(error));
            }
            active.prepared = true;
            #[cfg(test)]
            if let Some(pause) = &active.cancellation_pause {
                pause.at(CancellationPoint::Prepare).await;
            }
        }
        let response = need_secrets(active.allocation, &active.expected);
        BeginOutcome::Active {
            response,
            active: Box::new(active),
        }
    }

    async fn query(
        &self,
        tenant: &Tenant,
        request: Frame,
        opcode: Opcode,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        let Some(payload) = request.control_payload() else {
            return BeginOutcome::Terminal(refusal("invalid_request", 400, "never"));
        };
        let query: ReceiptQuery = match canonical_control(payload) {
            Ok(query) => query,
            Err(()) => return BeginOutcome::Terminal(refusal("invalid_request", 400, "never")),
        };
        let Some(receipt) =
            decode_identity(&query.receipt_id).and_then(ReceiptId::from_protocol_bytes)
        else {
            return BeginOutcome::Terminal(refusal("invalid_request", 400, "never"));
        };
        match self.replay_frame(tenant, receipt, true, opcode, deadline) {
            Ok(frame) => BeginOutcome::Terminal(frame),
            Err(frame) => BeginOutcome::Terminal(frame),
        }
    }

    async fn replay(
        &self,
        receipt: ReceiptId,
        replayed: bool,
        deadline: &DeadlineController,
    ) -> BeginOutcome {
        if deadline
            .decided(receipt_identity(receipt), Unresolved::Internal)
            .is_err()
        {
            return BeginOutcome::Terminal(post_refusal("internal_refusal", 500, receipt));
        }
        let publication = match self.coordinator.publication(receipt) {
            Ok(Some(bytes)) => match parse_publication(&bytes) {
                Ok(publication) => publication,
                Err(_) => {
                    deadline.unresolved(Unresolved::Internal);
                    return BeginOutcome::Terminal(post_refusal("internal_refusal", 500, receipt));
                }
            },
            _ => {
                deadline.unresolved(Unresolved::Internal);
                return BeginOutcome::Terminal(post_refusal("internal_refusal", 500, receipt));
            }
        };
        match self.coordinator.publication_is_complete(receipt) {
            Ok(true) => {}
            Ok(false) => {
                if let Err(error) = apply_publication(&self.state, receipt, &publication) {
                    deadline.unresolved(publication_unresolved(error));
                    return BeginOutcome::Terminal(publication_refusal(error, receipt));
                }
                if self.coordinator.mark_published(receipt).is_err() {
                    deadline.unresolved(Unresolved::Store);
                    return BeginOutcome::Terminal(post_refusal("store_unavailable", 503, receipt));
                }
            }
            Err(_) => {
                deadline.unresolved(Unresolved::Store);
                return BeginOutcome::Terminal(post_refusal("store_unavailable", 503, receipt));
            }
        }
        deadline.terminal();
        BeginOutcome::Terminal(receipt_frame(&publication, receipt, replayed))
    }

    fn replay_frame(
        &self,
        tenant: &Tenant,
        receipt: ReceiptId,
        replayed: bool,
        expected: Opcode,
        deadline: &DeadlineController,
    ) -> Result<Frame, Frame> {
        let bytes = self
            .coordinator
            .publication(receipt)
            .map_err(|_| refusal("store_unavailable", 503, "operator"))?
            .ok_or_else(|| refusal("invalid_request", 400, "never"))?;
        if deadline
            .decided(receipt_identity(receipt), Unresolved::Internal)
            .is_err()
        {
            return Err(post_refusal("internal_refusal", 500, receipt));
        }
        let publication = parse_publication(&bytes)
            .map_err(|_| post_refusal("internal_refusal", 500, receipt))?;
        if publication.tenant != tenant.as_str() {
            return Err(refusal("invalid_request", 400, "never"));
        }
        let actual = publication.receipt_opcode();
        if actual != expected {
            return Err(refusal("invalid_request", 400, "never"));
        }
        match self
            .coordinator
            .publication_is_complete(receipt)
            .map_err(|_| {
                deadline.unresolved(Unresolved::Store);
                post_refusal("store_unavailable", 503, receipt)
            })? {
            true => {}
            false => {
                apply_publication(&self.state, receipt, &publication).map_err(|error| {
                    deadline.unresolved(publication_unresolved(error));
                    publication_refusal(error, receipt)
                })?;
                self.coordinator.mark_published(receipt).map_err(|_| {
                    deadline.unresolved(Unresolved::Store);
                    post_refusal("store_unavailable", 503, receipt)
                })?;
            }
        }
        deadline.terminal();
        Ok(receipt_frame(&publication, receipt, replayed))
    }
}

/// One live secret-bearing state machine. It is never stored in application state.
pub(super) struct ActiveCeremony {
    allocation: Allocation,
    batch: SecretBatch,
    coordinator: Arc<TransactionCoordinator>,
    expected: Vec<SecretNeed>,
    next_ordinal: u16,
    prepared: bool,
    publication: Publication,
    state: AppState,
    cancellation: CeremonyCancellation,
    #[cfg(test)]
    cancellation_pause: Option<Arc<CancellationPause>>,
    _claim: Option<Claim>,
    _tenant_claim: Option<Claim>,
}

impl ActiveCeremony {
    #[cfg(test)]
    pub(super) fn abort_probe(
        allocation: Allocation,
        coordinator: Arc<TransactionCoordinator>,
        state: AppState,
        deadline: DeadlineController,
    ) -> Self {
        let scope = CredentialScope::new("local", "example.test")
            .expect("the fixed abort-probe credential scope is valid");
        Self {
            allocation,
            batch: SecretBatch::new(scope),
            coordinator: coordinator.clone(),
            expected: Vec::new(),
            next_ordinal: 1,
            prepared: false,
            publication: Publication {
                action: PublicationAction::Connect,
                connector: "test".to_owned(),
                expected_head: None,
                instance: "00000000000000000000000000".to_owned(),
                label: "test".to_owned(),
                next_head: "1111111111111111111111111111111111111111111111111111111111111111"
                    .to_owned(),
                schema: PUBLICATION_SCHEMA.to_owned(),
                settings: Vec::new(),
                tenant: "local".to_owned(),
            },
            state,
            cancellation: CeremonyCancellation::new(allocation, coordinator.clone(), deadline),
            cancellation_pause: None,
            _claim: None,
            _tenant_claim: None,
        }
    }

    pub(super) async fn accept(
        &mut self,
        request: Frame,
        deadline: &DeadlineController,
    ) -> AdvanceOutcome {
        if request.opcode() == Opcode::Secret {
            return self.accept_secret(request).await;
        }
        if request.opcode() != self.publication.commit_opcode() {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        }
        if usize::from(self.next_ordinal) <= self.expected.len() {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        }
        let Some(payload) = request.control_payload() else {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        };
        let commit: Commit = match canonical_control(payload) {
            Ok(commit) => commit,
            Err(()) => {
                self.abort().await;
                return AdvanceOutcome::Terminal(refusal("invalid_request", 400, "never"));
            }
        };
        if decode_identity(&commit.transaction_id)
            != Some(self.allocation.transaction_protocol_bytes())
            || decode_identity(&commit.proposal_digest)
                != Some(self.allocation.proposal_protocol_bytes())
        {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("proposal_conflict", 409, "refresh"));
        }
        if !self.prepared {
            if let Err(error) = self.coordinator.prepare(self.allocation, &self.batch).await {
                self.abort().await;
                return AdvanceOutcome::Terminal(coordinator_refusal(error));
            }
            self.prepared = true;
            #[cfg(test)]
            if let Some(pause) = &self.cancellation_pause {
                pause.at(CancellationPoint::Prepare).await;
            }
        }
        let receipt = self.allocation.receipt_id();
        if deadline
            .begin_decision(receipt_identity(receipt), Unresolved::Store)
            .is_err()
        {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("deadline_exceeded", 408, "refresh"));
        }
        let decision = self.coordinator.decide_commit(self.allocation);
        self.cancellation.disarm();
        if decision.is_err() {
            let _ = deadline.decided(receipt_identity(receipt), Unresolved::Store);
            return AdvanceOutcome::Terminal(post_refusal("store_unavailable", 503, receipt));
        }
        if deadline
            .decided(receipt_identity(receipt), Unresolved::Store)
            .is_err()
        {
            return AdvanceOutcome::Terminal(post_refusal("internal_refusal", 500, receipt));
        }
        #[cfg(test)]
        if let Some(pause) = &self.cancellation_pause {
            pause.at(CancellationPoint::Commit).await;
        }

        // Once the decision journal is durable, this owned task must outlive a transport timeout
        // or disconnect. Dropping the JoinHandle detaches rather than aborts it, so recovery and
        // the provider see only roll-forward after this boundary.
        let coordinator = self.coordinator.clone();
        let allocation = self.allocation;
        let state = self.state.clone();
        let publication = self.publication.clone();
        let rollforward_deadline = deadline.clone();
        let rollforward = tokio::spawn(async move {
            let receipt = match coordinator.commit(allocation).await {
                Ok(receipt) => receipt,
                Err(error) => {
                    rollforward_deadline.unresolved(Unresolved::Store);
                    return coordinator_refusal(error);
                }
            };
            if let Err(error) = apply_publication(&state, receipt, &publication) {
                rollforward_deadline.unresolved(publication_unresolved(error));
                return publication_refusal(error, receipt);
            }
            if coordinator.mark_published(receipt).is_err() {
                rollforward_deadline.unresolved(Unresolved::Store);
                return post_refusal("store_unavailable", 503, receipt);
            }
            rollforward_deadline.terminal();
            receipt_frame(&publication, receipt, false)
        });
        match rollforward.await {
            Ok(frame) => AdvanceOutcome::Terminal(frame),
            Err(_) => {
                deadline.unresolved(Unresolved::Internal);
                AdvanceOutcome::Terminal(post_refusal("internal_refusal", 500, receipt))
            }
        }
    }

    async fn accept_secret(&mut self, request: Frame) -> AdvanceOutcome {
        let Some(secret) = request.secret_payload() else {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        };
        let index = usize::from(self.next_ordinal.saturating_sub(1));
        let Some(expected) = self.expected.get(index) else {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        };
        if secret.ordinal() != self.next_ordinal {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("unexpected_frame", 409, "never"));
        }
        let value = match String::from_utf8(secret.bytes().to_vec()) {
            Ok(value) if !value.is_empty() => value,
            _ => {
                self.abort().await;
                return AdvanceOutcome::Terminal(refusal("invalid_request", 400, "never"));
            }
        };
        if self
            .batch
            .put(expected.reference.clone(), Secret::new(value))
            .is_err()
        {
            self.abort().await;
            return AdvanceOutcome::Terminal(refusal("invalid_request", 400, "never"));
        }
        #[cfg(test)]
        if let Some(pause) = &self.cancellation_pause {
            pause.at(CancellationPoint::Secret).await;
        }
        self.next_ordinal = self.next_ordinal.saturating_add(1);
        AdvanceOutcome::Awaiting
    }

    pub(super) async fn abort(&mut self) {
        self.cancellation.abort().await;
    }
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CancellationPoint {
    Begin,
    Prepare,
    Secret,
    Commit,
}

#[cfg(test)]
struct CancellationPause {
    point: CancellationPoint,
    entered: tokio::sync::Notify,
}

#[cfg(test)]
impl CancellationPause {
    fn new(point: CancellationPoint) -> Arc<Self> {
        Arc::new(Self {
            point,
            entered: tokio::sync::Notify::new(),
        })
    }

    async fn at(&self, point: CancellationPoint) {
        if self.point != point {
            return;
        }
        self.entered.notify_one();
        std::future::pending::<()>().await;
    }

    async fn wait_until_entered(&self) {
        self.entered.notified().await;
    }
}

/// One fail-closed tombstone guard armed from allocation until the durable decision boundary.
struct CeremonyCancellation {
    allocation: Allocation,
    coordinator: Arc<TransactionCoordinator>,
    deadline: DeadlineController,
    armed: bool,
}

impl CeremonyCancellation {
    fn new(
        allocation: Allocation,
        coordinator: Arc<TransactionCoordinator>,
        deadline: DeadlineController,
    ) -> Self {
        Self {
            allocation,
            coordinator,
            deadline,
            armed: true,
        }
    }

    async fn abort(&mut self) {
        if !self.armed || !self.deadline.may_abort() {
            return;
        }
        if self
            .coordinator
            .abort_before_decision(self.allocation)
            .await
            .is_ok()
        {
            self.armed = false;
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for CeremonyCancellation {
    fn drop(&mut self) {
        if !self.armed || !self.deadline.may_abort() {
            return;
        }
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let allocation = self.allocation;
        let coordinator = self.coordinator.clone();
        let deadline = self.deadline.clone();
        runtime.spawn(async move {
            if deadline.may_abort() {
                let _ = coordinator.abort_before_decision(allocation).await;
            }
        });
    }
}

pub(super) enum BeginOutcome {
    Terminal(Frame),
    Active {
        response: Frame,
        active: Box<ActiveCeremony>,
    },
}

pub(super) enum AdvanceOutcome {
    Awaiting,
    Terminal(Frame),
}

struct SecretTargets {
    batch: SecretBatch,
    needs: Vec<SecretNeed>,
}

async fn secret_targets<'a>(
    state: &AppState,
    tenant: &Tenant,
    connector: &str,
    new_instance: Option<&InstanceId>,
    creating: bool,
    selected: impl Iterator<Item = &'a str>,
) -> Result<SecretTargets, Frame> {
    let provider = connector_catalog::provider(connector_catalog::ProviderKey::id(connector))
        .ok_or_else(|| refusal("unknown_connector", 404, "refresh"))?;
    // C-515 requires a valid empty batch for a settings-only connector. The declaration id is a
    // stable provider-owned scope component when that connector declares no separate authority.
    let authority = provider.authority.unwrap_or(provider.id);
    let scope = CredentialScope::new(tenant.as_str(), authority)
        .map_err(|_| refusal("invalid_request", 400, "never"))?;
    let mut batch = SecretBatch::new(scope);
    let declared = provider
        .auth
        .iter()
        .map(|credential| DeclaredCredential {
            name: credential.name,
            leaf: credential.leaf,
        })
        .collect::<Vec<_>>();
    let declaration = ConnectorDeclaration {
        connector: provider.id,
        authority: provider.authority,
        credentials: &declared,
    };
    if declared.is_empty() {
        return Ok(SecretTargets {
            batch,
            needs: Vec::new(),
        });
    }
    let registry = state
        .connection_registry()
        .ok_or_else(|| refusal("local_management_unavailable", 503, "operator"))?;
    let entries = registry
        .entries(tenant, connector)
        .map_err(|_| refusal("store_unavailable", 503, "operator"))?;
    let mut held = entries
        .iter()
        .map(|entry| entry.instance.clone())
        .collect::<Vec<_>>();
    let selected_instance = if creating {
        let instance = new_instance.ok_or_else(|| refusal("internal_refusal", 500, "operator"))?;
        held.push(instance.clone());
        Some(instance)
    } else {
        Some(new_instance.ok_or_else(|| refusal("unknown_label", 404, "refresh"))?)
    };
    if creating && entries.len() == 1 {
        let old = &entries[0].instance;
        let sources = declaration
            .addresses_for(
                tenant,
                TenantInstances::held(std::slice::from_ref(old), Some(old)),
            )
            .map_err(|_| refusal("invalid_request", 400, "never"))?;
        let destinations = declaration
            .addresses_for(tenant, TenantInstances::held(&held, Some(old)))
            .map_err(|_| refusal("invalid_request", 400, "never"))?;
        let credentials = state
            .credentials()
            .ok_or_else(|| refusal("local_management_unavailable", 503, "operator"))?;
        for ((_, source), (_, destination)) in sources.into_iter().zip(destinations) {
            match credentials.get(&source).await {
                Ok(_) => {
                    batch
                        .move_secret(source, destination)
                        .map_err(|_| refusal("internal_refusal", 500, "operator"))?;
                }
                Err(error) if error.is_not_found() => {}
                Err(_) => return Err(refusal("store_unavailable", 503, "operator")),
            }
        }
    }
    let named = selected_instance;
    let addresses = declaration
        .addresses_for(tenant, TenantInstances::held(&held, named))
        .map_err(|_| refusal("invalid_request", 400, "never"))?;
    let selected = selected.collect::<Vec<_>>();
    let needs = addresses
        .into_iter()
        .filter_map(|(declared, reference)| {
            let target = format!("credential.{}", declared.name);
            selected
                .contains(&target.as_str())
                .then_some((target, reference))
        })
        .enumerate()
        .map(|(index, (target, reference))| SecretNeed {
            ordinal: u16::try_from(index + 1).expect("the proposal target bound is below u16"),
            reference,
            target,
        })
        .collect();
    Ok(SecretTargets { batch, needs })
}

async fn credential_state_admits(
    state: &AppState,
    targets: &SecretTargets,
    action: CredentialAction,
) -> Result<bool, ()> {
    let Some(store) = state.credentials() else {
        return Err(());
    };
    let mut present = Vec::with_capacity(targets.needs.len());
    for need in &targets.needs {
        match store.get(&need.reference).await {
            Ok(_) => present.push(true),
            Err(error) if error.is_not_found() => present.push(false),
            Err(_) => return Err(()),
        }
    }
    Ok(match action {
        CredentialAction::Acquire => present.iter().all(|present| !present),
        CredentialAction::Rotate => present.iter().all(|present| *present),
    })
}

#[derive(Serialize)]
struct NeedSecrets<'a> {
    proposal_digest: String,
    secrets: Vec<Need<'a>>,
    transaction_id: String,
}

#[derive(Serialize)]
struct Need<'a> {
    ordinal: u16,
    target: &'a str,
}

struct SecretNeed {
    ordinal: u16,
    reference: CredentialRef,
    target: String,
}

fn need_secrets(allocation: Allocation, needs: &[SecretNeed]) -> Frame {
    let body = NeedSecrets {
        proposal_digest: lowerhex(&allocation.proposal_protocol_bytes()),
        secrets: needs
            .iter()
            .map(|need| Need {
                ordinal: need.ordinal,
                target: &need.target,
            })
            .collect(),
        transaction_id: lowerhex(&allocation.transaction_protocol_bytes()),
    };
    control(Opcode::NeedSecrets, &body)
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Commit {
    proposal_digest: String,
    transaction_id: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReceiptQuery {
    receipt_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Publication {
    action: PublicationAction,
    connector: String,
    expected_head: Option<String>,
    instance: String,
    label: String,
    next_head: String,
    schema: String,
    settings: Vec<PublishedSetting>,
    tenant: String,
}

impl Publication {
    const fn commit_opcode(&self) -> Opcode {
        match self.action {
            PublicationAction::Connect => Opcode::ConnectCommit,
            PublicationAction::Acquire | PublicationAction::Rotate => Opcode::CredentialCommit,
        }
    }

    const fn receipt_opcode(&self) -> Opcode {
        match self.action {
            PublicationAction::Connect => Opcode::ConnectReceipt,
            PublicationAction::Acquire | PublicationAction::Rotate => Opcode::CredentialReceipt,
        }
    }

    const fn operation(&self) -> &'static str {
        match self.action {
            PublicationAction::Connect => "connect",
            PublicationAction::Acquire => "acquire",
            PublicationAction::Rotate => "rotate",
        }
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum PublicationAction {
    Connect,
    Acquire,
    Rotate,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PublishedSetting {
    authority: bool,
    target: String,
    value: String,
}

#[derive(Serialize)]
struct Receipt<'a> {
    commit: ReceiptCommit,
    connector: &'a str,
    label: &'a str,
    operation: &'static str,
    receipt_id: String,
    replayed: bool,
    schema: &'static str,
}

#[derive(Serialize)]
struct ReceiptCommit {
    audit: &'static str,
    resource: &'static str,
}

fn receipt_frame(publication: &Publication, receipt: ReceiptId, replayed: bool) -> Frame {
    control(
        publication.receipt_opcode(),
        &Receipt {
            commit: ReceiptCommit {
                audit: "committed",
                resource: "committed",
            },
            connector: &publication.connector,
            label: &publication.label,
            operation: publication.operation(),
            receipt_id: lowerhex(&receipt.protocol_bytes()),
            replayed,
            schema: "exchange.connect-receipt.v1",
        },
    )
}

#[derive(Clone, Copy)]
pub(crate) enum PublicationRefusal {
    Store,
    Audit,
    Invariant,
}

fn receipt_identity(receipt: ReceiptId) -> ReceiptIdentity {
    ReceiptIdentity::from_protocol_bytes(receipt.protocol_bytes())
        .expect("transaction receipts are always nonzero")
}

const fn publication_unresolved(refusal: PublicationRefusal) -> Unresolved {
    match refusal {
        PublicationRefusal::Store => Unresolved::Store,
        PublicationRefusal::Audit => Unresolved::Audit,
        PublicationRefusal::Invariant => Unresolved::Internal,
    }
}

impl PublicationRefusal {
    pub(crate) const fn startup_reason(self) -> &'static str {
        match self {
            Self::Store => "the durable connection publication store is unavailable",
            Self::Audit => "the durable connection publication audit is unavailable",
            Self::Invariant => "the durable connection publication image is invalid",
        }
    }
}

fn apply_publication(
    state: &AppState,
    receipt: ReceiptId,
    publication: &Publication,
) -> Result<(), PublicationRefusal> {
    validate_publication(publication)?;
    let tenant =
        Tenant::new(publication.tenant.clone()).map_err(|_| PublicationRefusal::Invariant)?;
    let key = CredentialHeadKey::new(tenant.as_str(), &publication.connector, &publication.label)
        .map_err(|_| PublicationRefusal::Invariant)?;
    let heads = state.credential_heads().ok_or(PublicationRefusal::Store)?;
    let next = CredentialHead::parse(publication.next_head.clone())
        .map_err(|_| PublicationRefusal::Invariant)?;
    match publication.action {
        PublicationAction::Connect => {
            let instance = InstanceId::parse(&publication.instance)
                .map_err(|_| PublicationRefusal::Invariant)?;
            let registry = state
                .connection_registry()
                .ok_or(PublicationRefusal::Store)?;
            let entries = registry
                .entries(&tenant, &publication.connector)
                .map_err(|_| PublicationRefusal::Store)?;
            if entries.len() == 1 && entries[0].instance != instance {
                state
                    .settings()
                    .ok_or(PublicationRefusal::Store)?
                    .qualify_instance(&tenant, &publication.connector, &entries[0].instance)
                    .map_err(|_| PublicationRefusal::Store)?;
                publication_boundary("qualify")?;
            }
            let selected_instance = (!entries.is_empty()).then_some(&instance);
            let settings = state.settings().ok_or(PublicationRefusal::Store)?;
            for (index, setting) in publication.settings.iter().enumerate() {
                let (service, field) = setting_target(&setting.target)?;
                let declared =
                    DeclaredSetting::parse(service, field).ok_or(PublicationRefusal::Invariant)?;
                if setting.authority {
                    settings
                        .ensure_authority_proposal_for_instance(
                            &tenant,
                            &publication.connector,
                            selected_instance,
                            &declared,
                            &setting.value,
                        )
                        .map_err(|_| PublicationRefusal::Store)?;
                    publication_boundary(&format!("authority-{index}"))?;
                } else {
                    settings
                        .set_for_instance(
                            &tenant,
                            &publication.connector,
                            selected_instance,
                            &declared,
                            &setting.value,
                        )
                        .map_err(|_| PublicationRefusal::Store)?;
                    publication_boundary(&format!("setting-{index}"))?;
                }
            }
            match heads.current(&key) {
                Ok(current) if current == next => {}
                Ok(_) => return Err(PublicationRefusal::Invariant),
                Err(crate::credential_head::CredentialHeadError::UnknownKey) => heads
                    .insert_new(key.clone(), next)
                    .map_err(|_| PublicationRefusal::Store)?,
                Err(_) => return Err(PublicationRefusal::Store),
            }
            publication_boundary("head")?;
            // The unresolved journal row gates readers and competing mutations while this private
            // metadata image is assembled. Append the canonical terminal event only after that
            // image is complete and before publishing the label.
            record_publication_audit(state, receipt, publication)?;
            let label = ConnectionLabel::new(publication.label.clone())
                .map_err(|_| PublicationRefusal::Invariant)?;
            match registry.resolve(&tenant, &publication.connector, &label) {
                Ok(Some(current)) if current == instance => {}
                Ok(Some(_)) => return Err(PublicationRefusal::Invariant),
                Ok(None) => registry
                    .assign(&tenant, &publication.connector, &label, &instance)
                    .map_err(|_| PublicationRefusal::Store)?,
                Err(_) => return Err(PublicationRefusal::Store),
            }
            publication_boundary("label")?;
        }
        PublicationAction::Acquire | PublicationAction::Rotate => {
            let expected = publication
                .expected_head
                .clone()
                .ok_or(PublicationRefusal::Invariant)
                .and_then(|value| {
                    CredentialHead::parse(value).map_err(|_| PublicationRefusal::Invariant)
                })?;
            match heads.current(&key) {
                Ok(current) if current == next => {}
                Ok(current) if current == expected => heads
                    .compare_and_advance(&key, &expected, next)
                    .map_err(|_| PublicationRefusal::Store)?,
                Ok(_) => return Err(PublicationRefusal::Invariant),
                Err(_) => return Err(PublicationRefusal::Store),
            }
            publication_boundary("head")?;
            // The changed head remains hidden behind the same pending-publication predicate until
            // its transaction-derived audit event has been durably deduplicated.
            record_publication_audit(state, receipt, publication)?;
        }
    }
    Ok(())
}

fn record_publication_audit(
    state: &AppState,
    receipt: ReceiptId,
    publication: &Publication,
) -> Result<(), PublicationRefusal> {
    let audit = state.audit().ok_or(PublicationRefusal::Audit)?;
    let request_id = RequestId::generate().map_err(|_| PublicationRefusal::Audit)?;
    let action = match publication.action {
        PublicationAction::Connect => Action::ConnectionCreated,
        PublicationAction::Acquire => Action::CredentialAcquired,
        PublicationAction::Rotate => Action::CredentialRotated,
    };
    audit
        .record_terminal_once(
            &lowerhex(&receipt.protocol_bytes()),
            &request_id,
            action,
            Outcome::Succeeded,
            None,
            Target::ConnectionInstance {
                connector: publication.connector.clone(),
                label: publication.label.clone(),
            },
        )
        .map_err(|_| PublicationRefusal::Audit)?;
    publication_boundary("audit")
}

fn publication_boundary(step: &str) -> Result<(), PublicationRefusal> {
    #[cfg(feature = "native-root-test-seam")]
    {
        if std::env::var(PUBLICATION_CRASH_AFTER_ENV).as_deref() == Ok(step) {
            // This test-only seam deliberately bypasses destructors so native process tests can
            // prove recovery from the exact durable boundary rather than a graceful shutdown.
            std::process::exit(86);
        }
        if std::env::var(PUBLICATION_FAIL_AFTER_ENV).as_deref() == Ok(step)
            && PUBLICATION_FAILURE_INJECTED
                .compare_exchange(
                    false,
                    true,
                    std::sync::atomic::Ordering::SeqCst,
                    std::sync::atomic::Ordering::SeqCst,
                )
                .is_ok()
        {
            return Err(PublicationRefusal::Store);
        }
    }
    #[cfg(not(feature = "native-root-test-seam"))]
    let _ = step;
    Ok(())
}

fn validate_publication(publication: &Publication) -> Result<(), PublicationRefusal> {
    if publication.schema != PUBLICATION_SCHEMA {
        return Err(PublicationRefusal::Invariant);
    }
    Tenant::new(publication.tenant.clone()).map_err(|_| PublicationRefusal::Invariant)?;
    CredentialHeadKey::new(
        &publication.tenant,
        &publication.connector,
        &publication.label,
    )
    .map_err(|_| PublicationRefusal::Invariant)?;
    CredentialHead::parse(publication.next_head.clone())
        .map_err(|_| PublicationRefusal::Invariant)?;
    ConnectionLabel::new(publication.label.clone()).map_err(|_| PublicationRefusal::Invariant)?;
    InstanceId::parse(&publication.instance).map_err(|_| PublicationRefusal::Invariant)?;
    match publication.action {
        PublicationAction::Connect if publication.expected_head.is_some() => {
            return Err(PublicationRefusal::Invariant);
        }
        PublicationAction::Acquire | PublicationAction::Rotate => {
            let expected = publication
                .expected_head
                .clone()
                .ok_or(PublicationRefusal::Invariant)?;
            CredentialHead::parse(expected).map_err(|_| PublicationRefusal::Invariant)?;
            if !publication.settings.is_empty() {
                return Err(PublicationRefusal::Invariant);
            }
        }
        PublicationAction::Connect => {}
    }
    for setting in &publication.settings {
        let (service, field) = setting_target(&setting.target)?;
        DeclaredSetting::parse(service, field).ok_or(PublicationRefusal::Invariant)?;
    }
    Ok(())
}

fn parse_publication(bytes: &[u8]) -> Result<Publication, PublicationRefusal> {
    canonical_control(bytes).map_err(|_| PublicationRefusal::Invariant)
}

fn setting_target(target: &str) -> Result<(&str, &str), PublicationRefusal> {
    target
        .strip_prefix("setting.")
        .and_then(|value| value.split_once('.'))
        .filter(|(service, field)| !service.is_empty() && !field.is_empty())
        .ok_or(PublicationRefusal::Invariant)
}

fn resolved_instance(
    state: &AppState,
    tenant: &Tenant,
    connector: &str,
    label: &str,
) -> Option<InstanceId> {
    let label = ConnectionLabel::new(label).ok()?;
    state
        .connection_registry()?
        .resolve(tenant, connector, &label)
        .ok()?
}

fn target_facts(snapshot: &crate::routes::NativePlanSnapshot) -> Vec<TargetFact<'_>> {
    snapshot
        .targets
        .iter()
        .map(|target| TargetFact {
            target: &target.id,
            revision: &target.revision,
            required: target.required,
            partition: target.partition,
        })
        .collect()
}

fn proposal_refusal(
    begin: &ConnectBegin,
    snapshot: &crate::routes::NativePlanSnapshot,
    _error: ProposalError,
) -> Frame {
    if begin.targets().iter().any(|target| {
        !snapshot
            .targets
            .iter()
            .any(|known| known.id == target.target())
    }) {
        refusal("unknown_target", 422, "refresh")
    } else if begin.targets().iter().any(|target| {
        snapshot
            .targets
            .iter()
            .find(|known| known.id == target.target())
            .is_some_and(|known| known.revision != target.revision())
    }) {
        refusal("stale_plan", 409, "refresh")
    } else {
        refusal("invalid_request", 400, "never")
    }
}

fn proposal_refusal_credential(
    begin: &CredentialBegin,
    snapshot: &crate::routes::NativePlanSnapshot,
    _error: ProposalError,
) -> Frame {
    if begin.targets().iter().any(|target| {
        !snapshot
            .targets
            .iter()
            .any(|known| known.id == target.target())
    }) {
        refusal("unknown_target", 422, "refresh")
    } else if begin.targets().iter().any(|target| {
        snapshot
            .targets
            .iter()
            .find(|known| known.id == target.target())
            .is_some_and(|known| known.revision != target.revision())
    }) {
        refusal("stale_plan", 409, "refresh")
    } else {
        refusal("invalid_request", 400, "never")
    }
}

fn protocol_digest(digest: &super::proposal::ProposalDigest) -> SecretProposalDigest {
    SecretProposalDigest::from_protocol_bytes(
        decode_identity(digest.as_str()).expect("proposal digest has the closed lowerhex grammar"),
    )
}

fn canonical_control<T>(bytes: &[u8]) -> Result<T, ()>
where
    T: for<'de> Deserialize<'de> + Serialize,
{
    let value = serde_json::from_slice::<T>(bytes).map_err(|_| ())?;
    (serde_json::to_vec(&value).map_err(|_| ())? == bytes)
        .then_some(value)
        .ok_or(())
}

fn mint_instance() -> Result<InstanceId, ()> {
    let mut bytes = crate::entropy::bytes::<16>().map_err(|_| ())?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let text = format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        bytes[8], bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    );
    InstanceId::parse(&text).map_err(|_| ())
}

fn lowerhex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[usize::from(byte >> 4)] as char);
        value.push(HEX[usize::from(byte & 0x0f)] as char);
    }
    value
}

fn decode_identity(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        bytes[index] = (nibble(pair[0])? << 4) | nibble(pair[1])?;
    }
    Some(bytes)
}

const fn nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    commit: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt_id: Option<String>,
    retry: &'static str,
    schema: &'static str,
    status: u16,
}

fn refusal(code: &'static str, status: u16, retry: &'static str) -> Frame {
    error(ErrorBody {
        code,
        commit: "none",
        receipt_id: None,
        retry,
        schema: "exchange.local-management-error.v1",
        status,
    })
}

fn post_refusal(code: &'static str, status: u16, receipt: ReceiptId) -> Frame {
    error(ErrorBody {
        code,
        commit: "query_receipt",
        receipt_id: Some(lowerhex(&receipt.protocol_bytes())),
        retry: "same_proposal",
        schema: "exchange.local-management-error.v1",
        status,
    })
}

fn coordinator_refusal(refusal: CoordinatorRefusal) -> Frame {
    match refusal.receipt {
        Some(receipt) => post_refusal(refusal.code, refusal.status, receipt),
        None => refusal_frame(refusal.code, refusal.status, refusal.retry),
    }
}

fn refusal_frame(code: &'static str, status: u16, retry: &'static str) -> Frame {
    refusal(code, status, retry)
}

fn publication_refusal(error: PublicationRefusal, receipt: ReceiptId) -> Frame {
    match error {
        PublicationRefusal::Store => post_refusal("store_unavailable", 503, receipt),
        PublicationRefusal::Audit => post_refusal("audit_unavailable", 503, receipt),
        PublicationRefusal::Invariant => post_refusal("internal_refusal", 500, receipt),
    }
}

fn control<T: Serialize>(opcode: Opcode, value: &T) -> Frame {
    let bytes = serde_json::to_vec(value).expect("closed FXLM response is serializable");
    Frame::control(Direction::ServerToClient, opcode, bytes)
        .expect("closed FXLM response is within the control bound")
}

fn error(body: ErrorBody) -> Frame {
    control(Opcode::Error, &body)
}

#[cfg(test)]
mod cancellation_tests {
    use std::collections::BTreeSet;
    use std::sync::Arc;

    use exchange_host::{
        ConnectionLabel, ConnectionRegistry as _, CredentialStore, InstanceId,
        MemoryConnectionRegistry, SettingsStore,
    };
    use serde_json::{json, Value};

    use super::*;
    use crate::audit::AuditJournal;
    use crate::credential_head::CredentialHeadStore;
    use crate::local_management::proposal::TargetPartition;

    struct Harness {
        root: std::path::PathBuf,
        _store: CredentialStore,
        coordinator: Arc<TransactionCoordinator>,
        state: AppState,
        tenant: Tenant,
    }

    impl Harness {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
            let root = std::env::temp_dir().join(format!(
                "flux-exchange-x135-real-cancellation-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
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
            let tenant = Tenant::new("local").expect("tenant");
            let registry = Arc::new(MemoryConnectionRegistry::default());
            let label = ConnectionLabel::new("work").expect("label");
            registry
                .assign(
                    &tenant,
                    "github",
                    &label,
                    &InstanceId::parse("11111111-1111-4111-8111-111111111111").expect("instance"),
                )
                .expect("seed held connection");
            let head_key =
                CredentialHeadKey::new("local", "github", "work").expect("credential-head key");
            let heads = Arc::new(
                CredentialHeadStore::migrate_legacy(&root, &[head_key])
                    .expect("credential-head migration"),
            );
            let settings = Arc::new(
                SettingsStore::bind(root.join("settings/store.json")).expect("settings store"),
            );
            let audit = Arc::new(
                AuditJournal::bind(root.join("audit/journal.jsonl")).expect("audit journal"),
            );
            let state = AppState::without_identity()
                .with_credentials(store.secrets())
                .with_settings(settings)
                .with_connection_registry(registry)
                .with_credential_heads(heads)
                .with_transaction_coordinator(coordinator.clone())
                .with_audit(audit);
            Self {
                root,
                _store: store,
                coordinator,
                state,
                tenant,
            }
        }

        fn connect_begin(&self, label: &str) -> (Frame, SecretProposalDigest) {
            let snapshot =
                crate::routes::native_plan_snapshot(&self.state, &self.tenant, "freshdesk", None)
                    .unwrap_or_else(|_| panic!("freshdesk plan"));
            let mut selected = BTreeSet::new();
            let mut targets = Vec::new();
            let mut settings = Vec::new();
            let mut authorities = Vec::new();
            for target in snapshot.targets.iter().filter(|target| target.required) {
                if selected.insert(target.id.as_str()) {
                    targets.push(json!({"revision": target.revision, "target": target.id}));
                }
                match target.partition {
                    TargetPartition::ConnectionName | TargetPartition::Credential => {}
                    TargetPartition::Setting => settings.push(json!({
                        "target": target.id,
                        "value": setting_value(&target.id),
                    })),
                    TargetPartition::Authority => {
                        authorities.push(json!({"revision": null, "target": target.id}));
                    }
                }
            }
            let value = json!({
                "authorities": authorities,
                "connector": "freshdesk",
                "label": label,
                "plan_revision": snapshot.plan_revision,
                "settings": settings,
                "targets": targets,
            });
            proposal_frame(Opcode::ConnectBegin, value, |bytes| {
                let parsed = ConnectBegin::parse_canonical(bytes).expect("canonical connect BEGIN");
                protocol_digest(&parsed.proposal_digest())
            })
        }

        fn credential_begin(&self) -> (Frame, SecretProposalDigest) {
            let snapshot = crate::routes::native_plan_snapshot(
                &self.state,
                &self.tenant,
                "github",
                Some("work"),
            )
            .unwrap_or_else(|_| panic!("github selected plan"));
            let targets = snapshot
                .targets
                .iter()
                .filter(|target| target.required && target.partition == TargetPartition::Credential)
                .map(|target| json!({"revision": target.revision, "target": target.id}))
                .collect::<Vec<_>>();
            assert!(!targets.is_empty(), "github must request one credential");
            let value = json!({
                "action": "acquire",
                "connector": "github",
                "credential_revision": snapshot.credential_revision.expect("legacy head"),
                "label": "work",
                "plan_revision": snapshot.plan_revision,
                "targets": targets,
            });
            proposal_frame(Opcode::CredentialBegin, value, |bytes| {
                let parsed =
                    CredentialBegin::parse_canonical(bytes).expect("canonical credential BEGIN");
                protocol_digest(&parsed.proposal_digest())
            })
        }

        fn proposal_state(
            &self,
            kind: TransactionKind,
            connector: &str,
            label: &str,
            proposal: SecretProposalDigest,
        ) -> Option<ProposalState> {
            self.coordinator
                .proposal_state_for_tenant(kind, self.tenant.as_str(), connector, label, proposal)
                .expect("proposal state")
        }

        async fn wait_tombstoned(
            &self,
            kind: TransactionKind,
            connector: &str,
            label: &str,
            proposal: SecretProposalDigest,
        ) {
            for _ in 0..64 {
                if self
                    .proposal_state(kind, connector, label, proposal)
                    .is_none()
                {
                    return;
                }
                tokio::task::yield_now().await;
            }
            panic!("cancelled production operation was not tombstoned");
        }

        fn finish(self) {
            let root = self.root.clone();
            drop(self);
            let _ = std::fs::remove_dir_all(root);
        }
    }

    fn proposal_frame(
        opcode: Opcode,
        value: Value,
        digest: impl FnOnce(&[u8]) -> SecretProposalDigest,
    ) -> (Frame, SecretProposalDigest) {
        let payload = serde_json::to_vec(&value).expect("canonical proposal");
        let digest = digest(&payload);
        (
            Frame::control(Direction::ClientToServer, opcode, payload).expect("BEGIN frame"),
            digest,
        )
    }

    fn setting_value(target: &str) -> &'static str {
        if target.ends_with(".domain") {
            "acme.freshdesk.com"
        } else {
            panic!("no closed fixture value for {target}")
        }
    }

    async fn cancel_begin_at(
        harness: &Harness,
        point: CancellationPoint,
        label: &str,
    ) -> SecretProposalDigest {
        let (request, proposal) = harness.connect_begin(label);
        let pause = CancellationPause::new(point);
        let ceremony = Ceremony::with_cancellation_pause(
            harness.state.clone(),
            harness.coordinator.clone(),
            pause.clone(),
        );
        let tenant = harness.tenant.clone();
        let deadline = DeadlineController::start();
        let task = tokio::spawn(async move { ceremony.begin(&tenant, request, &deadline).await });
        pause.wait_until_entered().await;
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        harness
            .wait_tombstoned(TransactionKind::Connect, "freshdesk", label, proposal)
            .await;
        proposal
    }

    #[tokio::test]
    async fn allocated_ceremony_drop_tombstones_only_until_the_decision_guard_disarms() {
        let harness = Harness::new();

        for (point, label) in [
            (CancellationPoint::Begin, "begin-drop"),
            (CancellationPoint::Prepare, "prepare-drop"),
        ] {
            let proposal = cancel_begin_at(&harness, point, label).await;
            let (retry, retry_proposal) = harness.connect_begin(label);
            assert_eq!(proposal.protocol_bytes(), retry_proposal.protocol_bytes());
            assert!(matches!(
                Ceremony::new(harness.state.clone(), harness.coordinator.clone())
                    .begin(&harness.tenant, retry, &DeadlineController::start())
                    .await,
                BeginOutcome::Active { .. }
            ));
        }

        let (credential, proposal) = harness.credential_begin();
        let pause = CancellationPause::new(CancellationPoint::Secret);
        let ceremony = Ceremony::with_cancellation_pause(
            harness.state.clone(),
            harness.coordinator.clone(),
            pause.clone(),
        );
        let BeginOutcome::Active { mut active, .. } = ceremony
            .begin(&harness.tenant, credential, &DeadlineController::start())
            .await
        else {
            panic!("credential BEGIN must become active");
        };
        let secret = Frame::secret(Direction::ClientToServer, 1, b"phase-secret".to_vec())
            .expect("SECRET frame");
        let task =
            tokio::spawn(async move { active.accept(secret, &DeadlineController::start()).await });
        pause.wait_until_entered().await;
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        harness
            .wait_tombstoned(TransactionKind::Credential, "github", "work", proposal)
            .await;

        let (credential, proposal) = harness.credential_begin();
        let pause = CancellationPause::new(CancellationPoint::Commit);
        let ceremony = Ceremony::with_cancellation_pause(
            harness.state.clone(),
            harness.coordinator.clone(),
            pause.clone(),
        );
        let BeginOutcome::Active {
            response,
            mut active,
        } = ceremony
            .begin(&harness.tenant, credential, &DeadlineController::start())
            .await
        else {
            panic!("credential BEGIN must become active");
        };
        active
            .accept(
                Frame::secret(Direction::ClientToServer, 1, b"commit-secret".to_vec())
                    .expect("SECRET frame"),
                &DeadlineController::start(),
            )
            .await;
        let needed: Value = serde_json::from_slice(response.control_payload().expect("NEED body"))
            .expect("NEED JSON");
        let commit = json!({
            "proposal_digest": needed["proposal_digest"],
            "transaction_id": needed["transaction_id"],
        });
        let commit = Frame::control(
            Direction::ClientToServer,
            Opcode::CredentialCommit,
            serde_json::to_vec(&commit).expect("COMMIT JSON"),
        )
        .expect("COMMIT frame");
        let deadline = DeadlineController::start();
        let task = tokio::spawn(async move { active.accept(commit, &deadline).await });
        pause.wait_until_entered().await;
        task.abort();
        assert!(matches!(task.await, Err(error) if error.is_cancelled()));
        assert!(matches!(
            harness.proposal_state(TransactionKind::Credential, "github", "work", proposal),
            Some(ProposalState::Active)
        ));
        harness
            .coordinator
            .recover()
            .await
            .expect("durable decision rolls forward after cancellation");
        assert!(
            Ceremony::new(harness.state.clone(), harness.coordinator.clone())
                .recover()
                .is_ok(),
            "public metadata recovery"
        );
        assert!(matches!(
            harness.proposal_state(TransactionKind::Credential, "github", "work", proposal),
            Some(ProposalState::Committed(_))
        ));

        harness.finish();
    }
}
