//! The authorization requests this host has opened **on a signed-in person's behalf**, and where
//! the credential they authorize is kept.
//!
//! X-75 gave this composition an acquisition it can perform with a secret the person types. X-147
//! is the other half: the person authorizes at the vendor, in their own browser, with their own
//! account, and this host never sees the password at all. What comes back through the browser is an
//! authorization code, and everything in this module exists to make sure the code that comes back
//! belongs to a request this host opened, for that browser, on behalf of that principal.
//!
//! # Why this is a sibling store and not [`crate::oidc::flow::PendingAuthorizations`]
//!
//! The two are the same shape and answer different questions, and the differences are in the entry
//! rather than in the mechanism:
//!
//! - Sign-in's [`Pending`](crate::oidc::flow::Pending) carries a **`nonce`**, which exists to tie an
//!   OIDC id token to the request that asked for it. A connector grant returns no id token and has
//!   nothing to echo one, so generalising that type would mean an `Option<String>` nobody reads on
//!   the path where it matters most — and the field's documentation would have to stop saying what
//!   it says.
//! - This one carries a **[`Principal`]**, which sign-in cannot have: obtaining one is what sign-in
//!   is for. It is what decides the address the acquired credential is written to, so it is not
//!   optional here either.
//!
//! What is **not** duplicated is the part worth keeping in one place: [`Binder`], its entropy
//! source, its redaction and its emptiness rule all come from `oidc::flow`, and so does the
//! `Set-Cookie` formatter.
//!
//! # There is deliberately no `take(state)`-only method
//!
//! Restated here rather than pointed at, because it is the rule a later story reaches past and the
//! pointer is what makes that cheap. [`PendingDelegations::claim`] takes **both** halves or nothing.
//! A `take(state)` beside it would be a path that completes an acquisition without ever asking which
//! browser opened it — and a `state` travels through the vendor and sits in a URL, so an attacker
//! who starts an acquisition here, authorizes at the vendor as themselves, and then walks a victim
//! into the callback would have every server-side check pass. In sign-in that ends with the victim
//! holding the attacker's session, which is the login-CSRF X-15 closed. Here it ends with the
//! **attacker's vendor credential written into the victim's tenant**, at an address the victim's
//! operations then run under — the same attack with the direction of the theft reversed. The only
//! reliable way to stop a later story reaching for the cheaper call is for it not to exist.
//!
//! # Where the credential goes: a reserved service segment per principal
//!
//! `docs/designs/credential-acquisition.md` § "Addressing a delegated credential" carries the
//! decision and the alternative that was rejected. The short form: a delegated token acts **as the
//! person**, so one kept at the tenant-wide address would let any member of the tenant act as any
//! other. [`CredentialRef`] has no principal component and `connector-address` is upstream, so the
//! principal goes in the one segment this host already reserves for state a connector did not
//! declare — the service — as [`DELEGATED_SERVICE_PREFIX`] followed by a digest of the principal.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;
use std::time::Instant;

use exchange_host::{CredentialRef, Principal};
use sha2::{Digest, Sha256};

use crate::oidc::flow::{Binder, FlowError, PENDING_TTL};
use crate::oidc::pkce::{Challenge, Verifier};
use crate::{entropy, session};

/// How many bytes of entropy the delegated `state` carries. 256 bits, from the OS.
const OPAQUE_BYTES: usize = 32;

/// The cookie that ties one pending delegated acquisition to the browser that opened it.
///
/// **Distinct from `crate::oidc::flow::BINDER_COOKIE`, and that is a decision rather than a
/// spelling.** A person connecting a connector is, by construction, already signed in, so a sign-in
/// binder and an acquisition binder can be live in the same browser at the same time. One cookie
/// name would mean the second flow overwrites the first browser's binding: the first callback then
/// arrives with a value that matches nothing and is refused, which reads as a broken sign-in and is
/// really a name collision. Two names cost nothing and the failure they prevent is silent.
///
/// Everything else about it is [`crate::oidc::flow::BINDER_COOKIE`]'s argument unchanged — the
/// `__Host-` prefix so only this host can set it, `HttpOnly` so script cannot lift it, and
/// `SameSite=Lax` because its whole job is to survive exactly one cross-site navigation.
pub const BINDER_COOKIE: &str = "__Host-flux_exchange_acquire";

/// The reserved service segment a delegated credential lives under, before its principal digest.
///
/// It sits beside `exchange-acquisition` — the reserved service X-75 already keeps a connection's
/// refresh token and expiry in — and it is reserved the same way: a connector declares services
/// under its own vendor vocabulary and cannot declare one starting `exchange-`.
pub const DELEGATED_SERVICE_PREFIX: &str = "exchange-delegated-";

