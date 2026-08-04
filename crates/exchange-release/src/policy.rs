use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};
use semver::Version;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use time::{Duration, OffsetDateTime};

use crate::archive::ensure_one_root;
use crate::model::*;
use crate::{
    digest_hex, read_bounded_file, Error, Result, JCS_SAFE_INTEGER, ORIGIN, SUPPORTED_TARGETS,
};

pub(crate) struct VerifiedTrust {
    pub document: TrustDocument,
    pub bytes: Vec<u8>,
}

pub(crate) fn verify_trust(
    directory: &Path,
    policy: &RootPolicy,
    now: OffsetDateTime,
) -> Result<VerifiedTrust> {
    validate_root_policy(policy)?;
    let path = directory.join("flux-exchange-release-trust.json");
    let bytes = read_bounded_file(&path, 64 * 1024)?;
    let document: TrustDocument = crate::canonical::parse(&bytes, 64 * 1024)?;
    if document.schema != "exchange.release-trust.v1" || document.origin != ORIGIN {
        return Err(Error::Schema(
            "trust schema or origin is not the provider v1 identity".into(),
        ));
    }
    if document.version == 0 || document.version > JCS_SAFE_INTEGER {
        return Err(Error::Bounds(
            "trust version is outside 1..=JCS-safe maximum".into(),
        ));
    }
    let issued = parse_time(&document.issued_at)?;
    let expires = parse_time(&document.expires_at)?;
    validate_document_time(issued, expires, now, Duration::days(366))?;
    sorted_unique(&document.root_signing_key_ids, 1, 4, validate_key_id)?;
    let policy_by_id: BTreeMap<_, _> = policy
        .keys
        .iter()
        .map(|key| (key.key_id.as_str(), key))
        .collect();
    if !document
        .root_signing_key_ids
        .iter()
        .all(|id| policy_by_id.contains_key(id.as_str()))
    {
        return Err(Error::Signature(
            "trust names a root outside the pinned policy".into(),
        ));
    }
    let mut root_material = BTreeSet::new();
    for key in &policy.keys {
        root_material.insert(validate_public_key(&key.minisign_public_key)?);
    }
    if root_material.len() != policy.keys.len() {
        return Err(Error::Signature(
            "offline root Ed25519 material is reused".into(),
        ));
    }
    let root_valid = verify_signatures(
        directory,
        "flux-exchange-release-trust.json",
        &bytes,
        &document.root_signing_key_ids,
        policy.threshold,
        |id| {
            policy_by_id
                .get(id)
                .map(|key| key.minisign_public_key.as_str())
        },
    )?;
    if root_valid < policy.threshold {
        return Err(Error::Signature("offline root threshold is not met".into()));
    }
    let mut ids = BTreeSet::new();
    let mut delegated_material = BTreeSet::new();
    validate_role(
        "channel",
        &document.roles.channel,
        issued,
        expires,
        now,
        &mut ids,
        &mut delegated_material,
    )?;
    validate_role(
        "release",
        &document.roles.release,
        issued,
        expires,
        now,
        &mut ids,
        &mut delegated_material,
    )?;
    if delegated_material
        .iter()
        .any(|material| root_material.contains(material))
    {
        return Err(Error::Signature(
            "online and offline root Ed25519 material is reused".into(),
        ));
    }
    Ok(VerifiedTrust { document, bytes })
}

