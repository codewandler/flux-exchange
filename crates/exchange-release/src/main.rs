use anyhow::{anyhow, bail, Context};
use base64::Engine as _;
use flux_exchange_release as release;
use minisign::{PublicKey, SecretKeyBox};
use release::{
    canonical, Channel, FixtureSet, Manifest, Protocols, ReleaseEntry, RollbackState, RootPolicy,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

const HELP: &str = r#"Provider verifier for Exchange local release protocol v1.

USAGE:
  flux-exchange-release verify-staged <dir> --root-policy <file> --now <UTC-seconds> [--state <file>] [--allow-test-policy]
  flux-exchange-release verify-offline <dir> --target <triple> --root-policy <file> --now <UTC-seconds> [--state <file>] [--allow-test-policy]
  flux-exchange-release verify-published <dir> --target <triple> --root-policy <file> --now <UTC-seconds> [--state <file>]
  flux-exchange-release verify-trust <dir> --root-policy <file> --now <UTC-seconds> [--allow-test-policy]
  flux-exchange-release verify-channel <dir> --root-policy <file> --now <UTC-seconds> [--state <file>] [--allow-test-policy]
  flux-exchange-release stage-release --spec <manifest-draft.json> --directory <dir> --output <manifest.json>
  flux-exchange-release package --version <v> --target <triple> --executable <path> --license LICENSE-APACHE --license LICENSE-MIT [--documentation README.md] --output-directory <dir>
  flux-exchange-release update-channel --existing <channel.json|-> --entry <entry.json> --issued-at <UTC-seconds> --expires-at <UTC-seconds> --signing-key-id <id> --output <channel.json>
  flux-exchange-release sign <payload> --secret-key-env <NAME> --trust-directory <dir> --root-policy <file> --now <UTC-seconds> --role <channel|release> --output-directory <dir>
  flux-exchange-release verify-transport-fixture <fixture.json>
  flux-exchange-release verify-compatibility --executable <path> --entry <entry.json> --executable-sha256 <digest>
  flux-exchange-release self-test <tests/fixtures/exchange-release-v2>

Every production verification command requires explicit pinned root policy. Policies marked
test_only are refused unless --allow-test-policy is explicit; self-test is the only command that
automatically enables it. No key or mutable network origin is built in.
"#;

fn main() {
    if let Err(error) = run(std::env::args().skip(1).collect()) {
        eprintln!("flux-exchange-release: {error:#}");
        std::process::exit(1);
    }
}

fn run(arguments: Vec<String>) -> anyhow::Result<()> {
    let Some(command) = arguments.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    match command {
        "help" | "--help" | "-h" => print!("{HELP}"),
        "verify-staged" => verify_set(&arguments[1..], None, true, false)?,
        "verify-offline" => verify_set(
            &arguments[1..],
            Some(required_option(&arguments[1..], "--target")?),
            false,
            false,
        )?,
        "verify-published" => verify_set(
            &arguments[1..],
            Some(required_option(&arguments[1..], "--target")?),
            true,
            true,
        )?,
        "verify-trust" => verify_trust(&arguments[1..])?,
        "verify-channel" => verify_channel(&arguments[1..])?,
        "stage-release" => stage_release(&arguments[1..])?,
        "package" => package(&arguments[1..])?,
        "update-channel" => update_channel(&arguments[1..])?,
        "sign" => sign(&arguments[1..])?,
        "verify-transport-fixture" => {
            verify_transport_fixture(Path::new(positional(&arguments[1..], 0)?))?
        }
        "verify-compatibility" => verify_compatibility(&arguments[1..])?,
        "self-test" => self_test(Path::new(positional(&arguments[1..], 0)?))?,
        unknown => bail!("unknown command {unknown:?}\n\n{HELP}"),
    }
    Ok(())
}

fn verify_set(
    arguments: &[String],
    target: Option<&str>,
    full_set: bool,
    run_compatibility: bool,
) -> anyhow::Result<()> {
    let directory = Path::new(positional(arguments, 0)?);
    let policy = read_policy(required_option(arguments, "--root-policy")?)?;
    admit_policy(&policy, flag(arguments, "--allow-test-policy"))?;
    let now = release::parse_utc(required_option(arguments, "--now")?)?;
    let prior = option(arguments, "--state")?
        .map(|path| read_canonical::<RollbackState>(Path::new(&path), 16 * 1024))
        .transpose()?
        .unwrap_or_default();
    let archive_target = if full_set { None } else { target };
    let verified = release::verify_directory(
        directory,
        &policy,
        now,
        &Protocols::v2(),
        &prior,
        archive_target,
    )?;
    let allowed = closed_client_files(
        directory,
        &verified.manifest,
        if full_set { None } else { target },
    )?;
    for entry in
        std::fs::read_dir(directory).with_context(|| format!("read {}", directory.display()))?
    {
        let entry = entry?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| anyhow!("non-UTF-8 release filename"))?;
        if !entry.file_type()?.is_file() || !allowed.contains_key(&name) {
            bail!("undeclared or non-file client asset {name:?}");
        }
    }
    if allowed.len() != std::fs::read_dir(directory)?.count() {
        bail!("client set is missing a declared metadata, signature or archive file");
    }
    if run_compatibility {
        let host_target =
            target.ok_or_else(|| anyhow!("published verification requires a host target"))?;
        let asset = verified
            .manifest
            .assets
            .iter()
            .find(|asset| asset.target == host_target)
            .ok_or_else(|| anyhow!("host target has no asset"))?;
        let bytes = release::verified_executable_bytes(directory, asset)?;
        let temporary = tempfile::tempdir().context("create private compatibility directory")?;
        let executable = temporary.path().join(if host_target.contains("windows") {
            "flux-exchange.exe"
        } else {
            "flux-exchange"
        });
        std::fs::write(&executable, bytes)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700))?;
        }
        execute_compatibility(&executable, &verified.selected, &asset.executable.sha256)?;
    }
    status(&VerifyStatus {
        schema: "exchange.release-verification.v1",
        result: "accepted",
        version: &verified.selected.version,
        target,
        trust_floor: verified.state.trust.as_ref().map(|floor| floor.number),
        channel_floor: verified.state.channel.as_ref().map(|floor| floor.number),
    })
}

