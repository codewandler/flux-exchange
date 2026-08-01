//! The flux engine line, proved by **linking** rather than by resolving (X-11).
//!
//! A manifest that resolves says only that Cargo found a set of versions it could write down. What
//! this repository needs is the stronger fact: that the `flux-runtime` `connector-pack` hands its
//! tools out through and the one `flux-web` builds its tools against are the **same** crate, and
//! that the credential vocabulary `exchange_host` derives addresses in is the **same** vocabulary
//! the pack resolves them through.
//!
//! Two versions of either resolve perfectly happily — Cargo is content to build both — and produce
//! two distinct traits with identical names. The failure is at type-check, at the call site that
//! tries to pass one to the other, which is why this is a compiled test and not an assertion about
//! `cargo tree` output. Every `assert!` below is incidental; the linking is what is being tested,
//! and a diff that breaks the engine line does not reach the assertions.
//!
//! # Why this is a dev-dependency
//!
//! `codewandler-flux-exchange-host` is the published artifact, and nothing in it executes an
//! operation yet — X-12 is what gives this repository a reason to *ship* `connector-pack`. A
//! `[dev-dependencies]` entry proves the link without putting the whole flux engine into the
//! dependency graph of every consumer of the published crate to satisfy a proof. X-12 promotes it
//! by moving the entries into `[dependencies]`.

use std::sync::Arc;

use connector_pack::{Configuration, Credentials, Egress, Layout, MemoryConfig, TenantLayout};
use exchange_host::{address_path, ConnectorDeclaration, DeclaredCredential, SecretStore, Tenant};
use flux_runtime::ToolRegistry;
use flux_web::{http::HttpRequestTool, WebOptions};

/// Any tenant that spells; which one it is does not matter to a linking proof.
const TENANT: &str = "acme";

/// `connector_pack::pack` and `flux_web::http::HttpRequestTool` compose into one registry.
///
/// This is the story's original Acceptance — "a trivial binary here links `connector_pack::pack`
/// and `flux_web::http::HttpRequestTool` together and compiles" — as a test rather than a binary,
/// so that `cargo test --workspace` is what keeps it true.
///
/// The load-bearing lines are the two coercions, not the assertion:
///
/// - `Egress::new` takes an `Arc<dyn Tool>` where `Tool` is resolved through `connector-pack`,
///   and `HttpRequestTool` implements the `Tool` resolved through `flux-web`. On two engine lines
///   those are two traits and the unsizing coercion does not exist.
/// - `ToolRegistry` is resolved through `flux-runtime` directly, and `pack`'s closure takes the one
///   resolved through `connector-pack`. Same argument, from the other end.
#[test]
fn connector_pack_links_against_the_engine_line_flux_web_is_built_on() {
    // Default options: this tool is never called here, only built and handed over. Nothing in this
    // test opens a socket.
    let egress = Egress::new(Arc::new(HttpRequestTool::new(&WebOptions::default())));

    // The host's store port and the pack's credential port are one trait, or this annotation is a
    // type error. `exchange_host::SecretStore` is `connector-secrets`', re-exported; `Credentials`
    // resolves it through `connector-pack`, which re-exports the same crate's.
    let store: Arc<dyn SecretStore> = Arc::new(connector_pack::MemoryStore::new());
    let credentials = Credentials::new(store, TENANT).expect("`acme` is a usable tenant");

    let configuration = Configuration::new(Arc::new(MemoryConfig::new()), TENANT)
        .expect("`acme` is a usable tenant");

    let mut registry = ToolRegistry::new();
    connector_pack::pack(&["zendesk"], egress, credentials, configuration)(&mut registry)
        .expect("the catalogue declares `zendesk` and its operations project");

    assert!(
        registry.get("zendesk.ticket.show").is_some(),
        "the pack installed nothing under the dotted name it exists to provide",
    );
}

/// The address this host derives is the address the pack resolves, and it has not moved.
///
/// Two facts in one test because they are one property. The annotation is the first: the
/// `CredentialRef` `ConnectorDeclaration::address_of` returns must be `connector-pack`'s, or a
/// derived address cannot be handed to the thing that fetches the secret at it — the failure X-11
/// exists to remove, in the vocabulary rather than in the engine.
///
/// The rendered string is the second, and it is why `connector-spec` 0.8 → `connector-address` 0.9
/// had to be read rather than search-and-replaced. That release also landed C-406's **instance
/// dimension**: `CredentialRef` grew an optional `@instances/<uuid>` level between the authority
/// and the service. It is opt-in through `CredentialRef::for_instance`, and `CredentialRef::new` —
/// what this host calls — still renders the un-instanced form byte for byte. That is asserted here
/// literally rather than trusted, because a silently widened address is a credential written where
/// nothing will look for it.
///
/// Wiring the instance level up is X-14 and deliberately not done here.
#[test]
fn the_address_this_host_derives_is_the_one_the_pack_resolves_and_has_not_moved() {
    let tenant = Tenant::new(TENANT).expect("`acme` is a usable tenant");
    let zendesk = ConnectorDeclaration {
        connector: "zendesk",
        authority: Some("com.zendesk.api"),
        credentials: &[DeclaredCredential {
            name: "zendesk.api_token",
            leaf: "api_token",
        }],
    };

    let derived = zendesk
        .address_of(&tenant, "zendesk.api_token")
        .expect("zendesk declares an authority and this credential");

    // One vocabulary, or this binding is a type error naming two identical-looking types.
    let as_the_pack_sees_it: &connector_pack::CredentialRef = &derived;

    // `default` elided, no instance segment: the address every stored credential is already at.
    assert_eq!(
        TenantLayout.render(as_the_pack_sees_it),
        "tenants/acme/com.zendesk.api/api_token",
        "the derived address moved, which strands every credential already stored at the old one",
    );

    // And the host's own renderer agrees with the pack's, since they are the same renderer.
    assert_eq!(
        address_path(&derived),
        TenantLayout.render(as_the_pack_sees_it)
    );
}

