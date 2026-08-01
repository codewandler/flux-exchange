//! An [`Identity`] for local development — armed explicitly, or not present at all.
//!
//! # Why this lives in the binary and not in the host
//!
//! `exchange_host` is the crate a product embeds, and it deliberately carries no identity provider.
//! A development identity shipped inside it would be a development identity every downstream
//! composition links, whether or not it wanted one — and the only thing standing between that and
//! a production hole would be each of those products remembering not to arm it. Here it is one
//! composition's own code, and a product that embeds the host never sees it.
//!
//! # How it is armed, and what happens if it is not
//!
//! One environment variable, [`DEV_IDENTITY_ENV`], holding a roster of the principals it will mint:
//!
//! ```text
//! FLUX_EXCHANGE_DEV_IDENTITY=user:alice@acme,agent:triage-bot@acme
//! ```
//!
//! - **Unset** — no identity port is bound at all. That is the state the host already refuses a
//!   reachable bind in, so the default is the safe one and nothing has to be turned off.
//! - **Set and malformed** — the process refuses to start and names the entry. It does not skip the
//!   bad entry and arm the rest: a roster that silently lost a principal is a roster whose operator
//!   is debugging the wrong thing.
//! - **Set and well-formed** — the port is bound, announced at startup as a development one, and
//!   the bind rule refuses any address other than loopback for as long as it is armed. See
//!   [`IdentityBinding::Development`](crate::bind::IdentityBinding::Development).
//!
//! The tenant of every principal is fixed **here**, at startup, from the roster. That is the whole
//! reason this type is shaped as a roster rather than as "trust whatever the caller claims": there
//! is no request field a caller could set that reaches it.

use std::collections::BTreeMap;
use std::fmt;

use exchange_host::{
    async_trait, Identity, IdentityError, Principal, PrincipalKind, Tenant, TenantError,
};

use crate::session::{SessionError, SessionStore, SessionToken};

/// The environment variable that arms the development identity.
pub const DEV_IDENTITY_ENV: &str = "FLUX_EXCHANGE_DEV_IDENTITY";

/// The kinds a roster entry may name, in the spelling it must use.
const KINDS: &[(&str, PrincipalKind)] = &[
    ("user", PrincipalKind::User),
    ("agent", PrincipalKind::Agent),
    ("service", PrincipalKind::Service),
];

/// An identity port for local development.
///
/// It resolves two things, and it is worth being blunt about the first:
///
/// 1. A **roster handle** — `alice` — presented as a bearer token, which resolves straight to that
///    principal with no secret at all. That is not an oversight; it is what makes this usable
///    without a provider, and it is precisely why arming it forces a loopback bind.
/// 2. A **session token** minted by `POST /session`, which is what a browser then carries in a
///    cookie.
pub struct DevIdentity {
    /// Handle to principal. The tenant lives here, put there at startup and never by a request.
    roster: BTreeMap<String, Principal>,
    /// The sessions this port has opened.
    sessions: SessionStore,
}

impl DevIdentity {
    /// Read the roster from the environment.
    ///
    /// `Ok(None)` means nothing armed it, which is not an error — it is the default, and it is the
    /// state in which this host has no identity port and refuses a reachable bind.
    pub fn armed() -> Result<Option<Self>, DevIdentityRefusal> {
        match std::env::var(DEV_IDENTITY_ENV) {
            Err(_) => Ok(None),
            Ok(roster) => Self::from_roster(&roster).map(Some),
        }
    }

    /// Parse a roster, or refuse and name the entry that could not be read.
    ///
    /// Separate from [`DevIdentity::armed`] so the refusals below can be tested without a test
    /// mutating the process environment out from under its neighbours.
    pub fn from_roster(raw: &str) -> Result<Self, DevIdentityRefusal> {
        let entries: Vec<&str> = raw
            .split(',')
            .map(str::trim)
            .filter(|entry| !entry.is_empty())
            .collect();

        // Refuse; never repair. Naming the variable and then leaving it empty is a mistake with a
        // silent success mode — the operator believes they armed a dev identity and did not.
        if entries.is_empty() {
            return Err(DevIdentityRefusal::EmptyRoster);
        }

        let mut roster = BTreeMap::new();
        for entry in entries {
            let (handle, principal) = read_entry(entry)?;

            // Two entries claiming one handle means one of them silently loses, and which one
            // depends on the order they were written in. Refuse instead.
            if roster.insert(handle.clone(), principal).is_some() {
                return Err(DevIdentityRefusal::DuplicateHandle { handle });
            }
        }

        Ok(Self {
            roster,
            sessions: SessionStore::new(),
        })
    }

    /// The principals this port will mint, for the startup announcement.
    pub fn roster(&self) -> impl Iterator<Item = (&str, &Principal)> {
        self.roster
            .iter()
            .map(|(handle, principal)| (handle.as_str(), principal))
    }