fn verify_trust(arguments: &[String]) -> anyhow::Result<()> {
    let directory = Path::new(positional(arguments, 0)?);
    let policy = read_policy(required_option(arguments, "--root-policy")?)?;
    admit_policy(&policy, flag(arguments, "--allow-test-policy"))?;
    let now = release::parse_utc(required_option(arguments, "--now")?)?;
    let trust = release::verify_trust_document(directory, &policy, now)?;
    let requirements = options(arguments, "--require-secret")?;
    let mut matched_channel = Vec::new();
    let mut matched_release = Vec::new();
    for requirement in requirements {
        let (role, environment) = requirement
            .split_once(':')
            .ok_or_else(|| anyhow!("--require-secret must be role:ENV"))?;
        let (_, public) = secret_from_environment(environment)?;
        let public = public.to_base64();
        let keys = match role {
            "channel" => &trust.roles.channel.keys,
            "release" => &trust.roles.release.keys,
            _ => bail!("secret role must be channel or release"),
        };
        let key = keys
            .iter()
            .find(|key| key.minisign_public_key == public)
            .ok_or_else(|| anyhow!("{environment} is not delegated for {role}"))?;
        let before = release::parse_utc(&key.not_before)?;
        let after = release::parse_utc(&key.not_after)?;
        if !(before <= now && now < after) {
            bail!("{environment} delegation is not currently valid");
        }
        match role {
            "channel" => matched_channel.push(key.key_id.as_str()),
            "release" => matched_release.push(key.key_id.as_str()),
            _ => unreachable!(),
        }
    }
    let channel_key_ids = if matched_channel.is_empty() {
        trust
            .roles
            .channel
            .keys
            .iter()
            .map(|key| key.key_id.as_str())
            .collect()
    } else {
        matched_channel
    };
    let release_key_ids = if matched_release.is_empty() {
        trust
            .roles
            .release
            .keys
            .iter()
            .map(|key| key.key_id.as_str())
            .collect()
    } else {
        matched_release
    };
    status(&TrustStatus {
        schema: "exchange.release-trust-verification.v1",
        result: "accepted",
        version: trust.version,
        channel_key_ids,
        release_key_ids,
    })
}

fn verify_channel(arguments: &[String]) -> anyhow::Result<()> {
    let directory = Path::new(positional(arguments, 0)?);
    let policy = read_policy(required_option(arguments, "--root-policy")?)?;
    admit_policy(&policy, flag(arguments, "--allow-test-policy"))?;
    let now = release::parse_utc(required_option(arguments, "--now")?)?;
    let prior = option(arguments, "--state")?
        .map(|path| read_canonical::<RollbackState>(Path::new(&path), 16 * 1024))
        .transpose()?
        .unwrap_or_default();
    let verified = release::verify_metadata(directory, &policy, now, &prior)?;
    status(&ChannelStatus {
        schema: "exchange.release-channel-verification.v1",
        result: "accepted",
        generation: verified.channel.generation,
        sha256: verified
            .state
            .channel
            .as_ref()
            .map(|floor| floor.sha256.as_str()),
    })
}

fn stage_release(arguments: &[String]) -> anyhow::Result<()> {
    let spec = Path::new(required_option(arguments, "--spec")?);
    let directory = Path::new(required_option(arguments, "--directory")?);
    let output = Path::new(required_option(arguments, "--output")?);
    let bytes = release::read_bounded_file(spec, 256 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).context("parse manifest draft")?;
    let canonical = release::stage_manifest(directory, &manifest)?;
    write_new_or_identical(output, &canonical)?;
    status(&OutputStatus {
        schema: "exchange.release-output.v1",
        output: output.display().to_string(),
        sha256: release::digest_hex(&canonical),
    })
}

fn package(arguments: &[String]) -> anyhow::Result<()> {
    let licenses: Vec<PathBuf> = options(arguments, "--license")?
        .into_iter()
        .map(PathBuf::from)
        .collect();
    let asset = release::package_archive(
        required_option(arguments, "--version")?,
        required_option(arguments, "--target")?,
        Path::new(required_option(arguments, "--executable")?),
        &licenses,
        option(arguments, "--documentation")?.map(Path::new),
        Path::new(required_option(arguments, "--output-directory")?),
    )?;
    status(&asset)
}

fn update_channel(arguments: &[String]) -> anyhow::Result<()> {
    let existing_path = required_option(arguments, "--existing")?;
    let entry: ReleaseEntry =
        read_canonical(Path::new(required_option(arguments, "--entry")?), 64 * 1024)?;
    let issued_at = required_option(arguments, "--issued-at")?.to_owned();
    let expires_at = required_option(arguments, "--expires-at")?.to_owned();
    release::parse_utc(&issued_at)?;
    release::parse_utc(&expires_at)?;
    let signing_key_id = required_option(arguments, "--signing-key-id")?.to_owned();
    let output = Path::new(required_option(arguments, "--output")?);
    let bytes = if existing_path == "-" {
        canonical::encode(&Channel {
            schema: "exchange.release-channel.v2".into(),
            channel: "stable".into(),
            origin: release::ORIGIN.into(),
            generation: 1,
            issued_at,
            expires_at,
            signing_key_ids: vec![signing_key_id],
            releases: vec![entry],
        })?
    } else {
        let existing_bytes = release::read_bounded_file(Path::new(existing_path), 256 * 1024)?;
        let mut existing: Channel = canonical::parse(&existing_bytes, 256 * 1024)?;
        if let Some(prior) = existing
            .releases
            .iter()
            .find(|prior| prior.version == entry.version || prior.tag == entry.tag)
        {
            if prior != &entry {
                bail!("release identity equivocation for {}", entry.version);
            }
            existing_bytes
        } else {
            let last = existing
                .releases
                .last()
                .ok_or_else(|| anyhow!("existing channel is empty"))?;
            if semver::Version::parse(&entry.version)? <= semver::Version::parse(&last.version)? {
                bail!("new release is not greater than the channel tail");
            }
            existing.generation = existing
                .generation
                .checked_add(1)
                .filter(|value| *value <= release::JCS_SAFE_INTEGER)
                .ok_or_else(|| anyhow!("channel generation overflow"))?;
            existing.issued_at = issued_at;
            existing.expires_at = expires_at;
            existing.signing_key_ids = vec![signing_key_id];
            existing.releases.push(entry);
            canonical::encode(&existing)?
        }
    };
    write_new_or_identical(output, &bytes)?;
    status(&OutputStatus {
        schema: "exchange.release-output.v1",
        output: output.display().to_string(),
        sha256: release::digest_hex(&bytes),
    })
}

