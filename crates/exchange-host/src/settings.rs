//! **A tenant's non-secret connection settings** — the `{subdomain}` in a templated base URL.
//!
//! X-12 made this host execute and exposed the gap immediately: sixteen of the fifty-three shipped
//! connectors declare a `base_url` templated on a per-instance value, and there was nowhere for a
//! tenant to put it. The invoker bound an empty `MemoryConfig`, so those connectors refused by name
//! — the right failure, and still a surface that ran forty of fifty-three.
//!
//! `docs/designs/connections.md` recorded why this was deferred out of X-10: *"a vendor subdomain is
//! exactly the per-instance fact with no home until two instances can be told apart"*. This module is
//! that home. What it is **not** is the instance dimension itself — see § *Where this sits relative
//! to X-14* below.
//!
//! # Configuration is not a credential, and is deliberately not stored as one
//!
//! This is the decision the story asks to be argued rather than assumed, and it is a decision about
//! **which store**, not about which directory.
//!
//! - **A subdomain is not a secret.** It is in the URL of every request the connector makes, in
//!   every audit record of one, and in the vendor's own dashboard. Keeping it at `0600` beside an
//!   API token would claim a protection it does not need, and — worse — would make the claim about
//!   the token weaker by association: a store whose contents are "mostly not secret" is one nobody
//!   treats as a secret store.
//! - **`held` would come to mean two things.** `GET /api/connections` reports, per declared
//!   credential, whether this tenant holds a value at its address. A subdomain written into the
//!   credential store is a value at an address, so a connection carrying a subdomain and **no
//!   token** would report as held. `DELETE`, whose reason to exist is revoking a leaked secret,
//!   would then report a subdomain among the credentials it destroyed.
//! - **The tenant occupancy bound would come to mean two things.** [`MAX_TENANT_STORE_BYTES`]
//!   bounds a tenant's share of the *credential* file, and the argument for the number is that
//!   `FileStore` rewrites and `fsync`s the whole file under one mutex, so one tenant's size is every
//!   other tenant's **write latency for credentials**. Settings spent against that allowance would
//!   let an operator fill it with subdomains and be told to "disconnect a connector you no longer
//!   use" — advice about the wrong thing entirely. So this store carries its own two bounds,
//!   [`MAX_SETTING_VALUE_BYTES`] and [`MAX_TENANT_SETTINGS_BYTES`], with their own arguments.
//! - **Upstream already drew this line.** `connector-pack` has two ports, not one: a value arriving
//!   through [`ConfigStore`] "carries no redaction guarantee", which is precisely why a secret may
//!   not travel through it. Storing settings as credentials here would be this repository
//!   disagreeing with the crate it hands both to.
//!
//! So: a **second file, a second store, a second pair of bounds**, and the credential store is not
//! touched. What the two share is where they may sit — [`crate::paths`], asked before either is
//! created — because "is this path somewhere a commit could pick it up" is one question and a
//! tenant's vendor account identifiers committed to a repository is a real leak even though it is
//! not a credential one.
//!
//! # The value is per tenant, and the tenant is not the caller's to name
//!
//! Every read and every write here is keyed by `(tenant, connector, service, kind, name)`, and the
//! tenant arrives as a [`Tenant`] — validated at construction, so it cannot walk out of its own
//! prefix — read off the resolved principal by the surface that calls this. There is no method here
//! that takes a tenant as a bare `&str` **except** [`ConfigStore::get`], which is upstream's
//! signature and is the read side: the pack asks for the tenant its [`Configuration`] was bound to,
//! which `Invoker::invoke` binds from the principal in the same expression as the credential port.
//!
//! # What a connector declares, read off the connector and not off its base URL
//!
//! [`declared_settings`] answers *what does this connector need configured*, and it answers it from
//! each operation's own compiled Flux through `connector_pack::Rehearsal` — the same derivation the
//! pack itself makes when it projects an operation, rather than a second one beside it.
//!
//! Scanning `base_url` for `{placeholders}` is the obvious cheaper version and it is **wrong**, by
//! measurement rather than by argument: it finds zendesk's `subdomain` and misses bitbucket's
//! `workspace`, cloudflare's `zone_id`, contentful's two `space_id`s, statuspage's `page_id`,
//! vercel's `teamId` and docusign's `account_id`, each of which is a configuration variable the
//! operation's Flux carries somewhere other than the base URL. A host enumerating the surface that
//! way would tell an operator they had supplied everything and then refuse the call.
//!
//! # This host stores what it is given; the pack decides what may be substituted
//!
//! Nothing here inspects a value for host-shapedness, and that is deliberate. `connector-pack`
//! validates the **composed authority** at the one substitution point — an allow-list of host
//! characters, so `acme.zendesk.com@evil.example` is refused rather than turned into a request to
//! `evil.example.zendesk.com` — and it does so knowing which of a URL's positions the value lands
//! in, which this crate does not. A second opinion here would be a second spelling of one rule, and
//! the one that disagreed would be the one deciding whether a tenant's value can move an origin.
//! `tests/connection_settings.rs::no_setting_can_move_the_destination_host` is what holds that
//! refusal to arriving.
//!
//! What this module *does* refuse is a value at an address the connector never declared, for
//! [`ConnectorDeclaration`](crate::ConnectorDeclaration)'s reason: a value stored under an
//! undeclared name sits where no operation reads it, which is a loss that looks like a success from
//! every side.
//!
//! # Where this sits relative to X-14
//!
//! **Before it, and it does not pre-empt it.** X-14 gives the *credential* address an
//! `@instances/<uuid>` level so a tenant can hold a sandbox Zendesk beside a production one; this
//! story gives a connection the values it needs to resolve at all. The two are independent today
//! and compose later: the key here is `(tenant, connector, service, kind, name)` — the key
//! `connector-pack`'s own port already uses, which this host does not get to change — and the
//! instance level lands in it exactly where it lands in the credential address, as one more
//! component between the connector and the service. Doing X-14 first would have meant designing an
//! instance-aware settings key against a `ConfigStore` that has no instance parameter, which is a
//! shape upstream would have to move first. Doing this first costs X-14 one added component in
//! [`SettingsStore::at`] and nothing else.

