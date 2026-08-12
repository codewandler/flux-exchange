//! How a credential was **obtained**, and the named weaknesses in obtaining it that way.
//!
//! Every credential this service holds today arrived the same way: a human pasted it in. That is
//! `connector_catalog::Acquisition::Static` — *the stored secret, unchanged* — and it is the reason
//! nothing in this crate has ever had to describe an acquisition. A connector that mints its
//! credential by presenting the resource owner's own password does have to be described, because
//! the presenting is a weakness a deployment must be able to refuse.
//!
//! This module carries both the refusal vocabulary and the transport-free acquisition port.
//! [`AuthHazard`] is the word the filter is written against; [`CredentialAcquirer`] is the
//! secret-in/secret-out boundary a composition binds without giving this crate an HTTP client.
//! `docs/designs/credential-acquisition.md` is the argument.
//!
//! # Why a deployment refuses a property and not a connector
//!
//! `AGENTS.md` § Invariants: **grants select by declared metadata, not by name.** An operator who
//! wants no password-grant authentication anywhere should say that once, about a declared property,
//! and have every connector carrying it refuse — including the fifty-fifth provider, added next
//! month by somebody who never read their policy. The alternative is a list of connector names,
//! which is correct on the day it is written and silently wrong afterwards.

use std::collections::BTreeSet;

use connector_secrets::Secret;
use serde::{Deserialize, Serialize};

use crate::async_trait;

/// The resource-owner credentials presented once to an acquisition performer.
///
/// Both values are already secret-shaped at the port boundary. This type deliberately has no
/// serialisation implementation and its `Debug` output names only the fields, so a composition
/// cannot accidentally turn the password back into ordinary diagnostic data.
pub struct PasswordRedemption<'a> {
    username: &'a Secret,
    password: &'a Secret,
}

impl<'a> PasswordRedemption<'a> {
    /// Construct a one-use password redemption.
    pub const fn new(username: &'a Secret, password: &'a Secret) -> Self {
        Self { username, password }
    }

    /// The resource-owner identifier, exposed only to the bound performer.
    pub fn username(&self) -> &str {
        self.username.expose_secret()
    }

    /// The resource-owner password, exposed only to the bound performer.
    pub fn password(&self) -> &str {
        self.password.expose_secret()
    }
}

impl std::fmt::Debug for PasswordRedemption<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordRedemption")
            .field("username", &"[REDACTED]")
            .field("password", &"[REDACTED]")
            .finish()
    }
}

/// A refresh token presented to the same acquisition performer that minted the access token.
pub struct RefreshRedemption<'a> {
    refresh_token: &'a Secret,
}

impl<'a> RefreshRedemption<'a> {
    /// Construct a one-use refresh redemption.
    pub const fn new(refresh_token: &'a Secret) -> Self {
        Self { refresh_token }
    }

    /// The refresh token, exposed only to the bound performer.
    pub fn refresh_token(&self) -> &str {
        self.refresh_token.expose_secret()
    }
}

impl std::fmt::Debug for RefreshRedemption<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshRedemption")
            .field("refresh_token", &"[REDACTED]")
            .finish()
    }
}

/// The two halves of one delegated authorization, presented once to an acquisition performer.
///
/// **Both are secrets, and the second is the one that is easy to get wrong.** The authorization
/// code came back through a browser, so anything that saw that URL has it; the code verifier never
/// left the host process and is what proves the redeemer is the party that opened the request
/// (RFC 7636). A verifier in a log line is an authorization code anyone reading the log can spend,
/// so this type redacts exactly as [`PasswordRedemption`] does.
///
/// What is deliberately **not** here: the authorization endpoint, the redirect URI, the client
/// identifier and the scopes. Every one of them is deployment configuration or connector
/// declaration owned by the composing binary, and a port that carried them would be this crate
/// describing a browser flow it does not perform.
pub struct AuthorizationCodeRedemption<'a> {
    code: &'a Secret,
    verifier: &'a Secret,
}

impl<'a> AuthorizationCodeRedemption<'a> {
    /// Construct a one-use delegated redemption.
    pub const fn new(code: &'a Secret, verifier: &'a Secret) -> Self {
        Self { code, verifier }
    }

    /// The authorization code, exposed only to the bound performer.
    pub fn code(&self) -> &str {
        self.code.expose_secret()
    }

    /// The proof-key verifier this host drew when it opened the request.
    pub fn verifier(&self) -> &str {
        self.verifier.expose_secret()
    }
}

