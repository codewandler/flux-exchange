//! What every route is built with.

use std::sync::Arc;

use exchange_host::{ConnectionSettings, Identity, Invoker, SecretStore};

use crate::agent::AgentStore;
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
    /// Where a tenant's **non-secret** connection values are kept, as the port.
    ///
    /// A separate binding from [`credentials`](Self::credentials) and a separate store behind it,
    /// which is X-47's central decision rather than a filing one: a subdomain is not a secret, and
    /// putting it in the credential store would make `held` and the tenant occupancy bound each
    /// mean two things at once. `exchange_host::settings` carries the whole argument.
    ///
    /// `None` is a composition that bound none. Every route that would write one refuses and names
    /// the setting, and the invoker gets an empty configuration — so the connectors that need
    /// nothing still run and the ones that need something still refuse by name.
    settings: Option<Arc<dyn ConnectionSettings>>,
    /// Which connection changes are in flight, so two of them cannot decide against the same
    /// empty address and both write.
    ///
    /// Shared across every clone of this state, which is why it lives here rather than in the
    /// module that uses it: `AppState` is the only thing every request holds a handle to. It is
    /// not an `Option` — a guard with no store behind it costs nothing and guards nothing, and
    /// making it absent would give the routes a second thing to branch on.
    connections: Arc<ConnectionGuard>,
    /// The agents this host has minted tokens for, if this composition bound a store for them.
    ///
    /// A separate binding from [`credentials`](Self::credentials) and deliberately so: an agent
    /// verifier is this host's own record and a credential is a tenant's vendor secret, and
    /// `crate::agent`'s module documentation carries the argument for why they are two stores. Every
    /// combination of the two is a real composition, so neither implies the other. `None` is a
    /// composition that bound none, and the mint route refuses rather than pretending.
    agents: Option<Arc<AgentStore>>,
    /// What runs an operation, if this composition bound one.
    ///
    /// A separate binding from [`credentials`](Self::credentials) even though one is built from the
    /// other, and for the reason every port here is separate: an invoker also carries a transport, a
    /// deployment class and a context source, none of which a credential store implies. `None` is a
    /// composition that runs nothing, and `POST /api/operations/{operation}/invoke` refuses rather
    /// than pretending — see [`crate::routes::invoke`].
    invoker: Option<Arc<Invoker>>,
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

