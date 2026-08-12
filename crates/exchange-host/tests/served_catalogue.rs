//! **Which catalogue is being served, and what it refuses to serve** (X-153).
//!
//! Decision 0022 makes the catalogue a *data* dependency rather than a code one: a deployment may
//! point this host at a pack on disk and serve a catalogue newer than the binary it was built with,
//! so a new provider stops requiring an Exchange release. The capability is only worth having if
//! the refusals are trustworthy, and that is what this file is about.
//!
//! # The seam
//!
//! [`exchange_host::ServedCatalogue`] is the one answer to *"which catalogue is answering"*. It is
//! constructed **once** at startup — embedded, or loaded from a configured path — and every surface
//! that reports or resolves through the catalogue reads it rather than deciding for itself. A
//! per-call-site choice between "the embedded pack" and "the loaded pack" is how two surfaces come
//! to answer the same question differently, and an operator debugging a missing operation cannot
//! tell which one lied.
//!
//! # What a refusal must be
//!
//! *Refuse; never repair.* Every failure below is a refusal **before a single record is served**,
//! and none of them falls back to the embedded catalogue. That is the property worth testing rather
//! than asserting: a host that quietly served its built-in catalogue when the operator's pack was
//! unreadable would answer every request successfully with the wrong catalogue, and nothing about
//! the deployment would look wrong.
//!
//! The four the story names each refuse **distinguishably**, so an operator reads one remedy rather
//! than four possibilities — see [`CatalogueRefusal::check`](exchange_host::CatalogueRefusal::check)
//! for the word each names.

use exchange_host::{CatalogueOrigin, CatalogueRefusal, ServedCatalogue};

// ---------------------------------------------------------------------------------------------
// Fixtures — synthetic packs, digested independently of the code under test
// ---------------------------------------------------------------------------------------------

/// A synthetic version-1 pack around `body`, its digest computed with `sha2` rather than with the
/// reader's vendored SHA-256.
///
/// Independence is the point: a fixture digested by the implementation under test would agree with
/// it however wrong both were. This is the same construction the reader's own test suite uses, and
/// it is repeated here rather than imported because a consumer cannot import another crate's tests
/// — which is also what makes it an independent check rather than the same one run twice.
fn with_digest(body: &str) -> Vec<u8> {
    use sha2::{Digest, Sha256};

    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(body.as_bytes()) {
        hex.push_str(&format!("{byte:02x}"));
    }
    format!("flux-connectors-catalog-pack 1\ndigest sha256 {hex}\n{body}").into_bytes()
}

/// The body of a well-formed one-provider, one-operation pack at `schema`.
///
/// Small on purpose: nothing here is testing the catalogue's contents, only whether a pack is
/// served or refused, and a fixture whose contents matter is one that has to be regenerated every
/// time the real catalogue moves.
fn body_at_schema(schema: u32) -> String {
    let payload = "{\"id\":\"acme\"}\n";
    format!(
        "schema {schema}\nproviders 1\noperations 1\n\
         p acme 0 {len}\no acme-thing-get acme default 0 {len}\npayload {len}\n{payload}",
        len = payload.len(),
    )
}

/// Write `bytes` into a fresh temporary file and hand back its path.
///
/// Hand-rolled rather than a `tempfile` dependency: this crate's dependency table is an allow-list
/// (`tests/no_second_request_path.rs`) and a dev-dependency added for four tests is still a
/// dependency somebody has to read a reason for.
fn pack_file(name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let directory =
        std::env::temp_dir().join(format!("flux-exchange-x153-{}-{name}", std::process::id(),));
    std::fs::create_dir_all(&directory).expect("a writable temporary directory");
    let path = directory.join("catalog.pack");
    std::fs::write(&path, bytes).expect("the fixture pack is writable");
    path
}

// ---------------------------------------------------------------------------------------------
// The embedded catalogue, read through the reader
// ---------------------------------------------------------------------------------------------

