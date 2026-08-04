//! **A tenant's non-secret connection settings** — the `{subdomain}` in a templated base URL.
//!
//! X-12 made this host execute and exposed the gap immediately: **seventeen** of the fifty-three
//! shipped connectors declare a per-connection value their operations substitute into a request, and
//! there was nowhere for a tenant to put one. The invoker bound an empty `MemoryConfig`, so those
//! connectors refused by name — the right failure, and still a surface that ran thirty-six of
//! fifty-three.
//!
//! **Thirteen of the seventeen were made configurable. Four were refused on purpose** — see
//! [`HostPinning`] — so the surface X-47 shipped was forty-nine of fifty-three, and the four that
//! were left said so rather than looking broken.
//!
//! Re-measured against catalogue 0.13 (X-85): **fifty-four connectors, nineteen of which declare a
//! per-connection value, and three of those are refused** — `docusign`, `freshdesk` and `okta`,
//! whose host is a bare placeholder with nothing declared to pick from. `intercom` and `newrelic`
//! template their whole authority too and are *not* refused, because the catalogue publishes the
//! closed set of region hostnames each of them permits: see [`HostPinning::ChosenFrom`]. Fifty-one
//! of fifty-four.
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
//! measurement rather than by argument. It finds twelve of the seventeen and misses five: the
//! endpoint variables of `bitbucket`, `cloudflare`, `contentful` and `vercel` live somewhere in the
//! operation's Flux other than the base URL, and `twilio` needs only the non-secret user half of a
//! Basic credential, which no URL scan could find at all. A host enumerating the surface that way
//! would tell an operator they had supplied everything and then refuse the call.
//!
//! # Two questions about a value, and only one of them is the pack's
//!
//! **The characters** of a substituted value are `connector-pack`'s business, and this module does
//! not second-guess them. The pack holds the composed authority to an allow-list of host characters
//! at the one substitution point it makes, so `acme.zendesk.com@evil.example` is refused rather
//! than turned into a request to `evil.example.zendesk.com`; it does that knowing which of a URL's
//! positions the value lands in, which this crate does not. A second opinion here would be a second
//! spelling of one rule, and the one that disagreed would be the one deciding what may travel.
//!
//! **The identity of the host** is not a question the pack can answer, and X-47's first cut assumed
//! it was. A character allow-list constrains what a value *looks like*; it says nothing about where
//! the request goes. Where the connector's template pins a suffix — `{subdomain}.zendesk.com` — the
//! two coincide, because any admissible value composes an authority inside the vendor's domain.
//! Where the template **is** the variable — okta's and freshdesk's `{domain}`, docusign's
//! `{account_host}` — they come apart completely: `evil.example` is a perfectly valid hostname, the
//! character check admits it, and the request goes wherever the caller said, carrying that tenant's
//! credential.
//!
//! That is `AGENTS.md`'s *"an agent's token grants access to an operation, never to a credential"*
//! broken through a configuration field, and it is this module's question rather than the pack's
//! because the fact it turns on — the connector's own host template — is catalogue data this module
//! reads and the pack does not expose. [`host_pinning`] is the answer; [`ConnectionSettings::set`]
//! refuses on it, and [`ConfigStore::get`] refuses on it again so the property belongs to the port
//! rather than to one write path.
//!
//! **A closed set the catalogue publishes is a third case, and it is still catalogue data** (X-70).
//! `intercom` and `newrelic` template their whole authority *and* declare, per field, the vendor
//! hostnames that field permits — three regions and two. A tenant picking one of those is choosing
//! a region, not naming a destination, so the value is admitted; a tenant offering anything else is
//! refused, by equality against a set nothing in this repository wrote down. That is deliberately
//! not a value rule of the kind [`HostPinning`] argues against: the admitted set is published by
//! the same source the host templates are, and what stays refused is admitting a value because it
//! *looks* fine.
//!
//! What this module also refuses is a value at an address the connector never declared, for
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

use connector_address::InstanceId;
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
/// every configurable connector in the compiled-in catalogue supplied — thirteen connectors,
/// twenty-odd values, at the tens of bytes a subdomain actually occupies — sits three orders of
/// magnitude under it.
///
/// **This allowance is separate from the credential one and is not summed with it.** A tenant that
/// has filled its credential allowance can still supply a subdomain, and a tenant that has filled
/// this one is told to remove a *setting* rather than to disconnect a connector. Collapsing the two
/// would make one refusal give advice about the other store, which is the whole of why the
/// configuration lives here in the first place — see this module's documentation.
pub const MAX_TENANT_SETTINGS_BYTES: usize = 16 * 1024;

/// Policy seam for catalogue-declared custom origins.
///
/// The production binding derives this only from released typed declarations. Tests use an
/// explicit fixture policy to exercise policy-change refusals without turning a connector id or
/// field name into another production policy source.
trait CustomOriginPolicy: Send + Sync {
    /// The released rule for this exact declaration, including its grammar and normalization.
    fn rule(&self, connector: &str, declared: &DeclaredSetting) -> Option<&dyn CustomOriginRule>;
}

/// One typed declaration's authority grammar.
///
/// The production adapter comes from released connector metadata. Keeping both outputs explicit
/// avoids reconstructing the reviewed origin from the setting bytes a connector consumes.
trait CustomOriginRule: Send + Sync {
    /// Validate and normalize a submitted origin for storage and operator inspection.
    fn normalize(&self, value: &str) -> Result<NormalizedOrigin, OriginPolicyRefusal>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NormalizedOrigin {
    setting_value: String,
    origin: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OriginPolicyRefusal {
    UnsupportedScheme,
    Malformed,
}

/// Value-free lifecycle state for one custom-origin setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthorityState {
    /// No value is stored.
    Unset,
    /// A value awaits explicit review.
    Proposed,
    /// The exact proposal revision is admitted to the runtime.
    Approved,
    /// The proposal remains stored but is not admitted to the runtime.
    Revoked,
}

/// Authority status returned to operator-only inspection surfaces.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthorityStatus {
    /// Current lifecycle state.
    pub state: AuthorityState,
    /// Durable proposal revision, absent only while unset.
    pub revision: Option<u64>,
    /// Exact normalized origin, present only for a stored proposal.
    ///
    /// Ordinary connection-plan projections intentionally omit this operator-only value.
    pub origin: Option<String>,
}

impl AuthorityStatus {
    fn unset() -> Self {
        Self {
            state: AuthorityState::Unset,
            revision: None,
            origin: None,
        }
    }
}

/// Opaque, single-use proposal prepared before its audit record begins.
///
/// It intentionally has no `Debug`, `Clone`, serialization or value accessor: the submitted origin
/// remains inside the settings port between read-only validation and the exact checked commit.
pub struct PreparedAuthorityProposal {
    store_path: std::path::PathBuf,
    tenant: Tenant,
    connector: String,
    instance: Option<InstanceId>,
    declared: DeclaredSetting,
    submitted: String,
    normalized: NormalizedOrigin,
    expected_revision: Option<u64>,
    revision: u64,
}

impl PreparedAuthorityProposal {
    /// Store-wide revision an audit begin record may name before durable mutation.
    pub const fn revision(&self) -> u64 {
        self.revision
    }
}

// The two bounds answer different questions, and the second is only worth having while it is the
// tighter one. Asserted at compile time rather than in a test, for
// `crate::connections`' reason: a tenant allowance below one whole value refuses every real
// connection, and one so far above it that no tenant reaches it bounds nothing.
const _: () = assert!(MAX_TENANT_SETTINGS_BYTES > MAX_SETTING_VALUE_BYTES);
const _: () = assert!(MAX_TENANT_SETTINGS_BYTES < 32 * MAX_SETTING_VALUE_BYTES);

/// **Which kind of non-secret value** a connector asks a tenant for.
///
/// The kinds `connector-pack`'s [`Field`] distinguishes, as an owned type this crate can put in
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
    /// A non-secret value placed in one generated channel's WebSocket query string. `name` on the
    /// surrounding [`DeclaredSetting`] is `<binding>.query.<parameter>`, preserving the pack's
    /// declaration address without inventing a second channel configuration vocabulary.
    ChannelQuery,
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
            Self::ChannelQuery => "channel",
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
            SettingKind::ChannelQuery => {
                let (channel, parameter) = self.name.split_once(".query.").unwrap_or(("", ""));
                Field::ChannelQuery { channel, parameter }
            }
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
            "channel"
                if name
                    .split_once(".query.")
                    .is_some_and(|(channel, parameter)| {
                        !channel.is_empty() && !parameter.is_empty()
                    }) =>
            {
                SettingKind::ChannelQuery
            }
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
/// Three kinds are collected, under the service that asks for them:
///
/// - every endpoint variable the operation's Flux carries;
/// - the non-secret user half of every `basic` credential the **connector** declares, which the
///   pack requires under each service that authenticates with it.
/// - non-secret generated-channel query values published by the connector catalogue.
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

        // The management surface offers every provider-declared Basic user half under every
        // service, because another operation on that service may select that auth mechanism. The
        // operation-granular projection below narrows this to the mechanism one operation names.
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

    for field in provider.config.iter().filter(|field| !field.secret) {
        if let Some(declared) = DeclaredSetting::parse(field.service, field.binds) {
            found.insert(declared);
        }
    }

    Ok(found.into_iter().collect())
}

/// Every non-secret setting one operation needs before it can be projected for invocation.
///
/// This is the operation-granular half of [`declared_settings`]. Effective discovery uses it so a
/// connector with one configured service does not advertise an operation from an unconfigured
/// sibling service. Generated channel query settings are deliberately absent: they configure a
/// persistent channel, not a one-shot operation.
///
/// # Errors
///
/// [`SettingsRefusal::Unreadable`] when the operation's emitted Flux cannot be rehearsed.
pub fn operation_settings(
    provider: &'static connector_catalog::Provider,
    entry: &'static connector_catalog::Operation,
) -> Result<Vec<DeclaredSetting>, SettingsRefusal> {
    let rehearsal = connector_pack::Rehearsal::of(entry.id, provider.id, entry.service, entry.flux)
        .map_err(|error| SettingsRefusal::Unreadable {
            connector: provider.id.to_owned(),
            operation: entry.id.to_owned(),
            reason: error.to_string(),
        })?;
    let mut found = BTreeSet::new();

    for variable in rehearsal.endpoint_variables() {
        found.insert(DeclaredSetting {
            service: entry.service.to_owned(),
            kind: SettingKind::Endpoint,
            name: variable.clone(),
        });
    }

    // The user half of Basic auth is needed only when this operation names that mechanism. Keeping
    // the operation's credential declaration in the predicate prevents an unrelated Basic option
    // on the provider from hiding a bearer-authenticated operation.
    for credential in provider.auth {
        let used = entry
            .credentials
            .iter()
            .flat_map(|mechanism| mechanism.iter())
            .any(|name| *name == credential.name);
        if used
            && matches!(
                credential.acquire,
                connector_catalog::Acquisition::BasicJoin { .. }
            )
        {
            found.insert(DeclaredSetting {
                service: entry.service.to_owned(),
                kind: SettingKind::Username,
                name: credential.name.to_owned(),
            });
        }
    }

    Ok(found.into_iter().collect())
}