/// How many hex characters of the principal digest the service segment carries.
///
/// 32, so 128 bits of SHA-256. What that has to resist is a **second preimage**: some other
/// principal of the same tenant whose digest equals this one, because two principals at one address
/// is exactly the thing this addressing exists to prevent. 128 bits is not a number anybody reaches
/// by search, and the ids being hashed are minted by the identity provider rather than chosen by a
/// caller, so there is no grinding position to attack from either. The rest is path length, which
/// every credential of every tenant pays.
const DIGEST_HEX: usize = 32;

/// The most delegated acquisitions **one tenant** will hold open at once.
///
/// This bound is per tenant, and X-147's first review is why. A single global bound refuses the
/// newest — which is right here, unlike sign-in's store, because this one is filled only by
/// authenticated humans and the caller an eviction would cost is a colleague half way through
/// authorizing a vendor account. But *whose* newest it refuses matters just as much: with one global
/// counter, one signed-in person looping 256 times makes every other tenant's members wait out the
/// [`PENDING_TTL`], which is one tenant buying a denial of service against the rest of the
/// deployment for the price of a session. A tenant filling its own bound is a tenant refusing itself,
/// which is the blast radius every other bound in this host already has.
///
/// The tenant comes from the resolved principal, as everywhere else, so it is not something a caller
/// can spread its load across.
const MAX_PENDING_PER_TENANT: usize = 64;

/// The most delegated acquisitions this host will hold open across every tenant.
///
/// The memory ceiling, and deliberately not the fairness rule — [`MAX_PENDING_PER_TENANT`] is that.
/// It exists because the per-tenant bound alone is unbounded in the number of tenants, and a
/// multi-tenant deployment's memory is not a per-tenant quantity. Reaching it is an operational
/// event rather than an attack by one caller: it takes as many tenants as the quotient of the two.
const MAX_PENDING: usize = 4_096;

/// One delegated acquisition this host opened.
pub struct PendingDelegation {
    /// The connector whose credential this acquisition mints. Bound at `begin` and never read from
    /// the callback, so a callback cannot redirect a credential into another connector's address.
    connector: String,
    /// **The principal that asked**, and the only thing that decides where the credential lands.
    ///
    /// A [`Principal`] and not a session handle, deliberately. There is no session handle every
    /// caller has: a person signed in through `LocalUsers` or a service-account bearer holds no
    /// [`SessionToken`](crate::session::SessionToken), so a store keyed on one would work for the
    /// OIDC deployment and silently exclude the others. Every guarded route has
    /// `Extension<Principal>` from `routes::require_principal`, and a principal carries its tenant.
    principal: Principal,
    /// The PKCE verifier bound to it. Never leaves this process; see [`crate::oidc::pkce`].
    verifier: Verifier,
    /// The browser that opened it, which the callback must present. See [`BINDER_COOKIE`].
    ///
    /// It lives here rather than in a store of its own, for the reason
    /// [`crate::oidc::flow::Pending`] records: two stores would need two sweeps and two bounds, and
    /// any drift between them is a window in which a `state` outlives the binder that guards it.
    binder: Binder,
    /// When it was opened, for the expiry sweep.
    opened: Instant,
}

impl PendingDelegation {
    /// The connector this acquisition was opened for.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// The principal that opened it, and the one the credential is addressed to.
    pub fn principal(&self) -> &Principal {
        &self.principal
    }

    /// The verifier the token request has to present.
    pub fn verifier(&self) -> &Verifier {
        &self.verifier
    }
}

impl fmt::Debug for PendingDelegation {
    /// Names the connector and the principal, and neither secret. A `Verifier` redacts itself and a
    /// `Binder` redacts itself; this states it once more at the type that holds both, so a future
    /// field cannot be printed by a derive nobody re-read.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingDelegation")
            .field("connector", &self.connector)
            .field("principal", &self.principal)
            .field("verifier", &"[REDACTED]")
            .field("binder", &"[REDACTED]")
            .finish()
    }
}

/// What the authorize route needs to build the vendor URL and answer the browser.
pub struct BegunDelegation {
    /// The `state` parameter. Spent once, at the callback.
    pub state: String,
    /// The PKCE code challenge — the public half, safe to put in the URL.
    pub challenge: Challenge,
    /// The binder to plant in the browser that asked. **Not a URL parameter**: it never travels
    /// through the vendor, which is the whole point of it.
    pub binder: Binder,
}

