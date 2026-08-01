//! Who is asking, and on whose behalf.

use serde::{Deserialize, Serialize};

/// The longest a tenant identifier may be. Generous for any id a real deployment mints, and short
/// enough that a tenant id can never be a path-length attack on the credential address it prefixes.
const MAX_TENANT_LEN: usize = 64;

/// A tenant: the isolation boundary every credential address is prefixed with.
///
/// Validated **at construction**, not at use. A crafted tenant id must be impossible to hold, not
/// merely impossible to use — a value that validates late gets copied into a log line, a metric
/// label or an error message on the way there.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
pub struct Tenant(String);

impl Tenant {
    /// Validate and wrap a tenant identifier.
    ///
    /// Accepts ASCII alphanumerics, `-` and `_`. Everything else is refused, which in particular
    /// refuses `.` and `/` — the two characters that would let a tenant walk out of its own prefix
    /// in the credential address `tenants/<tenant>/<authority>/<credential>`.
    pub fn new(raw: impl Into<String>) -> Result<Self, TenantError> {
        let raw = raw.into();

        if raw.is_empty() {
            return Err(TenantError::Empty);
        }
        if raw.len() > MAX_TENANT_LEN {
            return Err(TenantError::TooLong {
                len: raw.len(),
                max: MAX_TENANT_LEN,
            });
        }
        if let Some(bad) = raw
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || *c == '-' || *c == '_'))
        {
            return Err(TenantError::IllegalCharacter(bad));
        }

        Ok(Self(raw))
    }

    /// The validated identifier.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Tenant {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a tenant identifier was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TenantError {
    /// The identifier was empty.
    #[error("a tenant identifier may not be empty")]
    Empty,

    /// The identifier was too long to be an address segment.
    #[error("a tenant identifier may be at most {max} bytes, got {len}")]
    TooLong {
        /// What was supplied.
        len: usize,
        /// The limit.
        max: usize,
    },

    /// The identifier contained a character that is not addressable.
    #[error("a tenant identifier may contain only ASCII alphanumerics, `-` and `_`; found {0:?}")]
    IllegalCharacter(char),
}

/// What kind of thing is asking.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrincipalKind {
    /// A signed-in human. Manages connections, credentials and grants; may call operations
    /// interactively.
    User,
    /// A non-human caller holding its own minted token. **The primary caller** — humans sign in to
    /// wire things up and to see what happened; agents are what call operations all day.
    Agent,
    /// Another backend acting on behalf of one of its own accounts and actors.
    Service,
}

/// A resolved caller.
///
/// The tenant is part of the principal, and that is the whole point: it is read from whatever
/// authenticated the caller, and never from a path segment, a body field, or a header a client may
/// set. There is deliberately no constructor that takes a tenant separately from an identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Principal {
    kind: PrincipalKind,
    id: String,
    tenant: Tenant,
}

impl Principal {
    /// Construct a principal that an [`Identity`](crate::Identity) implementation has resolved.
    pub fn new(kind: PrincipalKind, id: impl Into<String>, tenant: Tenant) -> Self {
        Self {
            kind,
            id: id.into(),
            tenant,
        }
    }

    /// What kind of caller this is.
    pub fn kind(&self) -> PrincipalKind {
        self.kind
    }

    /// Its stable identifier within the tenant.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The tenant it belongs to.
    pub fn tenant(&self) -> &Tenant {
        &self.tenant
    }
}

impl std::fmt::Display for Principal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}:{}@{}", self.kind, self.id, self.tenant)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_identifier_is_accepted() {
        assert_eq!(
            Tenant::new("acme-9f3a4b2c").unwrap().as_str(),
            "acme-9f3a4b2c"
        );
    }

    /// The refusal that matters: a tenant id must not be able to walk out of its own prefix in the
    /// credential address it leads.
    #[test]
    fn a_traversing_spelling_is_refused() {
        for hostile in ["../../etc", "a/b", "..", "a.b"] {
            assert!(
                Tenant::new(hostile).is_err(),
                "`{hostile}` must be refused as a tenant identifier",
            );
        }
    }

    #[test]
    fn empty_and_overlong_are_refused_distinctly() {
        assert_eq!(Tenant::new("").unwrap_err(), TenantError::Empty);
        assert!(matches!(
            Tenant::new("x".repeat(MAX_TENANT_LEN + 1)).unwrap_err(),
            TenantError::TooLong { .. },
        ));
        assert!(Tenant::new("x".repeat(MAX_TENANT_LEN)).is_ok());
    }

    /// A principal renders its tenant, because an audit line that omits it cannot be read twice.
    #[test]
    fn a_principal_displays_its_tenant() {
        let principal = Principal::new(
            PrincipalKind::Agent,
            "triage-bot",
            Tenant::new("acme").unwrap(),
        );
        assert_eq!(principal.to_string(), "Agent:triage-bot@acme");
    }
}
