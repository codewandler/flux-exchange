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
//! # There is still no discovery
//!
//! A real client reads `/.well-known/openid-configuration` and learns the endpoints. This host does
//! not, and now that it *has* an HTTP client that is a choice rather than a limitation: see
//! [`JWKS_URI_ENV`]. Every endpoint it talks to — authorization, token, key set — is named by the
//! operator, so which provider and which keys can mint a session here is legible from the
//! environment alone rather than from a document re-fetched at runtime.
//!
//! # Cleartext is refused, and that is a claim about the channel only
//!
//! Every configured URL must be `https`, or `http` on loopback for a local test provider. See
//! [`transport_checked`] for which variables and why each is on the list, and
//! [`on_a_channel_this_host_will_use`] for the rule and — the part worth reading — for what
//! satisfying it does **not** say. It does not say the operator named the right provider.

use std::fmt;

use exchange_host::{Tenant, TenantError};

/// The issuer this host will accept id tokens from, e.g. `https://accounts.example.com`.
///
/// Checked against the `iss` claim on every sign-in. Without it, a token minted by *any* provider
/// the exchange happens to trust would be accepted here.
pub const ISSUER_ENV: &str = "FLUX_EXCHANGE_OIDC_ISSUER";

/// Where `/api/signin` sends the browser.
///
/// **Must be `https`, or `http` on loopback.** See [`transport_checked`]: the URL this host builds
/// on top of it carries `state`, the `nonce` and the PKCE challenge.
pub const AUTHORIZATION_ENDPOINT_ENV: &str = "FLUX_EXCHANGE_OIDC_AUTHORIZATION_ENDPOINT";

/// Where the authorization code is redeemed, back-channel.
///
/// **Must be `https`, or `http` on loopback.** See [`transport_checked`]: this is the request that
/// carries [`CLIENT_SECRET_ENV`] as HTTP Basic credentials.
pub const TOKEN_ENDPOINT_ENV: &str = "FLUX_EXCHANGE_OIDC_TOKEN_ENDPOINT";

/// Where the provider publishes the keys that sign its id tokens.
///
/// # Why this is configured and not discovered
///
/// Following [`AUTHORIZATION_ENDPOINT_ENV`]'s precedent: every endpoint this host talks to is named
/// by the operator. Discovery — fetching `<issuer>/.well-known/openid-configuration` and believing
/// what it says — would mean the set of keys that can mint a session here is decided by a document
/// this host re-fetches at runtime, so an operator reading the configuration could not tell which
/// keys those are. One more variable buys a deployment whose trust is legible from its environment.
///
/// **Must be `https`, or `http` on loopback.** See [`transport_checked`]: this decides which keys
/// can mint a session here.
pub const JWKS_URI_ENV: &str = "FLUX_EXCHANGE_OIDC_JWKS_URI";

/// This host's client identifier at the provider. Checked against the `aud` claim.
pub const CLIENT_ID_ENV: &str = "FLUX_EXCHANGE_OIDC_CLIENT_ID";

/// This host's client secret. **The environment and nowhere else** — see [`ClientSecret`].
pub const CLIENT_SECRET_ENV: &str = "FLUX_EXCHANGE_OIDC_CLIENT_SECRET";

/// Where the provider sends the browser back, exactly as registered with the provider.
///
/// **Must be `https`, or `http` on loopback.** See [`transport_checked`]: this is the address the
/// authorization code comes back to.
pub const REDIRECT_URI_ENV: &str = "FLUX_EXCHANGE_OIDC_REDIRECT_URI";

/// The tenant every principal this provider authenticates belongs to. See [`OidcConfig::tenant`].
pub const TENANT_ENV: &str = "FLUX_EXCHANGE_OIDC_TENANT";

/// The signed hosted-domain claim required for admission, when this provider represents a Google
/// Workspace organization. The same value is sent as Google's `hd` account-selection hint, but
/// admission is decided only from the signature-verified id-token claim.
pub const HOSTED_DOMAIN_ENV: &str = "FLUX_EXCHANGE_OIDC_HOSTED_DOMAIN";

/// Every variable naming a URL this host will not accept in cleartext, in the order the read
/// visits them.
///
/// # Why this is a read and not a list
///
/// **X-27.** It *is* the read: [`recorded`] performs [`Supplied::read`] against an empty
/// environment and this reports which variables that read took through [`Reader::channel`]. So the
/// list quoted at an operator cannot name a variable this host does not check, or miss one it does.
///
/// It was a constant until X-27, sitting beside [`required`] and beside a sequence of positional
/// reads — three lists describing one set of variables, kept in step by convention. This module had
/// already shipped one drift out of that arrangement.
///
/// # Why the name changed
///
/// X-17 called this `BACK_CHANNEL` and it held two variables, on the argument that only the
/// requests this process makes *itself* are ones this process can insist on TLS for.
/// [`TOKEN_ENDPOINT_ENV`] is the one request that carries [`CLIENT_SECRET_ENV`], as HTTP Basic
/// credentials — in cleartext, over `http`, to anybody on the path, with no refusal and no symptom.
/// [`JWKS_URI_ENV`] carries no secret, but it decides **which keys can mint a session here**, and a
/// key set an attacker can rewrite in flight is a host that accepts tokens they signed.
///
/// X-23 added the two browser-facing variables, which makes `BACK_CHANNEL` a name that contradicts
/// its contents. What the list actually is, and now says, is *the URLs whose transport this host
/// checks* — the property they share is the check, not the direction of the request.
///
/// # Why the browser-facing two are in it after all
///
/// X-17 and X-19 left them out because a browser navigates to them, so the browser enforces their
/// transport, and a `redirect_uri` is in any case re-checked by the provider against a registration
/// this host does not own. Both halves of that are true and neither is the whole argument:
///
/// - The browser does not *refuse* an `http` URL; it uses it. And the authorization URL this host
///   builds on top of [`AUTHORIZATION_ENDPOINT_ENV`] carries `state`, the `nonce` and the PKCE
///   challenge, each of which is readable and modifiable in flight over `http` — which is the same
///   position X-15 closed from the other direction, by drawing those values from the OS and
///   spending them once.
/// - An operator who typed `http` did not decide anything. They made a mistake, at startup, in a
///   place this host is looking, and the previous behaviour was to say nothing about it.
///
/// # What is deliberately *not* here
///
/// [`ISSUER_ENV`] is a URL and is not on this list, because it is not a channel: nothing dials it —
/// this host does no discovery, see the module documentation — and it is compared to the `iss` claim
/// as a string. A scheme rule on it would be a rule about spelling, not about transport.
///
/// # What passing this does not promise
///
/// See [`on_a_channel_this_host_will_use`]. `https` short-circuits before the host is even
/// examined, so this vouches for the **channel** and never for who is on the other end of it.
fn transport_checked() -> Vec<&'static str> {
    recorded().checked
}

