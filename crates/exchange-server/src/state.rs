//! What every route is built with.

use std::sync::Arc;

use exchange_host::{Identity, SecretStore};

use crate::bind::IdentityBinding;
use crate::dev_identity::DevIdentity;

/// The state the router hands to every route.
///
/// It carries the *ports* a composition bound, never a credential and never a tenant. A tenant is
/// read from a resolved principal and from nothing else — there is deliberately nothing here that
/// a route could reach for instead.
#[derive(Clone)]
pub struct AppState {
    identity: BoundIdentity,
    /// Where credentials are kept, as the **port** rather than as the concrete store.
    ///
    /// `exchange_host::CredentialStore` is one binding of this and is `#[cfg(unix)]`, because only
    /// the file store is; holding the port keeps the surface off that gate and keeps a deployment
    /// that binds Vault instead able to do so. `None` is a composition that bound none, and every
    /// route that would reach for one refuses rather than pretending — see
    /// [`crate::routes::connections`].
    credentials: Option<Arc<dyn SecretStore>>,
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
    /// Nothing in this binary constructs one yet — X-04 binds the first, and removes this
    /// attribute when it does. The variant is not speculative: it is what
    /// `routes::identity::tests::a_rejected_credential_and_an_unreachable_provider_are_distinguishable`
    /// drives, and that test needs a port which is neither absent nor the development one.
    #[allow(dead_code)]
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
            credentials: None,
        }
    }

    /// A composition with a real identity provider bound.
    ///
    /// This is the constructor that makes a reachable bind legal, which is why arming the
    /// development identity goes through a different one rather than through this with a flag.
    ///
    /// Unused outside tests until X-04 binds a provider to it; see [`BoundIdentity::Real`].
    #[allow(dead_code)]
    pub fn with_identity(identity: Arc<dyn Identity>) -> Self {
        Self {
            identity: BoundIdentity::Real(identity),
            credentials: None,
        }
    }

    /// A composition with the development identity armed.
    pub fn with_development_identity(identity: Arc<DevIdentity>) -> Self {
        Self {
            identity: BoundIdentity::Development(identity),
            credentials: None,
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