/// **Whether a tenant's value can change which host a request reaches.**
///
/// The distinction this whole module's safety rests on, and the one the first cut of X-47 did not
/// draw. `connector-pack` validates a substituted value against an allow-list of **host
/// characters**, so no permitted character can delimit and the composed authority is exactly the
/// string the template produced. That constrains the *characters* of a value. It says nothing about
/// the *identity* of the host, and the difference between those two is the difference between these
/// variants:
///
/// ```text
/// zendesk    hosts: ["{subdomain}.zendesk.com"]   -> PinnedTo(".zendesk.com")
/// okta       hosts: ["{domain}"]                  -> WholeAuthority("{domain}")
/// bitbucket  hosts: ["api.bitbucket.org"]         -> OutsideTheAuthority
/// intercom   hosts: ["{host}"], three choices     -> ChosenFrom([three vendor hostnames])
/// ```
///
/// For zendesk the composed authority always ends in `.zendesk.com`, whatever a tenant supplies, so
/// the origin cannot leave the vendor and a tenant naming its own subdomain is exactly the intended
/// use. For okta the value **is** the authority: `evil.example` is a perfectly valid hostname, the
/// character check admits it, and the request — carrying that tenant's `okta.api_token` — goes
/// wherever the caller said. Three shipped connectors are in that state, and two more would be if
/// the catalogue did not publish the closed set of hostnames they permit.
///
/// **This distinction is about the vendor and not about the account**, and it is the whole of what
/// this function decides. A pinned suffix keeps the origin at zendesk; it does not keep it at *this
/// tenant's* zendesk, because a vendor subdomain is something anybody can register. Who may write a
/// value at a pinned address is therefore a second question, answered by
/// `routes::connections::MAY_CONFIGURE` rather than here — there is no principal in scope at this
/// call, and deliberately so: this is a question about `&'static` catalogue data, decided once per
/// variable and the same answer for every caller.
///
/// # Why the rule is about the declaration and never about the value
///
/// A rule that inspected values would be a blocklist of hosts, and a blocklist only ever catches
/// what somebody enumerated — the same argument `tests/no_second_request_path.rs` makes for its
/// dependency allow-list. This asks a question about the **connector's own declaration**, which is
/// `&'static` catalogue data a request cannot reach, so it is decided once per variable and is the
/// same answer for every caller.
///
/// [`ChosenFrom`](Self::ChosenFrom) is the one answer that then looks at a value, and it does not
/// weaken that: **the admitted set is itself declared catalogue data**, published by the same source
/// the host templates are read from, so admitting a value because the catalogue lists it as one of a
/// closed set is still deciding from the catalogue. What stays refused is admitting a value because
/// it *looks* fine — see [`admits`](Self::admits), which compares for equality against a set no
/// tenant can influence and does nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostPinning {
    /// The variable appears in no host template, so it lands in a path or a query and cannot reach
    /// the authority at all. `connector-pack` holds it to that position's own rule.
    OutsideTheAuthority,
    /// Every host template carrying this variable ends in this literal suffix, so the composed
    /// authority is always inside the vendor's own domain.
    ///
    /// **That is true and it is not a safety argument.** `*.zendesk.com`, `*.atlassian.net`,
    /// `*.myshopify.com`, `*.supabase.co` and `*.my.salesforce.com` are self-service registrable
    /// namespaces: a suffix pin constrains **which vendor** a request reaches, never **whose
    /// account** at that vendor. What it buys is a bound rather than a boundary — the value cannot
    /// become an arbitrary origin — and what keeps a caller from choosing an account inside that
    /// bound is a different rule in a different place: only a `User` may write a setting at all
    /// (`routes::connections::MAY_CONFIGURE`). See `docs/designs/connection-settings.md` § 4.
    ///
    /// The suffix is what is quoted in a listing, so an operator can see *why* a value is accepted.
    PinnedTo(String),
    /// At least one host template carrying this variable pins no suffix, so the value **is** the
    /// destination authority. Carries the offending template, for a refusal that shows its working.
    WholeAuthority(String),
    /// **The catalogue publishes a closed set of values for this field**, so what a tenant supplies
    /// is a choice out of the vendor's own list rather than a string they compose (X-70, upstream
    /// C-225). Carries the declared values, in the vendor's order, for a refusal that says what
    /// would have worked.
    ///
    /// Intercom is the shipped case: its `base_url` is `https://{host}`, which reads as
    /// [`WholeAuthority`](Self::WholeAuthority) from the template alone — and its
    /// `config_choices` declare `{host}` to be one of `api.intercom.io`, `api.eu.intercom.io` and
    /// `api.au.intercom.io`. A caller picking among three hostnames the **vendor** published is
    /// choosing a region, not naming a destination, and `evil.example` is not on the list.
    ///
    /// **This is not the value rule X-47 refused to write.** The set is a second piece of declared
    /// catalogue data from the same source the host templates come from, and it is not something
    /// this repository enumerated — a blocklist catches only what somebody wrote down, and there is
    /// nothing written down here. [`admits`](Self::admits) compares a value against it for
    /// **equality**: not a prefix, not a suffix, not case-insensitively, because
    /// `api.eu.intercom.io.evil.example` contains a declared choice and resolves wherever its
    /// registrant says.
    ///
    /// A closed set is a **stronger** constraint than any template pin, so it is the answer
    /// wherever the catalogue publishes one — including over a template that pins a suffix. Erring
    /// that way is the direction that cannot be wrong: upstream documents these as *"the permitted
    /// values"*, and a host that offered a value it was not offered would be widening a set the
    /// vendor closed.
    ChosenFrom(Vec<String>),
}

impl HostPinning {
    /// Whether a tenant may supply a value **at this address at all**.
    ///
    /// Not whether a particular value may be supplied — that is [`admits`](Self::admits), and the
    /// two differ for exactly one answer: a [`ChosenFrom`](Self::ChosenFrom) address accepts a
    /// value, and accepts only the declared ones. A caller that decides about a *value* must ask
    /// `admits`; this one answers the question a listing asks, which is whether there is anything
    /// to offer here.
    ///
    /// Matched exhaustively with no wildcard arm, deliberately: a variant added here is a new
    /// answer to "can a caller name the host", and it must be a compile error at the one place that
    /// decides rather than something that silently falls to `true`.
    pub fn tenant_may_supply(&self) -> bool {
        match self {
            Self::OutsideTheAuthority | Self::PinnedTo(_) | Self::ChosenFrom(_) => true,
            Self::WholeAuthority(_) => false,
        }
    }

    /// **Whether this exact value may be supplied here**, which is the question both enforcement
    /// points ask.
    ///
    /// For three of the four answers it is [`tenant_may_supply`](Self::tenant_may_supply): the
    /// value is irrelevant, because the decision was made about the template. For
    /// [`ChosenFrom`](Self::ChosenFrom) it is set membership by **byte equality** — the value has
    /// to be one the catalogue published, with nothing trimmed, nothing case-folded and no prefix
    /// or suffix admitted. `API.EU.INTERCOM.IO` resolves the same as its lower-case spelling and is
    /// still refused: a comparison that normalises is a comparison somebody has to get right, and
    /// the set was published to be compared against literally.
    ///
    /// Matched exhaustively for [`tenant_may_supply`](Self::tenant_may_supply)'s reason.
    pub fn admits(&self, value: &str) -> bool {
        match self {
            Self::OutsideTheAuthority | Self::PinnedTo(_) => true,
            Self::WholeAuthority(_) => false,
            Self::ChosenFrom(choices) => choices.iter().any(|choice| choice == value),
        }
    }
}

/// **Whether `declared` can move the origin of `provider`'s requests** — read from the catalogue.
///
/// `connector_catalog::Operation::hosts` publishes each operation's host templates with their
/// templating intact, which is what makes this decidable without a new dependency and without
/// reaching into `connector-pack` (whose `Slot` is `pub(crate)` and not available).
///
/// Only [`SettingKind::Endpoint`] can reach an authority. Username and channel-query settings are
/// placed in a header or query string, never in the authority, so both are always
/// [`OutsideTheAuthority`](HostPinning::OutsideTheAuthority).
///
/// # A declared closed set is asked about first
///
/// `connector_catalog::Provider::choices_for` publishes, per `(service, kind, name)`, the values a
/// field permits — upstream C-225, and the same catalogue this function already reads its host
/// templates out of. Where there is one, it is the answer: a closed set bounds the value more
/// tightly than any suffix pin does, so consulting it before the template errs closed rather than
/// open, and a set that is **empty or absent** changes nothing at all — the template decides, and
/// an unpinned one is still [`WholeAuthority`](HostPinning::WholeAuthority).
///
/// # What counts as pinned
///
/// The text after the **last** placeholder must be a literal starting with `.` and carrying at
/// least two further labels — `.zendesk.com`, `.my.salesforce.com`. Two labels rather than one
/// because `.com` pins nothing anybody cannot register under, and the honest name for the thing
/// wanted here is a public-suffix list, which is a dependency this crate may not take. The
/// approximation is stated rather than hidden, and it errs closed: a template it cannot read as
/// pinned is refused.
pub fn host_pinning(
    provider: &'static connector_catalog::Provider,
    declared: &DeclaredSetting,
) -> HostPinning {
    if declared.kind != SettingKind::Endpoint {
        return HostPinning::OutsideTheAuthority;
    }

    // The vendor's own closed set, addressed exactly as this port addresses a stored value. An
    // entry with no choices in it is not a closed set — it declares nothing to admit from — and
    // falls through to the template below, so a connector that publishes an empty set is refused
    // exactly as one that publishes none is.
    if let Some(published) =
        provider.choices_for(&declared.service, declared.kind.as_str(), &declared.name)
    {
        if !published.choices.is_empty() {
            return HostPinning::ChosenFrom(
                published
                    .choices
                    .iter()
                    .map(|choice| choice.value.to_owned())
                    .collect(),
            );
        }
    }

    let mut pinned: Option<String> = None;

    for entry in provider.operations {
        // A setting belongs to one service, and only that service's operations read it — the key
        // carries the service for exactly this reason (C-197).
        if entry.service != declared.service {
            continue;
        }

        for host in entry.hosts {
            if !mentions(host, &declared.name) {
                continue;
            }

            match suffix_of(host) {
                // One unpinned template is enough. A variable pinned in five operations and bare in
                // a sixth is a variable a caller can name the host with, by choosing the sixth.
                None => return HostPinning::WholeAuthority((*host).to_owned()),
                Some(suffix) => pinned = Some(suffix),
            }
        }
    }

    pinned.map_or(HostPinning::OutsideTheAuthority, HostPinning::PinnedTo)
}

/// Whether `template` carries `{variable}` as one of its placeholders.
///
/// Matched as the whole braced name rather than as a substring, so `{host}` and `{account_host}`
/// are two variables and not one containing the other.
fn mentions(template: &str, variable: &str) -> bool {
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else {
            return false;
        };
        if &after[..close] == variable {
            return true;
        }
        rest = &after[close + 1..];
    }
    false
}

