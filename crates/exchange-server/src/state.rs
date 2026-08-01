//! What every route is built with.

use std::sync::Arc;

use exchange_host::{Identity, SecretStore};

use crate::bind::IdentityBinding;
use crate::connection_guard::ConnectionGuard;
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
    /// Where credentials are kept, as the **port** rather than as the concrete store.
    ///
    /// `exchange_host::CredentialStore` is one binding of this and is `#[cfg(unix)]`, because only
    /// the file store is; holding the port keeps the surface off that gate and keeps a deployment
    /// that binds Vault instead able to do so. `None` is a composition that bound none, and every
    /// route that would reach for one refuses rather than pretending — see
    /// [`crate::routes::connections`].
    credentials: Option<Arc<dyn SecretStore>>,
    /// Which connection changes are in flight, so two of them cannot decide against the same
    /// empty address and both write.
    ///
    /// Shared across every clone of this state, which is why it lives here rather than in the
    /// module that uses it: `AppState` is the only thing every request holds a handle to. It is
    /// not an `Option` — a guard with no store behind it costs nothing and guards nothing, and
    /// making it absent would give the routes a second thing to branch on.
    connections: Arc<ConnectionGuard>,
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
            credentials: None,
            connections: Arc::default(),
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
            credentials: None,
            connections: Arc::default(),
        }
    }

    /// A composition with the development identity armed.
    pub fn with_development_identity(identity: Arc<DevIdentity>) -> Self {
        Self {
            identity: BoundIdentity::Development(identity),
            sign_in: SignIn::Unconfigured,
            credentials: None,
            connections: Arc::default(),
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
            credentials: None,
            connections: Arc::default(),
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
            credentials: None,
            connections: Arc::default(),
        }
    }

    /// Bind the credential store this composition holds.
    ///
    /// A builder method rather than a fourth constructor or a widened signature on the three above,
    /// for two reasons. The identity binding and the credential store are independent — every
    /// combination of them is a real composition, and four constructors times two would be eight —
    /// and an additive method leaves the existing spellings untouched, which is what lets a story
    /// that adds an identity provider and a story that adds a store land without colliding.
    ///
    /// Not calling it is a composition with no store, which is a state every route that needs one
    /// refuses in. There is no default here for the reason `CredentialStore` has none: a store
    /// nobody chose is a store nobody can find the credentials in.
    pub fn with_credentials(mut self, credentials: Arc<dyn SecretStore>) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// The credential store this composition bound, if it bound one.
    ///
    /// The port, so a route can neither reopen the store nor learn where it is. What a route may do
    /// with it is `get`, `put` and `delete` at an address this host **derived** — never one a
    /// caller supplied.
    pub fn credentials(&self) -> Option<&Arc<dyn SecretStore>> {
        self.credentials.as_ref()
    }

    /// The claims on connection changes in flight.
    ///
    /// Every clone of this state shares one, which is the whole point: two concurrent requests hold
    /// two clones and must contend on the same set. See [`ConnectionGuard`] for the window it
    /// closes and the single-process limit it closes it within.
    pub fn connections(&self) -> &Arc<ConnectionGuard> {
        &self.connections
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

    use crate::bind::admit_bind;

    fn dev() -> Arc<DevIdentity> {
        Arc::new(DevIdentity::from_roster("user:alice@acme").expect("a well-formed roster"))
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

    /// The mapping itself, stated as a table so that every constructor is pinned rather than only
    /// the one this story added.
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
