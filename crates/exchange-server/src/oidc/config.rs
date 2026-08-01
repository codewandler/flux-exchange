//! What an operator must supply before this host can federate sign-in, and the refusal when they
//! have not.
//!
//! # Missing configuration does not stop the process
//!
//! This is the one refusal in this binary that is **not** a [`StartupRefusal`](crate::bind::StartupRefusal),
//! and the difference is deliberate. A reachable bind with no identity provider is a hole, so the
//! process must not start. Unconfigured OIDC is an *absent feature*: `/health` still answers and the
//! catalogue still serves, and killing the process would take those down to punish an operator who
//! simply has not set up sign-in yet.
//!
//! So the refusal is loud and local. It names every unset variable at startup, in one message rather
//! than one per restart, and `/api/signin` explains itself instead of redirecting somewhere that
//! will fail later. "Refuse; never repair" is satisfied by refusing to *pretend sign-in works* — the
//! failure the story names is a login that looks fine and dies at the callback.
//!
//! # There is no discovery
//!
//! A real client reads `/.well-known/openid-configuration` and learns the endpoints. That is an HTTP
//! request, and this binary has no HTTP client (see [`super::exchange`]), so the authorization
//! endpoint is configured rather than discovered. The token endpoint is not configured here at all:
//! it belongs to whatever binds [`TokenExchange`](super::exchange::TokenExchange), which owns every
//! call that leaves this process.

use std::fmt;

use exchange_host::{Tenant, TenantError};

/// The issuer this host will accept id tokens from, e.g. `https://accounts.example.com`.
///
/// Checked against the `iss` claim on every sign-in. Without it, a token minted by *any* provider
/// the exchange happens to trust would be accepted here.
pub const ISSUER_ENV: &str = "FLUX_EXCHANGE_OIDC_ISSUER";

/// Where `/api/signin` sends the browser.
pub const AUTHORIZATION_ENDPOINT_ENV: &str = "FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT";

/// This host's client identifier at the provider. Checked against the `aud` claim.
pub const CLIENT_ID_ENV: &str = "FLUX_EXCHANGE_OIDC_CLIENT_ID";

/// This host's client secret. **The environment and nowhere else** — see [`ClientSecret`].
pub const CLIENT_SECRET_ENV: &str = "FLUX_EXCHANGE_OIDC_CLIENT_SECRET";

/// Where the provider sends the browser back, exactly as registered with the provider.
pub const REDIRECT_URI_ENV: &str = "FLUX_EXCHANGE_OIDC_REDIRECT_URI";

/// The tenant every principal this provider authenticates belongs to. See [`OidcConfig::tenant`].
pub const TENANT_ENV: &str = "FLUX_EXCHANGE_OIDC_TENANT";

/// Every variable this module reads, in the order a refusal lists them.
const REQUIRED: &[&str] = &[
    ISSUER_ENV,
    AUTHORIZATION_ENDPOINT_ENV,
    CLIENT_ID_ENV,
    CLIENT_SECRET_ENV,
    REDIRECT_URI_ENV,
    TENANT_ENV,
];

/// The scopes this host asks for, and the whole of what it asks for.
///
/// **Signing in is not connecting.** `openid` identifies the human, `email` and `profile` are what
/// a console needs to show who is signed in. Nothing here grants access to anything at the
/// provider, and no vendor scope belongs in this list — connecting a provider is a different flow
/// with a different consent screen, and a user who agreed to "sign in" has not agreed to that.
/// Widening this constant would silently turn one consent into the other.
pub const SCOPES: &str = "openid email profile";

/// This host's client secret at the provider.
///
/// # Where it comes from
///
/// The environment, through [`OidcConfig::from_env`], and nowhere else. There is no other
/// constructor outside this module's own tests: not from a request, not from a query parameter, not
/// from a file this process reads, not from a field in any other configuration. A secret with a
/// second source is a secret with a second place to leak from, and the two most common of those —
/// a checked-in file and a value echoed back through an error — are exactly what this shape removes.
///
/// # Why it does not print
///
/// `Debug` redacts and there is no `Display`, following [`SessionToken`](crate::session::SessionToken).
/// This type is a field of [`OidcConfig`], so the derived `Debug` of anything holding the config
/// would otherwise carry the secret into a log line the moment somebody added `?config` to a
/// `tracing` call. The value leaves only through [`ClientSecret::expose`], and every call site of
/// that is a deliberate disclosure.
#[derive(Clone, PartialEq, Eq)]
pub struct ClientSecret(String);