impl std::fmt::Debug for AuthorizationCodeRedemption<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorizationCodeRedemption")
            .field("code", &"[REDACTED]")
            .field("verifier", &"[REDACTED]")
            .finish()
    }
}

/// The secret material returned by a credential acquisition.
///
/// `expires_at` is an absolute Unix timestamp reported or derived by the concrete performer. It is
/// intentionally not a requested lifetime: this port gives the connector no generic TTL knob.
pub struct AcquiredCredential {
    access_token: Secret,
    refresh_token: Option<Secret>,
    expires_at: Option<i64>,
}

impl AcquiredCredential {
    /// Construct the result returned by a concrete performer.
    pub const fn new(
        access_token: Secret,
        refresh_token: Option<Secret>,
        expires_at: Option<i64>,
    ) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at,
        }
    }

    /// Consume the result into the values a composition persists atomically.
    pub fn into_parts(self) -> (Secret, Option<Secret>, Option<i64>) {
        (self.access_token, self.refresh_token, self.expires_at)
    }
}

impl std::fmt::Debug for AcquiredCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquiredCredential")
            .field("access_token", &"[REDACTED]")
            .field(
                "refresh_token",
                &self.refresh_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

/// A server-owned performer that exchanges one secret for another.
///
/// The port has no endpoint, HTTP method, account identifier or requested lifetime. Those are
/// transport and endpoint quirks owned by the composing server, never connector-independent host
/// vocabulary.
#[async_trait]
pub trait CredentialAcquirer: Send + Sync {
    /// A connector identity structurally fixed by this performer, when it has one.
    ///
    /// Generic product bindings may return `None`. A vendor-specific performer returns `Some` so
    /// the composing registry can refuse attaching its quirks to a different connector.
    fn binding_connector(&self) -> Option<&str> {
        None
    }

    /// Redeem a resource-owner password once.
    async fn redeem_password(
        &self,
        redemption: PasswordRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal>;

    /// Rotate an acquired credential through its refresh token.
    async fn redeem_refresh(
        &self,
        redemption: RefreshRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal>;

    /// Redeem one authorization code a person granted through their own browser.
    ///
    /// The delegated half of the acquisition port. The host still sees only secrets in and secrets
    /// out: composing the authorization URL, planting the browser binding, spending the `state` and
    /// comparing the redirect URI all belong to the composing binary, which is where the browser
    /// and the transport already are.
    ///
    /// # Why this has a body, when the two above do not
    ///
    /// `codewandler-flux-exchange-host` is published, so a **required** method here would break
    /// every downstream performer at the version that added it — including the ones bound to
    /// connectors whose acquisition carries no delegated grant at all and which would have to write
    /// this exact refusal by hand. The default is that refusal, stated once: a performer that does
    /// not perform the grant says so, by name, before anything is sent.
    ///
    /// This is not a silent fallback. [`AcquisitionRefusal::GrantNotPerformed`] is a refusal like
    /// every other variant here — nothing is attempted, nothing is stored, and a composition that
    /// meant to perform the grant learns at the first attempt rather than at a vendor's `400`.
    ///
    /// # Errors
    ///
    /// [`AcquisitionRefusal::GrantNotPerformed`] unless a performer overrides this, and whatever
    /// the vendor leg refuses with when one does.
    async fn redeem_authorization_code(
        &self,
        _redemption: AuthorizationCodeRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        // Never read. The refusal is decided from the performer alone, so a default body that
        // touched the code or the verifier would be one that could log or store either.
        Err(AcquisitionRefusal::GrantNotPerformed)
    }
}

/// A safe, value-free reason why an admitted acquisition did not produce a credential.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AcquisitionRefusal {
    /// The vendor requires a multi-step authentication flow this acquisition cannot represent.
    #[error("the credential endpoint requires multi-factor authentication")]
    MfaRequired,
    /// The vendor rejected the presented credential.
    #[error("the credential endpoint rejected the presented credential")]
    CredentialsRejected,
    /// The vendor refused the request for a reason that is not safe to echo.
    #[error("the credential endpoint rejected the acquisition")]
    VendorRejected,
    /// The endpoint could not be reached.
    #[error("the credential endpoint is unreachable")]
    Unreachable,
    /// A successful response omitted or malformed required credential material.
    #[error("the credential endpoint returned an invalid credential response")]
    InvalidResponse,
    /// A refresh endpoint answered success but did not return enough state to commit its rotation.
    ///
    /// The old refresh token may already be invalid at the vendor. This is deliberately distinct
    /// from an ordinary invalid response so a composition cannot advise retrying the spent token.
    #[error("the refresh endpoint may have rotated the credential, but returned unusable state")]
    RefreshOutcomeUnusable,

    /// The bound performer does not perform the grant this acquisition needs.
    ///
    /// Decided **here**, before any request leaves the process, so it is a composition fault an
    /// operator fixes rather than a vendor rejection they would re-enter a credential for. It is
    /// what [`CredentialAcquirer::redeem_authorization_code`]'s default body answers with, and it
    /// names no grant: the connector and the acquisition the composition bound are what an operator
    /// looks at, and neither is this crate's to quote.
    #[error("the bound acquisition performer does not perform this grant")]
    GrantNotPerformed,
}

impl AcquisitionRefusal {
    /// The stable machine-readable reason returned by an HTTP composition.
    pub const fn code(self) -> &'static str {
        match self {
            Self::MfaRequired => "mfa_required",
            Self::CredentialsRejected => "credentials_rejected",
            Self::VendorRejected => "vendor_rejected",
            Self::Unreachable => "credential_endpoint_unreachable",
            Self::InvalidResponse => "invalid_credential_response",
            Self::RefreshOutcomeUnusable => "refresh_outcome_unusable",
            Self::GrantNotPerformed => "grant_not_performed",
        }
    }
}

/// A named weakness in **how a credential is obtained**, declared by the connector.
///
/// A closed set, and the closedness is the feature. The obvious alternative — a free-form
/// `hazard = "..."` string carrying its own citations — is more expressive and strictly worse,
/// because it makes the filter a string match: a near-miss spelling matches no allow-list entry,
/// reads as *no hazard declared*, and is admitted by the very deployment that refused the thing it
/// names. An unknown value here is a refusal at deserialisation instead, which
/// `tests/auth_hazard.rs` is the whole of. The cost is that a new hazard is a deliberate edit to
/// this file, and that cost is the point.
///
/// # This is a *kind*. [`Risk`](crate::Risk) is a *level*
///
/// The two are not interchangeable and the difference is load-bearing, because [`Risk`](crate::Risk)
/// is **ordered** and [`Selector::at_most`](crate::Selector::at_most) compares against that
/// ordering. A hazard has no position on that ladder: a password grant that buys a **read-only**
/// token is `Risk::Low` *and* hazardous. A fifth rung is therefore wrong in one direction or the
/// other — high enough to catch it and every destructive operation inherits a weakness it does not
/// have; low enough not to and `at_most(Risk::High)` silently admits password-grant authentication
/// to every grant an operator has already written.
///
/// It is also deliberately **not** a field on [`OperationFacts`](crate::OperationFacts). A hazard is
/// a property of an *acquisition*, which happens once per connection; an operation happens per call.
/// Putting it there would restate one connection's fact on 389 rows and invite a per-call answer to
/// a per-connection question.
///
/// # `#[non_exhaustive]`, from the start
///
/// A second hazard is a matter of when, not whether, and adding one must not be a breaking change
/// for a published crate. Marking it at birth is free; retrofitting it is itself the break it was
/// meant to prevent.
///
/// **That is not licence for a wildcard arm inside this crate.** `#[non_exhaustive]` binds
/// *consumers*; a match written here is still exhaustive with no `_`, exactly as
/// [`OperationFacts::of`](crate::OperationFacts::of) argues for the vocabularies it mirrors. When
/// upstream declares a hazard and this crate maps it, a catch-all would answer a hazard it had never
/// heard of with a plausible wrong one — and the filter would then admit it without anybody having
/// decided to.
///
/// # Ordering
///
/// `Ord` is derived so a set of hazards can be a `BTreeSet`, which is the shape an allow-list wants,
/// and for **no other reason**. It is declaration order, it is not severity, and nothing may read it
/// as a ladder — that is the mistake the section above exists to prevent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthHazard {
    /// The resource owner's own password is presented to **this host** rather than to the
    /// authorization server.
    ///
    /// The OAuth 2.0 resource owner password credentials grant. The name states the property a
    /// filter is written against and an auditor can check — *the resource owner's secret was
    /// shared* — rather than naming the grant that happens to be the shipped instance of it.
    ///
    /// **RFC 9700 §2.4** (Best Current Practice for OAuth 2.0 Security, 2025) — the grant
    /// **MUST NOT** be used. Three stated reasons, and together they are the whole hazard:
    ///
    /// 1. it exposes the resource owner's credentials to the client;
    /// 2. it widens where those credentials can leak, beyond the authorization server;
    /// 3. it cannot carry two-factor or any other multi-step authentication.
    ///
    /// **RFC 6749 §4.3** — the client MUST discard those credentials once an access token is
    /// obtained. Here the client is this host, so that MUST is ours: the password is never stored,
    /// never logged, and never in an error body.
    ///
    /// **CWE-522**, *Insufficiently Protected Credentials*, is the nearest weakness-catalogue entry,
    /// and is what an auditor's tooling will be looking for.
    ///
    /// OAuth 2.1 drops the grant entirely. It remains worth supporting behind an explicit opt-in
    /// because a vendor that offers nothing else offers this or nothing — which is a decision for a
    /// deployment to make knowingly, and the reason this type exists rather than a refusal being
    /// hard-coded.
    ResourceOwnerSecretShared,
}

