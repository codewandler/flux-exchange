//! Deployment-owned operator authority, separate from principal kind.

use std::collections::BTreeSet;

use exchange_host::{Principal, PrincipalKind};

/// Comma-separated immutable identity-provider subjects permitted to administer this deployment.
///
/// The value is deployment metadata, not request input. Production keeps it outside the committed
/// Fly configuration because subjects identify real people even though they are not secrets.
pub const OPERATOR_SUBJECTS_ENV: &str = "FLUX_EXCHANGE_OPERATOR_SUBJECTS";

/// A fail-closed set of immutable principal ids.
#[derive(Clone, Default)]
pub struct OperatorPolicy {
    subjects: BTreeSet<String>,
    available: bool,
}

impl OperatorPolicy {
    /// Read the deployment policy. Missing, non-Unicode, empty, or malformed input admits nobody.
    pub fn from_env() -> Self {
        match std::env::var(OPERATOR_SUBJECTS_ENV) {
            Ok(value) => Self::parse(&value).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    fn parse(value: &str) -> Option<Self> {
        let entries: Vec<&str> = value.split(',').map(str::trim).collect();
        if entries.is_empty()
            || entries.iter().any(|entry| entry.is_empty())
            || entries
                .iter()
                .any(|entry| entry.chars().any(char::is_whitespace))
        {
            return None;
        }

        let subjects: BTreeSet<String> = entries.iter().map(|entry| (*entry).to_owned()).collect();
        (subjects.len() == entries.len()).then_some(Self {
            subjects,
            available: true,
        })
    }

    /// Whether this signed-in principal is an operator.
    pub fn admits(&self, principal: &Principal) -> bool {
        #[cfg(test)]
        if self.subjects.contains("__all_test_users__") {
            return principal.kind() == PrincipalKind::User;
        }
        self.available
            && principal.kind() == PrincipalKind::User
            && self.subjects.contains(principal.id())
    }

    /// Whether a usable policy was supplied, for value-free startup diagnostics.
    pub fn available(&self) -> bool {
        self.available
    }

    /// A policy containing one immutable subject, used by `--dev` and focused tests.
    pub fn one(subject: impl Into<String>) -> Self {
        Self {
            subjects: [subject.into()].into_iter().collect(),
            available: true,
        }
    }

    /// Preserve pre-policy route fixtures while focused X-91 tests bind explicit subjects.
    #[cfg(test)]
    pub fn all_users_for_test() -> Self {
        Self::one("__all_test_users__")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_host::{Tenant, TenantError};

    fn principal(kind: PrincipalKind, id: &str) -> Principal {
        Principal::new(
            kind,
            id,
            Tenant::new("acme").unwrap_or_else(|_: TenantError| unreachable!()),
        )
    }

    #[test]
    fn policy_is_exact_fail_closed_and_separate_from_kind() {
        let policy = OperatorPolicy::parse("248289761001, 248289761002").expect("policy");

        assert!(policy.admits(&principal(PrincipalKind::User, "248289761001")));
        assert!(!policy.admits(&principal(PrincipalKind::User, "24828976100")));
        assert!(!policy.admits(&principal(PrincipalKind::ServiceAccount, "248289761001")));
        assert!(!OperatorPolicy::default().admits(&principal(PrincipalKind::User, "248289761001")));
    }

    #[test]
    fn malformed_or_ambiguous_policy_is_unavailable() {
        for value in ["", " ", "one,", ",one", "one,,two", "one,one", "one two"] {
            assert!(OperatorPolicy::parse(value).is_none(), "{value:?}");
        }
    }
}