/// What a callback's `(state, binder)` pair turned out to be.
///
/// Three outcomes and not two, for [`crate::oidc::flow::Claimed`]'s reason: an operator reading the
/// log needs to tell a callback this host never opened from a callback opened for another browser,
/// and the caller must not be able to tell them apart at all.
pub enum ClaimedDelegation {
    /// This host opened it and the browser presenting it is the one that did. **Spent.**
    Delegation(Box<PendingDelegation>),

    /// No live delegation answers to that `state` — never opened here, already spent, or aged out.
    Unknown,

    /// A live delegation answers to that `state`, but a different browser opened it. **Left
    /// unspent**: it is somebody else's acquisition, and a hostile callback must not be able to
    /// cancel it.
    AnotherBrowser,
}

/// The delegated acquisitions in flight.
#[derive(Default)]
pub struct PendingDelegations {
    live: Mutex<HashMap<String, PendingDelegation>>,
}

impl PendingDelegations {
    /// An empty set.
    pub fn new() -> Self {
        Self::default()
    }

    /// Open a delegated acquisition: draw `state` and a PKCE verifier, and remember them beside the
    /// principal that asked.
    ///
    /// # Errors
    ///
    /// [`FlowError::NoEntropy`] when the operating system's randomness is unreadable — refusing
    /// rather than falling back to a predictable `state` or verifier — and
    /// [`DelegationRefusal::TooManyPending`] when this host is already holding its bound.
    pub fn begin(
        &self,
        connector: &str,
        principal: &Principal,
    ) -> Result<BegunDelegation, DelegationRefusal> {
        let state = entropy::hex::<OPAQUE_BYTES>()
            .map_err(|source| DelegationRefusal::NoFlow(FlowError::NoEntropy { source }))?;
        let verifier = Verifier::generate()
            .map_err(|source| DelegationRefusal::NoFlow(FlowError::NoEntropy { source }))?;
        let challenge = verifier.challenge();
        let binder = Binder::draw()
            .map_err(|source| DelegationRefusal::NoFlow(FlowError::NoEntropy { source }))?;

        let mut live = self.live();
        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);
        // The tenant's own bound first, because it is the one a caller can reach and the one whose
        // refusal is actionable: the person who hits it is the person looping.
        let held_by_tenant = live
            .values()
            .filter(|pending| pending.principal.tenant() == principal.tenant())
            .count();
        if held_by_tenant >= MAX_PENDING_PER_TENANT || live.len() >= MAX_PENDING {
            return Err(DelegationRefusal::TooManyPending);
        }

        live.insert(
            state.clone(),
            PendingDelegation {
                connector: connector.to_owned(),
                principal: principal.clone(),
                verifier,
                binder: binder.clone(),
                opened: Instant::now(),
            },
        );