use std::collections::BTreeSet;

use connector_pack::{ConfigStore, Field};

use crate::Tenant;

/// The most bytes one connection setting may occupy. **Stated once, here.**
///
/// A setting on this surface is what a connector templates into a request: a subdomain, a vendor
/// host, a workspace or zone or space id, a team slug, or the non-secret user half of a Basic
/// credential — an account name or an email address. The largest of those is a hostname, which DNS
/// bounds at 253 bytes; 1 KiB is four times that.
///
/// The bound is therefore about *kind* rather than about thrift, exactly as
/// [`MAX_CREDENTIAL_VALUE_BYTES`](crate::MAX_CREDENTIAL_VALUE_BYTES) is: a value that does not fit
/// is not a setting that grew, it is something that is not a setting. **It is deliberately not the
/// credential bound and must not be unified with it** — the two answer questions about different
/// kinds of value, and the smaller number here is the one that is true about this kind.
pub const MAX_SETTING_VALUE_BYTES: usize = 1024;

/// The most bytes one tenant may occupy across the whole settings store. **Stated once, here.**
///
/// The bound that protects the neighbours, and a different question from the one above — the same
/// split [`MAX_TENANT_STORE_BYTES`](crate::MAX_TENANT_STORE_BYTES) makes on the credential side, and
/// for the same mechanical reason: this store is one file, and every write rewrites and `fsync`s the
/// whole of it under one lock, so one tenant's size is every other tenant's write latency.
///
/// 16 KiB is sixteen values at the per-value bound, or several hundred real ones. A tenant with
/// every configurable connector in the compiled-in catalogue supplied — sixteen connectors,
/// twenty-odd values, at the tens of bytes a subdomain actually occupies — sits three orders of
/// magnitude under it.
///
/// **This allowance is separate from the credential one and is not summed with it.** A tenant that
/// has filled its credential allowance can still supply a subdomain, and a tenant that has filled
/// this one is told to remove a *setting* rather than to disconnect a connector. Collapsing the two
/// would make one refusal give advice about the other store, which is the whole of why the
/// configuration lives here in the first place — see this module's documentation.
pub const MAX_TENANT_SETTINGS_BYTES: usize = 16 * 1024;

// The two bounds answer different questions, and the second is only worth having while it is the
// tighter one. Asserted at compile time rather than in a test, for
// `crate::connections`' reason: a tenant allowance below one whole value refuses every real
// connection, and one so far above it that no tenant reaches it bounds nothing.
const _: () = assert!(MAX_TENANT_SETTINGS_BYTES > MAX_SETTING_VALUE_BYTES);
const _: () = assert!(MAX_TENANT_SETTINGS_BYTES < 32 * MAX_SETTING_VALUE_BYTES);

/// **Which kind of non-secret value** a connector asks a tenant for.
///
/// The two kinds `connector-pack`'s [`Field`] distinguishes, as an owned type this crate can put in
/// a refusal, serialise into a store and hand back from [`declared_settings`]. `Field` borrows its
/// name; a declared setting outlives the catalogue read that produced it.
///
/// Matched exhaustively with no wildcard arm wherever it is read, deliberately: a third kind
/// arriving upstream is a value this host would otherwise silently stop offering a place for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SettingKind {
    /// A `{var}` in a **service's** `base_url` — `subdomain`, `shop`, `zone_id`, `account_host`.
    Endpoint,
    /// The **non-secret** user half of a `basic` credential, named by the credential it joins —
    /// `zendesk.api_token`, `jira.api_token`.
    ///
    /// Not a secret and not stored as one: it is an account name or an email address, and the token
    /// it is joined with lives in the credential store where it belongs. Zendesk's `/token` suffix
    /// is the connector's own declared data and is appended by the pack, so what a tenant supplies
    /// here is the plain account identifier and there is no join for it to get wrong.
    Username,
}

impl SettingKind {
    /// The word this kind is spelled by, in a `binds` target and in the store.
    ///
    /// The same two words `connector-pack` keys its own port on, so that a `binds` target read out
    /// of one of its refusals is a `binds` target this host accepts.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Endpoint => "endpoint",
            Self::Username => "username",
        }
    }
}

/// **One value a connector asks a tenant for**, at the address it is asked for it.
///
/// Owned rather than borrowed — unlike [`DeclaredCredential`](crate::DeclaredCredential), which is a
/// view of `&'static` catalogue data — because the endpoint variables are derived by parsing an
/// operation's Flux rather than read out of a table, so there is no `&'static str` to borrow.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct DeclaredSetting {
    /// The service this value belongs to — `delivery`, `management`, or the reserved `default` for
    /// a connector with a single API surface.
    ///
    /// **Part of the address, not a hint.** A service owns its own `base_url`, so a `{var}` in one
    /// belongs to that service, and two services of one connector may spell the same one.
    /// `contentful` is the shipped case: `delivery` and `management` both bind `endpoint.space_id`,
    /// and collapsing them sent a management write into whichever space the delivery service had
    /// been given — a `200` from a real server rather than a refusal.
    pub service: String,
    /// Which kind of value it is.
    pub kind: SettingKind,
    /// The name, without its kind: `subdomain`, or `zendesk.api_token`.
    pub name: String,
}