/// **The default is the embedded pack, and it reports itself as such** (acceptance 1).
///
/// A deployment that configures nothing serves what it was built with, exactly as it did before
/// this story — and it can now say so, with the digest that identifies which catalogue that is.
#[test]
fn the_default_served_catalogue_is_the_embedded_pack() {
    let served = ServedCatalogue::embedded();

    assert_eq!(served.origin(), CatalogueOrigin::Embedded);
    assert_eq!(served.path(), None, "the embedded pack is not at a path");
    assert_eq!(
        served.digest().len(),
        64,
        "a pack's identity is its lowercase-hex SHA-256: {:?}",
        served.digest(),
    );
    assert!(
        served.digest().chars().all(|c| c.is_ascii_hexdigit()),
        "the digest is hex: {:?}",
        served.digest(),
    );
    assert!(
        served.provider_count() > 0 && served.operation_count() > 0,
        "the embedded pack serves {} providers and {} operations",
        served.provider_count(),
        served.operation_count(),
    );
}

/// **Reading the catalogue through the reader is not a different catalogue** (acceptance 1).
///
/// The golden in `tests/golden/catalogue-configuration-surface.txt` proves the *configuration
/// surface* did not move. This proves the other half, and it is the half a golden cannot see: the
/// pack the reader serves lists exactly the providers and operations the generated `&'static`
/// tables do. Two catalogue views in one binary that disagreed about which operations exist is the
/// failure this makes impossible to ship quietly.
///
/// It is an identity check rather than a contents check, deliberately. The typed tables still carry
/// `Operation::flux` — emitted Flux text the canonical documents replaced with a request template —
/// so the two views are not byte-comparable and will not be until upstream retires the emitter
/// (C-540). What they must agree on today is *what exists*.
#[test]
fn the_reader_and_the_typed_tables_serve_the_same_catalogue() {
    let served = ServedCatalogue::embedded();

    let mut from_reader = served.provider_ids();
    from_reader.sort_unstable();
    let mut from_tables: Vec<&str> = connector_catalog::providers()
        .iter()
        .map(|provider| provider.id)
        .collect();
    from_tables.sort_unstable();

    assert_eq!(
        from_reader, from_tables,
        "the embedded pack and the generated tables disagree about which providers exist",
    );

    let mut operations_from_reader = served.operation_ids();
    operations_from_reader.sort_unstable();
    let mut operations_from_tables: Vec<&str> = connector_catalog::operations()
        .map(|operation| operation.id)
        .collect();
    operations_from_tables.sort_unstable();

    assert_eq!(
        operations_from_reader, operations_from_tables,
        "the embedded pack and the generated tables disagree about which operations exist",
    );
}

/// **The canonical document is reachable through the seam**, which is what makes it load-bearing.
///
/// X-154 recorded the one declared fact the generated tables do not carry: `OAuth2::endpoint` names
/// a service, and only the *default* service has a `base_url` in those tables, so GitLab's `login`
/// endpoint base is refused by name rather than guessed. The document carries it under
/// `services[].base_url`, and resolving it *through the catalogue being served* is X-154 round 2.
/// This asserts the accessor that round needs exists and answers from the served pack.
#[test]
fn the_seam_serves_each_provider_its_canonical_document() {
    let served = ServedCatalogue::embedded();

    let document = served
        .provider_document("gitlab")
        .expect("gitlab is in the released catalogue");
    assert!(
        document.contains("\"services\""),
        "a provider's document carries its services",
    );
    assert!(
        served.provider_document("no-such-vendor").is_none(),
        "an unknown provider is absent rather than invented",
    );
}

// ---------------------------------------------------------------------------------------------
// Loading a pack from a path
// ---------------------------------------------------------------------------------------------

