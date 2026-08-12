//! X-158 — the resolved provider artifact is the one the C-515 evidence actually describes.
//!
//! `native-evidence-v1.json` binds the inherited C-515 obligations to one *published artifact*: a
//! version, the registry SHA-256 of its `.crate` bytes, and the upstream commit that released them.
//! Publication then asserts those obligations about the binary this workspace resolves. If
//! `Cargo.lock` moves to a different connector-secrets release and the authority stays behind, the
//! release still publishes — and it publishes evidence about a `FileStore` that is not the one
//! shipped.
//!
//! That is not hypothetical. X-146 moved the workspace to 0.21 and X-155 to 0.23 while the authority
//! held 0.20.0, every gate stayed green, and the disagreement surfaced only when the `v0.18.0` tag
//! reached `check-publication-readiness.sh` — after the version number had been spent. Nothing in
//! the ordinary gate compared the two, because the checks that did exist each compared the lock to
//! their *own* copy of the expected line: `engine_line.rs` carries index-derived checksums and
//! `native_evidence.rs` compiles in the upstream triple, so both moved with the bump and neither
//! noticed the authority had not.
//!
//! This test is the missing edge. It fails in the pull request that bumps the pin, naming both
//! values and the re-derivation procedure, so the drift cannot reach a tag again.

use flux_exchange_release::native_evidence::{AuthorityClass, NativeEvidenceAuthority};

const WORKSPACE_LOCK: &str = include_str!("../../../Cargo.lock");

/// Where a failure here is repaired. Named once so the refusal can quote it.
const PROCEDURE: &str =
    "AGENTS.md § Moving a connector pin moves the C-515 evidence with it — re-derive \
     crates/exchange-release/native-evidence-v1.json in the same change";

#[test]
fn the_locked_provider_is_the_artifact_the_c515_authority_describes() {
    let authority = NativeEvidenceAuthority::bundled().expect("canonical native authority");
    let package = authority
        .authorities
        .values()
        .filter(|authority| authority.class == AuthorityClass::InheritedUpstream)
        .find_map(|authority| authority.package.as_ref())
        .expect("one inherited upstream package identity");

    let blocks: Vec<&str> = WORKSPACE_LOCK
        .split("[[package]]")
        .filter(|block| {
            block.lines().find_map(|line| value_of(line, "name")) == Some(package.name.as_str())
        })
        .collect();
    assert_eq!(
        blocks.len(),
        1,
        "Cargo.lock holds {} copies of {}, and the C-515 evidence can describe only one artifact. \
         {PROCEDURE}",
        blocks.len(),
        package.name,
    );
    let block = blocks[0];

    let locked_version = block
        .lines()
        .find_map(|line| value_of(line, "version"))
        .unwrap_or("<no version>");
    assert_eq!(
        locked_version, package.version,
        "Cargo.lock resolves {name} {locked_version}, but the C-515 evidence authority describes \
         {name} {authority_version}. Publishing this tree would assert the inherited C-515 \
         obligations — recovery, the lifetime lease, the upgrade fixture — about {authority_version} \
         while the binary ships {locked_version}. {PROCEDURE}",
        name = package.name,
        authority_version = package.version,
    );

    // The version alone is not the artifact. Two uploads can carry one version number only if one
    // was yanked and replaced, but a checksum is what actually names the bytes the evidence was read
    // out of — and it is the field a hand-edited authority is most likely to leave behind.
    let locked_checksum = block
        .lines()
        .find_map(|line| value_of(line, "checksum"))
        .unwrap_or("<no checksum>");
    assert_eq!(
        locked_checksum, package.registry_sha256,
        "Cargo.lock authenticates {name} {locked_version} as {locked_checksum}, but the C-515 \
         evidence authority pins {authority_checksum}. One of them was not read out of the \
         crates.io sparse index. {PROCEDURE}",
        name = package.name,
        authority_checksum = package.registry_sha256,
    );

    assert_eq!(
        block.lines().find_map(|line| value_of(line, "source")),
        Some("registry+https://github.com/rust-lang/crates.io-index"),
        "{} did not resolve from crates.io, so no registry checksum describes it and the C-515 \
         evidence has no published artifact to be about. {PROCEDURE}",
        package.name,
    );
}

fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let (_, after) = line.split_once(&format!("{key} = \""))?;
    after.split_once('"').map(|(value, _)| value)
}
