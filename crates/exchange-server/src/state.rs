//! What every route is built with.

use std::sync::Arc;

use exchange_host::Identity;

use crate::bind::IdentityBinding;
use crate::dev_identity::DevIdentity;
use crate::oidc::Oidc;

/// The state the router hands to every route.
///
/// It carries the *ports* a composition bound, never a credential and never a tenant. A tenant is
/// read from a resolved principal and from nothing else — there is deliberately nothing here that
/// a route could reach for instead.
#[derive(Clone)]
pub struct AppState {
    identity: BoundIdentity,
    sign_in: SignIn,
}

/// What this composition can offer a human who wants to sign in.
///
/// Three states rather than an `Option`, because "not configured" and "configured but unable to
/// finish" are different mistakes with different fixes, and a caller shown one message for both
/// gets sent to the wrong place. Both are answered at `/api/signin` rather than at the callback:
/// the failure the story names is a login that looks fine and dies at the last step.
#[derive(Clone)]
pub enum SignIn {
    /// No OIDC configuration was supplied, or not all of it was. `/api/signin` explains.
    Unconfigured,

    /// OIDC is configured, but this composition bound no
    /// [`TokenExchange`](crate::oidc::exchange::TokenExchange), so an authorization code could
    /// never be redeemed. `/api/signin` explains rather than sending the browser to a provider it
    /// cannot return from usefully.
    NoTokenExchange,

    /// A provider is bound and the flow can complete.
    Oidc(Arc<Oidc>),
}

/// The identity port a composition bound, and what the bind rule may conclude from it.
///
/// One enum rather than a port plus a flag, so "this is the development identity" is not a claim
/// something could make about a different port, and not one the development port could avoid
/// making about itself.
#[derive(Clone)]
enum BoundIdentity {
    /// Nothing can authenticate a caller.
    None,
    /// A real provider. This is what makes a reachable bind legal.
    ///
    /// [`AppState::with_oidc`] is what constructs one: a federated principal is backed by a secret
    /// the caller proved to a third party, which is the property the development identity lacks and
    /// the reason that one is a separate variant.
    Real(Arc<dyn Identity>),
    /// The development identity, which is loopback-only for as long as it is armed.
    Development(Arc<DevIdentity>),
}

impl AppState {
    /// A composition with no identity provider bound.
    ///
    /// Every route that needs a principal refuses, which is the honest answer rather than a hole:
    /// the host cannot attribute the request, so it does not serve it.
    pub fn without_identity() -> Self {
        Self {
            identity: BoundIdentity::None,
            sign_in: SignIn::Unconfigured,
        }
    }

    /// A composition with a real identity provider bound.
    ///
    /// This is the constructor that makes a reachable bind legal, which is why arming the
    /// development identity goes through a different one rather than through this with a flag.
    ///
    /// Test-only: the binary's real provider arrives through [`AppState::with_oidc`], which binds
    /// the identity port and the sign-in flow **together** so the two cannot drift apart. This one
    /// exists for tests that need a port which is neither absent nor the development one.
    #[cfg(test)]
    pub fn with_identity(identity: Arc<dyn Identity>) -> Self {
        Self {
            identity: BoundIdentity::Real(identity),
            sign_in: SignIn::Unconfigured,
        }
    }

    /// A composition with the development identity armed.
    pub fn with_development_identity(identity: Arc<DevIdentity>) -> Self {
        Self {
            identity: BoundIdentity::Development(identity),
            sign_in: SignIn::Unconfigured,
        }
    }

    /// A composition that federates sign-in to an OIDC provider.
    ///
    /// One argument sets **both** the identity port and the sign-in flow, deliberately. They are
    /// the same object: the callback opens a session in it and the guard resolves that session out
    /// of it, so a composition that could set one without the other would be one where a completed
    /// sign-in resolves to nothing. This is the constructor that legitimately reports
    /// [`IdentityBinding::Bound`].
    ///
    /// Unused in this binary until a composition binds a
    /// [`TokenExchange`](crate::oidc::exchange::TokenExchange); see `docs/designs/oidc-signin.md`.
    #[allow(dead_code)]
    pub fn with_oidc(oidc: Arc<Oidc>) -> Self {
        Self {
            identity: BoundIdentity::Real(oidc.clone()),
            sign_in: SignIn::Oidc(oidc),
        }
    }

