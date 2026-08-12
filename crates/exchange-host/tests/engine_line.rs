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
use connector_secrets::{
    CredentialRef, CredentialScope, MemoryStore, PreparedSecretError, PreparedSecretStore, Secret,
    SecretBatch, SecretProposalDigest, SecretTransactionGeneration, SecretTransactionId,
    SecretTransactionState,
};
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

/// The resolved graph, read at compile time — what the manifest above actually produced.
const WORKSPACE_LOCK: &str = include_str!("../../../Cargo.lock");

/// **The lock carries one engine line, not two** (X-67).
///
/// The test above reads the *manifest*, because that is where a pin is authored and where a bump is
/// done. This one reads the *lock*, because a manifest stating one line proves nothing about what
/// resolved: a dependency requiring `^0.49` alongside this workspace's `0.52` puts both in the tree,
/// Cargo writes both down without complaint, and the manifest is silent about it. That is the
/// failure the `ENGINE_LINE` comment records having reasoned about by hand for `connector-pack`,
/// and reasoning is what this replaces.
///
/// Stolen from flux-connectors' `crates/connector-cli/tests/flux_engine_line.rs`, which asserts the
/// same property over both graphs there. It is rewritten to read lines rather than to parse TOML,
/// following [`value_of`] below: `Cargo.lock` writes `name` and `version` one per line inside each
/// `[[package]]` block, so a parse would buy nothing and would cost a dependency.
///
/// **Only the `0.x` crates are the engine.** flux also publishes stable-vocabulary crates
/// (`codewandler-flux-spec`, `codewandler-flux-evidence`, `codewandler-flux-datasource`, …) on
/// `1.x` lines of their own, which are not versioned with the engine and must not be dragged onto
/// it. `codewandler-flux-exchange-host` is this workspace's own crate and carries the workspace
/// version, so it is excluded by the same rule that excludes it from [`flux_pins`] — a `0.9.0` that
/// answers a different question.
#[test]
fn the_lock_carries_one_engine_line() {
    let recorded = WORKSPACE_MANIFEST
        .lines()
        .find_map(|line| value_of(line, "# ENGINE_LINE"))
        .expect("the workspace manifest records the engine line");

    let locked: Vec<(&str, &str)> = locked_flux_packages().collect();
    assert!(
        !locked.is_empty(),
        "no `codewandler-flux-*` package found in `Cargo.lock`, so this test is reading the wrong \
         file and would pass whatever resolved",
    );

    let off_line: Vec<String> = locked
        .iter()
        .filter(|(_, version)| version.starts_with("0.") && !on_line(version, recorded))
        .map(|(package, version)| format!("{package} {version}"))
        .collect();

    assert!(
        off_line.is_empty(),
        "`Cargo.lock` resolves flux engine crates off the recorded line {recorded:?}: {off_line:?} \
         — a `0.x` requirement is semver-incompatible across minor versions, so each of these is a \
         second, separate copy of flux in one build: two `flux_runtime::Tool` traits with identical \
         names, and a host that can link only one of them",
    );
}

/// X-134 consumes the released provider line from crates.io, with one copy of each connector
/// package and the exact audited archive checksums. A path/git source or stale lock is a contract
/// failure even when the public Rust signatures happen to compile.
///
/// **X-146 moved this to 0.21 and moved nothing else.** The checksums below were read out of the
/// crates.io sparse index before the manifest was edited, not copied out of the lock afterwards —
/// a checksum taken from the file under test proves only that the file agrees with itself.
#[test]
fn the_lock_carries_the_exact_registry_connector_021_line() {
    const EXPECTED: [(&str, &str); 4] = [
        (
            "codewandler-connector-address",
            "65dbef43261a0009f6c68843321ac36f06eab3b195eb2f10c33d98d0968343b9",
        ),
        (
            "codewandler-connector-catalog",
            "c88c13adb47b3a105eb0b1798b35a15978e5e61c752c53e8e44b7d874c9f6f0e",
        ),
        (
            "codewandler-connector-secrets",
            "5521a8afe9127c0e887beb540e0fc037dd434e86a80935e606551340b8f78cba",
        ),
        (
            "codewandler-connector-pack",
            "bfda4109b04d68c8dcb4265470fc96cc20fca3c811cbf2361eb1f65dcf86f2d0",
        ),
    ];

    for (name, checksum) in EXPECTED {
        let matches: Vec<&str> = WORKSPACE_LOCK
            .split("[[package]]")
            .filter(|block| {
                value_of(
                    block
                        .lines()
                        .find(|line| line.starts_with("name = "))
                        .unwrap_or(""),
                    "name",
                ) == Some(name)
            })
            .collect();
        assert_eq!(
            matches.len(),
            1,
            "Cargo.lock must contain exactly one `{name}` package"
        );
        let block = matches[0];
        assert_eq!(
            block.lines().find_map(|line| value_of(line, "version")),
            Some("0.21.0"),
            "`{name}` is not locked to the released 0.21.0 provider line",
        );
        assert_eq!(
            block.lines().find_map(|line| value_of(line, "source")),
            Some("registry+https://github.com/rust-lang/crates.io-index"),
            "`{name}` did not resolve from crates.io",
        );
        assert_eq!(
            block.lines().find_map(|line| value_of(line, "checksum")),
            Some(checksum),
            "`{name}` archive checksum differs from the independently published artifact",
        );
    }
}