impl AuthHazard {
    /// The stable configuration spelling of this hazard.
    ///
    /// Matched exhaustively here rather than obtained by serialising the enum. Deployment policy
    /// is configuration an operator audits, and changing a serde helper must not silently change
    /// the string that grants a weaker authentication path.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ResourceOwnerSecretShared => "resource_owner_secret_shared",
        }
    }
}

impl std::fmt::Display for AuthHazard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A deployment's startup-selected policy over declared authentication hazards.
///
/// The default refuses every hazard. A composition may explicitly construct a posture from typed
/// hazards it read at startup; there is deliberately no method that takes a request, header, body,
/// connector id or other caller-controlled policy input. The connector id reaches only
/// [`admit`](Self::admit), where it makes a refusal actionable and never changes the decision.
///
/// Admission is evaluated when an acquisition is attempted, not when a catalogue is loaded. That
/// is what keeps the rule true after a running deployment learns a connector declaration it did
/// not have at boot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AuthPosture {
    allowed: BTreeSet<AuthHazard>,
}

impl AuthPosture {
    /// The production default: no declared authentication hazard is allowed.
    pub fn fail_closed() -> Self {
        Self::default()
    }

    /// Construct the policy an operator explicitly selected at startup.
    ///
    /// This takes only closed, typed [`AuthHazard`] values. Parsing deployment configuration and
    /// refusing unknown spellings belongs to the composition, before it constructs this value.
    pub fn allowing(hazards: impl IntoIterator<Item = AuthHazard>) -> Self {
        Self {
            allowed: hazards.into_iter().collect(),
        }
    }

