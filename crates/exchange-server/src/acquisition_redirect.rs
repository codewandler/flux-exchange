//! Where a vendor sends a person's browser back after they authorize a connection.
//!
//! # Deployment configuration, not connector data (X-147)
//!
//! A connector declares what a vendor *is*; where this particular deployment is reachable is not
//! something any connector can know. Upstream's `OAuthRedirect` models a loopback port and a path,
//! which is the shape a desktop tool registers — a hosted Exchange has an origin an operator chose
//! and registered at the vendor, and that origin is exactly the sort of thing
//! `docs/vision.md` says a caller may never name.
//!
//! # Why this is not [`REDIRECT_URI_ENV`](crate::oidc::config::REDIRECT_URI_ENV)
//!
//! That one points at `/api/signin/callback` and is registered with the **identity provider**. The
//! two flows come back to different routes, are registered with different parties, and mean
//! different things: one ends in a session that says who the human is, the other in a vendor
//! credential that acts as them. Reusing the sign-in variable would mean a deployment that signs
//! people in through Google and connects them to GitLab has one string that has to be two.
//!
//! # Compared exactly
//!
//! The configured value goes into the authorization URL and into the token request **verbatim**,
//! and it is never re-derived from a request's `Host` header — which a caller controls, and which is
//! how a redirect URI becomes an open redirect. To make "verbatim" checkable rather than intended,
//! [`configured`] refuses any spelling that is not already canonical: if a URL parser would
//! normalise the operator's string at all, this host refuses at startup and says so, instead of
//! sending one spelling to the authorization endpoint and a different one to the token endpoint —
//! which the vendor answers with `invalid_grant`, at the last step, for a reason nothing reports.

use std::net::IpAddr;

use reqwest::Url;

/// The variable naming this deployment's acquisition redirect URI.
pub const SETTING: &str = "FLUX_EXCHANGE_ACQUISITION_REDIRECT_URI";

/// The route the configured URI has to point at.
///
/// Spelled here rather than in the route table, so the startup check and the route it guards read
/// one value: a redirect URI pointing at a path this host does not serve is a connection that dies
/// at the last step with the vendor reporting success.
pub const CALLBACK_PATH: &str = "/api/acquisitions/callback";

/// **This deployment's redirect URI, as a value that cannot exist without having been checked.**
///
/// A newtype and not a `String`, because X-147's first review found the reason: the checking below
/// guarded a value that never left the process, while the string that *did* reach a vendor came
/// from a composition argument and was checked only for being non-empty. Two strings for one fact
/// is how "compared exactly" becomes a sentence in a doc comment. There is exactly one constructor,
/// it runs [`canonical`], and every redirect anywhere in this composition is one of these.
///
/// Equality is byte equality, deliberately: two spellings a vendor would treat as one URL are still
/// two strings, and RFC 6749 §4.1.3 has the token request re-present the *same* value the
/// authorization request carried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AcquisitionRedirect(String);

impl AcquisitionRedirect {
    /// Check one redirect URI and keep it.
    ///
    /// # Errors
    ///
    /// A value-free reason naming the setting and the shape. It never quotes the operator's value
    /// back: the refusal is about a shape, and a startup log is a place a mistyped URL would be
    /// permanent.
    pub fn parse(value: &str) -> Result<Self, String> {
        canonical(value).map(Self)
    }

    /// The URI as it goes into the authorization request and back out at the token endpoint.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for AcquisitionRedirect {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// This deployment's acquisition redirect URI, or `None` if it configured none.
///
/// Unset is not an error and does not refuse startup: a deployment that never connects a delegated
/// connector needs no redirect, and every route that does need one refuses by name when it is
/// missing. Explicitly empty **is** an error, following
/// [`hosted_origin`](crate::hosted_origin): `FLUX_EXCHANGE_ACQUISITION_REDIRECT_URI=` in an
/// environment file is an operator who has not chosen a value, not one who chose `""`.
///
/// # Errors
///
/// A value-free reason naming the setting. It never quotes the operator's value back: the refusal
/// is about a shape, and a startup log is a place a mistyped URL with a credential in its query
/// would be permanent.
pub fn configured() -> Result<Option<AcquisitionRedirect>, String> {
    match std::env::var(SETTING) {
        Ok(value) if value.is_empty() => Err(format!("{SETTING} is explicitly empty")),
        Ok(value) => AcquisitionRedirect::parse(&value).map(Some),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{SETTING} is not Unicode")),
        Err(std::env::VarError::NotPresent) => Ok(None),
    }
}

/// Admit one exactly-canonical redirect URI pointing at [`CALLBACK_PATH`].
fn canonical(value: &str) -> Result<String, String> {
    if !value.is_ascii() || value.trim() != value {
        return Err(refusal());
    }
    let parsed = Url::parse(value).map_err(|_| refusal())?;

    // Anything a parser would rewrite is refused rather than rewritten. This is the whole of
    // "compared exactly": what goes to the vendor is these bytes, and a value that is not already
    // what the parser produces would go out as two different strings from two different call sites.
    if parsed.as_str() != value {
        return Err(refusal());
    }
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(refusal());
    }
    if !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(refusal());
    }
    let Some(host) = parsed.host_str() else {
        return Err(refusal());
    };
    // `http` only where the browser is not on a network. The authorization code comes back in this
    // URL, and a code in cleartext is a code anyone on the path can spend — the same argument
    // `crate::oidc::config`'s transport check makes for the browser-facing sign-in variables, and
    // the same exception for the loopback bind a checkout runs on.
    if parsed.scheme() == "http" && !is_loopback_literal(host) {
        return Err(refusal());
    }
    if parsed.path() != CALLBACK_PATH {
        return Err(format!(
            "{SETTING} must end at the path this host serves the acquisition callback on, \
             {CALLBACK_PATH}"
        ));
    }
    Ok(value.to_owned())
}