fn sign(arguments: &[String]) -> anyhow::Result<()> {
    let payload = Path::new(positional(arguments, 0)?);
    let environment = required_option(arguments, "--secret-key-env")?;
    let (secret, public) = secret_from_environment(environment)?;
    let trust_directory = Path::new(required_option(arguments, "--trust-directory")?);
    let policy = read_policy(required_option(arguments, "--root-policy")?)?;
    admit_policy(&policy, flag(arguments, "--allow-test-policy"))?;
    let now = release::parse_utc(required_option(arguments, "--now")?)?;
    let trust = release::verify_trust_document(trust_directory, &policy, now)?;
    let role = required_option(arguments, "--role")?;
    let public_base64 = public.to_base64();
    let metadata_id = release::delegated_signing_key_id(&trust, role, &public_base64, now)?;
    let bytes = release::read_bounded_file(payload, 256 * 1024)?;
    let signature = minisign::sign(
        Some(&public),
        &secret,
        Cursor::new(bytes),
        Some("exchange.release.v1"),
        Some("untrusted comment: Exchange release v1"),
    )?;
    let basename = payload
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| anyhow!("payload has no UTF-8 basename"))?;
    let output = Path::new(required_option(arguments, "--output-directory")?)
        .join(format!("{basename}.{metadata_id}.minisig"));
    let text = signature.into_string();
    write_new_or_identical(&output, text.as_bytes())?;
    status(&SignStatus {
        schema: "exchange.release-signature.v1",
        key_id: &metadata_id,
        minisign_public_key: &public_base64,
        output: output.display().to_string(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TransportFixture {
    status: u16,
    location: String,
    forwarded_credentials: bool,
    final_status: u16,
    second_redirect: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InitialTransportFixture {
    credential_kind: String,
    kind: String,
    url: String,
}

fn verify_transport_fixture(path: &Path) -> anyhow::Result<()> {
    validate_transport_fixture(path)?;
    status(&SimpleStatus {
        schema: "exchange.release-transport-verification.v1",
        result: "accepted",
    })
}

fn validate_transport_fixture(path: &Path) -> anyhow::Result<()> {
    let fixture: TransportFixture = read_canonical(path, 16 * 1024)?;
    release::transport::validate_redirect(
        fixture.status,
        &fixture.location,
        fixture.forwarded_credentials,
    )?;
    release::transport::validate_final(fixture.final_status, fixture.second_redirect)?;
    Ok(())
}

fn verify_compatibility(arguments: &[String]) -> anyhow::Result<()> {
    let executable = Path::new(required_option(arguments, "--executable")?);
    let expected_digest = required_option(arguments, "--executable-sha256")?;
    let entry: ReleaseEntry =
        read_canonical(Path::new(required_option(arguments, "--entry")?), 64 * 1024)?;
    execute_compatibility(executable, &entry, expected_digest)?;
    status(&SimpleStatus {
        schema: "exchange.compatibility-verification.v1",
        result: "accepted",
    })
}

fn execute_compatibility(
    executable: &Path,
    entry: &ReleaseEntry,
    expected_digest: &str,
) -> anyhow::Result<()> {
    let executable_bytes = release::read_bounded_file(executable, release::MAX_MEMBER_BYTES)?;
    if release::digest_hex(&executable_bytes) != expected_digest {
        bail!("executable digest disagrees with signed target");
    }
    let output = std::process::Command::new(executable)
        .args(["compatibility", "--json"])
        .env_clear()
        .output()
        .with_context(|| format!("execute {}", executable.display()))?;
    if !output.status.success() || !output.stderr.is_empty() || output.stdout.len() > 16 * 1024 {
        bail!("compatibility command failed, wrote stderr, or exceeded 16 KiB");
    }
    release::verify_compatibility(&output.stdout, entry)?;
    Ok(())
}

fn self_test(directory: &Path) -> anyhow::Result<()> {
    let manifest_path = directory.join("fixture-set.json");
    let fixture: FixtureSet = read_canonical(&manifest_path, 256 * 1024)?;
    if fixture.schema != "exchange.release-fixture-set.v2" {
        bail!("unknown fixture-set schema");
    }
    let mut actual_files = BTreeMap::new();
    fixture_files(directory, directory, &mut actual_files)?;
    actual_files.remove("fixture-set.json");
    if actual_files.keys().ne(fixture.files.keys()) {
        bail!("fixture-set manifest is not the exact recursive file set");
    }
    for (relative, expected) in &fixture.files {
        safe_fixture_path(relative)?;
        let bytes =
            release::read_bounded_file(&directory.join(relative), release::MAX_ARCHIVE_BYTES)
                .with_context(|| format!("read bounded fixture {relative}"))?;
        if release::digest_hex(&bytes) != *expected {
            bail!("fixture digest disagreement for {relative}");
        }
    }
    let mut case_ids: BTreeSet<&str> = BTreeSet::new();
    for case in &fixture.cases {
        if !case_ids.insert(case.id.as_str()) {
            bail!("duplicate fixture case id {:?}", case.id);
        }
        safe_fixture_path(&case.input)?;
        if !fixture.files.contains_key(&case.input)
            && !fixture
                .files
                .keys()
                .any(|path| path.starts_with(&format!("{}/", case.input)))
        {
            bail!(
                "fixture case {} names an unlisted input {:?}",
                case.id,
                case.input
            );
        }
        if !matches!(case.expected_result.as_str(), "accepted" | "refused") {
            bail!("fixture case {} has an unknown expected_result", case.id);
        }
        if case.expected_stage != case.operation {
            bail!(
                "fixture case {} expected_stage does not name its executed validator",
                case.id
            );
        }
        let observation = execute_case(directory, case);
        let accepted = observation.result.is_ok();
        let expected = case.expected_result == "accepted";
        if accepted != expected {
            bail!(
                "fixture case {} expected {}, observed {}",
                case.id,
                case.expected_result,
                if accepted { "accepted" } else { "refused" }
            );
        }
        if observation.state != case.expected_state {
            bail!(
                "fixture case {} durable state disagrees: expected {:?}, observed {:?}",
                case.id,
                case.expected_state,
                observation.state
            );
        }
        if observation.install != case.expected_install {
            bail!(
                "fixture case {} installed identity changed unexpectedly",
                case.id
            );
        }
        if observation.stage != case.expected_stage {
            bail!(
                "fixture case {} refusal/acceptance stage disagrees",
                case.id
            );
        }
        match (&observation.result, &case.expected_error_contains) {
            (Err(error), Some(fragment)) if !format!("{error:#}").contains(fragment) => {
                bail!(
                    "fixture case {} error did not contain {:?}: {error:#}",
                    case.id,
                    fragment
                );
            }
            (Err(_), None) => bail!(
                "fixture case {} refused without a checked expected error",
                case.id
            ),
            (Ok(_), Some(_)) => bail!(
                "fixture case {} accepted but declares an expected error",
                case.id
            ),
            _ => {}
        }
    }
    let required_provider: BTreeSet<_> = REQUIRED_PROVIDER_CASES.iter().copied().collect();
    if required_provider.len() != REQUIRED_PROVIDER_CASES.len() {
        bail!("compiled required provider inventory contains a duplicate id");
    }
    if case_ids != required_provider {
        let missing: Vec<_> = required_provider.difference(&case_ids).copied().collect();
        let unexpected: Vec<_> = case_ids.difference(&required_provider).copied().collect();
        bail!(
            "fixture-set provider case inventory disagrees; missing={missing:?}; unexpected={unexpected:?}"
        );
    }
    let mut native_ids = BTreeSet::new();
    let mut native_bindings = BTreeSet::new();
    for native in &fixture.native_cases {
        if !native_ids.insert(native.id.as_str()) {
            bail!("fixture-set has duplicate native case {:?}", native.id);
        }
        if case_ids.contains(native.id.as_str()) {
            bail!("fixture case {:?} is both portable and native", native.id);
        }
        if native.evidence.is_empty() {
            bail!(
                "native fixture case {:?} has no process evidence",
                native.id
            );
        }
        for evidence in &native.evidence {
            let targets: BTreeSet<_> = evidence.targets.iter().map(String::as_str).collect();
            if targets.len() != evidence.targets.len()
                || targets
                    .iter()
                    .any(|target| !release::SUPPORTED_TARGETS.contains(target))
            {
                bail!(
                    "native fixture case {:?} has duplicate or unsupported targets",
                    native.id
                );
            }
            let target_list = targets.into_iter().collect::<Vec<_>>().join(",");
            if !native_bindings.insert((
                native.id.as_str(),
                target_list,
                evidence.test_target.as_str(),
                evidence.exact_test.as_str(),
            )) {
                bail!("fixture-set has duplicate native process evidence");
            }
        }
    }
    let required_native: BTreeSet<_> = REQUIRED_NATIVE_CASES.iter().copied().collect();
    if required_native.len() != REQUIRED_NATIVE_CASES.len() {
        bail!("compiled required native inventory contains a duplicate id");
    }
    if native_ids != required_native {
        let missing: Vec<_> = required_native.difference(&native_ids).copied().collect();
        let unexpected: Vec<_> = native_ids.difference(&required_native).copied().collect();
        bail!(
            "fixture-set native inventory disagrees; missing={missing:?}; unexpected={unexpected:?}"
        );
    }
    let required_bindings: BTreeSet<_> = REQUIRED_NATIVE_BINDINGS
        .iter()
        .map(|binding| {
            (
                binding.id,
                binding.targets.join(","),
                binding.test_target,
                binding.exact_test,
            )
        })
        .collect();
    if required_bindings.len() != REQUIRED_NATIVE_BINDINGS.len()
        || native_bindings != required_bindings
    {
        bail!("fixture-set native cases are not bound to the exact reviewed process tests");
    }
    status(&SelfTestStatus {
        schema: "exchange.release-self-test.v1",
        result: "accepted",
        cases: fixture.cases.len(),
        fixture_set_sha256: release::digest_hex(&release::read_bounded_file(
            &manifest_path,
            256 * 1024,
        )?),
    })
}

const REQUIRED_PROVIDER_CASES: &[&str] = &[
    "positive-linux",
    "positive-macos",
    "positive-windows",
    "positive-signer-overlap",
    "compatibility-positive",
    "integer-over-jcs-safe",
    "canonical-duplicate-field",
    "canonical-noncanonical-bytes",
    "canonical-unknown-field",
    "trust-rollback",
    "trust-equivocation",
    "channel-rollback",
    "channel-equivocation",
    "minisign-key-malformed",
    "minisign-key-wrong-length",
    "minisign-key-wrong-algorithm",
    "minisign-key-embedded-id-disagreement",
    "minisign-key-reused",
    "minisign-key-reused-within-role",
    "minisign-key-reused-with-root",
    "delegation-expired",
    "role-confusion",
    "key-id-empty",
    "key-id-overlong",
    "key-id-slash",
    "key-id-double-hyphen",
    "key-id-leading-punctuation",
    "key-id-trailing-punctuation",
    "key-id-nonascii",
    "key-id-uppercase",
    "trust-future-issued",
    "root-threshold-failure",
    "trust-signature-missing",
    "trust-signature-substituted",
    "release-threshold-failure",
    "channel-expired",
    "channel-future-issued",
    "channel-threshold-failure",
    "channel-signature-missing",
    "channel-signature-substituted",
    "expiry-equality-stopped",
    "manifest-digest-substituted",
    "manifest-signature-missing",
    "manifest-signature-key-id-disagree",
    "channel-release-count-129",
    "higher-channel-no-compatible",
    "decimal-noncanonical",
    "decimal-sign",
    "decimal-21-digits",
    "decimal-overflow",
    "readiness-bind-domain",
    "readiness-bind-scheme",
    "readiness-port-zero",
    "readiness-port-overflow",
    "readiness-pid-zero",
    "readiness-pid-overflow",
    "readiness-start-kind",
    "readiness-linux-start",
    "readiness-macos-start",
    "readiness-windows-start",
    "github-redirect-positive",
    "github-redirect-uppercase-host",
    "github-redirect-status",
    "github-redirect-scheme",
    "github-redirect-host",
    "github-redirect-port",
    "github-redirect-path",
    "github-redirect-query-name",
    "github-redirect-query-bound",
    "github-redirect-credential",
    "github-redirect-second-redirect",
    "github-redirect-userinfo",
    "github-redirect-fragment",
    "github-redirect-location-bound",
    "github-redirect-location-nonascii",
    "github-redirect-query-empty",
    "github-redirect-query-value-bound",
    "github-redirect-query-duplicate",
    "github-redirect-query-encoded-name",
    "github-redirect-query-percent",
    "github-redirect-query-control",
    "github-redirect-query-raw-character",
    "github-redirect-path-repository",
    "github-redirect-path-uuid",
    "github-redirect-final-status",
    "github-initial-trust",
    "github-initial-channel",
    "github-initial-immutable",
    "github-initial-scheme",
    "github-initial-host",
    "github-initial-port",
    "github-initial-userinfo",
    "github-initial-fragment",
    "github-initial-query",
    "github-initial-repository",
    "github-initial-tag",
    "github-initial-basename",
    "github-initial-mutable-form",
    "github-initial-authorization",
    "github-initial-proxy-authorization",
    "github-initial-cookie",
    "higher-incompatible-skipped",
    "newest-compatible-selected",
    "delegation-rollback",
    "provenance-client-input",
    "plugin-or-connector-executable",
    "expiry-during-target-download",
    "channel-floor-survives-rotation",
    "same-number-different-bytes",
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
    "higher-channel-target-fails",
    "higher-channel-target-fails-aarch64-unknown-linux-gnu",
    "higher-channel-target-fails-x86_64-apple-darwin",
    "higher-channel-target-fails-x86_64-pc-windows-msvc",
    "higher-channel-target-fails-x86_64-unknown-linux-gnu",
];

const REQUIRED_NATIVE_CASES: &[&str] = &[
    "four-form-secret-sentinel-process-scan",
    "production-root-inherited-environment",
    "windows-production-root-unsafe-metadata",
    "c515-server-lifetime-lease",
    "expiry-equality-live",
    "supervisor-death-normal-responsive-unix",
    "supervisor-death-normal-wedged-unix",
    "supervisor-death-sigkill-responsive-unix",
    "supervisor-death-sigkill-wedged-unix",
    "supervisor-death-terminate-responsive-windows",
    "supervisor-death-terminate-wedged-windows",
    "unix-inherited-abi",
    "windows-inherited-abi",
];

struct RequiredNativeBinding {
    id: &'static str,
    targets: &'static [&'static str],
    test_target: &'static str,
    exact_test: &'static str,
}

const UNIX_RELEASE_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "x86_64-unknown-linux-gnu",
];
const WINDOWS_RELEASE_TARGETS: &[&str] = &["x86_64-pc-windows-msvc"];

const REQUIRED_NATIVE_BINDINGS: &[RequiredNativeBinding] = &[
    RequiredNativeBinding {
        id: "four-form-secret-sentinel-process-scan",
        targets: release::SUPPORTED_TARGETS,
        test_target: "x134_sentinel_evidence",
        exact_test:
            "transformed_secret_sentinels_never_enter_refusal_abort_crash_or_restart_outputs",
    },
    RequiredNativeBinding {
        id: "production-root-inherited-environment",
        targets: release::SUPPORTED_TARGETS,
        test_target: "local_state_regressions",
        exact_test: "native_process_derives_production_root_from_the_authenticated_os_account",
    },
    RequiredNativeBinding {
        id: "windows-production-root-unsafe-metadata",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "windows_native_root_poisoning",
        exact_test:
            "windows_supervised_startup_refuses_reparse_point_owner_root_ancestor_without_repair",
    },
    RequiredNativeBinding {
        id: "windows-production-root-unsafe-metadata",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "windows_native_root_poisoning",
        exact_test:
            "windows_supervised_startup_refuses_untrusted_writable_owner_root_ancestor_without_repair",
    },
    RequiredNativeBinding {
        id: "c515-server-lifetime-lease",
        targets: release::SUPPORTED_TARGETS,
        test_target: "credential_store_process_lease",
        exact_test: "real_server_retains_the_c515_lease_through_recovery_and_readiness",
    },
    RequiredNativeBinding {
        id: "expiry-equality-live",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop",
    },
    RequiredNativeBinding {
        id: "expiry-equality-live",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "supervised_windows",
        exact_test: "verified_metadata_expiry_keeps_the_same_healthy_child_until_owner_stop",
    },
    RequiredNativeBinding {
        id: "supervisor-death-normal-responsive-unix",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "real_server_emits_one_canonical_record_after_bind_and_dies_on_liveness_eof",
    },
    RequiredNativeBinding {
        id: "supervisor-death-normal-wedged-unix",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "native_liveness_exits_an_exchange_whose_tokio_main_future_is_wedged",
    },
    RequiredNativeBinding {
        id: "supervisor-death-sigkill-responsive-unix",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test:
            "sigkill_of_the_real_supervisor_kills_a_responsive_exchange_and_releases_its_port",
    },
    RequiredNativeBinding {
        id: "supervisor-death-sigkill-wedged-unix",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test:
            "sigkill_of_the_real_supervisor_kills_a_tokio_wedged_exchange_and_releases_its_port",
    },
    RequiredNativeBinding {
        id: "supervisor-death-terminate-responsive-windows",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "supervised_windows",
        exact_test: "terminate_process_of_supervisor_kills_responsive_exchange_and_releases_port",
    },
    RequiredNativeBinding {
        id: "supervisor-death-terminate-wedged-windows",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "supervised_windows",
        exact_test: "terminate_process_of_supervisor_kills_wedged_exchange_and_releases_port",
    },
    RequiredNativeBinding {
        id: "unix-inherited-abi",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "exact_unix_abi_refuses_missing_and_wrong_capabilities",
    },
    RequiredNativeBinding {
        id: "unix-inherited-abi",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "unix_abi_refuses_alias_wrong_kind_direction_and_extra_inherited_fd",
    },
    RequiredNativeBinding {
        id: "unix-inherited-abi",
        targets: UNIX_RELEASE_TARGETS,
        test_target: "supervised_unix",
        exact_test: "unix_abi_refuses_each_missing_fd_and_does_not_discover_env_other_fd_or_stdout",
    },
    RequiredNativeBinding {
        id: "windows-inherited-abi",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "supervised_windows",
        exact_test: "malformed_windows_handle_flags_refuse_without_stdout_readiness",
    },
    RequiredNativeBinding {
        id: "windows-inherited-abi",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "supervised_windows",
        exact_test:
            "environment_stdout_and_handles_outside_the_explicit_list_are_not_capabilities",
    },
    RequiredNativeBinding {
        id: "windows-inherited-abi",
        targets: WINDOWS_RELEASE_TARGETS,
        test_target: "lib",
        exact_test:
            "supervisor::tests::windows_validator_refuses_noninherited_nonpipe_and_each_wrong_direction",
    },
];

struct CaseObservation {
    state: RollbackState,
    install: Option<release::InstalledIdentity>,
    stage: String,
    result: anyhow::Result<()>,
}

fn execute_case(directory: &Path, case: &release::FixtureCase) -> CaseObservation {
    let mut state = case.prior_state.clone();
    let install = case.prior_install.clone();
    let result = (|| -> anyhow::Result<()> {
        let input = directory.join(&case.input);
        match case.operation.as_str() {
            "canonical" => {
                canonical::parse_value(
                    &release::read_bounded_file(&input, 256 * 1024)?,
                    256 * 1024,
                )?;
            }
            "transport" => {
                validate_transport_fixture(&input)?;
            }
            "initial-transport" => {
                let fixture: InitialTransportFixture = read_canonical(&input, 16 * 1024)?;
                let resource = match fixture.kind.as_str() {
                    "trust" => release::transport::InitialResource::Trust,
                    "channel" => release::transport::InitialResource::Channel,
                    "immutable" => release::transport::InitialResource::Immutable {
                        version: "0.17.0",
                        basename: "flux-exchange-release-manifest.json",
                    },
                    unknown => bail!("unknown initial transport kind {unknown:?}"),
                };
                let credentials_present = match fixture.credential_kind.as_str() {
                    "none" => false,
                    "authorization" | "proxy-authorization" | "cookie" => true,
                    unknown => bail!("unknown credential kind {unknown:?}"),
                };
                release::transport::validate_initial_request(
                    &fixture.url,
                    resource,
                    credentials_present,
                )?;
            }
            "selection" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct Selection {
                    releases: Vec<ReleaseEntry>,
                    supported: Protocols,
                }
                let selection: Selection = read_canonical(&input, 256 * 1024)?;
                release::select_compatible(&selection.releases, &selection.supported)?;
            }
            "compatibility" => {
                let selected: ReleaseEntry =
                    read_canonical(&directory.join("release-entry.json"), 64 * 1024)?;
                release::verify_compatibility(
                    &release::read_bounded_file(&input, 16 * 1024)?,
                    &selected,
                )?;
            }
            "floor" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct FloorCase {
                    prior: Option<release::Floor>,
                    number: u64,
                    sha256: String,
                    kind: String,
                }
                let value: FloorCase = read_canonical(&input, 16 * 1024)?;
                release::advance_floor(
                    value.prior.as_ref(),
                    value.number,
                    &value.sha256,
                    &value.kind,
                )?;
            }
            "root-threshold" => {
                let policy = read_policy(&input.display().to_string())?;
                release::verify_trust_document(
                    &directory.join("positive"),
                    &policy,
                    release::parse_utc(&case.clock)?,
                )?;
            }
            "verify-directory" => {
                let policy = read_policy(
                    &directory
                        .join("root-policy.test.json")
                        .display()
                        .to_string(),
                )?;
                let now = release::parse_utc(&case.clock)?;
                let attempt = release::verify_directory_layered(
                    &input,
                    &policy,
                    now,
                    &Protocols::v2(),
                    &case.prior_state,
                    Some(&case.platform),
                );
                state = attempt.state;
                attempt.outcome?;
            }
            "verify-directory-expiry" => {
                #[derive(Deserialize)]
                #[serde(deny_unknown_fields)]
                struct ExpiryInput {
                    directory: String,
                    commit_clock: String,
                }
                let expiry: ExpiryInput = read_canonical(&input, 16 * 1024)?;
                safe_fixture_path(&expiry.directory)?;
                let policy = read_policy(
                    &directory
                        .join("root-policy.test.json")
                        .display()
                        .to_string(),
                )?;
                let attempt = release::verify_directory_layered_with_commit_clock(
                    &directory.join(expiry.directory),
                    &policy,
                    release::parse_utc(&case.clock)?,
                    release::parse_utc(&expiry.commit_clock)?,
                    &Protocols::v2(),
                    &case.prior_state,
                    Some(&case.platform),
                );
                state = attempt.state;
                attempt.outcome?;
            }
            "manifest-mutation" => execute_manifest_mutation(directory, &case.id)?,
            "readiness" => {
                let entry: ReleaseEntry =
                    read_canonical(&directory.join("release-entry.json"), 64 * 1024)?;
                release::verify_readiness(
                    &release::read_bounded_file(&input, 16 * 1024)?,
                    &case.platform,
                    &entry,
                    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                )?;
            }
            "manifest-document" => {
                let _: Manifest = read_canonical(&input, 256 * 1024)?;
            }
            unknown => bail!("fixture case {} has unknown operation {unknown:?}", case.id),
        }
        Ok(())
    })();
    CaseObservation {
        state,
        install,
        stage: case.operation.clone(),
        result,
    }
}