impl ClientSecret {
    /// The secret as it goes to the token endpoint.
    ///
    /// The single place the value leaves this type, and the only caller is a `TokenExchange` —
    /// which this binary does not bind, hence the `allow`. Keeping the disclosure to one named
    /// method is the point: `expose` is greppable, and a reviewer can enumerate every use of it.
    #[allow(dead_code)]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ClientSecret {
    /// Redacts. A client secret in a log line is this host's identity at the provider.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ClientSecret(redacted)")
    }
}

/// Everything this host needs to federate a sign-in.
#[derive(Debug, Clone)]
pub struct OidcConfig {
    issuer: String,
    authorization_endpoint: String,
    client_id: String,
    client_secret: ClientSecret,
    redirect_uri: String,
    tenant: Tenant,
}

impl OidcConfig {
    /// Read the configuration from the process environment.
    ///
    /// `Err` is not fatal — see the module documentation. It carries every unset variable so an
    /// operator fixes them in one pass rather than one restart at a time.
    pub fn from_env() -> Result<Self, ConfigRefusal> {
        Self::read(|name| std::env::var(name).ok())
    }

    /// The environment, injected.
    ///
    /// Private, with the tests below as its only other caller, so the claim on [`ClientSecret`]
    /// stays true of every build: outside this module there is no way to supply a secret. It exists
    /// so those tests do not mutate the process environment out from under their neighbours — the
    /// same reason `DevIdentity::from_roster` is separate from `DevIdentity::armed`.
    fn read(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigRefusal> {
        // A variable that is set but empty is unset. Naming one and leaving it blank is a mistake
        // with a silent success mode: the operator believes they configured a client secret, and
        // what they have is a host that authenticates as nobody.
        let read = |name: &str| lookup(name).filter(|value| !value.trim().is_empty());

        let supplied: Vec<Option<String>> = REQUIRED.iter().map(|name| read(name)).collect();

        if supplied.iter().any(Option::is_none) {
            let unset: Vec<&'static str> = REQUIRED
                .iter()
                .zip(&supplied)
                .filter(|(_, value)| value.is_none())
                .map(|(name, _)| *name)
                .collect();

            return Err(ConfigRefusal::Unset {
                // Whether *anything* was supplied is what separates "this operator has not set up
                // sign-in" from "this operator set it up wrong", and those deserve different
                // volumes at startup.
                partial: unset.len() != REQUIRED.len(),
                unset,
            });
        }

        let mut supplied = supplied.into_iter().map(|value| value.unwrap_or_default());
        let mut next = || supplied.next().unwrap_or_default();

        let issuer = next();
        let authorization_endpoint = next();
        let client_id = next();
        let client_secret = ClientSecret(next());
        let redirect_uri = next();
        let tenant = next();

        // `Tenant::new` is the authority on what a tenant may be spelled; do not re-validate here.
        // Refused at startup rather than at sign-in, so a tenant that could walk out of its own
        // credential prefix is impossible to hold rather than merely impossible to use.
        let tenant =
            Tenant::new(tenant).map_err(|source| ConfigRefusal::UnusableTenant { source })?;

        Ok(Self {
            issuer,
            authorization_endpoint,
            client_id,
            client_secret,
            redirect_uri,
            tenant,
        })
    }

    /// A configuration for tests, without a process environment.
    ///
    /// `#[cfg(test)]`, so the claim on [`ClientSecret`] stays literally true of the shipped binary:
    /// there, the environment is the only source a secret has.
    #[cfg(test)]
    pub fn for_test(issuer: &str, client_id: &str, tenant: &str) -> Self {
        Self::for_test_with_endpoint(issuer, client_id, tenant, &format!("{issuer}/authorize"))
    }

    /// As [`OidcConfig::for_test`], with the authorization endpoint spelled out.
    #[cfg(test)]
    pub fn for_test_with_endpoint(
        issuer: &str,
        client_id: &str,
        tenant: &str,
        authorization_endpoint: &str,
    ) -> Self {
        Self {
            issuer: issuer.to_string(),
            authorization_endpoint: authorization_endpoint.to_string(),
            client_id: client_id.to_string(),
            client_secret: ClientSecret("a-test-secret".to_string()),
            redirect_uri: "https://exchange.example.com/api/signin/callback".to_string(),
            tenant: Tenant::new(tenant).expect("a literal tenant"),
        }
    }

    /// The issuer an id token must claim.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Where the browser is sent to authenticate.
    pub fn authorization_endpoint(&self) -> &str {
        &self.authorization_endpoint
    }

    /// This host's client identifier, which an id token must be audienced to.
    pub fn client_id(&self) -> &str {
        &self.client_id
    }

    /// This host's client secret.
    pub fn client_secret(&self) -> &ClientSecret {
        &self.client_secret
    }

    /// Where the provider returns the browser.
    pub fn redirect_uri(&self) -> &str {
        &self.redirect_uri
    }

    /// The tenant every principal from this provider belongs to.
    ///
    /// **Fixed here, at startup, by the operator** — the same shape as the development roster, and
    /// for the same reason. `AGENTS.md` § Invariants: *the tenant comes from the resolved principal
    /// and from nothing a caller controls*. A tenant read from a claim would be one the provider
    /// controls, which is better than one the caller controls but still not this; and at a provider
    /// where users can edit their own profile, some claims are caller-controlled after all. One
    /// configured provider serving one tenant has no such question in it.
    ///
    /// The cost is that this composition federates one tenant. Serving several from one provider is
    /// a real design question — it decides how a claim is mapped and who is trusted to assert it —
    /// and it deserves its own story rather than a default chosen here.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }
}

/// Why this host will not federate sign-in.
///
/// Hand-written rather than derived: `thiserror` is the library's convention and this binary does
/// not carry the dependency, so this follows [`StartupRefusal`](crate::bind::StartupRefusal).
#[derive(Debug, PartialEq, Eq)]
pub enum ConfigRefusal {
    /// Some or all of the required variables are unset.
    Unset {
        /// Every variable that is unset or empty, in declaration order.
        unset: Vec<&'static str>,
        /// Whether anything at all was supplied. Partial configuration is a mistake; nothing at all
        /// is a deployment that has not enabled sign-in.
        partial: bool,
    },