/// The literal suffix `template` pins its authority to, if it pins one.
///
/// `None` for a template whose last placeholder is at the end — the value is the authority — and for
/// one whose trailing literal is too short to pin anything. See [`host_pinning`] for why two labels.
fn suffix_of(template: &str) -> Option<String> {
    let (_, suffix) = template.rsplit_once('}')?;

    if !suffix.starts_with('.') || suffix.contains('{') {
        return None;
    }

    let labels = suffix
        .trim_start_matches('.')
        .split('.')
        .filter(|label| !label.is_empty())
        .count();

    (labels >= 2).then(|| suffix.to_owned())
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

    /// Put one value for a selected connection UUID.
    fn set_for_instance(
        &self,
        tenant: &Tenant,
        connector: &str,
        instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
        value: &str,
    ) -> Result<(), SettingsRefusal> {
        match instance {
            None => self.set(tenant, connector, declared, value),
            Some(instance) => Err(SettingsRefusal::InstanceUnsupported {
                connector: connector.to_owned(),
                instance: instance.to_string(),
            }),
        }
    }

    /// Clear one value for a selected connection UUID.
    fn clear_for_instance(
        &self,
        tenant: &Tenant,
        connector: &str,
        instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
    ) -> Result<bool, SettingsRefusal> {
        match instance {
            None => self.clear(tenant, connector, declared),
            Some(instance) => Err(SettingsRefusal::InstanceUnsupported {
                connector: connector.to_owned(),
                instance: instance.to_string(),
            }),
        }
    }

    /// Whether one selected connection has this value.
    fn is_set_for_instance(
        &self,
        tenant: &Tenant,
        connector: &str,
        instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
    ) -> bool {
        match instance {
            None => self.is_set(tenant, connector, declared),
            Some(_) => false,
        }
    }

    /// Whether a released typed policy marks this exact declaration as a custom origin.
    fn is_custom_origin(&self, _connector: &str, _declared: &DeclaredSetting) -> bool {
        false
    }

    /// Read the value-free authority lifecycle for one selected connection.
    fn authority_status_for_instance(
        &self,
        _tenant: &Tenant,
        _connector: &str,
        _instance: Option<&InstanceId>,
        _declared: &DeclaredSetting,
    ) -> Result<AuthorityStatus, SettingsRefusal> {
        Ok(AuthorityStatus::unset())
    }

    /// Create an initial custom-origin proposal or replace one exact current revision.
    ///
    /// `None` is create-only. `Some(revision)` is replacement-only and compare-and-swaps that
    /// exact durable revision, so a client can never turn an inspection followed by a concurrent
    /// proposal into a blind overwrite.
    fn propose_authority_for_instance(
        &self,
        tenant: &Tenant,
        connector: &str,
        instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
        value: &str,
        expected_revision: Option<u64>,
    ) -> Result<AuthorityStatus, SettingsRefusal> {
        let prepared = self.prepare_authority_proposal_for_instance(
            tenant,
            connector,
            instance,
            declared,
            value,
            expected_revision,
        )?;
        self.commit_authority_proposal_for_instance(prepared)
    }

    /// Validate one proposal and reserve no state while exposing its candidate revision to audit.
    fn prepare_authority_proposal_for_instance(
        &self,
        _tenant: &Tenant,
        connector: &str,
        _instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
        _value: &str,
        _expected_revision: Option<u64>,
    ) -> Result<PreparedAuthorityProposal, SettingsRefusal> {
        Err(SettingsRefusal::AuthorityUnsupported {
            connector: connector.to_owned(),
            setting: declared.binds(),
        })
    }

    /// Commit exactly one prepared proposal, checking address and allocation revisions together.
    fn commit_authority_proposal_for_instance(
        &self,
        prepared: PreparedAuthorityProposal,
    ) -> Result<AuthorityStatus, SettingsRefusal> {
        Err(SettingsRefusal::AuthorityUnsupported {
            connector: prepared.connector,
            setting: prepared.declared.binds(),
        })
    }

    /// Approve one exact proposal revision.
    fn approve_authority_for_instance(
        &self,
        _tenant: &Tenant,
        connector: &str,
        _instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
        _revision: u64,
    ) -> Result<AuthorityStatus, SettingsRefusal> {
        Err(SettingsRefusal::AuthorityUnsupported {
            connector: connector.to_owned(),
            setting: declared.binds(),
        })
    }

    /// Revoke one exact proposal revision.
    fn revoke_authority_for_instance(
        &self,
        _tenant: &Tenant,
        connector: &str,
        _instance: Option<&InstanceId>,
        declared: &DeclaredSetting,
        _revision: u64,
    ) -> Result<AuthorityStatus, SettingsRefusal> {
        Err(SettingsRefusal::AuthorityUnsupported {
            connector: connector.to_owned(),
            setting: declared.binds(),
        })
    }

    /// Move the sole legacy settings namespace under its newly minted UUID.
    fn qualify_instance(
        &self,
        _tenant: &Tenant,
        connector: &str,
        instance: &InstanceId,
    ) -> Result<(), SettingsRefusal> {
        Err(SettingsRefusal::InstanceUnsupported {
            connector: connector.to_owned(),
            instance: instance.to_string(),
        })
    }

    /// Delete one instance's settings and move the survivor back to the legacy namespace.
    fn collapse_instances(
        &self,
        _tenant: &Tenant,
        connector: &str,
        _removed: &InstanceId,
        remaining: &InstanceId,
    ) -> Result<(), SettingsRefusal> {
        Err(SettingsRefusal::InstanceUnsupported {
            connector: connector.to_owned(),
            instance: remaining.to_string(),
        })
    }

    /// Delete one instance's settings while several others remain qualified.
    fn discard_instance(
        &self,
        _tenant: &Tenant,
        connector: &str,
        instance: &InstanceId,
    ) -> Result<(), SettingsRefusal> {
        Err(SettingsRefusal::InstanceUnsupported {
            connector: connector.to_owned(),
            instance: instance.to_string(),
        })
    }
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
    /// The field has no released typed custom-origin policy.
    #[error(
        "connector `{connector}` setting `{setting}` has no released custom-origin authority policy"
    )]
    AuthorityUnsupported {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
    },
    /// A create-only proposal found an authority record and needs an explicit CAS revision.
    #[error(
        "connector `{connector}` setting `{setting}` already has authority revision {current}; replacement requires that exact expected revision"
    )]
    AuthorityRevisionRequired {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
        /// Current durable proposal revision.
        current: u64,
    },
    /// No proposal exists at the derived setting address.
    #[error("connector `{connector}` setting `{setting}` has no authority proposal to transition")]
    AuthorityUnset {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
    },
    /// Optimistic authority transition named a stale proposal.
    #[error(
        "connector `{connector}` setting `{setting}` authority revision conflict: expected {expected}, current {current}"
    )]
    AuthorityRevisionConflict {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
        /// Revision supplied by the operator.
        expected: u64,
        /// Current durable proposal revision.
        current: u64,
    },
    /// The matching revision is not in a state admitted by the requested transition.
    #[error(
        "connector `{connector}` setting `{setting}` authority revision {revision} is {current:?} and cannot be {transition}"
    )]
    AuthorityStateConflict {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
        /// Current durable proposal revision.
        revision: u64,
        /// Current value-free lifecycle state.
        current: AuthorityState,
        /// Value-free transition verb.
        transition: &'static str,
    },
    /// The typed declaration does not admit the submitted origin scheme.
    #[error(
        "connector `{connector}` setting `{setting}` does not admit the submitted origin scheme; nothing was stored"
    )]
    OriginSchemeUnsupported {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
    },
    /// The submitted origin does not satisfy the typed declaration's grammar.
    #[error(
        "connector `{connector}` setting `{setting}` is not a well-formed origin under its typed policy; nothing was stored"
    )]
    MalformedOrigin {
        /// Connector named by the route.
        connector: String,
        /// Declared binds target.
        setting: String,
    },
    /// The bound settings port has not implemented instance-aware addressing.
    #[error(
        "connector `{connector}` instance `{instance}` needs instance-aware connection settings, but the bound settings store does not provide them"
    )]
    InstanceUnsupported {
        /// The connector being configured.
        connector: String,
        /// The host-minted UUID.
        instance: String,
    },
    /// The settings namespaces could not make the same instance transition as the credentials.
    #[error("connector `{connector}` settings cannot change instance layout: {reason}")]
    InstanceTransition {
        /// The connector being migrated.
        connector: String,
        /// The conflicting or unwritable state, containing no value.
        reason: String,
    },
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

    /// **The value would be the destination host, so no tenant may supply it.**
    ///
    /// Not about the value offered — `acme.okta.com` is refused exactly as `evil.example` is —
    /// because the defect is in the *template*: it pins no suffix, so whatever is substituted is
    /// the whole authority and a caller who can write here can name the host this process sends a
    /// tenant's credential to. `AGENTS.md`: an agent's token grants access to an operation, never
    /// to a credential.
    ///
    /// Raised only where the catalogue declares **no** closed set for the field. Where it declares
    /// one, the value is not free and [`NotADeclaredChoice`](Self::NotADeclaredChoice) is the
    /// refusal a wrong value gets instead.
    ///
    /// The consequence is stated plainly rather than softened: this connector cannot be configured
    /// by a tenant on this host, and it stays uninvocable. A smaller working surface beats a larger
    /// one that leaks. An operator who genuinely wants it binds their own [`ConfigStore`] in a
    /// composition they control, which is a decision made once at startup by somebody who can read
    /// this paragraph — not one a request can make.
    #[error(
        "connector `{connector}` cannot be configured by a tenant: its `{setting}` is the whole \
         destination host. Its own declaration templates the host as `{template}`, which pins no \
         vendor suffix — so whatever is supplied *is* the origin this host would send \
         `{connector}`'s credential to, and no value is safe rather than some values being unsafe. \
         Nothing was stored. A deployment that needs this connector binds its connection settings \
         in its own composition, where the choice is an operator's and not a caller's"
    )]
    WouldNameTheHost {
        /// The connector that cannot be configured.
        connector: String,
        /// The `binds` target that would have been the authority.
        setting: String,
        /// The connector's own host template, quoted so the refusal shows its working.
        template: String,
    },

    /// **The value is not one of the ones the catalogue declares for this field.**
    ///
    /// The refusal that comes with [`HostPinning::ChosenFrom`]: the connector publishes a closed
    /// set for this setting, and what was offered is not in it. Comparison is by equality, so this
    /// is also what refuses a value that merely *contains* a declared one —
    /// `api.eu.intercom.io.evil.example` is a hostname somebody else registered.
    ///
    /// The declared choices are quoted because they are the catalogue's own published data and are
    /// the whole of what makes the refusal actionable; the value that was offered is not, and there
    /// is deliberately no field here one could occupy.
    #[error(
        "connector `{connector}` setting `{setting}` takes one of the values its own catalogue \
         entry declares: {}. Nothing was stored, and this refusal does not repeat what was sent — \
         the value is matched exactly, so a hostname that merely extends one of these is a \
         different host",
        crate::settings::quoted(choices)
    )]
    NotADeclaredChoice {
        /// The connector that was named.
        connector: String,
        /// The `binds` target whose value is a closed set.
        setting: String,
        /// The values the catalogue declares for it, in the vendor's own order.
        choices: Vec<String>,
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

pub use file::{SettingsStore, SettingsStoreError};

/// The file-backed binding of [`ConnectionSettings`].
///
/// The same portable owner-only filesystem boundary as [`crate::credentials`] protects this
/// customer configuration even though the values themselves are not credentials.
mod file {
    use std::collections::BTreeMap;
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, RwLock};

    use connector_pack::{ConfigStore, ConfigValue, Configuration, Field, Rehearsal};
    use serde::{Deserialize, Serialize};

    use super::{
        admit_tenant_settings, AuthorityState, AuthorityStatus, ConnectionSettings,
        CustomOriginPolicy, CustomOriginRule, DeclaredSetting, InstanceId, NormalizedOrigin,
        OriginPolicyRefusal, PreparedAuthorityProposal, SettingsRefusal, MAX_SETTING_VALUE_BYTES,
    };
    use crate::paths::{enclosing_working_tree, resolve};
    use crate::{private_fs, Tenant, CONNECTION_SETTINGS_SETTING};

    /// The setting a composing binary reads the settings store path from.
    ///
    /// The host does not read the environment itself — a binary passes the value it found to
    /// [`SettingsStore::bind_configured`]. The *name* lives here so the refusal below and the reader
    /// that produced the value cannot drift apart into two different spellings.
    /// A location that would have worked, quoted in every refusal.
    ///
    /// Written with `$HOME` rather than expanded: nothing here reads the environment, and a refusal
    /// that quoted this machine's home directory would be a refusal that had already made a choice.
    const EXAMPLE_PATH: &str = "$HOME/.local/share/flux-exchange/settings";

    /// Released operator-approved origin declarations, validated by the pack that executes them.
    struct CatalogueCustomOriginPolicy {
        rules: BTreeMap<(String, DeclaredSetting), CatalogueOriginRule>,
    }

    struct CatalogueOriginRule {
        provider: String,
        service: String,
        field: String,
        rehearsal: Rehearsal,
    }

    impl CatalogueCustomOriginPolicy {
        fn read() -> Result<Self, String> {
            let mut rules = BTreeMap::new();
            for provider in connector_catalog::providers() {
                for field in provider.config {
                    match field.approval {
                        connector_catalog::Approval::None => continue,
                        connector_catalog::Approval::Operator => {}
                    }
                    if field.secret || field.format != "origin" || !field.also_binds.is_empty() {
                        return Err(format!(
                            "connector `{}` field `{}` declares an unsupported operator-approval shape",
                            provider.id, field.name
                        ));
                    }
                    let declared =
                        DeclaredSetting::parse(field.service, field.binds).ok_or_else(|| {
                            format!(
                                "connector `{}` field `{}` has an unreadable binds target",
                                provider.id, field.name
                            )
                        })?;
                    if declared.kind != super::SettingKind::Endpoint || declared.name != field.name
                    {
                        return Err(format!(
                            "connector `{}` field `{}` does not bind its own endpoint name",
                            provider.id, field.name
                        ));
                    }
                    let verify = provider.verify.ok_or_else(|| {
                        format!(
                            "connector `{}` declares an operator-approved origin without a verification operation",
                            provider.id
                        )
                    })?;
                    let operation = provider
                        .operations
                        .iter()
                        .find(|operation| operation.id == verify && operation.service == field.service)
                        .ok_or_else(|| {
                            format!(
                                "connector `{}` verification operation does not belong to origin service `{}`",
                                provider.id, field.service
                            )
                        })?;
                    let rehearsal = Rehearsal::of(
                        operation.id,
                        provider.id,
                        operation.service,
                        operation.flux,
                    )
                    .map_err(|error| {
                        format!(
                            "connector `{}` verification operation cannot validate its origin: {error}",
                            provider.id
                        )
                    })?;
                    if !rehearsal
                        .endpoint_variables()
                        .iter()
                        .any(|variable| variable == field.name)
                    {
                        return Err(format!(
                            "connector `{}` verification operation does not consume origin field `{}`",
                            provider.id, field.name
                        ));
                    }
                    rules.insert(
                        (provider.id.to_owned(), declared),
                        CatalogueOriginRule {
                            provider: provider.id.to_owned(),
                            service: field.service.to_owned(),
                            field: field.name.to_owned(),
                            rehearsal,
                        },
                    );
                }
            }
            Ok(Self { rules })
        }
    }

    impl CustomOriginPolicy for CatalogueCustomOriginPolicy {
        fn rule(
            &self,
            connector: &str,
            declared: &DeclaredSetting,
        ) -> Option<&dyn CustomOriginRule> {
            self.rules
                .get(&(connector.to_owned(), declared.clone()))
                .map(|rule| rule as &dyn CustomOriginRule)
        }
    }

    struct CandidateOrigin {
        provider: String,
        service: String,
        field: String,
        value: String,
    }

    impl ConfigStore for CandidateOrigin {
        fn get(
            &self,
            tenant: &str,
            provider: &str,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.resolve_for_instance(tenant, provider, None, service, field)
                .map(|resolved| resolved.value().to_owned())
        }

        fn resolve_for_instance(
            &self,
            _tenant: &str,
            provider: &str,
            _instance: Option<&InstanceId>,
            service: &str,
            field: Field<'_>,
        ) -> Option<ConfigValue> {
            (provider == self.provider
                && service == self.service
                && matches!(field, Field::Endpoint(name) if name == self.field))
            .then(|| ConfigValue::operator_approved(self.value.clone()))
        }
    }

    impl CustomOriginRule for CatalogueOriginRule {
        fn normalize(&self, value: &str) -> Result<NormalizedOrigin, OriginPolicyRefusal> {
            let configuration = Configuration::new(
                Arc::new(CandidateOrigin {
                    provider: self.provider.clone(),
                    service: self.service.clone(),
                    field: self.field.clone(),
                    value: value.to_owned(),
                }),
                "validation",
            )
            .map_err(|_| OriginPolicyRefusal::Malformed)?;
            self.rehearsal
                .request(&configuration, &serde_json::json!({}))
                .map_err(|_| {
                    if value.starts_with("https://") {
                        OriginPolicyRefusal::Malformed
                    } else {
                        OriginPolicyRefusal::UnsupportedScheme
                    }
                })?;
            Ok(NormalizedOrigin {
                setting_value: value.to_owned(),
                origin: value.to_owned(),
            })
        }
    }

    /// One tenant's connections, one connector's services, one service's values.
    ///
    /// Nested rather than flat because the file is meant to be read by a person: an operator
    /// looking at why a connector will not resolve should see their tenant, their connector and the
    /// `binds` targets under it, not a list of rendered keys. It is also structurally unlike the
    /// credential store's layout, which is the point — the two files should not be mistakable for
    /// each other.
    type Values =
        BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, StoredValue>>>>;

    const STORE_SCHEMA: &str = "exchange.connection-settings.v2";

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Document {
        schema: String,
        next_origin_revision: u64,
        values: Values,
    }

    impl Default for Document {
        fn default() -> Self {
            Self {
                schema: STORE_SCHEMA.to_owned(),
                next_origin_revision: 1,
                values: Values::new(),
            }
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(untagged)]
    enum StoredValue {
        Ordinary(String),
        Origin(StoredOrigin),
    }

    impl StoredValue {
        fn occupied_bytes(&self) -> usize {
            match self {
                Self::Ordinary(value) => value.len(),
                Self::Origin(origin) => origin.value.len().saturating_add(origin.origin.len()),
            }
        }
    }

    #[derive(Clone, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct StoredOrigin {
        kind: OriginKind,
        value: String,
        // Empty is the deliberate missing-field marker for the pre-normalization X-125 slice.
        // An explicit null still refuses because the wire type is a string; binding resolves a
        // missing field through the typed rule or refuses, and new writes never store empty.
        #[serde(default, skip_serializing_if = "String::is_empty")]
        origin: String,
        state: StoredAuthorityState,
        revision: u64,
    }

    #[derive(Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum OriginKind {
        CustomOrigin,
    }

    #[derive(Clone, Copy, Serialize, Deserialize)]
    #[serde(rename_all = "snake_case")]
    enum StoredAuthorityState {
        Proposed,
        Approved,
        Revoked,
    }

    #[derive(Clone, Copy)]
    enum AuthorityTransition {
        Approve,
        Revoke,
    }

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
    pub struct SettingsStore {
        path: PathBuf,
        document: RwLock<Document>,
        custom_origins: Arc<dyn CustomOriginPolicy>,
    }

    impl std::fmt::Debug for SettingsStore {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter
                .debug_struct("SettingsStore")
                .field("path", &self.path)
                .finish_non_exhaustive()
        }
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
            let policy = CatalogueCustomOriginPolicy::read().map_err(|reason| {
                SettingsStoreError::Unusable {
                    path: requested.display().to_string(),
                    reason: format!("released custom-origin policy is unreadable: {reason}"),
                }
            })?;
            Self::bind_with_custom_origin_policy(path, Arc::new(policy))
        }

        /// Internal construction seam. Production derives policy from the released catalogue;
        /// unit tests can replace it to prove policy-change and persistence refusals.
        fn bind_with_custom_origin_policy(
            path: impl AsRef<Path>,
            custom_origins: Arc<dyn CustomOriginPolicy>,
        ) -> Result<Self, SettingsStoreError> {
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
            private_fs::ensure_directory(directory).map_err(|error| {
                SettingsStoreError::Unusable {
                    path: resolved.display().to_string(),
                    reason: error.to_string(),
                }
            })?;

            let mut document = read(&resolved)?;
            migrate_tagged_custom_origins(&resolved, &mut document, custom_origins.as_ref())?;
            revalidate_tagged_custom_origins(&resolved, &document, custom_origins.as_ref())?;
            refuse_untyped_custom_origins(&resolved, &document, custom_origins.as_ref())?;

            Ok(Self {
                path: resolved,
                document: RwLock::new(document),
                custom_origins,
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
                "connection settings: {} (platform owner-only file store, non-secret values only)",
                self.path.display()
            )
        }

        /// The address a value lives at, as the nested key path this store renders.
        ///
        /// **The one place a settings address is composed**, and therefore the seam X-14 extends:
        /// an instance level lands between the connector and the service, exactly where it lands in
        /// the credential address, and no other call site re-spells the key.
        fn at(
            tenant: &str,
            connector: &str,
            instance: Option<&InstanceId>,
            service: &str,
            binds: &str,
        ) -> [String; 4] {
            let connector = match instance {
                Some(instance) => format!("{connector}@{}", instance.as_str()),
                None => connector.to_owned(),
            };
            [
                tenant.to_owned(),
                connector,
                service.to_owned(),
                binds.to_owned(),
            ]
        }

        fn transition_authority(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            expected: u64,
            transition: AuthorityTransition,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let Some(_rule) = custom_origin_rule(self.custom_origins.as_ref(), connector, declared)
            else {
                return Err(SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            };
            let [t, c, s, b] = Self::at(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                &declared.binds(),
            );
            let Some(StoredValue::Origin(current)) = document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b))
            else {
                return Err(SettingsRefusal::AuthorityUnset {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            };
            let normalized = self
                .admit(connector, declared, &current.origin)?
                .ok_or_else(|| SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                })?;
            if normalized.setting_value != current.value || normalized.origin != current.origin {
                return Err(SettingsRefusal::MalformedOrigin {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            }
            if current.revision != expected {
                return Err(SettingsRefusal::AuthorityRevisionConflict {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    expected,
                    current: current.revision,
                });
            }
            let transition_admitted = match transition {
                AuthorityTransition::Approve => {
                    matches!(current.state, StoredAuthorityState::Proposed)
                }
                AuthorityTransition::Revoke => matches!(
                    current.state,
                    StoredAuthorityState::Proposed | StoredAuthorityState::Approved
                ),
            };
            if !transition_admitted {
                return Err(SettingsRefusal::AuthorityStateConflict {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    revision: current.revision,
                    current: authority_state(current.state),
                    transition: match transition {
                        AuthorityTransition::Approve => "approved",
                        AuthorityTransition::Revoke => "revoked",
                    },
                });
            }
            let desired = match transition {
                AuthorityTransition::Approve => StoredAuthorityState::Approved,
                AuthorityTransition::Revoke => StoredAuthorityState::Revoked,
            };
            let previous = document.clone();
            let current = document
                .values
                .get_mut(&t)
                .and_then(|connectors| connectors.get_mut(&c))
                .and_then(|services| services.get_mut(&s))
                .and_then(|settings| settings.get_mut(&b))
                .and_then(|stored| match stored {
                    StoredValue::Origin(origin) => Some(origin),
                    StoredValue::Ordinary(_) => None,
                })
                .expect("the origin record was resolved under the same write lock");
            current.state = desired;
            let status = authority_status(current);
            if let Err(refusal) = self.persist(&document) {
                *document = previous;
                return Err(refusal);
            }
            Ok(status)
        }

        /// Persist the current map, or say why it could not be.
        ///
        /// Whole-file, through a sibling temporary and a `rename(2)`, so a reader never sees a
        /// half-written store and an interrupted write leaves the previous one intact. The same
        /// shape `connector_secrets::FileStore` uses, and for the same reason: this file is small
        /// and rewriting it is cheaper than being able to corrupt it.
        fn persist(&self, document: &Document) -> Result<(), SettingsRefusal> {
            let unwritable = |reason: String| SettingsRefusal::Unwritable {
                path: self.path.display().to_string(),
                reason,
            };

            let encoded = serde_json::to_vec_pretty(document)
                .map_err(|error| unwritable(error.to_string()))?;
            private_fs::write_atomic(&self.path, &encoded)
                .map_err(|error| unwritable(error.to_string()))
        }

        /// Refuse a write this connector does not ask for, and one past the per-value bound.
        ///
        /// Both before anything is written and both in one place, so `set` has no ordering to get
        /// wrong: the connector's declared surface decides whether there is an address at all, and
        /// the bound decides whether what is going in it is a setting.
        fn admit(
            &self,
            connector: &str,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<Option<NormalizedOrigin>, SettingsRefusal> {
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

            // **The authority rule**, after the address is known to exist and before anything is
            // written. It sits above the size bound because "no value is acceptable here" is a
            // stronger statement than "that one is too big", and an operator should read the
            // stronger one.
            let pinning = super::host_pinning(provider, declared);

            // A value that would be the destination host is refused whatever it says, because the
            // defect is in the connector's own template rather than in the value — see
            // `HostPinning`. Raised before the membership check below so that "nobody may supply
            // this" is never reported as "you picked the wrong one from a list".
            if let super::HostPinning::WholeAuthority(template) = &pinning {
                if custom_origin_rule(self.custom_origins.as_ref(), connector, declared).is_none() {
                    return Err(SettingsRefusal::WouldNameTheHost {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                        template: template.clone(),
                    });
                }
            }

            // And the one place a *value* is decided about: it has to be one the catalogue
            // declares. Only `ChosenFrom` reaches this, and only because the set it compares
            // against is the connector's own published data (X-70).
            if let super::HostPinning::ChosenFrom(choices) = &pinning {
                if !pinning.admits(value) {
                    return Err(SettingsRefusal::NotADeclaredChoice {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                        choices: choices.clone(),
                    });
                }
            }

            if value.len() > MAX_SETTING_VALUE_BYTES {
                return Err(SettingsRefusal::SettingTooLarge {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    bytes: value.len(),
                    limit: MAX_SETTING_VALUE_BYTES,
                });
            }

            custom_origin_rule(self.custom_origins.as_ref(), connector, declared)
                .map(|rule| normalize_origin(rule, connector, declared, value))
                .transpose()
        }
    }

    /// Read the store at `path`, or start from an empty one if it is not there yet.
    ///
    /// A file that is *there* and unreadable is a refusal: a store this process could not parse is
    /// one whose values are about to be silently absent, which reads to an operator as thirteen
    /// connectors that stopped working for no reason. **Refuse; never repair** — there is no arm
    /// here that starts empty because parsing failed.
    fn read(path: &Path) -> Result<Document, SettingsStoreError> {
        let Some(raw) =
            private_fs::read(path, 1024 * 1024).map_err(|error| SettingsStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?
        else {
            return Ok(Document::default());
        };

        if raw.is_empty() {
            return Ok(Document::default());
        }

        let decoded: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|error| SettingsStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?;
        if let Some(schema) = decoded.get("schema").and_then(serde_json::Value::as_str) {
            if schema != STORE_SCHEMA {
                return Err(SettingsStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: format!(
                        "unsupported settings schema `{}`; supported schema is `{STORE_SCHEMA}`",
                        schema
                    ),
                });
            }
            let document: Document =
                serde_json::from_value(decoded).map_err(|error| SettingsStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: error.to_string(),
                })?;
            if document.next_origin_revision == 0
                || document.values.values().any(|connectors| {
                    connectors.values().any(|services| {
                        services.values().any(|settings| {
                            settings.values().any(|stored| {
                                matches!(stored, StoredValue::Origin(origin) if origin.revision == 0 || origin.revision >= document.next_origin_revision)
                            })
                        })
                    })
                })
            {
                return Err(SettingsStoreError::Unusable {
                    path: path.display().to_string(),
                    reason: "origin revisions are outside the durable store high-water mark"
                        .to_owned(),
                });
            }
            return Ok(document);
        }

        type LegacyValues =
            BTreeMap<String, BTreeMap<String, BTreeMap<String, BTreeMap<String, String>>>>;
        let legacy: LegacyValues =
            serde_json::from_value(decoded).map_err(|error| SettingsStoreError::Unusable {
                path: path.display().to_string(),
                reason: error.to_string(),
            })?;
        Ok(Document {
            values: legacy
                .into_iter()
                .map(|(tenant, connectors)| {
                    (
                        tenant,
                        connectors
                            .into_iter()
                            .map(|(connector, services)| {
                                (
                                    connector,
                                    services
                                        .into_iter()
                                        .map(|(service, settings)| {
                                            (
                                                service,
                                                settings
                                                    .into_iter()
                                                    .map(|(binds, value)| {
                                                        (binds, StoredValue::Ordinary(value))
                                                    })
                                                    .collect(),
                                            )
                                        })
                                        .collect(),
                                )
                            })
                            .collect(),
                    )
                })
                .collect(),
            ..Document::default()
        })
    }

    fn custom_origin_rule<'a>(
        policy: &'a dyn CustomOriginPolicy,
        connector: &str,
        declared: &DeclaredSetting,
    ) -> Option<&'a dyn CustomOriginRule> {
        policy.rule(connector, declared)
    }

    fn normalize_origin(
        rule: &dyn CustomOriginRule,
        connector: &str,
        declared: &DeclaredSetting,
        value: &str,
    ) -> Result<NormalizedOrigin, SettingsRefusal> {
        let normalized = rule.normalize(value).map_err(|refusal| match refusal {
            OriginPolicyRefusal::UnsupportedScheme => SettingsRefusal::OriginSchemeUnsupported {
                connector: connector.to_owned(),
                setting: declared.binds(),
            },
            OriginPolicyRefusal::Malformed => SettingsRefusal::MalformedOrigin {
                connector: connector.to_owned(),
                setting: declared.binds(),
            },
        })?;
        if normalized.setting_value.is_empty() || normalized.origin.is_empty() {
            return Err(SettingsRefusal::MalformedOrigin {
                connector: connector.to_owned(),
                setting: declared.binds(),
            });
        }
        let normalized_bytes = normalized.setting_value.len().max(normalized.origin.len());
        if normalized_bytes > MAX_SETTING_VALUE_BYTES {
            return Err(SettingsRefusal::SettingTooLarge {
                connector: connector.to_owned(),
                setting: declared.binds(),
                bytes: normalized_bytes,
                limit: MAX_SETTING_VALUE_BYTES,
            });
        }
        Ok(normalized)
    }

    fn authority_state(state: StoredAuthorityState) -> AuthorityState {
        match state {
            StoredAuthorityState::Proposed => AuthorityState::Proposed,
            StoredAuthorityState::Approved => AuthorityState::Approved,
            StoredAuthorityState::Revoked => AuthorityState::Revoked,
        }
    }

    fn authority_status(origin: &StoredOrigin) -> AuthorityStatus {
        AuthorityStatus {
            state: authority_state(origin.state),
            revision: Some(origin.revision),
            origin: Some(origin.origin.clone()),
        }
    }

    fn migrate_tagged_custom_origins(
        path: &Path,
        document: &mut Document,
        policy: &dyn CustomOriginPolicy,
    ) -> Result<(), SettingsStoreError> {
        for connectors in document.values.values_mut() {
            for (connector_key, services) in connectors {
                let connector = connector_key
                    .split_once('@')
                    .map_or(connector_key.as_str(), |(connector, _)| connector);
                for (service, settings) in services {
                    for (binds, stored) in settings {
                        let StoredValue::Origin(origin) = stored else {
                            continue;
                        };
                        if !origin.origin.is_empty() {
                            continue;
                        }
                        let Some(declared) = DeclaredSetting::parse(service, binds) else {
                            return Err(SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: "a legacy tagged custom-origin record has an unreadable setting address"
                                    .to_owned(),
                            });
                        };
                        let declared_now = connector_catalog::provider(
                            connector_catalog::ProviderKey::id(connector),
                        )
                        .and_then(|provider| super::declared_settings(provider).ok())
                        .is_some_and(|surface| surface.contains(&declared));
                        if !declared_now {
                            return Err(SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: format!(
                                    "a custom-origin record at connector `{connector}` setting `{binds}` has no current declaration"
                                ),
                            });
                        }
                        let Some(rule) = custom_origin_rule(policy, connector, &declared) else {
                            return Err(SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: format!(
                                    "a legacy tagged custom-origin record at connector `{connector}` setting `{binds}` has no current typed migration rule"
                                ),
                            });
                        };
                        let normalized = rule.normalize(&origin.value).map_err(|_| {
                            SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: format!(
                                    "a legacy tagged custom-origin record at connector `{connector}` setting `{binds}` does not satisfy the current typed migration rule"
                                ),
                            }
                        })?;
                        origin.value = normalized.setting_value;
                        origin.origin = normalized.origin;
                    }
                }
            }
        }
        Ok(())
    }

    fn revalidate_tagged_custom_origins(
        path: &Path,
        document: &Document,
        policy: &dyn CustomOriginPolicy,
    ) -> Result<(), SettingsStoreError> {
        for connectors in document.values.values() {
            for (connector_key, services) in connectors {
                let connector = connector_key
                    .split_once('@')
                    .map_or(connector_key.as_str(), |(connector, _)| connector);
                for (service, settings) in services {
                    for (binds, stored) in settings {
                        let StoredValue::Origin(origin) = stored else {
                            continue;
                        };
                        let Some(declared) = DeclaredSetting::parse(service, binds) else {
                            return Err(SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: "a tagged custom-origin record has an unreadable setting address"
                                    .to_owned(),
                            });
                        };
                        let Some(rule) = custom_origin_rule(policy, connector, &declared) else {
                            continue;
                        };
                        let valid = rule.normalize(&origin.origin).is_ok_and(|normalized| {
                            !normalized.setting_value.is_empty()
                                && !normalized.origin.is_empty()
                                && normalized.setting_value == origin.value
                                && normalized.origin == origin.origin
                                && normalized.setting_value.len().max(normalized.origin.len())
                                    <= MAX_SETTING_VALUE_BYTES
                        });
                        if !valid {
                            return Err(SettingsStoreError::Unusable {
                                path: path.display().to_string(),
                                reason: format!(
                                    "a custom-origin record at connector `{connector}` setting `{binds}` does not satisfy the current typed rule"
                                ),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn refuse_untyped_custom_origins(
        path: &Path,
        document: &Document,
        policy: &dyn CustomOriginPolicy,
    ) -> Result<(), SettingsStoreError> {
        for connectors in document.values.values() {
            for (connector_key, services) in connectors {
                let connector = connector_key
                    .split_once('@')
                    .map_or(connector_key.as_str(), |(connector, _)| connector);
                for (service, settings) in services {
                    for (binds, stored) in settings {
                        let Some(declared) = DeclaredSetting::parse(service, binds) else {
                            continue;
                        };
                        if custom_origin_rule(policy, connector, &declared).is_some()
                            && matches!(stored, StoredValue::Ordinary(_))
                        {
                            return Err(SettingsStoreError::MigrationRequired {
                                path: path.display().to_string(),
                                connector: connector.to_owned(),
                                setting: binds.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(())
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
        ///
        /// # The authority rule is applied here too, and that is not belt-and-braces
        ///
        /// [`ConnectionSettings::set`] refuses a value that would be the whole destination host, so
        /// nothing arriving through this host's own surface can reach this map. This asks the same
        /// question again on the way **out**, because `set` is not the only way bytes get into the
        /// file: an edited store, a file restored from a backup taken before this rule existed, or a
        /// value written by an older build all bypass it. Deciding it on read makes "no tenant value
        /// is the destination authority" a property of the *port* rather than of one write path.
        ///
        /// It costs a walk of the connector's `&'static` host templates per lookup, on a port that
        /// is read once per projection. That is the right side of the trade for the one rule whose
        /// failure sends a tenant's credential to a caller's server.
        ///
        /// **Since X-70 the question is asked about the value**, not only about the address: a
        /// field whose values are a closed set the catalogue declares is checked against that set
        /// here as well as in `set`, so a planted `api.eu.intercom.io.evil.example` is refused on
        /// the way out exactly as a planted `evil.example` at an unpinned address is. The read
        /// happens first and the decision second, because there is no value to decide about until
        /// there is one.
        ///
        /// The value is not deleted, and deliberately: **refuse; never repair.** A store that
        /// silently rewrote a file it found suspicious would destroy the evidence of how the value
        /// got there, on the one path where somebody has to find that out.
        fn get(
            &self,
            tenant: &str,
            provider: &str,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.get_for_instance(tenant, provider, None, service, field)
        }

        fn get_for_instance(
            &self,
            tenant: &str,
            provider: &str,
            instance: Option<&InstanceId>,
            service: &str,
            field: Field<'_>,
        ) -> Option<String> {
            self.resolve_for_instance(tenant, provider, instance, service, field)
                .map(|resolved| resolved.value().to_owned())
        }

        fn resolve_for_instance(
            &self,
            tenant: &str,
            provider: &str,
            instance: Option<&InstanceId>,
            service: &str,
            field: Field<'_>,
        ) -> Option<ConfigValue> {
            let binds = binds_of(field);
            let connector_key = match instance {
                Some(instance) => format!("{provider}@{}", instance.as_str()),
                None => provider.to_owned(),
            };

            let catalogued =
                connector_catalog::provider(connector_catalog::ProviderKey::id(provider))
                    .map(|catalogued| (catalogued, DeclaredSetting::parse(service, &binds)));

            let stored = self
                .document
                .read()
                .ok()?
                .values
                .get(tenant)?
                .get(&connector_key)?
                .get(service)?
                .get(&binds)
                .cloned()?;

            if let Some((catalogued, declared)) = catalogued {
                // A `binds` target the pack asked for that this host cannot parse is not one it
                // can decide about, and an undecided value is not one it hands over.
                let declared = declared?;
                if !super::declared_settings(catalogued)
                    .ok()?
                    .contains(&declared)
                {
                    return None;
                }
                if let Some(rule) =
                    custom_origin_rule(self.custom_origins.as_ref(), provider, &declared)
                {
                    return match stored {
                        StoredValue::Origin(origin)
                            if matches!(origin.state, StoredAuthorityState::Approved) =>
                        {
                            normalize_origin(rule, provider, &declared, &origin.origin)
                                .ok()
                                .filter(|normalized| {
                                    normalized.origin == origin.origin
                                        && normalized.setting_value == origin.value
                                })
                                .map(|normalized| {
                                    ConfigValue::operator_approved(normalized.setting_value)
                                })
                        }
                        StoredValue::Origin(_) | StoredValue::Ordinary(_) => None,
                    };
                }
                // An origin record never degrades into an ordinary executable setting when typed
                // policy disappears or changes beneath a persisted approval.
                let StoredValue::Ordinary(value) = stored else {
                    return None;
                };
                if !super::host_pinning(catalogued, &declared).admits(&value) {
                    return None;
                }
                return Some(ConfigValue::proposed(value));
            }

            match stored {
                StoredValue::Ordinary(value) => Some(ConfigValue::proposed(value)),
                StoredValue::Origin(_) => None,
            }
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
            Field::ChannelQuery { channel, parameter } => {
                format!("channel.{channel}.query.{parameter}")
            }
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
            self.set_for_instance(tenant, connector, None, declared, value)
        }

        fn set_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            value: &str,
        ) -> Result<(), SettingsRefusal> {
            if custom_origin_rule(self.custom_origins.as_ref(), connector, declared).is_some() {
                return self
                    .propose_authority_for_instance(
                        tenant, connector, instance, declared, value, None,
                    )
                    .map(|_| ());
            }
            self.admit(connector, declared, value)?;

            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            // Policy is re-evaluated while the write lock is held. The current policy object is
            // immutable, but keeping this decision inside the CAS boundary makes the 0.19 typed
            // implementation replaceable without moving the safety check.
            self.admit(connector, declared, value)?;

            // The allowance is decided against what this write *replaces*, not against the whole
            // new value on top of an occupancy that already includes the old one — the same reading
            // the credential surface takes for a rotation, and for the same reason: counting both
            // would refuse a one-byte change to a tenant sitting near its bound.
            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                &declared.binds(),
            );
            let replacing = document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b))
                .map_or(0, StoredValue::occupied_bytes);
            if document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b))
                .is_some_and(|stored| matches!(stored, StoredValue::Origin(_)))
            {
                return Err(SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            }
            let held = occupied(&document.values, tenant.as_str());
            admit_tenant_settings(held.saturating_sub(replacing), value.len())?;

            let previous = document.clone();
            document
                .values
                .entry(t)
                .or_default()
                .entry(c.clone())
                .or_default()
                .entry(s)
                .or_default()
                .insert(b, StoredValue::Ordinary(value.to_owned()));

            // Persisted under the same lock the map was changed under, and rolled back if the file
            // will not take it: a store whose memory and file disagree would answer the invoker
            // with a value that vanishes at the next restart, which is the failure this whole
            // module's durability argument is about.
            if let Err(refusal) = self.persist(&document) {
                *document = previous;
                return Err(refusal);
            }

            Ok(())
        }

        fn prepare_authority_proposal_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            value: &str,
            expected_revision: Option<u64>,
        ) -> Result<PreparedAuthorityProposal, SettingsRefusal> {
            let normalized = self.admit(connector, declared, value)?.ok_or_else(|| {
                SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                }
            })?;
            let document = self
                .document
                .read()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                &declared.binds(),
            );
            let current = document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b));
            match (expected_revision, current) {
                (None, None) => {}
                (None, Some(StoredValue::Origin(current))) => {
                    return Err(SettingsRefusal::AuthorityRevisionRequired {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                        current: current.revision,
                    });
                }
                (None, Some(StoredValue::Ordinary(_))) => {
                    return Err(SettingsRefusal::AuthorityUnsupported {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                    });
                }
                (Some(_), None) => {
                    return Err(SettingsRefusal::AuthorityUnset {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                    });
                }
                (Some(expected), Some(StoredValue::Origin(current)))
                    if expected != current.revision =>
                {
                    return Err(SettingsRefusal::AuthorityRevisionConflict {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                        expected,
                        current: current.revision,
                    });
                }
                (Some(_), Some(StoredValue::Origin(_))) => {}
                (Some(_), Some(StoredValue::Ordinary(_))) => {
                    return Err(SettingsRefusal::AuthorityUnsupported {
                        connector: connector.to_owned(),
                        setting: declared.binds(),
                    });
                }
            }

            let replacing = current.map_or(0, StoredValue::occupied_bytes);
            let held = occupied(&document.values, tenant.as_str());
            admit_tenant_settings(
                held.saturating_sub(replacing),
                normalized
                    .setting_value
                    .len()
                    .saturating_add(normalized.origin.len()),
            )?;
            let revision = document.next_origin_revision;
            revision
                .checked_add(1)
                .ok_or_else(|| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the custom-origin revision space is exhausted".to_owned(),
                })?;
            Ok(PreparedAuthorityProposal {
                store_path: self.path.clone(),
                tenant: tenant.clone(),
                connector: connector.to_owned(),
                instance: instance.cloned(),
                declared: declared.clone(),
                submitted: value.to_owned(),
                normalized,
                expected_revision,
                revision,
            })
        }

        fn commit_authority_proposal_for_instance(
            &self,
            prepared: PreparedAuthorityProposal,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            if prepared.store_path != self.path {
                return Err(SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the custom-origin proposal was prepared by a different settings store"
                        .to_owned(),
                });
            }
            let PreparedAuthorityProposal {
                store_path: _,
                tenant,
                connector,
                instance,
                declared,
                submitted,
                normalized: prepared_normalized,
                expected_revision,
                revision,
            } = prepared;
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            // The current typed rule, declaration, bounds and normalization are checked again under
            // the same lock as both CAS decisions. Preparation authorizes no later policy snapshot.
            let normalized = self
                .admit(&connector, &declared, &submitted)?
                .ok_or_else(|| SettingsRefusal::AuthorityUnsupported {
                    connector: connector.clone(),
                    setting: declared.binds(),
                })?;
            if normalized != prepared_normalized {
                return Err(SettingsRefusal::MalformedOrigin {
                    connector,
                    setting: declared.binds(),
                });
            }
            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                &connector,
                instance.as_ref(),
                &declared.service,
                &declared.binds(),
            );
            let current = document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b));
            match (expected_revision, current) {
                (None, None) => {}
                (None, Some(StoredValue::Origin(current))) => {
                    return Err(SettingsRefusal::AuthorityRevisionRequired {
                        connector,
                        setting: declared.binds(),
                        current: current.revision,
                    });
                }
                (None, Some(StoredValue::Ordinary(_))) => {
                    return Err(SettingsRefusal::AuthorityUnsupported {
                        connector,
                        setting: declared.binds(),
                    });
                }
                (Some(_), None) => {
                    return Err(SettingsRefusal::AuthorityUnset {
                        connector,
                        setting: declared.binds(),
                    });
                }
                (Some(expected), Some(StoredValue::Origin(current)))
                    if expected != current.revision =>
                {
                    return Err(SettingsRefusal::AuthorityRevisionConflict {
                        connector,
                        setting: declared.binds(),
                        expected,
                        current: current.revision,
                    });
                }
                (Some(_), Some(StoredValue::Origin(_))) => {}
                (Some(_), Some(StoredValue::Ordinary(_))) => {
                    return Err(SettingsRefusal::AuthorityUnsupported {
                        connector,
                        setting: declared.binds(),
                    });
                }
            }
            if document.next_origin_revision != revision {
                return Err(SettingsRefusal::AuthorityRevisionConflict {
                    connector,
                    setting: declared.binds(),
                    expected: revision,
                    current: document.next_origin_revision,
                });
            }
            let replacing = current.map_or(0, StoredValue::occupied_bytes);
            let held = occupied(&document.values, tenant.as_str());
            admit_tenant_settings(
                held.saturating_sub(replacing),
                normalized
                    .setting_value
                    .len()
                    .saturating_add(normalized.origin.len()),
            )?;
            let next_revision =
                revision
                    .checked_add(1)
                    .ok_or_else(|| SettingsRefusal::Unwritable {
                        path: self.path.display().to_string(),
                        reason: "the custom-origin revision space is exhausted".to_owned(),
                    })?;
            let previous = document.clone();
            document.next_origin_revision = next_revision;
            let status = AuthorityStatus {
                state: AuthorityState::Proposed,
                revision: Some(revision),
                origin: Some(normalized.origin.clone()),
            };
            document
                .values
                .entry(t)
                .or_default()
                .entry(c)
                .or_default()
                .entry(s)
                .or_default()
                .insert(
                    b,
                    StoredValue::Origin(StoredOrigin {
                        kind: OriginKind::CustomOrigin,
                        value: normalized.setting_value,
                        origin: normalized.origin,
                        state: StoredAuthorityState::Proposed,
                        revision,
                    }),
                );
            if let Err(refusal) = self.persist(&document) {
                *document = previous;
                return Err(refusal);
            }
            Ok(status)
        }

        fn clear(
            &self,
            tenant: &Tenant,
            connector: &str,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            self.clear_for_instance(tenant, connector, None, declared)
        }

        fn clear_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> Result<bool, SettingsRefusal> {
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;

            let [t, c, s, b] = SettingsStore::at(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                &declared.binds(),
            );
            let previous_document = document.clone();
            let Some(_previous) = document
                .values
                .get_mut(&t)
                .and_then(|connectors| connectors.get_mut(&c))
                .and_then(|services| services.get_mut(&s))
                .and_then(|settings| settings.remove(&b))
            else {
                return Ok(false);
            };

            if let Err(refusal) = self.persist(&document) {
                *document = previous_document;
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

        fn is_set_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> bool {
            self.get_for_instance(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                declared.field(),
            )
            .is_some()
        }

        fn is_custom_origin(&self, connector: &str, declared: &DeclaredSetting) -> bool {
            custom_origin_rule(self.custom_origins.as_ref(), connector, declared).is_some()
        }

        fn authority_status_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            let document = self
                .document
                .read()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            if custom_origin_rule(self.custom_origins.as_ref(), connector, declared).is_none() {
                return Err(SettingsRefusal::AuthorityUnsupported {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                });
            }
            let [t, c, s, b] = Self::at(
                tenant.as_str(),
                connector,
                instance,
                &declared.service,
                &declared.binds(),
            );
            Ok(document
                .values
                .get(&t)
                .and_then(|connectors| connectors.get(&c))
                .and_then(|services| services.get(&s))
                .and_then(|settings| settings.get(&b))
                .and_then(|stored| match stored {
                    StoredValue::Origin(origin) => Some(authority_status(origin)),
                    StoredValue::Ordinary(_) => None,
                })
                .unwrap_or_else(AuthorityStatus::unset))
        }

        fn approve_authority_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            revision: u64,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            self.transition_authority(
                tenant,
                connector,
                instance,
                declared,
                revision,
                AuthorityTransition::Approve,
            )
        }

        fn revoke_authority_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
            declared: &DeclaredSetting,
            revision: u64,
        ) -> Result<AuthorityStatus, SettingsRefusal> {
            self.transition_authority(
                tenant,
                connector,
                instance,
                declared,
                revision,
                AuthorityTransition::Revoke,
            )
        }

        fn qualify_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: &InstanceId,
        ) -> Result<(), SettingsRefusal> {
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let Some(connectors) = document.values.get(tenant.as_str()) else {
                return Ok(());
            };
            let qualified = format!("{connector}@{}", instance.as_str());
            if connectors.contains_key(&qualified) {
                return Err(SettingsRefusal::InstanceTransition {
                    connector: connector.to_owned(),
                    reason: format!("destination namespace `{qualified}` already exists"),
                });
            }
            if !connectors.contains_key(connector) {
                return Ok(());
            }
            let previous = document.clone();
            let connectors = document
                .values
                .get_mut(tenant.as_str())
                .expect("the tenant existed above");
            let legacy = connectors
                .remove(connector)
                .expect("the legacy namespace existed above");
            connectors.insert(qualified, legacy);
            if let Err(refusal) = self.persist(&document) {
                *document = previous;
                return Err(refusal);
            }
            Ok(())
        }

        fn collapse_instances(
            &self,
            tenant: &Tenant,
            connector: &str,
            removed: &InstanceId,
            remaining: &InstanceId,
        ) -> Result<(), SettingsRefusal> {
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let Some(connectors) = document.values.get_mut(tenant.as_str()) else {
                return Ok(());
            };
            if connectors.contains_key(connector) {
                return Err(SettingsRefusal::InstanceTransition {
                    connector: connector.to_owned(),
                    reason: "the legacy destination namespace is already occupied".to_owned(),
                });
            }
            let previous = document.clone();
            let connectors = document
                .values
                .get_mut(tenant.as_str())
                .expect("the tenant existed above");
            connectors.remove(&format!("{connector}@{}", removed.as_str()));
            if let Some(survivor) =
                connectors.remove(&format!("{connector}@{}", remaining.as_str()))
            {
                connectors.insert(connector.to_owned(), survivor);
            }
            if let Err(refusal) = self.persist(&document) {
                *document = previous;
                return Err(refusal);
            }
            Ok(())
        }

        fn discard_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: &InstanceId,
        ) -> Result<(), SettingsRefusal> {
            let mut document = self
                .document
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let previous = document.clone();
            let removed = document
                .values
                .get_mut(tenant.as_str())
                .and_then(|connectors| {
                    connectors.remove(&format!("{connector}@{}", instance.as_str()))
                })
                .is_some();
            if removed {
                if let Err(refusal) = self.persist(&document) {
                    *document = previous;
                    return Err(refusal);
                }
            }
            Ok(())
        }

        fn held_bytes(&self, tenant: &Tenant) -> usize {
            self.document
                .read()
                .map_or(0, |document| occupied(&document.values, tenant.as_str()))
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
                .map(StoredValue::occupied_bytes)
                .sum()
        })
    }

    /// Why a settings store could not be bound. Every variant refuses; none falls back.
    #[derive(Debug, thiserror::Error)]
    pub enum SettingsStoreError {
        /// A legacy ordinary value now has typed custom-origin policy and cannot be guessed safe.
        #[error(
            "the connection-settings store at `{path}` needs an explicit migration: connector `{connector}` setting `{setting}` is an ordinary legacy value but is now a custom origin; it was not auto-approved"
        )]
        MigrationRequired {
            /// Bound settings file.
            path: String,
            /// Connector owning the value.
            connector: String,
            /// Declared setting address.
            setting: String,
        },
        /// No store was named, and there is no default worth choosing on an operator's behalf.
        #[error(
            "no connection-settings store is configured: set `{setting}` to a path outside every \
             working tree, for example `{EXAMPLE_PATH}`. This host does not fall back to an \
             in-memory store — one would accept every value and lose it on restart, which reads as \
             thirteen connectors that stopped resolving for no reason"
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

    #[cfg(all(test, unix))]
    mod authority_tests {
        use super::*;
        use crate::HostPinning;
        use std::fs;
        use std::os::unix::fs::PermissionsExt as _;
        use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
        use std::sync::{Arc, Barrier};

        struct TestPolicy;

        struct TestHttpsOriginRule;

        struct SwitchablePolicy {
            reject: AtomicBool,
        }

        static TEST_HTTPS_ORIGIN_RULE: TestHttpsOriginRule = TestHttpsOriginRule;

        impl CustomOriginPolicy for TestPolicy {
            fn rule(
                &self,
                connector: &str,
                declared: &DeclaredSetting,
            ) -> Option<&dyn CustomOriginRule> {
                let (candidate_connector, candidate) = candidate();
                (connector == candidate_connector && declared == &candidate)
                    .then_some(&TEST_HTTPS_ORIGIN_RULE as &dyn CustomOriginRule)
            }
        }

        impl CustomOriginRule for TestHttpsOriginRule {
            fn normalize(&self, value: &str) -> Result<NormalizedOrigin, OriginPolicyRefusal> {
                let Some((scheme, authority)) = value.split_once("://") else {
                    return Err(OriginPolicyRefusal::Malformed);
                };
                if !scheme.eq_ignore_ascii_case("https") {
                    return Err(OriginPolicyRefusal::UnsupportedScheme);
                }
                if authority.is_empty()
                    || authority.contains(['/', '?', '#', '@', ':'])
                    || authority.starts_with('.')
                    || authority.ends_with('.')
                    || authority.split('.').any(|label| {
                        label.is_empty()
                            || label.starts_with('-')
                            || label.ends_with('-')
                            || !label
                                .bytes()
                                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    })
                {
                    return Err(OriginPolicyRefusal::Malformed);
                }
                let authority = authority.to_ascii_lowercase();
                Ok(NormalizedOrigin {
                    setting_value: authority.clone(),
                    origin: format!("https://{authority}"),
                })
            }
        }

        impl CustomOriginPolicy for SwitchablePolicy {
            fn rule(
                &self,
                connector: &str,
                declared: &DeclaredSetting,
            ) -> Option<&dyn CustomOriginRule> {
                let (candidate_connector, candidate) = candidate();
                (connector == candidate_connector && declared == &candidate)
                    .then_some(self as &dyn CustomOriginRule)
            }
        }

        impl CustomOriginRule for SwitchablePolicy {
            fn normalize(&self, value: &str) -> Result<NormalizedOrigin, OriginPolicyRefusal> {
                if self.reject.load(Ordering::SeqCst) {
                    Err(OriginPolicyRefusal::Malformed)
                } else {
                    TEST_HTTPS_ORIGIN_RULE.normalize(value)
                }
            }
        }

        struct Scratch(PathBuf);

        impl Scratch {
            fn new(label: &str) -> Self {
                static NEXT: AtomicU64 = AtomicU64::new(0);
                let path = std::env::temp_dir().join(format!(
                    "exchange-origin-{label}-{}-{}",
                    std::process::id(),
                    NEXT.fetch_add(1, Ordering::Relaxed)
                ));
                crate::ensure_private_state_directory(&path).expect("scratch");
                Self(path)
            }
        }

        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = fs::set_permissions(&self.0, fs::Permissions::from_mode(0o700));
                let _ = fs::remove_dir_all(&self.0);
            }
        }

        fn candidate() -> (&'static str, DeclaredSetting) {
            connector_catalog::providers()
                .iter()
                .copied()
                .find_map(|provider| {
                    super::super::declared_settings(provider)
                        .ok()?
                        .into_iter()
                        .find(|declared| {
                            matches!(
                                super::super::host_pinning(provider, declared),
                                HostPinning::WholeAuthority(_)
                            )
                        })
                        .map(|declared| (provider.id, declared))
                })
                .expect("catalogue has a whole-authority declaration")
        }

        fn connector() -> &'static str {
            candidate().0
        }

        fn setting() -> DeclaredSetting {
            candidate().1
        }

        fn instance() -> InstanceId {
            InstanceId::parse("11111111-1111-4111-8111-111111111111").expect("instance")
        }

        fn other_instance() -> InstanceId {
            InstanceId::parse("22222222-2222-4222-8222-222222222222").expect("instance")
        }

        fn bind(path: &Path) -> SettingsStore {
            SettingsStore::bind_with_custom_origin_policy(path, Arc::new(TestPolicy))
                .expect("test policy store")
        }

        #[test]
        fn released_catalogue_origin_is_the_production_authority_policy() {
            let scratch = Scratch::new("released-policy");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = DeclaredSetting::parse("default", "endpoint.origin")
                .expect("GitLab origin declaration");
            let store = Arc::new(SettingsStore::bind(&path).expect("production settings store"));
            let provider =
                connector_catalog::provider(connector_catalog::ProviderKey::id("gitlab"))
                    .expect("GitLab provider");
            let verify = provider
                .operations
                .iter()
                .find(|operation| Some(operation.id) == provider.verify)
                .expect("GitLab verification operation");
            let rehearsal = Rehearsal::of(verify.id, provider.id, verify.service, verify.flux)
                .expect("released verification operation");
            let configuration =
                Configuration::new(store.clone(), "acme").expect("tenant-bound configuration");

            assert!(store.is_custom_origin("gitlab", &declared));
            assert!(matches!(
                store.propose_authority_for_instance(
                    &tenant,
                    "gitlab",
                    None,
                    &declared,
                    "http://gitlab.internal.example",
                    None,
                ),
                Err(SettingsRefusal::OriginSchemeUnsupported { .. })
            ));

            let proposal = store
                .propose_authority_for_instance(
                    &tenant,
                    "gitlab",
                    None,
                    &declared,
                    "https://gitlab.internal.example:8443",
                    None,
                )
                .expect("proposal");
            let revision = proposal.revision.expect("revision");
            assert_eq!(
                store.get("acme", "gitlab", "default", Field::Endpoint("origin")),
                None,
                "a proposal must not reach connector-pack"
            );
            assert!(rehearsal
                .request(&configuration, &serde_json::json!({}))
                .is_err());

            store
                .approve_authority_for_instance(&tenant, "gitlab", None, &declared, revision)
                .expect("approval");
            let resolved = store
                .resolve_for_instance("acme", "gitlab", None, "default", Field::Endpoint("origin"))
                .expect("approved value");
            assert_eq!(resolved.value(), "https://gitlab.internal.example:8443");
            assert!(resolved.is_operator_approved());
            assert_eq!(
                rehearsal
                    .request(&configuration, &serde_json::json!({}))
                    .expect("approved request projection")
                    .url,
                "https://gitlab.internal.example:8443/api/v4/user"
            );

            store
                .revoke_authority_for_instance(&tenant, "gitlab", None, &declared, revision)
                .expect("revocation");
            assert!(store
                .resolve_for_instance("acme", "gitlab", None, "default", Field::Endpoint("origin"),)
                .is_none());
            assert!(rehearsal
                .request(&configuration, &serde_json::json!({}))
                .is_err());
        }

        fn proposal_revision(
            store: &SettingsStore,
            tenant: &Tenant,
            declared: &DeclaredSetting,
            instance: &InstanceId,
        ) -> u64 {
            store
                .authority_status_for_instance(tenant, connector(), Some(instance), declared)
                .expect("authority status")
                .revision
                .expect("proposal revision")
        }

        #[test]
        fn replacement_proposals_are_create_or_compare_and_swap() {
            let scratch = Scratch::new("replacement-cas");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);

            let first = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://FIRST.example",
                    None,
                )
                .expect("create proposal");
            let first_revision = first.revision.expect("first revision");
            assert_eq!(first.origin.as_deref(), Some("https://first.example"));

            assert!(matches!(
                store.propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://blind.example",
                    None,
                ),
                Err(SettingsRefusal::AuthorityRevisionRequired { current, .. })
                    if current == first_revision
            ));
            assert!(matches!(
                store.set_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://generic-blind.example",
                ),
                Err(SettingsRefusal::AuthorityRevisionRequired { current, .. })
                    if current == first_revision
            ));
            assert!(matches!(
                store.propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://stale.example",
                    Some(first_revision + 1),
                ),
                Err(SettingsRefusal::AuthorityRevisionConflict {
                    expected,
                    current,
                    ..
                }) if expected == first_revision + 1 && current == first_revision
            ));
            assert_eq!(
                proposal_revision(&store, &tenant, &declared, &instance),
                first_revision
            );

            let replacement = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://replacement.example",
                    Some(first_revision),
                )
                .expect("checked replacement");
            assert_eq!(replacement.state, AuthorityState::Proposed);
            assert!(replacement.revision.expect("replacement revision") > first_revision);
        }

        #[test]
        fn prepared_revision_is_read_only_and_commit_cas_covers_the_high_water_mark() {
            let scratch = Scratch::new("prepared-revision");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let first_instance = instance();
            let second_instance = other_instance();
            let store = bind(&path);

            let first = store
                .prepare_authority_proposal_for_instance(
                    &tenant,
                    connector(),
                    Some(&first_instance),
                    &declared,
                    "https://first.example",
                    None,
                )
                .expect("first preparation");
            let second = store
                .prepare_authority_proposal_for_instance(
                    &tenant,
                    connector(),
                    Some(&second_instance),
                    &declared,
                    "https://second.example",
                    None,
                )
                .expect("concurrent preparation");
            assert_eq!(first.revision(), second.revision());
            assert!(!path.exists(), "preparation must not persist or allocate");
            assert_eq!(
                store
                    .authority_status_for_instance(
                        &tenant,
                        connector(),
                        Some(&first_instance),
                        &declared,
                    )
                    .expect("first status")
                    .state,
                AuthorityState::Unset
            );

            let committed = store
                .commit_authority_proposal_for_instance(first)
                .expect("first exact commit");
            let committed_revision = committed.revision.expect("committed revision");
            let before_loser = fs::read(&path).expect("durable first commit");
            assert!(matches!(
                store.commit_authority_proposal_for_instance(second),
                Err(SettingsRefusal::AuthorityRevisionConflict {
                    expected,
                    current,
                    ..
                }) if expected == committed_revision && current == committed_revision + 1
            ));
            assert_eq!(
                fs::read(&path).expect("unchanged durable state"),
                before_loser
            );
            assert_eq!(
                store
                    .authority_status_for_instance(
                        &tenant,
                        connector(),
                        Some(&second_instance),
                        &declared,
                    )
                    .expect("second status")
                    .state,
                AuthorityState::Unset,
                "the losing prepared proposal must not mutate its address"
            );

            let retried = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&second_instance),
                    &declared,
                    "https://second.example",
                    None,
                )
                .expect("retry after allocation race");
            assert_eq!(
                retried.revision,
                committed_revision.checked_add(1),
                "the losing commit must not advance the durable high-water mark"
            );
        }

        #[test]
        fn proposal_persists_only_the_normalized_setting_and_inspection_origin() {
            let scratch = Scratch::new("normalized-origin");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);

            let status = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "HTTPS://MiXeD.Example",
                    None,
                )
                .expect("normalized proposal");
            assert_eq!(status.origin.as_deref(), Some("https://mixed.example"));
            assert_eq!(status.state, AuthorityState::Proposed);
            assert_eq!(
                store.get_for_instance(
                    tenant.as_str(),
                    connector(),
                    Some(&instance),
                    "default",
                    declared.field(),
                ),
                None
            );
            let persisted = fs::read_to_string(&path).expect("persisted proposal");
            assert!(!persisted.contains("MiXeD"));
            assert!(persisted.contains("https://mixed.example"));
            assert!(persisted.contains("\"value\": \"mixed.example\""));
        }

        #[test]
        fn custom_origin_grammar_refuses_without_persisting_or_echoing_values() {
            let scratch = Scratch::new("origin-grammar");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);

            for candidate in [
                "http://plaintext.invalid",
                "https://user@credential-shaped.invalid",
                "https://path.invalid/secret",
                "https://double..label.invalid",
            ] {
                let refusal = store
                    .propose_authority_for_instance(
                        &tenant,
                        connector(),
                        Some(&instance),
                        &declared,
                        candidate,
                        None,
                    )
                    .expect_err("typed grammar refusal");
                assert!(!refusal.to_string().contains(candidate));
                assert!(!path.exists());
            }
        }

        #[test]
        fn concurrent_replacements_admit_exactly_one_matching_revision() {
            let scratch = Scratch::new("concurrent-replacement");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = Arc::new(bind(&path));
            store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://first.example",
                    None,
                )
                .expect("initial proposal");
            let revision = proposal_revision(&store, &tenant, &declared, &instance);
            let barrier = Arc::new(Barrier::new(3));
            let mut workers = Vec::new();
            for value in ["https://second.example", "https://third.example"] {
                let store = store.clone();
                let barrier = barrier.clone();
                let tenant = tenant.clone();
                let declared = declared.clone();
                let instance = instance.clone();
                workers.push(std::thread::spawn(move || {
                    barrier.wait();
                    store.propose_authority_for_instance(
                        &tenant,
                        connector(),
                        Some(&instance),
                        &declared,
                        value,
                        Some(revision),
                    )
                }));
            }
            barrier.wait();
            let outcomes = workers
                .into_iter()
                .map(|worker| worker.join().expect("replacement worker"))
                .collect::<Vec<_>>();

            assert_eq!(outcomes.iter().filter(|outcome| outcome.is_ok()).count(), 1);
            assert_eq!(
                outcomes
                    .iter()
                    .filter(|outcome| matches!(
                        outcome,
                        Err(SettingsRefusal::AuthorityRevisionConflict { .. })
                    ))
                    .count(),
                1
            );
            assert!(proposal_revision(&store, &tenant, &declared, &instance) > revision);
        }

        #[test]
        fn revoked_revision_cannot_be_approved_or_replayed() {
            let scratch = Scratch::new("revoked-replay");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);
            let proposal = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://first.example",
                    None,
                )
                .expect("proposal");
            let revision = proposal.revision.expect("revision");
            store
                .approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                )
                .expect("approve proposal");
            assert!(matches!(
                store.approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                ),
                Err(SettingsRefusal::AuthorityStateConflict {
                    current: AuthorityState::Approved,
                    ..
                })
            ));
            store
                .revoke_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                )
                .expect("revoke approval");
            let before = fs::read(&path).expect("durable revoked bytes");

            assert!(matches!(
                store.revoke_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                ),
                Err(SettingsRefusal::AuthorityStateConflict {
                    current: AuthorityState::Revoked,
                    ..
                })
            ));

            assert!(matches!(
                store.approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                ),
                Err(SettingsRefusal::AuthorityStateConflict {
                    current: AuthorityState::Revoked,
                    ..
                })
            ));
            assert_eq!(fs::read(&path).expect("unchanged store"), before);

            let replacement = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://second.example",
                    Some(revision),
                )
                .expect("new proposal after revoke");
            assert_eq!(replacement.state, AuthorityState::Proposed);
            assert!(replacement.revision.expect("new revision") > revision);
        }

        #[test]
        fn origin_lifecycle_survives_restart_and_clear_cannot_reuse_a_revision() {
            let scratch = Scratch::new("lifecycle");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);

            store
                .set_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://ONE.example",
                )
                .expect("proposal");
            let first = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status");
            assert_eq!(first.state, AuthorityState::Proposed);
            let first_revision = first.revision.expect("revision");
            assert_eq!(
                store.get_for_instance(
                    tenant.as_str(),
                    connector(),
                    Some(&instance),
                    "default",
                    declared.field(),
                ),
                None
            );

            // Even byte-identical writes are new proposals and cannot inherit approval.
            store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://ONE.example",
                    Some(first_revision),
                )
                .expect("new proposal");
            let second = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status");
            let second_revision = second.revision.expect("revision");
            assert!(second_revision > first_revision);
            let before_stale = fs::read(&path).expect("store bytes");
            assert!(matches!(
                store.approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    first_revision
                ),
                Err(SettingsRefusal::AuthorityRevisionConflict { .. })
            ));
            assert_eq!(fs::read(&path).expect("store bytes"), before_stale);

            store
                .approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    second_revision,
                )
                .expect("approve");
            drop(store);
            let store = bind(&path);
            assert_eq!(
                store
                    .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                    .expect("status")
                    .state,
                AuthorityState::Approved
            );
            assert_eq!(
                store.get_for_instance(
                    tenant.as_str(),
                    connector(),
                    Some(&instance),
                    "default",
                    declared.field(),
                ),
                Some("one.example".to_owned())
            );
            store
                .revoke_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    second_revision,
                )
                .expect("revoke");
            drop(store);
            let store = bind(&path);
            assert_eq!(
                store
                    .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                    .expect("status")
                    .state,
                AuthorityState::Revoked
            );
            assert_eq!(
                store.get_for_instance(
                    tenant.as_str(),
                    connector(),
                    Some(&instance),
                    "default",
                    declared.field(),
                ),
                None
            );
            store
                .clear_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("clear");
            store
                .set_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://ONE.example",
                )
                .expect("recreate");
            let recreated = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status");
            assert!(recreated.revision.expect("revision") > second_revision);
        }

        #[test]
        fn current_policy_revalidates_restart_transition_and_runtime_reads() {
            let scratch = Scratch::new("policy-revalidation");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let policy = Arc::new(SwitchablePolicy {
                reject: AtomicBool::new(false),
            });
            let store = SettingsStore::bind_with_custom_origin_policy(&path, policy.clone())
                .expect("test policy store");
            let proposed = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://previously-valid.example",
                    None,
                )
                .expect("proposal");
            let revision = proposed.revision.expect("revision");
            store
                .approve_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                )
                .expect("approval under initial policy");

            policy.reject.store(true, Ordering::SeqCst);
            assert_eq!(
                store.get_for_instance(
                    tenant.as_str(),
                    connector(),
                    Some(&instance),
                    &declared.service,
                    declared.field(),
                ),
                None,
                "an approval must not outlive the typed rule that admitted it"
            );
            let transition = store
                .revoke_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    revision,
                )
                .expect_err("every transition revalidates the current typed rule");
            assert!(matches!(
                transition,
                SettingsRefusal::MalformedOrigin { .. }
            ));
            let message = transition.to_string();
            assert!(!message.contains("previously-valid.example"), "{message}");
            drop(store);

            let restart = SettingsStore::bind_with_custom_origin_policy(&path, policy)
                .expect_err("restart refuses a record the current typed rule rejects");
            let message = restart.to_string();
            assert!(!message.contains("previously-valid.example"), "{message}");
        }

        #[test]
        fn concurrent_set_and_approval_never_approve_the_replacement() {
            let scratch = Scratch::new("cas");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = Arc::new(bind(&path));
            store
                .set_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://first.example",
                )
                .expect("proposal");
            let revision = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status")
                .revision
                .expect("revision");
            let barrier = Arc::new(Barrier::new(3));
            let writer = {
                let store = store.clone();
                let barrier = barrier.clone();
                let tenant = tenant.clone();
                let declared = declared.clone();
                let instance = instance.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.propose_authority_for_instance(
                        &tenant,
                        connector(),
                        Some(&instance),
                        &declared,
                        "https://replacement.example",
                        Some(revision),
                    )
                })
            };
            let approver = {
                let store = store.clone();
                let barrier = barrier.clone();
                let tenant = tenant.clone();
                let declared = declared.clone();
                let instance = instance.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    store.approve_authority_for_instance(
                        &tenant,
                        connector(),
                        Some(&instance),
                        &declared,
                        revision,
                    )
                })
            };
            barrier.wait();
            writer.join().expect("writer").expect("write");
            let _ = approver.join().expect("approver");
            let status = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status");
            assert_eq!(status.state, AuthorityState::Proposed);
            assert!(status.revision.expect("revision") > revision);
        }

        #[test]
        fn legacy_custom_origin_and_unknown_schema_refuse_binding() {
            let scratch = Scratch::new("schema");
            let path = scratch.0.join("settings");
            crate::write_private_state_file(
                &path,
                br#"{"schema":"exchange.connection-settings.v999","next_origin_revision":1,"values":{}}"#,
            )
            .expect("unknown schema fixture");
            assert!(SettingsStore::bind(&path).is_err());

            crate::write_private_state_file(
                &path,
                br#"{"schema":{"ordinary":{"default":{"endpoint.name":"legacy.example"}}}}"#,
            )
            .expect("legacy tenant named schema");
            SettingsStore::bind(&path).expect("non-string schema is a legacy tenant id");

            let declared = setting();
            crate::write_private_state_file(
                &path,
                format!(
                    r#"{{"acme":{{"{}":{{"{}":{{"{}":"legacy.example"}}}}}}}}"#,
                    connector(),
                    declared.service,
                    declared.binds(),
                )
                .as_bytes(),
            )
            .expect("legacy fixture");
            SettingsStore::bind(&path)
                .expect("production accepts legacy data outside its declared origin address");
            assert!(matches!(
                SettingsStore::bind_with_custom_origin_policy(&path, Arc::new(TestPolicy)),
                Err(SettingsStoreError::MigrationRequired { .. })
            ));

            crate::write_private_state_file(
                &path,
                br#"{"acme":{"gitlab":{"default":{"endpoint.origin":"https://legacy.example"}}}}"#,
            )
            .expect("released origin legacy fixture");
            assert!(matches!(
                SettingsStore::bind(&path),
                Err(SettingsStoreError::MigrationRequired { connector, setting, .. })
                    if connector == "gitlab" && setting == "endpoint.origin"
            ));
        }

        #[test]
        fn legacy_tagged_origin_migrates_only_through_the_current_typed_rule() {
            let scratch = Scratch::new("tagged-migration");
            let path = scratch.0.join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let fixture = serde_json::json!({
                "schema": STORE_SCHEMA,
                "next_origin_revision": 8,
                "values": {
                    "acme": {
                        (format!("{}@{}", connector(), instance.as_str())): {
                            (declared.service.clone()): {
                                (declared.binds()): {
                                    "kind": "custom_origin",
                                    "value": "HTTPS://LEGACY.example",
                                    "state": "proposed",
                                    "revision": 7
                                }
                            }
                        }
                    }
                }
            });
            crate::write_private_state_file(
                &path,
                &serde_json::to_vec(&fixture).expect("serialize legacy tagged fixture"),
            )
            .expect("legacy tagged fixture");

            assert!(SettingsStore::bind(&path).is_err());
            let store = bind(&path);
            let status = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("migrated inspection");
            assert_eq!(status.origin.as_deref(), Some("https://legacy.example"));
            store
                .approve_authority_for_instance(&tenant, connector(), Some(&instance), &declared, 7)
                .expect("persist deliberate migration");
            let persisted = fs::read_to_string(&path).expect("migrated bytes");
            assert!(persisted.contains("https://legacy.example"));
            assert!(!persisted.contains("LEGACY"));
        }

        #[test]
        fn v2_persistence_is_forward_closed() {
            let scratch = Scratch::new("forward-closed");
            let path = scratch.0.join("settings");
            let declared = setting();
            let instance = instance();
            let record = |overrides: &str| {
                r#"{"schema":"SCHEMA","next_origin_revision":2,"values":{"acme":{"CONNECTOR_INSTANCE":{"SERVICE":{"BINDS":{"kind":"custom_origin","value":"closed.example","origin":"https://closed.example","state":"proposed","revision":1OVERRIDES}}}}}}"#
                    .replace("SCHEMA", STORE_SCHEMA)
                    .replace(
                        "CONNECTOR_INSTANCE",
                        &format!("{}@{}", connector(), instance.as_str()),
                    )
                    .replace("SERVICE", &declared.service)
                    .replace("BINDS", &declared.binds())
                    .replace("OVERRIDES", overrides)
            };
            let fixtures = [
                (
                    "unknown root field",
                    format!(
                        r#"{{"schema":"{STORE_SCHEMA}","next_origin_revision":1,"values":{{}},"future":true}}"#
                    ),
                ),
                ("unknown record field", record(",\"future\":true")),
                (
                    "null normalized origin",
                    record("").replace("\"origin\":\"https://closed.example\"", "\"origin\":null"),
                ),
                (
                    "unknown kind",
                    record("").replace("custom_origin", "future_origin"),
                ),
                (
                    "unknown state",
                    record("").replace("proposed", "future_state"),
                ),
                (
                    "zero high-water",
                    record("").replace("\"next_origin_revision\":2", "\"next_origin_revision\":0"),
                ),
                (
                    "occupied high-water",
                    record("").replace("\"next_origin_revision\":2", "\"next_origin_revision\":1"),
                ),
                (
                    "unknown high-water shape",
                    record("").replace(
                        "\"next_origin_revision\":2",
                        "\"next_origin_revision\":\"2\"",
                    ),
                ),
            ];

            for (label, fixture) in fixtures {
                crate::write_private_state_file(&path, fixture.as_bytes())
                    .expect("forward-closed fixture");
                assert!(
                    SettingsStore::bind_with_custom_origin_policy(&path, Arc::new(TestPolicy))
                        .is_err(),
                    "{label} must refuse"
                );
            }
        }

        #[test]
        fn persistence_refusal_rolls_back_proposal_and_revision() {
            let scratch = Scratch::new("rollback");
            let path = scratch.0.join("state").join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);
            store
                .set_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://first.example",
                )
                .expect("proposal");
            let before = store
                .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                .expect("status");
            let directory = path.parent().expect("directory");
            fs::set_permissions(directory, fs::Permissions::from_mode(0o500))
                .expect("make unwritable");
            let refused = store.propose_authority_for_instance(
                &tenant,
                connector(),
                Some(&instance),
                &declared,
                "https://second.example",
                before.revision,
            );
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .expect("restore directory");
            assert!(refused.is_err());
            assert_eq!(
                store
                    .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                    .expect("status"),
                before
            );
            let replacement = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://second.example",
                    before.revision,
                )
                .expect("replacement after storage recovers");
            assert_eq!(
                replacement.revision,
                before.revision.and_then(|revision| revision.checked_add(1)),
                "a failed persistence attempt must roll back the revision high-water mark"
            );
        }

        #[test]
        fn persistence_refusal_rolls_back_authority_transition() {
            let scratch = Scratch::new("transition-rollback");
            let path = scratch.0.join("state").join("settings");
            let tenant = Tenant::new("acme").expect("tenant");
            let declared = setting();
            let instance = instance();
            let store = bind(&path);
            let proposal = store
                .propose_authority_for_instance(
                    &tenant,
                    connector(),
                    Some(&instance),
                    &declared,
                    "https://first.example",
                    None,
                )
                .expect("proposal");
            let revision = proposal.revision.expect("revision");
            let directory = path.parent().expect("directory");
            fs::set_permissions(directory, fs::Permissions::from_mode(0o500))
                .expect("make unwritable");
            let refused = store.approve_authority_for_instance(
                &tenant,
                connector(),
                Some(&instance),
                &declared,
                revision,
            );
            fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                .expect("restore directory");
            assert!(refused.is_err());
            assert_eq!(
                store
                    .authority_status_for_instance(&tenant, connector(), Some(&instance), &declared)
                    .expect("status")
                    .state,
                AuthorityState::Proposed
            );
        }
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