impl SignIn {
    /// Whether a human could complete a sign-in against this composition.
    ///
    /// Three states collapse to one boolean, and the collapse is the point rather than a loss.
    /// The three are what an **operator** needs, because "not configured" and "configured but
    /// unable to finish" have different remedies — `crate::routes::signin`'s two explanatory pages
    /// say so, to a human who asked for a login. What a **caller** may know is narrower: whether
    /// sign-in works is a fact about the service, and which of the two ways it does not work is a
    /// fact about the configuration. Telling them apart on the wire would report to an anonymous
    /// caller whether this host's OIDC settings are set, which is a piece of the deployment's
    /// shape and none of their business.
    ///
    /// Matched exhaustively and with no wildcard arm, deliberately: a fourth variant must be a
    /// compile error here rather than something that silently falls to `false`, because the arm a
    /// new state belongs in is the whole decision.
    pub fn available(&self) -> bool {
        match self {
            SignIn::Oidc(_) => true,
            SignIn::Unconfigured | SignIn::NoTokenExchange => false,
        }
    }
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
            settings: None,
            connections: Arc::default(),
            agents: None,
            invoker: None,
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
            settings: None,
            connections: Arc::default(),
            agents: None,
            invoker: None,
        }
    }

    /// A composition with the development identity armed.
    pub fn with_development_identity(identity: Arc<DevIdentity>) -> Self {
        Self {
            identity: BoundIdentity::Development(identity),
            sign_in: SignIn::Unconfigured,
            credentials: None,
            settings: None,
            connections: Arc::default(),
            agents: None,
            invoker: None,
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
    /// The binary reaches this once `HttpTokenExchange` is built; see `docs/designs/oidc-signin.md`.
    pub fn with_oidc(oidc: Arc<Oidc>) -> Self {
        Self {
            identity: BoundIdentity::Real(oidc.clone()),
            sign_in: SignIn::Oidc(oidc),
            credentials: None,
            settings: None,
            connections: Arc::default(),
            agents: None,
            invoker: None,
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
            settings: None,
            connections: Arc::default(),
            agents: None,
            invoker: None,
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

    /// Bind the connection-settings store this composition holds.
    ///
    /// An additive builder method for [`with_credentials`](Self::with_credentials)' reason, and a
    /// *separate* one from it on purpose: every combination of the two is a real composition. A
    /// host with credentials and no settings store runs the thirty-seven connectors that need
    /// nothing per connection and refuses the seventeen that do, by name — which is X-12's behaviour,
    /// kept rather than papered over.
    ///
    /// **Binding one does not make a caller resolvable**, so it must never influence
    /// [`identity_binding`](Self::identity_binding) —
    /// `binding_a_settings_store_does_not_admit_a_reachable_bind` is that claim.
    pub fn with_settings(mut self, settings: Arc<dyn ConnectionSettings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// The connection-settings store this composition bound, if it bound one.
    ///
    /// The port, so a route can neither reopen the store nor learn where it is. What a route may do
    /// with it is ask whether a value is set, set one, and clear one — always at an address this
    /// host **derived** from the resolved principal and the connector's own declaration.
    pub fn settings(&self) -> Option<&Arc<dyn ConnectionSettings>> {
        self.settings.as_ref()
    }

    /// Bind the agent store this composition holds.
    ///
    /// An additive builder method for [`with_credentials`](Self::with_credentials)' reason, and the
    /// reason matters more with every one of these: the identity binding, the credential store and
    /// the agent store are independent, every combination of them is a real composition, and five
    /// constructors times two ports times two would be twenty. An additive method also leaves the
    /// existing spellings untouched, which is what lets two stories that each add a port land
    /// without colliding.
    ///
    /// **Binding one does not make a caller resolvable**, so it must never influence
    /// [`identity_binding`](Self::identity_binding) — X-36 mints agent tokens and nothing yet
    /// verifies one, and even once X-37 does, an agent store is not an identity provider a *human*
    /// can sign in through. `binding_an_agent_store_does_not_admit_a_reachable_bind` is that claim,
    /// and it is here rather than in `bind` because pinning the enum is not pinning the wiring.
    pub fn with_agents(mut self, agents: Arc<AgentStore>) -> Self {
        self.agents = Some(agents);
        self
    }

    /// The agent store this composition bound, if it bound one.
    pub fn agents(&self) -> Option<&Arc<AgentStore>> {
        self.agents.as_ref()
    }

    /// Bind what this composition runs operations with.
    ///
    /// An additive builder method for [`with_credentials`](Self::with_credentials)' reason.
    ///
    /// **Binding one does not make a caller resolvable**, so it must never influence
    /// [`identity_binding`](Self::identity_binding): a host that can run operations and cannot
    /// identify anybody is exactly the composition `admit_bind` exists to refuse, and it is *more*
    /// dangerous than one that can only serve a catalogue.
    /// `binding_an_invoker_does_not_admit_a_reachable_bind` is that claim.
    pub fn with_invoker(mut self, invoker: Arc<Invoker>) -> Self {
        self.invoker = Some(invoker);
        self
    }

    /// What this composition runs operations with, if it bound anything.
    ///
    /// The invoker, not its parts. A route cannot reach the transport, the store or the deployment
    /// class through it — the only thing it can do is name an operation and hand over a principal.
    pub fn invoker(&self) -> Option<&Arc<Invoker>> {
        self.invoker.as_ref()
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

    /// **X-36.** Binding an agent store does not legalise a reachable bind.
    ///
    /// The same shape as the two above, and here for the reason they are here: pinning the enum is
    /// not pinning the wiring. An agent store is a place to keep verifiers, not a thing that can
    /// resolve a caller — nothing in this build verifies an agent token at all yet, and even once
    /// X-37 does, a host on `0.0.0.0` whose only identity is the development roster is exactly the
    /// composition `admit_bind` exists to refuse. The mistake this catches is somebody reasoning
    /// "agents can authenticate now, so this host has an identity provider" and teaching
    /// `identity_binding` to say so.
    #[test]
    fn binding_an_agent_store_does_not_admit_a_reachable_bind() {
        let directory =
            std::env::temp_dir().join(format!("flux-exchange-state-agents-{}", std::process::id()));
        let store = Arc::new(
            crate::agent::AgentStore::open(directory.join("agents.json")).expect("a fresh store"),
        );

        for state in [
            AppState::without_identity().with_agents(store.clone()),
            AppState::with_development_identity(dev()).with_agents(store.clone()),
            AppState::oidc_without_a_token_exchange().with_agents(store.clone()),
        ] {
            assert!(
                admit_bind(addr("0.0.0.0:8080"), state.identity_binding()).is_err(),
                "binding an agent store must not make a reachable bind legal",
            );
        }

        assert_eq!(
            AppState::with_development_identity(dev())
                .with_agents(store)
                .identity_binding(),
            IdentityBinding::Development,
            "and it must not change the binding a composition reports either",
        );

        let _ = std::fs::remove_dir_all(&directory);
    }

    /// **X-12.** Binding an invoker does not legalise a reachable bind.
    ///
    /// The same shape as the three above and here for the same reason — pinning the enum is not
    /// pinning the wiring — but this is the one where getting it wrong costs the most. An invoker
    /// is the thing that spends a tenant's credentials against a vendor; a host on `0.0.0.0` that
    /// can do that and cannot identify a caller is strictly worse than one that can only serve a
    /// catalogue. The mistake this catches is somebody reasoning "operations run now, so this host
    /// is a real service" and teaching `identity_binding` to say `Bound`.
    #[test]
    fn binding_an_invoker_does_not_admit_a_reachable_bind() {
        struct NoStore;

        #[exchange_host::async_trait]
        impl exchange_host::SecretStore for NoStore {
            async fn get(
                &self,
                reference: &exchange_host::CredentialRef,
            ) -> Result<exchange_host::Secret, exchange_host::StoreError> {
                Err(exchange_host::StoreError::NotFound {
                    path: exchange_host::address_path(reference),
                })
            }
            async fn put(
                &self,
                _: &exchange_host::CredentialRef,
                _: &exchange_host::Secret,
            ) -> Result<(), exchange_host::StoreError> {
                unreachable!("deciding an identity binding writes no credential")
            }
            async fn delete(
                &self,
                _: &exchange_host::CredentialRef,
            ) -> Result<(), exchange_host::StoreError> {
                unreachable!("deciding an identity binding destroys no credential")
            }
        }

        /// A bound grant store that admits nothing.
        ///
        /// A local type rather than a memory store re-exported from `exchange_host`, for the reason
        /// `routes::invoke`'s `EmptyStore` is one: publishing a grant store that holds nothing and
        /// forgets everything would put the "just bind this one" fallback one line away from a
        /// composition, and an invoker whose grants vanish on restart is worse than one that
        /// refuses to be built.
        struct NoGrants;

        impl exchange_host::Grants for NoGrants {
            fn held(&self, _: &exchange_host::Tenant) -> Vec<exchange_host::Grant> {
                Vec::new()
            }

            fn set(
                &self,
                _: &exchange_host::Tenant,
                _: &[exchange_host::Grant],
            ) -> Result<(), exchange_host::GrantRefusal> {
                unreachable!("deciding an identity binding grants nothing")
            }
        }

        let invoker = Arc::new(
            crate::execution::invoker(
                Arc::new(NoStore),
                // Deciding an identity binding reads no connection setting either.
                Arc::new(exchange_host::MemoryConfig::new()),
                // Nor any grant: what is under test is what the composition *reports*, and an
                // invoker that admits nothing reports it exactly as one that admits everything.
                Arc::new(NoGrants),
            )
            .expect("a usable workspace root"),
        );

        for state in [
            AppState::without_identity().with_invoker(invoker.clone()),
            AppState::with_development_identity(dev()).with_invoker(invoker.clone()),
            AppState::oidc_without_a_token_exchange().with_invoker(invoker.clone()),
        ] {
            assert!(
                admit_bind(addr("0.0.0.0:8080"), state.identity_binding()).is_err(),
                "binding an invoker must not make a reachable bind legal",
            );
        }

        assert_eq!(
            AppState::with_development_identity(dev())
                .with_invoker(invoker)
                .identity_binding(),
            IdentityBinding::Development,
            "and it must not change the binding a composition reports either",
        );
    }

    /// **X-47.** Binding a connection-settings store does not legalise a reachable bind.
    ///
    /// The fourth of this shape, and here for the reason the other three are: pinning the enum is
    /// not pinning the wiring. The mistake it catches is the one this story makes plausible —
    /// somebody reasons "the templated connectors work now, so this host is a real service" and
    /// teaches `identity_binding` to say `Bound`. What a settings store holds is a subdomain; it
    /// authenticates nobody, and a host on `0.0.0.0` that can reach thirteen more vendors with a
    /// tenant's credentials and still cannot identify a caller is *worse* than one that cannot.
    #[test]
    fn binding_a_settings_store_does_not_admit_a_reachable_bind() {
        let directory = std::env::temp_dir().join(format!(
            "flux-exchange-state-settings-{}",
            std::process::id()
        ));
        let store = Arc::new(
            exchange_host::SettingsStore::bind(directory.join("settings")).expect("a fresh store"),
        );

        for state in [
            AppState::without_identity().with_settings(store.clone()),
            AppState::with_development_identity(dev()).with_settings(store.clone()),
            AppState::oidc_without_a_token_exchange().with_settings(store.clone()),
        ] {
            assert!(
                admit_bind(addr("0.0.0.0:8080"), state.identity_binding()).is_err(),
                "binding a connection-settings store must not make a reachable bind legal",
            );
        }

        assert_eq!(
            AppState::with_development_identity(dev())
                .with_settings(store)
                .identity_binding(),
            IdentityBinding::Development,
            "and it must not change the binding a composition reports either",
        );

        let _ = std::fs::remove_dir_all(&directory);
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
