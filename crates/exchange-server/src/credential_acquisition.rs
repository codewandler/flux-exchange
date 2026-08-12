//! Server-owned credential acquisition performers and their startup bindings.
//!
//! The host crate owns only the secret-in/secret-out port. HTTP, endpoint URLs and vendor form
//! quirks stay here in the composing binary.
//!
//! # Production composes from the released catalogue (X-154)
//!
//! This module used to say *"production composes an empty [`AcquisitionBindings`]"*, and then, once
//! connector 0.21 landed, that upstream C-440 had shipped and **nothing here read it**. Both
//! sentences are retired: [`configured`] derives the registry from `connector_catalog`'s own
//! declarations — the OAuth2 acquisition's endpoint, authorize path, token path, scopes, permitted
//! grants, and the credential's declared [`AuthHazard`] — for the connectors **this deployment
//! registered**, and refuses at composition rather than composing something a vendor would reject
//! at the last step of a redirect nobody can replay.
//!
//! What is *not* read from the catalogue is the registration identity. Decision 0022 (amended
//! 2026-08-12): *"OAuth2 registration identity (`client_id`, `client_secret`, redirect URI) is
//! deployment configuration, not vendor truth… The artifact publishes the registration
//! **requirement**, never a value."* So [`grant_from_declaration`] destructures the declaration
//! exhaustively and names `client_id` as a field it discards — a deployment that supplied none is
//! refused by name even if a future document carried a non-empty string.
//!
//! # The one declared fact the generated tables do not carry
//!
//! `catalog::OAuth2::endpoint` is a **service name**, and the base URL it resolves against lives in
//! the connector's canonical document (`services[].base_url`) — not in the generated `&'static`
//! tables this crate reads, where `catalog::Provider` carries exactly one `base_url`, its *default*
//! service's. GitLab's OAuth2 declaration names `login`, whose document base URL is `{origin}`;
//! `catalog::providers()` cannot answer that, and neither `Provider::base_url`
//! (`{origin}/api/v4` — the API service, not the login service) nor `ConfigField::also_services`
//! (which says the *variable* is shared, not what the template is) is the same fact.
//!
//! [`endpoint_base`] therefore **refuses a named endpoint by name** rather than guessing one, so the
//! gap is a startup refusal an operator reads once instead of a malformed authorize URL. Reading the
//! canonical document is X-153's pack-and-reader path; when it lands, `endpoint_base` is the one
//! function that changes.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::acquisition_redirect::AcquisitionRedirect;
use connector_catalog::{
    Acquisition, AuthHazard as DeclaredHazard, Credential, OAuth2, OAuthGrant, Provider,
    ProviderKey,
};
use exchange_host::{
    async_trait, AcquiredCredential, AcquisitionRefusal, AuthHazard, AuthPosture,
    AuthPostureRefusal, AuthorizationCodeRedemption, CredentialAcquirer, PasswordRedemption,
    RefreshRedemption, Secret,
};
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use serde::Deserialize;

/// The deployment-owned half of a delegated authorization-code grant.
///
/// # One grant, shared — and checked against the deployment before it is bound
///
/// X-147's first review found this claim overstated and it is now what it says. The browser-facing
/// half — where to send the person, which client is asking, which scopes — is what
/// [`crate::routes::acquisitions`] needs; the back-channel half is what [`HttpCredentialAcquirer`]
/// sends to the token endpoint. They must agree, because RFC 6749 §4.1.3 has the token request
/// re-present the *same* `redirect_uri`, and a vendor answers a disagreement with `invalid_grant` at
/// the last step of a redirect nobody can replay.
///
/// Three things now make that agreement real rather than asserted:
///
/// 1. The redirect is an [`AcquisitionRedirect`], which cannot be constructed without passing
///    `crate::acquisition_redirect`'s canonical check. There is no `String` path into this field.
/// 2. [`AcquisitionBinding::delegating`] is the only way the concrete HTTP performer receives a
///    grant, and it hands **one `Arc`** to the binding and to the performer in one call. There is no
///    second argument to spell differently.
/// 3. [`AcquisitionBindings::new`] refuses a registry whose grant is not byte-equal to the redirect
///    this deployment configured. A binding that disagrees with the deployment does not exist.
///
/// # Where each half now comes from (X-154)
///
/// The redirect and the client identity are **not** connector metadata and never were. What changed
/// is the other half: the authorization endpoint and the scopes are composed from the connector's
/// own `Acquisition::OAuth2` declaration by [`grant_from_declaration`], rather than stated by a
/// composition. A grant constructed by hand is still admissible — that is what every test double
/// here does — but production reaches this type through the declaration.
#[derive(Clone)]
pub struct DelegatedGrant {
    authorization_endpoint: String,
    client_id: String,
    client_secret: Option<Secret>,
    scopes: Vec<String>,
    redirect: AcquisitionRedirect,
}

impl DelegatedGrant {
    /// Bind one delegated grant, refusing a shape that cannot produce a working authorization.
    ///
    /// # Errors
    ///
    /// A value-free reason when the authorization endpoint is not an absolute URL, when it is
    /// cleartext off a loopback literal, when the client id is empty, or when a scope is empty or
    /// carries the space that separates scopes. Each is a composition fault an operator reads once,
    /// rather than a vendor rejection at the end of a redirect nobody can replay.
    pub fn new(
        authorization_endpoint: &str,
        client_id: impl Into<String>,
        client_secret: Option<Secret>,
        scopes: impl IntoIterator<Item = String>,
        redirect: AcquisitionRedirect,
    ) -> Result<Self, &'static str> {
        let parsed = Url::parse(authorization_endpoint)
            .map_err(|_| "the authorization endpoint is not a URL")?;
        // The URL a person's browser navigates to carries `state` and the PKCE challenge, and the
        // answer comes back carrying an authorization code. `crate::oidc::config`'s transport check
        // makes the same argument for the sign-in authorization endpoint.
        //
        // **A loopback literal rather than `cfg!(test)`** (X-147 review, M4). Keying the exception
        // to the build meant no unit test in this crate could reach the refusal at all, so the rule
        // was unpinned exactly where it is written down. Keying it to the address is the rule
        // `crate::acquisition_redirect` and `crate::hosted_origin` already use, it is the same
        // exception a checkout actually needs, and it is a case a test can drive.
        if parsed.scheme() != "https"
            && !(parsed.scheme() == "http"
                && parsed
                    .host_str()
                    .is_some_and(crate::acquisition_redirect::is_loopback_literal))
        {
            return Err("the authorization endpoint must use HTTPS, or HTTP on a loopback literal");
        }
        let client_id = client_id.into();
        if client_id.is_empty() {
            return Err("a delegated grant needs a client id");
        }
        let scopes: Vec<String> = scopes.into_iter().collect();
        if scopes.is_empty() {
            return Err("a delegated grant needs at least one scope");
        }
        if scopes
            .iter()
            .any(|scope| scope.is_empty() || scope.contains(' '))
        {
            return Err("a scope is one space-free token; the list is what carries the separator");
        }
        Ok(Self {
            authorization_endpoint: authorization_endpoint.to_owned(),
            client_id,
            client_secret,
            scopes,
            redirect,
        })
    }

    /// Where the person's browser is sent.
    pub fn authorization_endpoint(&self) -> &str {
        &self.authorization_endpoint
    }

    /// This host's client identifier at the vendor.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// The scopes this host asks for, in the spelling one `scope` parameter carries.
    ///
    /// Composed from the declared list rather than taken as a string, so a caller cannot name a
    /// scope and a composition cannot smuggle two through one entry — the emptiness and space rules
    /// in [`new`](Self::new) are what make that true.
    pub fn scope(&self) -> String {
        self.scopes.join(" ")
    }

    /// The deployment's redirect URI, re-presented verbatim at the token endpoint.
    pub fn redirect(&self) -> &AcquisitionRedirect {
        &self.redirect
    }
}

impl std::fmt::Debug for DelegatedGrant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DelegatedGrant")
            .field("authorization_endpoint", &self.authorization_endpoint)
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .field("scopes", &self.scopes)
            .field("redirect", &self.redirect)
            .finish()
    }
}

/// One connector-declared acquisition, fixed at composition time.
#[derive(Clone)]
pub struct AcquisitionBinding {
    connector: String,
    credential: String,
    hazard: Option<AuthHazard>,
    delegated: Option<Arc<DelegatedGrant>>,
    performer: Arc<dyn CredentialAcquirer>,
}

impl AcquisitionBinding {
    /// Bind one connector and its acquired credential to a performer.
    ///
    /// `hazard` is an [`Option`], and the `None` is a decision rather than a default. See
    /// [`hazard`](Self::hazard).
    pub fn new(
        connector: impl Into<String>,
        credential: impl Into<String>,
        hazard: Option<AuthHazard>,
        performer: Arc<dyn CredentialAcquirer>,
    ) -> Self {
        Self {
            connector: connector.into(),
            credential: credential.into(),
            hazard,
            delegated: None,
            performer,
        }
    }

    /// **Bind one delegated grant to the concrete HTTP performer that will redeem it**, in one call.
    ///
    /// This exists instead of two builder calls, and that is the whole of it: the browser-facing
    /// half and the back-channel half are set here from **one `Arc`**, so there is no second
    /// argument for a composition to spell differently. X-147's first review found the previous
    /// shape — `with_delegated_grant` on the binding and `performing_delegated_grant` on the
    /// performer — was two independent fields with no equality check, while the documentation
    /// claimed the agreement was structural.
    ///
    /// It takes the performer **by value and concretely**, which is what makes that possible: an
    /// `Arc<dyn CredentialAcquirer>` cannot be handed a grant after the fact. A generic performer
    /// that holds no redirect of its own still uses
    /// [`with_delegated_grant`](Self::with_delegated_grant), where there is nothing to diverge.
    pub fn delegating(
        connector: impl Into<String>,
        credential: impl Into<String>,
        hazard: Option<AuthHazard>,
        grant: DelegatedGrant,
        performer: HttpCredentialAcquirer,
    ) -> Self {
        let grant = Arc::new(grant);
        let performer = performer.performing_delegated_grant(Arc::clone(&grant));
        Self {
            connector: connector.into(),
            credential: credential.into(),
            hazard,
            delegated: Some(grant),
            performer: Arc::new(performer),
        }
    }

