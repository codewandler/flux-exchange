//! Where a tenant's credential for a connector lives — derived, never accepted.
//!
//! A **connection** is a tenant plus a connector plus the credential values that connector
//! declares. This module owns the one question that decides whether any of that is safe: *what is
//! the address*, and the answer is a function of three facts, none of which a caller supplies.
//!
//! ```text
//! tenants/<tenant>/<authority>/<credential>
//!         ^^^^^^^^ ^^^^^^^^^^^ ^^^^^^^^^^^^
//!         |        |           the connector's declared credential leaf
//!         |        the connector's declared authority
//!         the resolved principal's tenant
//! ```
//!
//! [`Tenant`] is already validated at construction, so the first segment cannot walk out of its own
//! prefix. The other two come from a [`ConnectorDeclaration`] — a *view* of what a connector
//! declares, built by the composing binary from whatever catalogue it carries, the same way
//! [`ConnectorSurface`](crate::ConnectorSurface) is a view of a manifest. This crate learns nothing
//! about `connector-catalog` in the process.
//!
//! # Nothing is defaulted
//!
//! A connector that declares no authority has **no** address, and this refuses rather than guessing
//! one. `connector_spec::Connector::credential_ref_for` returns `Ok(None)` for exactly that case;
//! turning it into a default would write a value to an address no operation ever reads from, which
//! is a lost credential presented to the operator as a success. `AGENTS.md` § Invariants, last
//! entry: refuse; never repair, and name the address rather than the value.
//!
//! # The address has no instance dimension *in the version this pins*
//!
//! Nothing above varies per *connection*, so a tenant with two accounts on one connector — a
//! sandbox Zendesk and a production one — renders one address for both. That gap is **X-14** here
//! and **C-406** upstream, where the dimension has now landed:
//!
//! ```text
//! tenants/<tenant>/<authority>[/@instances/<uuid>][/<service>]/<credential>
//! ```
//!
//! It is **not published**: crates.io still serves `codewandler-connector-spec` 0.8.0 and this
//! workspace pins `"0.8"` from the registry, so the level cannot be spelled here yet — and it is
//! not closed by spelling a second address locally either, because two spellings of an address is
//! how two components stop agreeing where a credential lives.
//!
//! What this module does instead is make the collision *visible* and leave the level one insertion
//! away. [`ConnectorDeclaration::addresses`] is total and deterministic, so a caller can ask whether
//! an address is already occupied before it writes; and
//! [`address_of_declared`](ConnectorDeclaration::address_of_declared) is the single place any
//! address is composed, which is where the `@instances/<uuid>` level goes when the pin moves.
//! Upstream's note is that mapping an operator's label to that uuid is the *host's* job — this
//! repository's, i.e. X-14 — so that function is the seam X-14 extends rather than replaces. The
//! refusal that stands in for it meanwhile belongs to the surface that writes; see
//! `docs/designs/connections.md`.

use connector_secrets::{CredentialRef, Layout, TenantLayout};
use connector_spec::DEFAULT_SERVICE;

use crate::Tenant;

/// One credential a connector declares, as much of it as an address needs.
///
/// Both fields come straight from the connector's own declaration. `name` is the flat-namespace
/// name an operation references (`zendesk.api_token`) and `leaf` is the last segment of the address
/// (`api_token`) — the path already carries the authority, so the vendor prefix would be said twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DeclaredCredential<'a> {
    /// The flat-namespace name, e.g. `zendesk.api_token`. This is what a caller names.
    pub name: &'a str,
    /// The last segment of the address, e.g. `api_token`.
    pub leaf: &'a str,
}

/// What a connector declares that an address is derived from.
///
/// A borrowed view rather than an owned model: the composing binary already holds this data — for
/// the `flux-exchange` binary it is `&'static` catalogue data compiled in — and copying it here
/// would create a second thing to keep in step with the catalogue.
#[derive(Debug, Clone, Copy)]
pub struct ConnectorDeclaration<'a> {
    /// The connector id, as the catalogue spells it, e.g. `zendesk`.
    pub connector: &'a str,
    /// The reverse-DNS authority the connector publishes under, e.g. `com.zendesk.api`.
    ///
    /// `Option`, and deliberately not defaulted: without it no address renders at all.
    pub authority: Option<&'a str>,
    /// Every credential the connector declares, in declaration order.
    pub credentials: &'a [DeclaredCredential<'a>],
}