/// **A deployment may point this host at a pack on disk, and what it serves is that pack.**
///
/// The assertion that matters is the last one. A `load` that verified the file and then went on
/// serving the embedded catalogue would satisfy every refusal test in this file, report `Loaded`,
/// and answer every query with the wrong catalogue — so the digest of what is served must be the
/// digest of what was loaded, and it must not be the embedded one.
#[test]
fn a_configured_pack_is_loaded_and_is_the_one_served() {
    let bytes = with_digest(&body_at_schema(1));
    let path = pack_file("loaded", &bytes);

    let served = ServedCatalogue::load(&path).expect("a well-formed version-1 pack loads");

    assert_eq!(served.origin(), CatalogueOrigin::Loaded);
    assert_eq!(served.path(), Some(path.as_path()));
    assert_eq!(served.provider_ids(), vec!["acme"]);
    assert_eq!(served.operation_ids(), vec!["acme-thing-get"]);
    assert_ne!(
        served.digest(),
        ServedCatalogue::embedded().digest(),
        "the loaded catalogue is being served, not the embedded one under a `Loaded` label",
    );
}

/// **Additive minor growth loads** (acceptance 3, second half).
///
/// The pack's schema version is one integer and the document schema's own rule is what decides
/// compatibility: a change a reader must not ignore is a *version* bump, and everything else —
/// an unknown header line, an unknown index-row kind — is additive and must keep working. A reader
/// that refused those would make every additive catalogue release a coordinated one, which is the
/// whole thing Decision 0022 removes.
#[test]
fn an_additively_grown_pack_still_loads() {
    let payload = "{\"id\":\"acme\"}\n";
    let body = format!(
        "schema 1\nflavor experimental\nproviders 1\noperations 1\n\
         p acme 0 {len}\no acme-thing-get acme default 0 {len}\n\
         e acme-event acme 7 4\npayload {len}\n{payload}",
        len = payload.len(),
    );
    let path = pack_file("additive", &with_digest(&body));

    let served = ServedCatalogue::load(&path)
        .expect("an unknown header line and an unknown row kind are additive, not fatal");

    assert_eq!(served.provider_ids(), vec!["acme"]);
    assert_eq!(
        served.provider_document("acme"),
        Some(payload),
        "additive growth does not disturb the records a version-1 consumer asks for",
    );
}

// ---------------------------------------------------------------------------------------------
// The four refusals — each distinguishable, none falling back
// ---------------------------------------------------------------------------------------------

/// **A path that does not exist is refused, naming the path** (acceptance 6).
#[test]
fn a_pack_that_is_not_there_is_refused() {
    let missing = std::env::temp_dir().join("flux-exchange-x153-no-such-catalog.pack");
    let _ = std::fs::remove_file(&missing);

    let refusal = ServedCatalogue::load(&missing)
        .expect_err("a configured pack that is not there must refuse, never fall back");

    assert!(
        matches!(refusal, CatalogueRefusal::Unreadable { .. }),
        "a missing file is a readability refusal, not a corruption one: {refusal:?}",
    );
    assert_eq!(refusal.check(), "readable");
    assert!(
        refusal.to_string().contains("no-such-catalog.pack"),
        "the refusal names the path the operator configured: {refusal}",
    );
}

/// **A truncated pack is refused** (acceptance 6), and the two ways a file gets cut short are
/// recorded rather than collapsed.
///
/// A cut *inside the header* leaves a file that never states its own digest, so it is refused as
/// structurally incomplete. A cut in the payload leaves a well-formed header whose digest no longer
/// describes the bytes under it, so it is refused by the digest instead — which is the honest
/// statement of what catches it, and it is caught either way. Neither serves a record.
#[test]
fn a_truncated_pack_is_refused() {
    let whole = with_digest(&body_at_schema(1));

    // Cut inside the digest line: the file ends before it says what it should hash to.
    let path = pack_file("truncated-header", &whole[..40]);
    let refusal = ServedCatalogue::load(&path)
        .expect_err("a pack cut short in its header must refuse, never fall back");
    assert!(
        matches!(refusal, CatalogueRefusal::Incomplete { .. }),
        "a header that ends early is a structural refusal: {refusal:?}",
    );
    assert_eq!(refusal.check(), "structure");

    // Cut in the payload: the header is intact and the digest above it is now a lie.
    let path = pack_file("truncated-payload", &whole[..whole.len() - 6]);
    let refusal = ServedCatalogue::load(&path)
        .expect_err("a pack cut short in its payload must refuse, never fall back");
    assert!(
        matches!(refusal, CatalogueRefusal::DigestMismatch { .. }),
        "a truncated payload is caught by the digest: {refusal:?}",
    );
}