    /// Declare that this connector's credential may be acquired by delegated authorization, for a
    /// performer that holds no redirect of its own.
    ///
    /// The seam a product binding uses, and the one every test double here uses. Where the performer
    /// *does* re-present a redirect at a token endpoint — which is every HTTP one — use
    /// [`delegating`](Self::delegating), so the two halves cannot be two values.
    pub fn with_delegated_grant(mut self, grant: DelegatedGrant) -> Self {
        self.delegated = Some(Arc::new(grant));
        self
    }

    /// The connector catalogue key this binding belongs to.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// The flat catalogue name of the access credential this performer mints.
    pub fn credential(&self) -> &str {
        &self.credential
    }

    /// The connector-declared acquisition hazard, when the acquisition declares one.
    ///
    /// # Why this is an `Option` and not an `AuthHazard::None` (X-147)
    ///
    /// `AuthHazard` is documented as *a named weakness*, and `crate::routes` reaches
    /// `AuthPosture::admit` with one only when there is a weakness to admit. A `None` **variant**
    /// would put "there is nothing wrong with this" inside a closed set whose every other member is
    /// a citation, so a filter written as `at_most`-style reasoning over that set would have a
    /// no-op to compare against, and the exhaustive matches `exchange_host::acquisition` argues for
    /// would each gain an arm meaning *skip*. The absence of a hazard is the absence of a value.
    ///
    /// The cost is real and is why it is stated here: a `None` **admits unconditionally**, so a
    /// binding that forgot its hazard is a binding a fail-closed deployment now performs. What
    /// stops that is that there is no default — [`new`](Self::new) takes the option positionally, so
    /// every composition site says which it meant, and a delegated grant declaring `None` is saying
    /// the thing RFC 9700 §2.4 says about it: the authorization-code grant with PKCE is the one the
    /// resource owner's secret never crosses.
    pub const fn hazard(&self) -> Option<AuthHazard> {
        self.hazard
    }

    /// The delegated grant this composition bound, when it bound one.
    pub fn delegated(&self) -> Option<&DelegatedGrant> {
        self.delegated.as_deref()
    }

    /// Decide a deployment's posture against whatever hazard this binding declares.
    ///
    /// **The unconditional-admit path, in one place.** An acquisition that declares no hazard has
    /// nothing for [`AuthPosture`] to decide, and `AuthPosture::fail_closed` refuses every hazard
    /// there *is* — so the branch has to live somewhere, and one function every acquisition route
    /// calls is the shape that cannot drift. Writing `if let Some(hazard)` at each route would be
    /// three copies of a decision, and the third one is where somebody eventually inverts it.
    ///
    /// # Errors
    ///
    /// [`AuthPostureRefusal::HazardNotAllowed`], naming the connector and the hazard and no value,
    /// when this binding declares a hazard the deployment did not opt into.
    pub fn admit(&self, posture: &AuthPosture) -> Result<(), AuthPostureRefusal> {
        match self.hazard {
            Some(hazard) => posture.admit(&self.connector, hazard),
            None => Ok(()),
        }
    }

    /// The fixed server-owned performer.
    pub fn performer(&self) -> &Arc<dyn CredentialAcquirer> {
        &self.performer
    }
}

impl std::fmt::Debug for AcquisitionBinding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcquisitionBinding")
            .field("connector", &self.connector)
            .field("credential", &self.credential)
            .field("hazard", &self.hazard)
            .field("delegated", &self.delegated)
            .field("performer", &"[BOUND]")
            .finish()
    }
}

/// The acquisition declarations this composition explicitly bound, and the redirect they agree with.
///
/// **The redirect lives here and nowhere else** (X-147 re-review, nit 3). It was briefly a second
/// field on `AppState`, wired independently — which is the shape B1 was filed for, one level up: a
/// composition could pass one redirect to this constructor and a different one to the state, and the
/// authorize URL would carry the state's while the token request re-presented the grant's. Holding
/// it on the value that validated it, and having `AppState::acquisition_redirect` read through to
/// it, means there is one wiring and nothing to keep in step.
#[derive(Clone, Debug, Default)]
pub struct AcquisitionBindings {
    by_connector: BTreeMap<String, AcquisitionBinding>,
    redirect: Option<AcquisitionRedirect>,
}

impl AcquisitionBindings {
    /// Construct a registry, refusing duplicate connector bindings and any delegated grant that
    /// disagrees with the redirect URI **this deployment** configured.
    ///
    /// # Why the deployment's redirect is an argument here (X-147 review, B1)
    ///
    /// It is the one place every registry passes through, so it is the one place that can make
    /// "compared exactly" true rather than intended. Before this, `crate::acquisition_redirect`'s
    /// canonical check guarded a value that reached no vendor while the string that *did* was
    /// checked for being non-empty — a control that only appeared to exist.
    ///
    /// A grant is refused when the deployment configured **no** redirect, and when its redirect is
    /// not byte-equal to the configured one. Byte equality and not URL equivalence: RFC 6749 §4.1.3
    /// has the token request re-present the same value, and a vendor comparing strings does not know
    /// which two spellings we consider the same.
    ///
    /// # Errors
    ///
    /// A value-free reason. It names the disagreement, never either URL: a startup log is permanent
    /// and a redirect is deployment topology.
    pub fn new(
        bindings: impl IntoIterator<Item = AcquisitionBinding>,
        configured: Option<&AcquisitionRedirect>,
    ) -> Result<Self, &'static str> {
        let mut by_connector = BTreeMap::new();
        for binding in bindings {
            if binding
                .performer
                .binding_connector()
                .is_some_and(|owner| owner != binding.connector)
            {
                return Err("a credential-acquisition performer is bound to another connector");
            }
            if let Some(grant) = binding.delegated.as_ref() {
                match configured {
                    None => return Err(
                        "a delegated acquisition grant is bound and this deployment configured \
                             no acquisition redirect URI",
                    ),
                    Some(configured) if configured != grant.redirect() => return Err(
                        "a delegated acquisition grant names a redirect URI that is not the one \
                             this deployment configured",
                    ),
                    Some(_) => {}
                }
            }
            if by_connector
                .insert(binding.connector.clone(), binding)
                .is_some()
            {
                return Err("a connector has more than one credential-acquisition binding");
            }
        }
        Ok(Self {
            by_connector,
            redirect: configured.cloned(),
        })
    }

    /// Look up only by the catalogue connector selected by the route.
    pub fn get(&self, connector: &str) -> Option<&AcquisitionBinding> {
        self.by_connector.get(connector)
    }

    /// The redirect URI every grant in this registry was checked against.
    ///
    /// `Some` whenever any binding here carries a delegated grant — [`new`](Self::new) refuses the
    /// alternative — so a route that has found a grant has found this too.
    pub fn redirect(&self) -> Option<&AcquisitionRedirect> {
        self.redirect.as_ref()
    }
}

/// The connectors this deployment holds an OAuth2 registration for, comma-separated.
///
/// **The selection is deployment configuration, and there is deliberately no catalogue-driven
/// default.** A registry derived from every connector that *declares* an OAuth2 acquisition would
/// offer a delegated authorization for a vendor nobody registered an application with, and the
/// person who clicked it would arrive at the vendor's own error page holding no credential — which
/// is a connection outage that looks like a bug in the vendor. A deployment says which vendors it is
/// registered at; the catalogue says what running that registration means.
pub const ACQUISITION_CONNECTORS_ENV: &str = "FLUX_EXCHANGE_ACQUISITION_CONNECTORS";

/// The prefix of the per-connector OAuth2 client id, completed by the connector id in upper case.
pub const CLIENT_ID_ENV_PREFIX: &str = "FLUX_EXCHANGE_OAUTH_CLIENT_ID_";

/// The prefix of the per-connector OAuth2 client secret, completed the same way.
///
/// Optional: a public client registered for PKCE alone has none, and RFC 7636 is what makes that
/// safe rather than an omission.
pub const CLIENT_SECRET_ENV_PREFIX: &str = "FLUX_EXCHANGE_OAUTH_CLIENT_SECRET_";

/// The OAuth2 grants **this composition performs**, in the order it decides them.
///
/// `AuthorizationCode` is how a credential is *obtained* here and `RefreshToken` is how it is
/// renewed, which is exactly the pair `CredentialAcquirer` exposes to the delegated lane.
///
/// # Why `Password` is not in this list, when `redeem_password` exists
///
/// It is a grant this host performs and **not one a composition may derive from a declaration**.
/// X-75's password lane is composed explicitly: a deployment states the endpoint, opts into
/// [`AuthHazard::ResourceOwnerSecretShared`] by name, and a resource owner types a secret into this
/// host at request time. None of those three is in a catalogue declaration, so deriving one from
/// `grants: [password]` would stand up the exact grant RFC 9700 §2.4 says MUST NOT be used, out of
/// vendor metadata, without an operator having asked for it. `ClientCredentials` this host performs
/// nowhere at all.
///
/// A connector declaring a grant that is not here is refused **naming that grant**, and is never
/// quietly downgraded to another entry in its list — see [`performable`].
const PERFORMED_GRANTS: &[OAuthGrant] = &[OAuthGrant::AuthorizationCode, OAuthGrant::RefreshToken];

/// One deployment's registration identity at one vendor.
///
/// **Never connector data**, and the artifact agrees: upstream C-536 refuses to emit a `client_id`
/// into a canonical document at all, so what a declaration publishes is the *requirement* and this
/// is where the value comes from. It is the same shape [`AcquisitionRedirect`] already has, which
/// X-147 established and this makes the rule rather than the exception.
#[derive(Clone)]
pub struct OAuthRegistration {
    client_id: String,
    client_secret: Option<Secret>,
}

impl OAuthRegistration {
    /// Hold one registration identity.
    ///
    /// The client id is public by specification (RFC 6749 §2.2) and may appear in a refusal or a
    /// log; the secret may not, and [`Debug`](std::fmt::Debug) below is what makes that structural.
    pub fn new(client_id: impl Into<String>, client_secret: Option<Secret>) -> Self {
        Self {
            client_id: client_id.into(),
            client_secret,
        }
    }

    /// This deployment's client identifier at the vendor.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
}

impl std::fmt::Debug for OAuthRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OAuthRegistration")
            .field("client_id", &self.client_id)
            .field(
                "client_secret",
                &self.client_secret.as_ref().map(|_| "[REDACTED]"),
            )
            .finish()
    }
}