impl DeclaredSetting {
    /// How this setting is spelled in a `binds` target and in a refusal — `endpoint.subdomain`.
    ///
    /// The vocabulary `connector-pack`'s own refusal uses, so an operator who reads *"needs
    /// `endpoint.subdomain`"* out of a failed invocation can supply exactly that string without
    /// translating it.
    pub fn binds(&self) -> String {
        format!("{}.{}", self.kind.as_str(), self.name)
    }

    /// This setting as the pack's own [`Field`], borrowing this value's name.
    pub fn field(&self) -> Field<'_> {
        match self.kind {
            SettingKind::Endpoint => Field::Endpoint(&self.name),
            SettingKind::Username => Field::Username(&self.name),
        }
    }

    /// A setting from a service and a `binds` target, or `None` when the target is not one.
    ///
    /// The inverse of [`binds`](Self::binds), for a surface whose caller names a field. It parses
    /// the *shape* only — whether the connector declares the result is [`declared_settings`]'
    /// question, and the store refuses on it.
    pub fn parse(service: &str, binds: &str) -> Option<Self> {
        let (kind, name) = binds.split_once('.')?;
        if name.is_empty() {
            return None;
        }

        let kind = match kind {
            "endpoint" => SettingKind::Endpoint,
            "username" => SettingKind::Username,
            _ => return None,
        };

        Some(Self {
            service: service.to_owned(),
            kind,
            name: name.to_owned(),
        })
    }
}

/// **Every value `provider` asks a tenant for**, in stable order.
///
/// Read off each operation's own compiled Flux through `connector_pack::Rehearsal`, which is the
/// derivation the pack makes when it projects an operation for real — so what this reports is what
/// that projection will require, rather than a second guess that could disagree with it. See this
/// module's documentation for the measured reason a `base_url` scan is not an alternative.
///
/// Two kinds are collected, under the service of the operation that asks for them:
///
/// - every endpoint variable the operation's Flux carries;
/// - the non-secret user half of every `basic` credential the **connector** declares, which the
///   pack requires under each service that authenticates with it.
///
/// The answer is deduplicated and sorted, so a connector's surface is stable across calls: an
/// enumeration that reordered between two requests would make a UI flicker and a diff meaningless.
///
/// # Errors
///
/// [`SettingsRefusal::Unreadable`] when one of the connector's operations cannot be rehearsed. The
/// whole connector is refused rather than the readable part reported, because a partial answer is
/// one that tells an operator they have supplied everything and is wrong.
pub fn declared_settings(
    provider: &'static connector_catalog::Provider,
) -> Result<Vec<DeclaredSetting>, SettingsRefusal> {
    let mut found = BTreeSet::new();

    for entry in provider.operations {
        let rehearsal =
            connector_pack::Rehearsal::of(entry.id, provider.id, entry.service, entry.flux)
                .map_err(|error| SettingsRefusal::Unreadable {
                    connector: provider.id.to_owned(),
                    operation: entry.id.to_owned(),
                    reason: error.to_string(),
                })?;

        for variable in rehearsal.endpoint_variables() {
            found.insert(DeclaredSetting {
                service: entry.service.to_owned(),
                kind: SettingKind::Endpoint,
                name: variable.clone(),
            });
        }

        // The user half of a Basic credential is declared at *connector* level, and the pack asks
        // for it under the service of whichever operation is being projected — so a connector with
        // two services that both authenticate this way needs it supplied under each. Following the
        // pack rather than the credential address, which elides the service, is what keeps a value
        // supplied here visible to the operation that reads it.
        for credential in provider.auth {
            if matches!(
                credential.acquire,
                connector_catalog::Acquisition::BasicJoin { .. }
            ) {
                found.insert(DeclaredSetting {
                    service: entry.service.to_owned(),
                    kind: SettingKind::Username,
                    name: credential.name.to_owned(),
                });
            }
        }
    }

    Ok(found.into_iter().collect())
}

/// **The write side of a tenant's connection settings**, as the port a composition binds.
///
/// A supertrait of [`ConfigStore`] rather than a type beside it, because they are two halves of one
/// store and separating them would let a composition bind a *reader* to the invoker and a *writer*
/// to the surface — two stores that could disagree about what a tenant configured, which is the
/// failure `ConfigStore::get`'s stability requirement exists to prevent, arriving through
/// composition instead of through mutation.
///
/// The read side is upstream's and is keyed by `(tenant, provider, service, field)`. The write side
/// is this host's and takes a [`Tenant`] rather than a `&str`, so a tenant that could traverse
/// cannot be spelled at all on the side that creates entries.
///
/// # This port is a place to put values, never a place to decide about them
///
/// An implementation refuses an undeclared address and a value past a bound, and refuses nothing
/// else. Whether a value may be *substituted into a request* is `connector-pack`'s decision at the
/// one substitution point it makes it — see this module's documentation.
pub trait ConnectionSettings: ConfigStore {
    /// Put `value` at `declared`'s address for `tenant`'s connection to `connector`.
    ///
    /// Replaces whatever was there. There is deliberately no create-versus-replace distinction of
    /// the kind the credential surface draws: a setting is not a secret, so replacing one silently
    /// destroys nothing an operator cannot look up again from the vendor, and the `409` that
    /// distinction buys on the credential side would be cost with no purchase.
    ///
    /// # Errors
    ///
    /// [`SettingsRefusal`], every variant of which refuses and none of which repairs.
    fn set(
        &self,
        tenant: &Tenant,
        connector: &str,
        declared: &DeclaredSetting,
        value: &str,
    ) -> Result<(), SettingsRefusal>;