impl<'a> ConnectorDeclaration<'a> {
    /// The credential this connector declares under `name`, if it declares one.
    pub fn declares(&self, name: &str) -> Option<DeclaredCredential<'a>> {
        self.credentials
            .iter()
            .copied()
            .find(|credential| credential.name == name)
    }

    /// The address of one declared credential, for one tenant.
    ///
    /// # Errors
    ///
    /// [`ConnectionRefusal::UndeclaredAuthority`] when the connector declares none, so there is no
    /// address to render; [`ConnectionRefusal::NoCredentialDeclared`] when it declares no
    /// credential at all; [`ConnectionRefusal::UndeclaredCredential`] when `name` is not one of
    /// them; and [`ConnectionRefusal::Unaddressable`] when the components are declared but do not
    /// compose into an address the addressing scheme admits.
    pub fn address_of(
        &self,
        tenant: &Tenant,
        name: &str,
    ) -> Result<CredentialRef, ConnectionRefusal> {
        if self.credentials.is_empty() {
            return Err(ConnectionRefusal::NoCredentialDeclared {
                connector: self.connector.to_string(),
            });
        }

        let Some(declared) = self.declares(name) else {
            return Err(ConnectionRefusal::UndeclaredCredential {
                connector: self.connector.to_string(),
                credential: name.to_string(),
                declared: self.declared_names(),
            });
        };

        self.address_of_declared(tenant, declared)
    }

    /// Every declared credential's address, for one tenant, in declaration order.
    ///
    /// The whole set rather than one at a time, because every question this host asks about a
    /// connection — does it exist, destroy it, which values are set — is a question about all of
    /// them, and deriving them one by one is how a caller ends up covering only the first.
    ///
    /// # Errors
    ///
    /// The same refusals as [`address_of`](Self::address_of), except
    /// [`UndeclaredCredential`](ConnectionRefusal::UndeclaredCredential), which cannot arise from a
    /// name this method did not take.
    pub fn addresses(
        &self,
        tenant: &Tenant,
    ) -> Result<Vec<(DeclaredCredential<'a>, CredentialRef)>, ConnectionRefusal> {
        if self.credentials.is_empty() {
            return Err(ConnectionRefusal::NoCredentialDeclared {
                connector: self.connector.to_string(),
            });
        }

        self.credentials
            .iter()
            .copied()
            .map(|declared| {
                self.address_of_declared(tenant, declared)
                    .map(|reference| (declared, reference))
            })
            .collect()
    }

    /// The address of a credential this declaration is already known to carry.
    ///
    /// **The one place an address is composed**, and therefore the seam X-14 extends. When
    /// `connector-spec` publishes the instance level, the `@instances/<uuid>` segment is inserted
    /// *here* — the function grows the tenant's chosen instance as an argument and passes it to the
    /// upstream constructor — and no other call site re-spells the address. Upstream states that
    /// resolving an operator's label to that uuid is the host's job, which makes it this function's
    /// job rather than a new one beside it.
    fn address_of_declared(
        &self,
        tenant: &Tenant,
        declared: DeclaredCredential<'a>,
    ) -> Result<CredentialRef, ConnectionRefusal> {
        let Some(authority) = self.authority else {
            return Err(ConnectionRefusal::UndeclaredAuthority {
                connector: self.connector.to_string(),
            });
        };

        // The service segment is `DEFAULT_SERVICE`, which the layout elides — a credential is
        // declared at connector level and belongs to the connector rather than to one of its
        // services, exactly as `connector_spec::Connector::credential_ref_for` composes it. This is
        // a view of that composition, not a second one.
        CredentialRef::new(tenant.as_str(), authority, DEFAULT_SERVICE, declared.leaf).map_err(
            |reason| ConnectionRefusal::Unaddressable {
                connector: self.connector.to_string(),
                credential: declared.name.to_string(),
                reason,
            },
        )
    }

    /// The names this connector declares, for a refusal that says what would have worked.
    fn declared_names(&self) -> Vec<String> {
        self.credentials
            .iter()
            .map(|credential| credential.name.to_string())
            .collect()
    }
}

