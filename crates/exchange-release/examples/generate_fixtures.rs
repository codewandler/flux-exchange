use base64::Engine as _;
use flux_exchange_release as release;
use minisign::{sign, KeyPair};
use release::*;
use std::collections::BTreeMap;
use std::io::{Cursor, Read as _};
use std::path::{Path, PathBuf};

// This maintainer-only generator intentionally creates fresh TEST-ONLY minisign keys. CI verifies
// the committed bytes and inventory with `self-test`; it never regenerates fixtures or compares a
// regenerated signature set for byte equality.

fn main() -> anyhow::Result<()> {
    let root =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/exchange-release-v2");
    if std::env::args().nth(1).as_deref() == Some("--refresh-manifest") {
        return refresh_manifest(&root);
    }
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
        schema: "exchange.release-manifest.v2".into(),
        origin: ORIGIN.into(),
        tag: format!("refs/tags/v{version}"),
        version: version.into(),
        source_commit: "4e398a73dcb8de17466cbedea77122dd489bed4f".into(),
        build_id: "TEST-ONLY-X126-FIXTURE".into(),
        protocols: Protocols::v2(),
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
        schema: "exchange.release-channel.v2".into(),
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
    for (name, public_key) in malformed_public_keys(&old_channel.pk.to_base64())? {
        let mut malformed_trust = trust.clone();
        malformed_trust.roles.channel.keys[0].minisign_public_key = public_key;
        signed_trust_variant(&root, name, &positive, &malformed_trust, &root_key, root_id)?;
    }
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
    let mut reused_within_role = trust.clone();
    reused_within_role.roles.channel.keys[1].minisign_public_key =
        reidentify_public_key(&old_channel.pk.to_base64())?;
    signed_trust_variant(
        &root,
        "trust-key-reused-within-role",
        &positive,
        &reused_within_role,
        &root_key,
        root_id,
    )?;
    let mut reused_with_root = trust.clone();
    reused_with_root.roles.channel.keys[0].minisign_public_key = root_key.pk.to_base64();
    signed_trust_variant(
        &root,
        "trust-key-reused-with-root",
        &positive,
        &reused_with_root,
        &root_key,
        root_id,
    )?;
    let mut expired_delegation = trust.clone();
    expired_delegation.roles.channel.keys[0].not_after = "2026-08-04T12:00:01Z".into();
    signed_trust_variant(
        &root,
        "delegation-expired",
        &positive,
        &expired_delegation,
        &root_key,
        root_id,
    )?;
    let mut role_confusion = trust.clone();
    role_confusion.roles.release.keys[0].key_id = old_channel_id.into();
    signed_trust_variant(
        &root,
        "role-confusion",
        &positive,
        &role_confusion,
        &root_key,
        root_id,
    )?;
    for (name, key_id) in [
        ("key-id-empty", "".to_owned()),
        ("key-id-overlong", "a".repeat(65)),
        ("key-id-slash", "unsafe/key".to_owned()),
        ("key-id-double-hyphen", "unsafe--key".to_owned()),
        ("key-id-leading-punctuation", "-unsafe".to_owned()),
        ("key-id-trailing-punctuation", "unsafe-".to_owned()),
        ("key-id-nonascii", "kéy".to_owned()),
        ("key-id-uppercase", "Unsafe".to_owned()),
    ] {
        let mut invalid = trust.clone();
        invalid.roles.channel.keys[0].key_id = key_id;
        signed_trust_variant(&root, name, &positive, &invalid, &root_key, root_id)?;
    }
    let mut future_trust = trust.clone();
    future_trust.issued_at = "2026-08-04T12:05:01Z".into();
    signed_trust_variant(
        &root,
        "trust-future-issued",
        &positive,
        &future_trust,
        &root_key,
        root_id,
    )?;

    let missing_trust_signature = root.join("trust-signature-missing");
    copy_directory(&positive, &missing_trust_signature)?;
    std::fs::remove_file(missing_trust_signature.join(format!(
        "flux-exchange-release-trust.json.{root_id}.minisig"
    )))?;
    let substituted_trust_signature = root.join("trust-signature-substituted");
    copy_directory(&positive, &substituted_trust_signature)?;
    std::fs::copy(
        positive.join(format!(
            "flux-exchange-release-channel.json.{old_channel_id}.minisig"
        )),
        substituted_trust_signature.join(format!(
            "flux-exchange-release-trust.json.{root_id}.minisig"
        )),
    )?;

    let root_two = KeyPair::generate_unencrypted_keypair()?;
    write(
        &root.join("root-policy-threshold-two.test.json"),
        &canonical::encode(&RootPolicy {
            schema: "exchange.release-root-policy.v1".into(),
            threshold: 2,
            test_only: true,
            keys: vec![
                RootKey {
                    key_id: root_id.into(),
                    minisign_public_key: root_key.pk.to_base64(),
                },
                RootKey {
                    key_id: "test-only-root-2026-02".into(),
                    minisign_public_key: root_two.pk.to_base64(),
                },
            ],
        })?,
    )?;

    let release_two = KeyPair::generate_unencrypted_keypair()?;
    let mut release_threshold = trust.clone();
    release_threshold.roles.release.threshold = 2;
    release_threshold.roles.release.keys.push(DelegatedKey {
        key_id: "test-only-release-2026-02".into(),
        minisign_public_key: release_two.pk.to_base64(),
        not_before: issued.into(),
        not_after: trust_expires.into(),
    });
    signed_trust_variant(
        &root,
        "release-threshold-failure",
        &positive,
        &release_threshold,
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
    let mut future_channel = channel.clone();
    future_channel.issued_at = "2026-08-04T12:05:01Z".into();
    future_channel.expires_at = "2026-08-05T12:05:01Z".into();
    signed_channel_variant(
        &root,
        "channel-future-issued",
        &positive,
        &future_channel,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    let mut threshold_trust = trust.clone();
    threshold_trust.roles.channel.threshold = 2;
    let threshold_directory = root.join("channel-threshold-failure");
    signed_trust_variant(
        &root,
        "channel-threshold-failure",
        &positive,
        &threshold_trust,
        &root_key,
        root_id,
    )?;
    let mut one_signature_channel = channel.clone();
    one_signature_channel.signing_key_ids = vec![old_channel_id.into()];
    let one_signature_bytes = canonical::encode(&one_signature_channel)?;
    write(
        &threshold_directory.join("flux-exchange-release-channel.json"),
        &one_signature_bytes,
    )?;
    sign_file(
        &old_channel,
        &one_signature_bytes,
        &threshold_directory.join(format!(
            "flux-exchange-release-channel.json.{old_channel_id}.minisig"
        )),
    )?;
    let missing_channel_signature = root.join("channel-signature-missing");
    copy_directory(&positive, &missing_channel_signature)?;
    std::fs::remove_file(missing_channel_signature.join(format!(
        "flux-exchange-release-channel.json.{old_channel_id}.minisig"
    )))?;
    let substituted_channel_signature = root.join("channel-signature-substituted");
    copy_directory(&positive, &substituted_channel_signature)?;
    std::fs::copy(
        positive.join(format!(
            "flux-exchange-release-manifest.json.{release_id}.minisig"
        )),
        substituted_channel_signature.join(format!(
            "flux-exchange-release-channel.json.{old_channel_id}.minisig"
        )),
    )?;
    let missing_manifest_signature = root.join("manifest-signature-missing");
    copy_directory(&positive, &missing_manifest_signature)?;
    std::fs::remove_file(missing_manifest_signature.join(format!(
        "flux-exchange-release-manifest.json.{release_id}.minisig"
    )))?;
    let disagree_manifest_signature = root.join("manifest-signature-key-id-disagree");
    copy_directory(&positive, &disagree_manifest_signature)?;
    std::fs::copy(
        positive.join(format!(
            "flux-exchange-release-channel.json.{old_channel_id}.minisig"
        )),
        disagree_manifest_signature.join(format!(
            "flux-exchange-release-manifest.json.{release_id}.minisig"
        )),
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
    incompatible_channel.releases[0].protocols.supervisor = "exchange.supervisor-ready.v3".into();
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
    let mut older = entry.clone();
    older.version = "0.16.0".into();
    older.tag = "refs/tags/v0.16.0".into();
    older.source_commit = "3e398a73dcb8de17466cbedea77122dd489bed4f".into();
    older.build_id = "TEST-ONLY-X126-OLDER-FIXTURE".into();
    older.manifest_sha256 = "c".repeat(64);
    multi_channel.releases.insert(0, older);
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
    let mut higher_incompatible = channel.clone();
    higher_incompatible.generation = 12;
    let mut newer = entry.clone();
    newer.version = "0.18.0".into();
    newer.tag = "refs/tags/v0.18.0".into();
    newer.source_commit = "5e398a73dcb8de17466cbedea77122dd489bed4f".into();
    newer.manifest_sha256 = "d".repeat(64);
    newer.protocols.supervisor = "exchange.supervisor-ready.v3".into();
    higher_incompatible.releases.push(newer);
    signed_channel_variant(
        &root,
        "selection-higher-incompatible",
        &positive,
        &higher_incompatible,
        &[
            (&old_channel, old_channel_id),
            (&new_channel, new_channel_id),
        ],
    )?;
    forbidden_executable_variant(
        &root,
        &positive,
        &manifest,
        &channel,
        &release_key,
        release_id,
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
            schema: "exchange.compatibility.v2".into(),
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
        let readiness = serde_json::json!({"bind":{"host":"127.0.0.1","port":1,"scheme":"http"},"process":{"pid":1,"start_identity":identity},"protocols":entry.protocols,"release":{"build_id":entry.build_id,"executable_sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","source_commit":entry.source_commit,"tag":entry.tag,"version":entry.version},"schema":"exchange.supervisor-ready.v2"});
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
    for (name, path, value) in [
        ("readiness-decimal-sign.json", "ticks", "-1"),
        (
            "readiness-decimal-21-digits.json",
            "ticks",
            "100000000000000000000",
        ),
        (
            "readiness-decimal-overflow.json",
            "ticks",
            "18446744073709551616",
        ),
    ] {
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
        invalid["process"]["start_identity"][path] = serde_json::json!(value);
        write(&root.join(name), &canonical::encode(&invalid)?)?;
    }
    for (name, field, value) in [
        (
            "readiness-bad-scheme.json",
            "scheme",
            serde_json::json!("https"),
        ),
        ("readiness-port-zero.json", "port", serde_json::json!(0)),
        (
            "readiness-port-overflow.json",
            "port",
            serde_json::json!(65536),
        ),
    ] {
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
        invalid["bind"][field] = value;
        write(&root.join(name), &canonical::encode(&invalid)?)?;
    }
    for (name, pid) in [
        ("readiness-pid-zero.json", 0_u64),
        ("readiness-pid-overflow.json", 4_294_967_296_u64),
    ] {
        let mut invalid: serde_json::Value =
            serde_json::from_slice(&std::fs::read(root.join("readiness-linux.json"))?)?;
        invalid["process"]["pid"] = serde_json::json!(pid);
        write(&root.join(name), &canonical::encode(&invalid)?)?;
    }
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
        &root.join("invalid-noncanonical.json"),
        br#"{ "schema":"exchange.release-manifest.v2"}"#,
    )?;
    let mut unknown_manifest: serde_json::Value = serde_json::from_slice(&manifest_bytes)?;
    unknown_manifest["unknown"] = serde_json::json!(true);
    write(
        &root.join("invalid-unknown-manifest.json"),
        &canonical::encode(&unknown_manifest)?,
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
        (
            "redirect-userinfo.json",
            302,
            base.replacen("https://", "https://user@", 1),
            false,
            200,
            false,
        ),
        (
            "redirect-fragment.json",
            302,
            format!("{base}#fragment"),
            false,
            200,
            false,
        ),
        (
            "redirect-location-bound.json",
            302,
            format!("{}{}", base.replace("sig=x", "sig="), "x".repeat(8193)),
            false,
            200,
            false,
        ),
        (
            "redirect-location-nonascii.json",
            302,
            base.replace("sig=x", "sig=é"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-empty.json",
            302,
            base.replace("sig=x", "sig="),
            false,
            200,
            false,
        ),
        (
            "redirect-query-value-bound.json",
            302,
            format!("{}{}", base.replace("sig=x", "sig="), "x".repeat(2049)),
            false,
            200,
            false,
        ),
        (
            "redirect-query-duplicate.json",
            302,
            format!("{base}&sig=y"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-encoded-name.json",
            302,
            base.replace("sig=x", "s%69g=x"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-percent.json",
            302,
            base.replace("sig=x", "sig=%x0"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-control.json",
            302,
            base.replace("sig=x", "sig=%00"),
            false,
            200,
            false,
        ),
        (
            "redirect-query-raw-character.json",
            302,
            base.replace("sig=x", "sig=raw[value"),
            false,
            200,
            false,
        ),
        (
            "redirect-path-repository.json",
            302,
            base.replace("release-asset/1/", "release-asset/01/"),
            false,
            200,
            false,
        ),
        (
            "redirect-path-uuid.json",
            302,
            base.replace(
                "00000000-0000-0000-0000-000000000001",
                "00000000-0000-0000-0000-00000000000G",
            ),
            false,
            200,
            false,
        ),
        (
            "redirect-final-status.json",
            302,
            base.to_owned(),
            false,
            206,
            false,
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
    let immutable = "https://github.com/codewandler/flux-exchange/releases/download/v0.17.0/flux-exchange-release-manifest.json";
    for (name, kind, credential_kind, url) in [
        ("initial-trust-positive.json", "trust", "none", "https://github.com/codewandler/flux-exchange/releases/download/exchange-trust-v1/flux-exchange-release-trust.json".to_owned()),
        ("initial-channel-positive.json", "channel", "none", "https://github.com/codewandler/flux-exchange/releases/download/exchange-stable-v1/flux-exchange-release-channel.json".to_owned()),
        ("initial-immutable-positive.json", "immutable", "none", immutable.to_owned()),
        ("initial-scheme.json", "immutable", "none", immutable.replacen("https", "http", 1)),
        ("initial-host.json", "immutable", "none", immutable.replace("github.com", "www.github.com")),
        ("initial-port.json", "immutable", "none", immutable.replace("github.com/", "github.com:443/")),
        ("initial-userinfo.json", "immutable", "none", immutable.replacen("https://", "https://user@", 1)),
        ("initial-fragment.json", "immutable", "none", format!("{immutable}#x")),
        ("initial-query.json", "immutable", "none", format!("{immutable}?x=1")),
        ("initial-repository.json", "immutable", "none", immutable.replace("codewandler/flux-exchange", "codewandler/flux")),
        ("initial-tag.json", "immutable", "none", immutable.replace("v0.17.0", "v0.18.0")),
        ("initial-basename.json", "immutable", "none", immutable.replace("manifest.json", "channel.json")),
        ("initial-mutable-latest.json", "immutable", "none", immutable.replace("releases/download/v0.17.0", "releases/latest/download")),
        ("initial-authorization.json", "immutable", "authorization", immutable.to_owned()),
        ("initial-proxy-authorization.json", "immutable", "proxy-authorization", immutable.to_owned()),
        ("initial-cookie.json", "immutable", "cookie", immutable.to_owned()),
    ] {
        write(
            &root.join(name),
            &canonical::encode(&serde_json::json!({"credential_kind":credential_kind,"kind":kind,"url":url}))?,
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
        &root.join("trust-rollback.json"),
        &canonical::encode(
            &serde_json::json!({"kind":"trust","number":2,"prior":{"number":3,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )?,
    )?;
    write(
        &root.join("trust-equivocation.json"),
        &canonical::encode(
            &serde_json::json!({"kind":"trust","number":2,"prior":{"number":2,"sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"},"sha256":"bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"}),
        )?,
    )?;
    write(
        &root.join("no-compatible.json"),
        &canonical::encode(
            &serde_json::json!({"releases":[ReleaseEntry::test("1.0.0", Protocols { supervisor: "exchange.supervisor-ready.v3".into(), ..Protocols::v2() })],"supported":Protocols::v2()}),
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
    let digests = FixtureDigests {
        incompatible_channel: digest_hex(&canonical::encode(&incompatible_channel)?),
        digest_channel: digest_hex(&canonical::encode(&digest_channel)?),
        higher_trust: digest_hex(&higher_trust_bytes),
        higher_channel: digest_hex(&higher_channel_bytes),
        multi_channel: digest_hex(&canonical::encode(&multi_channel)?),
        higher_incompatible_channel: digest_hex(&canonical::encode(&higher_incompatible)?),
        threshold_trust: digest_hex(&canonical::encode(&threshold_trust)?),
        release_threshold: digest_hex(&canonical::encode(&release_threshold)?),
        embedded_id_trust: digest_hex(&std::fs::read(
            root.join("trust-key-embedded-id-disagreement/flux-exchange-release-trust.json"),
        )?),
        expired_delegation_trust: digest_hex(&std::fs::read(
            root.join("delegation-expired/flux-exchange-release-trust.json"),
        )?),
        forbidden_channel: digest_hex(&std::fs::read(
            root.join("forbidden-executable/flux-exchange-release-channel.json"),
        )?),
    };
    let cases = fixture_cases(issued, &accepted_state, &digests);
    let mut files = BTreeMap::new();
    inventory_files(&root, &root, &mut files)?;
    files.remove("fixture-set.json");
    let fixture_set = FixtureSet {
        schema: "exchange.release-fixture-set.v2".into(),
        // This is the committed provider/verifier baseline from which these generated bytes were
        // produced. The fixture-set digest identifies this manifest without requiring an
        // impossible self-referential Git commit hash.
        exchange_commit: "3f897506a0240fd6236f445f7af73eea122172db".into(),
        files,
        cases,
        native_cases: native_fixture_cases(),
    };
    write(
        &root.join("fixture-set.json"),
        &canonical::encode(&fixture_set)?,
    )?;
    std::fs::remove_dir_all(&scratch)?;
    Ok(())
}

fn refresh_manifest(root: &Path) -> anyhow::Result<()> {
    let path = root.join("fixture-set.json");
    let previous =
        canonical::parse_value(&release::read_bounded_file(&path, 256 * 1024)?, 256 * 1024)?;
    let cases: Vec<FixtureCase> = serde_json::from_value(
        previous
            .get("cases")
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("fixture-set has no provider cases"))?,
    )?;
    let mut files = BTreeMap::new();
    inventory_files(root, root, &mut files)?;
    files.remove("fixture-set.json");
    let fixture_set = FixtureSet {
        schema: "exchange.release-fixture-set.v2".into(),
        exchange_commit: "3f897506a0240fd6236f445f7af73eea122172db".into(),
        files,
        cases,
        native_cases: native_fixture_cases(),
    };
    write(&path, &canonical::encode(&fixture_set)?)
}

fn native_fixture_cases() -> Vec<NativeFixtureCase> {
    const UNIX: &[&str] = &[
        "aarch64-apple-darwin",
        "aarch64-unknown-linux-gnu",
        "x86_64-apple-darwin",
        "x86_64-unknown-linux-gnu",
    ];
    const WINDOWS: &[&str] = &["x86_64-pc-windows-msvc"];

    fn evidence(targets: &[&str], test_target: &str, exact_test: &str) -> NativeFixtureEvidence {
        NativeFixtureEvidence {
            targets: targets.iter().map(|target| (*target).into()).collect(),
            test_target: test_target.into(),
            exact_test: exact_test.into(),
        }
    }

    vec![
        NativeFixtureCase {
            id: "four-form-secret-sentinel-process-scan".into(),
            evidence: vec![evidence(
                &[
                    "aarch64-apple-darwin",
                    "aarch64-unknown-linux-gnu",
                    "x86_64-apple-darwin",
                    "x86_64-pc-windows-msvc",
                    "x86_64-unknown-linux-gnu",
                ],
                "x134_sentinel_evidence",
                "transformed_secret_sentinels_never_enter_refusal_abort_crash_or_restart_outputs",
            )],
        },
        NativeFixtureCase {
            id: "production-root-inherited-environment".into(),
            evidence: vec![evidence(
                &[
                    "aarch64-apple-darwin",
                    "aarch64-unknown-linux-gnu",
                    "x86_64-apple-darwin",
                    "x86_64-pc-windows-msvc",
                    "x86_64-unknown-linux-gnu",
                ],
                "local_state_regressions",
                "native_process_derives_production_root_from_the_authenticated_os_account",
            )],
        },
        NativeFixtureCase {
            id: "expiry-equality-live".into(),
            evidence: vec![
                evidence(
                    UNIX,
                    "supervised_unix",
                    "verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop",
                ),
                evidence(
                    WINDOWS,
                    "supervised_windows",
                    "verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop",
                ),
            ],
        },
        NativeFixtureCase {
            id: "supervisor-death-normal-responsive-unix".into(),
            evidence: vec![evidence(
                UNIX,
                "supervised_unix",
                "real_server_emits_one_canonical_record_after_bind_and_dies_on_liveness_eof",
            )],
        },
        NativeFixtureCase {
            id: "supervisor-death-normal-wedged-unix".into(),
            evidence: vec![evidence(
                UNIX,
                "supervised_unix",
                "native_liveness_exits_an_exchange_whose_tokio_main_future_is_wedged",
            )],
        },
        NativeFixtureCase {
            id: "supervisor-death-sigkill-responsive-unix".into(),
            evidence: vec![evidence(
                UNIX,
                "supervised_unix",
                "sigkill_of_the_real_supervisor_kills_a_responsive_exchange_and_releases_its_port",
            )],
        },
        NativeFixtureCase {
            id: "supervisor-death-sigkill-wedged-unix".into(),
            evidence: vec![evidence(
                UNIX,
                "supervised_unix",
                "sigkill_of_the_real_supervisor_kills_a_tokio_wedged_exchange_and_releases_its_port",
            )],
        },
        NativeFixtureCase {
            id: "supervisor-death-terminate-responsive-windows".into(),
            evidence: vec![evidence(
                WINDOWS,
                "supervised_windows",
                "terminate_process_of_supervisor_kills_responsive_exchange_and_releases_port",
            )],
        },
        NativeFixtureCase {
            id: "supervisor-death-terminate-wedged-windows".into(),
            evidence: vec![evidence(
                WINDOWS,
                "supervised_windows",
                "terminate_process_of_supervisor_kills_wedged_exchange_and_releases_port",
            )],
        },
        NativeFixtureCase {
            id: "unix-inherited-abi".into(),
            evidence: vec![
                evidence(
                    UNIX,
                    "supervised_unix",
                    "exact_unix_abi_refuses_missing_and_wrong_capabilities",
                ),
                evidence(
                    UNIX,
                    "supervised_unix",
                    "unix_abi_refuses_alias_wrong_kind_direction_and_extra_inherited_fd",
                ),
                evidence(
                    UNIX,
                    "supervised_unix",
                    "unix_abi_refuses_each_missing_fd_and_does_not_discover_env_other_fd_or_stdout",
                ),
            ],
        },
        NativeFixtureCase {
            id: "windows-inherited-abi".into(),
            evidence: vec![
                evidence(
                    WINDOWS,
                    "supervised_windows",
                    "malformed_windows_handle_flags_refuse_without_stdout_readiness",
                ),
                evidence(
                    WINDOWS,
                    "supervised_windows",
                    "environment_stdout_and_handles_outside_the_explicit_list_are_not_capabilities",
                ),
                evidence(
                    WINDOWS,
                    "lib",
                    "supervisor::tests::windows_validator_refuses_noninherited_nonpipe_and_each_wrong_direction",
                ),
            ],
        },
    ]
}

struct FixtureDigests {
    incompatible_channel: String,
    digest_channel: String,
    higher_trust: String,
    higher_channel: String,
    multi_channel: String,
    higher_incompatible_channel: String,
    threshold_trust: String,
    release_threshold: String,
    embedded_id_trust: String,
    expired_delegation_trust: String,
    forbidden_channel: String,
}

fn fixture_cases(
    now: &str,
    accepted: &RollbackState,
    digests: &FixtureDigests,
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
        "basename-empty",
        "basename-overlong",
        "basename-dotdot",
        "basename-nonascii",
        "basename-leading-punctuation",
        "basename-trailing-punctuation",
        "protocol-empty",
        "protocol-overlong",
        "protocol-no-version",
        "protocol-version-zero",
        "protocol-version-leading-zero",
        "protocol-empty-token",
        "protocol-double-hyphen",
        "protocol-uppercase",
        "protocol-leading-punctuation",
        "protocol-trailing-punctuation",
        "protocol-nonascii",
        "manifest-tag-disagreement",
        "manifest-version-disagreement",
        "manifest-source-sha-disagreement",
        "archive-path-absolute",
        "archive-path-parent",
        "archive-path-backslash",
        "archive-path-duplicate",
        "archive-path-case-fold",
        "archive-total-expanded-overflow",
        "archive-trailing-zstd-frame",
        "archive-trailing-tar-data",
        "archive-trailing-zip-data",
        "archive-member-decompression-bound",
        "archive-link-member",
        "archive-device-member",
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
        FixtureCase {
            prior_state: RollbackState {
                trust: Some(Floor {
                    number: 1,
                    sha256: "0".repeat(64),
                }),
                channel: Some(Floor {
                    number: 6,
                    sha256: "1".repeat(64),
                }),
            },
            ..case(
                "positive-signer-overlap",
                "verify-directory",
                "positive",
                "accepted",
                "aarch64-unknown-linux-gnu",
                accepted.clone(),
            )
        },
        case(
            "compatibility-positive",
            "compatibility",
            "compatibility.json",
            "accepted",
            "none",
            empty.clone(),
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
            "canonical-duplicate-field",
            "canonical",
            "invalid-duplicate.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "canonical-noncanonical-bytes",
            "canonical",
            "invalid-noncanonical.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "canonical-unknown-field",
            "manifest-document",
            "invalid-unknown-manifest.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "trust-rollback",
            "floor",
            "trust-rollback.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "trust-equivocation",
            "floor",
            "trust-equivocation.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "channel-rollback",
            "floor",
            "channel-rollback.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "channel-equivocation",
            "floor",
            "channel-equivocation.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "minisign-key-malformed",
            "verify-directory",
            "trust-key-base64-noncanonical",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "minisign-key-wrong-length",
            "verify-directory",
            "trust-key-wrong-length",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "minisign-key-wrong-algorithm",
            "verify-directory",
            "trust-key-wrong-algorithm",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "minisign-key-embedded-id-disagreement",
            "verify-directory",
            "trust-key-embedded-id-disagreement",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(Floor {
                    number: 2,
                    sha256: digests.embedded_id_trust.clone(),
                }),
                channel: None,
            },
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
            "minisign-key-reused-within-role",
            "verify-directory",
            "trust-key-reused-within-role",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "minisign-key-reused-with-root",
            "verify-directory",
            "trust-key-reused-with-root",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        FixtureCase {
            clock: "2026-08-04T12:00:01Z".into(),
            ..case(
                "delegation-expired",
                "verify-directory",
                "delegation-expired",
                "refused",
                "x86_64-unknown-linux-gnu",
                RollbackState {
                    trust: Some(Floor {
                        number: 2,
                        sha256: digests.expired_delegation_trust.clone(),
                    }),
                    channel: None,
                },
            )
        },
        case(
            "role-confusion",
            "verify-directory",
            "role-confusion",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-empty",
            "verify-directory",
            "key-id-empty",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-overlong",
            "verify-directory",
            "key-id-overlong",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-slash",
            "verify-directory",
            "key-id-slash",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-double-hyphen",
            "verify-directory",
            "key-id-double-hyphen",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-leading-punctuation",
            "verify-directory",
            "key-id-leading-punctuation",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-trailing-punctuation",
            "verify-directory",
            "key-id-trailing-punctuation",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-nonascii",
            "verify-directory",
            "key-id-nonascii",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "key-id-uppercase",
            "verify-directory",
            "key-id-uppercase",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "trust-future-issued",
            "verify-directory",
            "trust-future-issued",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "root-threshold-failure",
            "root-threshold",
            "root-policy-threshold-two.test.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "trust-signature-missing",
            "verify-directory",
            "trust-signature-missing",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "trust-signature-substituted",
            "verify-directory",
            "trust-signature-substituted",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "release-threshold-failure",
            "verify-directory",
            "release-threshold-failure",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(Floor {
                    number: 2,
                    sha256: digests.release_threshold.clone(),
                }),
                channel: None,
            },
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
            "channel-future-issued",
            "verify-directory",
            "channel-future-issued",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: None,
            },
        ),
        case(
            "channel-threshold-failure",
            "verify-directory",
            "channel-threshold-failure",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: Some(Floor {
                    number: 2,
                    sha256: digests.threshold_trust.clone(),
                }),
                channel: None,
            },
        ),
        case(
            "channel-signature-missing",
            "verify-directory",
            "channel-signature-missing",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: None,
            },
        ),
        case(
            "channel-signature-substituted",
            "verify-directory",
            "channel-signature-substituted",
            "refused",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
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
                    sha256: digests.digest_channel.clone(),
                }),
            },
        ),
        case(
            "manifest-signature-missing",
            "verify-directory",
            "manifest-signature-missing",
            "refused",
            "x86_64-unknown-linux-gnu",
            accepted.clone(),
        ),
        case(
            "manifest-signature-key-id-disagree",
            "verify-directory",
            "manifest-signature-key-id-disagree",
            "refused",
            "x86_64-unknown-linux-gnu",
            accepted.clone(),
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
                    sha256: digests.incompatible_channel.clone(),
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
            "decimal-sign",
            "readiness",
            "readiness-decimal-sign.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "decimal-21-digits",
            "readiness",
            "readiness-decimal-21-digits.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "decimal-overflow",
            "readiness",
            "readiness-decimal-overflow.json",
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
            "readiness-bind-scheme",
            "readiness",
            "readiness-bad-scheme.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-port-zero",
            "readiness",
            "readiness-port-zero.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-port-overflow",
            "readiness",
            "readiness-port-overflow.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-pid-zero",
            "readiness",
            "readiness-pid-zero.json",
            "refused",
            "x86_64-unknown-linux-gnu",
            empty.clone(),
        ),
        case(
            "readiness-pid-overflow",
            "readiness",
            "readiness-pid-overflow.json",
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
            "github-redirect-userinfo",
            "transport",
            "redirect-userinfo.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-fragment",
            "transport",
            "redirect-fragment.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-location-bound",
            "transport",
            "redirect-location-bound.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-location-nonascii",
            "transport",
            "redirect-location-nonascii.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-empty",
            "transport",
            "redirect-query-empty.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-value-bound",
            "transport",
            "redirect-query-value-bound.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-duplicate",
            "transport",
            "redirect-query-duplicate.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-encoded-name",
            "transport",
            "redirect-query-encoded-name.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-percent",
            "transport",
            "redirect-query-percent.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-control",
            "transport",
            "redirect-query-control.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-query-raw-character",
            "transport",
            "redirect-query-raw-character.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-path-repository",
            "transport",
            "redirect-path-repository.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-path-uuid",
            "transport",
            "redirect-path-uuid.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-redirect-final-status",
            "transport",
            "redirect-final-status.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-trust",
            "initial-transport",
            "initial-trust-positive.json",
            "accepted",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-channel",
            "initial-transport",
            "initial-channel-positive.json",
            "accepted",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-immutable",
            "initial-transport",
            "initial-immutable-positive.json",
            "accepted",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-scheme",
            "initial-transport",
            "initial-scheme.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-host",
            "initial-transport",
            "initial-host.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-port",
            "initial-transport",
            "initial-port.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-userinfo",
            "initial-transport",
            "initial-userinfo.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-fragment",
            "initial-transport",
            "initial-fragment.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-query",
            "initial-transport",
            "initial-query.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-repository",
            "initial-transport",
            "initial-repository.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-tag",
            "initial-transport",
            "initial-tag.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-basename",
            "initial-transport",
            "initial-basename.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-mutable-form",
            "initial-transport",
            "initial-mutable-latest.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-authorization",
            "initial-transport",
            "initial-authorization.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-proxy-authorization",
            "initial-transport",
            "initial-proxy-authorization.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "github-initial-cookie",
            "initial-transport",
            "initial-cookie.json",
            "refused",
            "none",
            empty.clone(),
        ),
        case(
            "higher-incompatible-skipped",
            "verify-directory",
            "selection-higher-incompatible",
            "accepted",
            "x86_64-unknown-linux-gnu",
            RollbackState {
                trust: accepted.trust.clone(),
                channel: Some(Floor {
                    number: 12,
                    sha256: digests.higher_incompatible_channel.clone(),
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
                    sha256: digests.multi_channel.clone(),
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
    cases.push(case(
        "plugin-or-connector-executable",
        "verify-directory",
        "forbidden-executable",
        "refused",
        "aarch64-apple-darwin",
        RollbackState {
            trust: accepted.trust.clone(),
            channel: Some(Floor {
                number: 7,
                sha256: digests.forbidden_channel.clone(),
            }),
        },
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
            sha256: digests.higher_trust.clone(),
        }),
        channel: Some(Floor {
            number: 10,
            sha256: digests.higher_channel.clone(),
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

fn malformed_public_keys(valid: &str) -> anyhow::Result<Vec<(&'static str, String)>> {
    let decoded = base64::engine::general_purpose::STANDARD_NO_PAD.decode(valid)?;
    let mut wrong_algorithm = decoded.clone();
    wrong_algorithm[..2].copy_from_slice(b"ED");
    let mut embedded_id = decoded;
    embedded_id[2] ^= 1;
    Ok(vec![
        ("trust-key-base64-noncanonical", format!("{valid}=")),
        ("trust-key-wrong-length", valid[..55].to_owned()),
        (
            "trust-key-wrong-algorithm",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(wrong_algorithm),
        ),
        (
            "trust-key-embedded-id-disagreement",
            base64::engine::general_purpose::STANDARD_NO_PAD.encode(embedded_id),
        ),
    ])
}

fn reidentify_public_key(valid: &str) -> anyhow::Result<String> {
    let mut decoded = base64::engine::general_purpose::STANDARD_NO_PAD.decode(valid)?;
    decoded[2] ^= 1;
    Ok(base64::engine::general_purpose::STANDARD_NO_PAD.encode(decoded))
}

fn forbidden_executable_variant(
    root: &Path,
    positive: &Path,
    manifest: &Manifest,
    channel: &Channel,
    release_signer: &KeyPair,
    release_id: &str,
    channel_signers: &[(&KeyPair, &str)],
) -> anyhow::Result<()> {
    let directory = root.join("forbidden-executable");
    copy_directory(positive, &directory)?;
    let mut changed_manifest = manifest.clone();
    let asset = &mut changed_manifest.assets[0];
    if asset.format != "tar.zst" {
        anyhow::bail!("first fixture asset is not the expected Unix archive");
    }
    let archive_bytes = std::fs::read(positive.join(&asset.archive))?;
    let decoder = zstd::stream::read::Decoder::new(Cursor::new(archive_bytes))?;
    let mut archive = tar::Archive::new(decoder);
    let mut members = Vec::new();
    for member in archive.entries()? {
        let mut member = member?;
        let path = member.path()?.to_string_lossy().into_owned();
        let mut bytes = Vec::new();
        member.read_to_end(&mut bytes)?;
        let mode = if path.ends_with("/LICENSE-APACHE") {
            0o755
        } else {
            member.header().mode()?
        };
        members.push((path, bytes, mode));
    }
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, bytes, mode) in &members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(*mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            builder.append_data(&mut header, path, Cursor::new(bytes))?;
        }
        builder.finish()?;
    }
    let changed_archive = zstd::stream::encode_all(Cursor::new(tar_bytes), 19)?;
    write(&directory.join(&asset.archive), &changed_archive)?;
    asset.archive_bytes = changed_archive.len() as u64;
    asset.archive_sha256 = digest_hex(&changed_archive);
    let manifest_bytes = canonical::encode(&changed_manifest)?;
    write(
        &directory.join("flux-exchange-release-manifest.json"),
        &manifest_bytes,
    )?;
    sign_file(
        release_signer,
        &manifest_bytes,
        &directory.join(format!(
            "flux-exchange-release-manifest.json.{release_id}.minisig"
        )),
    )?;
    let mut changed_channel = channel.clone();
    changed_channel.releases[0].manifest_sha256 = digest_hex(&manifest_bytes);
    let channel_bytes = canonical::encode(&changed_channel)?;
    write(
        &directory.join("flux-exchange-release-channel.json"),
        &channel_bytes,
    )?;
    for (signer, id) in channel_signers {
        sign_file(
            signer,
            &channel_bytes,
            &directory.join(format!("flux-exchange-release-channel.json.{id}.minisig")),
        )?;
    }
    Ok(())
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
        "minisign-key-malformed"
        | "minisign-key-wrong-length"
        | "minisign-key-wrong-algorithm"
        | "minisign-key-embedded-id-disagreement"
        | "minisign-key-reused"
        | "minisign-key-reused-within-role"
        | "minisign-key-reused-with-root" => "signature refused",
        "root-threshold-failure"
        | "trust-signature-missing"
        | "trust-signature-substituted"
        | "channel-threshold-failure"
        | "channel-signature-missing"
        | "channel-signature-substituted"
        | "release-threshold-failure"
        | "manifest-signature-missing"
        | "manifest-signature-key-id-disagree" => "signature refused",
        "trust-future-issued" | "channel-future-issued" => "time refused",
        "delegation-expired" => "role/time",
        "role-confusion" => "signature refused",
        "channel-expired" | "expiry-equality-stopped" | "expiry-during-target-download" => {
            "time refused"
        }
        "manifest-digest-substituted" => "digest refused",
        "archive-corrupt-after-digest"
        | "archive-executable-substituted"
        | "archive-path-absolute"
        | "archive-path-parent"
        | "archive-path-backslash"
        | "archive-path-duplicate"
        | "archive-path-case-fold"
        | "archive-trailing-zstd-frame"
        | "archive-trailing-tar-data"
        | "archive-trailing-zip-data"
        | "archive-link-member"
        | "archive-device-member" => "archive refused",
        "channel-release-count-129"
        | "manifest-oversized"
        | "archive-oversized"
        | "archive-member-count-17"
        | "archive-member-oversized"
        | "archive-total-expanded-overflow"
        | "archive-member-decompression-bound"
        | "asset-missing-platform" => "bound refused",
        "higher-channel-no-compatible" => "no compatible Exchange release",
        "key-id-substituted"
        | "logical-origin-changed"
        | "foreign-origin"
        | "unsupported-protocol-set"
        | "id-or-basename-unsafe"
        | "manifest-tag-disagreement"
        | "manifest-version-disagreement"
        | "manifest-source-sha-disagreement"
        | "provenance-client-input" => "schema refused",
        id if id.starts_with("key-id-")
            || id.starts_with("basename-")
            || id.starts_with("protocol-") =>
        {
            "schema refused"
        }
        "asset-undeclared" => "undeclared staged asset",
        "plugin-or-connector-executable" | "archive-member-path-241" | "executable-renamed" => {
            "archive refused"
        }
        "github-redirect-query-bound" => "bound refused",
        "github-redirect-location-bound"
        | "github-redirect-location-nonascii"
        | "github-redirect-query-empty"
        | "github-redirect-query-value-bound" => "bound refused",
        id if id.starts_with("github-redirect") || id.starts_with("github-initial") => {
            "transport refused"
        }
        "decimal-overflow" => "bound refused",
        id if id.starts_with("readiness-")
            || matches!(
                id,
                "decimal-noncanonical" | "decimal-sign" | "decimal-21-digits"
            ) =>
        {
            "schema refused"
        }
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