    /// Remove whatever is at `declared`'s address for `tenant`'s connection to `connector`.
    ///
    /// Answers whether anything was there, so a surface can tell "removed" from "there was nothing
    /// to remove" rather than reporting both as a success.
    ///
    /// # Errors
    ///
    /// [`SettingsRefusal::Unwritable`] when the store could not be updated. An address the connector
    /// does not declare is **not** an error here: nothing can be stored at one, so nothing is there
    /// to remove, and refusing would leave a value stranded if a connector ever stopped declaring a
    /// field it once did.
    fn clear(
        &self,
        tenant: &Tenant,
        connector: &str,
        declared: &DeclaredSetting,
    ) -> Result<bool, SettingsRefusal>;

    /// Whether this tenant has supplied `declared` for `connector`.
    ///
    /// The question a surface asks, and the only one it asks: this port hands back *whether* a value
    /// is set and never the value. See [`SettingsStore`] for why the value does not come back out.
    fn is_set(&self, tenant: &Tenant, connector: &str, declared: &DeclaredSetting) -> bool;

    /// How many bytes this tenant occupies across the whole settings store.
    ///
    /// The input to [`MAX_TENANT_SETTINGS_BYTES`], measured as lengths and never as values — there
    /// is nothing in the answer a later `debug!` could turn into a disclosure.
    fn held_bytes(&self, tenant: &Tenant) -> usize;
}

/// Why a connection setting was refused. Every variant refuses; none repairs, and none repeats a
/// value.
///
/// The two kinds `ConnectionRefusal`(crate::ConnectionRefusal) already draws, for the same reason
/// and answered by one mapping: *there is no address for this* — the connector declares no such
/// service or no such field — and *the address is fine and what would go in it is not*, because it
/// is past a bound. [`Unreadable`](Self::Unreadable) and [`Unwritable`](Self::Unwritable) are the
/// third kind, and are about this host rather than about the request.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SettingsRefusal {
    /// The connector asks for nothing per connection, so there is nothing to supply.
    #[error(
        "connector `{connector}` asks for no per-connection value, so there is nothing to \
         configure for it. Its base URL is literal and it declares no credential with a non-secret \
         user half — that is the connector's own declaration and not a gap here"
    )]
    NothingDeclared {
        /// The connector that asks for nothing.
        connector: String,
    },

    /// The connector declares no such service.
    #[error(
        "connector `{connector}` has no service `{service}`; it has {}. A service owns its own \
         base URL, so a value stored under one the connector does not have sits where no operation \
         reads it",
        crate::settings::quoted(services)
    )]
    UndeclaredService {
        /// The connector that was named.
        connector: String,
        /// The service that is not one of its own.
        service: String,
        /// The services it does have.
        services: Vec<String>,
    },

    /// The connector's service asks for no such value.
    #[error(
        "connector `{connector}` service `{service}` asks for no `{setting}`; it asks for {}. A \
         value stored under a name nothing binds would sit at an address no operation reads",
        crate::settings::quoted(declared)
    )]
    UndeclaredSetting {
        /// The connector that was named.
        connector: String,
        /// The service it was named under.
        service: String,
        /// The `binds` target that is not declared.
        setting: String,
        /// What that service does ask for.
        declared: Vec<String>,
    },

    /// A supplied value is larger than a connection setting is.
    ///
    /// Names the setting and the bound. The size is quoted because it is what makes the refusal
    /// actionable and it is the caller's own number; the value itself is not, and there is
    /// deliberately no field here one could occupy.
    #[error(
        "connector `{connector}` setting `{setting}` was sent as {bytes} bytes, and one connection \
         setting may be at most {limit}. A setting is a hostname or a vendor id, not a document — \
         nothing was stored, and this refusal does not repeat what was sent"
    )]
    SettingTooLarge {
        /// The connector that was named.
        connector: String,
        /// The `binds` target whose value is past the bound.
        setting: String,
        /// How many bytes were sent.
        bytes: usize,
        /// [`MAX_SETTING_VALUE_BYTES`], so a refusal carries the bound rather than implying it.
        limit: usize,
    },

    /// This tenant already occupies as much of the settings store as one tenant may.
    ///
    /// Both numbers are this tenant's own — no other tenant's occupancy is disclosed, or consulted.
    /// The remedy named is about *settings*, and deliberately not about credentials: this allowance
    /// is not the credential one and is never summed with it.
    #[error(
        "this tenant occupies {held} bytes of the connection-settings store and this request would \
         add {adding}, past the {limit} bytes one tenant may hold. Every write rewrites the whole \
         store, so one tenant's size is every other tenant's write latency — remove a setting for a \
         connector you no longer use before supplying another"
    )]
    TenantAllowanceExhausted {
        /// What this tenant already occupies, across every connector.
        held: usize,
        /// What this request would add.
        adding: usize,
        /// [`MAX_TENANT_SETTINGS_BYTES`].
        limit: usize,
    },

    /// One of the connector's operations could not be read, so what it needs cannot be stated.
    ///
    /// A defect in the connector or in this host's reading of it, not in the request — and a
    /// distinct variant because the remedy is: nothing the operator supplies makes this connector
    /// configurable.
    #[error(
        "connector `{connector}` cannot say what it needs configured: operation `{operation}` \
         could not be read ({reason}). Nothing can be supplied for it until that is fixed, and \
         guessing at the value would put a setting where no operation reads it"
    )]
    Unreadable {
        /// The connector whose surface could not be derived.
        connector: String,
        /// The operation that could not be rehearsed.
        operation: String,
        /// What the reader said.
        reason: String,
    },

    /// The store could not be written.
    ///
    /// Carries the path so an operator knows which file to look at, and never the value that was
    /// being written.
    #[error("the connection-settings store at `{path}` could not be written: {reason}")]
    Unwritable {
        /// The store's path, as resolved.
        path: String,
        /// What the filesystem said.
        reason: String,
    },
}

/// Render a list of names for a refusal, or say plainly that there are none.
///
/// The same helper `crate::connections` keeps for its own refusals, and a second copy rather than a
/// shared one on purpose: these are two vocabularies that happen to render alike today, and joining
/// them would make a change to one refusal's phrasing a change to the other's.
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

