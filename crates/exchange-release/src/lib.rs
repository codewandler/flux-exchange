//! Provider-owned verifier for the Exchange local release protocol v1.

mod archive;
pub mod canonical;
mod model;
mod policy;
pub mod transport;

pub use model::*;

use semver::Version;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use time::OffsetDateTime;

pub const ORIGIN: &str = "https://github.com/codewandler/flux-exchange";
pub const JCS_SAFE_INTEGER: u64 = 9_007_199_254_740_991;
pub const MAX_ARCHIVE_BYTES: u64 = 268_435_456;
pub const MAX_MEMBER_BYTES: u64 = 268_435_456;
pub const MAX_EXPANDED_BYTES: u64 = 536_870_912;
pub const SUPPORTED_TARGETS: &[&str] = &[
    "aarch64-apple-darwin",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "x86_64-pc-windows-msvc",
    "x86_64-unknown-linux-gnu",
];

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("archive refused: {0}")]
    Archive(String),
    #[error("bound refused: {0}")]
    Bounds(String),
    #[error("canonical JSON refused: {0}")]
    Canonical(String),
    #[error("digest refused: {0}")]
    Digest(String),
    #[error("equivocation refused: {0}")]
    Equivocation(String),
    #[error("I/O at {0}: {1}")]
    Io(PathBuf, #[source] std::io::Error),
    #[error("JSON refused: {0}")]
    Json(String),
    #[error("rollback refused: {0}")]
    Rollback(String),
    #[error("schema refused: {0}")]
    Schema(String),
    #[error("no compatible Exchange release: {0}")]
    Selection(String),
    #[error("signature refused: {0}")]
    Signature(String),
    #[error("time refused: {0}")]
    Time(String),
    #[error("transport refused: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Read one regular file through one opened handle and a hard receive bound.
///
/// Metadata from the handle is only an early refusal. The bounded read remains authoritative, so
/// replacing or growing a path cannot turn the check into an unbounded allocation or make callers
/// verify bytes from one file and consume bytes reopened from another.
pub fn read_bounded_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut file = std::fs::File::open(path).map_err(|error| Error::Io(path.to_owned(), error))?;
    let metadata = file
        .metadata()
        .map_err(|error| Error::Io(path.to_owned(), error))?;
    if !metadata.is_file() {
        return Err(Error::Bounds(format!(
            "{} is not a regular file",
            path.display()
        )));
    }
    if metadata.len() > limit {
        return Err(Error::Bounds(format!(
            "{} exceeds {limit} bytes",
            path.display()
        )));
    }
    let capacity = usize::try_from(metadata.len().min(limit))
        .map_err(|_| Error::Bounds("file size does not fit this platform".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    file.by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| Error::Io(path.to_owned(), error))?;
    if bytes.len() as u64 > limit {
        return Err(Error::Bounds(format!(
            "{} grew past {limit} bytes while reading",
            path.display()
        )));
    }
    Ok(bytes)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Platform {
    Unix,
    Windows,
}

impl Platform {
    pub fn from_target(target: &str) -> Result<Self> {
        match target {
            "aarch64-apple-darwin"
            | "x86_64-apple-darwin"
            | "aarch64-unknown-linux-gnu"
            | "x86_64-unknown-linux-gnu" => Ok(Self::Unix),
            "x86_64-pc-windows-msvc" => Ok(Self::Windows),
            _ => Err(Error::Schema(format!("unsupported target {target:?}"))),
        }
    }
}

#[derive(Debug)]
pub struct Verification {
    pub selected: ReleaseEntry,
    pub manifest: Manifest,
    pub state: RollbackState,
}

#[derive(Debug)]
pub struct VerificationAttempt {
    /// Authenticated metadata floors reached before selection or target failure.
    pub state: RollbackState,
    /// Final selected release verification, or the named refusal after those durable floors.
    pub outcome: Result<(ReleaseEntry, Manifest)>,
}

#[derive(Debug)]
pub struct MetadataVerification {
    pub channel: Channel,
    pub state: RollbackState,
}

/// Verify trust, channel, deterministic compatibility selection and release assets.
///
/// The returned floors are advanced as soon as their authenticated metadata layer passes. Callers
/// persist each layer atomically before continuing to the next step.
pub fn verify_directory(
    directory: &Path,
    root_policy: &RootPolicy,
    now: OffsetDateTime,
    supported: &Protocols,
    prior: &RollbackState,
    target: Option<&str>,
) -> Result<Verification> {
    let attempt = verify_directory_layered(directory, root_policy, now, supported, prior, target);
    let (selected, manifest) = attempt.outcome?;
    Ok(Verification {
        selected,
        manifest,
        state: attempt.state,
    })
}

/// Verify layer-by-layer while retaining the exact authenticated floors reached before a refusal.
pub fn verify_directory_layered(
    directory: &Path,
    root_policy: &RootPolicy,
    now: OffsetDateTime,
    supported: &Protocols,
    prior: &RollbackState,
    target: Option<&str>,
) -> VerificationAttempt {
    verify_directory_layered_with_commit_clock(
        directory,
        root_policy,
        now,
        now,
        supported,
        prior,
        target,
    )
}

/// Verify with a separately injected target-commit clock to prove expiry during download.
pub fn verify_directory_layered_with_commit_clock(
    directory: &Path,
    root_policy: &RootPolicy,
    metadata_now: OffsetDateTime,
    commit_now: OffsetDateTime,
    supported: &Protocols,
    prior: &RollbackState,
    target: Option<&str>,
) -> VerificationAttempt {
    let mut advanced = prior.clone();
    let trust = match policy::verify_trust(directory, root_policy, metadata_now) {
        Ok(trust) => trust,
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    let trust_sha = digest_hex(&trust.bytes);
    let trust_floor = match policy::check_floor(
        prior.trust.as_ref(),
        trust.document.version,
        &trust_sha,
        "trust",
    ) {
        Ok(floor) => floor,
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    advanced.trust = Some(trust_floor);
    let (channel, channel_bytes) = match policy::verify_channel(directory, &trust, metadata_now) {
        Ok(channel) => channel,
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    let channel_sha = digest_hex(&channel_bytes);
    advanced.channel = match policy::check_floor(
        prior.channel.as_ref(),
        channel.generation,
        &channel_sha,
        "channel",
    ) {
        Ok(floor) => Some(floor),
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    let selected = match select_compatible(&channel.releases, supported) {
        Ok(selected) => selected.clone(),
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    let trust_expires = match policy::parse_time(&trust.document.expires_at) {
        Ok(value) => value,
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    let channel_expires = match policy::parse_time(&channel.expires_at) {
        Ok(value) => value,
        Err(error) => {
            return VerificationAttempt {
                state: advanced,
                outcome: Err(error),
            }
        }
    };
    if commit_now >= trust_expires || commit_now >= channel_expires {
        return VerificationAttempt {
            state: advanced,
            outcome: Err(Error::Time("metadata expired before target commit".into())),
        };
    }
    match policy::verify_manifest(directory, &trust, &selected, metadata_now, target) {
        Ok((manifest, _)) => VerificationAttempt {
            state: advanced,
            outcome: Ok((selected, manifest)),
        },
        Err(error) => VerificationAttempt {
            state: advanced,
            outcome: Err(error),
        },
    }
}

/// Verify trust and signed stable-channel metadata, advancing global floors before selection.
pub fn verify_metadata(
    directory: &Path,
    root_policy: &RootPolicy,
    now: OffsetDateTime,
    prior: &RollbackState,
) -> Result<MetadataVerification> {
    let trust = policy::verify_trust(directory, root_policy, now)?;
    let trust_sha = digest_hex(&trust.bytes);
    let trust_floor = policy::check_floor(
        prior.trust.as_ref(),
        trust.document.version,
        &trust_sha,
        "trust",
    )?;
    let (channel, bytes) = policy::verify_channel(directory, &trust, now)?;
    let channel_floor = policy::check_floor(
        prior.channel.as_ref(),
        channel.generation,
        &digest_hex(&bytes),
        "channel",
    )?;
    Ok(MetadataVerification {
        channel,
        state: RollbackState {
            trust: Some(trust_floor),
            channel: Some(channel_floor),
        },
    })
}

/// Verify only root-signed trust metadata for CI signer preflight.
pub fn verify_trust_document(
    directory: &Path,
    root_policy: &RootPolicy,
    now: OffsetDateTime,
) -> Result<TrustDocument> {
    policy::verify_trust(directory, root_policy, now).map(|verified| verified.document)
}

/// Resolve one delegated online signer and enforce its half-open validity at signing time.
pub fn delegated_signing_key_id(
    trust: &TrustDocument,
    role: &str,
    minisign_public_key: &str,
    now: OffsetDateTime,
) -> Result<String> {
    let keys = match role {
        "channel" => &trust.roles.channel.keys,
        "release" => &trust.roles.release.keys,
        _ => {
            return Err(Error::Signature(
                "signer role is not channel or release".into(),
            ))
        }
    };
    let key = keys
        .iter()
        .find(|key| key.minisign_public_key == minisign_public_key)
        .ok_or_else(|| Error::Signature(format!("public key is not delegated for role {role}")))?;
    let before = policy::parse_time(&key.not_before)?;
    let after = policy::parse_time(&key.not_after)?;
    if !(before <= now && now < after) {
        return Err(Error::Time(
            "delegated signer is outside its half-open validity interval".into(),
        ));
    }
    Ok(key.key_id.clone())
}

/// Validate a manifest's closed provider shape and every archive in `directory`.
pub fn stage_manifest(directory: &Path, manifest: &Manifest) -> Result<Vec<u8>> {
    policy::validate_manifest(manifest)?;
    if manifest.protocols != Protocols::v1() {
        return Err(Error::Schema(
            "staged binary does not advertise the provider v1 protocol set".into(),
        ));
    }
    for asset in &manifest.assets {
        archive::verify_asset(directory, asset)?;
    }
    canonical::encode(manifest)
}

/// Verify the signed manifest identity against the selected signed channel entry.
pub fn verify_manifest_identity(manifest: &Manifest, selected: &ReleaseEntry) -> Result<()> {
    if manifest.tag != selected.tag
        || manifest.version != selected.version
        || manifest.source_commit != selected.source_commit
        || manifest.build_id != selected.build_id
        || manifest.protocols != selected.protocols
        || manifest.signing_key_ids != selected.release_key_ids
    {
        return Err(Error::Schema(
            "manifest identity disagrees with selected channel entry".into(),
        ));
    }
    Ok(())
}

/// Produce one deterministic, closed three-member platform archive and its manifest asset record.
pub fn package_archive(
    version: &str,
    target: &str,
    executable: &Path,
    licenses: &[PathBuf],
    documentation: Option<&Path>,
    output_directory: &Path,
) -> Result<Asset> {
    archive::package(
        version,
        target,
        executable,
        licenses,
        documentation,
        output_directory,
    )
}

/// Return the independently verified executable bytes from a verified target archive.
pub fn verified_executable_bytes(directory: &Path, asset: &Asset) -> Result<Vec<u8>> {
    archive::executable_bytes(directory, asset)
}

/// Apply the global rollback/equivocation rule to authenticated metadata.
pub fn advance_floor(
    existing: Option<&Floor>,
    number: u64,
    sha256: &str,
    kind: &str,
) -> Result<Floor> {
    policy::validate_sha256(sha256)?;
    policy::check_floor(existing, number, sha256, kind)
}

/// Parse the contract's exact UTC-seconds syntax.
pub fn parse_utc(value: &str) -> Result<OffsetDateTime> {
    policy::parse_time(value)
}

/// Verify canonical compatibility output against the selected signed channel identity.
pub fn verify_compatibility(bytes: &[u8], selected: &ReleaseEntry) -> Result<Compatibility> {
    let compatibility: Compatibility = canonical::parse(bytes, 16 * 1024)?;
    if compatibility.schema != "exchange.compatibility.v1"
        || compatibility.release.tag != selected.tag
        || compatibility.release.version != selected.version
        || compatibility.release.source_commit != selected.source_commit
        || compatibility.release.build_id != selected.build_id
        || compatibility.protocols != selected.protocols
    {
        return Err(Error::Schema(
            "compatibility output disagrees with selected signed release".into(),
        ));
    }
    policy::validate_protocols(&compatibility.protocols)?;
    Ok(compatibility)
}

/// Verify the closed one-shot readiness shape for a selected native target.
pub fn verify_readiness(
    bytes: &[u8],
    target: &str,
    selected: &ReleaseEntry,
    executable_sha256: &str,
) -> Result<()> {
    let value = canonical::parse_value(bytes, 16 * 1024)?;
    let object = exact_object(
        &value,
        &["bind", "process", "protocols", "release", "schema"],
    )?;
    if object.get("schema").and_then(serde_json::Value::as_str)
        != Some("exchange.supervisor-ready.v1")
    {
        return Err(Error::Schema("readiness schema is not v1".into()));
    }
    let protocols: Protocols = serde_json::from_value(object["protocols"].clone())
        .map_err(|error| Error::Schema(error.to_string()))?;
    if protocols != selected.protocols || protocols.supervisor != "exchange.supervisor-ready.v1" {
        return Err(Error::Schema("readiness protocols disagree".into()));
    }
    let release = exact_object(
        &object["release"],
        &[
            "build_id",
            "executable_sha256",
            "source_commit",
            "tag",
            "version",
        ],
    )?;
    if release["tag"] != selected.tag
        || release["version"] != selected.version
        || release["source_commit"] != selected.source_commit
        || release["build_id"] != selected.build_id
        || release["executable_sha256"] != executable_sha256
    {
        return Err(Error::Schema("readiness release identity disagrees".into()));
    }
    let bind = exact_object(&object["bind"], &["host", "port", "scheme"])?;
    if bind["scheme"] != "http"
        || !matches!(bind["host"].as_str(), Some("127.0.0.1" | "::1"))
        || !matches!(bind["port"].as_u64(), Some(1..=65535))
    {
        return Err(Error::Schema(
            "readiness bind is outside the loopback domain".into(),
        ));
    }
    let process = exact_object(&object["process"], &["pid", "start_identity"])?;
    if !matches!(process["pid"].as_u64(), Some(1..=4_294_967_295)) {
        return Err(Error::Schema(
            "readiness pid is outside the native domain".into(),
        ));
    }
    let identity = process
        .get("start_identity")
        .ok_or_else(|| Error::Schema("start_identity absent".into()))?;
    match Platform::from_target(target)? {
        Platform::Unix if target.ends_with("linux-gnu") => {
            let identity = exact_object(identity, &["boot_id", "kind", "ticks"])?;
            if identity["kind"] != "linux-proc-start"
                || !identity["boot_id"].as_str().is_some_and(lower_uuid)
            {
                return Err(Error::Schema("Linux start identity is invalid".into()));
            }
            decimal(identity["ticks"].as_str(), u64::MAX)?;
        }
        Platform::Unix => {
            let identity = exact_object(identity, &["kind", "microseconds", "seconds"])?;
            if identity["kind"] != "macos-proc-start"
                || !matches!(identity["microseconds"].as_u64(), Some(0..=999_999))
            {
                return Err(Error::Schema("macOS start identity is invalid".into()));
            }
            decimal(identity["seconds"].as_str(), i64::MAX as u64)?;
        }
        Platform::Windows => {
            let identity = exact_object(identity, &["filetime", "kind"])?;
            if identity["kind"] != "windows-process-creation" {
                return Err(Error::Schema("Windows start identity is invalid".into()));
            }
            decimal(identity["filetime"].as_str(), u64::MAX)?;
        }
    }
    Ok(())
}

fn exact_object<'a>(
    value: &'a serde_json::Value,
    keys: &[&str],
) -> Result<&'a serde_json::Map<String, serde_json::Value>> {
    let object = value
        .as_object()
        .ok_or_else(|| Error::Schema("expected object".into()))?;
    if object.len() != keys.len() || !keys.iter().all(|key| object.contains_key(*key)) {
        return Err(Error::Schema(
            "object has unknown or missing members".into(),
        ));
    }
    Ok(object)
}
fn decimal(value: Option<&str>, maximum: u64) -> Result<u64> {
    let value = value.ok_or_else(|| Error::Schema("decimal string absent".into()))?;
    if value.is_empty()
        || value.len() > 20
        || (value.len() > 1 && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(Error::Schema("decimal string is noncanonical".into()));
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| Error::Bounds("decimal string overflow".into()))?;
    if parsed == 0 || parsed > maximum {
        return Err(Error::Bounds("decimal string outside field domain".into()));
    }
    Ok(parsed)
}
fn lower_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

/// Select the greatest stable SemVer matching all six independently versioned protocols.
pub fn select_compatible<'a>(
    releases: &'a [ReleaseEntry],
    supported: &Protocols,
) -> Result<&'a ReleaseEntry> {
    let mut best: Option<(&ReleaseEntry, Version)> = None;
    for release in releases {
        if &release.protocols != supported {
            continue;
        }
        let version = policy::parse_stable_version(&release.version)?;
        if best.as_ref().is_none_or(|(_, prior)| version > *prior) {
            best = Some((release, version));
        }
    }
    best.map(|(release, _)| release).ok_or_else(|| {
        Error::Selection("channel has no entry matching all six protocol ids".into())
    })
}

/// Verify an archive directly after download against a closed member list.
pub fn verify_archive(
    path: &Path,
    format: &str,
    platform: Platform,
    expected: &[(String, u64, String)],
) -> Result<()> {
    let expected: BTreeMap<_, _> = expected
        .iter()
        .map(|(path, bytes, digest)| (path.clone(), (*bytes, digest.clone())))
        .collect();
    archive::verify_archive(path, format, platform, &expected)
}

pub fn digest_hex(bytes: &[u8]) -> String {
    lower_hex(&Sha256::digest(bytes))
}

fn lower_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}
