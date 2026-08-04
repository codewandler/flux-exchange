use flux_exchange_release::{
    canonical, package_archive, select_compatible, transport, verify_archive, Platform, Protocols,
    ReleaseEntry,
};
use std::fs;

#[test]
fn noncanonical_json_refuses() {
    let result = canonical::parse_value(br#"{ "schema":"x"}"#, 32);
    assert!(result.is_err());
}

#[test]
fn selection_skips_a_newer_incompatible_release() {
    let supported = Protocols::v1();
    let mut incompatible = supported.clone();
    incompatible.supervisor = "exchange.supervisor-ready.v2".into();
    let releases = [
        ReleaseEntry::test("1.0.0", supported),
        ReleaseEntry::test("2.0.0", incompatible),
    ];
    let selected =
        select_compatible(&releases, &Protocols::v1()).expect("older compatible release");
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
