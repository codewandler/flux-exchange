use flux_exchange_release::{
    canonical, delegated_signing_key_id, package_archive, read_bounded_file, select_compatible,
    stage_manifest, transport, verify_archive, FixtureSet, Manifest, Platform, Protocols,
    ReleaseEntry, TrustDocument,
};
use serde::Deserialize;
use std::fs;
use std::io::Cursor;
use std::path::Path;
use std::process::Command;

#[test]
fn fixture_and_release_guards_are_derived_from_the_candidate_commit() {
    use flux_exchange_release::native_evidence::NativeEvidenceAuthority;

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture_path = root.join("tests/fixtures/exchange-release-v2/fixture-set.json");
    let fixture: FixtureSet = canonical::parse(
        &read_bounded_file(&fixture_path, 256 * 1024).expect("bounded fixture manifest"),
        256 * 1024,
    )
    .expect("canonical fixture manifest");
    let authority = NativeEvidenceAuthority::bundled().expect("canonical native authority");

    assert_eq!(
        fixture.native_evidence_sha256,
        authority.identity().expect("authority identity"),
        "the frozen fixture was not regenerated from the current canonical authority"
    );
    assert_eq!(
        canonical::encode(&fixture.native_cases).expect("frozen native projection"),
        canonical::encode(
            &authority
                .fixture_cases()
                .expect("derived native projection")
        )
        .expect("canonical derived projection"),
        "the frozen fixture carries a copied or stale native projection"
    );
    assert_eq!(fixture.exchange_commit.len(), 40, "full candidate SHA");
    assert!(
        fixture
            .exchange_commit
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "candidate SHA is not lowercase hexadecimal"
    );

    let head = git(&root, &["rev-parse", "HEAD"]);
    assert_ne!(
        fixture.exchange_commit, head,
        "the frozen fixture cannot name its own self-referential follow-up commit"
    );
    let ancestor = Command::new("git")
        .args([
            "merge-base",
            "--is-ancestor",
            &fixture.exchange_commit,
            &head,
        ])
        .current_dir(&root)
        .status()
        .expect("inspect candidate ancestry");
    assert!(
        ancestor.success(),
        "fixture source is not a committed ancestor"
    );

    let candidate_authority = git(
        &root,
        &[
            "show",
            &format!(
                "{}:crates/exchange-release/native-evidence-v1.json",
                fixture.exchange_commit
            ),
        ],
    );
    let candidate: NativeEvidenceAuthority = canonical::parse(
        candidate_authority.as_bytes(),
        flux_exchange_release::native_evidence::MAX_BYTES,
    )
    .expect("candidate authority");
    candidate.validate().expect("candidate authority contract");
    assert_eq!(
        candidate.identity().expect("candidate authority identity"),
        fixture.native_evidence_sha256,
        "fixture authority identity did not come from the named candidate"
    );
    assert_eq!(
        canonical::encode(&candidate.fixture_cases().expect("candidate projection"))
            .expect("canonical candidate projection"),
        canonical::encode(&fixture.native_cases).expect("canonical frozen projection"),
        "fixture selection did not come from the named candidate"
    );

    let candidate_generator = git(
        &root,
        &[
            "show",
            &format!(
                "{}:crates/exchange-release/examples/generate_fixtures.rs",
                fixture.exchange_commit
            ),
        ],
    );
    for required in [
        "FLUX_EXCHANGE_FIXTURE_SOURCE_COMMIT",
        "native_authority.identity()",
        "native_authority.fixture_cases()",
    ] {
        assert!(
            candidate_generator.contains(required),
            "candidate generator does not derive {required}"
        );
    }

    for relative in [
        "scripts/release-native-fixtures.sh",
        "scripts/check-publication-readiness.sh",
        ".github/workflows/ci.yml",
        ".github/workflows/local-release.yml",
    ] {
        let source = fs::read_to_string(root.join(relative)).expect("release guard source");
        assert!(
            source.contains("native-evidence-v1.json") || source.contains("native-authority"),
            "{relative} does not consume the canonical native authority"
        );
        for forbidden in [
            "EXPECTED_MATRIX_SHA256",
            "native_fixture_cases()",
            "native-evidence.tsv",
            "native_cases.tsv",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} retains duplicate native oracle {forbidden}"
            );
        }
    }

    let ci = fs::read_to_string(root.join(".github/workflows/ci.yml")).expect("CI workflow");
    assert_eq!(
        ci.matches("Execute every authority-selected native process proof exactly")
            .count(),
        1,
        "CI must invoke one authority-derived exact runner"
    );
    assert!(
        !ci.contains("--exact $name") && !ci.contains("--exact \"$name\""),
        "CI retains a hand-written exact-test inventory outside the authority runner"
    );
}