/// Every variable this module reads, in the order a refusal lists them.
///
/// **X-27.** Derived by performing the read itself — see [`recorded`] — rather than written out a
/// second time. Until then this was a constant that the positional reads in [`OidcConfig::read`]
/// had to match entry for entry: a variable added to one and not the other shifted every value
/// after it into the wrong field, and a variable read but never listed here was one an operator
/// could set and this host would silently ignore.
fn required() -> Vec<&'static str> {
    recorded().visited
}

/// One read of an empty environment, which is where both of the lists above come from.
///
/// Nothing is looked up, so this touches no environment and yields no value. What it yields is what
/// [`Supplied::read`] *did*: which variables it visited, and which of those it read as channels.
/// Both lists are therefore reports of the read rather than descriptions of it, and a description
/// is the thing that can be wrong.
fn recorded() -> Reader<impl Fn(&str) -> Option<String>> {
    let mut reader = Reader::of(|_: &str| None::<String>);

    // Every value comes back absent and every one of them is discarded; only the reader is wanted.
    drop(Supplied::read(&mut reader));

    reader
}

/// The scopes this host asks for, and the whole of what it asks for.
///
/// **Signing in is not connecting.** `openid` identifies the human. Nothing here grants access to anything at the
/// provider, and no vendor scope belongs in this list — connecting a provider is a different flow
/// with a different consent screen, and a user who agreed to "sign in" has not agreed to that.
/// Widening this constant would silently turn one consent into the other.
pub const SCOPES: &str = "openid";

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
    /// namely `HttpTokenExchange`, which sends it as HTTP Basic credentials. Keeping the disclosure
    /// to one named method is the point: `expose` is greppable, and a reviewer can enumerate every
    /// use of it — there is exactly one.
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
    token_endpoint: String,
    jwks_uri: String,
    client_id: String,
    client_secret: ClientSecret,
    redirect_uri: String,
    tenant: Tenant,
    hosted_domain: Option<String>,
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
    ///
    /// **Every value is read by name and bound by name.** [`Supplied::read`] is the one place a
    /// variable is paired with the field it lands in, and [`Reader`] is why the lists this
    /// function's refusals quote are produced by that read rather than written down beside it.
    fn read(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, ConfigRefusal> {
        let mut reader = Reader::of(lookup);

        // Destructured rather than kept whole, so every field below travels under its own name and
        // reaches the constructor by field-init shorthand. A field added to `Supplied` and not
        // mentioned in this pattern does not compile; see `Supplied::read` for the other half.
        let Supplied {
            issuer,
            authorization_endpoint,
            token_endpoint,
            jwks_uri,
            client_id,
            client_secret,
            redirect_uri,
            tenant,
            hosted_domain,
        } = Supplied::read(&mut reader);

        if !reader.unset.is_empty() {
            return Err(ConfigRefusal::Unset {
                // Whether *anything* was supplied is what separates "this operator has not set up
                // sign-in" from "this operator set it up wrong", and those deserve different
                // volumes at startup. Counted against what the read visited rather than against a
                // constant, so the two cannot disagree about how many variables there are.
                partial: reader.unset.len() != reader.visited.len(),
                unset: reader.unset,
            });
        }

        // `Tenant::new` is the authority on what a tenant may be spelled; do not re-validate here.
        // Refused at startup rather than at sign-in, so a tenant that could walk out of its own
        // credential prefix is impossible to hold rather than merely impossible to use.
        let tenant =
            Tenant::new(tenant).map_err(|source| ConfigRefusal::UnusableTenant { source })?;

        // Every transport-checked URL, refused here rather than at somebody's first sign-in. A host
        // that starts and then sends its client secret in cleartext has already sent it by the time
        // anybody could notice; a host that refuses to offer sign-in has sent nothing. The same
        // moment serves the browser-facing two for a different reason: the operator is present at
        // startup, and a mistyped scheme is something they can still fix cheaply.
        //
        // Named in `transport_checked`'s order and all at once, following `Unset`: an operator who
        // got one of these wrong very likely got all of them wrong the same way, and fixing them one
        // restart at a time is a thing we would be doing to them.
        //
        // The rule was applied at the read — `Reader::channel` — rather than against a list of
        // pairs written out here, which is what made the list and the read able to disagree.
        if !reader.insecure.is_empty() {
            return Err(ConfigRefusal::InsecureEndpoint {
                insecure: reader.insecure,
            });
        }

        Ok(Self {
            issuer,
            authorization_endpoint,
            token_endpoint,
            jwks_uri,
            client_id,
            client_secret,
            redirect_uri,
            tenant,
            hosted_domain,
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
            token_endpoint: format!("{issuer}/token"),
            jwks_uri: format!("{issuer}/jwks"),
            client_id: client_id.to_string(),
            client_secret: ClientSecret("a-test-secret".to_string()),
            redirect_uri: "https://exchange.example.com/api/signin/callback".to_string(),
            tenant: Tenant::new(tenant).expect("a literal tenant"),
            hosted_domain: None,
        }
    }

    /// Require a signed hosted-domain claim in tests.
    #[cfg(test)]
    pub fn with_hosted_domain_for_test(mut self, hosted_domain: &str) -> Self {
        self.hosted_domain = Some(hosted_domain.to_string());
        self
    }

    /// As [`OidcConfig::for_test`], with the back-channel endpoints pointed at a local stub.
    ///
    /// Its own constructor because the exchange tests need a token endpoint and a JWKS URI on a
    /// loopback port that only exists once the stub provider is listening.
    #[cfg(test)]
    pub fn for_test_against(issuer: &str, client_id: &str, tenant: &str, base: &str) -> Self {
        Self {
            token_endpoint: format!("{base}/token"),
            jwks_uri: format!("{base}/jwks"),
            ..Self::for_test(issuer, client_id, tenant)
        }
    }

    /// The issuer an id token must claim.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Where the authorization code is redeemed. Back-channel: the browser never sees this.
    pub fn token_endpoint(&self) -> &str {
        &self.token_endpoint
    }

    /// Where the keys that sign this provider's id tokens are published.
    pub fn jwks_uri(&self) -> &str {
        &self.jwks_uri
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

    /// The exact signed hosted-domain value required for admission, if one was configured.
    pub fn hosted_domain(&self) -> Option<&str> {
        self.hosted_domain.as_deref()
    }
}

/// Every variable this module reads, each in its own named field.
///
/// **X-27**, and the whole of it. A value gets here by being named on the same line as the field it
/// lands in, so there is no order for it to be out of and no position for it to shift by. The three
/// things a variable needs — a field, an environment variable, a transport rule — are stated once,
/// together, in [`Supplied::read`].
///
/// The secret is wrapped in [`ClientSecret`] on the line it is read, so the bare `String` does not
/// outlive that expression and this struct carries no unredacted secret at any point. There is
/// deliberately no `Debug` here either: nothing needs one, and a type this close to the environment
/// is not where to start printing it.
struct Supplied {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    jwks_uri: String,
    client_id: String,
    client_secret: ClientSecret,
    redirect_uri: String,
    tenant: String,
    hosted_domain: Option<String>,
}

impl Supplied {
    /// One line per variable: the field it lands in, the environment variable it is read from, and
    /// — by which method reads it — whether this host checks its transport.
    ///
    /// This is the only place those three are paired, and each pairing is a single line the
    /// compiler holds together. Adding a variable is adding one line and one field:
    ///
    /// - a line naming a field this struct does not have does not compile;
    /// - a field this function does not fill does not compile;
    /// - a field [`OidcConfig::read`]'s pattern does not mention does not compile;
    /// - and there is no list anywhere else to add it to, because [`required`] and
    ///   [`transport_checked`] are reports of what this function did.
    ///
    /// The order is the order a refusal names them in: a struct literal evaluates its fields in
    /// written order, so this order is the one an operator reads at startup.
    fn read<L: Fn(&str) -> Option<String>>(reader: &mut Reader<L>) -> Self {
        Self {
            issuer: reader.value(ISSUER_ENV),
            authorization_endpoint: reader.channel(AUTHORIZATION_ENDPOINT_ENV),
            token_endpoint: reader.channel(TOKEN_ENDPOINT_ENV),
            jwks_uri: reader.channel(JWKS_URI_ENV),
            client_id: reader.value(CLIENT_ID_ENV),
            client_secret: ClientSecret(reader.value(CLIENT_SECRET_ENV)),
            redirect_uri: reader.channel(REDIRECT_URI_ENV),
            tenant: reader.value(TENANT_ENV),
            hosted_domain: reader.optional(HOSTED_DOMAIN_ENV),
        }
    }
}

/// Reads each variable by name, and remembers what it read.
///
/// Every list this module's refusals need — which variables an operator must set, which of those
/// are unset, which name a URL whose transport is checked, and which of *those* broke the rule — is
/// accumulated here, by the call that reads the variable. None of them is written down anywhere
/// else, so none of them can describe a read that does not happen or miss one that does.
///
/// That is what X-27 changed. Before it, `REQUIRED` and `TRANSPORT_CHECKED` were two lists standing
/// beside a third — the sequence of positional reads in [`OidcConfig::read`] — and all three had to
/// be edited together, correctly, with nothing but a hand-written test watching.
struct Reader<L> {
    /// The environment, injected. See [`OidcConfig::read`].
    lookup: L,
    /// Every variable read, in the order it was read: the list [`required`] reports.
    visited: Vec<&'static str>,
    /// Those of them the environment did not supply.
    unset: Vec<&'static str>,
    /// Those read through [`Reader::channel`], whatever their value: the list
    /// [`transport_checked`] reports.
    checked: Vec<&'static str>,
    /// Those of *those* not on a channel this host will use.
    insecure: Vec<&'static str>,
}

impl<L: Fn(&str) -> Option<String>> Reader<L> {
    fn of(lookup: L) -> Self {
        Self {
            lookup,
            visited: Vec::new(),
            unset: Vec::new(),
            checked: Vec::new(),
            insecure: Vec::new(),
        }
    }

    /// A variable this host neither dials nor navigates to: an issuer it compares as a string, an
    /// identifier, a secret, a tenant. No transport rule applies — see [`transport_checked`] for
    /// what is deliberately not on that list, and why [`ISSUER_ENV`] is the interesting case.
    fn value(&mut self, name: &'static str) -> String {
        self.visited.push(name);

        // A variable that is set but empty is unset. Naming one and leaving it blank is a mistake
        // with a silent success mode: the operator believes they configured a client secret, and
        // what they have is a host that authenticates as nobody.
        match (self.lookup)(name).filter(|value| !value.trim().is_empty()) {
            Some(value) => value,
            None => {
                self.unset.push(name);
                String::new()
            }
        }
    }

    /// An optional, non-secret deployment value. Empty is the same as absent: no domain gate.
    fn optional(&mut self, name: &'static str) -> Option<String> {
        (self.lookup)(name).filter(|value| !value.trim().is_empty())
    }

    /// A variable naming a URL this host will put an OIDC value on.
    ///
    /// The transport rule is applied here, at the read, which is what makes [`transport_checked`] a
    /// report of these calls rather than a list beside them.
    ///
    /// An unset variable is not a channel either, so it lands in `insecure` as well.
    /// [`OidcConfig::read`] refuses for the unset first: "set this" is the refusal an operator can
    /// act on, and "this is not https" about a variable they never set would be noise.
    fn channel(&mut self, name: &'static str) -> String {
        let value = self.value(name);
        self.checked.push(name);

        if !on_a_channel_this_host_will_use(&value) {
            self.insecure.push(name);
        }

        value
    }
}

/// Whether `endpoint` is on a channel this host will put an OIDC value on — its client secret, a
/// key set it trusts, or the `state`, `nonce` and PKCE challenge it sends a browser away with.
///
/// # The judgment call X-17 had to make: cleartext is refused, except on loopback
///
/// `https` is the answer, and the question is only what to do about `http`. Refusing it outright
/// is the tidier rule and the wrong one: **a local test identity provider is a real workflow**, it
/// is how somebody exercises this flow before they have a certificate for anything, and a rule that
/// forbids it pushes them towards the two worse habits — disabling verification somewhere, or
/// testing against a production provider.
///
/// So `http` is permitted **only on loopback**, and the reason is not leniency: those packets do
/// not reach a network interface. There is no path to be on. Anybody positioned to read them is
/// already inside this process's own machine, where the secret is readable from the environment
/// anyway, so the transport was never what was protecting it.
///
/// Everything else is refused, including an `http` address on a private range. "It is only the
/// internal network" is precisely the assumption that makes a cleartext client secret interesting
/// to an attacker who has got that far, and this host cannot tell a private range from a public one
/// by looking at a string in any case. A scheme this function does not recognise is refused too —
/// **refuse; never repair**: a value with a typo'd scheme, or none at all, is not a channel whose
/// safety this host has established, and guessing `https` for it would be repairing.
///
/// `localhost` is accepted by name. It resolves through the operator's own machine, and an operator
/// who has pointed it somewhere else has made a decision this host is not positioned to second-guess.
///
/// **Which host this is deciding about** is [`host_in`]'s problem, and it is the whole of the
/// difficulty: a rule about loopback is worth nothing if the address it reads is not the one reqwest
/// dials. See that function for what the agreement promises and where it stops.
///
/// # What a `true` here does not promise
///
/// **This is a statement about the channel and never about who is on the other end of it.** X-19
/// recorded the mechanism and X-23 did not change it: `https` returns immediately, before the host
/// is looked at at all. So a `true` means *if that URL is dialled or navigated to, it will be over
/// TLS* — nothing more. In particular it does not say that
/// [`AUTHORIZATION_ENDPOINT_ENV`] belongs to [`ISSUER_ENV`]'s provider, that [`JWKS_URI_ENV`]
/// publishes the keys that provider actually signs with, that [`REDIRECT_URI_ENV`] is a registration
/// the provider will honour — the provider re-checks that one against a registration this host does
/// not own — or that the certificate on the far side is anybody in particular beyond being valid for
/// the name the operator wrote. An operator who points these at a host they did not mean to gets a
/// confidential channel to the wrong place, and this function is not the thing that would notice.
///
/// Extending the check to the browser-facing variables widened *which URLs* get this promise. It did
/// not widen the promise.
fn on_a_channel_this_host_will_use(endpoint: &str) -> bool {
    let Some((scheme, rest)) = endpoint.split_once("://") else {
        return false;
    };

    match scheme.to_ascii_lowercase().as_str() {
        "https" => true,
        "http" => host_in(rest).is_some_and(is_loopback),
        _ => false,
    }
}

/// The host of a URL, given everything after its `://`, or `None` if this module cannot say.
///
/// Hand-rolled rather than parsed, because this crate carries no URL parser and adding one to
/// answer "is this loopback" would be a dependency taken for a twenty-line function.
///
/// # What this promises, and what it does not
///
/// The thing that matters is not that this parser is *correct*; it is that it agrees with the
/// parser that actually dials the endpoint — `url`, which reqwest resolves the very same string
/// with. Where the two disagree in the *admitting* direction, [`on_a_channel_this_host_will_use`] clears a
/// configuration whose client secret then goes to a host this module never looked at. X-17 shipped
/// exactly one such disagreement: WHATWG ends a special scheme's authority at `\` as well as at
/// `/`, `?` and `#`, so `url` reads `http://evil.example\@127.0.0.1/token` as `evil.example` while
/// this read the userinfo split and answered `127.0.0.1`.
///
/// So the promise is one-directional and it is this: **whenever this returns a host, `url` returns
/// the same host.** Everything below is that claim and nothing else — the WHATWG terminator set,
/// the last-`@` userinfo split, a bracketed literal that must actually be IPv6, and a port `url`
/// would accept. X-19 measured it against `url` 2.5.8 over 475,270 generated spellings and found no
/// case where this returns a host and `url` resolves a different one, or none.
///
/// The converse is deliberately **not** promised, and this is where the honest limit is. Plenty of
/// addresses `url` dials happily come back `None` here: IPv4 shorthand (`2130706433`, `0x7f.0.0.1`),
/// a trailing dot, a percent-encoded or IDNA-mapped spelling of `localhost`, a `\\` after the
/// scheme, whitespace `url` strips. Each of those refuses a working configuration, which an
/// operator meets at startup and fixes by spelling the address plainly. That is the trade this
/// function is making, and it is only sound in that one direction.
///
/// It is a measured agreement rather than a proved one. The parser that cannot disagree is `url`
/// itself, and adopting it is a dependency decision this story deliberately left open.
fn host_in(rest: &str) -> Option<&str> {
    // WHATWG's authority terminator set for a *special* scheme, which is what `http` is. The `\` is
    // the one X-17 did not have, and the whole of the divergence X-19 exists for.
    let authority = rest.split(['/', '\\', '?', '#']).next().unwrap_or_default();

    // `user:password@host`. Everything before the last `@` is userinfo, not the host — reading the
    // wrong side of it is how `http://127.0.0.1@evil.example/` would pass for loopback.
    let authority = match authority.rsplit_once('@') {
        Some((_, host)) => host,
        None => authority,
    };

    let (host, port) = match authority.strip_prefix('[') {
        // `[::1]:8080` — a bracketed IPv6 literal, whose own colons are not the port separator. The
        // bracket must close, what is inside it must really be IPv6, and only a port may follow:
        // `[127.0.0.1]` and `[::1]evil.example` are URLs `url` refuses outright, so this must not
        // read a host out of either.
        Some(bracketed) => {
            let (literal, after) = bracketed.split_once(']')?;
            literal.parse::<std::net::Ipv6Addr>().ok()?;

            match after {
                "" => (literal, ""),
                _ => (literal, after.strip_prefix(':')?),
            }
        }
        None => match authority.split_once(':') {
            Some((host, port)) => (host, port),
            None => (authority, ""),
        },
    };

    // WHATWG's port state takes ASCII digits and nothing else, and refuses what will not fit a
    // `u16`. An authority this reads a port out of and `url` does not is one where the two are
    // describing different URLs, whatever they then say about the host.
    let port_is_dialable = port.is_empty()
        || (port.bytes().all(|byte| byte.is_ascii_digit()) && port.parse::<u16>().is_ok());

    port_is_dialable.then_some(host)
}

/// Whether `host` names this machine.
///
/// `IpAddr::is_loopback` rather than a prefix test on the string, so the whole of `127.0.0.0/8` and
/// `::1` are covered without this module deciding what those ranges are.
fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|address| address.is_loopback())
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

    /// A configured URL is not on a channel this host will use.
    ///
    /// See [`on_a_channel_this_host_will_use`] for what is permitted and why, and
    /// [`transport_checked`] for which variables this applies to. Refused at startup rather than at
    /// the first sign-in, because by the time a sign-in has failed the client secret has already
    /// crossed the network in cleartext and a `state` has already been offered to the path.
    InsecureEndpoint {
        /// Every offending variable, in [`transport_checked`]'s order. Names only the variables —
        /// the value is the operator's own and does not need repeating back at them.
        insecure: Vec<&'static str>,
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
                        required().join(", "),
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
            // Says what is wrong, what the rule is, and what the loopback exception is for — an
            // operator meeting this at startup is very often the one running a local test provider.
            //
            // What is at stake is said as well as what the rule is, because an operator who reads
            // "must be https" about a URL a *browser* navigates to is entitled to ask why. The
            // clauses are in `transport_checked`'s order, one per variable, so the list above and
            // the reasons below can be read against each other.
            Self::InsecureEndpoint { insecure } => write!(
                f,
                "{} {} not on a channel this host will use. {} must each name an https URL — or an \
                 http one on loopback, for a local test provider. Between them they carry the \
                 state, the nonce and the PKCE challenge a browser is sent away with, \
                 {CLIENT_SECRET_ENV} as HTTP Basic credentials, the key set that decides which \
                 tokens can mint a session here, and the authorization code on its way back. OIDC \
                 sign-in will not be offered. /health and the catalogue are unaffected",
                insecure.join(", "),
                if insecure.len() > 1 { "are" } else { "is" },
                transport_checked().join(", "),
            ),
        }
    }
}