/// Why a catalogue declaration and this deployment's configuration did not compose an acquisition.
///
/// **Every variant refuses at composition and names a connector**, because that is the one fact an
/// operator acts on: which connector to register, which field the connector's own declaration is
/// missing, or which grant this host will not run. None carries a value — a client secret, a token
/// or a URL an operator typed — for the reason a startup log is permanent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompositionRefusal {
    /// The deployment named a connector this build does not catalogue.
    UnknownConnector {
        /// The unrecognised connector id, which is a name rather than a value.
        connector: String,
    },
    /// The connector declares no OAuth2-acquired credential to delegate.
    NoDeclaration {
        /// The connector the deployment registered.
        connector: String,
    },
    /// The connector declares more than one OAuth2-acquired credential.
    ///
    /// Refused rather than resolved by order: picking the first would make which credential a
    /// deployment acquires depend on the declaration order of a file in another repository.
    SeveralDeclarations {
        /// The connector the deployment registered.
        connector: String,
    },
    /// The connector declares a grant this composition does not perform — see [`PERFORMED_GRANTS`].
    GrantNotPerformed {
        /// The connector the deployment registered.
        connector: String,
        /// The declared grant, in the catalogue's own spelling.
        grant: &'static str,
    },
    /// The connector declares no grant by which this host could obtain a first credential.
    NoAcquiringGrant {
        /// The connector the deployment registered.
        connector: String,
    },
    /// A field the grant needs is empty in the declaration.
    IncompleteDeclaration {
        /// The connector whose declaration is short.
        connector: String,
        /// The declared field that is empty, in the catalogue's own spelling.
        field: &'static str,
    },
    /// The declaration names an endpoint the generated catalogue tables carry no base URL for.
    ///
    /// The module documentation has the whole of it: this is a real gap in the *tables*, not in the
    /// artifact — the canonical document carries `services[].base_url` and X-153 is the story that
    /// reads it. Refusing by name is what keeps it a startup refusal rather than a malformed URL.
    UnresolvableEndpoint {
        /// The connector whose declaration names it.
        connector: String,
        /// The declared endpoint (service) name. A name, never a URL.
        endpoint: String,
    },
    /// The declaration's base URL still carries a `{placeholder}` this composition cannot fill.
    ///
    /// Endpoint variables are per-connection settings resolved for one tenant at request time; a
    /// startup composition has no tenant, so a templated base URL is refused rather than sent with
    /// the braces still in it.
    TemplatedBaseUrl {
        /// The connector whose base URL is templated.
        connector: String,
    },
    /// The deployment registered the connector and supplied no client id.
    NoRegistration {
        /// The connector the deployment registered.
        connector: String,
        /// The variable that would have carried it. A name, never a value.
        setting: String,
    },
    /// The deployment registered a connector and configured no acquisition redirect URI.
    NoRedirect {
        /// The connector the deployment registered.
        connector: String,
    },
    /// The composed grant, performer or registry refused the shape the declaration produced.
    Unusable {
        /// The connector being composed.
        connector: String,
        /// The value-free reason from the constructor that refused.
        reason: &'static str,
    },
}

impl std::fmt::Display for CompositionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "refusing to start: ")?;
        match self {
            Self::UnknownConnector { connector } => write!(
                f,
                "{ACQUISITION_CONNECTORS_ENV} names `{connector}`, which this build does not \
                 catalogue",
            ),
            Self::NoDeclaration { connector } => write!(
                f,
                "`{connector}` declares no OAuth2-acquired credential, so there is no delegated \
                 authorization to compose",
            ),
            Self::SeveralDeclarations { connector } => write!(
                f,
                "`{connector}` declares more than one OAuth2-acquired credential, and which one a \
                 deployment acquires must not depend on declaration order",
            ),
            Self::GrantNotPerformed { connector, grant } => write!(
                f,
                "`{connector}` declares the `{grant}` grant, which this host does not perform from \
                 a catalogue declaration; it is not downgraded to another grant in that list",
            ),
            Self::NoAcquiringGrant { connector } => write!(
                f,
                "`{connector}` declares no `authorization_code` grant, so this host has no way to \
                 obtain a first credential for it",
            ),
            Self::IncompleteDeclaration { connector, field } => write!(
                f,
                "`{connector}`'s OAuth2 declaration has an empty `{field}`, which is too \
                 incomplete to compose an authorization from",
            ),
            Self::UnresolvableEndpoint {
                connector,
                endpoint,
            } => write!(
                f,
                "`{connector}`'s OAuth2 declaration resolves against the `{endpoint}` endpoint, \
                 and the generated catalogue tables carry no base URL for it — only the \
                 connector's default service has one. The canonical document does; reading it is \
                 X-153",
            ),
            Self::TemplatedBaseUrl { connector } => write!(
                f,
                "`{connector}`'s base URL carries an unresolved `{{placeholder}}`, which is a \
                 per-connection setting and not something a startup composition can fill",
            ),
            Self::NoRegistration { connector, setting } => write!(
                f,
                "{ACQUISITION_CONNECTORS_ENV} names `{connector}` and {setting} is unset; the \
                 registration identity is this deployment's and is never read from the catalogue",
            ),
            Self::NoRedirect { connector } => write!(
                f,
                "{ACQUISITION_CONNECTORS_ENV} names `{connector}` and this deployment configured \
                 no acquisition redirect URI ({})",
                crate::acquisition_redirect::SETTING,
            ),
            Self::Unusable { connector, reason } => {
                write!(f, "`{connector}`'s acquisition does not compose: {reason}")
            }
        }
    }
}

impl std::error::Error for CompositionRefusal {}

/// **This deployment's acquisition registry, derived from the released catalogue.**
///
/// The production composition, and the replacement for the empty registry this module bound while
/// no declaration existed to read.
///
/// # Errors
///
/// One [`CompositionRefusal`], naming the connector. Startup refuses rather than binding a partial
/// registry: a deployment that registered a connector and cannot acquire for it must be told, not
/// handed a surface that answers `422` for the one connector its operator configured.
pub fn configured(
    redirect: Option<&AcquisitionRedirect>,
) -> Result<AcquisitionBindings, CompositionRefusal> {
    from_environment(|name| std::env::var(name).ok(), redirect)
}

/// [`configured`], reading by name from an injected source so tests do not mutate process-wide
/// environment — the shape `crate::auth_posture::read` already uses.
fn from_environment(
    lookup: impl Fn(&str) -> Option<String>,
    redirect: Option<&AcquisitionRedirect>,
) -> Result<AcquisitionBindings, CompositionRefusal> {
    let mut bindings = Vec::new();
    for connector in lookup(ACQUISITION_CONNECTORS_ENV)
        .as_deref()
        .unwrap_or_default()
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        let Some(provider) = connector_catalog::provider(ProviderKey::id(connector)) else {
            return Err(CompositionRefusal::UnknownConnector {
                connector: connector.to_owned(),
            });
        };
        let client_id_setting = registration_variable(CLIENT_ID_ENV_PREFIX, connector);
        let Some(client_id) = lookup(&client_id_setting).filter(|value| !value.trim().is_empty())
        else {
            return Err(CompositionRefusal::NoRegistration {
                connector: connector.to_owned(),
                setting: client_id_setting,
            });
        };
        let registration = OAuthRegistration::new(
            client_id.trim(),
            lookup(&registration_variable(CLIENT_SECRET_ENV_PREFIX, connector))
                .filter(|value| !value.is_empty())
                .map(|value| Secret::new(&value)),
        );
        let Some(redirect) = redirect else {
            return Err(CompositionRefusal::NoRedirect {
                connector: connector.to_owned(),
            });
        };
        bindings.push(binding_from_catalogue(provider, &registration, redirect)?);
    }

    let composed = bindings.len();
    AcquisitionBindings::new(bindings, redirect).map_err(|reason| CompositionRefusal::Unusable {
        // The registry's own refusals are about the set rather than one member, and the set is
        // empty of members only when nothing was selected — so naming the count is the honest
        // address here, where naming one connector would be a guess.
        connector: format!("{composed} registered connector(s)"),
        reason,
    })
}

/// The environment variable one connector's registration half is read from.
///
/// Upper case with `-` folded to `_`, which is the ordinary environment spelling and the one every
/// catalogued connector id survives: ids are lowercase ASCII with `_`, so `microsoft_graph` becomes
/// `MICROSOFT_GRAPH` and nothing collides.
fn registration_variable(prefix: &str, connector: &str) -> String {
    format!(
        "{prefix}{}",
        connector.to_ascii_uppercase().replace('-', "_")
    )
}

/// Compose one connector's delegated binding from its own catalogue declaration.
///
/// # Errors
///
/// One [`CompositionRefusal`] naming this connector.
pub fn binding_from_catalogue(
    provider: &'static Provider,
    registration: &OAuthRegistration,
    redirect: &AcquisitionRedirect,
) -> Result<AcquisitionBinding, CompositionRefusal> {
    let (credential, spec) = declared_oauth2(provider)?;
    let base = endpoint_base(provider.id, provider.base_url, spec)?;
    binding_from_declaration(
        provider.id,
        credential.name,
        credential.hazard,
        spec,
        &base,
        registration,
        redirect,
    )
}

/// The single OAuth2-acquired credential a connector declares, with its declaration.
///
/// # Errors
///
/// [`CompositionRefusal::NoDeclaration`] or [`CompositionRefusal::SeveralDeclarations`]. A
/// connector's other credentials — GitLab's `Static` personal access token beside its
/// `gitlab.oauth_token` — are not candidates and are not counted.
pub fn declared_oauth2(
    provider: &'static Provider,
) -> Result<(&'static Credential, &'static OAuth2), CompositionRefusal> {
    let mut declared = provider.auth.iter().filter_map(|credential| {
        match credential.acquire {
            Acquisition::OAuth2(spec) => Some((credential, spec)),
            // Named rather than caught by a wildcard: `catalog::Acquisition` is deliberately not
            // `#[non_exhaustive]` so a new acquisition kind is a compile error at every consumer
            // that decides what to do with one, and a `_` arm here would silently make the next one
            // "not delegated".
            Acquisition::Static | Acquisition::Minted { .. } | Acquisition::BasicJoin { .. } => {
                None
            }
        }
    });
    let Some(first) = declared.next() else {
        return Err(CompositionRefusal::NoDeclaration {
            connector: provider.id.to_owned(),
        });
    };
    if declared.next().is_some() {
        return Err(CompositionRefusal::SeveralDeclarations {
            connector: provider.id.to_owned(),
        });
    }
    Ok(first)
}

