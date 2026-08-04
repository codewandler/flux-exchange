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
  flux-exchange-release self-test <tests/fixtures/exchange-release-v1>

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
        &Protocols::v1(),
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
    let bytes = std::fs::read(spec).with_context(|| format!("read {}", spec.display()))?;
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
            schema: "exchange.release-channel.v1".into(),
            channel: "stable".into(),
            origin: release::ORIGIN.into(),
            generation: 1,
            issued_at,
            expires_at,
            signing_key_ids: vec![signing_key_id],
            releases: vec![entry],
        })?
    } else {
        let existing_bytes =
            std::fs::read(existing_path).with_context(|| format!("read {existing_path}"))?;
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
    let keys = match role {
        "channel" => &trust.roles.channel.keys,
        "release" => &trust.roles.release.keys,
        _ => bail!("--role must be channel or release"),
    };
    let public_base64 = public.to_base64();
    let metadata_id = keys
        .iter()
        .find(|key| key.minisign_public_key == public_base64)
        .map(|key| key.key_id.as_str())
        .ok_or_else(|| anyhow!("secret public key is not delegated for role {role}"))?;
    let bytes = std::fs::read(payload).with_context(|| format!("read {}", payload.display()))?;
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
        key_id: metadata_id,
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
    let executable_bytes =
        std::fs::read(executable).with_context(|| format!("read {}", executable.display()))?;
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
    if fixture.schema != "exchange.release-fixture-set.v1" {
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
        let bytes = std::fs::read(directory.join(relative))
            .with_context(|| format!("read fixture {relative}"))?;
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
    for required in REQUIRED_PROVIDER_CASES {
        if !case_ids.contains(required) {
            bail!("fixture-set is missing required provider case {required:?}");
        }
    }
    let mut deferred = BTreeSet::new();
    for id in &fixture.deferred_cases {
        if !deferred.insert(id.as_str()) {
            bail!("fixture-set has duplicate deferred case {id:?}");
        }
        if case_ids.contains(id.as_str()) {
            bail!("fixture case {id:?} is both executed and deferred");
        }
    }
    for native in REQUIRED_NATIVE_CASES {
        if !case_ids.contains(native) && !deferred.contains(*native) {
            bail!("fixture-set neither executes nor defers native case {native:?}");
        }
    }
    status(&SelfTestStatus {
        schema: "exchange.release-self-test.v1",
        result: "accepted",
        cases: fixture.cases.len(),
        fixture_set_sha256: release::digest_hex(&std::fs::read(manifest_path)?),
    })
}

const REQUIRED_PROVIDER_CASES: &[&str] = &[
    "positive-linux",
    "positive-macos",
    "positive-windows",
    "positive-signer-overlap",
    "canonical-duplicate-field",
    "canonical-unknown-field",
    "canonical-noncanonical-bytes",
    "integer-over-jcs-safe",
    "decimal-noncanonical",
    "root-threshold-failure",
    "channel-threshold-failure",
    "release-threshold-failure",
    "trust-future-issued",
    "channel-future-issued",
    "trust-rollback",
    "trust-equivocation",
    "channel-rollback",
    "channel-equivocation",
    "trust-signature-missing",
    "trust-signature-substituted",
    "channel-signature-missing",
    "channel-signature-substituted",
    "manifest-signature-missing",
    "manifest-signature-key-id-disagree",
    "manifest-tag-disagreement",
    "manifest-version-disagreement",
    "manifest-source-sha-disagreement",
    "archive-path-absolute",
    "archive-path-parent",
    "archive-path-backslash",
    "archive-path-duplicate",
    "archive-path-case-fold",
    "archive-link-member",
    "archive-device-member",
    "archive-total-expanded-overflow",
    "archive-trailing-zstd-frame",
    "archive-trailing-tar-data",
    "archive-trailing-zip-data",
    "archive-member-decompression-bound",
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
    "github-redirect-positive",
    "github-redirect-status",
    "github-redirect-scheme",
    "github-redirect-host",
    "github-redirect-port",
    "github-redirect-userinfo",
    "github-redirect-fragment",
    "github-redirect-path",
    "github-redirect-path-repository",
    "github-redirect-path-uuid",
    "github-redirect-query-name",
    "github-redirect-query-bound",
    "github-redirect-query-empty",
    "github-redirect-query-value-bound",
    "github-redirect-query-duplicate",
    "github-redirect-query-encoded-name",
    "github-redirect-query-percent",
    "github-redirect-query-control",
    "github-redirect-query-raw-character",
    "github-redirect-location-bound",
    "github-redirect-location-nonascii",
    "github-redirect-credential",
    "github-redirect-second-redirect",
    "github-redirect-final-status",
];

const REQUIRED_NATIVE_CASES: &[&str] = &[
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
                canonical::parse_value(&std::fs::read(input)?, 256 * 1024)?;
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
                    &Protocols::v1(),
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
                    &Protocols::v1(),
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
                    &std::fs::read(&input)?,
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
        "plugin-or-connector-executable" => {
            let mut member = manifest.assets[0].other_members[0].clone();
            member.path = member.path.replace("LICENSE-APACHE", "connector-plugin");
            manifest.assets[0].other_members.push(member);
            manifest.assets[0]
                .other_members
                .sort_by(|a, b| a.path.cmp(&b.path));
        }
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
            manifest.protocols.supervisor = "exchange.supervisor-ready.v2".into()
        }
        "id-or-basename-unsafe" => manifest.assets[0].archive = "../unsafe".into(),
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
            let mut bytes = std::fs::read(&archive)?;
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
    let mut bytes = std::fs::read(&archive)?;
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
        &std::fs::read(path).with_context(|| format!("read {}", path.display()))?,
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
    if let Ok(existing) = std::fs::read(path) {
        if existing == bytes {
            return Ok(());
        }
        bail!(
            "refusing to overwrite different bytes at {}",
            path.display()
        );
    }
    std::fs::write(path, bytes).with_context(|| format!("write {}", path.display()))
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