/// Refuse a write that would take this tenant past [`MAX_TENANT_SETTINGS_BYTES`].
///
/// `held` is what the tenant already occupies across the whole settings store and `adding` is what
/// the request would put there. Inclusive at the bound: a tenant sitting exactly on its allowance
/// has not exceeded it.
///
/// The settings twin of [`admit_tenant_occupancy`](crate::admit_tenant_occupancy), and deliberately
/// **not** that function with a different constant passed in: the two bound different stores for
/// different reasons and quote different remedies, and one function taking a limit is how a refusal
/// about one store comes to give advice about the other.
///
/// # Errors
///
/// [`SettingsRefusal::TenantAllowanceExhausted`], naming both numbers and the bound, never a value.
pub fn admit_tenant_settings(held: usize, adding: usize) -> Result<(), SettingsRefusal> {
    if held.saturating_add(adding) > MAX_TENANT_SETTINGS_BYTES {
        return Err(SettingsRefusal::TenantAllowanceExhausted {
            held,
            adding,
            limit: MAX_TENANT_SETTINGS_BYTES,
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------------------------
// The file binding
// ---------------------------------------------------------------------------------------------

#[cfg(unix)]
pub use file::{SettingsStore, SettingsStoreError, CONNECTION_SETTINGS_SETTING};

/// The file-backed binding of [`ConnectionSettings`].
///
/// `#[cfg(unix)]` for [`crate::credentials`]' reason and with one difference stated plainly: the
/// modes there are *what protects a credential*, and here they are ordinary hygiene for a
/// customer's data. A platform without them would get a settings store that is merely readable by
/// other local users, which is a smaller loss than a credential store would be — but the port above
/// is not gated, so a composition elsewhere binds its own rather than getting a weaker one silently.
#[cfg(unix)]
mod file {
    use std::collections::BTreeMap;
    use std::fs;
    use std::io::Write as _;
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
    use std::path::{Path, PathBuf};
    use std::sync::RwLock;

    use connector_pack::{ConfigStore, Field};

    use super::{
        admit_tenant_settings, ConnectionSettings, DeclaredSetting, SettingsRefusal,
        MAX_SETTING_VALUE_BYTES,
    };
    use crate::paths::{enclosing_working_tree, resolve};
    use crate::Tenant;

    /// The setting a composing binary reads the settings store path from.
    ///
    /// The host does not read the environment itself — a binary passes the value it found to
    /// [`SettingsStore::bind_configured`]. The *name* lives here so the refusal below and the reader
    /// that produced the value cannot drift apart into two different spellings.
    pub const CONNECTION_SETTINGS_SETTING: &str = "FLUX_EXCHANGE_SETTINGS";

    /// A location that would have worked, quoted in every refusal.
    ///
    /// Written with `$HOME` rather than expanded: nothing here reads the environment, and a refusal
    /// that quoted this machine's home directory would be a refusal that had already made a choice.
    const EXAMPLE_PATH: &str = "$HOME/.local/share/flux-exchange/settings";

    /// One tenant's connections, one connector's services, one service's values.
    ///
    /// Nested rather than flat because the file is meant to be read by a person: an operator
    /// looking at why a connector will not resolve should see their tenant, their connector and the
    /// `binds` targets under it, not a list of rendered keys. It is also structurally unlike the
    /// credential store's layout, which is the point — the two files should not be mistakable for
    /// each other.
    type Values = BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>>;

    /// **A tenant's non-secret connection settings, in one file.**
    ///
    /// Held in memory and written through on every change. The in-memory half is not a cache and is
    /// not optional: [`ConfigStore::get`] is **synchronous and infallible by signature** — upstream
    /// has no other option, because `Tool::permission_subjects` returns a `Vec` and can neither fail
    /// nor await — so a store that read the disk on every lookup would have nowhere to put an IO
    /// error. The file is the durable record; the map is what answers.
    ///
    /// # What a value here is protected by, stated plainly
    ///
    /// A file mode, and nothing else — and unlike the credential store, that is not a claim this
    /// store needs to make. Nothing here is a secret: a subdomain is in the URL of every request the
    /// connector makes. The mode is ordinary hygiene for a customer's data, not a security boundary,
    /// and this store would still be doing its job on a platform that could not set one.
    ///
    /// # Values go in and do not come back out
    ///
    /// There is deliberately no method here that hands a stored value to a caller. [`is_set`] is the
    /// question the surface asks. That is stricter than the argument requires — a subdomain is not a
    /// secret — and it is the direction that cannot be wrong: a `username` is an account name or an
    /// email address, which is a customer's personal data whatever the field is called, and adding a
    /// read later is additive where removing one is not.
    ///
    /// [`is_set`]: ConnectionSettings::is_set
    #[derive(Debug)]
    pub struct SettingsStore {
        path: PathBuf,
        values: RwLock<Values>,
    }

    impl SettingsStore {
        /// Bind the store a configuration value names, or refuse.
        ///
        /// `configured` is what the operator set — `None` when nothing was set at all. Surrounding
        /// whitespace is trimmed and a value that is then empty counts as unset, because
        /// `FLUX_EXCHANGE_SETTINGS=` in an environment file is an operator who has not chosen a
        /// path, not one who has chosen the current directory.
        ///
        /// # Errors
        ///
        /// [`SettingsStoreError::Unconfigured`] when nothing was named, and otherwise whatever
        /// [`bind`](Self::bind) refuses with.
        pub fn bind_configured(configured: Option<&str>) -> Result<Self, SettingsStoreError> {
            match configured.map(str::trim).filter(|value| !value.is_empty()) {
                Some(path) => Self::bind(path),
                None => Err(SettingsStoreError::Unconfigured {
                    setting: CONNECTION_SETTINGS_SETTING,
                }),
            }
        }

        /// Open — or create — the settings file at `path`, or refuse.
        ///
        /// The path is resolved against the current directory and through any symlink that already
        /// exists, so what is checked below is the location the store will really occupy rather than
        /// the spelling it was given — [`crate::paths`] is the same walk the credential store makes,
        /// shared rather than copied.
        ///
        /// # Errors
        ///
        /// [`SettingsStoreError::Unconfigured`] for an empty path, which is not a location;
        /// [`SettingsStoreError::InsideWorkingTree`] for a path under a checkout;
        /// [`SettingsStoreError::Unresolvable`] when the path cannot be made absolute at all; and
        /// [`SettingsStoreError::Unusable`] when the file cannot be created or parsed.
        pub fn bind(path: impl AsRef<Path>) -> Result<Self, SettingsStoreError> {
            let requested = path.as_ref();
            if requested.as_os_str().is_empty() {
                return Err(SettingsStoreError::Unconfigured {
                    setting: CONNECTION_SETTINGS_SETTING,
                });
            }

            let resolved =
                resolve(requested).map_err(|error| SettingsStoreError::Unresolvable {
                    path: requested.display().to_string(),
                    reason: error.to_string(),
                })?;

            // Checked before anything is created, so a refused path is one nothing was written at.
            // A subdomain is not a credential, but a tenant's list of vendor accounts committed to
            // a repository is still a leak, and the rule is the credential store's rather than a
            // second, laxer one — see `crate::paths`.
            if let Some(root) = enclosing_working_tree(&resolved) {
                return Err(SettingsStoreError::InsideWorkingTree {
                    path: resolved.display().to_string(),
                    root: root.display().to_string(),
                });
            }

            // The directory, at startup rather than at the first write. A store bound over a path
            // this process cannot create is a mistake with no later moment at which it announces
            // itself: the file itself is created lazily — there is nothing to write until a tenant
            // supplies something — so without this, a bad path surfaces as a `503` on somebody's
            // first `PUT` instead of as a refusal to start.
            let directory = resolved
                .parent()
                .ok_or_else(|| SettingsStoreError::Unusable {
                    path: resolved.display().to_string(),
                    reason: "the store path has no parent directory".to_owned(),
                })?;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|error| SettingsStoreError::Unusable {
                    path: resolved.display().to_string(),
                    reason: error.to_string(),
                })?;

            let values = read(&resolved)?;

            Ok(Self {
                path: resolved,
                values: RwLock::new(values),
            })
        }

        /// The file this store is kept in.
        ///
        /// Read back off the bound store, not remembered from the configuration.
        pub fn path(&self) -> &Path {
            &self.path
        }

        /// The line a binary prints at startup, naming the store it is actually holding.
        ///
        /// It says what this file is *for* as well as where it is, because an operator who reads
        /// only `settings: /var/lib/…` beside `credentials: /var/lib/…` will reasonably assume the
        /// two hold the same kind of thing — and the whole design here is that they do not.
        pub fn banner(&self) -> String {
            format!(
                "connection settings: {} (file store, mode 0600, non-secret values only)",
                self.path.display()
            )
        }

        /// The address a value lives at, as the nested key path this store renders.
        ///
        /// **The one place a settings address is composed**, and therefore the seam X-14 extends:
        /// an instance level lands between the connector and the service, exactly where it lands in
        /// the credential address, and no other call site re-spells the key.
        fn at(tenant: &str, connector: &str, service: &str, binds: &str) -> [String; 4] {
            [
                tenant.to_owned(),
                connector.to_owned(),
                service.to_owned(),
                binds.to_owned(),
            ]
        }

        /// Persist the current map, or say why it could not be.
        ///
        /// Whole-file, through a sibling temporary and a `rename(2)`, so a reader never sees a
        /// half-written store and an interrupted write leaves the previous one intact. The same
        /// shape `connector_secrets::FileStore` uses, and for the same reason: this file is small
        /// and rewriting it is cheaper than being able to corrupt it.
        fn persist(&self, values: &Values) -> Result<(), SettingsRefusal> {
            let unwritable = |reason: String| SettingsRefusal::Unwritable {
                path: self.path.display().to_string(),
                reason,
            };

            let directory = self
                .path
                .parent()
                .ok_or_else(|| unwritable("the store path has no parent directory".to_owned()))?;
            fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(directory)
                .map_err(|error| unwritable(error.to_string()))?;

            let encoded =
                serde_json::to_vec_pretty(values).map_err(|error| unwritable(error.to_string()))?;

            let temporary = self.path.with_extension("tmp");
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temporary)
                .map_err(|error| unwritable(error.to_string()))?;
            file.write_all(&encoded)
                .map_err(|error| unwritable(error.to_string()))?;
            file.sync_all()
                .map_err(|error| unwritable(error.to_string()))?;
            drop(file);

            fs::rename(&temporary, &self.path).map_err(|error| unwritable(error.to_string()))
        }

        /// Refuse a write this connector does not ask for, and one past the per-value bound.
        ///
        /// Both before anything is written and both in one place, so `set` has no ordering to get
        /// wrong: the connector's declared surface decides whether there is an address at all, and
        /// the bound decides whether what is going in it is a setting.
        fn admit(
            connector: &str,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            let provider =
                connector_catalog::provider(connector_catalog::ProviderKey::id(connector))
                    .ok_or_else(|| SettingsRefusal::NothingDeclared {
                        connector: connector.to_owned(),
                    })?;

            let surface = super::declared_settings(provider)?;
            if surface.is_empty() {
                return Err(SettingsRefusal::NothingDeclared {
                    connector: connector.to_owned(),
                });
            }

            let mut services: Vec<String> = surface
                .iter()
                .map(|setting| setting.service.clone())
                .collect();
            services.dedup();

            if !services.contains(&declared.service) {
                return Err(SettingsRefusal::UndeclaredService {
                    connector: connector.to_owned(),
                    service: declared.service.clone(),
                    services,
                });
            }

            if !surface.contains(declared) {
                return Err(SettingsRefusal::UndeclaredSetting {
                    connector: connector.to_owned(),
                    service: declared.service.clone(),
                    setting: declared.binds(),
                    declared: surface
                        .iter()
                        .filter(|setting| setting.service == declared.service)
                        .map(DeclaredSetting::binds)
                        .collect(),
                });
            }

            if value.len() > MAX_SETTING_VALUE_BYTES {
                return Err(SettingsRefusal::SettingTooLarge {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    bytes: value.len(),
                    limit: MAX_SETTING_VALUE_BYTES,
                });
            }

            Ok(())
        }
    }

    /// Read the store at `path`, or start from an empty one if it is not there yet.
    ///
    /// A file that is *there* and unreadable is a refusal: a store this process could not parse is
    /// one whose values are about to be silently absent, which reads to an operator as sixteen
    /// connectors that stopped working for no reason. **Refuse; never repair** — there is no arm
    /// here that starts empty because parsing failed.
    fn read(path: &Path) -> Result<Values, SettingsStoreError> {
        let raw = match fs::read(path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Values::new()),
            Err(error) => {
                return Err(SettingsStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })
            }
        };

        if raw.is_empty() {
            return Ok(Values::new());
        }

        serde_json::from_slice(&raw).map_err(|error| SettingsStoreError::Unusable {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    }

    impl ConfigStore for SettingsStore {
        /// The value bound to `field` of `provider`'s `service`, for `tenant`.
        ///
        /// Upstream's signature, and its tenant is a `&str` because the pack composes no path from
        /// it. That is safe here for the same reason: this store renders no filesystem path from a
        /// tenant either — the whole map is one file, and the tenant is a key inside it.
        ///
        /// `None` means "this tenant has not configured it", and the pack turns that into a refusal
        /// naming the field rather than a request to a host with a brace in it.
        fn get(
            &self,
            tenant: &str,
            provider: &str,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            let binds = binds_of(field);
            self.values
                .read()
                .ok()?
                .get(tenant)?
                .get(provider)?
                .get(service)?
                .get(&binds)
                .cloned()
        }
    }

    /// The `binds` target one of the pack's [`Field`]s is spelled by.
    ///
    /// Matched exhaustively with no wildcard arm, deliberately: `Field` is not `#[non_exhaustive]`
    /// precisely so that a host must decide about every kind of value the pack can ask for, and a
    /// new one should be a compile error here rather than a `None` that reads as "not configured".
    fn binds_of(field: Field<'_>) -> String {
        match field {
            Field::Endpoint(name) => format!("endpoint.{name}"),
            Field::Username(name) => format!("username.{name}"),
        }
    }

    impl ConnectionSettings for SettingsStore {
        fn set(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            SettingsStore::admit(connector, declared, value)?;

            let mut values = self
                .values
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;

            // The allowance is decided against what this write *replaces*, not against the whole
            // new value on top of an occupancy that already includes the old one — the same reading
            // the credential surface takes for a rotation, and for the same reason: counting both
            // would refuse a one-byte change to a tenant sitting near its bound.
            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                connector,
                &declared.service,
                &declared.binds(),
            );
            let replacing = values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b))
                .map_or(0, String::len);
            let held = occupied(&values, tenant.as_str());
            admit_tenant_settings(held.saturating_sub(replacing), value.len())?;

            let previous = values
                .entry(t)
                .or_default()
                .entry(c)
                .or_default()
                .entry(s)
                .or_default()
                .insert(b.clone(), value.to_owned());

            // Persisted under the same lock the map was changed under, and rolled back if the file
            // will not take it: a store whose memory and file disagree would answer the invoker
            // with a value that vanishes at the next restart, which is the failure this whole
            // module's durability argument is about.
            if let Err(refusal) = self.persist(&values) {
                let settings = values
                    .get_mut(tenant.as_str())
                    .and_then(|connectors| connectors.get_mut(connector))
                    .and_then(|services| services.get_mut(&declared.service));
                if let Some(settings) = settings {
                    match previous {
                        Some(previous) => settings.insert(b, previous),
                        None => settings.remove(&b),
                    };
                }
                return Err(refusal);
            }

            Ok(())
        }

        fn clear(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            let mut values = self
                .values
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;

            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                connector,
                &declared.service,
                &declared.binds(),
            );
            let Some(previous) = values
                .get_mut(&t)
                .and_then(|connectors| connectors.get_mut(&c))
                .and_then(|services| services.get_mut(&s))
                .and_then(|settings| settings.remove(&b))
            else {
                return Ok(false);
            };

            if let Err(refusal) = self.persist(&values) {
                values
                    .entry(t)
                    .or_default()
                    .entry(c)
                    .or_default()
                    .entry(s)
                    .or_default()
                    .insert(b, previous);
                return Err(refusal);
            }

            Ok(true)
        }

        fn is_set(&self, tenant: &Tenant, connector: &str, declared: &DeclaredSetting) -> bool {
            self.get(
                tenant.as_str(),
                connector,
                &declared.service,
                declared.field(),
            )
            .is_some()
        }

        fn held_bytes(&self, tenant: &Tenant) -> usize {
            self.values
                .read()
                .map_or(0, |values| occupied(&values, tenant.as_str()))
        }
    }

    /// How many bytes one tenant occupies, as lengths and never as values.
    ///
    /// Nothing in this function binds a stored value to a name, so there is nothing here a later
    /// `debug!` could turn into a disclosure — the same care [`crate::stored_bytes`] takes on the
    /// credential side, where it matters more and is therefore worth being consistent about.
    fn occupied(values: &Values, tenant: &str) -> usize {
        values.get(tenant).map_or(0, |connectors| {
            connectors
                .values()
                .flat_map(|services| services.values())
                .flat_map(|settings| settings.values())
                .map(String::len)
                .sum()
        })
    }

    /// Why a settings store could not be bound. Every variant refuses; none falls back.
    #[derive(Debug, thiserror::Error)]
    pub enum SettingsStoreError {
        /// No store was named, and there is no default worth choosing on an operator's behalf.
        #[error(
            "no connection-settings store is configured: set `{setting}` to a path outside every \
             working tree, for example `{EXAMPLE_PATH}`. This host does not fall back to an \
             in-memory store — one would accept every value and lose it on restart, which reads as \
             sixteen connectors that stopped resolving for no reason"
        )]
        Unconfigured {
            /// The setting that would have named one.
            setting: &'static str,
        },

        /// The configured path is inside a working tree.
        #[error(
            "refusing a connection-settings store at `{path}`: it is inside the working tree at \
             `{root}`, one `git add -A` from a committed list of a tenant's vendor accounts. Put it \
             outside a checkout, for example `{EXAMPLE_PATH}`"
        )]
        InsideWorkingTree {
            /// The store path, as resolved.
            path: String,
            /// The root of the working tree it falls under.
            root: String,
        },

        /// The path could not be made absolute — an unreadable current directory, most likely.
        #[error(
            "refusing a connection-settings store at `{path}`: it cannot be resolved: {reason}"
        )]
        Unresolvable {
            /// The path as configured.
            path: String,
            /// What the filesystem said.
            reason: String,
        },

        /// The store exists and this host cannot read it.
        ///
        /// **Not** started empty. A store that could not be parsed and was replaced by an empty one
        /// would take every tenant's configuration with it, silently, at the moment an operator is
        /// least looking.
        #[error(
            "the connection-settings store at `{path}` cannot be read: {reason}. Nothing here \
             starts from an empty store on a parse failure — that would discard every tenant's \
             configuration and report it as a clean start"
        )]
        Unusable {
            /// The store path, as resolved.
            path: String,
            /// What the reader said.
            reason: String,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two spellings of one setting agree in both directions, or a surface that takes a `binds`
    /// target and a store that renders one are addressing two different things.
    #[test]
    fn a_binds_target_round_trips() {
        for (service, binds) in [
            ("default", "endpoint.subdomain"),
            ("management", "endpoint.space_id"),
            ("default", "username.zendesk.api_token"),
        ] {
            let parsed = DeclaredSetting::parse(service, binds)
                .unwrap_or_else(|| panic!("`{binds}` is a well-formed binds target"));
            assert_eq!(parsed.binds(), binds);
            assert_eq!(parsed.service, service);
        }
    }

    /// A `binds` target this host does not know is not guessed at.
    ///
    /// `credential.zendesk.api_token` is the case worth having: it is a real row of the design's
    /// `binds` table and it is a **secret**, which belongs in the credential store and must not
    /// become storable here by being parsed into an unknown kind.
    #[test]
    fn a_binds_target_that_is_not_one_of_the_two_kinds_is_refused() {
        for hostile in [
            "credential.zendesk.api_token",
            "oauth.client_secret",
            "subdomain",
            "endpoint.",
            "",
        ] {
            assert!(
                DeclaredSetting::parse("default", hostile).is_none(),
                "`{hostile}` must not parse into a setting this store would accept",
            );
        }
    }

    /// The tenant allowance is inclusive at the bound and refuses with both of the tenant's own
    /// numbers, and its advice is about **settings** rather than about credentials.
    #[test]
    fn the_tenant_settings_allowance_is_inclusive_and_names_its_own_remedy() {
        assert!(admit_tenant_settings(0, MAX_TENANT_SETTINGS_BYTES).is_ok());
        assert!(admit_tenant_settings(MAX_TENANT_SETTINGS_BYTES, 0).is_ok());

        let refusal = admit_tenant_settings(MAX_TENANT_SETTINGS_BYTES, 1)
            .expect_err("one byte past the allowance is past it");

        assert_eq!(
            refusal,
            SettingsRefusal::TenantAllowanceExhausted {
                held: MAX_TENANT_SETTINGS_BYTES,
                adding: 1,
                limit: MAX_TENANT_SETTINGS_BYTES,
            },
        );

        let message = refusal.to_string();
        assert!(message.contains("remove a setting"), "{message}");
        assert!(
            !message.contains("disconnect a connector"),
            "a settings refusal must not give advice about the credential store: {message}",
        );
    }

    /// The two stores' bounds are different numbers, and that is the decision rather than an
    /// oversight.
    ///
    /// Asserted so that a later simplification which "tidies up" by pointing one constant at the
    /// other has to argue with a test — the whole of this module's placement argument is that a
    /// setting and a credential are different kinds of value with different bounds.
    #[test]
    fn a_setting_is_not_bounded_like_a_credential() {
        // Compile-time, because these are constants and a runtime comparison of two `const`s is a
        // test that runs after the thing it is checking has already been compiled in. The failure
        // it guards against is an edit, and an edit should not build.
        const { assert!(MAX_SETTING_VALUE_BYTES != crate::MAX_CREDENTIAL_VALUE_BYTES) };
        const { assert!(MAX_TENANT_SETTINGS_BYTES != crate::MAX_TENANT_STORE_BYTES) };
        // A hostname is smaller than a PEM-encoded key, and the bound should say so.
        const { assert!(MAX_SETTING_VALUE_BYTES < crate::MAX_CREDENTIAL_VALUE_BYTES) };
    }
}