/// The base URL a declaration's `authorize_path` and `token_path` are joined onto.
///
/// An **empty** `endpoint` means the connector's own base URL, which is what
/// `catalog::OAuth2::endpoint` documents. A **named** one is a service, and the generated tables
/// carry no base URL for a service — see this module's documentation for the measurement and for
/// what closes it.
///
/// # Errors
///
/// [`CompositionRefusal::UnresolvableEndpoint`] for a named endpoint, and
/// [`CompositionRefusal::TemplatedBaseUrl`] for a base URL still carrying a `{placeholder}`. Never
/// a guess: a wrong base URL is an authorization request sent to somebody else's host.
pub fn endpoint_base(
    connector: &str,
    provider_base_url: &str,
    spec: &OAuth2,
) -> Result<String, CompositionRefusal> {
    if !spec.endpoint.is_empty() {
        return Err(CompositionRefusal::UnresolvableEndpoint {
            connector: connector.to_owned(),
            endpoint: spec.endpoint.to_owned(),
        });
    }
    if provider_base_url.contains('{') {
        return Err(CompositionRefusal::TemplatedBaseUrl {
            connector: connector.to_owned(),
        });
    }
    Ok(provider_base_url.trim_end_matches('/').to_owned())
}

/// Refuse a declared grant list carrying one this composition does not perform.
///
/// Both halves matter and they are separate refusals: a grant that is *not* in
/// [`PERFORMED_GRANTS`] is named and refused, and a list carrying none of the grants that could
/// obtain a first credential is refused as a whole. Between them there is no path where a
/// connector declaring `[password, refresh_token]` quietly acquires by refresh — which is a
/// renewal of a credential nothing ever obtained.
///
/// # Errors
///
/// [`CompositionRefusal::GrantNotPerformed`] naming the grant, or
/// [`CompositionRefusal::NoAcquiringGrant`].
pub fn performable(connector: &str, grants: &[OAuthGrant]) -> Result<(), CompositionRefusal> {
    for grant in grants {
        if !PERFORMED_GRANTS.contains(grant) {
            return Err(CompositionRefusal::GrantNotPerformed {
                connector: connector.to_owned(),
                grant: grant_word(*grant),
            });
        }
    }
    if !grants.contains(&OAuthGrant::AuthorizationCode) {
        return Err(CompositionRefusal::NoAcquiringGrant {
            connector: connector.to_owned(),
        });
    }
    Ok(())
}

/// The catalogue's own spelling of one grant.
///
/// Matched exhaustively rather than derived, for the reason `AuthHazard::as_str` gives: the word
/// reaches an operator's startup log, and it must not move because a derive attribute did.
const fn grant_word(grant: OAuthGrant) -> &'static str {
    match grant {
        OAuthGrant::AuthorizationCode => "authorization_code",
        OAuthGrant::Password => "password",
        OAuthGrant::RefreshToken => "refresh_token",
        OAuthGrant::ClientCredentials => "client_credentials",
    }
}

/// The declared acquisition hazard, in this host's vocabulary.
///
/// Exhaustive and wildcard-free on purpose: `catalog::AuthHazard` is not `#[non_exhaustive]`
/// precisely so that a hazard added upstream is a **compile error** here rather than a value a
/// catch-all arm quietly maps to "no weakness declared". `None` means *nothing was declared*, which
/// is not the same as *reviewed and found safe* — the catalogue says so itself.
pub const fn declared_hazard(declared: Option<DeclaredHazard>) -> Option<AuthHazard> {
    match declared {
        None => None,
        Some(DeclaredHazard::ResourceOwnerSecretShared) => {
            Some(AuthHazard::ResourceOwnerSecretShared)
        }
    }
}

/// Compose one delegated grant from a declaration and this deployment's registration.
///
/// # Where each value comes from, which is the whole point of this function
///
/// The declaration supplies the authorize path and the scopes; the deployment supplies the client
/// id, the client secret and the redirect URI. **A caller supplies nothing** — there is no argument
/// here a request could reach, so a scope absent from the connector's list is one this host does not
/// request and a host is one nobody can name.
///
/// # Errors
///
/// [`CompositionRefusal::IncompleteDeclaration`] naming the empty field, or
/// [`CompositionRefusal::Unusable`] carrying [`DelegatedGrant::new`]'s value-free reason.
pub fn grant_from_declaration(
    connector: &str,
    spec: &OAuth2,
    base: &str,
    registration: &OAuthRegistration,
    redirect: &AcquisitionRedirect,
) -> Result<DelegatedGrant, CompositionRefusal> {
    // **Exhaustive, so the two fields this host must not read are named rather than merely
    // unused.** `client_id` is the one Decision 0022 settles: the artifact publishes the
    // registration *requirement* and upstream C-536 refuses to emit a value, so reading one here
    // would be trusting vendor metadata for a deployment's own identity. `redirect` models a
    // loopback port and path — the local-development shape a desktop tool binds — and X-147 already
    // decided a hosted deployment's redirect is configuration. A destructure means a field added
    // upstream is a compile error rather than a fact silently ignored.
    let OAuth2 {
        endpoint: _,
        authorize_path,
        token_path: _,
        client_id: _,
        scopes,
        grants,
        redirect: _,
    } = *spec;
    performable(connector, grants)?;
    if authorize_path.is_empty() {
        return Err(CompositionRefusal::IncompleteDeclaration {
            connector: connector.to_owned(),
            field: "authorize_path",
        });
    }
    if scopes.is_empty() {
        return Err(CompositionRefusal::IncompleteDeclaration {
            connector: connector.to_owned(),
            field: "scopes",
        });
    }
    DelegatedGrant::new(
        &joined(base, authorize_path),
        registration.client_id(),
        registration.client_secret.clone(),
        scopes.iter().map(|scope| (*scope).to_owned()),
        redirect.clone(),
    )
    .map_err(|reason| CompositionRefusal::Unusable {
        connector: connector.to_owned(),
        reason,
    })
}

/// Compose one binding from a declaration whose endpoint base URL is already resolved.
///
/// Split from [`binding_from_catalogue`] at exactly the seam this module's documentation names: the
/// base URL is the one declared fact the generated tables do not carry, so it is an argument here
/// and [`endpoint_base`] is what production has to get it from.
///
/// # Errors
///
/// One [`CompositionRefusal`] naming this connector.
#[allow(clippy::too_many_arguments)]
pub fn binding_from_declaration(
    connector: &str,
    credential: &str,
    hazard: Option<DeclaredHazard>,
    spec: &OAuth2,
    base: &str,
    registration: &OAuthRegistration,
    redirect: &AcquisitionRedirect,
) -> Result<AcquisitionBinding, CompositionRefusal> {
    let grant = grant_from_declaration(connector, spec, base, registration, redirect)?;
    if spec.token_path.is_empty() {
        return Err(CompositionRefusal::IncompleteDeclaration {
            connector: connector.to_owned(),
            field: "token_path",
        });
    }
    // `TokenEndpointBehavior::Standard`, and that is a measurement rather than an omission: the
    // table on `TokenEndpointBehavior::Babelforce` records that `authorization_code` is the one
    // grant babelforce's endpoint does not read `expires_in` for, and babelforce declares no
    // `authorization_code` grant anyway — so no connector reaching this line has a quirk that
    // applies to the form it will send.
    let performer = HttpCredentialAcquirer::new(
        connector,
        &joined(base, spec.token_path),
        TokenEndpointBehavior::Standard,
    )
    .map_err(|reason| CompositionRefusal::Unusable {
        connector: connector.to_owned(),
        reason,
    })?;
    Ok(AcquisitionBinding::delegating(
        connector,
        credential,
        // **Read from the declaration, not stated by the composition** (X-154). babelforce is the
        // first released connector to declare one, so X-74's gate is now driven by released
        // metadata rather than only by a fixture.
        declared_hazard(hazard),
        grant,
        performer,
    ))
}

/// Join a declared path onto a base URL without doubling or dropping the separator.
fn joined(base: &str, path: &str) -> String {
    format!(
        "{}/{}",
        base.trim_end_matches('/'),
        path.trim_start_matches('/')
    )
}

/// Endpoint-specific request behavior owned by the concrete HTTP performer.
#[derive(Clone)]
pub enum TokenEndpointBehavior {
    /// OAuth password and refresh forms without vendor additions.
    Standard,
    /// babelforce's measured token-endpoint additions.
    ///
    /// Measured against the vendor implementation on 2026-08-02 (there is no vendor specification
    /// for this field): password accepts an optional `expires_in`; refresh consumes `expires_in`
    /// and `account_id`; authorization-code ignores it; link clamps its requested lifetime; client
    /// credentials defaults to `-1`. This binding intentionally implements only password and
    /// refresh, and confines both extra fields to this variant.
    Babelforce(BabelforceTokenEndpointQuirks),
}

/// The babelforce-only inputs selected by server configuration, never by a request.
#[derive(Clone)]
pub struct BabelforceTokenEndpointQuirks {
    /// Requested password-grant lifetime, when the deployment selected one for this endpoint.
    pub password_expires_in: Option<u64>,
    /// Requested refresh lifetime, when selected for this endpoint.
    pub refresh_expires_in: Option<u64>,
    /// babelforce account identifier required by this endpoint's refresh form, when applicable.
    pub refresh_account_id: Option<String>,
}

/// The server's concrete HTTP binding of the host acquisition port.
pub struct HttpCredentialAcquirer {
    connector: String,
    client: Client,
    endpoint: Url,
    behavior: TokenEndpointBehavior,
    delegated: Option<Arc<DelegatedGrant>>,
}

impl HttpCredentialAcquirer {
    /// Perform the delegated authorization-code grant against this same token endpoint.
    ///
    /// Without this, [`redeem_authorization_code`](CredentialAcquirer::redeem_authorization_code)
    /// keeps the port's default refusal: a performer bound with no delegated grant does not silently
    /// half-perform one.
    ///
    /// `pub(crate)` and taking the shared `Arc` since X-147's review: the only caller is
    /// [`AcquisitionBinding::delegating`], which sets the binding's half and this one from one
    /// value. A composition that could call this separately could give the performer a redirect the
    /// authorization URL never carried, and the vendor would answer that at the last step.
    pub(crate) fn performing_delegated_grant(mut self, grant: Arc<DelegatedGrant>) -> Self {
        self.delegated = Some(grant);
        self
    }