    /// The configured tenant is not usable as an address segment.
    UnusableTenant {
        /// Why it was refused.
        source: TenantError,
    },
}

impl fmt::Display for ConfigRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // Names every unset variable, not the first one. An operator fixing these one restart
            // at a time is an operator we made do six restarts.
            Self::Unset { unset, partial } => {
                let unset = unset.join(", ");

                if *partial {
                    write!(
                        f,
                        "OIDC sign-in is partially configured and will not be offered: {unset} \
                         {} unset. Set {} to enable it, or unset all of them to turn sign-in off \
                         deliberately. /health and the catalogue are unaffected",
                        if unset.contains(", ") { "are" } else { "is" },
                        REQUIRED.join(", "),
                    )
                } else {
                    write!(
                        f,
                        "OIDC sign-in is not configured, so this host offers no way to sign in. \
                         Set {unset} to enable it. /health and the catalogue are unaffected",
                    )
                }
            }
            Self::UnusableTenant { source } => write!(
                f,
                "{TENANT_ENV} names an unusable tenant: {source}. OIDC sign-in will not be offered",
            ),
        }
    }
}

impl std::error::Error for ConfigRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unset { .. } => None,
            Self::UnusableTenant { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    /// A complete, well-formed environment.
    fn complete() -> HashMap<&'static str, String> {
        HashMap::from([
            (ISSUER_ENV, "https://accounts.example.com".to_string()),
            (
                AUTHORIZATION_ENDPOINT_ENV,
                "https://accounts.example.com/authorize".to_string(),
            ),
            (CLIENT_ID_ENV, "flux-exchange".to_string()),
            (CLIENT_SECRET_ENV, "s3cr3t-value".to_string()),
            (
                REDIRECT_URI_ENV,
                "https://exchange.example.com/api/signin/callback".to_string(),
            ),
            (TENANT_ENV, "acme".to_string()),
        ])
    }

    fn read(environment: &HashMap<&'static str, String>) -> Result<OidcConfig, ConfigRefusal> {
        OidcConfig::read(|name| environment.get(name).cloned())
    }

    #[test]
    fn a_complete_environment_configures_the_flow() {
        let config = read(&complete()).expect("a complete environment configures OIDC");

        assert_eq!(config.issuer(), "https://accounts.example.com");
        assert_eq!(config.client_id(), "flux-exchange");
        assert_eq!(config.tenant().as_str(), "acme");
        assert_eq!(config.client_secret().expose(), "s3cr3t-value");
    }

    /// The Acceptance's fourth item, at the source: the refusal names **every** unset variable, so
    /// an operator fixes them in one pass.
    #[test]
    fn the_refusal_names_every_unset_variable() {
        let mut environment = complete();
        environment.remove(CLIENT_SECRET_ENV);
        environment.remove(TENANT_ENV);

        let refusal = read(&environment).expect_err("an incomplete environment is refused");

        assert_eq!(
            refusal,
            ConfigRefusal::Unset {
                unset: vec![CLIENT_SECRET_ENV, TENANT_ENV],
                partial: true,
            },
        );

        let message = refusal.to_string();
        assert!(message.contains(CLIENT_SECRET_ENV), "{message}");
        assert!(message.contains(TENANT_ENV), "{message}");
        assert!(
            !message.contains(ISSUER_ENV) || message.contains("Set "),
            "a variable that is set must not be reported as unset: {message}",
        );
    }

    /// Nothing configured is a deployment that has not enabled sign-in; some of it configured is a
    /// mistake. They read differently because an operator does different things about them.
    #[test]
    fn nothing_configured_and_half_configured_are_distinguished() {
        let nothing = read(&HashMap::new()).expect_err("an empty environment is refused");
        assert!(matches!(
            nothing,
            ConfigRefusal::Unset { partial: false, .. }
        ));

        let mut half = complete();
        half.remove(TENANT_ENV);
        let half = read(&half).expect_err("a partial environment is refused");
        assert!(matches!(half, ConfigRefusal::Unset { partial: true, .. }));

        assert_ne!(nothing.to_string(), half.to_string());
        // The one that is a mistake says so; the one that is a choice does not accuse anybody.
        assert!(half.to_string().contains("partially configured"));
    }

    /// Set-but-empty is unset. Treating it as configured would arm a client that authenticates as
    /// nobody, and the operator would believe they had configured a secret.
    #[test]
    fn a_variable_that_is_set_but_empty_is_unset() {
        for blank in ["", "   ", "\t"] {
            let mut environment = complete();
            environment.insert(CLIENT_SECRET_ENV, blank.to_string());

            let refusal = read(&environment)
                .expect_err(&format!("a blank {CLIENT_SECRET_ENV} must be refused"));

            assert_eq!(
                refusal,
                ConfigRefusal::Unset {
                    unset: vec![CLIENT_SECRET_ENV],
                    partial: true,
                },
                "for {blank:?}",
            );
        }
    }

    /// The tenant goes through `Tenant::new` at startup, so a spelling that could walk out of its
    /// own credential prefix is impossible to hold rather than merely impossible to use.
    #[test]
    fn a_traversing_tenant_is_refused_at_startup() {
        for hostile in ["../../etc", "a/b", "a.b"] {
            let mut environment = complete();
            environment.insert(TENANT_ENV, hostile.to_string());

            let refusal =
                read(&environment).expect_err(&format!("`{hostile}` must be refused as a tenant"));

            assert!(
                matches!(refusal, ConfigRefusal::UnusableTenant { .. }),
                "`{hostile}` was refused as {refusal:?} rather than for its tenant",
            );
        }
    }

    /// The Acceptance's third item: the secret does not print itself.
    ///
    /// Asserted through the whole config rather than through the secret alone, because that is how
    /// it actually reaches a log — somebody adds the configuration to a `tracing` call, and the
    /// derived `Debug` walks into the field.
    #[test]
    fn the_client_secret_redacts_itself_even_inside_the_config() {
        let config = read(&complete()).expect("a complete environment configures OIDC");

        let printed = format!("{:?}", config.client_secret());
        assert_eq!(printed, "ClientSecret(redacted)");

        let whole = format!("{config:?}");
        assert!(
            !whole.contains("s3cr3t-value"),
            "the config must not carry its secret into a log line: {whole}",
        );
        assert!(
            whole.contains("ClientSecret(redacted)"),
            "and must say that it withheld one: {whole}",
        );
        assert!(
            whole.contains("flux-exchange"),
            "while still being useful about everything that is not a secret: {whole}",
        );
    }

    /// Sign-in is not connecting. This host asks to learn who the human is and nothing else — any
    /// vendor scope here would turn one consent screen into a different one without anybody
    /// deciding to.
    #[test]
    fn the_requested_scopes_identify_the_human_and_grant_nothing() {
        let scopes: Vec<&str> = SCOPES.split_whitespace().collect();

        assert_eq!(scopes, ["openid", "email", "profile"]);
        assert!(
            scopes.contains(&"openid"),
            "without `openid` this is not OIDC and there is no id token to bind a nonce to",
        );
    }
}
