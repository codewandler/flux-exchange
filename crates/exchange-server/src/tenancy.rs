//! Deployment tenancy selected once at startup.

use exchange_host::{Deployment, Principal, Tenant, TenantError};

/// The tenant named once for a single-tenant deployment.
pub const TENANT_SETTING: &str = "FLUX_EXCHANGE_TENANT";

/// Whether this process serves several tenants or one startup-declared tenant.
///
/// This is independent of authentication: OIDC, verified local users and a development roster can
/// each be composed with either shape. A principal still carries its provider-issued tenant; the
/// single-tenant boundary admits an exact match and never rewrites authority from another tenant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tenancy {
    /// Principals may belong to several tenants; locally executing runtimes remain refused.
    MultiTenant,
    /// Every admitted principal belongs to this one tenant.
    SingleTenant(Tenant),
}

impl Tenancy {
    /// The ordinary shared-deployment shape.
    pub const fn multi() -> Self {
        Self::MultiTenant
    }

    /// Validate and select one startup tenant.
    pub fn single(raw: impl Into<String>) -> Result<Self, TenancyRefusal> {
        Tenant::new(raw)
            .map(Self::SingleTenant)
            .map_err(|source| TenancyRefusal { source })
    }

    /// The runtime admission class implied by this tenancy declaration.
    pub const fn deployment(&self) -> Deployment {
        match self {
            Self::MultiTenant => Deployment::MultiTenant,
            Self::SingleTenant(_) => Deployment::SingleTenant,
        }
    }

    /// The one configured tenant, when this is a single-tenant deployment.
    pub fn tenant(&self) -> Option<&Tenant> {
        match self {
            Self::MultiTenant => None,
            Self::SingleTenant(tenant) => Some(tenant),
        }
    }

    /// Whether a resolved principal agrees with this startup declaration.
    ///
    /// A mismatch refuses. Replacing the principal's tenant would let a token minted for one
    /// tenant exercise another tenant's authority after an operator changed one setting.
    pub fn admits(&self, principal: &Principal) -> bool {
        self.tenant()
            .is_none_or(|tenant| tenant == principal.tenant())
    }
}

impl Default for Tenancy {
    fn default() -> Self {
        Self::multi()
    }
}

/// Why a deployment tenant could not be selected.
#[derive(Debug, thiserror::Error)]
#[error("{TENANT_SETTING} names an unusable tenant: {source}")]
pub struct TenancyRefusal {
    source: TenantError,
}

#[cfg(test)]
mod tests {
    use super::*;
    use exchange_host::{PrincipalKind, Tenant};

    fn principal(tenant: &str) -> Principal {
        Principal::new(
            PrincipalKind::User,
            "alice",
            Tenant::new(tenant).expect("a literal tenant"),
        )
    }

    /// **X-59's failing-first test.** A provider may authenticate an identity, but the deployment
    /// boundary must resolve it only when its startup tenant agrees. Rewriting `beta` into `acme`
    /// would turn authority for one tenant into authority for another, so disagreement refuses.
    #[test]
    fn one_startup_tenant_admits_only_principals_of_that_tenant() {
        let tenancy = Tenancy::single("acme").expect("a literal tenant");

        assert!(tenancy.admits(&principal("acme")));
        assert!(!tenancy.admits(&principal("beta")));
        assert_eq!(
            tenancy.deployment(),
            exchange_host::Deployment::SingleTenant
        );
    }

    #[test]
    fn multi_tenant_keeps_admitting_provider_principals_without_rewriting_them() {
        let tenancy = Tenancy::multi();

        assert!(tenancy.admits(&principal("acme")));
        assert!(tenancy.admits(&principal("beta")));
        assert_eq!(tenancy.deployment(), exchange_host::Deployment::MultiTenant);
        assert_eq!(tenancy.tenant(), None);
    }

    #[test]
    fn the_startup_setting_uses_the_address_safe_tenant_vocabulary() {
        for raw in ["", "../acme", "acme.example"] {
            let refusal = Tenancy::single(raw).expect_err("the tenant must be refused");
            let message = refusal.to_string();
            assert!(message.contains(TENANT_SETTING), "{message}");
            if !raw.is_empty() {
                assert!(
                    !message.contains(raw),
                    "the configured value need not be repeated"
                );
            }
        }
    }
}