    /// Construct one startup-owned endpoint binding.
    ///
    /// # Errors
    ///
    /// A value-free reason when the endpoint is not a URL, when it is cleartext anywhere but a
    /// loopback literal, or when babelforce's quirks are bound to another connector.
    pub fn new(
        connector: &str,
        endpoint: &str,
        behavior: TokenEndpointBehavior,
    ) -> Result<Self, &'static str> {
        if matches!(&behavior, TokenEndpointBehavior::Babelforce(_)) && connector != "babelforce" {
            return Err("babelforce token-endpoint quirks may only bind connector `babelforce`");
        }
        let endpoint = Url::parse(endpoint).map_err(|_| "credential endpoint URL is invalid")?;
        // **A loopback literal rather than `cfg!(test)`**, for the reason M4 gives on
        // `DelegatedGrant::new` — the re-review caught this one still standing a screen below that
        // argument. Keying the exception to the build meant the rule was switched off in the only
        // place that could exercise it, and this endpoint carries resource-owner passwords and
        // refresh tokens in a POST body. Keying it to the address is the same rule
        // `crate::acquisition_redirect` and `crate::hosted_origin` use, it is the exception a
        // checkout actually needs, and a test can drive the refusal.
        if endpoint.scheme() != "https"
            && !(endpoint.scheme() == "http"
                && endpoint
                    .host_str()
                    .is_some_and(crate::acquisition_redirect::is_loopback_literal))
        {
            return Err("credential endpoint URL must use HTTPS, or HTTP on a loopback literal");
        }
        // Token forms contain replayable resource-owner and refresh secrets. Even a same-origin
        // redirect changes which endpoint made the decision, and reqwest's default redirect policy
        // is therefore not admissible here. The client is constructed inside this type so callers
        // cannot accidentally replace this rule with `Client::new()`.
        let client = Client::builder()
            .redirect(Policy::none())
            .build()
            .map_err(|_| "credential HTTP client could not be constructed")?;
        Ok(Self {
            connector: connector.to_owned(),
            client,
            endpoint,
            behavior,
            delegated: None,
        })
    }

    async fn send(
        &self,
        form: &[(&str, &str)],
        require_rotated_refresh: bool,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        let unusable = || {
            if require_rotated_refresh {
                AcquisitionRefusal::RefreshOutcomeUnusable
            } else {
                AcquisitionRefusal::InvalidResponse
            }
        };
        let response = self
            .client
            .post(self.endpoint.clone())
            .form(form)
            .send()
            .await
            .map_err(|_| AcquisitionRefusal::Unreachable)?;
        let status = response.status();
        let body = response.text().await.map_err(|_| unusable())?;

        if !status.is_success() {
            return Err(classify_rejection(status.as_u16(), &body));
        }

        let response: TokenResponse = serde_json::from_str(&body).map_err(|_| unusable())?;
        if response.access_token.is_empty() {
            return Err(unusable());
        }
        if require_rotated_refresh && response.refresh_token.as_deref().is_none_or(str::is_empty) {
            return Err(unusable());
        }
        let expires_at = expiry_from_response(
            now_unix().map_err(|_| unusable())?,
            response.expire_time,
            response.expires_in,
        )
        .map_err(|_| unusable())?;
        Ok(AcquiredCredential::new(
            Secret::new(&response.access_token),
            response.refresh_token.as_deref().map(Secret::new),
            expires_at,
        ))
    }
}

#[async_trait]
impl CredentialAcquirer for HttpCredentialAcquirer {
    fn binding_connector(&self) -> Option<&str> {
        Some(&self.connector)
    }

    async fn redeem_password(
        &self,
        redemption: PasswordRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        let expires = match &self.behavior {
            TokenEndpointBehavior::Babelforce(quirks) => {
                quirks.password_expires_in.map(|value| value.to_string())
            }
            TokenEndpointBehavior::Standard => None,
        };
        let mut form = vec![
            ("grant_type", "password"),
            ("username", redemption.username()),
            ("password", redemption.password()),
        ];
        if let Some(expires) = expires.as_deref() {
            form.push(("expires_in", expires));
        }
        self.send(&form, false).await
    }

    async fn redeem_refresh(
        &self,
        redemption: RefreshRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        let mut expires = None;
        let mut account_id = None;
        let require_rotated_refresh =
            matches!(&self.behavior, TokenEndpointBehavior::Babelforce(_));
        if let TokenEndpointBehavior::Babelforce(quirks) = &self.behavior {
            expires = quirks.refresh_expires_in.map(|value| value.to_string());
            account_id = quirks.refresh_account_id.as_deref();
        }
        let mut form = vec![
            ("grant_type", "refresh_token"),
            ("refresh_token", redemption.refresh_token()),
        ];
        if let Some(expires) = expires.as_deref() {
            form.push(("expires_in", expires));
        }
        if let Some(account_id) = account_id {
            form.push(("account_id", account_id));
        }
        self.send(&form, require_rotated_refresh).await
    }

    /// Redeem one authorization code a person granted in their own browser.
    ///
    /// **No endpoint quirk applies here, and that is measured rather than assumed.** The behaviour
    /// table on [`TokenEndpointBehavior::Babelforce`] records that `authorization_code` is the one
    /// grant that does not read `expires_in` at all — so a requested lifetime attached to this form
    /// would be a field the vendor ignores, which is the shape a caller reads as honoured.
    ///
    /// `code_verifier` is mandatory and there is no branch that omits it. PKCE is what makes the
    /// code useless to everything else that saw the redirect URL, and a performer that could send
    /// this form without one would be one a later refactor sends it without.
    async fn redeem_authorization_code(
        &self,
        redemption: AuthorizationCodeRedemption<'_>,
    ) -> Result<AcquiredCredential, AcquisitionRefusal> {
        let Some(delegated) = self.delegated.as_ref() else {
            return Err(AcquisitionRefusal::GrantNotPerformed);
        };
        let mut form = vec![
            ("grant_type", "authorization_code"),
            ("code", redemption.code()),
            ("code_verifier", redemption.verifier()),
            // Re-presented verbatim (RFC 6749 §4.1.3). It is the same value the authorization
            // request carried, from one `DelegatedGrant`, so the two cannot be two spellings.
            ("redirect_uri", delegated.redirect().as_str()),
            ("client_id", delegated.client_id()),
        ];
        if let Some(secret) = delegated.client_secret.as_ref() {
            form.push(("client_secret", secret.expose_secret()));
        }
        self.send(&form, false).await
    }
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    expires_in: Option<i64>,
    #[serde(default)]
    expire_time: Option<i64>,
}

fn now_unix() -> Result<i64, AcquisitionRefusal> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AcquisitionRefusal::InvalidResponse)
        .and_then(|duration| {
            i64::try_from(duration.as_secs()).map_err(|_| AcquisitionRefusal::InvalidResponse)
        })
}

