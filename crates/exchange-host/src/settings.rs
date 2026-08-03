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
        admit_tenant_settings, ConnectionSettings, DeclaredSetting, InstanceId, SettingsRefusal,
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
                return Err(SettingsRefusal::WouldNameTheHost {
                    connector: connector.to_owned(),
                    setting: declared.binds(),
                    template: template.clone(),
                });
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

            Ok(())
        }
    }

    /// Read the store at `path`, or start from an empty one if it is not there yet.
    ///
    /// A file that is *there* and unreadable is a refusal: a store this process could not parse is
    /// one whose values are about to be silently absent, which reads to an operator as thirteen
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
            let binds = binds_of(field);
            let connector_key = match instance {
                Some(instance) => format!("{provider}@{}", instance.as_str()),
                None => provider.to_owned(),
            };

            let catalogued =
                connector_catalog::provider(connector_catalog::ProviderKey::id(provider))
                    .map(|catalogued| (catalogued, DeclaredSetting::parse(service, &binds)));

            let value = self
                .values
                .read()
                .ok()?
                .get(tenant)?
                .get(&connector_key)?
                .get(service)?
                .get(&binds)
                .cloned()?;

            if let Some((catalogued, declared)) = catalogued {
                // A `binds` target the pack asked for that this host cannot parse is not one it
                // can decide about, and an undecided value is not one it hands over.
                let declared = declared?;
                if !super::host_pinning(catalogued, &declared).admits(&value) {
                    return None;
                }
            }

            Some(value)
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
                instance,
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
                .entry(c.clone())
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
                    .and_then(|connectors| connectors.get_mut(&c))
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
            self.clear_for_instance(tenant, connector, None, declared)
        }

        fn clear_for_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: Option<&InstanceId>,
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
                instance,
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

        fn qualify_instance(
            &self,
            tenant: &Tenant,
            connector: &str,
            instance: &InstanceId,
        ) -> Result<(), SettingsRefusal> {
            let mut values = self
                .values
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let Some(connectors) = values.get(tenant.as_str()) else {
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
            let previous = values.clone();
            let connectors = values
                .get_mut(tenant.as_str())
                .expect("the tenant existed above");
            let legacy = connectors
                .remove(connector)
                .expect("the legacy namespace existed above");
            connectors.insert(qualified, legacy);
            if let Err(refusal) = self.persist(&values) {
                *values = previous;
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
            let mut values = self
                .values
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let Some(connectors) = values.get_mut(tenant.as_str()) else {
                return Ok(());
            };
            if connectors.contains_key(connector) {
                return Err(SettingsRefusal::InstanceTransition {
                    connector: connector.to_owned(),
                    reason: "the legacy destination namespace is already occupied".to_owned(),
                });
            }
            let previous = values.clone();
            let connectors = values
                .get_mut(tenant.as_str())
                .expect("the tenant existed above");
            connectors.remove(&format!("{connector}@{}", removed.as_str()));
            if let Some(survivor) =
                connectors.remove(&format!("{connector}@{}", remaining.as_str()))
            {
                connectors.insert(connector.to_owned(), survivor);
            }
            if let Err(refusal) = self.persist(&values) {
                *values = previous;
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
            let mut values = self
                .values
                .write()
                .map_err(|_| SettingsRefusal::Unwritable {
                    path: self.path.display().to_string(),
                    reason: "the store lock is poisoned".to_owned(),
                })?;
            let previous = values.clone();
            let removed = values
                .get_mut(tenant.as_str())
                .and_then(|connectors| {
                    connectors.remove(&format!("{connector}@{}", instance.as_str()))
                })
                .is_some();
            if removed {
                if let Err(refusal) = self.persist(&values) {
                    *values = previous;
                    return Err(refusal);
                }
            }
            Ok(())
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