        Ok(BegunDelegation {
            state,
            challenge,
            binder,
        })
    }

    /// Claim the delegation `state` was bound to, for the browser holding `binder`.
    ///
    /// **Both halves, or nothing**, and **single use**, and **only on the branch that succeeds** —
    /// the module documentation carries why there is no `take(state)` beside this. A binder mismatch
    /// takes nothing; see [`ClaimedDelegation::AnotherBrowser`].
    ///
    /// The sweep happens first, so an expired `state` is a miss rather than a hit: a delegation left
    /// open for the TTL is one the person abandoned, and finishing it would spend an authorization
    /// code that has been sitting in a URL all that time.
    ///
    /// A hash lookup, not a constant-time comparison, on the reasoning
    /// [`SessionStore::resolve`](crate::session::SessionStore::resolve) records: a `state` is 256
    /// bits from the OS, so there is no prefix to walk and no shorter guess to confirm.
    ///
    /// The whole decision happens under **one** guard, so there is no window in which a delegation
    /// is checked against one browser and spent by another.
    pub fn claim(&self, state: &str, binder: &str) -> ClaimedDelegation {
        let mut live = self.live();

        live.retain(|_, pending| pending.opened.elapsed() < PENDING_TTL);

        match live.get(state) {
            None => ClaimedDelegation::Unknown,
            Some(pending) if !pending.binder.matches(binder) => ClaimedDelegation::AnotherBrowser,
            Some(_) => match live.remove(state) {
                Some(pending) => ClaimedDelegation::Delegation(Box::new(pending)),
                // Unreachable while the guard above is held, and `Unknown` is the direction to fail
                // in if it ever stops being: it acquires nothing.
                None => ClaimedDelegation::Unknown,
            },
        }
    }

    /// How many delegations are open. For the tests below.
    #[cfg(test)]
    pub fn open(&self) -> usize {
        self.live().len()
    }

    /// Bring one delegation's deadline forward, so expiry is a state a test can reach rather than
    /// one it has to wait ten minutes for.
    ///
    /// Following [`SessionStore::expire_now`](crate::session::SessionStore::expire_now): the store
    /// still reads its own clock and still makes its own decision, so what is asserted afterwards
    /// is the store's answer and not a clock a test handed it. `false` when the entry is absent, or
    /// when the machine has been up for less than the TTL and cannot represent an instant that old.
    #[cfg(test)]
    pub fn expire_now(&self, state: &str) -> bool {
        let mut live = self.live();
        let Some(entry) = live.get_mut(state) else {
            return false;
        };
        let Some(aged) = entry.opened.checked_sub(PENDING_TTL) else {
            return false;
        };
        entry.opened = aged;
        true
    }

    /// The open delegations.
    ///
    /// Recovers from a poisoned lock rather than propagating it, following
    /// [`crate::oidc::flow::PendingAuthorizations`]: the guarded value has no cross-key invariant,
    /// so a panic while holding the lock cannot have left it half-updated.
    fn live(&self) -> std::sync::MutexGuard<'_, HashMap<String, PendingDelegation>> {
        self.live
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Why a delegated acquisition did not proceed, decided by **this host** before any vendor request.
///
/// Separate from `AcquisitionRefusal`, which is what a credential endpoint's answer becomes. Every
/// variant here is a decision taken with nothing sent, and the split is the one X-75 already drew
/// between `AuthPostureRefusal` and a vendor rejection: an operator responds to them differently.
///
/// **None of these carries a value.** Not the `state`, not the code, not the verifier, not the
/// binder — and the three that describe a callback are deliberately indistinguishable to the caller,
/// following `SignInRefusal::caller_facing`. A caller told which of them happened is a caller being
/// told whether a `state` it invented is one this host is holding.
#[derive(Debug)]
pub enum DelegationRefusal {
    /// This connector has no delegated grant bound in this composition.
    NotDelegable,
    /// This deployment configured no acquisition redirect URI, so there is nowhere to come back to.
    NoRedirectUri,
    /// The callback presented no binder cookie at all.
    NoBinder,
    /// No live delegation answers to the presented `state`.
    UnknownState,
    /// A live delegation answers to it, and a different browser opened it.
    AnotherBrowser,
    /// This host is already holding as many delegations as it will.
    TooManyPending,
    /// The delegation could not be opened at all.
    NoFlow(FlowError),
    /// The acquired credential has no address under the initiating principal.
    Unaddressable(String),
}

impl DelegationRefusal {
    /// The one sentence every callback refusal answers with.
    ///
    /// One phrase for [`NoBinder`](Self::NoBinder), [`UnknownState`](Self::UnknownState) and
    /// [`AnotherBrowser`](Self::AnotherBrowser), exactly as sign-in does: the three are told apart
    /// in the log, where an operator needs them, and nowhere else. A caller that could tell them
    /// apart could probe this host for which `state` values it is holding.
    pub const fn caller_facing(&self) -> &'static str {
        match self {
            Self::NoBinder | Self::UnknownState | Self::AnotherBrowser => {
                "this callback could not be matched to an authorization this host opened for this \
                 browser; start the connection again"
            }
            Self::NotDelegable => {
                "this connector has no delegated authorization grant bound in this deployment"
            }
            Self::NoRedirectUri => {
                "this deployment configured no acquisition redirect URI, so it cannot open a \
                 delegated authorization"
            }
            Self::TooManyPending => {
                "this host is holding as many unfinished authorizations as it will; finish or \
                 abandon one and start again"
            }
            Self::NoFlow(_) | Self::Unaddressable(_) => {
                "this host could not open a delegated authorization"
            }
        }
    }

    /// The stable machine-readable reason, for a console that renders one.
    ///
    /// The three callback refusals share a code for the reason they share a sentence.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoBinder | Self::UnknownState | Self::AnotherBrowser => "authorization_unmatched",
            Self::NotDelegable => "grant_not_bound",
            Self::NoRedirectUri => "redirect_uri_unconfigured",
            Self::TooManyPending => "too_many_pending_authorizations",
            Self::NoFlow(_) | Self::Unaddressable(_) => "authorization_unavailable",
        }
    }
}

impl fmt::Display for DelegationRefusal {
    /// The **operator's** rendering: it separates the three the caller cannot, and still carries no
    /// value from the request.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDelegable => f.write_str("no delegated grant is bound for this connector"),
            Self::NoRedirectUri => f.write_str("no acquisition redirect URI is configured"),
            Self::NoBinder => f.write_str("the callback presented no binder cookie"),
            Self::UnknownState => {
                f.write_str("the callback's state names no delegation this host is holding")
            }
            Self::AnotherBrowser => {
                f.write_str("the callback's state was opened by a different browser")
            }
            Self::TooManyPending => f.write_str("this host is holding its bound of delegations"),
            Self::NoFlow(source) => write!(f, "{source}"),
            Self::Unaddressable(reason) => write!(
                f,
                "the acquired credential has no address under the initiating principal: {reason}"
            ),
        }
    }
}