    /// Decide one connector acquisition from the hazard the connector declares.
    ///
    /// `connector` is carried only into the refusal. A deployment opts in to a property once,
    /// never to a list of connector names, so a newly catalogued connector inherits the same rule.
    ///
    /// # Errors
    ///
    /// [`AuthPostureRefusal::HazardNotAllowed`] when this deployment did not explicitly allow the
    /// declared hazard.
    pub fn admit(&self, connector: &str, hazard: AuthHazard) -> Result<(), AuthPostureRefusal> {
        if self.allowed.contains(&hazard) {
            return Ok(());
        }

        Err(AuthPostureRefusal::HazardNotAllowed {
            connector: connector.to_owned(),
            hazard,
        })
    }
}

/// Why a credential acquisition was refused by deployment policy.
///
/// Separate from a vendor rejection: this host made the decision before any request left the
/// process, so telling an operator to re-enter a correct password would be the wrong remedy.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthPostureRefusal {
    /// The connector declared a hazard this deployment did not opt into.
    #[error(
        "connector `{connector}` declares authentication hazard `{hazard}`, and this deployment did \
         not allow it; no acquisition request was sent"
    )]
    HazardNotAllowed {
        /// The connector whose acquisition was attempted.
        connector: String,
        /// The declared hazard that was not allowed.
        hazard: AuthHazard,
    },
}

impl AuthPostureRefusal {
    /// The HTTP status this refusal carries when a composition exposes acquisition over HTTP.
    ///
    /// Kept on the refusal so two acquisition surfaces cannot answer the same policy decision
    /// differently. `403` says the deployment forbade a declared path. A vendor rejecting
    /// otherwise admitted credentials is a later, separate refusal and must not use this status.
    pub const fn status(&self) -> u16 {
        match self {
            Self::HazardNotAllowed { .. } => 403,
        }
    }
}