impl std::error::Error for ConfigRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unset { .. } | Self::InsecureEndpoint { .. } => None,
            Self::UnusableTenant { source } => Some(source),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::HashMap;

    /// A complete, well-formed environment.
    ///
    /// Every value is distinguishable from every other, so a test asserting where one landed cannot
    /// pass on a coincidence. `every_configured_value_lands_in_its_own_field` is what depends on
    /// that; the rest only need this map to satisfy every variable the read consumes.
    fn complete() -> HashMap<&'static str, String> {
        HashMap::from([
            (ISSUER_ENV, "https://accounts.example.com".to_string()),
            (
                AUTHORIZATION_ENDPOINT_ENV,
                "https://accounts.example.com/authorize".to_string(),
            ),
            (
                TOKEN_ENDPOINT_ENV,
                "https://accounts.example.com/oauth/token".to_string(),
            ),
            (
                JWKS_URI_ENV,
                "https://keys.example.com/jwks.json".to_string(),
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

    /// Every variable lands in the field that names it, asserted through the accessors the flow
    /// uses.
    ///
    /// **X-27 changed what this test is for, and left the assertions alone.** `read` used to consume
    /// the supplied values positionally, in `REQUIRED`'s order, so a variable added to one list and
    /// not the other shifted every value after it — silently, into a config that was well-formed
    /// and wrong, and this test was the only thing watching. Values are bound by name now and that
    /// shift cannot be expressed. What this still holds is [`Supplied::read`]'s pairing of a field
    /// to a variable, which is the one pairing a person can still get wrong.
    ///
    /// Every value here is distinguishable, so this fails on any misplacement rather than only on
    /// one between two fields that happened to differ.
    #[test]
    fn every_configured_value_lands_in_its_own_field() {
        let environment = complete();
        let config = read(&environment).expect("a complete environment configures OIDC");

        // Read back through the accessors, since those are what the flow uses. Each assertion names
        // the variable it came from, so a shift reads as "the token endpoint holds the JWKS URI"
        // rather than as an opaque inequality.
        let expected = |name: &'static str| environment[name].clone();

        assert_eq!(config.issuer(), expected(ISSUER_ENV));
        assert_eq!(
            config.authorization_endpoint(),
            expected(AUTHORIZATION_ENDPOINT_ENV),
        );
        assert_eq!(config.token_endpoint(), expected(TOKEN_ENDPOINT_ENV));
        assert_eq!(config.jwks_uri(), expected(JWKS_URI_ENV));
        assert_eq!(config.client_id(), expected(CLIENT_ID_ENV));
        assert_eq!(config.client_secret().expose(), expected(CLIENT_SECRET_ENV));
        assert_eq!(config.redirect_uri(), expected(REDIRECT_URI_ENV));
        assert_eq!(config.tenant().as_str(), expected(TENANT_ENV));

        // The other half of "in step": a variable this test does not account for is one whose value
        // nothing above would notice going astray.
        let required = required();
        assert_eq!(
            required.len(),
            environment.len(),
            "every variable the read consumes must have a distinguishable value here: {required:?}",
        );
    }

    /// **X-27.** The variables the read consumes and the variables this module documents are one
    /// set, and the read is the only place either is written down.
    ///
    /// [`required`] and [`transport_checked`] are reports of [`Supplied::read`], so they cannot
    /// drift from it — there is no second list to disagree with. What they *can* drift from is this
    /// module's public constants, in either direction: a variable read from a bare literal would be
    /// one an operator is never told about, and a documented constant nothing reads is one an
    /// operator is told to set and this host then ignores. That second one is the shape X-27 exists
    /// for, and it is the one that used to leave a green gate behind it.
    ///
    /// The list below is a deliberate restatement — the only one left in this module — and each
    /// assertion names the variable rather than reporting a count.
    #[test]
    fn every_variable_read_is_one_this_module_documents_and_no_other() {
        let documented = [
            ISSUER_ENV,
            AUTHORIZATION_ENDPOINT_ENV,
            TOKEN_ENDPOINT_ENV,
            JWKS_URI_ENV,
            CLIENT_ID_ENV,
            CLIENT_SECRET_ENV,
            REDIRECT_URI_ENV,
            TENANT_ENV,
        ];

        for variable in required() {
            assert!(
                documented.contains(&variable),
                "{variable} is read, and is not one of this module's documented constants",
            );
        }

        for variable in documented {
            assert!(
                required().contains(&variable),
                "{variable} is documented as required, and no read consumes it",
            );
        }

        // And the transport rule applies only to variables an operator is actually asked for: a
        // checked variable nothing requires would be a rule about a value that is never read.
        for variable in transport_checked() {
            assert!(
                required().contains(&variable),
                "{variable} has a transport rule, and is not a variable this host reads",
            );
        }
    }

    /// **X-27.** An empty environment names every variable the read consumes, in the read's own
    /// order, and reads as "not configured" rather than as "configured wrong".
    ///
    /// Both halves of that refusal now come out of the read itself: the list from what it visited,
    /// and `partial` from how much of that same visit came back unset. The coupling is what this
    /// pins — a refusal naming fewer variables than the read consumes is one an operator cannot fix
    /// in a single pass, which is what this module's documentation promises them.
    ///
    /// That the read consumes the right *set* in the first place is
    /// `every_variable_read_is_one_this_module_documents_and_no_other`'s claim, not this one's.
    #[test]
    fn an_empty_environment_names_every_variable_the_read_consumes() {
        let refusal = read(&HashMap::new()).expect_err("an empty environment is refused");

        assert_eq!(
            refusal,
            ConfigRefusal::Unset {
                unset: required(),
                partial: false,
            },
        );

        // In one message, in one pass, in a stable order — the whole point of naming all of them.
        let message = refusal.to_string();
        for variable in required() {
            assert!(
                message.contains(variable),
                "{variable} is missing: {message}"
            );
        }
    }

    /// **X-17.** A cleartext back channel is refused **at startup**, naming the variable.
    ///
    /// At startup and not at the first sign-in, because the failure it prevents is irreversible:
    /// by the time a sign-in has gone wrong, the client secret has already crossed the network as
    /// HTTP Basic credentials, and the remedy is rotating it rather than fixing a URL.
    #[test]
    fn a_cleartext_back_channel_is_refused_at_startup_by_name() {
        for (variable, cleartext) in [
            (
                TOKEN_ENDPOINT_ENV,
                "http://accounts.example.com/oauth/token",
            ),
            (JWKS_URI_ENV, "http://keys.example.com/jwks.json"),
        ] {
            let mut environment = complete();
            environment.insert(variable, cleartext.to_string());

            let refusal = read(&environment).expect_err(&format!(
                "a cleartext {variable} must be refused at startup"
            ));

            assert_eq!(
                refusal,
                ConfigRefusal::InsecureEndpoint {
                    insecure: vec![variable],
                },
                "and refused for its transport, not for something else",
            );

            let message = refusal.to_string();
            assert!(
                message.contains(variable),
                "the operator must be told which variable to fix: {message}",
            );
            // The rule, and the exception, so the operator running a local test provider is not
            // left guessing that loopback would have been allowed.
            assert!(message.contains("https"), "{message}");
            assert!(message.contains("loopback"), "{message}");
        }
    }

    /// All of them at once, following `Unset`: an operator who got one wrong — a proxy in front of
    /// everything, a copied-and-edited block of settings — very likely got all of them wrong.
    #[test]
    fn a_refusal_names_every_cleartext_endpoint_at_once() {
        let mut environment = complete();
        environment.insert(
            AUTHORIZATION_ENDPOINT_ENV,
            "http://accounts.example.com/a".to_string(),
        );
        environment.insert(
            TOKEN_ENDPOINT_ENV,
            "http://accounts.example.com/t".to_string(),
        );
        environment.insert(JWKS_URI_ENV, "http://keys.example.com/j".to_string());
        environment.insert(
            REDIRECT_URI_ENV,
            "http://exchange.example.com/api/signin/callback".to_string(),
        );

        let refusal = read(&environment).expect_err("all four are refused");

        assert_eq!(
            refusal,
            ConfigRefusal::InsecureEndpoint {
                insecure: transport_checked(),
            },
            "one restart, not four",
        );

        // Named in one message, and every one of them named. An operator who has to discover the
        // fourth by restarting three times has been made to do our bookkeeping.
        let message = refusal.to_string();
        for variable in transport_checked() {
            assert!(
                message.contains(variable),
                "{variable} is missing: {message}"
            );
        }
    }

    /// The judgment call, pinned: **loopback http is permitted, everything else is not.**
    ///
    /// A local test identity provider is a real workflow and the rule has to leave room for it —
    /// see [`on_a_channel_this_host_will_use`], which carries the whole argument. The refused half of this
    /// table is the part that matters: a private range is still a network, an unrecognised scheme
    /// is not a channel whose safety this host has established, and `127.0.0.1` appearing as
    /// *userinfo* is a host that is not on loopback at all.
    #[test]
    fn cleartext_is_permitted_on_loopback_and_nowhere_else() {
        for permitted in [
            "https://accounts.example.com/token",
            "HTTPS://accounts.example.com/token",
            "http://localhost:8080/token",
            "http://LOCALHOST:8080/token",
            "http://127.0.0.1:8080/token",
            "http://127.9.9.9/token",
            "http://[::1]:8080/token",
            "http://user:pass@127.0.0.1:8080/token",
        ] {
            assert!(
                on_a_channel_this_host_will_use(permitted),
                "{permitted} must be permitted",
            );
        }

        for refused in [
            "http://accounts.example.com/token",
            // "It is only the internal network" is exactly the assumption that makes a cleartext
            // client secret worth having, to an attacker who has got that far.
            "http://10.0.0.7/token",
            "http://192.168.1.10/token",
            // The host is `evil.example`; `127.0.0.1` is userinfo. A check that read the wrong side
            // of the `@` would call this loopback.
            "http://127.0.0.1@evil.example/token",
            "http://localhost.evil.example/token",
            // Refuse; never repair: a scheme this host does not recognise, or none at all.
            "htps://accounts.example.com/token",
            "accounts.example.com/token",
            "ftp://accounts.example.com/token",
            "",
        ] {
            assert!(
                !on_a_channel_this_host_will_use(refused),
                "{refused} must be refused",
            );
        }
    }

    /// **X-19.** The parser that *decides* and the parser that *dials* must not disagree.
    ///
    /// [`on_a_channel_this_host_will_use`] answers "is this host loopback"; `url` 2.5.8 — which reqwest
    /// resolves the very same string with — answers "what host is this". Where those two disagree in
    /// the *admitting* direction, this module clears a configuration that then sends
    /// [`CLIENT_SECRET_ENV`] as HTTP Basic credentials, in cleartext, to whatever host reqwest
    /// picked. That is not a check with a bug in it; that is no check at all for that spelling.
    ///
    /// The one spelling that did it was a backslash before the `@`:
    /// `http://evil.example\@127.0.0.1/token`. WHATWG ends a special scheme's authority at `\` as
    /// well as at `/`, `?` and `#`, so `url` reads the host as `evil.example` while this module read
    /// the userinfo split and answered `127.0.0.1`.
    ///
    /// So the table is the **class**, not the instance. It carries every spelling X-17's reviewer
    /// measured against `url` in one program and found conservative, plus the whole backslash family
    /// the divergence lives in. Refusing is the safe side of any disagreement — an operator meets it
    /// at startup, before a secret has moved — which is why spellings `url` would happily dial on
    /// loopback are refused here too, and marked as such.
    #[test]
    fn no_hostile_authority_is_read_as_loopback() {
        for hostile in [
            // The divergence X-19 exists for. `url` resolves each of these to `evil.example`,
            // because the authority ends at the `\`.
            "http://evil.example\\@127.0.0.1/token",
            "http://evil.example\\@localhost/token",
            "http://evil.example\\@[::1]/token",
            "http://evil.example\\127.0.0.1/token",
            "http://evil.example\\?@127.0.0.1/token",
            "http://evil.example\\#@127.0.0.1/token",
            // A bracketed literal that does not end the host. `url` refuses the URL outright, so
            // reqwest would never dial it; reading `::1` out of it and calling the address safe is
            // still this module answering a question it cannot answer.
            "http://[::1]evil.example/token",
            "http://[::1]@evil.example/token",
            // The fifteen X-17's reviewer confirmed conservative, pinned so they stay that way.
            // Several are loopback to `url` and refused here — an IPv4 shorthand, a trailing dot, an
            // IDNA mapping — which is the safe side of a disagreement and deliberate.
            "http://127.0.0.1.evil.com/token",
            "http://0x7f.0.0.1/token",
            "http://0177.0.0.1/token",
            "http://2130706433/token",
            "http://127.0.0.1./token",
            "http://[::ffff:127.0.0.1]/token",
            "http://[::1%eth0]/token",
            "http://localhost./token",
            "http://localhost.evil.example/token",
            "http://\u{24DB}ocalhost/token",
            "http://127\u{3002}0\u{3002}0\u{3002}1/token",
            "http://127.0.0.1\t@evil.example/token",
            "http://127.0.0.1\n@evil.example/token",
            "http://127.0.0.1@evil.example/token",
            " http://127.0.0.1/token",
            // `#`, `?` and `/` placements: the authority ends at the first of them, so what follows
            // is a fragment, a query or a path and never a host.
            "http://evil.example#@127.0.0.1/token",
            "http://evil.example?@127.0.0.1/token",
            "http://evil.example/@127.0.0.1/token",
            // The scheme separator itself. WHATWG accepts `\\`, `/\` and `\/` here for a special
            // scheme; this module accepts only `://`, so all of them are refused.
            "http:\\\\evil.example\\@127.0.0.1/token",
            "http:/\\127.0.0.1/token",
        ] {
            assert!(
                !on_a_channel_this_host_will_use(hostile),
                "{hostile:?} must be refused: `url` does not read a loopback host out of it",
            );
        }

        // The other half, in the same run: a fix that refuses everything is not a fix. These are the
        // local-test-provider addresses X-17 exists to keep working, and `url` reads a loopback host
        // out of every one of them.
        for genuine in [
            "http://127.0.0.1:8080/token",
            "http://localhost/token",
            "http://[::1]/token",
            "http://user:pass@127.0.0.1:8080/token",
            "https://accounts.example.com/token",
        ] {
            assert!(
                on_a_channel_this_host_will_use(genuine),
                "{genuine:?} must still be permitted",
            );
        }
    }

    /// Every variable [`transport_checked`] claims is checked is actually checked, and every
    /// variable that is checked is claimed.
    ///
    /// The constant is what the refusal message tells an operator the rule applies to, so a variable
    /// listed there and not enforced would be a promise this module does not keep. The converse
    /// matters as much now that the list is four long and lives beside a second list: a variable
    /// enforced in [`OidcConfig::read`]'s pairing and missing from the constant would be refused
    /// under a rule the message never stated.
    #[test]
    fn every_transport_checked_variable_is_actually_enforced_and_no_other() {
        for variable in transport_checked() {
            let mut environment = complete();
            environment.insert(variable, "http://not-loopback.example/x".to_string());

            assert_eq!(
                read(&environment).expect_err(&format!("{variable} is enforced")),
                ConfigRefusal::InsecureEndpoint {
                    insecure: vec![variable],
                },
            );
        }

        // The other direction. Setting *every* variable to a cleartext URL and comparing the
        // refusal against the constant catches an enforced-but-unlisted variable, which iterating
        // the constant cannot see. `ISSUER_ENV` is the live case: it is a URL, it is deliberately
        // not transport-checked, and this is what says so mechanically rather than in prose.
        let mut environment = complete();
        for variable in required() {
            environment.insert(variable, "http://not-loopback.example/x".to_string());
        }
        // `Tenant::new` runs before the transport check and would refuse that value first, which
        // would make this assert about the wrong thing. The tenant is not a URL in any case.
        environment.insert(TENANT_ENV, "acme".to_string());

        assert_eq!(
            read(&environment).expect_err("a wholly cleartext environment is refused"),
            ConfigRefusal::InsecureEndpoint {
                insecure: transport_checked(),
            },
            "exactly the listed variables are enforced, and in the listed order",
        );
    }

    /// **X-23.** A browser-facing endpoint is refused in cleartext too, naming its own variable.
    ///
    /// X-17 and X-19 checked only the two variables that carry a secret directly, on the argument
    /// that a browser enforces the transport of the addresses it navigates to. It does not: the
    /// browser will use an `http` authorization URL exactly as given, and that URL carries `state`,
    /// the `nonce` and the PKCE challenge, each readable and modifiable in flight. An operator who
    /// typed `http` here made a mistake this host can catch at startup, and until now it said
    /// nothing at all.
    ///
    /// Asserted through the message rather than through the variant, so this test says the same
    /// thing before and after the fix — it is the failing-first test, and it has to compile against
    /// the code that does not yet have the refusal. The variant itself is pinned by
    /// `every_transport_checked_variable_is_actually_enforced_and_no_other`, and the other half of
    /// the rule — that a fix which refuses everything is not a fix — by
    /// `every_checked_variable_admits_https_and_loopback_http`.
    #[test]
    fn a_cleartext_browser_facing_endpoint_is_refused_at_startup_by_name() {
        for (variable, cleartext) in [
            (
                AUTHORIZATION_ENDPOINT_ENV,
                "http://accounts.example.com/authorize",
            ),
            (
                REDIRECT_URI_ENV,
                "http://exchange.example.com/api/signin/callback",
            ),
        ] {
            let mut environment = complete();
            environment.insert(variable, cleartext.to_string());

            let refusal = read(&environment).expect_err(&format!(
                "a cleartext {variable} must be refused at startup"
            ));

            let message = refusal.to_string();
            assert!(
                message.contains(variable),
                "the operator must be told which variable to fix: {message}",
            );
            // Refused for its transport and not for something else, and told the rule *and* the
            // exception — the operator meeting this is very often running a local test provider.
            assert!(message.contains("https"), "{message}");
            assert!(message.contains("loopback"), "{message}");
        }
    }

    /// **X-23, and the half that keeps the refusal from being a wall.** Both spellings that must go
    /// on being accepted, for every transport-checked variable, in one run.
    ///
    /// The loopback exemption is the load-bearing one and it is asserted here *through the
    /// variables* rather than through the predicate alone. X-17's argument for it is unchanged by
    /// widening the list: **a local test identity provider is a real workflow**, it is how somebody
    /// exercises this flow before they have a certificate for anything, and forbidding it pushes
    /// them towards disabling verification somewhere or testing against a production provider. A
    /// browser will navigate to `http://localhost:9000/authorize` quite happily, and those packets
    /// reach no network interface, so there is no path to be on.
    ///
    /// The `https` half is here rather than left to `a_complete_environment_configures_the_flow`
    /// because a refusal is only correct in the company of what it still admits.
    #[test]
    fn every_checked_variable_admits_https_and_loopback_http() {
        // A whole local provider: every checked variable on loopback, spelled three different ways
        // so this does not pass on one accepted form.
        let mut loopback = complete();
        loopback.insert(
            AUTHORIZATION_ENDPOINT_ENV,
            "http://localhost:9000/authorize".to_string(),
        );
        loopback.insert(
            TOKEN_ENDPOINT_ENV,
            "http://127.0.0.1:9000/token".to_string(),
        );
        loopback.insert(JWKS_URI_ENV, "http://[::1]:9000/jwks".to_string());
        loopback.insert(
            REDIRECT_URI_ENV,
            "http://127.0.0.1:8080/api/signin/callback".to_string(),
        );

        let config =
            read(&loopback).expect("a local test identity provider must still be admitted");
        assert_eq!(
            config.authorization_endpoint(),
            "http://localhost:9000/authorize",
            "and admitted unaltered — refuse; never repair",
        );
        assert_eq!(
            config.redirect_uri(),
            "http://127.0.0.1:8080/api/signin/callback",
        );

        // And the ordinary deployment, in the same run: `https` everywhere, admitted.
        let https = read(&complete()).expect("an https environment must still be admitted");
        for (variable, configured) in [
            (AUTHORIZATION_ENDPOINT_ENV, https.authorization_endpoint()),
            (TOKEN_ENDPOINT_ENV, https.token_endpoint()),
            (JWKS_URI_ENV, https.jwks_uri()),
            (REDIRECT_URI_ENV, https.redirect_uri()),
        ] {
            assert_eq!(configured, complete()[variable], "for {variable}");
        }
    }

    /// Sign-in is not connecting. This host asks to learn who the human is and nothing else — any
    /// vendor scope here would turn one consent screen into a different one without anybody
    /// deciding to.
    #[test]
    fn the_requested_scopes_identify_the_human_and_grant_nothing() {
        let scopes: Vec<&str> = SCOPES.split_whitespace().collect();

        assert_eq!(scopes, ["openid"]);
        assert!(
            scopes.contains(&"openid"),
            "without `openid` this is not OIDC and there is no id token to bind a nonce to",
        );
    }
}