fn execute_manifest_mutation(directory: &Path, id: &str) -> anyhow::Result<()> {
    let positive = directory.join("positive");
    if id == "manifest-oversized" {
        let bytes = format!("{{\"padding\":\"{}\"}}", "x".repeat(256 * 1024));
        canonical::parse_value(bytes.as_bytes(), 256 * 1024)?;
        return Ok(());
    }
    let mut manifest: Manifest = read_canonical(
        &positive.join("flux-exchange-release-manifest.json"),
        256 * 1024,
    )?;
    let selected: ReleaseEntry = read_canonical(&directory.join("release-entry.json"), 64 * 1024)?;
    match id {
        "asset-missing-platform" => {
            manifest.assets.pop();
        }
        "asset-undeclared" => {
            let temporary = tempfile::tempdir()?;
            for entry in std::fs::read_dir(&positive)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    std::fs::copy(entry.path(), temporary.path().join(entry.file_name()))?;
                }
            }
            std::fs::write(temporary.path().join("undeclared.bin"), b"undeclared")?;
            let allowed = closed_client_files(temporary.path(), &manifest, None)?;
            if std::fs::read_dir(temporary.path())?.any(|entry| {
                entry.ok().is_some_and(|entry| {
                    !allowed.contains_key(&entry.file_name().to_string_lossy().into_owned())
                })
            }) {
                bail!("undeclared staged asset");
            }
            return Ok(());
        }
        "executable-renamed" => manifest.assets[0].executable.path.push_str("-renamed"),
        "archive-oversized" => manifest.assets[0].archive_bytes = release::MAX_ARCHIVE_BYTES + 1,
        "archive-member-count-17" => {
            while manifest.assets[0].other_members.len() < 16 {
                let mut member = manifest.assets[0].other_members[0].clone();
                member.path = format!(
                    "{}/doc-{:02}.md",
                    manifest.assets[0]
                        .executable
                        .path
                        .split('/')
                        .next()
                        .unwrap_or("root"),
                    manifest.assets[0].other_members.len()
                );
                manifest.assets[0].other_members.push(member);
            }
            manifest.assets[0]
                .other_members
                .sort_by(|a, b| a.path.cmp(&b.path));
        }
        "archive-member-oversized" => {
            manifest.assets[0].other_members[0].bytes = release::MAX_MEMBER_BYTES + 1
        }
        "archive-member-path-241" => {
            manifest.assets[0].other_members[0].path =
                format!("{}/{}", manifest.assets[0].target, "x".repeat(241))
        }
        "archive-executable-substituted" => manifest.assets[0].executable.sha256 = "b".repeat(64),
        "key-id-substituted" => manifest.signing_key_ids = vec!["Unsafe/Key".into()],
        "logical-origin-changed" | "foreign-origin" => {
            manifest.origin = "https://example.invalid/foreign".into()
        }
        "unsupported-protocol-set" => {
            manifest.protocols.supervisor = "exchange.supervisor-ready.v3".into()
        }
        "id-or-basename-unsafe" => manifest.assets[0].archive = "unsafe/name".into(),
        "basename-empty" => manifest.assets[0].archive.clear(),
        "basename-overlong" => manifest.assets[0].archive = "a".repeat(129),
        "basename-dotdot" => manifest.assets[0].archive = "unsafe..archive".into(),
        "basename-nonascii" => manifest.assets[0].archive = "archive-é".into(),
        "basename-leading-punctuation" => manifest.assets[0].archive = "-unsafe".into(),
        "basename-trailing-punctuation" => manifest.assets[0].archive = "unsafe-".into(),
        "protocol-empty" => manifest.protocols.exchange_api.clear(),
        "protocol-overlong" => manifest.protocols.exchange_api = format!("{}.v1", "a".repeat(126)),
        "protocol-no-version" => manifest.protocols.exchange_api = "exchange.api".into(),
        "protocol-version-zero" => manifest.protocols.exchange_api = "exchange.api.v0".into(),
        "protocol-version-leading-zero" => {
            manifest.protocols.exchange_api = "exchange.api.v01".into()
        }
        "protocol-empty-token" => manifest.protocols.exchange_api = "exchange..api.v1".into(),
        "protocol-double-hyphen" => manifest.protocols.exchange_api = "exchange.a--pi.v1".into(),
        "protocol-uppercase" => manifest.protocols.exchange_api = "Exchange.api.v1".into(),
        "protocol-leading-punctuation" => {
            manifest.protocols.exchange_api = "exchange.-api.v1".into()
        }
        "protocol-trailing-punctuation" => {
            manifest.protocols.exchange_api = "exchange.api-.v1".into()
        }
        "protocol-nonascii" => manifest.protocols.exchange_api = "exchange.apé.v1".into(),
        "manifest-tag-disagreement" => {
            manifest.tag = "refs/tags/v0.18.0".into();
            return release::verify_manifest_identity(&manifest, &selected).map_err(Into::into);
        }
        "manifest-version-disagreement" => {
            manifest.version = "0.18.0".into();
            return release::verify_manifest_identity(&manifest, &selected).map_err(Into::into);
        }
        "manifest-source-sha-disagreement" => {
            manifest.source_commit = "5e398a73dcb8de17466cbedea77122dd489bed4f".into();
            return release::verify_manifest_identity(&manifest, &selected).map_err(Into::into);
        }
        "archive-path-absolute" => manifest.assets[0].other_members[0].path = "/absolute".into(),
        "archive-path-parent" => manifest.assets[0].other_members[0].path = "root/../escape".into(),
        "archive-path-backslash" => {
            manifest.assets[0].other_members[0].path = "root\\escape".into()
        }
        "archive-path-duplicate" => {
            manifest.assets[0].other_members[0].path = manifest.assets[0].executable.path.clone()
        }
        "archive-path-case-fold" => {
            let mut duplicate = manifest.assets[0].other_members[0].clone();
            duplicate.path = duplicate.path.to_ascii_lowercase();
            manifest.assets[0].other_members.push(duplicate);
            manifest.assets[0]
                .other_members
                .sort_by(|left, right| left.path.cmp(&right.path));
        }
        "archive-total-expanded-overflow" => {
            manifest.assets[0].other_members[0].bytes = release::MAX_MEMBER_BYTES;
            manifest.assets[0].other_members[1].bytes = release::MAX_MEMBER_BYTES;
        }
        "archive-trailing-zstd-frame" => {
            return mutate_archive_and_stage(&positive, &manifest, 0, |bytes| {
                bytes.extend(zstd::stream::encode_all(Cursor::new(b"second-frame"), 19)?);
                Ok(())
            });
        }
        "archive-trailing-tar-data" => {
            return mutate_archive_and_stage(&positive, &manifest, 0, |bytes| {
                let mut expanded = Vec::new();
                zstd::stream::read::Decoder::new(Cursor::new(bytes.as_slice()))?
                    .read_to_end(&mut expanded)?;
                expanded.extend_from_slice(b"nonzero trailing tar bytes");
                *bytes = zstd::stream::encode_all(Cursor::new(expanded), 19)?;
                Ok(())
            });
        }
        "archive-trailing-zip-data" => {
            let index = manifest
                .assets
                .iter()
                .position(|asset| asset.format == "zip")
                .ok_or_else(|| anyhow!("fixture manifest has no zip asset"))?;
            return mutate_archive_and_stage(&positive, &manifest, index, |bytes| {
                bytes.extend_from_slice(b"trailing zip bytes");
                Ok(())
            });
        }
        "archive-member-decompression-bound" => {
            manifest.assets[0].executable.bytes -= 1;
        }
        "archive-link-member" | "archive-device-member" => {
            return execute_special_tar_member(id);
        }
        "archive-corrupt-after-digest" => {
            let temporary = tempfile::tempdir()?;
            for entry in std::fs::read_dir(&positive)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    std::fs::copy(entry.path(), temporary.path().join(entry.file_name()))?;
                }
            }
            let archive = temporary.path().join(&manifest.assets[0].archive);
            let mut bytes = release::read_bounded_file(&archive, release::MAX_ARCHIVE_BYTES)?;
            bytes.push(0);
            std::fs::write(&archive, bytes)?;
            return release::stage_manifest(temporary.path(), &manifest)
                .map(|_| ())
                .map_err(Into::into);
        }
        unknown => bail!("unknown manifest mutation {unknown}"),
    }
    release::stage_manifest(&positive, &manifest)?;
    Ok(())
}