pub(crate) fn verify_channel(
    directory: &Path,
    trust: &VerifiedTrust,
    now: OffsetDateTime,
) -> Result<(Channel, Vec<u8>)> {
    let path = directory.join("flux-exchange-release-channel.json");
    let bytes = read_bounded_file(&path, 256 * 1024)?;
    let channel: Channel = crate::canonical::parse(&bytes, 256 * 1024)?;
    if channel.schema != "exchange.release-channel.v1"
        || channel.channel != "stable"
        || channel.origin != ORIGIN
    {
        return Err(Error::Schema(
            "channel schema, name or origin is not provider stable v1".into(),
        ));
    }
    if channel.generation == 0 || channel.generation > JCS_SAFE_INTEGER {
        return Err(Error::Bounds(
            "channel generation is outside 1..=JCS-safe maximum".into(),
        ));
    }
    validate_document_time(
        parse_time(&channel.issued_at)?,
        parse_time(&channel.expires_at)?,
        now,
        Duration::days(7),
    )?;
    sorted_unique(&channel.signing_key_ids, 1, 4, validate_key_id)?;
    let keys = valid_role_keys(&trust.document.roles.channel, now)?;
    let valid = verify_signatures(
        directory,
        "flux-exchange-release-channel.json",
        &bytes,
        &channel.signing_key_ids,
        trust.document.roles.channel.threshold,
        |id| keys.get(id).map(String::as_str),
    )?;
    if valid < trust.document.roles.channel.threshold {
        return Err(Error::Signature("channel role threshold is not met".into()));
    }
    if channel.releases.is_empty() || channel.releases.len() > 128 {
        return Err(Error::Bounds(
            "channel must contain 1..=128 releases".into(),
        ));
    }
    let release_keys = valid_role_keys(&trust.document.roles.release, now)?;
    let mut versions = Vec::new();
    let mut tags = BTreeSet::new();
    let mut manifests = BTreeSet::new();
    let mut identities = BTreeSet::new();
    for release in &channel.releases {
        validate_release_entry(
            release,
            &release_keys,
            trust.document.roles.release.threshold,
        )?;
        let version = parse_stable_version(&release.version)?;
        if versions
            .last()
            .is_some_and(|prior: &Version| prior >= &version)
        {
            return Err(Error::Selection(
                "channel releases are not strictly ascending SemVer".into(),
            ));
        }
        versions.push(version);
        if !tags.insert(&release.tag)
            || !manifests.insert(&release.manifest_sha256)
            || !identities.insert((&release.version, &release.source_commit, &release.build_id))
        {
            return Err(Error::Schema(
                "channel has duplicate release identity".into(),
            ));
        }
    }
    Ok((channel, bytes))
}

pub(crate) fn verify_manifest(
    directory: &Path,
    trust: &VerifiedTrust,
    selected: &ReleaseEntry,
    now: OffsetDateTime,
    target: Option<&str>,
) -> Result<(Manifest, Vec<u8>)> {
    let path = directory.join("flux-exchange-release-manifest.json");
    let bytes = read_bounded_file(&path, 256 * 1024)?;
    if digest_hex(&bytes) != selected.manifest_sha256 {
        return Err(Error::Digest(
            "manifest digest disagrees with selected channel entry".into(),
        ));
    }
    let manifest: Manifest = crate::canonical::parse(&bytes, 256 * 1024)?;
    if manifest.schema != "exchange.release-manifest.v1" || manifest.origin != ORIGIN {
        return Err(Error::Schema(
            "manifest schema or origin is not provider v1".into(),
        ));
    }
    crate::verify_manifest_identity(&manifest, selected)?;
    let keys = valid_role_keys(&trust.document.roles.release, now)?;
    verify_signatures(
        directory,
        "flux-exchange-release-manifest.json",
        &bytes,
        &manifest.signing_key_ids,
        trust.document.roles.release.threshold,
        |id| keys.get(id).map(String::as_str),
    )?;
    validate_manifest(&manifest)?;
    for asset in &manifest.assets {
        if target.is_none_or(|wanted| wanted == asset.target) {
            crate::archive::verify_asset(directory, asset)?;
        }
    }
    if target.is_some()
        && !manifest
            .assets
            .iter()
            .any(|asset| Some(asset.target.as_str()) == target)
    {
        return Err(Error::Schema(
            "selected target has no manifest asset".into(),
        ));
    }
    Ok((manifest, bytes))
}