fn git(root: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(root)
        .output()
        .expect("run git for release fixture contract");
    assert!(
        output.status.success(),
        "git {arguments:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("git output is UTF-8")
        .trim()
        .to_owned()
}

#[test]
fn noncanonical_json_refuses() {
    let result = canonical::parse_value(br#"{ "schema":"x"}"#, 32);
    assert!(result.is_err());
}

#[test]
fn selection_skips_a_newer_incompatible_release() {
    let supported = Protocols::v2();
    let mut incompatible = supported.clone();
    incompatible.supervisor = "exchange.supervisor-ready.v3".into();
    let releases = [
        ReleaseEntry::test("1.0.0", supported),
        ReleaseEntry::test("2.0.0", incompatible),
    ];
    let selected =
        select_compatible(&releases, &Protocols::v2()).expect("older compatible release");
    assert_eq!(selected.version, "1.0.0");
}

#[test]
fn redirect_policy_is_closed() {
    let accepted = "https://release-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sp=r&sig=x";
    transport::validate_redirect(302, accepted, false).expect("documented redirect");
    assert!(transport::validate_redirect(
        302,
        &accepted.replace("release-assets", "objects"),
        false
    )
    .is_err());
    assert!(transport::validate_redirect(301, accepted, false).is_err());
    assert!(transport::validate_redirect(302, accepted, true).is_err());
    assert!(transport::validate_redirect(
        302,
        &accepted.replace("release-assets", "RELEASE-assets"),
        false
    )
    .is_err());
    transport::validate_redirect(302, &accepted.replace(".com/", ".com:443/"), false)
        .expect("explicit default port");
}

#[test]
fn github_initial_urls_are_closed() {
    use transport::InitialResource;

    transport::validate_initial_url(
        "https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json",
        InitialResource::Trust,
    )
    .expect("fixed trust URL");
    transport::validate_initial_url(
        "https://github.com/codewandler/flux-exchange/releases/download/v0.17.0/flux-exchange-release-manifest.json",
        InitialResource::Immutable {
            version: "0.17.0",
            basename: "flux-exchange-release-manifest.json",
        },
    )
    .expect("immutable tag URL");
    assert!(transport::validate_initial_url(
        "https://github.com/codewandler/flux-exchange/releases/latest/download/flux-exchange-release-manifest.json",
        InitialResource::Immutable {
            version: "0.17.0",
            basename: "flux-exchange-release-manifest.json",
        },
    )
    .is_err());
}

#[test]
fn rust_and_download_validator_refuse_the_same_raw_query_character() {
    let url = "https://release-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sig=raw[value";
    assert!(transport::validate_redirect(302, url, false).is_err());

    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts/release-validate-redirect.py");
    let status = Command::new("python3")
        .arg(script)
        .arg(url)
        .status()
        .expect("run download redirect validator");
    assert!(!status.success());
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RedirectFixture {
    status: u16,
    location: String,
    forwarded_credentials: bool,
    final_status: u16,
    second_redirect: bool,
}

#[test]
fn rust_and_python_transport_admission_match_the_shared_fixture_inventory() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures = root.join("tests/fixtures/exchange-release-v2");
    let set: FixtureSet = canonical::parse(
        &read_bounded_file(&fixtures.join("fixture-set.json"), 256 * 1024)
            .expect("bounded fixture manifest"),
        256 * 1024,
    )
    .expect("fixture manifest");
    let script = root.join("scripts/release-validate-redirect.py");
    let mut compared = 0;
    for case in set
        .cases
        .iter()
        .filter(|case| case.operation == "transport")
    {
        let path = fixtures.join(&case.input);
        let fixture: RedirectFixture = canonical::parse(
            &read_bounded_file(&path, 16 * 1024).expect("bounded redirect fixture"),
            16 * 1024,
        )
        .expect("redirect fixture");
        let rust = transport::validate_redirect(
            fixture.status,
            &fixture.location,
            fixture.forwarded_credentials,
        )
        .and_then(|()| transport::validate_final(fixture.final_status, fixture.second_redirect))
        .is_ok();
        let python = Command::new("python3")
            .arg(&script)
            .arg("--fixture")
            .arg(&path)
            .status()
            .expect("run Python redirect validator")
            .success();
        assert_eq!(rust, python, "Rust/Python disagreement for {}", case.id);
        assert_eq!(rust, case.expected_result == "accepted", "{}", case.id);
        compared += 1;
    }
    assert_eq!(
        compared, 25,
        "transport fixture inventory changed unreviewed"
    );
}

#[test]
fn duplicate_members_and_unsafe_integers_refuse_before_mapping() {
    assert!(canonical::parse_value(br#"{"x":1,"x":1}"#, 64).is_err());
    assert!(canonical::parse_value(br#"{"x":9007199254740992}"#, 64).is_err());
}

#[test]
fn locked_packager_is_reproducible_and_archive_is_exact() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let inputs = temporary.path().join("inputs");
    let first = temporary.path().join("first");
    let second = temporary.path().join("second");
    fs::create_dir_all(&inputs).expect("inputs");
    fs::write(inputs.join("flux-exchange"), b"binary").expect("executable");
    fs::write(inputs.join("LICENSE-APACHE"), b"apache").expect("Apache license");
    fs::write(inputs.join("LICENSE-MIT"), b"mit").expect("MIT license");
    let licenses = [inputs.join("LICENSE-APACHE"), inputs.join("LICENSE-MIT")];
    let left = package_archive(
        "1.2.3",
        "x86_64-unknown-linux-gnu",
        &inputs.join("flux-exchange"),
        &licenses,
        None,
        &first,
    )
    .expect("first package");
    let right = package_archive(
        "1.2.3",
        "x86_64-unknown-linux-gnu",
        &inputs.join("flux-exchange"),
        &licenses,
        None,
        &second,
    )
    .expect("second package");
    assert_eq!(
        fs::read(first.join(&left.archive)).expect("left"),
        fs::read(second.join(&right.archive)).expect("right")
    );
    let expected = std::iter::once((
        &left.executable.path,
        left.executable.bytes,
        &left.executable.sha256,
    ))
    .chain(
        left.other_members
            .iter()
            .map(|member| (&member.path, member.bytes, &member.sha256)),
    )
    .map(|(path, bytes, digest)| (path.clone(), bytes, digest.clone()))
    .collect::<Vec<_>>();
    verify_archive(
        &first.join(&left.archive),
        "tar.zst",
        Platform::Unix,
        &expected,
    )
    .expect("exact archive");
    let mut corrupt = fs::read(first.join(&left.archive)).expect("archive");
    corrupt.push(0);
    fs::write(first.join("trailing.tar.zst"), corrupt).expect("corrupt archive");
    assert!(verify_archive(
        &first.join("trailing.tar.zst"),
        "tar.zst",
        Platform::Unix,
        &expected
    )
    .is_err());
}

#[test]
fn archive_paths_refuse_traversal_before_extraction() {
    let result = verify_archive(
        std::path::Path::new("missing.zip"),
        "zip",
        Platform::Windows,
        &[],
    );
    assert!(result.is_err());
}

#[test]
fn archive_member_modes_are_the_exact_packager_modes() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let path = temporary.path().join("wrong-mode.tar.zst");
    let payload = b"binary";
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o777);
        header.set_uid(0);
        header.set_gid(0);
        header.set_mtime(0);
        header.set_cksum();
        builder
            .append_data(&mut header, "root/flux-exchange", Cursor::new(payload))
            .expect("tar member");
        builder.finish().expect("tar finish");
    }
    fs::write(
        &path,
        zstd::stream::encode_all(Cursor::new(tar_bytes), 19).expect("zstd"),
    )
    .expect("archive");
    let expected = vec![(
        "root/flux-exchange".to_owned(),
        payload.len() as u64,
        flux_exchange_release::digest_hex(payload),
    )];
    assert!(verify_archive(&path, "tar.zst", Platform::Unix, &expected).is_err());
}

#[test]
fn raw_zip_member_names_must_be_utf8() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let executable = temporary.path().join("flux-exchange.exe");
    let apache = temporary.path().join("LICENSE-APACHE");
    let mit = temporary.path().join("LICENSE-MIT");
    fs::write(&executable, b"binary").expect("executable");
    fs::write(&apache, b"apache").expect("license");
    fs::write(&mit, b"mit").expect("license");
    let asset = package_archive(
        "1.2.3",
        "x86_64-pc-windows-msvc",
        &executable,
        &[apache, mit],
        None,
        temporary.path(),
    )
    .expect("package");
    let archive = temporary.path().join(&asset.archive);
    let mut bytes = fs::read(&archive).expect("zip");
    let needle = b"LICENSE-APACHE";
    let mut mutations = 0;
    for index in 0..=bytes.len() - needle.len() {
        if bytes[index..].starts_with(needle) {
            bytes[index] = 0xff;
            mutations += 1;
        }
    }
    assert_eq!(mutations, 2, "local and central ZIP names");
    fs::write(&archive, bytes).expect("mutated ZIP");
    let expected = std::iter::once((
        asset.executable.path,
        asset.executable.bytes,
        asset.executable.sha256,
    ))
    .chain(
        asset
            .other_members
            .into_iter()
            .map(|member| (member.path, member.bytes, member.sha256)),
    )
    .collect::<Vec<_>>();
    let error = verify_archive(&archive, "zip", Platform::Windows, &expected)
        .expect_err("invalid raw ZIP name");
    assert!(error
        .to_string()
        .contains("raw ZIP member path is not UTF-8"));
}

#[test]
fn bounded_reader_and_support_member_contract_refuse_widening() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let oversized = temporary.path().join("oversized");
    fs::write(&oversized, b"12345").expect("oversized input");
    assert!(read_bounded_file(&oversized, 4).is_err());

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/exchange-release-v2/positive");
    let mut manifest: Manifest = canonical::parse(
        &read_bounded_file(
            &fixtures.join("flux-exchange-release-manifest.json"),
            256 * 1024,
        )
        .expect("fixture manifest"),
        256 * 1024,
    )
    .expect("manifest");
    manifest.assets[0].other_members[0].path = manifest.assets[0].other_members[0]
        .path
        .replace("LICENSE-APACHE", "NOTICE");
    assert!(stage_manifest(&fixtures, &manifest).is_err());
}