fn mutate_archive_and_stage(
    positive: &Path,
    manifest: &Manifest,
    asset_index: usize,
    mutate: impl FnOnce(&mut Vec<u8>) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    for entry in std::fs::read_dir(positive)? {
        let entry = entry?;
        if entry.file_type()?.is_file() {
            std::fs::copy(entry.path(), temporary.path().join(entry.file_name()))?;
        }
    }
    let mut changed = manifest.clone();
    let archive = temporary.path().join(&changed.assets[asset_index].archive);
    let mut bytes = release::read_bounded_file(&archive, release::MAX_ARCHIVE_BYTES)?;
    mutate(&mut bytes)?;
    std::fs::write(&archive, &bytes)?;
    changed.assets[asset_index].archive_bytes = bytes.len() as u64;
    changed.assets[asset_index].archive_sha256 = release::digest_hex(&bytes);
    release::stage_manifest(temporary.path(), &changed)?;
    Ok(())
}

fn execute_special_tar_member(id: &str) -> anyhow::Result<()> {
    let temporary = tempfile::tempdir()?;
    let path = temporary.path().join("special.tar.zst");
    let file = std::fs::File::create(&path)?;
    let encoder = zstd::stream::write::Encoder::new(file, 19)?;
    let mut archive = tar::Builder::new(encoder);
    let mut header = tar::Header::new_gnu();
    header.set_size(0);
    header.set_mode(0o777);
    header.set_mtime(0);
    header.set_entry_type(if id == "archive-link-member" {
        tar::EntryType::Symlink
    } else {
        tar::EntryType::Char
    });
    header.set_cksum();
    archive.append_data(&mut header, "root/special", Cursor::new(Vec::<u8>::new()))?;
    archive.into_inner()?.finish()?;
    release::verify_archive(
        &path,
        "tar.zst",
        release::Platform::Unix,
        &[("root/special".into(), 0, release::digest_hex(&[]))],
    )?;
    Ok(())
}

