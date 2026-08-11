use flux_exchange_release::{Platform, SUPPORTED_TARGETS};
use std::path::Path;

const TARGETS: &[&str] = &["aarch64-unknown-linux-gnu", "x86_64-unknown-linux-gnu"];

#[test]
fn exchange_release_authority_is_exactly_two_linux_gnu_targets() {
    assert_eq!(SUPPORTED_TARGETS, TARGETS);
    for target in TARGETS {
        assert_eq!(
            Platform::from_target(target).expect("supported target"),
            Platform::Linux
        );
    }
    for target in [
        "aarch64-apple-darwin",
        "x86_64-apple-darwin",
        "x86_64-pc-windows-msvc",
        "x86_64-unknown-linux-musl",
        "i686-unknown-linux-gnu",
    ] {
        assert!(
            Platform::from_target(target).is_err(),
            "unsupported target {target} entered the product boundary"
        );
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let rows = std::fs::read_to_string(root.join("release-targets.tsv"))
        .expect("release target authority");
    assert_eq!(
        rows,
        concat!(
            "# target\\trunner\\tarchive-format\\texecutable\n",
            "aarch64-unknown-linux-gnu\tubuntu-24.04-arm\ttar.zst\tflux-exchange\n",
            "x86_64-unknown-linux-gnu\tubuntu-24.04\ttar.zst\tflux-exchange\n",
        )
    );

    let distribution = std::fs::read_to_string(root.join("dist-workspace.toml"))
        .expect("distribution target authority");
    assert!(distribution
        .contains("targets = [\"aarch64-unknown-linux-gnu\", \"x86_64-unknown-linux-gnu\"]"));
    for forbidden in ["apple-darwin", "windows-msvc", "zip", "flux-exchange.exe"] {
        assert!(
            !distribution.contains(forbidden) && !rows.contains(forbidden),
            "non-Linux release fact {forbidden} remains authoritative"
        );
    }
}

#[test]
fn release_scripts_refuse_every_non_linux_archive_shape() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for relative in [
        "scripts/release-package.sh",
        "scripts/release-check-assets.sh",
        "scripts/release-download.sh",
    ] {
        let source = std::fs::read_to_string(root.join(relative)).expect("release policy script");
        for forbidden in ["apple-darwin", "format = zip", "= zip", ".zip"] {
            assert!(
                !source.contains(forbidden),
                "{relative} retains non-Linux archive selector {forbidden}"
            );
        }
    }
}