    /// Open a session for an already-resolved principal.
    ///
    /// Takes the principal rather than a handle, so a session can only ever be opened for a caller
    /// the guard already resolved. There is no path from a request field to this argument.
    pub fn open_session(&self, principal: Principal) -> Result<SessionToken, SessionError> {
        self.sessions.open(principal)
    }

    /// Close whatever session the caller presented.
    pub fn close_session(&self, presented: &str) {
        self.sessions.close(presented);
    }
}

#[async_trait]
impl Identity for DevIdentity {
    async fn resolve(&self, presented: &str) -> Result<Option<Principal>, IdentityError> {
        // Anonymous, and deliberately not an error: the trait draws this line, and a caller that
        // reads "not signed in" as "your credential was rejected" sends its user to the wrong page.
        if presented.is_empty() {
            return Ok(None);
        }

        if let Some(principal) = self.sessions.resolve(presented) {
            return Ok(Some(principal));
        }

        self.roster
            .get(presented)
            .cloned()
            .map(Some)
            // Presented and not recognised. `Rejected` and not `Ok(None)`, because the caller did
            // hand over a credential and it was bad.
            .ok_or(IdentityError::Rejected)
    }
}

/// Read one `kind:id@tenant` entry.
fn read_entry(entry: &str) -> Result<(String, Principal), DevIdentityRefusal> {
    let malformed = || DevIdentityRefusal::MalformedEntry {
        entry: entry.to_string(),
    };

    let (kind, rest) = entry.split_once(':').ok_or_else(malformed)?;
    let (id, tenant) = rest.split_once('@').ok_or_else(malformed)?;

    if id.is_empty() {
        return Err(malformed());
    }

    let kind = KINDS
        .iter()
        .find(|(spelling, _)| *spelling == kind)
        .map(|(_, kind)| *kind)
        .ok_or_else(|| DevIdentityRefusal::UnknownKind {
            entry: entry.to_string(),
            kind: kind.to_string(),
        })?;

    // `Tenant::new` is the authority on what a tenant may be spelled; do not re-validate here.
    let tenant = Tenant::new(tenant).map_err(|source| DevIdentityRefusal::UnusableTenant {
        entry: entry.to_string(),
        source,
    })?;

    Ok((id.to_string(), Principal::new(kind, id, tenant)))
}

/// Why the development identity would not arm.
///
/// Every variant names the entry it could not read. A roster entry is a handle, a kind and a
/// tenant — none of them a secret — so naming it is the fastest route to the fix, and consistent
/// with `StartupRefusal`, which names the bind address for the same reason.
#[derive(Debug, PartialEq, Eq)]
pub enum DevIdentityRefusal {
    /// The variable was set but named no principal.
    EmptyRoster,

    /// An entry was not `kind:id@tenant`.
    MalformedEntry {
        /// The entry as written.
        entry: String,
    },

    /// An entry named a kind of principal that does not exist.
    UnknownKind {
        /// The entry as written.
        entry: String,
        /// The kind it named.
        kind: String,
    },

    /// An entry named a tenant that is not usable as an address segment.
    UnusableTenant {
        /// The entry as written.
        entry: String,
        /// Why the tenant was refused.
        source: TenantError,
    },

    /// Two entries claimed the same handle.
    DuplicateHandle {
        /// The handle they both claimed.
        handle: String,
    },
}

impl fmt::Display for DevIdentityRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let kinds = KINDS
            .iter()
            .map(|(spelling, _)| *spelling)
            .collect::<Vec<_>>()
            .join(", ");

        match self {
            Self::EmptyRoster => write!(
                f,
                "{DEV_IDENTITY_ENV} is set but names no principal. Either unset it, or give it a \
                 roster, e.g. {DEV_IDENTITY_ENV}=user:alice@acme",
            ),
            Self::MalformedEntry { entry } => write!(
                f,
                "{DEV_IDENTITY_ENV} entry {entry:?} is not `kind:id@tenant`, \
                 e.g. user:alice@acme. Accepted kinds: {kinds}",
            ),
            Self::UnknownKind { entry, kind } => write!(
                f,
                "{DEV_IDENTITY_ENV} entry {entry:?} names kind {kind:?}. Accepted kinds: {kinds}",
            ),
            Self::UnusableTenant { entry, source } => write!(
                f,
                "{DEV_IDENTITY_ENV} entry {entry:?} names an unusable tenant: {source}",
            ),
            Self::DuplicateHandle { handle } => write!(
                f,
                "{DEV_IDENTITY_ENV} names handle {handle:?} twice, so one of the two principals \
                 would silently win. Give each principal its own handle",
            ),
        }
    }
}