#[test]
fn packager_refuses_documentation_under_an_unadmitted_basename() {
    let temporary = tempfile::tempdir().expect("tempdir");
    let executable = temporary.path().join("flux-exchange");
    let apache = temporary.path().join("LICENSE-APACHE");
    let mit = temporary.path().join("LICENSE-MIT");
    let documentation = temporary.path().join("guide.md");
    fs::write(&executable, b"binary").expect("executable");
    fs::write(&apache, b"apache").expect("license");
    fs::write(&mit, b"mit").expect("license");
    fs::write(&documentation, b"guide").expect("documentation");
    assert!(package_archive(
        "1.2.3",
        "x86_64-unknown-linux-gnu",
        &executable,
        &[apache, mit],
        Some(&documentation),
        temporary.path(),
    )
    .is_err());
}

#[test]
fn delegated_signer_uses_a_half_open_validity_interval() {
    let trust_path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/exchange-release-v2/positive/flux-exchange-release-trust.json");
    let trust: TrustDocument = canonical::parse(
        &read_bounded_file(&trust_path, 64 * 1024).expect("trust bytes"),
        64 * 1024,
    )
    .expect("trust");
    let key = &trust.roles.release.keys[0];
    let before = flux_exchange_release::parse_utc(&key.not_before).expect("not_before");
    let after = flux_exchange_release::parse_utc(&key.not_after).expect("not_after");
    assert_eq!(
        delegated_signing_key_id(&trust, "release", &key.minisign_public_key, before)
            .expect("inclusive lower bound"),
        key.key_id
    );
    assert!(delegated_signing_key_id(&trust, "release", &key.minisign_public_key, after).is_err());
}