fn closed_client_files(
    directory: &Path,
    manifest: &Manifest,
    target: Option<&str>,
) -> anyhow::Result<BTreeMap<String, ()>> {
    let mut files = BTreeMap::new();
    for basename in [
        "flux-exchange-release-trust.json",
        "flux-exchange-release-channel.json",
        "flux-exchange-release-manifest.json",
    ] {
        files.insert(basename.into(), ());
    }
    let trust: release::TrustDocument = read_canonical(
        &directory.join("flux-exchange-release-trust.json"),
        64 * 1024,
    )?;
    let channel: Channel = read_canonical(
        &directory.join("flux-exchange-release-channel.json"),
        256 * 1024,
    )?;
    for id in trust.root_signing_key_ids {
        files.insert(format!("flux-exchange-release-trust.json.{id}.minisig"), ());
    }
    for id in channel.signing_key_ids {
        files.insert(
            format!("flux-exchange-release-channel.json.{id}.minisig"),
            (),
        );
    }
    for id in &manifest.signing_key_ids {
        files.insert(
            format!("flux-exchange-release-manifest.json.{id}.minisig"),
            (),
        );
    }
    let mut found = false;
    for asset in &manifest.assets {
        if target.is_none_or(|target| asset.target == target) {
            files.insert(asset.archive.clone(), ());
            found = true;
        }
    }
    if !found {
        bail!("target not found");
    }
    Ok(files)
}