/// **A pack whose bytes disagree with its own digest is refused, before any record** (acceptance 6).
///
/// Same length, one byte different — so nothing structural notices and only the digest can. This is
/// the case that decides whether verification really precedes service.
#[test]
fn a_digest_mismatch_is_refused() {
    let mut bytes = with_digest(&body_at_schema(1));
    let last = bytes.len() - 2;
    bytes[last] = if bytes[last] == b'x' { b'y' } else { b'x' };

    let path = pack_file("tampered", &bytes);
    let refusal = ServedCatalogue::load(&path)
        .expect_err("bytes that disagree with their own digest must refuse, never fall back");

    let CatalogueRefusal::DigestMismatch {
        stated, computed, ..
    } = &refusal
    else {
        panic!("a tampered payload is a digest refusal: {refusal:?}");
    };
    assert_ne!(stated, computed, "the refusal names both digests");
    assert_eq!(refusal.check(), "digest");
    assert!(
        refusal.to_string().contains(stated.as_str())
            && refusal.to_string().contains(computed.as_str()),
        "the refusal names what the pack claims and what it actually is: {refusal}",
    );
}

/// **A major schema version this build does not understand is refused, naming both versions**
/// (acceptance 3, first half, and acceptance 6).
///
/// The schema line sits *inside* the digested region, so this is a well-formed, digest-verified
/// pack that is refused anyway. Fail closed: a record served from a schema this build does not
/// understand is a record it cannot vouch for, and vouching for it is the whole job.
#[test]
fn a_schema_version_this_build_does_not_serve_is_refused_naming_both() {
    let path = pack_file("newer-schema", &with_digest(&body_at_schema(2)));

    let refusal = ServedCatalogue::load(&path)
        .expect_err("a schema this build does not serve must refuse, never fall back");

    let CatalogueRefusal::UnsupportedSchema {
        found, supported, ..
    } = &refusal
    else {
        panic!("a newer schema is a version refusal: {refusal:?}");
    };
    assert_eq!(*found, 2);
    assert_eq!(*supported, exchange_host::SUPPORTED_CATALOGUE_SCHEMA);
    assert_eq!(refusal.check(), "schema-version");

    let message = refusal.to_string();
    assert!(
        message.contains("2") && message.contains(&supported.to_string()),
        "the refusal names the version the pack carries and the one this build serves: {message}",
    );
}

/// **A newer container format is refused by name**, distinctly from a newer schema.
///
/// Two versions, two remedies. A newer *format* means this binary's reader cannot parse the file at
/// all; a newer *schema* means it parsed it and will not vouch for what is inside. An operator
/// upgrades the same way, but a refusal that could not tell them apart would be one nobody could
/// report upstream usefully.
#[test]
fn a_newer_container_format_is_refused_distinctly_from_a_newer_schema() {
    let whole = String::from_utf8(with_digest(&body_at_schema(1))).expect("a pack is text");
    let newer = whole.replacen(
        "flux-connectors-catalog-pack 1",
        "flux-connectors-catalog-pack 2",
        1,
    );
    let path = pack_file("newer-format", newer.as_bytes());

    let refusal = ServedCatalogue::load(&path)
        .expect_err("a container format this build does not implement must refuse");

    assert!(
        matches!(
            refusal,
            CatalogueRefusal::UnsupportedFormat { found: 2, .. }
        ),
        "a newer container format is its own refusal: {refusal:?}",
    );
    assert_eq!(refusal.check(), "container-format");
}