    /// A composition whose OIDC configuration is complete but which bound no token exchange.
    ///
    /// **Not** an identity binding. Nothing here can authenticate a caller — no sign-in can finish
    /// — so reporting [`IdentityBinding::Bound`] would legalise a reachable bind in front of a host
    /// where nobody can sign in. The same reasoning that made the development identity a third
    /// state: the bind rule asks whether anything *could* resolve a caller, and here nothing can.
    pub fn oidc_without_a_token_exchange() -> Self {
        Self {
            identity: BoundIdentity::None,
            sign_in: SignIn::NoTokenExchange,
        }
    }

    /// Whether a request could become a principal, and whether that is safe to expose.
    ///
    /// This is what [`admit_bind`](crate::bind::admit_bind) decides on.
    pub fn identity_binding(&self) -> IdentityBinding {
        match self.identity {
            BoundIdentity::None => IdentityBinding::Unbound,
            BoundIdentity::Real(_) => IdentityBinding::Bound,
            BoundIdentity::Development(_) => IdentityBinding::Development,
        }
    }

    /// The bound identity port, whichever kind it is.
    ///
    /// The guard resolves every caller through this and does not care which kind answered — a
    /// development principal and a federated one are the same thing by the time a route sees them.
    pub fn identity(&self) -> Option<Arc<dyn Identity>> {
        match &self.identity {
            BoundIdentity::None => None,
            BoundIdentity::Real(identity) => Some(identity.clone()),
            BoundIdentity::Development(identity) => Some(identity.clone()),
        }
    }

    /// The development identity, if that is what this composition armed.
    ///
    /// The session routes need the concrete port, because opening a session is not part of the
    /// [`Identity`] trait — and it should not be. The trait is what every provider implements, and
    /// a provider that federates to an IdP has no session of its own to open.
    pub fn development_identity(&self) -> Option<&Arc<DevIdentity>> {
        match &self.identity {
            BoundIdentity::Development(identity) => Some(identity),
            _ => None,
        }
    }