/// The workspace manifest, read at compile time — the file the engine line is written down in.
const WORKSPACE_MANIFEST: &str = include_str!("../../../Cargo.toml");

/// Every member manifest, so a second copy cannot be introduced out of sight of the first.
const MEMBER_MANIFESTS: &[(&str, &str)] = &[
    (
        "crates/exchange-host/Cargo.toml",
        include_str!("../Cargo.toml"),
    ),
    (
        "crates/exchange-server/Cargo.toml",
        include_str!("../../exchange-server/Cargo.toml"),
    ),
];

/// The engine line is recorded **once**, and this is what makes "once" a fact rather than a habit.
///
/// The Acceptance asks that the next bump be "a value change rather than an archaeology exercise".
/// A comment saying so is not that; Cargo has no variables, so several pins really do have to spell
/// the same number, and the thing that keeps them one value is a check. This is that check:
///
/// - the recorded value is the `ENGINE_LINE` marker in `[workspace.dependencies]`;
/// - every `codewandler-flux-*` pin in the workspace manifest must equal it;
/// - **no member manifest may pin one at all**, so the root manifest is the only place to look.
///
/// The failure it exists for is the quiet one: someone adds a flux crate at whatever is newest, the
/// workspace still builds because Cargo will happily carry two engine versions, and the breakage
/// surfaces later as a trait mismatch in whatever first tries to pass a tool across the seam.
#[test]
fn the_engine_line_is_recorded_in_exactly_one_place() {
    // The marker in assignment form, so that prose mentioning `ENGINE_LINE` in the comment around
    // it is not mistaken for the record itself.
    let recorded = WORKSPACE_MANIFEST
        .lines()
        .find_map(|line| value_of(line, "# ENGINE_LINE"))
        .expect(
            "the workspace manifest records the engine line as `# ENGINE_LINE = \"<line>\"` in \
             `[workspace.dependencies]`",
        );

    let pins: Vec<(&str, &str)> = flux_pins(WORKSPACE_MANIFEST).collect();
    assert!(
        !pins.is_empty(),
        "no `codewandler-flux-*` pin found in the workspace manifest, so this test is asserting \
         nothing — either the engine dependencies moved or the scanner stopped matching them",
    );

    for (package, version) in &pins {
        assert_eq!(
            *version, recorded,
            "`{package}` is pinned at {version:?} while the recorded engine line is \
             {recorded:?} — two flux versions in one graph are two distinct traits with identical \
             names, which is the failure X-11 removed",
        );
    }

    for (path, manifest) in MEMBER_MANIFESTS {
        let stray: Vec<&str> = flux_pins(manifest).map(|(package, _)| package).collect();
        assert!(
            stray.is_empty(),
            "{path} pins {stray:?} directly; an engine pin belongs in the workspace manifest and \
             is inherited with `.workspace = true`, so that there stays one place to change it",
        );
    }
}

/// Every flux **engine** pin in one manifest, as `(package, version)`.
///
/// A `path` dependency is skipped because it is one of this workspace's own crates —
/// `codewandler-flux-exchange-host` shares the family prefix and carries the workspace's version,
/// which is a different number answering a different question.
fn flux_pins(manifest: &str) -> impl Iterator<Item = (&str, &str)> {
    manifest
        .lines()
        .filter(|line| !line.contains("path = "))
        .filter_map(|line| {
            let package = value_of(line, "package")?;
            package
                .starts_with("codewandler-flux-")
                .then(|| Some((package, value_of(line, "version")?)))
                .flatten()
        })
}

/// The value of a `key = "…"` pair on one manifest line.
///
/// Deliberately not a TOML parse: reading these lines needs no dependency, and the shapes it
/// accepts are the shapes this repository actually writes. A pin it cannot read is a pin it does
/// not check, which is why the caller asserts it found some.
fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (_, after) = line.split_once(&format!("{key} = \""))?;
    after.split_once('"').map(|(value, _)| value)
}