/// **Something that is not a pack at all is refused**, rather than read as an empty catalogue.
///
/// An empty catalogue is the most dangerous successful answer this host could give: every operation
/// becomes "not in the catalogue", which reads as a connector that was removed rather than as a
/// configuration mistake.
#[test]
fn a_file_that_is_not_a_pack_is_refused() {
    let path = pack_file("not-a-pack", b"{\"this\": \"is json, not a pack\"}\n");

    let refusal = ServedCatalogue::load(&path).expect_err("a non-pack file must refuse");

    assert!(
        matches!(refusal, CatalogueRefusal::NotAPack { .. }),
        "a file that is not a pack is refused as such: {refusal:?}",
    );
    assert_eq!(refusal.check(), "container-format");
}

/// **Every refusal names a distinct failed check**, which is what "distinguishably" has to mean.
///
/// Written as a set rather than as four separate assertions because the property is about the
/// *whole* taxonomy: two refusals that collapsed onto one word would each still pass their own
/// test, and an operator would be told to look in the wrong place.
#[test]
fn the_four_refusals_name_four_different_checks() {
    let missing = std::env::temp_dir().join("flux-exchange-x153-absent.pack");
    let _ = std::fs::remove_file(&missing);
    let whole = with_digest(&body_at_schema(1));

    let mut tampered = whole.clone();
    let last = tampered.len() - 2;
    tampered[last] = if tampered[last] == b'x' { b'y' } else { b'x' };

    let paths = [
        missing,
        pack_file("set-truncated", &whole[..40]),
        pack_file("set-tampered", &tampered),
        pack_file("set-schema", &with_digest(&body_at_schema(2))),
    ];

    let checks: Vec<&'static str> = paths
        .iter()
        .map(|path| {
            ServedCatalogue::load(path)
                .expect_err("each of the four fixtures refuses")
                .check()
        })
        .collect();

    let mut unique = checks.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(
        unique.len(),
        4,
        "the four refusals must be distinguishable, and these collapsed: {checks:?}",
    );
}

/// **No refusal path produces a served catalogue** — the property every test above depends on.
///
/// *Refuse; never repair.* A silent fall back to the embedded pack is the failure mode with no
/// symptom: the deployment starts, every request succeeds, and the answers come from a catalogue
/// nobody configured. The return type makes it structurally impossible, and this holds the type to
/// it over every fixture at once so a future convenience constructor cannot quietly reintroduce it.
#[test]
fn no_refusal_ever_falls_back_to_the_embedded_catalogue() {
    let whole = with_digest(&body_at_schema(1));
    let embedded = ServedCatalogue::embedded();

    let mut tampered = whole.clone();
    let last = tampered.len() - 2;
    tampered[last] = if tampered[last] == b'x' { b'y' } else { b'x' };

    let fixtures: Vec<(&str, Vec<u8>)> = vec![
        ("cut-header", whole[..40].to_vec()),
        ("cut-payload", whole[..whole.len() - 6].to_vec()),
        ("tampered", tampered),
        ("newer-schema", with_digest(&body_at_schema(2))),
        ("not-a-pack", b"not a pack at all\n".to_vec()),
    ];

    for (name, bytes) in fixtures {
        let path = pack_file(&format!("fallback-{name}"), &bytes);
        match ServedCatalogue::load(&path) {
            Err(_) => {}
            Ok(served) => panic!(
                "`{name}` was served rather than refused, as {:?} with digest {} (the embedded \
                 catalogue's is {}) — a host that repairs a bad pack by serving the one it was \
                 built with answers every request successfully from a catalogue nobody configured",
                served.origin(),
                served.digest(),
                embedded.digest(),
            ),
        }
    }
}