/// The C-515 prepared transaction port is object-safe and shares one concrete store with ordinary
/// reads. This is the provider API Exchange must retain for the process lifetime; reopening a
/// second file store for coordinator recovery would violate that ownership boundary.
///
/// Deliberately not named after a connector line: the property is about the port, and a name
/// carrying a version number invites a rename on every release that does not change it.
#[tokio::test]
async fn the_prepared_secret_port_is_ready_for_exchange_composition() {
    let concrete = Arc::new(MemoryStore::new());
    let ordinary: Arc<dyn SecretStore> = concrete.clone();
    let prepared: Arc<dyn PreparedSecretStore> = concrete;
    let generation = SecretTransactionGeneration::from_protocol_bytes(1_u64.to_be_bytes())
        .expect("generation one is non-zero");
    let next = generation
        .checked_next()
        .expect("generation one has a successor");
    let first = SecretTransactionId::new(generation, [0x13; 24]);
    let aborted = SecretTransactionId::new(next, [0x27; 24]);
    let digest = SecretProposalDigest::from_protocol_bytes([0x52; 32]);
    let reference = CredentialRef::new("acme", "com.example.api", "primary", "token")
        .expect("static credential reference");
    let batch = SecretBatch::new(
        CredentialScope::new(reference.tenant(), reference.authority())
            .expect("static credential scope"),
    );

    ordinary
        .put(&reference, &Secret::new("x134-registry-readiness-sentinel"))
        .await
        .expect("ordinary write through the shared backing store");
    assert!(
        prepared.get(&reference).await.is_ok(),
        "the prepared port does not share the ordinary backing store"
    );
    assert_eq!(
        prepared.state(first).await.map_err(prepared_error_kind),
        Ok(SecretTransactionState::Absent)
    );
    assert_eq!(
        prepared
            .prepare(first, digest, &batch)
            .await
            .map_err(prepared_error_kind),
        Ok(SecretTransactionState::Prepared)
    );
    assert_eq!(
        prepared.commit(first).await.map_err(prepared_error_kind),
        Ok(SecretTransactionState::Committed)
    );
    assert_eq!(
        prepared.abort(aborted).await.map_err(prepared_error_kind),
        Ok(SecretTransactionState::Absent)
    );
    assert_eq!(
        prepared
            .prepare(aborted, digest, &batch)
            .await
            .map_err(prepared_error_kind),
        Err("transaction_id_reused")
    );
    prepared
        .reclaim(generation)
        .await
        .map_err(prepared_error_kind)
        .expect("generation one can be reclaimed");
    assert_eq!(
        prepared.state(first).await.map_err(prepared_error_kind),
        Err("retired")
    );
}

/// Exhaustive mapping is intentional: a new provider refusal must make this consumer update its
/// handling instead of being silently folded into an assumed closed set.
fn prepared_error_kind(error: PreparedSecretError) -> &'static str {
    match error {
        PreparedSecretError::Unsupported => "unsupported",
        PreparedSecretError::Busy => "busy",
        PreparedSecretError::DigestMismatch => "digest_mismatch",
        PreparedSecretError::TransactionIdReused => "transaction_id_reused",
        PreparedSecretError::NotPrepared => "not_prepared",
        PreparedSecretError::AlreadyCommitted => "already_committed",
        PreparedSecretError::Retired => "retired",
        PreparedSecretError::Capacity => "capacity",
        PreparedSecretError::InvalidBatch => "invalid_batch",
        PreparedSecretError::Backend => "backend",
    }
}

/// Every `codewandler-flux-*` entry in the lock, as `(name, version)`, this workspace's own excluded.
///
/// A `Vec`-shaped walk rather than a map keyed by name, because the whole question is whether a
/// package appears more than once — collapsing duplicates would delete the finding.
fn locked_flux_packages() -> impl Iterator<Item = (&'static str, &'static str)> {
    WORKSPACE_LOCK.split("[[package]]").filter_map(|block| {
        let name = block.lines().find_map(|line| value_of(line, "name"))?;
        // This workspace's own crate shares the family prefix and carries the workspace version.
        if !name.starts_with("codewandler-flux-") || name == "codewandler-flux-exchange-host" {
            return None;
        }
        Some((
            name,
            block.lines().find_map(|line| value_of(line, "version"))?,
        ))
    })
}

/// Whether `version` (`0.52.1`) falls inside `line` (`0.52`).
///
/// A prefix comparison rather than a semver parse, for the same reason [`value_of`] is not a TOML
/// parse: the line *is* the recorded prefix, and nothing here needs to order two versions.
fn on_line(version: &str, line: &str) -> bool {
    version == line || version.starts_with(&format!("{line}."))
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
            let version = value_of(line, "version")?;
            // Stable vocabulary crates follow independent 1.x lines. Only 0.x packages are engine
            // traits whose minor version is a compatibility boundary.
            (package.starts_with("codewandler-flux-") && version.starts_with("0."))
                .then_some((package, version))
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
