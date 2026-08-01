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
//!
//! # What a tenant may occupy, and why the bound is here and not on the port
//!
//! Two bounds, [`MAX_CREDENTIAL_VALUE_BYTES`] and [`MAX_TENANT_STORE_BYTES`], each with its
//! argument written beside it. They answer different questions — *is this a credential* and *how
//! much of the store may one tenant hold* — and the second is the one that matters between tenants,
//! because `FileStore` rewrites and `fsync`s one file under one mutex on every write, so one
//! tenant's size is every other tenant's write latency. That is shared fate in the repository whose
//! claim is that tenants share nothing, which is why it is an invariant here rather than a capacity
//! note.
//!
//! **The bound is not on the [`SecretStore`](crate::SecretStore) port, and it cannot be.** That
//! port is `connector_secrets`', re-exported by this crate and deliberately not redefined — the
//! crate root says so, and says why: there is one credential addressing scheme in this ecosystem
//! and this crate is a doorway to it, not a second copy. Putting a bound on it would mean either
//! changing an upstream crate from here, or declaring a second `SecretStore` locally, and the
//! second is exactly what has already been refused.
//!
//! So the bound sits at the next seam in: [`ConnectorDeclaration::write_of`], which admits **one**
//! supplied value, and [`ConnectorDeclaration::writes`], which is a whole body's worth and is
//! written in terms of it. Those two are the **only** ways supplied values become writes, and the
//! second delegating to the first is what keeps the bound stated once — X-39 added the singular form
//! for rotation, and `one_value_is_admitted_by_the_same_step_as_a_whole_body` is what holds them
//! together. A composition cannot write a credential it has not addressed,
//! and it cannot get an addressed value out of this crate without the value being admitted first —
//! so the per-value bound is not a check a surface remembers to make, it is the step that produces
//! the thing there is to write. What that does *not* reach is a second `SecretStore` implementation
//! fed by something other than this host's create path: a Vault-backed store bound by another
//! composition inherits nothing from here, and saying otherwise would be a claim this repository
//! cannot keep.
//!
//! The per-tenant bound cannot have that shape, because deciding it needs a *reading of the store*
//! and this crate holds none. [`admit_tenant_occupancy`] is therefore the decision, and the caller
//! supplies the two numbers it is decided from; the surface that can read the store is the one that
//! must call it, and `routes::connections` does — inside the same claim that decides everything
//! else about a create, so what it read is still true when it answers.

use std::collections::BTreeMap;

use connector_secrets::{CredentialRef, Layout, Secret, TenantLayout};
use connector_spec::DEFAULT_SERVICE;

use crate::Tenant;

/// The most bytes one credential value may occupy. **Stated once, here.**
///
/// A credential on this surface is what a connector declares: an API token, a bearer or refresh
/// token, a signing or webhook secret, or — at the largest end anyone actually ships — a
/// PEM-encoded private key, which is about 3.2 KiB for RSA 4096. 8 KiB is a little over twice
/// that, so no credential a connector really declares is refused by it.
///
/// The bound is therefore about *kind* rather than about thrift: a value that does not fit is not
/// a credential that grew, it is something that is not a credential. It sits three orders of
/// magnitude below axum's 2 MB default body limit, which is what a caller would otherwise be
/// spending — and spending against every other tenant, because the store is one file.
///
/// It does **not** on its own bound what one tenant occupies; see [`MAX_TENANT_STORE_BYTES`].
pub const MAX_CREDENTIAL_VALUE_BYTES: usize = 8 * 1024;

/// The most bytes one tenant may occupy across the whole credential store. **Stated once, here.**
///
/// This is the bound that protects the neighbours, and it is a different question from the one
/// above. `connector_secrets::FileStore` holds one file and every `put`/`delete` rewrites and
/// `fsync`s the whole of it under one mutex, so the file's size is every tenant's write latency.
/// What this number buys is that a tenant's contribution to that shared cost is capped at 64 KiB
/// *whatever it does* — so the store's size is a function of how many tenants an operator has
/// admitted, a number they chose, rather than of what any one of them decided to upload.
///
/// 64 KiB is eight values at the per-value bound, or several hundred real tokens. A tenant with
/// every addressable connector in the compiled-in catalogue connected, at the ~100-byte tokens
/// those connectors actually declare, occupies single-digit kilobytes — two orders of magnitude
/// under this.
///
/// **The per-value bound alone would not give this.** A tenant may hold one value per declared
/// address, and the catalogue declares tens of them, so per-value alone leaves a ceiling of
/// `addresses × 8 KiB` — hundreds of kilobytes, and a ceiling that grows every time upstream adds a
/// connector. This one does not move when the catalogue does, which is the property worth having.
pub const MAX_TENANT_STORE_BYTES: usize = 64 * 1024;