impl std::error::Error for DevIdentityRefusal {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::UnusableTenant { source, .. } => Some(source),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The roster is where a tenant comes from, so this is the shape the rest of the story rests
    /// on: a handle resolves to a principal whose tenant was fixed at startup.
    #[tokio::test]
    async fn a_handle_resolves_to_the_principal_the_roster_armed() {
        let dev = DevIdentity::from_roster("user:alice@acme,agent:triage-bot@globex")
            .expect("a well-formed roster");

        let alice = dev
            .resolve("alice")
            .await
            .expect("a rostered handle resolves")
            .expect("and is not anonymous");
        assert_eq!(alice.tenant().as_str(), "acme");
        assert_eq!(alice.kind(), PrincipalKind::User);

        let bot = dev
            .resolve("triage-bot")
            .await
            .expect("a rostered handle resolves")
            .expect("and is not anonymous");
        assert_eq!(bot.tenant().as_str(), "globex");
        assert_eq!(bot.kind(), PrincipalKind::Agent);
    }

    /// The trait's line, at the port itself: nothing presented is anonymous, something presented
    /// and unrecognised is rejected.
    #[tokio::test]
    async fn nothing_presented_is_anonymous_and_something_bad_is_rejected() {
        let dev = DevIdentity::from_roster("user:alice@acme").expect("a well-formed roster");

        assert!(matches!(dev.resolve("").await, Ok(None)));
        assert!(matches!(
            dev.resolve("bob").await,
            Err(IdentityError::Rejected)
        ));
    }

    /// A session opened for a resolved principal carries that principal's tenant, and closing it
    /// stops it resolving.
    #[tokio::test]
    async fn a_session_carries_the_principal_it_was_opened_for() {
        let dev = DevIdentity::from_roster("user:alice@acme").expect("a well-formed roster");
        let alice = dev
            .resolve("alice")
            .await
            .expect("a rostered handle resolves")
            .expect("and is not anonymous");

        let token = dev.open_session(alice).expect("the OS has randomness");
        let resolved = dev
            .resolve(token.as_str())
            .await
            .expect("a live session resolves")
            .expect("and is not anonymous");
        assert_eq!(resolved.tenant().as_str(), "acme");

        dev.close_session(token.as_str());
        assert!(matches!(
            dev.resolve(token.as_str()).await,
            Err(IdentityError::Rejected)
        ));
    }

    /// Refuse; never repair — asserted across every way a roster can be wrong. In particular an
    /// empty value must not be treated as "unset", which would arm nothing while the operator
    /// believed otherwise.
    #[test]
    fn a_roster_that_cannot_be_read_is_refused_rather_than_repaired() {
        let cases = [
            ("", DevIdentityRefusal::EmptyRoster),
            ("   ", DevIdentityRefusal::EmptyRoster),
            (
                "alice@acme",
                DevIdentityRefusal::MalformedEntry {
                    entry: "alice@acme".into(),
                },
            ),
            (
                "user:alice",
                DevIdentityRefusal::MalformedEntry {
                    entry: "user:alice".into(),
                },
            ),
            (
                "user:@acme",
                DevIdentityRefusal::MalformedEntry {
                    entry: "user:@acme".into(),
                },
            ),
            (
                "root:alice@acme",
                DevIdentityRefusal::UnknownKind {
                    entry: "root:alice@acme".into(),
                    kind: "root".into(),
                },
            ),
            (
                "user:alice@acme,user:alice@globex",
                DevIdentityRefusal::DuplicateHandle {
                    handle: "alice".into(),
                },
            ),
        ];

        for (roster, expected) in cases {
            let refusal = DevIdentity::from_roster(roster)
                .map(|_| ())
                .expect_err(&format!("`{roster}` must be refused"));
            assert_eq!(refusal, expected, "for roster `{roster}`");
        }
    }

    /// The tenant in a roster entry goes through `Tenant::new`, so a spelling that could walk out
    /// of its own credential prefix is refused at arming rather than at use.
    #[test]
    fn a_traversing_tenant_is_refused_at_arming() {
        for hostile in ["user:alice@../../etc", "user:alice@a/b", "user:alice@"] {
            let refusal = DevIdentity::from_roster(hostile)
                .map(|_| ())
                .expect_err(&format!("`{hostile}` must be refused"));

            assert!(
                matches!(refusal, DevIdentityRefusal::UnusableTenant { .. }),
                "`{hostile}` was refused as {refusal:?} rather than for its tenant",
            );
        }
    }

    /// A refusal that does not say what would have worked just moves the guessing to the operator.
    #[test]
    fn a_refusal_names_the_entry_and_what_would_have_worked() {
        let message = DevIdentity::from_roster("root:alice@acme")
            .map(|_| ())
            .expect_err("an unknown kind is refused")
            .to_string();

        assert!(message.contains("root:alice@acme"), "{message}");
        assert!(message.contains(DEV_IDENTITY_ENV), "{message}");
        assert!(message.contains("user"), "{message}");
        assert!(message.contains("agent"), "{message}");
        assert!(message.contains("service"), "{message}");
    }
}
