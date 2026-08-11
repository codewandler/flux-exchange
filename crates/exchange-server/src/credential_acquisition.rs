//! Server-owned credential acquisition performers and their startup bindings.
//!
//! The host crate owns only the secret-in/secret-out port. HTTP, endpoint URLs and vendor form
//! quirks stay here in the composing binary. Until upstream C-440 ships a connector declaration,
//! production composes an empty [`AcquisitionBindings`]; tests inject an explicit binding and thus
//! cannot accidentally make the built-in catalogue claim a capability its released metadata does
//! not declare.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::acquisition_redirect::AcquisitionRedirect;
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
/// It is **not** connector metadata. Composing an authorize URL from the connector's own
/// `Acquisition::OAuth2` declaration is what waits on the 0.21 connector line; until then a
/// composition states these values explicitly, which is honest about where they came from.
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

/// The acquisition declarations this composition explicitly bound.
#[derive(Clone, Debug, Default)]
pub struct AcquisitionBindings {
    by_connector: BTreeMap<String, AcquisitionBinding>,
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
        Ok(Self { by_connector })
    }

    /// Look up only by the catalogue connector selected by the route.
    pub fn get(&self, connector: &str) -> Option<&AcquisitionBinding> {
        self.by_connector.get(connector)
    }
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
    pub fn new(
        connector: &str,
        endpoint: &str,
        behavior: TokenEndpointBehavior,
    ) -> Result<Self, &'static str> {
        if matches!(&behavior, TokenEndpointBehavior::Babelforce(_)) && connector != "babelforce" {
            return Err("babelforce token-endpoint quirks may only bind connector `babelforce`");
        }
        let endpoint = Url::parse(endpoint).map_err(|_| "credential endpoint URL is invalid")?;
        if endpoint.scheme() != "https" && !cfg!(test) {
            return Err("credential endpoint URL must use HTTPS");
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