/// The `Set-Cookie` that plants an acquisition binder. See [`BINDER_COOKIE`].
pub fn planted_binder(binder: &Binder) -> String {
    crate::oidc::flow::planted_binder(BINDER_COOKIE, binder)
}

/// The `Set-Cookie` that clears a spent acquisition binder.
pub fn cleared_binder() -> String {
    crate::oidc::flow::cleared_binder(BINDER_COOKIE)
}

/// The acquisition binder a request presented, if it carried one.
pub fn presented_binder(cookie_header: Option<&str>) -> Option<&str> {
    cookie_header
        .and_then(|header| session::from_cookie_header(header, BINDER_COOKIE))
        .filter(|binder| !binder.is_empty())
}

/// The reserved service segment one principal's delegated credentials live under.
///
/// Derived at every use from the resolved principal, so there is nothing stored to go stale and
/// nothing for a caller to name: a person who never authorized simply has no address, and one who
/// did has exactly one.
///
/// # Why a digest and not the principal's own id
///
/// The service grammar `connector-address` enforces is lowercase ASCII letters, digits and `-`. A
/// principal id is an OIDC `sub`, an email address or a roster handle — `alice@acme`,
/// `114982…|auth0`, `user:alice@dev` — and none of those fits it. Refusing every principal whose id
/// is not already an address segment would refuse most real deployments, and rewriting one into the
/// grammar is a lossy transform, which is the same thing as a collision. A fixed-length lowercase
/// hex digest fits by construction and cannot collide with the reserved prefix in front of it.
///
/// The tenant is hashed in even though the address already carries it, and the kind with it: two
/// principals differing only in kind, or in tenant, must be two addresses, and paying for that in
/// the digest costs nothing and removes the question.
pub fn delegated_service(principal: &Principal) -> String {
    // Length-prefixed rather than delimiter-joined. A separator that can appear inside a component
    // makes `("ab", "c")` and `("a", "bc")` the same input, which is a collision manufactured by the
    // encoding rather than by the hash.
    let mut digest = Sha256::new();
    digest.update(b"flux-exchange:delegated-credential:v1");
    for component in [
        principal.kind().to_string(),
        principal.id().to_owned(),
        principal.tenant().as_str().to_owned(),
    ] {
        digest.update((component.len() as u64).to_be_bytes());
        digest.update(component.as_bytes());
    }

    let mut service = String::with_capacity(DELEGATED_SERVICE_PREFIX.len() + DIGEST_HEX);
    service.push_str(DELEGATED_SERVICE_PREFIX);
    for byte in digest.finalize().iter().take(DIGEST_HEX / 2) {
        service.push_str(&format!("{byte:02x}"));
    }
    service
}

/// Where one principal's delegated access credential for one connector lives.
///
/// # Errors
///
/// The reason `connector-address` refused a component. A tenant or authority that cannot be an
/// address segment is refused rather than rewritten, which is `AGENTS.md`'s *refuse; never repair*
/// at the one place a credential's address is decided.
pub fn delegated_address(
    principal: &Principal,
    authority: &str,
    credential: &str,
) -> Result<CredentialRef, DelegationRefusal> {
    CredentialRef::new(
        principal.tenant().as_str(),
        authority,
        &delegated_service(principal),
        credential,
    )
    .map_err(DelegationRefusal::Unaddressable)
}

/// The refresh token, expiry and managed marker that travel with one delegated access credential.
///
/// The same three leaves X-75 keeps beside a connection-wide acquired credential, under the
/// principal's own reserved service rather than under `exchange-acquisition` — which is the whole
/// point: one shared companion address would put every member's refresh token in one slot, and the
/// last person to authorize would silently overwrite the rest.
pub struct DelegatedCompanions {
    /// The refresh token, when the vendor issued one.
    pub refresh: CredentialRef,
    /// The absolute expiry the vendor reported, as seconds.
    pub expiry: CredentialRef,
    /// The marker that says this address is vendor-managed and must not be pasted over.
    pub managed: CredentialRef,
}

/// The leaves [`DelegatedCompanions`] reserves inside a principal's delegated service.
///
/// A connector that declared a credential by one of these names would address the same path as this
/// host's own state. It is refused rather than accommodated: `delegated_companions` says so by name
/// before anything is written, so the collision is a composition fault an operator reads once and
/// never a value silently overwritten.
pub const RESERVED_LEAVES: [&str; 3] = ["refresh_token", "expires_at", "managed"];

