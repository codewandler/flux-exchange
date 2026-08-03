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

use exchange_host::{
    async_trait, AcquiredCredential, AcquisitionRefusal, AuthHazard, CredentialAcquirer,
    PasswordRedemption, RefreshRedemption, Secret,
};
use reqwest::redirect::Policy;
use reqwest::{Client, Url};
use serde::Deserialize;

/// One connector-declared acquisition, fixed at composition time.
#[derive(Clone)]
pub struct AcquisitionBinding {
    connector: String,
    credential: String,
    hazard: AuthHazard,
    performer: Arc<dyn CredentialAcquirer>,
}

impl AcquisitionBinding {
    /// Bind one connector and its acquired credential to a performer.
    pub fn new(
        connector: impl Into<String>,
        credential: impl Into<String>,
        hazard: AuthHazard,
        performer: Arc<dyn CredentialAcquirer>,
    ) -> Self {
        Self {
            connector: connector.into(),
            credential: credential.into(),
            hazard,
            performer,
        }
    }

    /// The connector catalogue key this binding belongs to.
    pub fn connector(&self) -> &str {
        &self.connector
    }

    /// The flat catalogue name of the access credential this performer mints.
    pub fn credential(&self) -> &str {
        &self.credential
    }

    /// The connector-declared acquisition hazard.
    pub const fn hazard(&self) -> AuthHazard {
        self.hazard
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
    /// Construct a registry, refusing duplicate connector bindings.
    pub fn new(
        bindings: impl IntoIterator<Item = AcquisitionBinding>,
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
}

impl HttpCredentialAcquirer {
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
        let result = AcquisitionBindings::new([AcquisitionBinding::new(
            "second",
            "second.access_token",
            AuthHazard::ResourceOwnerSecretShared,
            Arc::new(performer),
        )]);
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