/// Whether a host is a loopback IP **literal**.
///
/// A literal and not a name: `localhost` resolves through whatever the machine's resolver says, and
/// a name that resolves to loopback today is not a transport guarantee. `crate::hosted_origin`
/// draws the same line.
///
/// `pub` since X-147's review: `DelegatedGrant::new` needs the identical rule for the authorization
/// endpoint, and the alternative it replaced was a `cfg!(test)` escape that made the rule untestable
/// in the crate that states it.
pub fn is_loopback_literal(host: &str) -> bool {
    let unwrapped = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    unwrapped
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn refusal() -> String {
    format!(
        "{SETTING} must be one canonical ASCII https URL — or http on a loopback literal — with no \
         userinfo, query or fragment, ending at {CALLBACK_PATH}"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_an_exactly_canonical_callback_url_is_admitted() {
        for admitted in [
            "https://exchange.example.com/api/acquisitions/callback",
            "https://exchange.example.com:8443/api/acquisitions/callback",
            "http://127.0.0.1:3000/api/acquisitions/callback",
            "http://[::1]:3000/api/acquisitions/callback",
        ] {
            assert_eq!(canonical(admitted).as_deref(), Ok(admitted));
        }

        for refused in [
            // Cleartext off the loopback: the authorization code travels in this URL.
            "http://exchange.example.com/api/acquisitions/callback",
            // A name that happens to resolve to loopback is not a transport guarantee.
            "http://localhost:3000/api/acquisitions/callback",
            // The sign-in redirect, which is the mistake this variable exists to prevent.
            "https://exchange.example.com/api/signin/callback",
            // A path this host does not serve.
            "https://exchange.example.com/",
            "https://exchange.example.com/api/acquisitions/callback/",
            // Query and fragment: the vendor appends `code` and `state`, and a configured query
            // would make the two spellings differ by whichever the vendor chose to merge.
            "https://exchange.example.com/api/acquisitions/callback?tenant=acme",
            "https://exchange.example.com/api/acquisitions/callback#done",
            "https://user@exchange.example.com/api/acquisitions/callback",
            // Not a URL, or not a scheme a browser navigates.
            "javascript:alert(1)",
            "/api/acquisitions/callback",
            "",
        ] {
            assert!(canonical(refused).is_err(), "admitted {refused}");
        }
    }

    /// **The property "compared exactly" rests on.** Every spelling a parser would rewrite is
    /// refused at startup, so the string sent to the authorization endpoint and the string sent to
    /// the token endpoint cannot be two different strings.
    #[test]
    fn a_spelling_a_parser_would_rewrite_is_refused_rather_than_rewritten() {
        for rewritten in [
            "HTTPS://exchange.example.com/api/acquisitions/callback",
            "https://EXCHANGE.example.com/api/acquisitions/callback",
            "https://exchange.example.com:443/api/acquisitions/callback",
            "https://exchange.example.com/api/./acquisitions/callback",
            "https://exchange.example.com/api/other/../acquisitions/callback",
            " https://exchange.example.com/api/acquisitions/callback",
        ] {
            let refusal = canonical(rewritten)
                .err()
                .unwrap_or_else(|| panic!("admitted {rewritten}"));
            assert!(refusal.contains(SETTING), "{refusal}");
            assert!(
                !refusal.contains(rewritten),
                "a refusal names the setting, never the operator's value: {refusal}",
            );
        }
    }

    /// The two redirect settings are two settings, and this is the assertion that says so where a
    /// future edit would meet it.
    #[test]
    fn the_acquisition_redirect_is_not_the_sign_in_redirect() {
        assert_ne!(SETTING, crate::oidc::config::REDIRECT_URI_ENV);
        assert!(SETTING.starts_with("FLUX_EXCHANGE_"));
    }
}