pub(crate) fn validate_manifest(manifest: &Manifest) -> Result<()> {
    if manifest.schema != "exchange.release-manifest.v1" || manifest.origin != ORIGIN {
        return Err(Error::Schema(
            "manifest schema or origin is not provider v1".into(),
        ));
    }
    validate_release_identity(
        &manifest.tag,
        &manifest.version,
        &manifest.source_commit,
        &manifest.build_id,
    )?;
    validate_protocols(&manifest.protocols)?;
    sorted_unique(&manifest.signing_key_ids, 1, 4, validate_key_id)?;
    if manifest.assets.len() != SUPPORTED_TARGETS.len() {
        return Err(Error::Bounds(
            "manifest must contain exactly the five supported targets".into(),
        ));
    }
    let actual: Vec<_> = manifest
        .assets
        .iter()
        .map(|asset| asset.target.as_str())
        .collect();
    if actual != SUPPORTED_TARGETS {
        return Err(Error::Schema(
            "manifest targets are not the exact sorted supported set".into(),
        ));
    }
    let mut basenames = BTreeSet::new();
    for asset in &manifest.assets {
        let root = format!("flux-exchange-{}-{}", manifest.version, asset.target);
        let (expected_archive, expected_format, expected_executable) =
            if asset.target == "x86_64-pc-windows-msvc" {
                (
                    format!("{root}.zip"),
                    "zip",
                    format!("{root}/flux-exchange.exe"),
                )
            } else {
                (
                    format!("{root}.tar.zst"),
                    "tar.zst",
                    format!("{root}/flux-exchange"),
                )
            };
        if asset.archive != expected_archive || asset.format != expected_format {
            return Err(Error::Schema(format!(
                "archive name/format for {} is not exact",
                asset.target
            )));
        }
        validate_basename(&asset.archive)?;
        if !basenames.insert(asset.archive.to_ascii_lowercase()) {
            return Err(Error::Schema(
                "archive basenames collide under ASCII case folding".into(),
            ));
        }
        if !(1..=crate::MAX_ARCHIVE_BYTES).contains(&asset.archive_bytes) {
            return Err(Error::Bounds(
                "archive byte declaration is outside 1..=256 MiB".into(),
            ));
        }
        validate_sha256(&asset.archive_sha256)?;
        if asset.other_members.len() > 15 {
            return Err(Error::Bounds("more than 15 non-executable members".into()));
        }
        let paths = std::iter::once(asset.executable.path.clone())
            .chain(asset.other_members.iter().map(|member| member.path.clone()));
        ensure_one_root(paths)?;
        if asset.executable.path != expected_executable {
            return Err(Error::Archive(format!(
                "executable path for {} has the wrong basename",
                asset.target
            )));
        }
        if asset
            .other_members
            .iter()
            .any(|member| !member.path.starts_with(&format!("{root}/")))
        {
            return Err(Error::Archive(format!(
                "member root for {} disagrees with release identity",
                asset.target
            )));
        }
        validate_member(asset.executable.bytes, &asset.executable.sha256)?;
        let mut expanded = asset.executable.bytes;
        let mut prior = None;
        let mut support = BTreeSet::new();
        for member in &asset.other_members {
            if prior.is_some_and(|path: &str| path >= member.path.as_str()) {
                return Err(Error::Schema(
                    "other_members are not strictly sorted by path".into(),
                ));
            }
            prior = Some(&member.path);
            let basename = member
                .path
                .rsplit('/')
                .next()
                .ok_or_else(|| Error::Archive("supporting member has no basename".into()))?;
            let expected_kind = match basename {
                "LICENSE-APACHE" | "LICENSE-MIT" => MemberKind::License,
                "README.md" => MemberKind::Documentation,
                _ => {
                    return Err(Error::Archive(format!(
                        "supporting member basename {basename:?} is outside the packager contract"
                    )))
                }
            };
            if member.kind != expected_kind || !support.insert(basename) {
                return Err(Error::Archive(format!(
                    "supporting member {basename:?} has the wrong kind or is duplicated"
                )));
            }
            validate_member(member.bytes, &member.sha256)?;
            expanded = expanded
                .checked_add(member.bytes)
                .ok_or_else(|| Error::Bounds("declared expanded size overflow".into()))?;
            if expanded > crate::MAX_EXPANDED_BYTES {
                return Err(Error::Bounds(
                    "archive declares more than 512 MiB expanded bytes".into(),
                ));
            }
        }
        if !support.contains("LICENSE-APACHE") || !support.contains("LICENSE-MIT") {
            return Err(Error::Archive(
                "archive must contain both required license basenames".into(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn check_floor(
    existing: Option<&Floor>,
    number: u64,
    sha256: &str,
    kind: &str,
) -> Result<Floor> {
    if let Some(existing) = existing {
        if number < existing.number {
            return Err(Error::Rollback(format!(
                "{kind} rollback: {number} < {}",
                existing.number
            )));
        }
        if number == existing.number && sha256 != existing.sha256 {
            return Err(Error::Equivocation(format!(
                "{kind} {number} has different canonical bytes"
            )));
        }
    }
    Ok(Floor {
        number,
        sha256: sha256.into(),
    })
}

fn validate_root_policy(policy: &RootPolicy) -> Result<()> {
    if policy.schema != "exchange.release-root-policy.v1"
        || policy.keys.is_empty()
        || policy.keys.len() > 4
    {
        return Err(Error::Schema(
            "root policy schema or key count is invalid".into(),
        ));
    }
    if policy.threshold == 0 || policy.threshold as usize > policy.keys.len() {
        return Err(Error::Signature("root policy threshold is invalid".into()));
    }
    let ids: Vec<_> = policy.keys.iter().map(|key| key.key_id.clone()).collect();
    sorted_unique(&ids, 1, 4, validate_key_id)
}

fn validate_role(
    name: &str,
    role: &Role,
    trust_issued: OffsetDateTime,
    trust_expires: OffsetDateTime,
    _now: OffsetDateTime,
    ids: &mut BTreeSet<String>,
    materials: &mut BTreeSet<[u8; 32]>,
) -> Result<()> {
    if role.keys.is_empty()
        || role.keys.len() > 4
        || role.threshold == 0
        || role.threshold as usize > role.keys.len()
    {
        return Err(Error::Signature(format!(
            "{name} role key count or threshold is invalid"
        )));
    }
    let role_ids: Vec<_> = role.keys.iter().map(|key| key.key_id.clone()).collect();
    sorted_unique(&role_ids, 1, 4, validate_key_id)?;
    for key in &role.keys {
        if !ids.insert(key.key_id.clone()) {
            return Err(Error::Signature(format!(
                "delegated key id {} crosses roles",
                key.key_id
            )));
        }
        let before = parse_time(&key.not_before)?;
        let after = parse_time(&key.not_after)?;
        if before < trust_issued || after > trust_expires || before >= after {
            return Err(Error::Time(format!(
                "{name} delegation interval lies outside trust"
            )));
        }
        if !materials.insert(validate_public_key(&key.minisign_public_key)?) {
            return Err(Error::Signature(
                "delegated Ed25519 material is reused".into(),
            ));
        }
    }
    Ok(())
}

fn valid_role_keys(role: &Role, now: OffsetDateTime) -> Result<BTreeMap<String, String>> {
    role.keys
        .iter()
        .filter_map(|key| {
            let before = parse_time(&key.not_before).ok()?;
            let after = parse_time(&key.not_after).ok()?;
            (before <= now && now < after)
                .then(|| (key.key_id.clone(), key.minisign_public_key.clone()))
        })
        .collect::<BTreeMap<_, _>>()
        .pipe(Ok)
}

trait Pipe: Sized {
    fn pipe<T>(self, function: impl FnOnce(Self) -> T) -> T {
        function(self)
    }
}
impl<T> Pipe for T {}

fn verify_signatures<'a>(
    directory: &Path,
    basename: &str,
    payload: &[u8],
    ids: &[String],
    threshold: u64,
    key: impl Fn(&str) -> Option<&'a str>,
) -> Result<u64> {
    let mut valid = 0;
    for id in ids {
        validate_key_id(id)?;
        let public = key(id).ok_or_else(|| {
            Error::Signature(format!("signer {id} is not valid for this role/time"))
        })?;
        let path = directory.join(format!("{basename}.{id}.minisig"));
        let bytes = read_bounded_file(&path, 4096).map_err(|error| {
            Error::Signature(format!(
                "required signature {basename}.{id}.minisig is absent or unreadable: {error}"
            ))
        })?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|_| Error::Signature("signature is not UTF-8".into()))?;
        let public =
            PublicKey::from_base64(public).map_err(|error| Error::Signature(error.to_string()))?;
        let signature =
            Signature::decode(text).map_err(|error| Error::Signature(error.to_string()))?;
        public
            .verify(payload, &signature, false)
            .map_err(|error| Error::Signature(error.to_string()))?;
        valid += 1;
    }
    if valid < threshold {
        return Err(Error::Signature(format!(
            "only {valid} valid signatures; threshold is {threshold}"
        )));
    }
    Ok(valid)
}

fn validate_release_entry(
    entry: &ReleaseEntry,
    keys: &BTreeMap<String, String>,
    threshold: u64,
) -> Result<()> {
    validate_release_identity(
        &entry.tag,
        &entry.version,
        &entry.source_commit,
        &entry.build_id,
    )?;
    validate_sha256(&entry.manifest_sha256)?;
    validate_protocols(&entry.protocols)?;
    sorted_unique(&entry.release_key_ids, 1, 4, validate_key_id)?;
    if entry.release_key_ids.len() < threshold as usize
        || !entry.release_key_ids.iter().all(|id| keys.contains_key(id))
    {
        return Err(Error::Signature(
            "release entry does not name a valid release-role threshold".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_protocols(protocols: &Protocols) -> Result<()> {
    for id in [
        &protocols.exchange_api,
        &protocols.effective_catalogue_response,
        &protocols.invoke_request,
        &protocols.invoke_response,
        &protocols.connection_plan,
        &protocols.supervisor,
    ] {
        validate_protocol_id(id)?;
    }
    Ok(())
}

fn validate_release_identity(tag: &str, version: &str, source: &str, build: &str) -> Result<()> {
    parse_stable_version(version)?;
    if tag != format!("refs/tags/v{version}") {
        return Err(Error::Schema(
            "tag is not exactly refs/tags/v<version>".into(),
        ));
    }
    if source.len() != 40
        || !source
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Schema(
            "source_commit is not 40 lowercase hexadecimal characters".into(),
        ));
    }
    if build.is_empty()
        || build.len() > 128
        || !build.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
    {
        return Err(Error::Schema(
            "build_id is not 1..=128 printable ASCII bytes".into(),
        ));
    }
    Ok(())
}

fn validate_member(bytes: u64, sha256: &str) -> Result<()> {
    if !(1..=crate::MAX_MEMBER_BYTES).contains(&bytes) {
        return Err(Error::Bounds("member bytes are outside 1..=256 MiB".into()));
    }
    validate_sha256(sha256)
}

pub(crate) fn validate_sha256(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::Schema(
            "SHA-256 is not 64 lowercase hexadecimal characters".into(),
        ));
    }
    Ok(())
}

pub(crate) fn validate_key_id(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || value.contains("--")
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        || !value.as_bytes()[0].is_ascii_lowercase() && !value.as_bytes()[0].is_ascii_digit()
        || !value.as_bytes()[value.len() - 1].is_ascii_lowercase()
            && !value.as_bytes()[value.len() - 1].is_ascii_digit()
    {
        return Err(Error::Schema(format!("unsafe key id {value:?}")));
    }
    Ok(())
}

pub(crate) fn validate_basename(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 128
        || !value.is_ascii()
        || value.contains("..")
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value.as_bytes()[value.len() - 1].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::Schema(format!("unsafe derived basename {value:?}")));
    }
    Ok(())
}

fn validate_protocol_id(value: &str) -> Result<()> {
    if value.is_empty() || value.len() > 128 || !value.is_ascii() {
        return Err(Error::Schema(format!("unsafe protocol id {value:?}")));
    }
    let tokens: Vec<_> = value.split('.').collect();
    if tokens.len() < 2 {
        return Err(Error::Schema(format!(
            "protocol id {value:?} has no version token"
        )));
    }
    let (last, names) = tokens
        .split_last()
        .ok_or_else(|| Error::Schema("empty protocol id".into()))?;
    if !last.starts_with('v')
        || last.len() < 2
        || last.len() > 10
        || last.as_bytes()[1] == b'0'
        || !last.as_bytes()[1..].iter().all(u8::is_ascii_digit)
    {
        return Err(Error::Schema(format!(
            "protocol id {value:?} has an invalid version token"
        )));
    }
    if names.iter().any(|token| {
        token.is_empty()
            || token.contains("--")
            || !token.as_bytes()[0].is_ascii_lowercase()
            || !token.as_bytes()[token.len() - 1].is_ascii_alphanumeric()
            || !token
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    }) {
        return Err(Error::Schema(format!("unsafe protocol id {value:?}")));
    }
    Ok(())
}

pub(crate) fn parse_stable_version(value: &str) -> Result<Version> {
    let components: Vec<_> = value.split('.').collect();
    if components.len() != 3
        || components.iter().any(|part| {
            part.is_empty()
                || part.len() > 9
                || (part.len() > 1 && part.starts_with('0'))
                || !part.bytes().all(|byte| byte.is_ascii_digit())
        })
    {
        return Err(Error::Schema(format!(
            "version {value:?} is not stable canonical SemVer"
        )));
    }
    Version::parse(value).map_err(|error| Error::Schema(error.to_string()))
}

pub(crate) fn parse_time(value: &str) -> Result<OffsetDateTime> {
    if value.len() != 20
        || !value.ends_with('Z')
        || value.as_bytes().get(4) != Some(&b'-')
        || value.as_bytes().get(7) != Some(&b'-')
        || value.as_bytes().get(10) != Some(&b'T')
    {
        return Err(Error::Time(format!("{value:?} is not UTC RFC3339 seconds")));
    }
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339)
        .map_err(|error| Error::Time(error.to_string()))
}

fn validate_document_time(
    issued: OffsetDateTime,
    expires: OffsetDateTime,
    now: OffsetDateTime,
    maximum: Duration,
) -> Result<()> {
    if issued >= expires || expires - issued > maximum {
        return Err(Error::Time(
            "metadata validity interval is empty or too long".into(),
        ));
    }
    let future = now
        .checked_add(Duration::minutes(5))
        .ok_or_else(|| Error::Time("clock skew addition overflowed".into()))?;
    if issued > future || now >= expires {
        return Err(Error::Time("metadata is future-issued or expired".into()));
    }
    Ok(())
}

fn sorted_unique(
    values: &[String],
    minimum: usize,
    maximum: usize,
    validate: fn(&str) -> Result<()>,
) -> Result<()> {
    if values.len() < minimum || values.len() > maximum {
        return Err(Error::Bounds(format!(
            "list must contain {minimum}..={maximum} entries"
        )));
    }
    let mut previous = None;
    for value in values {
        validate(value)?;
        if previous.is_some_and(|prior: &str| prior >= value.as_str()) {
            return Err(Error::Schema(
                "list is not strictly lexically sorted and unique".into(),
            ));
        }
        previous = Some(value);
    }
    Ok(())
}

fn validate_public_key(encoded: &str) -> Result<[u8; 32]> {
    if encoded.len() != 56 || encoded.contains('=') {
        return Err(Error::Signature(
            "minisign public key must be 56 unpadded base64 characters".into(),
        ));
    }
    let decoded = base64::engine::general_purpose::STANDARD_NO_PAD
        .decode(encoded)
        .map_err(|error| Error::Signature(error.to_string()))?;
    if decoded.len() != 42
        || &decoded[..2] != b"Ed"
        || base64::engine::general_purpose::STANDARD_NO_PAD.encode(&decoded) != encoded
    {
        return Err(Error::Signature(
            "minisign public key packet is malformed or noncanonical".into(),
        ));
    }
    PublicKey::from_base64(encoded).map_err(|error| Error::Signature(error.to_string()))?;
    let mut material = [0u8; 32];
    material.copy_from_slice(&decoded[10..]);
    Ok(material)
}
