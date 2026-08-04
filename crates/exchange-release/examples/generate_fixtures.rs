use flux_exchange_release as release;
use minisign::{sign, KeyPair};
use release::*;
use std::collections::BTreeMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};

fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/exchange-release-v1");
    if root.exists() {
        std::fs::remove_dir_all(&root)?;
    }
    let positive = root.join("positive");
    std::fs::create_dir_all(&positive)?;
    let scratch = std::env::temp_dir().join("flux-exchange-x126-fixture-inputs");
    std::fs::create_dir_all(&scratch)?;
    write(
        &scratch.join("flux-exchange"),
        b"test-only unix fixture executable\n",
    )?;
    write(
        &scratch.join("flux-exchange.exe"),
        b"test-only windows fixture executable\n",
    )?;
    write(
        &scratch.join("LICENSE-APACHE"),
        b"test-only Apache fixture license\n",
    )?;
    write(
        &scratch.join("LICENSE-MIT"),
        b"test-only MIT fixture license\n",
    )?;

    let root_key = KeyPair::generate_unencrypted_keypair()?;
    let old_channel = KeyPair::generate_unencrypted_keypair()?;
    let new_channel = KeyPair::generate_unencrypted_keypair()?;
    let release_key = KeyPair::generate_unencrypted_keypair()?;
    let root_id = "test-only-root-2026-01";
    let old_channel_id = "test-only-channel-2026-01";
    let new_channel_id = "test-only-channel-2026-02";
    let release_id = "test-only-release-2026-01";
    let issued = "2026-08-04T12:00:00Z";
    let expires = "2026-08-05T12:00:00Z";
    let trust_expires = "2027-08-04T12:00:00Z";
    let policy = RootPolicy {
        schema: "exchange.release-root-policy.v1".into(),
        threshold: 1,
        test_only: true,
        keys: vec![RootKey {
            key_id: root_id.into(),
            minisign_public_key: root_key.pk.to_base64(),
        }],
    };
    write(
        &root.join("root-policy.test.json"),
        &canonical::encode(&policy)?,
    )?;
    let trust = TrustDocument {
        schema: "exchange.release-trust.v1".into(),
        origin: ORIGIN.into(),
        version: 2,
        issued_at: issued.into(),
        expires_at: trust_expires.into(),
        root_signing_key_ids: vec![root_id.into()],
        roles: Roles {
            channel: Role {
                threshold: 1,
                keys: vec![
                    DelegatedKey {
                        key_id: old_channel_id.into(),
                        minisign_public_key: old_channel.pk.to_base64(),
                        not_before: issued.into(),
                        not_after: trust_expires.into(),
                    },
                    DelegatedKey {
                        key_id: new_channel_id.into(),
                        minisign_public_key: new_channel.pk.to_base64(),
                        not_before: issued.into(),
                        not_after: trust_expires.into(),
                    },
                ],
            },
            release: Role {
                threshold: 1,
                keys: vec![DelegatedKey {
                    key_id: release_id.into(),
                    minisign_public_key: release_key.pk.to_base64(),
                    not_before: issued.into(),
                    not_after: trust_expires.into(),
                }],
            },
        },
    };
    let trust_bytes = canonical::encode(&trust)?;
    write(
        &positive.join("flux-exchange-release-trust.json"),
        &trust_bytes,
    )?;
    sign_file(
        &root_key,
        &trust_bytes,
        &positive.join(format!(
            "flux-exchange-release-trust.json.{root_id}.minisig"
        )),
    )?;

    let version = "0.17.0";
    let licenses = [scratch.join("LICENSE-APACHE"), scratch.join("LICENSE-MIT")];
    let mut assets = Vec::new();
    for target in SUPPORTED_TARGETS {
        let executable = if target.contains("windows") {
            scratch.join("flux-exchange.exe")
        } else {
            scratch.join("flux-exchange")
        };
        assets.push(package_archive(
            version,
            target,
            &executable,
            &licenses,
            None,
            &positive,
        )?);
    }
    assets.sort_by(|left, right| left.target.cmp(&right.target));
    let manifest = Manifest {
        schema: "exchange.release-manifest.v1".into(),
        origin: ORIGIN.into(),
        tag: format!("refs/tags/v{version}"),
        version: version.into(),
        source_commit: "4e398a73dcb8de17466cbedea77122dd489bed4f".into(),
        build_id: "TEST-ONLY-X126-FIXTURE".into(),
        protocols: Protocols::v1(),
        signing_key_ids: vec![release_id.into()],
        assets,
    };
    let manifest_bytes = canonical::encode(&manifest)?;
    write(
        &positive.join("flux-exchange-release-manifest.json"),
        &manifest_bytes,
    )?;
    sign_file(
        &release_key,
        &manifest_bytes,
        &positive.join(format!(
            "flux-exchange-release-manifest.json.{release_id}.minisig"
        )),
    )?;
    let entry = ReleaseEntry {
        tag: manifest.tag.clone(),
        version: manifest.version.clone(),
        source_commit: manifest.source_commit.clone(),
        build_id: manifest.build_id.clone(),
        manifest_sha256: digest_hex(&manifest_bytes),
        release_key_ids: manifest.signing_key_ids.clone(),
        protocols: manifest.protocols.clone(),
    };
    let channel = Channel {
        schema: "exchange.release-channel.v1".into(),
        channel: "stable".into(),
        origin: ORIGIN.into(),
        generation: 7,
        issued_at: issued.into(),
        expires_at: expires.into(),
        signing_key_ids: vec![old_channel_id.into(), new_channel_id.into()],
        releases: vec![entry.clone()],
    };
    let channel_bytes = canonical::encode(&channel)?;
    write(
        &positive.join("flux-exchange-release-channel.json"),
        &channel_bytes,
    )?;
    sign_file(
        &old_channel,
        &channel_bytes,
        &positive.join(format!(
            "flux-exchange-release-channel.json.{old_channel_id}.minisig"
        )),
    )?;
    sign_file(
        &new_channel,
        &channel_bytes,
        &positive.join(format!(
            "flux-exchange-release-channel.json.{new_channel_id}.minisig"
        )),
    )?;
    let mut malformed_trust = trust.clone();
    malformed_trust.roles.channel.keys[0].minisign_public_key = "not-base64".into();
    signed_trust_variant(
        &root,
        "trust-key-malformed",
        &positive,
        &malformed_trust,
        &root_key,
        root_id,
    )?;
    let mut reused_trust = trust.clone();
    reused_trust.roles.release.keys[0].minisign_public_key = reused_trust.roles.channel.keys[0]
        .minisign_public_key
        .clone();
    signed_trust_variant(
        &root,
        "trust-key-reused",
        &positive,
        &reused_trust,
        &root_key,
        root_id,
    )?;
    let mut expired_delegation = trust.clone();
    expired_delegation.roles.channel.keys[0].not_after = issued.into();
    signed_trust_variant(
        &root,
        "delegation-expired",
        &positive,
        &expired_delegation,
        &root_key,
        root_id,
    )?;
    let mut role_confusion = trust.clone();
    std::mem::swap(
        &mut role_confusion.roles.channel.keys[0].key_id,
        &mut role_confusion.roles.release.keys[0].key_id,
    );
    signed_trust_variant(
        &root,
        "role-confusion",
        &positive,
        &role_confusion,
        &root_key,
        root_id,
    )?;

    let mut expired_channel = channel.clone();
    expired_channel.expires_at = issued.into();
    signed_channel_variant(
        &root,
        "channel-expired",
        &positive,
        &expired_channel,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut digest_channel = channel.clone();
    digest_channel.releases[0].manifest_sha256 = "b".repeat(64);
    signed_channel_variant(
        &root,
        "manifest-digest-substituted",
        &positive,
        &digest_channel,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut incompatible_channel = channel.clone();
    incompatible_channel.generation = 8;
    incompatible_channel.releases[0].protocols.supervisor = "exchange.supervisor-ready.v2".into();
    signed_channel_variant(
        &root,
        "higher-no-compatible",
        &positive,
        &incompatible_channel,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut multi_channel = channel.clone();
    multi_channel.generation = 11;
    let mut newer = entry.clone();
    newer.version = "0.18.0".into();
    newer.tag = "refs/tags/v0.18.0".into();
    newer.source_commit = "5e398a73dcb8de17466cbedea77122dd489bed4f".into();
    newer.manifest_sha256 = "d".repeat(64);
    newer.protocols.supervisor = "exchange.supervisor-ready.v2".into();
    multi_channel.releases.push(newer);
    signed_channel_variant(
        &root,
        "selection-multi",
        &positive,
        &multi_channel,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut too_many = channel.clone();
    too_many.generation = 9;
    too_many.releases.clear();
    for index in 0..129u64 {
        let mut item = entry.clone();
        item.version = format!("1.0.{index}");
        item.tag = format!("refs/tags/v{}", item.version);
        item.source_commit = format!("{index:040x}");
        item.manifest_sha256 = format!("{:064x}", index + 1);
        too_many.releases.push(item);
    }
    signed_channel_variant(
        &root,
        "channel-129",
        &positive,
        &too_many,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut higher_trust = trust.clone();
    higher_trust.version = 3;
    let higher_trust_bytes = canonical::encode(&higher_trust)?;
    let mut higher_channel = channel.clone();
    higher_channel.generation = 10;
    let higher_channel_bytes = canonical::encode(&higher_channel)?;
    for asset in &manifest.assets {
        let directory = root.join(format!("higher-target-fails-{}", asset.target));
        copy_directory(&positive, &directory)?;
        write(
            &directory.join("flux-exchange-release-trust.json"),
            &higher_trust_bytes,
        )?;
        sign_file(
            &root_key,
            &higher_trust_bytes,
            &directory.join(format!(
                "flux-exchange-release-trust.json.{root_id}.minisig"
            )),
        )?;
        write(
            &directory.join("flux-exchange-release-channel.json"),
            &higher_channel_bytes,
        )?;
        sign_file(
            &old_channel,
            &higher_channel_bytes,
            &directory.join(format!(
                "flux-exchange-release-channel.json.{old_channel_id}.minisig"
            )),
        )?;
        sign_file(
            &new_channel,
            &higher_channel_bytes,
            &directory.join(format!(
                "flux-exchange-release-channel.json.{new_channel_id}.minisig"
            )),
        )?;
        let corrupt_path = directory.join(&asset.archive);
        let mut corrupt = std::fs::read(&corrupt_path)?;
        corrupt.push(0);
        std::fs::write(corrupt_path, corrupt)?;
    }
    write(
        &root.join("release-entry.json"),
        &canonical::encode(&entry)?,
    )?;
    write(
        &root.join("compatibility.json"),
        &canonical::encode(&Compatibility {
            schema: "exchange.compatibility.v1".into(),
            release: CompatibilityRelease {
                tag: entry.tag.clone(),
                version: entry.version.clone(),
                source_commit: entry.source_commit.clone(),
                build_id: entry.build_id.clone(),
            },
            protocols: entry.protocols.clone(),
        })?,
    )?;
    for (name, identity) in [
        (
            "readiness-linux.json",
            serde_json::json!({"boot_id":"00000000-0000-0000-0000-000000000001","kind":"linux-proc-start","ticks":"1"}),
        ),
        (
            "readiness-macos.json",
            serde_json::json!({"kind":"macos-proc-start","microseconds":0,"seconds":"1"}),
        ),
        (
            "readiness-windows.json",
            serde_json::json!({"filetime":"1","kind":"windows-process-creation"}),
        ),
    ] {
        let readiness = serde_json::json!({"bind":{"host":"127.0.0.1","port":1,"scheme":"http"},"process":{"pid":1,"start_identity":identity},"protocols":entry.protocols,"release":{"build_id":entry.build_id,"executable_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_commit":entry.source_commit,"tag":entry.tag,"version":entry.version},"schema":"exchange.supervisor-ready.v1"});
        write(&root.join(name), &canonical::encode(&readiness)?)?;
    }
    let mut bad_bind: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
    bad_bind["bind"]["host"] = serde_json::json!("localhost");
    write(
        &root.join("readiness-bad-bind.json"),
        &canonical::encode(&bad_bind)?,
    )?;
    let mut bad_kind: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
    bad_kind["process"]["start_identity"]["kind"] = serde_json::json!("unknown");
    write(
        &root.join("readiness-bad-kind.json"),
        &canonical::encode(&bad_kind)?,
    )?;
    let mut bad_decimal: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
    bad_decimal["process"]["start_identity"]["ticks"] = serde_json::json!("01");
    write(
        &root.join("readiness-bad-decimal.json"),
        &canonical::encode(&bad_decimal)?,
    )?;
    let mut provenance: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    provenance["provenance"] = serde_json::json!("forbidden-client-input");
    write(
        &root.join("manifest-with-provenance.json"),
        &canonical::encode(&provenance)?,
    )?;
    write(
        &root.join("expiry-during-target.json"),
        &canonical::encode(
            &serde_json::json!({"commit_clock":"2026-08-05T12:00:00Z","directory":"positive"}),
        )?,
    )?;
    write(
        &root.join("integer-over-jcs-safe.json"),
        br#"{"value":9007199254740992}"#,
    )?;
    write(
        &root.join("invalid-duplicate.json"),
        br#"{"schema":"x","schema":"x"}"#,
    )?;
    write(
        &root.join("redirect-positive.json"),
        &canonical::encode(
            &serde_json::json!({"final_status":200,"forwarded_credentials":false,"location":"https://release-assets.githubusercontent.com:443/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sig=x&sp=r","second_redirect":false,"status":302}),
        )?,
    )?;
    write(
        &root.join("redirect-bad-host.json"),
        &canonical::encode(
            &serde_json::json!({"final_status":200,"forwarded_credentials":false,"location":"https://RELEASE-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sig=x","second_redirect":false,"status":302}),
        )?,
    )?;
    let base = "https://release-assets.githubusercontent.com/github-production-release-asset/1/00000000-0000-0000-0000-000000000001?sig=x";
    let redirects = [
        (
            "redirect-status.json",
            301,
            base.to_owned(),
            false,
            200,
            false,
        ),
        (
            "redirect-scheme.json",
            302,
            base.replacen("https", "http", 1),
            false,
            200,
            false,
        ),
        (
            "redirect-host.json",
            302,
            base.replace("release-assets", "objects"),
            false,
            200,
            false,
        ),
        (
            "redirect-port.json",
            302,
            base.replace(".com/", ".com:444/"),
            false,
            200,
            false,
        ),
        (
            "redirect-path.json",
            302,
            base.replace("github-production-release-asset", "wrong"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-name.json",
            302,
            base.replace("sig=x", "unknown=x"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-bound.json",
            302,
            format!("{}{}", base.replace("sig=x", "sig="), "x".repeat(6145)),
            false,
            200,
            false,
        ),
        (
            "redirect-credential.json",
            302,
            base.to_owned(),
            true,
            200,
            false,
        ),
        (
            "redirect-second.json",
            302,
            base.to_owned(),
            false,
            302,
            true,
        ),
    ];
    for (name, status, location, forwarded_credentials, final_status, second_redirect) in redirects
    {
        write(
            &root.join(name),
            &canonical::encode(
                &serde_json::json!({"final_status":final_status,"forwarded_credentials":forwarded_credentials,"location":location,"second_redirect":second_redirect,"status":status}),
            )?,
        )?;
    }
    write(
        &root.join("channel-rollback.json"),
        &canonical::encode(
            &serde_json::json!({"kind":"channel","number":6,"prior":{"number":7,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )?,
    )?;
    write(
        &root.join("channel-equivocation.json"),
        &canonical::encode(
            &serde_json::json!({"kind":"channel","number":7,"prior":{"number":7,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )?,
    )?;
    write(
        &root.join("no-compatible.json"),
        &canonical::encode(
            &serde_json::json!({"releases":[ReleaseEntry::test("1.0.0", Protocols { supervisor: "exchange.supervisor-ready.v2".into(), ..Protocols::v1() })],"supported":Protocols::v1()}),
        )?,
    )?;

    let accepted_state = RollbackState {
        trust: Some(Floor {
            number: 2,
            sha256: digest_hex(&trust_bytes),
        }),
        channel: Some(Floor {
            number: 7,
            sha256: digest_hex(&channel_bytes),
        }),
    };
    let cases = fixture_cases(
        issued,
        &accepted_state,
        &digest_hex(&canonical::encode(&incompatible_channel)?),
        &digest_hex(&canonical::encode(&digest_channel)?),
        &digest_hex(&higher_trust_bytes),
        &digest_hex(&higher_channel_bytes),
        &digest_hex(&canonical::encode(&multi_channel)?),
    );
    let mut files = BTreeMap::new();
    inventory_files(&root, &root, &mut files)?;
    files.remove("fixture-set.json");
    let fixture_set = FixtureSet {
        schema: "exchange.release-fixture-set.v1".into(),
        exchange_commit: "4e398a73dcb8de17466cbedea77122dd489bed4f".into(),
        files,
        cases,
    };
    write(
        &root.join("fixture-set.json"),
        &canonical::encode(&fixture_set)?,
    )?;
    std::fs::remove_dir_all(&scratch)?;
    Ok(())
}

fn fixture_cases(
    now: &str,
    accepted: &RollbackState,
    incompatible_channel_sha256: &str,
    digest_channel_sha256: &str,
    higher_trust_sha256: &str,
    higher_channel_sha256: &str,
    multi_channel_sha256: &str,
) -> Vec<FixtureCase> {
    let empty = RollbackState::default();
    let installed = InstalledIdentity {
        version: "0.16.0".into(),
        source_commit: "1111111111111111111111111111111111111111".into(),
        manifest_sha256: "1".repeat(64),
        executable_sha256: "2".repeat(64),
    };
    let case = |id: &str,
                operation: &str,
                input: &str,
                result: &str,
                platform: &str,
                state: RollbackState| FixtureCase {
        id: id.into(),
        operation: operation.into(),
        input: input.into(),
        clock: now.into(),
        platform: platform.into(),
        prior_state: empty.clone(),
        prior_install: Some(installed.clone()),
        expected_result: result.into(),
        expected_state: state,
        expected_install: Some(installed.clone()),
        expected_stage: operation.into(),
        expected_error_contains: expected_error(id, operation, result).map(str::to_owned),
    };
    let manifest_mutations = [
        "archive-corrupt-after-digest",
        "asset-missing-platform",
        "asset-undeclared",
        "executable-renamed",
        "plugin-or-connector-executable",
        "manifest-oversized",
        "archive-oversized",
        "archive-member-count-17",
        "archive-member-oversized",
        "archive-member-path-241",
        "archive-executable-substituted",
        "key-id-substituted",
        "logical-origin-changed",
        "foreign-origin",
        "unsupported-protocol-set",
        "id-or-basename-unsafe",
    ];
    let mut cases = vec![
        case(
            "positive-linux",
            "verify-directory",
            "positive",
            "accepted",
            "x86_64-unknown-linux-gnu",
            accepted.clone(),
        ),
        case(
            "positive-macos",
            "verify-directory",
            "positive",
            "accepted",
            "x86_64-apple-darwin",
            accepted.clone(),
        ),
        case(
            "positive-windows",
            "verify-directory",
            "positive",
            "accepted",
            "x86_64-pc-windows-msvc",
            accepted.clone(),
        ),
        case(
            "positive-signer-overlap",
            "verify-directory",
            "positive",
            "accepted",
            "aarch64-unknown-linux-gnu",
            accepted.clone(),
        ),
        case(
            "integer-over-jcs-safe",
            "canonical",
            "integer-over-jcs-safe.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "minisign-key-malformed",
            "verify-directory",
            "trust-key-malformed",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "minisign-key-reused",
            "verify-directory",
            "trust-key-reused",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "delegation-expired",
            "verify-directory",
            "delegation-expired",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "role-confusion",
            "verify-directory",
            "role-confusion",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "channel-expired",
            "verify-directory",
            "channel-expired",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(accepted.trust.clone().expect("trust floor")),
                channel: None,
            },
        ),
        case(
            "expiry-equality-stopped",
            "verify-directory",
            "channel-expired",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(accepted.trust.clone().expect("trust floor")),
                channel: None,
            },
        ),
        case(
            "manifest-digest-substituted",
            "verify-directory",
            "manifest-digest-substituted",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: Some(Floor {
                    number: 7,
                    sha256: digest_channel_sha256.into(),
                }),
            },
        ),
        case(
            "channel-release-count-129",
            "verify-directory",
            "channel-129",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(accepted.trust.clone().expect("trust floor")),
                channel: None,
            },
        ),
        case(
            "higher-channel-no-compatible",
            "verify-directory",
            "higher-no-compatible",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: Some(Floor {
                    number: 8,
                    sha256: incompatible_channel_sha256.into(),
                }),
            },
        ),
        case(
            "decimal-noncanonical",
            "readiness",
            "readiness-bad-decimal.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-bind-domain",
            "readiness",
            "readiness-bad-bind.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-start-kind",
            "readiness",
            "readiness-bad-kind.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-linux-start",
            "readiness",
            "readiness-linux.json",
            "accepted",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-macos-start",
            "readiness",
            "readiness-macos.json",
            "accepted",
            "x86_64-apple-darwin",
            empty.clone(),
        ),
        case(
            "readiness-windows-start",
            "readiness",
            "readiness-windows.json",
            "accepted",
            "x86_64-pc-windows-msvc",
            empty.clone(),
        ),
        case(
            "github-redirect-positive",
            "transport",
            "redirect-positive.json",
            "accepted",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-uppercase-host",
            "transport",
            "redirect-bad-host.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-status",
            "transport",
            "redirect-status.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-scheme",
            "transport",
            "redirect-scheme.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-host",
            "transport",
            "redirect-host.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-port",
            "transport",
            "redirect-port.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-path",
            "transport",
            "redirect-path.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-name",
            "transport",
            "redirect-query-name.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-bound",
            "transport",
            "redirect-query-bound.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-credential",
            "transport",
            "redirect-credential.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-second-redirect",
            "transport",
            "redirect-second.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "higher-incompatible-skipped",
            "verify-directory",
            "selection-multi",
            "accepted",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: Some(Floor {
                    number: 11,
                    sha256: multi_channel_sha256.into(),
                }),
            },
        ),
        case(
            "newest-compatible-selected",
            "verify-directory",
            "selection-multi",
            "accepted",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: Some(Floor {
                    number: 11,
                    sha256: multi_channel_sha256.into(),
                }),
            },
        ),
    ];
    let rollback_prior = RollbackState {
        trust: Some(Floor {
            number: 3,
            sha256: "c".repeat(64),
        }),
        channel: accepted.channel.clone(),
    };
    cases.push(FixtureCase {
        id: "delegation-rollback".into(),
        operation: "verify-directory".into(),
        input: "positive".into(),
        clock: now.into(),
        platform: "x86_64-unknown-linux-gnu".into(),
        prior_state: rollback_prior.clone(),
        prior_install: Some(installed.clone()),
        expected_result: "refused".into(),
        expected_state: rollback_prior,
        expected_install: Some(installed.clone()),
        expected_stage: "verify-directory".into(),
        expected_error_contains: Some("rollback refused".into()),
    });
    cases.push(case(
        "provenance-client-input",
        "manifest-document",
        "manifest-with-provenance.json",
        "refused",
        "none",
        empty.clone(),
    ));
    cases.push(FixtureCase {
        id: "expiry-during-target-download".into(),
        operation: "verify-directory-expiry".into(),
        input: "expiry-during-target.json".into(),
        clock: "2026-08-05T11:59:59Z".into(),
        platform: "x86_64-unknown-linux-gnu".into(),
        prior_state: RollbackState::default(),
        prior_install: Some(installed.clone()),
        expected_result: "refused".into(),
        expected_state: accepted.clone(),
        expected_install: Some(installed.clone()),
        expected_stage: "verify-directory-expiry".into(),
        expected_error_contains: Some("time refused".into()),
    });
    let channel_rollback_prior = RollbackState {
        trust: None,
        channel: Some(Floor {
            number: 8,
            sha256: "e".repeat(64),
        }),
    };
    cases.push(FixtureCase {
        id: "channel-floor-survives-rotation".into(),
        operation: "verify-directory".into(),
        input: "positive".into(),
        clock: now.into(),
        platform: "x86_64-unknown-linux-gnu".into(),
        prior_state: channel_rollback_prior.clone(),
        prior_install: Some(installed.clone()),
        expected_result: "refused".into(),
        expected_state: RollbackState {
            trust: accepted.trust.clone(),
            channel: channel_rollback_prior.channel,
        },
        expected_install: Some(installed.clone()),
        expected_stage: "verify-directory".into(),
        expected_error_contains: Some("rollback refused".into()),
    });
    let channel_equivocation_prior = RollbackState {
        trust: None,
        channel: Some(Floor {
            number: 7,
            sha256: "e".repeat(64),
        }),
    };
    cases.push(FixtureCase {
        id: "same-number-different-bytes".into(),
        operation: "verify-directory".into(),
        input: "positive".into(),
        clock: now.into(),
        platform: "x86_64-unknown-linux-gnu".into(),
        prior_state: channel_equivocation_prior.clone(),
        prior_install: Some(installed.clone()),
        expected_result: "refused".into(),
        expected_state: RollbackState {
            trust: accepted.trust.clone(),
            channel: channel_equivocation_prior.channel,
        },
        expected_install: Some(installed.clone()),
        expected_stage: "verify-directory".into(),
        expected_error_contains: Some("equivocation refused".into()),
    });
    cases.extend(manifest_mutations.into_iter().map(|id| {
        case(
            id,
            "manifest-mutation",
            "positive",
            "refused",
            "none",
            empty.clone(),
        )
    }));
    let higher_state = RollbackState {
        trust: Some(Floor {
            number: 3,
            sha256: higher_trust_sha256.into(),
        }),
        channel: Some(Floor {
            number: 10,
            sha256: higher_channel_sha256.into(),
        }),
    };
    for (index, target) in SUPPORTED_TARGETS.iter().enumerate() {
        let id = if index == 0 {
            "higher-channel-target-fails".to_owned()
        } else {
            format!("higher-channel-target-fails-{target}")
        };
        let input = format!("higher-target-fails-{target}");
        cases.push(case(
            &id,
            "verify-directory",
            &input,
            "refused",
            target,
            higher_state.clone(),
        ));
    }
    cases
}

fn sign_file(pair: &KeyPair, bytes: &[u8], path: &Path) -> anyhow::Result<()> {
    let signature = sign(
        Some(&pair.pk),
        &pair.sk,
        Cursor::new(bytes),
        Some("exchange.release.fixture.v1"),
        Some("untrusted comment: TEST-ONLY Exchange fixture"),
    )?;
    write(path, signature.into_string().as_bytes())
}
fn expected_error(id: &str, operation: &str, result: &str) -> Option<&'static str> {
    if result == "accepted" {
        return None;
    }
    Some(match id {
        "integer-over-jcs-safe" => "bound refused",
        "minisign-key-malformed" | "minisign-key-reused" => "signature refused",
        "delegation-expired" => "time refused",
        "role-confusion" => "schema refused",
        "channel-expired" | "expiry-equality-stopped" | "expiry-during-target-download" => {
            "time refused"
        }
        "manifest-digest-substituted" => "digest refused",
        "archive-corrupt-after-digest" | "archive-executable-substituted" => "archive refused",
        "channel-release-count-129"
        | "manifest-oversized"
        | "archive-oversized"
        | "archive-member-count-17"
        | "archive-member-oversized"
        | "asset-missing-platform" => "bound refused",
        "higher-channel-no-compatible" => "no compatible Exchange release",
        "key-id-substituted"
        | "logical-origin-changed"
        | "foreign-origin"
        | "unsupported-protocol-set"
        | "id-or-basename-unsafe"
        | "provenance-client-input" => "schema refused",
        "asset-undeclared" => "undeclared staged asset",
        "plugin-or-connector-executable" | "archive-member-path-241" | "executable-renamed" => {
            "archive refused"
        }
        "github-redirect-query-bound" => "bound refused",
        id if id.starts_with("github-redirect") => "transport refused",
        id if id.starts_with("readiness-") || id == "decimal-noncanonical" => "schema refused",
        id if id.starts_with("higher-channel-target-fails") => "archive refused",
        _ if operation == "verify-directory" => "refused",
        _ => "refused",
    })
}
fn signed_trust_variant(
    root: &Path,
    name: &str,
    positive: &Path,
    trust: &TrustDocument,
    key: &KeyPair,
    key_id: &str,
) -> anyhow::Result<()> {
    let directory = root.join(name);
    copy_directory(positive, &directory)?;
    let bytes = canonical::encode(trust)?;
    write(&directory.join("flux-exchange-release-trust.json"), &bytes)?;
    sign_file(
        key,
        &bytes,
        &directory.join(format!("flux-exchange-release-trust.json.{key_id}.minisig")),
    )
}
fn signed_channel_variant(
    root: &Path,
    name: &str,
    positive: &Path,
    channel: &Channel,
    signers: &[(&KeyPair, &str)],
) -> anyhow::Result<()> {
    let directory = root.join(name);
    copy_directory(positive, &directory)?;
    let bytes = canonical::encode(channel)?;
    write(
        &directory.join("flux-exchange-release-channel.json"),
        &bytes,
    )?;
    for (key, key_id) in signers {
        sign_file(
            key,
            &bytes,
            &directory.join(format!(
                "flux-exchange-release-channel.json.{key_id}.minisig"
            )),
        )?;
    }
    Ok(())
}
fn copy_directory(source: &Path, destination: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(destination)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), destination.join(entry.file_name()))?;
        }
    }
    Ok(())
}
fn write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, bytes)?;
    Ok(())
}
fn inventory_files(
    root: &Path,
    directory: &Path,
    output: &mut BTreeMap<String, String>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        if entry.file_type()?.is_dir() {
            inventory_files(root, &entry.path(), output)?;
        } else {
            let relative = entry
                .path()
                .strip_prefix(root)?
                .to_str()
                .ok_or_else(|| anyhow::anyhow!("fixture path is not UTF-8"))?
                .replace('\\', "/");
            output.insert(relative, digest_hex(&std::fs::read(entry.path())?));
        }
    }
    Ok(())
}