/// The path a credential address renders to.
///
/// [`TenantLayout`] and nothing else, because that is what `connector_secrets::FileStore` renders
/// with: a refusal that quoted a second spelling would send an operator to a path the store never
/// looked at.
pub fn address_path(reference: &CredentialRef) -> String {
    TenantLayout.render(reference)
}

/// Why a connection has no address. Every variant refuses; none guesses one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConnectionRefusal {
    /// The connector declares no authority, so no address renders at all.
    ///
    /// Not defaulted to anything. A guessed authority is a credential written where no operation
    /// will read it — a loss that looks like a success from every side.
    #[error(
        "connector `{connector}` declares no authority, so there is no address to keep its \
         credential at. The connector must declare one before this tenant can connect it; nothing \
         here guesses a default, because a credential stored at a guessed address is one no \
         operation will ever read"
    )]
    UndeclaredAuthority {
        /// The connector that declares none.
        connector: String,
    },

    /// The connector declares no credential, so there is nothing to address.
    #[error(
        "connector `{connector}` declares no credential, so there is nothing to store for it. \
         That is the connector's own declaration and not a gap here"
    )]
    NoCredentialDeclared {
        /// The connector that declares none.
        connector: String,
    },

    /// A credential was named that the connector does not declare.
    #[error(
        "connector `{connector}` declares no credential named `{credential}`; it declares {}. A \
         value stored under an undeclared name would sit at an address no operation reads",
        crate::connections::quoted(declared)
    )]
    UndeclaredCredential {
        /// The connector that was named.
        connector: String,
        /// The credential name that is not declared.
        credential: String,
        /// What it does declare.
        declared: Vec<String>,
    },

    /// Every component was declared, and they do not compose into a usable address.
    ///
    /// Distinct from the two above because the remedy is different: this one is a malformed
    /// declaration upstream rather than a missing one.
    #[error("connector `{connector}` credential `{credential}` has no usable address: {reason}")]
    Unaddressable {
        /// The connector that was named.
        connector: String,
        /// The credential whose address would not render.
        credential: String,
        /// What the addressing scheme said.
        reason: String,
    },
}