    /// What this composition can offer a caller who wants to sign in.
    pub fn sign_in(&self) -> &SignIn {
        &self.sign_in
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::net::SocketAddr;

    use exchange_host::async_trait;

    use crate::bind::admit_bind;
    use crate::oidc::config::OidcConfig;
    use crate::oidc::exchange::{ExchangeError, Redemption, SignedClaims, TokenExchange};

    fn dev() -> Arc<DevIdentity> {
        Arc::new(DevIdentity::from_roster("user:alice@acme").expect("a well-formed roster"))
    }

    /// A federated composition.
    ///
    /// The exchange is never called — deciding a binding redeems nothing — but `with_oidc` needs
    /// one to exist, which is itself the point of that constructor: there is no way to build a
    /// composition that reports `Bound` without something able to complete a sign-in.
    fn oidc() -> Arc<Oidc> {
        struct Unused;

        #[async_trait]
        impl TokenExchange for Unused {
            async fn redeem(&self, _: Redemption<'_>) -> Result<SignedClaims, ExchangeError> {
                unreachable!("deciding an identity binding redeems no authorization code")
            }
        }

        Arc::new(Oidc::new(
            OidcConfig::for_test("https://accounts.example.com", "flux-exchange", "acme"),
            Arc::new(Unused),
        ))
    }

    fn addr(raw: &str) -> SocketAddr {
        raw.parse().expect("a literal socket address")
    }

    /// The seam `main` actually runs through, end to end: compose the state, ask it for its
    /// binding, and hand that to the bind rule.
    ///
    /// `bind::tests` pins the *enum* — that `Development` is refused on a reachable address. This
    /// pins the **wiring**, which is a separate claim and the one a future simplification breaks:
    /// collapsing `identity_binding` to return `Bound` for the development port would leave every
    /// test in `bind` green while making `FLUX_EXCHANGE_BIND=0.0.0.0:8080` serve a credential-free
    /// identity to the network.
    #[test]
    fn arming_the_development_identity_does_not_admit_a_reachable_bind() {
        let state = AppState::with_development_identity(dev());

        assert!(
            admit_bind(addr("0.0.0.0:8080"), state.identity_binding()).is_err(),
            "arming the development identity must not make a reachable bind legal",
        );
        assert!(
            admit_bind(addr("127.0.0.1:8080"), state.identity_binding()).is_ok(),
            "and must still allow the loopback bind it exists for",
        );
    }

    /// The mapping itself, stated as a table so that **every** constructor is pinned rather than
    /// only the one a given story added.
    ///
    /// The table is the point. X-04 added two constructors and pinned neither, which is the same
    /// omission X-03 was reworked for — a constructor absent from here is a binding nothing
    /// asserts, and `identity_binding` is what `admit_bind` decides a reachable bind on. Adding a
    /// constructor without adding a row is the whole failure mode.
    #[test]
    fn each_constructor_reports_its_own_binding() {
        assert_eq!(
            AppState::without_identity().identity_binding(),
            IdentityBinding::Unbound,
        );
        assert_eq!(
            AppState::with_identity(dev()).identity_binding(),
            IdentityBinding::Bound,
        );
        assert_eq!(
            AppState::with_development_identity(dev()).identity_binding(),
            IdentityBinding::Development,
            "the development port must never report itself as a real binding",
        );
        assert_eq!(
            AppState::with_oidc(oidc()).identity_binding(),
            IdentityBinding::Bound,
            "a federated principal is backed by a secret proved to a third party, which is what \
             the development identity lacks and what makes this one a real binding",
        );
        assert_eq!(
            AppState::oidc_without_a_token_exchange().identity_binding(),
            IdentityBinding::Unbound,
            "OIDC configured with nothing able to redeem an authorization code can authenticate \
             nobody, so it must not report itself as bound",
        );
    }

    /// The seam `main` runs through for the composition this build actually produces: compose the
    /// state, ask it for its binding, hand that to the bind rule.
    ///
    /// This is the OIDC twin of `arming_the_development_identity_does_not_admit_a_reachable_bind`,
    /// and it exists for the reason that one does — **pinning the enum is not pinning the wiring.**
    /// `bind::tests` pins that `Unbound` refuses a reachable address; nothing pinned that *this
    /// constructor* reports `Unbound`, so the claim `docs/designs/oidc-signin.md` calls
    /// load-bearing rested on a single unasserted literal.
    ///
    /// The mistake this catches is a plausible one, not a contrived one: somebody reasons "an OIDC
    /// provider is configured, so this host has an identity provider" and teaches
    /// `identity_binding` to say `Bound` for it. Every test in `bind` stays green, and
    /// `FLUX_EXCHANGE_BIND=0.0.0.0:8080` starts serving a host at which **no sign-in can complete**
    /// — an identity provider in name only.
    #[test]
    fn configuring_oidc_without_a_token_exchange_does_not_admit_a_reachable_bind() {
        let state = AppState::oidc_without_a_token_exchange();

        for reachable in ["0.0.0.0:8080", "[::]:8080", "192.168.1.10:8080"] {
            assert!(
                admit_bind(addr(reachable), state.identity_binding()).is_err(),
                "`{reachable}` must be refused: configuring OIDC without a token exchange \
                 authenticates nobody, so it cannot legalise a reachable bind",
            );
        }

        assert!(
            admit_bind(addr("127.0.0.1:8080"), state.identity_binding()).is_ok(),
            "and loopback must still be admitted, or the safe configuration is the unusable one",
        );
    }

    /// The concrete port is reachable only when it is the development one, because that is what
    /// decides whether the session routes mint anything.
    #[test]
    fn only_the_development_binding_hands_back_a_development_port() {
        assert!(AppState::with_development_identity(dev())
            .development_identity()
            .is_some());
        assert!(AppState::with_identity(dev())
            .development_identity()
            .is_none());
        assert!(AppState::without_identity()
            .development_identity()
            .is_none());
    }
}