/// Derive the three companion addresses beside one delegated access credential.
///
/// # Errors
///
/// [`DelegationRefusal::Unaddressable`] when the bound credential's own name is one of
/// [`RESERVED_LEAVES`], or when a component is not an address segment.
pub fn delegated_companions(
    access: &CredentialRef,
) -> Result<DelegatedCompanions, DelegationRefusal> {
    if RESERVED_LEAVES.contains(&access.credential()) {
        return Err(DelegationRefusal::Unaddressable(format!(
            "the bound credential is named {:?}, which this host reserves for the delegated \
             acquisition state kept beside it",
            access.credential(),
        )));
    }
    let build = |leaf: &str| {
        CredentialRef::new(access.tenant(), access.authority(), access.service(), leaf)
            .map_err(DelegationRefusal::Unaddressable)
    };
    Ok(DelegatedCompanions {
        refresh: build(RESERVED_LEAVES[0])?,
        expiry: build(RESERVED_LEAVES[1])?,
        managed: build(RESERVED_LEAVES[2])?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeSet;

    use exchange_host::{PrincipalKind, Tenant};

    fn member(id: &str) -> Principal {
        Principal::new(
            PrincipalKind::User,
            id,
            Tenant::new("acme").expect("a literal tenant"),
        )
    }

    fn claim_as(pending: &PendingDelegations, begun: &BegunDelegation) -> ClaimedDelegation {
        pending.claim(&begun.state, begun.binder.as_str())
    }

    /// **The whole of what a callback has to get past, one case at a time.**
    ///
    /// Every one of them is refused with the store untouched, which is the property that matters
    /// second: a hostile callback that *consumed* somebody's delegation would be a denial of service
    /// against the person half way through authorizing a vendor account.
    #[test]
    fn an_unknown_expired_used_or_foreign_state_claims_nothing() {
        let pending = PendingDelegations::new();
        let alice = member("alice");
        let bob = member("bob");

        let hers = pending.begin("gitlab", &alice).expect("randomness");
        let his = pending.begin("gitlab", &bob).expect("randomness");

        // 1. A `state` this host never issued, presented with a binder this host really drew.
        assert!(
            matches!(
                pending.claim("a-state-this-host-never-issued", hers.binder.as_str()),
                ClaimedDelegation::Unknown
            ),
            "a forged state must not claim anybody's delegation",
        );

        // 2. A `state` this host *is* holding, presented with no binder at all.
        assert!(
            matches!(
                pending.claim(&hers.state, ""),
                ClaimedDelegation::AnotherBrowser
            ),
            "a callback presenting no binder must not claim a delegation",
        );

        // 3. **The one this binding exists for.** A genuine, unspent `state` belonging to Alice,
        //    presented by Bob's browser. Every server-side check on the value passes — it is real
        //    and it is live — and it is refused because the browser presenting it is not the one it
        //    was issued to. Without this, Bob walks Alice into a callback carrying Bob's own
        //    authorization code and Bob's vendor credential lands at Alice's address.
        assert!(
            matches!(
                pending.claim(&hers.state, his.binder.as_str()),
                ClaimedDelegation::AnotherBrowser
            ),
            "a delegation opened by one member must not be claimable by another's browser",
        );

        assert_eq!(
            pending.open(),
            2,
            "none of those refusals may spend a delegation",
        );

        // 4. Already used. The successful claim spends it, and the replay finds nothing — the same
        //    answer a forged state gets, which is what makes the two indistinguishable from outside.
        let ClaimedDelegation::Delegation(claimed) = claim_as(&pending, &hers) else {
            panic!("the browser that opened it completes it");
        };
        assert_eq!(claimed.principal(), &alice, "and it names who opened it");
        assert_eq!(claimed.connector(), "gitlab");
        assert!(
            matches!(claim_as(&pending, &hers), ClaimedDelegation::Unknown),
            "a state must not be spendable twice",
        );

        // 5. Expired. Aged rather than waited for, following `oidc::flow`'s own tests.
        {
            let mut live = pending.live();
            let entry = live.get_mut(&his.state).expect("the delegation is open");
            let Some(aged) = entry.opened.checked_sub(PENDING_TTL) else {
                // A machine up for less than the TTL cannot represent an instant that old.
                return;
            };
            entry.opened = aged;
        }
        assert!(
            matches!(claim_as(&pending, &his), ClaimedDelegation::Unknown),
            "an expired delegation must not be completable",
        );
    }

    /// A `state` and a binder are what tie a callback to the request that asked for it, so a repeat
    /// of either would let one acquisition be finished by another's callback.
    #[test]
    fn every_state_and_binder_is_distinct_and_the_binder_never_travels() {
        let pending = PendingDelegations::new();
        let alice = member("alice");

        let mut states = BTreeSet::new();
        let mut binders = BTreeSet::new();
        for _ in 0..64 {
            let begun = pending.begin("gitlab", &alice).expect("randomness");
            states.insert(begun.state);
            binders.insert(begun.binder.as_str().to_owned());
        }

        assert_eq!(states.len(), 64, "a repeated state is a forgeable callback");
        assert_eq!(binders.len(), 64);
        // The one that matters most: a binder equal to the `state` it guards would travel through
        // the vendor in the redirect URL, and every party that saw it would hold the binder too.
        assert!(
            binders.is_disjoint(&states),
            "the binder must not be derived from anything that goes in the authorization URL",
        );
        for value in states.iter().chain(&binders) {
            assert_eq!(value.len(), OPAQUE_BYTES * 2, "256 bits, hex encoded");
        }
    }

    /// Each delegation keeps its own verifier, so the code redeemed at the callback is proved
    /// against the challenge that was sent with *that* authorization request.
    #[test]
    fn each_delegation_keeps_its_own_verifier() {
        let pending = PendingDelegations::new();
        let alice = member("alice");

        let one = pending.begin("gitlab", &alice).expect("randomness");
        let two = pending.begin("gitlab", &alice).expect("randomness");

        let (ClaimedDelegation::Delegation(first), ClaimedDelegation::Delegation(second)) =
            (claim_as(&pending, &one), claim_as(&pending, &two))
        else {
            panic!("each request is claimed by the browser that opened it");
        };

        assert_ne!(first.verifier(), second.verifier());
        assert_eq!(
            first.verifier().challenge(),
            one.challenge,
            "the challenge sent to the vendor must be the one this verifier answers",
        );
        assert_eq!(second.verifier().challenge(), two.challenge);
    }

    /// **The addressing acceptance, as one assertion.** Two members of one tenant, one connector,
    /// two addresses — and neither is the address the other's operations resolve.
    #[test]
    fn one_member_cannot_resolve_another_members_delegated_credential() {
        let alice = member("alice");
        let bob = member("bob");

        let hers = delegated_address(&alice, "com.gitlab.api", "access_token").expect("address");
        let his = delegated_address(&bob, "com.gitlab.api", "access_token").expect("address");

        assert_ne!(
            hers, his,
            "two members' delegated credentials must not share an address; a user-subject token \
             kept tenant-wide is one member acting as another",
        );
        assert_eq!(hers.tenant(), his.tenant(), "and both stay inside a tenant");
        assert_eq!(hers.credential(), his.credential());

        // The address is a *function of the principal*, so resolving it needs the principal and
        // nothing a caller supplies. Re-deriving Alice's twice gives Alice's; there is no input
        // Bob could present that produces it.
        assert_eq!(
            hers,
            delegated_address(&alice, "com.gitlab.api", "access_token").expect("address"),
        );

        // And the kind is in the digest, so a service account that somehow shared an id with a
        // human does not share a credential with them.
        let impostor = Principal::new(
            PrincipalKind::ServiceAccount,
            "alice",
            Tenant::new("acme").expect("a literal tenant"),
        );
        assert_ne!(
            hers,
            delegated_address(&impostor, "com.gitlab.api", "access_token").expect("address"),
        );
    }

    /// The reserved service is one segment, in the grammar `connector-address` enforces, and it
    /// renders into a path that parses back to the address it came from.
    #[test]
    fn a_delegated_service_is_a_legal_address_segment() {
        let service = delegated_service(&member("alice@example.test"));

        assert!(service.starts_with(DELEGATED_SERVICE_PREFIX), "{service}");
        assert_eq!(service.len(), DELEGATED_SERVICE_PREFIX.len() + DIGEST_HEX);
        assert!(
            service
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "{service}",
        );
        // The principal's own spelling must not survive into the path. It is not a secret, but a
        // store path is copied into logs, backups and support tickets, and an email address there
        // is personal data this host had no reason to publish.
        assert!(!service.contains("example"), "{service}");
    }

    /// A connector whose declared credential collides with the state kept beside it is refused by
    /// name, rather than having its value silently overwritten by a refresh token.
    #[test]
    fn a_credential_named_like_the_reserved_state_is_refused() {
        let alice = member("alice");

        for reserved in RESERVED_LEAVES {
            let access =
                delegated_address(&alice, "com.gitlab.api", reserved).expect("a legal address");
            let refusal = delegated_companions(&access)
                .err()
                .unwrap_or_else(|| panic!("{reserved} must be refused"));
            assert!(
                format!("{refusal}").contains(reserved),
                "the refusal names the collision: {refusal}",
            );
        }

        let access = delegated_address(&alice, "com.gitlab.api", "access_token").expect("address");
        let companions = delegated_companions(&access).expect("companions");
        assert_eq!(companions.refresh.service(), access.service());
        assert_ne!(companions.refresh, companions.expiry);
    }

    /// **A tenant that fills its bound refuses itself, and nobody else** (X-147 review, M3).
    ///
    /// Two claims in one run, and the second is the one the review was filed for. A full tenant
    /// refuses its own next delegation rather than evicting a live one — the opposite of
    /// `oidc::flow`, on [`MAX_PENDING_PER_TENANT`]'s argument, because the caller an eviction would
    /// cost is a colleague half way through authorizing a vendor account. And a **different**
    /// tenant is unaffected, which is what a single global counter got wrong: one signed-in person
    /// looping made every other tenant's members wait out the TTL.
    #[test]
    fn a_tenant_that_fills_its_bound_refuses_itself_and_not_another_tenant() {
        let pending = PendingDelegations::new();
        let alice = member("alice");
        let outsider = Principal::new(
            PrincipalKind::User,
            "carol",
            Tenant::new("globex").expect("a second tenant"),
        );

        let first = pending.begin("gitlab", &alice).expect("randomness");
        for _ in 1..MAX_PENDING_PER_TENANT {
            pending.begin("gitlab", &alice).expect("randomness");
        }
        assert_eq!(pending.open(), MAX_PENDING_PER_TENANT);

        assert!(
            matches!(
                pending.begin("gitlab", &alice),
                Err(DelegationRefusal::TooManyPending)
            ),
            "a tenant at its bound refuses its own newest",
        );
        assert!(
            matches!(
                pending.begin("gitlab", &member("bob")),
                Err(DelegationRefusal::TooManyPending)
            ),
            "and the bound is the tenant's, so a colleague shares it",
        );

        // The assertion this test exists for: another tenant is not paying for `acme`'s loop.
        assert!(
            pending.begin("gitlab", &outsider).is_ok(),
            "one tenant filling its bound must not lock another tenant out of connecting",
        );

        assert!(
            matches!(claim_as(&pending, &first), ClaimedDelegation::Delegation(_)),
            "and the oldest is still completable",
        );
    }

    /// No refusal a caller sees separates the three callback failures, and none of them carries a
    /// value from the request.
    #[test]
    fn a_callback_refusal_tells_the_caller_nothing_it_did_not_send() {
        let refusals = [
            DelegationRefusal::NoBinder,
            DelegationRefusal::UnknownState,
            DelegationRefusal::AnotherBrowser,
        ];

        let phrases: BTreeSet<&str> = refusals
            .iter()
            .map(DelegationRefusal::caller_facing)
            .collect();
        let codes: BTreeSet<&str> = refusals.iter().map(DelegationRefusal::code).collect();

        assert_eq!(phrases.len(), 1, "one phrase for all three");
        assert_eq!(codes.len(), 1, "and one code");

        // The operator's rendering does separate them — that is the half worth having — and still
        // carries nothing from the request.
        let operator: BTreeSet<String> = refusals.iter().map(ToString::to_string).collect();
        assert_eq!(operator.len(), 3);
    }

    /// The two browser bindings this host plants are two cookies. One name would mean connecting a
    /// connector while signed in silently invalidates the sign-in that is in flight.
    #[test]
    fn the_acquisition_binder_is_not_the_sign_in_binder() {
        assert_ne!(BINDER_COOKIE, crate::oidc::flow::BINDER_COOKIE);
        assert!(BINDER_COOKIE.starts_with("__Host-"));

        let binder = Binder::draw().expect("randomness");
        let planted = planted_binder(&binder);
        assert!(
            planted.starts_with(&format!("{BINDER_COOKIE}=")),
            "{planted}"
        );
        assert!(planted.contains("; Secure"), "{planted}");
        assert!(planted.contains("; HttpOnly"), "{planted}");
        assert!(planted.contains("SameSite=Lax"), "{planted}");

        assert_eq!(
            presented_binder(Some(&format!(
                "{}=other; {BINDER_COOKIE}={}",
                crate::oidc::flow::BINDER_COOKIE,
                binder.as_str(),
            ))),
            Some(binder.as_str()),
            "the acquisition binder is read from its own name, beside a sign-in binder",
        );
        assert_eq!(presented_binder(None), None);
        assert_eq!(
            presented_binder(Some(&format!("{BINDER_COOKIE}="))),
            None,
            "an empty cookie is no binder, not a binder that might match",
        );
    }
}