/// Render a list of names for a refusal, or say plainly that there are none.
fn quoted(names: &[String]) -> String {
    if names.is_empty() {
        return "none".to_string();
    }

    names
        .iter()
        .map(|name| format!("`{name}`"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const ZENDESK: &[DeclaredCredential<'static>] = &[DeclaredCredential {
        name: "zendesk.api_token",
        leaf: "api_token",
    }];

    fn zendesk() -> ConnectorDeclaration<'static> {
        ConnectorDeclaration {
            connector: "zendesk",
            authority: Some("com.zendesk.api"),
            credentials: ZENDESK,
        }
    }

    fn acme() -> Tenant {
        Tenant::new("acme").expect("a plain tenant id")
    }

    /// The Acceptance's address, rendered. The service segment is elided, which is why the story's
    /// `tenants/<tenant>/<authority>/<credential>` and upstream's
    /// `tenants/<tenant>/<authority>/<service>/<credential>` are the same address and not two.
    #[test]
    fn the_address_is_derived_from_the_tenant_and_the_declaration() {
        let reference = zendesk()
            .address_of(&acme(), "zendesk.api_token")
            .expect("a declared credential of a declared authority");

        assert_eq!(
            address_path(&reference),
            "tenants/acme/com.zendesk.api/api_token",
        );
        assert_eq!(reference.tenant(), "acme");
    }

    /// **The Acceptance's second failing-first test.** A connector that declares no authority is
    /// refused, and the refusal says which fact is missing — it does not fall back to the connector
    /// id, to the vendor name, or to anything else that would render a plausible path.
    #[test]
    fn a_connector_with_no_declared_authority_is_refused_rather_than_guessed() {
        let undeclared = ConnectorDeclaration {
            connector: "acme-crm",
            authority: None,
            credentials: &[DeclaredCredential {
                name: "acme-crm.api_token",
                leaf: "api_token",
            }],
        };

        let refusal = undeclared
            .address_of(&acme(), "acme-crm.api_token")
            .expect_err("a connector with no authority has no address");

        assert_eq!(
            refusal,
            ConnectionRefusal::UndeclaredAuthority {
                connector: "acme-crm".to_string(),
            },
        );

        let message = refusal.to_string();
        assert!(message.contains("acme-crm"), "{message}");
        assert!(message.contains("declares no authority"), "{message}");

        // The whole-set derivation refuses identically. A caller that reached for `addresses`
        // rather than `address_of` must not get a partial answer.
        assert!(undeclared.addresses(&acme()).is_err());
    }

    /// The other half of the same rule, over the whole set: one undeclared authority refuses the
    /// lot rather than returning the addresses that happened to render.
    #[test]
    fn every_declared_credential_gets_an_address() {
        let slack = ConnectorDeclaration {
            connector: "slack",
            authority: Some("com.slack.api"),
            credentials: &[
                DeclaredCredential {
                    name: "slack.bot_token",
                    leaf: "bot_token",
                },
                DeclaredCredential {
                    name: "slack.signing_secret",
                    leaf: "signing_secret",
                },
            ],
        };

        let rendered: Vec<String> = slack
            .addresses(&acme())
            .expect("both credentials are addressable")
            .iter()
            .map(|(_, reference)| address_path(reference))
            .collect();

        assert_eq!(
            rendered,
            vec![
                "tenants/acme/com.slack.api/bot_token".to_string(),
                "tenants/acme/com.slack.api/signing_secret".to_string(),
            ],
        );
    }

    /// A connector that declares nothing to store is refused before an address is attempted, and
    /// the refusal is its own variant: "the connector declares no credential" and "you named one it
    /// does not declare" send an operator to different places.
    #[test]
    fn a_connector_that_declares_no_credential_is_refused_distinctly() {
        let freshdesk = ConnectorDeclaration {
            connector: "freshdesk",
            authority: Some("com.freshdesk.api"),
            credentials: &[],
        };

        assert_eq!(
            freshdesk
                .address_of(&acme(), "freshdesk.api_key")
                .expect_err("nothing is declared"),
            ConnectionRefusal::NoCredentialDeclared {
                connector: "freshdesk".to_string(),
            },
        );
        assert!(freshdesk.addresses(&acme()).is_err());
    }

    /// An undeclared name is refused and the refusal lists what would have worked, so an operator
    /// with a typo does not have to go and read the catalogue to find it.
    #[test]
    fn an_undeclared_credential_is_refused_and_the_declared_ones_are_named() {
        let refusal = zendesk()
            .address_of(&acme(), "zendesk.api_key")
            .expect_err("`api_key` is not what zendesk declares");

        let message = refusal.to_string();
        assert!(message.contains("zendesk.api_key"), "{message}");
        assert!(message.contains("`zendesk.api_token`"), "{message}");
    }

    /// Two tenants, one connector, two addresses. This is the property every cross-tenant refusal
    /// on the surface rests on, asserted where it is decided rather than only over HTTP.
    #[test]
    fn two_tenants_never_share_an_address() {
        let globex = Tenant::new("globex").expect("a plain tenant id");

        let acme_reference = zendesk()
            .address_of(&acme(), "zendesk.api_token")
            .expect("an address");
        let globex_reference = zendesk()
            .address_of(&globex, "zendesk.api_token")
            .expect("an address");

        assert_ne!(acme_reference, globex_reference);
        assert_eq!(
            address_path(&globex_reference),
            "tenants/globex/com.zendesk.api/api_token",
        );
    }

    /// `Tenant` is the only way a tenant reaches an address, and it refuses a traversing spelling
    /// at construction — so there is no way to hold one that could walk out of its own prefix.
    /// Asserted here as well as in `principal`, because this is the module that depends on it.
    #[test]
    fn a_traversing_tenant_cannot_be_held_at_all() {
        for hostile in ["../../etc", "a/b", ".."] {
            assert!(
                Tenant::new(hostile).is_err(),
                "`{hostile}` must be unusable as a tenant, so no address can be built from it",
            );
        }
    }
}