// The two bounds answer different questions, and the second is only worth having while it is the
// tighter one. Asserted at compile time rather than in a test, because the failure it guards
// against is somebody editing one number: a tenant allowance below one whole credential refuses
// every real connection, and one so far above it that no tenant reaches it bounds nothing.
const _: () = assert!(MAX_TENANT_STORE_BYTES > MAX_CREDENTIAL_VALUE_BYTES);
const _: () = assert!(MAX_TENANT_STORE_BYTES < 16 * MAX_CREDENTIAL_VALUE_BYTES);

/// How many bytes a value occupies in the store, without exposing it.
///
/// The length and nothing else, so that a caller measuring a tenant's occupancy never has a line
/// holding the plaintext — there is nothing there for a `debug!` to turn into a disclosure.
pub fn stored_bytes(secret: &Secret) -> usize {
    secret.expose_secret().len()
}

/// Refuse a write that would take this tenant past [`MAX_TENANT_STORE_BYTES`].
///
/// `held` is what the tenant already occupies across the whole store and `adding` is what the
/// request would put there. Separated because only the caller can read the store, and inclusive at
/// the bound: a tenant sitting exactly on its allowance has not exceeded it.
///
/// # Errors
///
/// [`ConnectionRefusal::TenantAllowanceExhausted`], naming both numbers and the bound and never a
/// value.
pub fn admit_tenant_occupancy(held: usize, adding: usize) -> Result<(), ConnectionRefusal> {
    if held.saturating_add(adding) > MAX_TENANT_STORE_BYTES {
        return Err(ConnectionRefusal::TenantAllowanceExhausted {
            held,
            adding,
            limit: MAX_TENANT_STORE_BYTES,
        });
    }

    Ok(())
}

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

    /// Every supplied value, resolved to an address and admitted against the per-value bound.
    ///
    /// **The only way to turn supplied values into writes**, which is the point of it: the bound
    /// is not a check a surface remembers to make before writing, it is the step that produces the
    /// thing there is to write. A composition that resolves addresses one at a time through
    /// [`address_of`](Self::address_of) has no values in its hands and so cannot skip it.
    ///
    /// Nothing is written here and nothing may be: every name is resolved and every value admitted
    /// *before* the first pair is handed back, so a body with one good value and one that is not a
    /// credential stores neither. That is the same rule the address resolution already followed,
    /// extended to cover the values.
    ///
    /// The supplied *names* need no bound of their own — a name this connector does not declare is
    /// refused below, so the keys are bounded by the catalogue rather than by the caller.
    ///
    /// # Errors
    ///
    /// The refusals of [`address_of`](Self::address_of), and
    /// [`ConnectionRefusal::CredentialTooLarge`] for a value past
    /// [`MAX_CREDENTIAL_VALUE_BYTES`].
    pub fn writes(
        &self,
        tenant: &Tenant,
        supplied: &BTreeMap<String, String>,
    ) -> Result<Vec<(CredentialRef, Secret)>, ConnectionRefusal> {
        let mut writes = Vec::with_capacity(supplied.len());

        for (name, value) in supplied {
            writes.push(self.write_of(tenant, name, value)?);
        }

        Ok(writes)
    }

    /// **One** supplied value, resolved to its address and admitted against the per-value bound.
    ///
    /// The single-value form of [`writes`](Self::writes), and the same seam rather than a second
    /// one: [`writes`] is written in terms of this, so the bound is stated in one place and the
    /// two cannot drift.
    ///
    /// It exists because replacing a credential is an operation on exactly one of them (X-39), and
    /// asking the many-valued form for it would hand the caller a `Vec` whose length it has to
    /// assume — an assumption that is either an `unwrap` or a branch that can never be taken. A
    /// caller with one value says so in the type instead.
    ///
    /// # Errors
    ///
    /// The refusals of [`address_of`](Self::address_of), and
    /// [`ConnectionRefusal::CredentialTooLarge`] for a value past [`MAX_CREDENTIAL_VALUE_BYTES`].
    pub fn write_of(
        &self,
        tenant: &Tenant,
        name: &str,
        value: &str,
    ) -> Result<(CredentialRef, Secret), ConnectionRefusal> {
        let reference = self.address_of(tenant, name)?;

        if value.len() > MAX_CREDENTIAL_VALUE_BYTES {
            return Err(ConnectionRefusal::CredentialTooLarge {
                connector: self.connector.to_string(),
                credential: name.to_string(),
                bytes: value.len(),
                limit: MAX_CREDENTIAL_VALUE_BYTES,
            });
        }

        Ok((reference, Secret::new(value)))
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

/// Why a connection is refused before anything is written.
///
/// Most of these are "there is no address": something the connector must declare is missing, or a
/// name was supplied that it does not declare. The last two are the other kind — the address is
/// fine and what would go in it is not, because it is past a bound. Both kinds share this type
/// because both are decided in one place on the create path and answered by one mapping, and a
/// second refusal type beside it is how one of them comes to be answered differently by accident.
///
/// Every variant refuses; none guesses, and none repeats a credential value. The sizes they quote
/// are the caller's own numbers — what it sent, and what it already holds.
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

    /// A supplied value is larger than a credential is.
    ///
    /// Names the credential and the bound. The size is quoted because it is what makes the
    /// refusal actionable and it is the caller's own number; the value itself is not, and there is
    /// deliberately no field here one could occupy.
    #[error(
        "connector `{connector}` credential `{credential}` was sent as {bytes} bytes, and one \
         credential value may be at most {limit}. A credential is a token, not a file — nothing \
         was stored, and this refusal does not repeat what was sent"
    )]
    CredentialTooLarge {
        /// The connector that was named.
        connector: String,
        /// The credential whose value is past the bound.
        credential: String,
        /// How many bytes were sent.
        bytes: usize,
        /// [`MAX_CREDENTIAL_VALUE_BYTES`], so a refusal carries the bound rather than implying it.
        limit: usize,
    },

    /// This tenant already occupies as much of the store as one tenant may.
    ///
    /// Distinct from the variant above because the remedy is: that one is a value that is not a
    /// credential, this one is a tenant that has connected enough. Both numbers are this tenant's
    /// own — no other tenant's occupancy is disclosed, or even consulted.
    #[error(
        "this tenant occupies {held} bytes of the credential store and this request would add \
         {adding}, past the {limit} bytes one tenant may hold. Every write rewrites the whole \
         store, so one tenant's size is every other tenant's write latency — disconnect a \
         connector you no longer use before connecting another"
    )]
    TenantAllowanceExhausted {
        /// What this tenant already occupies, across every connector.
        held: usize,
        /// What this request would add.
        adding: usize,
        /// [`MAX_TENANT_STORE_BYTES`].
        limit: usize,
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

    /// A value larger than a credential is refused, and the refusal names the credential and the
    /// bound — never what was sent.
    #[test]
    fn a_value_past_the_bound_is_refused_and_names_the_credential_and_the_bound() {
        let oversized = "x".repeat(MAX_CREDENTIAL_VALUE_BYTES + 1);
        let supplied = BTreeMap::from([("zendesk.api_token".to_string(), oversized.clone())]);

        let refusal = zendesk()
            .writes(&acme(), &supplied)
            .expect_err("a value past the bound is not a credential");

        assert_eq!(
            refusal,
            ConnectionRefusal::CredentialTooLarge {
                connector: "zendesk".to_string(),
                credential: "zendesk.api_token".to_string(),
                bytes: MAX_CREDENTIAL_VALUE_BYTES + 1,
                limit: MAX_CREDENTIAL_VALUE_BYTES,
            },
        );

        let message = refusal.to_string();
        assert!(message.contains("zendesk.api_token"), "{message}");
        assert!(
            message.contains(&MAX_CREDENTIAL_VALUE_BYTES.to_string()),
            "the refusal must name the bound so an operator learns the limit: {message}",
        );
        assert!(
            !message.contains(&oversized),
            "the refusal must never repeat the value: {message}",
        );
    }

    /// The bound is inclusive: a value of exactly [`MAX_CREDENTIAL_VALUE_BYTES`] is a credential.
    #[test]
    fn a_value_at_the_bound_is_still_a_credential() {
        let supplied = BTreeMap::from([(
            "zendesk.api_token".to_string(),
            "x".repeat(MAX_CREDENTIAL_VALUE_BYTES),
        )]);

        let writes = zendesk()
            .writes(&acme(), &supplied)
            .expect("a value at the bound is admitted");

        assert_eq!(writes.len(), 1);
        assert_eq!(
            address_path(&writes[0].0),
            "tenants/acme/com.zendesk.api/api_token",
        );
        assert_eq!(stored_bytes(&writes[0].1), MAX_CREDENTIAL_VALUE_BYTES);
    }

    /// One value past the bound refuses the *whole* body, including the values that were fine —
    /// the same rule the address resolution already followed, extended to cover the values. A
    /// caller that got the good half written would have a connection nobody can tell from a
    /// working one until an operation fails.
    #[test]
    fn one_value_past_the_bound_refuses_every_value_in_the_body() {
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

        let supplied = BTreeMap::from([
            ("slack.bot_token".to_string(), "a-token".to_string()),
            (
                "slack.signing_secret".to_string(),
                "x".repeat(MAX_CREDENTIAL_VALUE_BYTES + 1),
            ),
        ]);

        assert!(
            matches!(
                slack.writes(&acme(), &supplied),
                Err(ConnectionRefusal::CredentialTooLarge { .. }),
            ),
            "one value past the bound must refuse the whole body",
        );
    }

    /// The second bound, which the first does not give: inclusive at the allowance, and refusing
    /// with both of the tenant's own numbers and the limit.
    #[test]
    fn the_tenant_allowance_is_inclusive_and_bounds_the_whole() {
        assert!(admit_tenant_occupancy(0, MAX_TENANT_STORE_BYTES).is_ok());
        assert!(admit_tenant_occupancy(MAX_TENANT_STORE_BYTES, 0).is_ok());

        let refusal = admit_tenant_occupancy(MAX_TENANT_STORE_BYTES, 1)
            .expect_err("one byte past the allowance is past it");

        assert_eq!(
            refusal,
            ConnectionRefusal::TenantAllowanceExhausted {
                held: MAX_TENANT_STORE_BYTES,
                adding: 1,
                limit: MAX_TENANT_STORE_BYTES,
            },
        );

        let message = refusal.to_string();
        assert!(
            message.contains(&MAX_TENANT_STORE_BYTES.to_string()),
            "{message}",
        );
        // The argument for the number, restated where it is enforced: the reason one tenant's size
        // is anybody else's business is that every write rewrites the whole store.
        assert!(message.contains("rewrites the whole store"), "{message}");
    }

    /// The single-value seam a rotation writes through resolves the same address and applies the
    /// same bound as the many-valued one — because it *is* the many-valued one's step.
    ///
    /// Both directions are pinned: what `write_of` produces for a name is what `writes` produces
    /// for a map holding only that name, and a value past the bound is refused identically. A
    /// second copy of the bound is exactly what this test exists to catch.
    #[test]
    fn one_value_is_admitted_by_the_same_step_as_a_whole_body() {
        let supplied = BTreeMap::from([("zendesk.api_token".to_string(), "a-token".to_string())]);

        let (reference, secret) = zendesk()
            .write_of(&acme(), "zendesk.api_token", "a-token")
            .expect("a declared credential of a declared authority");
        let writes = zendesk()
            .writes(&acme(), &supplied)
            .expect("the same, through the many-valued form");

        assert_eq!(writes.len(), 1);
        assert_eq!(reference, writes[0].0);
        assert_eq!(
            address_path(&reference),
            "tenants/acme/com.zendesk.api/api_token",
        );
        assert_eq!(stored_bytes(&secret), stored_bytes(&writes[0].1));

        // The bound is the same one, not a second copy of the number.
        let oversized = "x".repeat(MAX_CREDENTIAL_VALUE_BYTES + 1);
        assert_eq!(
            zendesk()
                .write_of(&acme(), "zendesk.api_token", &oversized)
                .expect_err("a value past the bound is not a credential"),
            ConnectionRefusal::CredentialTooLarge {
                connector: "zendesk".to_string(),
                credential: "zendesk.api_token".to_string(),
                bytes: MAX_CREDENTIAL_VALUE_BYTES + 1,
                limit: MAX_CREDENTIAL_VALUE_BYTES,
            },
        );

        // And an undeclared name has no address here either, so a caller naming one in a path
        // cannot reach one.
        assert!(matches!(
            zendesk().write_of(&acme(), "zendesk.api_key", "a-token"),
            Err(ConnectionRefusal::UndeclaredCredential { .. }),
        ));
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