fn read_policy(path: &str) -> anyhow::Result<RootPolicy> {
    read_canonical(Path::new(path), 64 * 1024)
}
fn read_canonical<T: serde::de::DeserializeOwned>(path: &Path, limit: usize) -> anyhow::Result<T> {
    Ok(canonical::parse(
        &release::read_bounded_file(path, limit as u64)?,
        limit,
    )?)
}
fn admit_policy(policy: &RootPolicy, allow_test: bool) -> anyhow::Result<()> {
    if policy.test_only && !allow_test {
        bail!("test-only root policy refused outside explicit test mode");
    }
    Ok(())
}
fn positional(arguments: &[String], index: usize) -> anyhow::Result<&str> {
    arguments
        .iter()
        .filter(|arg| !arg.starts_with('-'))
        .nth(index)
        .map(String::as_str)
        .ok_or_else(|| anyhow!("missing positional argument {index}"))
}
fn required_option<'a>(arguments: &'a [String], name: &str) -> anyhow::Result<&'a str> {
    option(arguments, name)?.ok_or_else(|| anyhow!("missing required {name}"))
}
fn option<'a>(arguments: &'a [String], name: &str) -> anyhow::Result<Option<&'a str>> {
    match arguments.iter().position(|arg| arg == name) {
        Some(index) => Ok(Some(
            arguments
                .get(index + 1)
                .ok_or_else(|| anyhow!("{name} requires a value"))?
                .as_str(),
        )),
        None => Ok(None),
    }
}
fn options<'a>(arguments: &'a [String], name: &str) -> anyhow::Result<Vec<&'a str>> {
    arguments
        .iter()
        .enumerate()
        .filter(|(_, arg)| *arg == name)
        .map(|(index, _)| {
            arguments
                .get(index + 1)
                .map(String::as_str)
                .ok_or_else(|| anyhow!("{name} requires a value"))
        })
        .collect()
}
fn flag(arguments: &[String], name: &str) -> bool {
    arguments.iter().any(|argument| argument == name)
}
fn safe_fixture_path(path: &str) -> anyhow::Result<()> {
    if path.is_empty()
        || path.starts_with('/')
        || path.contains('\\')
        || path
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        bail!("unsafe fixture path {path:?}");
    }
    Ok(())
}
fn fixture_files(
    root: &Path,
    directory: &Path,
    files: &mut BTreeMap<String, ()>,
) -> anyhow::Result<()> {
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            bail!("fixture set contains symlink {}", entry.path().display());
        }
        if kind.is_dir() {
            fixture_files(root, &entry.path(), files)?;
            continue;
        }
        if !kind.is_file() {
            bail!("fixture set contains non-file {}", entry.path().display());
        }
        let relative = entry
            .path()
            .strip_prefix(root)?
            .to_str()
            .ok_or_else(|| anyhow!("fixture path is not UTF-8"))?
            .replace('\\', "/");
        safe_fixture_path(&relative)?;
        files.insert(relative, ());
    }
    Ok(())
}
fn secret_from_environment(environment: &str) -> anyhow::Result<(minisign::SecretKey, PublicKey)> {
    let encoded = std::env::var(environment)
        .with_context(|| format!("required secret environment {environment} is absent"))?;
    let boxed = base64::engine::general_purpose::STANDARD
        .decode(&encoded)
        .context("secret environment is not padded RFC4648 base64")?;
    if base64::engine::general_purpose::STANDARD.encode(&boxed) != encoded {
        bail!("secret environment base64 is noncanonical");
    }
    let boxed = String::from_utf8(boxed).context("decoded secret-key file is not UTF-8")?;
    let secret = SecretKeyBox::from_string(&boxed)?
        .into_secret_key(Some(String::new()))
        .context("secret key must be the unencrypted canonical minisign key file")?;
    let public = PublicKey::from_secret_key(&secret)?;
    Ok((secret, public))
}
fn write_new_or_identical(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    match release::read_bounded_file(path, bytes.len() as u64) {
        Ok(existing) => {
            if existing == bytes {
                return Ok(());
            }
            bail!(
                "refusing to overwrite different bytes at {}",
                path.display()
            );
        }
        Err(release::Error::Io(_, error)) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::write_new_or_identical;

    #[test]
    fn output_writer_only_creates_absent_or_accepts_identical_files() {
        let temporary = tempfile::tempdir().expect("tempdir");
        let path = temporary.path().join("output");
        write_new_or_identical(&path, b"first").expect("create absent output");
        write_new_or_identical(&path, b"first").expect("accept identical output");
        assert!(write_new_or_identical(&path, b"other").is_err());
        assert_eq!(std::fs::read(&path).expect("retained output"), b"first");
        assert!(write_new_or_identical(&path, b"x").is_err());
        assert_eq!(std::fs::read(&path).expect("retained output"), b"first");
    }
}
fn status(value: &impl Serialize) -> anyhow::Result<()> {
    let bytes = canonical::encode(value)?;
    println!("{}", String::from_utf8(bytes)?);
    Ok(())
}

#[derive(Serialize)]
struct SimpleStatus<'a> {
    schema: &'a str,
    result: &'a str,
}
#[derive(Serialize)]
struct VerifyStatus<'a> {
    schema: &'a str,
    result: &'a str,
    version: &'a str,
    target: Option<&'a str>,
    trust_floor: Option<u64>,
    channel_floor: Option<u64>,
}
#[derive(Serialize)]
struct TrustStatus<'a> {
    schema: &'a str,
    result: &'a str,
    version: u64,
    channel_key_ids: Vec<&'a str>,
    release_key_ids: Vec<&'a str>,
}
#[derive(Serialize)]
struct ChannelStatus<'a> {
    schema: &'a str,
    result: &'a str,
    generation: u64,
    sha256: Option<&'a str>,
}
#[derive(Serialize)]
struct OutputStatus {
    schema: &'static str,
    output: String,
    sha256: String,
}
#[derive(Serialize)]
struct SignStatus<'a> {
    schema: &'a str,
    key_id: &'a str,
    minisign_public_key: &'a str,
    output: String,
}
#[derive(Serialize)]
struct SelfTestStatus {
    schema: &'static str,
    result: &'static str,
    cases: usize,
    fixture_set_sha256: String,
}