fn classify_rejection(status: u16, body: &str) -> AcquisitionRefusal {
    let structured = serde_json::from_str::<TokenEndpointError>(body).ok();
    let code = structured
        .as_ref()
        .and_then(|error| error.error.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let description = structured
        .as_ref()
        .and_then(|error| error.error_description.as_deref())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let mfa_code = matches!(
        code.as_str(),
        "mfa_required" | "multifactor_required" | "two_factor_required" | "2fa_required"
    );
    let interaction_is_mfa = code == "interaction_required"
        && ["multi-factor", "multifactor", "two-factor", "2fa", "mfa"]
            .iter()
            .any(|word| description.contains(word));
    if mfa_code || interaction_is_mfa {
        AcquisitionRefusal::MfaRequired
    } else if status == 400
        || status == 401
        || code == "invalid_grant"
        || code == "invalid_credentials"
    {
        AcquisitionRefusal::CredentialsRejected
    } else {
        AcquisitionRefusal::VendorRejected
    }
}

#[derive(Deserialize)]
struct TokenEndpointError {
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    error_description: Option<String>,
}

fn expiry_from_response(
    now: i64,
    expire_time_millis: Option<i64>,
    expires_in: Option<i64>,
) -> Result<Option<i64>, AcquisitionRefusal> {
    if let Some(expire_time_millis) = expire_time_millis {
        if expire_time_millis < 0 {
            return Err(AcquisitionRefusal::InvalidResponse);
        }
        return Ok(Some(expire_time_millis / 1_000));
    }
    match expires_in {
        None | Some(-1) => Ok(None),
        Some(value) if value >= 0 => now
            .checked_add(value)
            .map(Some)
            .ok_or(AcquisitionRefusal::InvalidResponse),
        Some(_) => Err(AcquisitionRefusal::InvalidResponse),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use axum::body::Bytes;
    use axum::extract::State;
    use axum::http::StatusCode;
    use axum::routing::post;
    use axum::Router;
    use exchange_host::CredentialAcquirer as _;
    use tokio::net::TcpListener;

    use super::*;

    async fn recording_endpoint(
        State(recorded): State<Arc<Mutex<Vec<String>>>>,
        body: Bytes,
    ) -> (StatusCode, &'static str) {
        recorded
            .lock()
            .expect("request recorder lock")
            .push(String::from_utf8(body.to_vec()).expect("form body is UTF-8"));
        (
            StatusCode::OK,
            r#"{"access_token":"access","refresh_token":"refresh","expires_in":60}"#,
        )
    }

    async fn echoing_rejection(body: Bytes) -> (StatusCode, String) {
        (
            StatusCode::UNAUTHORIZED,
            format!(
                r#"{{"error":"invalid_grant","echo":"{}"}}"#,
                String::from_utf8_lossy(&body)
            ),
        )
    }

    async fn serve(app: Router) -> (String, tokio::task::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind fixture endpoint");
        let address = listener.local_addr().expect("fixture address");
        let task = tokio::spawn(async move {
            axum::serve(listener, app)
                .await
                .expect("serve fixture endpoint");
        });
        (format!("http://{address}/token"), task)
    }

    #[tokio::test]
    async fn babelforce_form_quirks_do_not_leak_to_a_standard_endpoint() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, task) = serve(
            Router::new()
                .route("/token", post(recording_endpoint))
                .with_state(Arc::clone(&recorded)),
        )
        .await;
        let babelforce = HttpCredentialAcquirer::new(
            "babelforce",
            &endpoint,
            TokenEndpointBehavior::Babelforce(BabelforceTokenEndpointQuirks {
                password_expires_in: Some(3_600),
                refresh_expires_in: Some(7_200),
                refresh_account_id: Some("account-42".to_owned()),
            }),
        )
        .expect("babelforce fixture binding");
        let standard =
            HttpCredentialAcquirer::new("second", &endpoint, TokenEndpointBehavior::Standard)
                .expect("standard fixture binding");
        let username = Secret::new("alice@example.test");
        let password = Secret::new("password-secret");
        let refresh = Secret::new("refresh-secret");

        babelforce
            .redeem_password(PasswordRedemption::new(&username, &password))
            .await
            .expect("babelforce password response");
        standard
            .redeem_password(PasswordRedemption::new(&username, &password))
            .await
            .expect("standard password response");
        babelforce
            .redeem_refresh(RefreshRedemption::new(&refresh))
            .await
            .expect("babelforce refresh response");
        standard
            .redeem_refresh(RefreshRedemption::new(&refresh))
            .await
            .expect("standard refresh response");

        let forms = recorded.lock().expect("request recorder lock");
        assert!(forms[0].contains("expires_in=3600"));
        assert!(!forms[1].contains("expires_in="));
        assert!(!forms[1].contains("account_id="));
        assert!(forms[2].contains("expires_in=7200"));
        assert!(forms[2].contains("account_id=account-42"));
        assert!(!forms[3].contains("expires_in="));
        assert!(!forms[3].contains("account_id="));
        task.abort();
    }

    #[tokio::test]
    async fn a_vendor_echoing_the_password_cannot_put_it_in_our_refusal() {
        let (endpoint, task) = serve(Router::new().route("/token", post(echoing_rejection))).await;
        let performer =
            HttpCredentialAcquirer::new("standard", &endpoint, TokenEndpointBehavior::Standard)
                .expect("standard fixture binding");
        let username = Secret::new("alice");
        let password = Secret::new("vendor-echoed-mfa-password");

        let refusal = performer
            .redeem_password(PasswordRedemption::new(&username, &password))
            .await
            .expect_err("fixture endpoint rejects");

        assert_eq!(refusal, AcquisitionRefusal::CredentialsRejected);
        assert!(!format!("{refusal}").contains("vendor-echoed-mfa-password"));
        assert!(!format!("{refusal:?}").contains("vendor-echoed-mfa-password"));
        task.abort();
    }

    #[tokio::test]
    async fn acquisition_secrets_are_never_replayed_across_a_redirect() {
        async fn redirect() -> (StatusCode, [(axum::http::HeaderName, &'static str); 1]) {
            (
                StatusCode::TEMPORARY_REDIRECT,
                [(axum::http::header::LOCATION, "http://127.0.0.1:9/stolen")],
            )
        }

        let (endpoint, task) = serve(Router::new().route("/token", post(redirect))).await;
        let performer =
            HttpCredentialAcquirer::new("standard", &endpoint, TokenEndpointBehavior::Standard)
                .expect("redirect fixture binding");
        let username = Secret::new("alice");
        let password = Secret::new("redirect-must-not-replay-this");

        let refusal = performer
            .redeem_password(PasswordRedemption::new(&username, &password))
            .await
            .expect_err("a redirect is a vendor refusal, not another token request");
        assert_eq!(refusal, AcquisitionRefusal::VendorRejected);
        task.abort();
    }

    #[test]
    fn babelforce_quirks_refuse_a_non_babelforce_binding() {
        let performer = HttpCredentialAcquirer::new(
            "babelforce",
            "http://127.0.0.1:9/token",
            TokenEndpointBehavior::Babelforce(BabelforceTokenEndpointQuirks {
                password_expires_in: Some(60),
                refresh_expires_in: Some(60),
                refresh_account_id: Some("account".to_owned()),
            }),
        )
        .expect("babelforce performer");
        let result = AcquisitionBindings::new(
            [AcquisitionBinding::new(
                "second",
                "second.access_token",
                Some(AuthHazard::ResourceOwnerSecretShared),
                Arc::new(performer),
            )],
            None,
        );
        assert_eq!(
            result
                .expect_err("babelforce behavior must be bound structurally")
                .to_string(),
            "a credential-acquisition performer is bound to another connector",
        );
    }

    /// **A token endpoint may not be cleartext off a loopback literal** (X-147 re-review, nit 2).
    ///
    /// This rule was written as `!cfg!(test)`, which switched it off in the only place that could
    /// exercise it — one screen below M4's own argument against exactly that construct. The endpoint
    /// this guards carries resource-owner passwords and refresh tokens in a POST body, so the rule
    /// is worth having and worth being able to run.
    #[test]
    fn a_cleartext_token_endpoint_is_refused_unless_it_is_loopback() {
        assert_eq!(
            HttpCredentialAcquirer::new(
                "second",
                "http://vendor.example.test/token",
                TokenEndpointBehavior::Standard,
            )
            .err()
            .expect("cleartext to a routable host must be refused"),
            "credential endpoint URL must use HTTPS, or HTTP on a loopback literal",
        );
        assert!(
            HttpCredentialAcquirer::new(
                "second",
                "https://vendor.example.test/token",
                TokenEndpointBehavior::Standard,
            )
            .is_ok(),
            "and HTTPS anywhere is admitted",
        );
        assert!(
            HttpCredentialAcquirer::new(
                "second",
                "http://127.0.0.1:9/token",
                TokenEndpointBehavior::Standard,
            )
            .is_ok(),
            "as is a checkout's own loopback fixture, which is the exception that is actually needed",
        );
    }

    #[tokio::test]
    async fn babelforce_refresh_refuses_a_missing_or_empty_rotated_refresh_token() {
        async fn no_rotation() -> &'static str {
            r#"{"access_token":"new-access","refresh_token":""}"#
        }
        let (endpoint, task) = serve(Router::new().route("/token", post(no_rotation))).await;
        let performer = HttpCredentialAcquirer::new(
            "babelforce",
            &endpoint,
            TokenEndpointBehavior::Babelforce(BabelforceTokenEndpointQuirks {
                password_expires_in: None,
                refresh_expires_in: None,
                refresh_account_id: None,
            }),
        )
        .expect("babelforce fixture binding");
        let refresh = Secret::new("old-refresh");

        let refusal = performer
            .redeem_refresh(RefreshRedemption::new(&refresh))
            .await
            .expect_err("babelforce refresh must rotate its refresh token");
        assert_eq!(refusal, AcquisitionRefusal::RefreshOutcomeUnusable);
        task.abort();
    }

    #[test]
    fn babelforce_absolute_milliseconds_and_never_expiry_are_normalized() {
        assert_eq!(
            expiry_from_response(1_800_000_000, Some(1_900_000_123_456), Some(60)),
            Ok(Some(1_900_000_123)),
            "expire_time is the vendor's absolute UTC milliseconds and wins over expires_in",
        );
        assert_eq!(
            expiry_from_response(1_800_000_000, None, Some(-1)),
            Ok(None),
            "the vendor's -1 spelling means never expires, not one second ago",
        );
        assert_eq!(
            expiry_from_response(1_800_000_000, None, Some(-2)),
            Err(AcquisitionRefusal::InvalidResponse),
        );
    }

    /// The one canonical spelling `crate::acquisition_redirect` admits, as a checked value.
    fn redirect() -> AcquisitionRedirect {
        AcquisitionRedirect::parse("https://exchange.example.test/api/acquisitions/callback")
            .expect("a canonical redirect")
    }

    fn delegated(redirect: AcquisitionRedirect) -> DelegatedGrant {
        DelegatedGrant::new(
            // A loopback literal, so the transport rule is the one that actually runs here rather
            // than one a `cfg!(test)` escape switched off — see `DelegatedGrant::new`.
            "http://127.0.0.1:9/oauth/authorize",
            "exchange-client",
            Some(Secret::new("client-secret-never-print-this")),
            ["read_api".to_owned(), "read_user".to_owned()],
            redirect,
        )
        .expect("a delegated grant fixture")
    }

    /// **X-147.** The delegated token request presents the proof key, the redirect URI and the
    /// client — and none of the password grant's endpoint quirks.
    #[tokio::test]
    async fn a_delegated_token_request_carries_pkce_and_no_lifetime_quirk() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, task) = serve(
            Router::new()
                .route("/token", post(recording_endpoint))
                .with_state(Arc::clone(&recorded)),
        )
        .await;
        // Bound with babelforce's measured quirks on purpose: the table on `TokenEndpointBehavior`
        // records that `authorization_code` is the grant that does not read `expires_in`, and a
        // quirk leaking into this form would be a lifetime the vendor silently ignores.
        let performer = HttpCredentialAcquirer::new(
            "babelforce",
            &endpoint,
            TokenEndpointBehavior::Babelforce(BabelforceTokenEndpointQuirks {
                password_expires_in: Some(3_600),
                refresh_expires_in: Some(7_200),
                refresh_account_id: Some("account-42".to_owned()),
            }),
        )
        .expect("fixture binding")
        .performing_delegated_grant(Arc::new(delegated(redirect())));
        let code = Secret::new("authorization-code");
        let verifier = Secret::new("the-code-verifier");

        performer
            .redeem_authorization_code(AuthorizationCodeRedemption::new(&code, &verifier))
            .await
            .expect("the fixture endpoint answers a token");

        let forms = recorded.lock().expect("request recorder lock");
        let form = forms.first().expect("one token request");
        assert!(form.contains("grant_type=authorization_code"), "{form}");
        assert!(form.contains("code=authorization-code"), "{form}");
        assert!(form.contains("code_verifier=the-code-verifier"), "{form}");
        assert!(form.contains("client_id=exchange-client"), "{form}");
        assert!(
            form.contains("client_secret=client-secret-never-print-this"),
            "{form}",
        );
        assert!(
            form.contains(
                "redirect_uri=https%3A%2F%2Fexchange.example.test%2Fapi%2Facquisitions%2Fcallback"
            ),
            "the redirect URI is re-presented exactly as configured: {form}",
        );
        assert!(!form.contains("expires_in="), "{form}");
        assert!(!form.contains("account_id="), "{form}");
        task.abort();
    }

    /// A performer bound without a delegated grant refuses it by name rather than sending a form
    /// the vendor would have to reject.
    #[tokio::test]
    async fn a_performer_with_no_delegated_grant_sends_nothing() {
        let recorded = Arc::new(Mutex::new(Vec::new()));
        let (endpoint, task) = serve(
            Router::new()
                .route("/token", post(recording_endpoint))
                .with_state(Arc::clone(&recorded)),
        )
        .await;
        let performer =
            HttpCredentialAcquirer::new("second", &endpoint, TokenEndpointBehavior::Standard)
                .expect("standard fixture binding");
        let code = Secret::new("authorization-code");
        let verifier = Secret::new("the-code-verifier");

        let refusal = performer
            .redeem_authorization_code(AuthorizationCodeRedemption::new(&code, &verifier))
            .await
            .expect_err("an unbound delegated grant is refused");

        assert_eq!(refusal, AcquisitionRefusal::GrantNotPerformed);
        assert!(
            recorded.lock().expect("recorder lock").is_empty(),
            "a refusal this host decides must reach no vendor",
        );
        task.abort();
    }

    /// The grant refuses a shape that could not work, at composition, naming what was wrong.
    #[test]
    fn a_delegated_grant_refuses_an_unusable_shape() {
        let loopback = "http://127.0.0.1:9/oauth/authorize";
        for (why, result) in [
            (
                "no scopes",
                DelegatedGrant::new(loopback, "c", None, [], redirect()),
            ),
            (
                "a scope carrying the separator",
                DelegatedGrant::new(
                    loopback,
                    "c",
                    None,
                    ["read_api read_user".to_owned()],
                    redirect(),
                ),
            ),
            (
                "no client id",
                DelegatedGrant::new(loopback, "", None, ["s".to_owned()], redirect()),
            ),
            (
                "an endpoint that is not a URL",
                DelegatedGrant::new("not-a-url", "c", None, ["s".to_owned()], redirect()),
            ),
            // **The transport rule, now reachable from a test** (X-147 review, M4). It was behind
            // `cfg!(test)`, so the one place it is written down was the one place nothing could
            // drive it. Cleartext to a routable host puts `state` and the PKCE challenge on the
            // wire and brings an authorization code back the same way.
            (
                "a cleartext endpoint that is not loopback",
                DelegatedGrant::new(
                    "http://vendor.example.test/oauth/authorize",
                    "c",
                    None,
                    ["s".to_owned()],
                    redirect(),
                ),
            ),
        ] {
            assert!(result.is_err(), "admitted a delegated grant with {why}");
        }

        // And the two shapes that must still be admitted, or the rule above is one nobody can run
        // a checkout against.
        assert!(DelegatedGrant::new(
            "https://vendor.example.test/oauth/authorize",
            "c",
            None,
            ["s".to_owned()],
            redirect(),
        )
        .is_ok());
        assert!(
            DelegatedGrant::new(loopback, "c", None, ["s".to_owned()], redirect()).is_ok(),
            "a checkout's own loopback vendor fixture stays usable",
        );

        let grant = delegated(redirect());
        assert_eq!(grant.scope(), "read_api read_user");
        assert!(
            !format!("{grant:?}").contains("client-secret-never-print-this"),
            "a grant must not print the client secret it holds",
        );
    }

    /// **B1's failing-first test.** A grant whose redirect is not the one this deployment configured
    /// does not become a registry, so it can never reach a vendor.
    ///
    /// Both strings are canonical and both would pass every check
    /// `crate::acquisition_redirect` makes on their own — which is the point. What is wrong with the
    /// second is not its shape but that it is *not this deployment's*, and before this the only
    /// thing standing between it and the vendor was that a fixture happened to spell both the same.
    #[test]
    fn a_grant_whose_redirect_is_not_this_deployments_is_refused_at_composition() {
        let elsewhere =
            AcquisitionRedirect::parse("https://other.example.test/api/acquisitions/callback")
                .expect("a canonical redirect somewhere else");
        assert_ne!(elsewhere, redirect());

        let binding = || {
            AcquisitionBinding::new(
                "second",
                "second.access_token",
                None,
                Arc::new(
                    HttpCredentialAcquirer::new(
                        "second",
                        "https://vendor.example.test/token",
                        TokenEndpointBehavior::Standard,
                    )
                    .expect("a standard performer"),
                ),
            )
        };

        assert_eq!(
            AcquisitionBindings::new(
                [binding().with_delegated_grant(delegated(elsewhere))],
                Some(&redirect()),
            )
            .expect_err("a divergent redirect must not become a registry"),
            "a delegated acquisition grant names a redirect URI that is not the one this \
             deployment configured",
        );

        assert_eq!(
            AcquisitionBindings::new(
                [binding().with_delegated_grant(delegated(redirect()))],
                None
            )
            .expect_err("a grant with no deployment redirect must not become a registry"),
            "a delegated acquisition grant is bound and this deployment configured no acquisition \
             redirect URI",
        );

        // The agreeing case, or the two refusals above would pass for a registry that refuses
        // everything.
        assert!(AcquisitionBindings::new(
            [binding().with_delegated_grant(delegated(redirect()))],
            Some(&redirect()),
        )
        .is_ok());
    }

    /// **The other half of B1**: one grant reaches the browser and the token endpoint, because
    /// `delegating` sets both halves from one `Arc` and there is no second argument.
    #[test]
    fn one_arc_reaches_the_binding_and_the_performer_it_binds() {
        let performer = HttpCredentialAcquirer::new(
            "second",
            "https://vendor.example.test/token",
            TokenEndpointBehavior::Standard,
        )
        .expect("a standard performer");
        let binding = AcquisitionBinding::delegating(
            "second",
            "second.access_token",
            None,
            delegated(redirect()),
            performer,
        );

        let bound = binding.delegated().expect("the binding holds the grant");
        assert_eq!(bound.redirect(), &redirect());
        assert!(
            AcquisitionBindings::new([binding], Some(&redirect())).is_ok(),
            "and it agrees with the deployment it was composed against",
        );
    }

    // ---------------------------------------------------------------------------------------
    // X-154 — composing acquisitions from the released catalogue declaration.
    // ---------------------------------------------------------------------------------------

    fn provider(id: &str) -> &'static Provider {
        connector_catalog::provider(ProviderKey::id(id)).expect("the catalogue declares it")
    }

    /// This deployment's registration identity, which never comes from the catalogue.
    fn registration() -> OAuthRegistration {
        OAuthRegistration::new("deployment-client", None)
    }

    /// **The grant refusal and the incomplete-field refusal, both against babelforce's released
    /// declaration.**
    ///
    /// babelforce declares `[password, refresh_token]`, an empty `endpoint` and an empty
    /// `authorize_path`. Two independent things are wrong with composing a delegated acquisition
    /// from it, and each is refused on its own terms:
    ///
    /// 1. `password` is a grant this composition does not perform, and the refusal **names it**
    ///    rather than silently downgrading to `refresh_token` — which would be renewing a
    ///    credential nothing ever obtained.
    /// 2. Shaped for the grant this host *does* perform, the same declaration is still too
    ///    incomplete to compose from: there is no authorize path to send anybody to.
    ///
    /// The second half swaps only the grant list, so every field it refuses on is the released one.
    #[test]
    fn babelforce_is_refused_at_composition_by_its_grant_and_by_its_empty_authorize_path() {
        let babelforce = provider("babelforce");
        let (credential, spec) =
            declared_oauth2(babelforce).expect("babelforce declares one OAuth2 credential");

        // The released facts this test is written against, asserted rather than assumed.
        assert_eq!(credential.name, "babelforce.access_token");
        assert_eq!(
            spec.grants,
            &[OAuthGrant::Password, OAuthGrant::RefreshToken]
        );
        assert_eq!(spec.endpoint, "");
        assert_eq!(spec.authorize_path, "");

        // An empty endpoint *is* resolvable — it means the connector's own base URL — so the
        // refusals below are about the grant and the path, not about a base URL nothing could find.
        let base = endpoint_base(babelforce.id, babelforce.base_url, spec)
            .expect("an empty endpoint resolves to the connector's own base URL");
        assert_eq!(base, "https://services.babelforce.com");

        let refusal =
            grant_from_declaration(babelforce.id, spec, &base, &registration(), &redirect())
                .expect_err("a grant this composition does not perform must be refused");
        assert_eq!(
            refusal,
            CompositionRefusal::GrantNotPerformed {
                connector: "babelforce".to_owned(),
                grant: "password",
            },
        );
        let message = refusal.to_string();
        assert!(message.contains("babelforce"), "{message}");
        assert!(message.contains("password"), "{message}");
        assert!(
            !message.contains("refresh_token"),
            "the refusal must not read as an invitation to use the other grant in the list: \
             {message}",
        );

        // The same declaration, shaped for the grant this host performs: still refused, now naming
        // the field its own declaration leaves empty.
        let shaped = OAuth2 {
            grants: &[OAuthGrant::AuthorizationCode, OAuthGrant::RefreshToken],
            ..*spec
        };
        let refusal =
            grant_from_declaration(babelforce.id, &shaped, &base, &registration(), &redirect())
                .expect_err("an empty authorize path cannot compose an authorization");
        assert_eq!(
            refusal,
            CompositionRefusal::IncompleteDeclaration {
                connector: "babelforce".to_owned(),
                field: "authorize_path",
            },
        );
        let message = refusal.to_string();
        assert!(message.contains("babelforce"), "{message}");
        assert!(message.contains("authorize_path"), "{message}");
    }

    /// **A declared grant is never downgraded to another in the same list.**
    ///
    /// The pairing that could silently work is `[password, refresh_token]` — this host has a
    /// performer for the second — so it is the one worth pinning. `[client_credentials]` is the
    /// other direction: a grant this host performs nowhere.
    #[test]
    fn a_grant_this_host_does_not_perform_is_named_rather_than_skipped() {
        assert_eq!(
            performable("fixture", &[OAuthGrant::Password, OAuthGrant::RefreshToken],)
                .expect_err("password is not performed from a declaration"),
            CompositionRefusal::GrantNotPerformed {
                connector: "fixture".to_owned(),
                grant: "password",
            },
        );
        assert_eq!(
            performable("fixture", &[OAuthGrant::ClientCredentials])
                .expect_err("this host performs no client-credentials grant"),
            CompositionRefusal::GrantNotPerformed {
                connector: "fixture".to_owned(),
                grant: "client_credentials",
            },
        );
        assert_eq!(
            performable("fixture", &[OAuthGrant::RefreshToken])
                .expect_err("renewal alone obtains nothing"),
            CompositionRefusal::NoAcquiringGrant {
                connector: "fixture".to_owned(),
            },
        );
        // And the pair every delegated connector needs, which must still compose or the refusals
        // above would pass for a function that refuses everything.
        performable(
            "fixture",
            &[OAuthGrant::AuthorizationCode, OAuthGrant::RefreshToken],
        )
        .expect("the grants this composition performs");
    }

    /// **The generated tables cannot resolve a *named* endpoint, and this refuses rather than
    /// guessing one** (X-154 finding).
    ///
    /// GitLab's declaration resolves against the `login` service. `catalog::Provider` carries one
    /// `base_url` and it is the **default** service's — `{origin}/api/v4`, the API, not the
    /// authorization host — so there is nothing in the tables to join `/oauth/authorize` onto. The
    /// canonical document carries `services[].base_url` (`{origin}` for `login`); reading it is
    /// X-153.
    ///
    /// A guess here would be an authorization request sent to somebody else's host, so this is a
    /// refusal that names the connector and the endpoint.
    #[test]
    fn a_named_endpoint_has_no_base_url_in_the_generated_tables() {
        let gitlab = provider("gitlab");
        let (credential, spec) = declared_oauth2(gitlab).expect("gitlab declares one");

        assert_eq!(credential.name, "gitlab.oauth_token");
        assert_eq!(spec.endpoint, "login");
        assert_eq!(gitlab.base_url, "{origin}/api/v4");

        let refusal = endpoint_base(gitlab.id, gitlab.base_url, spec)
            .expect_err("a named endpoint is not resolvable from the generated tables");
        assert_eq!(
            refusal,
            CompositionRefusal::UnresolvableEndpoint {
                connector: "gitlab".to_owned(),
                endpoint: "login".to_owned(),
            },
        );
        let message = refusal.to_string();
        assert!(message.contains("gitlab"), "{message}");
        assert!(message.contains("login"), "{message}");

        // The other half of the same gap: even the connector's *own* base URL is templated, and a
        // startup composition has no tenant whose settings could fill `{origin}`.
        let unnamed = OAuth2 {
            endpoint: "",
            ..*spec
        };
        assert_eq!(
            endpoint_base(gitlab.id, gitlab.base_url, &unnamed)
                .expect_err("a templated base URL is not a base URL"),
            CompositionRefusal::TemplatedBaseUrl {
                connector: "gitlab".to_owned(),
            },
        );
    }

    /// **The registration identity is this deployment's, and a missing one refuses at startup
    /// naming the connector.**
    #[test]
    fn a_registered_connector_with_no_client_id_refuses_and_names_it() {
        let refusal = from_environment(
            |name| match name {
                ACQUISITION_CONNECTORS_ENV => Some("gitlab".to_owned()),
                _ => None,
            },
            Some(&redirect()),
        )
        .expect_err("a registered connector with no client id must refuse");

        assert_eq!(
            refusal,
            CompositionRefusal::NoRegistration {
                connector: "gitlab".to_owned(),
                setting: "FLUX_EXCHANGE_OAUTH_CLIENT_ID_GITLAB".to_owned(),
            },
        );
        let message = refusal.to_string();
        assert!(message.contains("gitlab"), "{message}");
        assert!(
            message.contains("FLUX_EXCHANGE_OAUTH_CLIENT_ID_GITLAB"),
            "{message}",
        );

        // An empty value is an operator who has not chosen one, not one who chose `""` — the rule
        // `crate::acquisition_redirect` and `crate::hosted_origin` already state.
        assert!(matches!(
            from_environment(
                |name| match name {
                    ACQUISITION_CONNECTORS_ENV => Some("gitlab".to_owned()),
                    "FLUX_EXCHANGE_OAUTH_CLIENT_ID_GITLAB" => Some("   ".to_owned()),
                    _ => None,
                },
                Some(&redirect()),
            ),
            Err(CompositionRefusal::NoRegistration { .. }),
        ));

        // A connector this build does not catalogue is refused by name rather than skipped, for
        // `DevIdentity`'s reason: a list that silently lost an entry is a list whose operator is
        // debugging the wrong thing.
        assert_eq!(
            from_environment(
                |name| (name == ACQUISITION_CONNECTORS_ENV).then(|| "not-a-connector".to_owned()),
                Some(&redirect()),
            )
            .expect_err("an uncatalogued connector must refuse"),
            CompositionRefusal::UnknownConnector {
                connector: "not-a-connector".to_owned(),
            },
        );

        // And a deployment that registered nothing composes an empty registry, which is what a
        // checkout runs as and is not an error.
        let empty = from_environment(|_| None, Some(&redirect()))
            .expect("registering nothing is not a refusal");
        assert!(empty.get("gitlab").is_none());
        assert_eq!(empty.redirect(), Some(&redirect()));
    }

    /// **A catalogue-supplied `client_id` is ignored rather than trusted.**
    ///
    /// Decision 0022: the artifact publishes the registration *requirement*, never a value.
    /// Upstream C-536 refuses to emit one, so every released declaration carries `""` — which means
    /// a test written against the released tables could not tell "ignored" from "empty anyway".
    /// This one supplies a non-empty catalogue value, which is the case that has to keep being
    /// refused when a future document carries one.
    #[test]
    fn a_catalogue_supplied_client_id_is_never_read() {
        const CATALOGUE_SUPPLIED: &str = "CATALOGUE-SUPPLIED-CLIENT-ID-NOT-TO-BE-TRUSTED";
        let spec = OAuth2 {
            endpoint: "",
            authorize_path: "/oauth/authorize",
            token_path: "/oauth/token",
            client_id: CATALOGUE_SUPPLIED,
            scopes: &["read_api"],
            grants: &[OAuthGrant::AuthorizationCode, OAuthGrant::RefreshToken],
            redirect: None,
        };

        let grant = grant_from_declaration(
            "fixture",
            &spec,
            "https://vendor.example.test",
            &registration(),
            &redirect(),
        )
        .expect("the declaration composes");

        assert_eq!(
            grant.client_id(),
            "deployment-client",
            "the client id is the deployment's",
        );
        assert!(
            !format!("{grant:?}").contains(CATALOGUE_SUPPLIED),
            "a catalogue-supplied client id must not reach the composed grant: {grant:?}",
        );
        assert_eq!(
            grant.authorization_endpoint(),
            "https://vendor.example.test/oauth/authorize",
            "and the endpoint and path are the declaration's",
        );
        assert_eq!(grant.scope(), "read_api");
    }

    /// **X-74's gate, driven by released metadata rather than by a fixture** (X-154).
    ///
    /// babelforce is the first released connector to declare a hazard. The binding's hazard is read
    /// from that declaration, so a deployment on the safe default refuses its acquisition by name,
    /// and one that opted in by name admits it — which is the difference the posture exists to
    /// express, now decided from a vendor fact this repository did not write.
    #[test]
    fn the_hazard_a_binding_carries_is_read_from_the_released_declaration() {
        let (babelforce, _) =
            declared_oauth2(provider("babelforce")).expect("babelforce declares one");
        let (gitlab, _) = declared_oauth2(provider("gitlab")).expect("gitlab declares one");

        assert_eq!(
            babelforce.hazard,
            Some(DeclaredHazard::ResourceOwnerSecretShared),
            "the released declaration, not a fixture",
        );
        assert_eq!(gitlab.hazard, None);

        let hazard = declared_hazard(babelforce.hazard);
        assert_eq!(hazard, Some(AuthHazard::ResourceOwnerSecretShared));
        assert_eq!(declared_hazard(gitlab.hazard), None);

        let binding = AcquisitionBinding::new(
            "babelforce",
            babelforce.name,
            hazard,
            Arc::new(
                HttpCredentialAcquirer::new(
                    "babelforce",
                    "https://services.babelforce.com/oauth/token",
                    TokenEndpointBehavior::Standard,
                )
                .expect("a performer"),
            ),
        );

        let refusal = binding
            .admit(&AuthPosture::fail_closed())
            .expect_err("the safe default refuses a declared hazard");
        let message = refusal.to_string();
        assert!(message.contains("babelforce"), "{message}");
        assert!(
            message.contains("resource_owner_secret_shared"),
            "{message}"
        );

        binding
            .admit(&AuthPosture::allowing([
                AuthHazard::ResourceOwnerSecretShared,
            ]))
            .expect("a deployment that opted in by name admits it");
    }

    /// **Nothing a composition holds prints its client secret**, and the client id does.
    ///
    /// RFC 6749 §2.2 makes the client id public, so it is what a refusal names and what an operator
    /// matches against the application they registered. The secret is the other half and appears
    /// nowhere — not in a grant, not in a binding, not in the registry that holds them, and not in
    /// the refusal a bad shape produces.
    #[test]
    fn a_composition_prints_its_client_id_and_never_its_client_secret() {
        const CLIENT_SECRET: &str = "CLIENT-SECRET-NOT-A-REAL-SECRET";
        let registration =
            OAuthRegistration::new("deployment-client", Some(Secret::new(CLIENT_SECRET)));
        let spec = OAuth2 {
            endpoint: "",
            authorize_path: "/oauth/authorize",
            token_path: "/oauth/token",
            client_id: "",
            scopes: &["read_api"],
            grants: &[OAuthGrant::AuthorizationCode, OAuthGrant::RefreshToken],
            redirect: None,
        };
        let binding = binding_from_declaration(
            "fixture",
            "fixture.oauth_token",
            None,
            &spec,
            "https://vendor.example.test",
            &registration,
            &redirect(),
        )
        .expect("the declaration composes");
        let registry =
            AcquisitionBindings::new([binding.clone()], Some(&redirect())).expect("one binding");
        // A shape that cannot compose, so the refusal rendering is covered too.
        let refusal = grant_from_declaration(
            "fixture",
            &OAuth2 {
                scopes: &[],
                ..spec
            },
            "https://vendor.example.test",
            &registration,
            &redirect(),
        )
        .expect_err("no scopes cannot compose");

        for rendering in [
            format!("{registration:?}"),
            format!("{binding:?}"),
            format!("{registry:?}"),
            format!("{refusal:?}"),
            format!("{refusal}"),
        ] {
            assert!(
                !rendering.contains(CLIENT_SECRET),
                "a client secret reached a rendering: {rendering}",
            );
        }
        assert!(
            format!("{registration:?}").contains("deployment-client"),
            "the client id is public by specification and is what an operator matches on",
        );
    }

    #[test]
    fn only_structured_vendor_error_fields_can_classify_mfa() {
        let echoed = r#"{"error":"invalid_grant","echo":"password-happens-to-contain-mfa"}"#;
        assert_eq!(
            classify_rejection(401, echoed),
            AcquisitionRefusal::CredentialsRejected,
        );
        let mfa =
            r#"{"error":"interaction_required","error_description":"MFA challenge required"}"#;
        assert_eq!(
            classify_rejection(400, mfa),
            AcquisitionRefusal::MfaRequired
        );
    }
}
