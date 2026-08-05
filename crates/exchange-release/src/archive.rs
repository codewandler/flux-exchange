//! Bounded archive verification without extraction.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::io::{BufReader, Cursor, Read, Seek, Write};
use std::path::{Path, PathBuf};

use crate::model::{Asset, MemberKind};
use crate::{
    digest_hex, read_bounded_file, Error, Platform, Result, MAX_ARCHIVE_BYTES, MAX_EXPANDED_BYTES,
    MAX_MEMBER_BYTES,
};

#[derive(Debug)]
struct SeenMember {
    bytes: u64,
    sha256: String,
}

/// Verify the selected archive against its complete signed member inventory.
pub fn verify_asset(directory: &Path, asset: &Asset) -> Result<()> {
    let archive_path = directory.join(&asset.archive);
    let bytes = read_bounded_file(&archive_path, MAX_ARCHIVE_BYTES)?;
    verify_asset_bytes(&bytes, asset)
}

fn verify_asset_bytes(bytes: &[u8], asset: &Asset) -> Result<()> {
    if bytes.len() as u64 != asset.archive_bytes {
        return Err(Error::Archive(format!(
            "archive size disagrees for {}",
            asset.archive
        )));
    }
    if digest_hex(bytes) != asset.archive_sha256 {
        return Err(Error::Digest(format!(
            "archive digest disagrees for {}",
            asset.archive
        )));
    }
    let expected = expected_members(asset)?;
    let platform = Platform::from_target(&asset.target)?;
    let seen = verify_archive_members(bytes, &asset.format, platform, &expected)?;
    if seen.len() != expected.len() {
        return Err(Error::Archive("archive member set is not exact".into()));
    }
    for (path, member) in seen {
        let expected_member = expected
            .get(&path)
            .ok_or_else(|| Error::Archive(format!("undeclared archive member {path}")))?;
        if (member.bytes, member.sha256.as_str()) != (expected_member.0, expected_member.1.as_str())
        {
            return Err(Error::Archive(format!(
                "member bytes or digest disagree for {path}"
            )));
        }
    }
    Ok(())
}

pub(crate) fn executable_bytes(directory: &Path, asset: &Asset) -> Result<Vec<u8>> {
    let path = directory.join(&asset.archive);
    let archive_bytes = read_bounded_file(&path, MAX_ARCHIVE_BYTES)?;
    verify_asset_bytes(&archive_bytes, asset)?;
    if asset.format != "tar.zst" {
        return Err(Error::Archive("unknown archive format".into()));
    }
    let decoder = zstd::stream::read::Decoder::new(Cursor::new(archive_bytes.as_slice()))
        .map_err(|error| Error::Archive(error.to_string()))?;
    let mut archive = tar::Archive::new(decoder);
    for entry in archive
        .entries()
        .map_err(|error| Error::Archive(error.to_string()))?
    {
        let mut entry = entry.map_err(|error| Error::Archive(error.to_string()))?;
        if entry
            .path()
            .map_err(|error| Error::Archive(error.to_string()))?
            == Path::new(&asset.executable.path)
        {
            return finish_executable(
                read_exact_member_bytes(&mut entry, asset.executable.bytes)?,
                asset,
            );
        }
    }
    Err(Error::Archive(
        "verified executable member disappeared".into(),
    ))
}

fn read_exact_member_bytes(reader: &mut impl Read, declared: u64) -> Result<Vec<u8>> {
    let capacity = usize::try_from(declared)
        .map_err(|_| Error::Bounds("member size does not fit this platform".into()))?;
    let mut bytes = Vec::with_capacity(capacity);
    reader
        .take(declared.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(|error| Error::Archive(error.to_string()))?;
    if bytes.len() as u64 != declared {
        return Err(Error::Archive(
            "returned executable bytes disagree with their declared size".into(),
        ));
    }
    Ok(bytes)
}

fn finish_executable(bytes: Vec<u8>, asset: &Asset) -> Result<Vec<u8>> {
    if digest_hex(&bytes) != asset.executable.sha256 {
        return Err(Error::Digest(
            "returned executable digest disagrees with signed member digest".into(),
        ));
    }
    Ok(bytes)
}

fn expected_members(asset: &Asset) -> Result<BTreeMap<String, (u64, String)>> {
    let mut expected = BTreeMap::new();
    expected.insert(
        asset.executable.path.clone(),
        (asset.executable.bytes, asset.executable.sha256.clone()),
    );
    for member in &asset.other_members {
        if expected
            .insert(member.path.clone(), (member.bytes, member.sha256.clone()))
            .is_some()
        {
            return Err(Error::Archive(format!(
                "duplicate declared path {}",
                member.path
            )));
        }
        match member.kind {
            MemberKind::Documentation | MemberKind::License => {}
        }
    }
    Ok(expected)
}

pub(crate) fn verify_archive(
    path: &Path,
    format: &str,
    platform: Platform,
    expected: &BTreeMap<String, (u64, String)>,
) -> Result<()> {
    let bytes = read_bounded_file(path, MAX_ARCHIVE_BYTES)?;
    verify_archive_members(&bytes, format, platform, expected).map(|_| ())
}

fn verify_archive_members(
    bytes: &[u8],
    format: &str,
    platform: Platform,
    expected: &BTreeMap<String, (u64, String)>,
) -> Result<BTreeMap<String, SeenMember>> {
    match (format, platform) {
        ("tar.zst", Platform::Linux) => verify_tar_zst(bytes, expected),
        _ => Err(Error::Archive(
            "archive format does not match target platform".into(),
        )),
    }
}

fn verify_tar_zst(
    bytes: &[u8],
    expected: &BTreeMap<String, (u64, String)>,
) -> Result<BTreeMap<String, SeenMember>> {
    let file_len = bytes.len() as u64;
    let mut decoder = zstd::stream::read::Decoder::with_buffer(BufReader::new(Cursor::new(bytes)))
        .map_err(|error| Error::Archive(error.to_string()))?
        .single_frame();
    let mut expanded_bytes = Vec::new();
    decoder
        .by_ref()
        .take(MAX_EXPANDED_BYTES + 16 * 1024 + 1)
        .read_to_end(&mut expanded_bytes)
        .map_err(|error| Error::Archive(error.to_string()))?;
    if expanded_bytes.len() as u64 > MAX_EXPANDED_BYTES + 16 * 1024 {
        return Err(Error::Bounds(
            "tar stream exceeds bounded expanded envelope".into(),
        ));
    }
    let mut buffered = decoder.finish();
    let physical = buffered
        .stream_position()
        .map_err(|error| Error::Archive(error.to_string()))?;
    let logical = physical
        .checked_sub(buffered.buffer().len() as u64)
        .ok_or_else(|| Error::Archive("zstd stream position underflow".into()))?;
    if logical != file_len {
        return Err(Error::Archive(
            "tar.zst has trailing compressed data or another frame".into(),
        ));
    }
    let cursor = Cursor::new(expanded_bytes.as_slice());
    let mut archive = tar::Archive::new(cursor);
    let mut seen = BTreeMap::new();
    let mut expanded = 0u64;
    for entry in archive
        .entries()
        .map_err(|error| Error::Archive(error.to_string()))?
    {
        let mut member = entry.map_err(|error| Error::Archive(error.to_string()))?;
        if !member.header().entry_type().is_file() {
            return Err(Error::Archive(
                "tar contains a link, directory or special member".into(),
            ));
        }
        let name = member
            .path()
            .map_err(|error| Error::Archive(error.to_string()))?;
        let name = name
            .to_str()
            .ok_or_else(|| Error::Archive("member path is not UTF-8".into()))?
            .to_owned();
        validate_member_path(&name)?;
        let mode = member
            .header()
            .mode()
            .map_err(|error| Error::Archive(error.to_string()))?;
        validate_member_mode(&name, Some(mode))?;
        let size = member
            .header()
            .size()
            .map_err(|error| Error::Archive(error.to_string()))?;
        if size > MAX_MEMBER_BYTES {
            return Err(Error::Bounds(format!(
                "archive member {name} exceeds 256 MiB"
            )));
        }
        expanded = expanded
            .checked_add(size)
            .ok_or_else(|| Error::Bounds("expanded size overflow".into()))?;
        if expanded > MAX_EXPANDED_BYTES {
            return Err(Error::Bounds("archive expands past 512 MiB".into()));
        }
        if seen.len() == 16 {
            return Err(Error::Bounds("archive has more than 16 members".into()));
        }
        let declared = expected
            .get(&name)
            .ok_or_else(|| Error::Archive(format!("undeclared archive member {name}")))?;
        let seen_member = read_member(&mut member, declared.0)?;
        if seen.insert(name.clone(), seen_member).is_some() {
            return Err(Error::Archive(format!("duplicate archive member {name}")));
        }
    }
    let cursor = archive.into_inner();
    let consumed = cursor.position() as usize;
    if expanded_bytes
        .get(consumed..)
        .is_some_and(|tail| tail.iter().any(|byte| *byte != 0))
    {
        return Err(Error::Archive("tar has trailing decompressed data".into()));
    }
    Ok(seen)
}

fn validate_member_mode(path: &str, mode: Option<u32>) -> Result<()> {
    let basename = path
        .rsplit('/')
        .next()
        .ok_or_else(|| Error::Archive("archive member has no basename".into()))?;
    let expected = match basename {
        "flux-exchange" => 0o755,
        "LICENSE-APACHE" | "LICENSE-MIT" | "README.md" => 0o644,
        _ => {
            return Err(Error::Archive(format!(
                "supporting member basename {basename:?} is outside the packager contract"
            )))
        }
    };
    if !matches!(mode, Some(actual) if actual == expected || actual == 0o100000 | expected) {
        return Err(Error::Archive(format!(
            "archive member {path} mode is not {expected:o}"
        )));
    }
    Ok(())
}

fn read_member(reader: &mut impl Read, declared: u64) -> Result<SeenMember> {
    let mut hasher = Sha256::new();
    let mut total = 0u64;
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| Error::Archive(error.to_string()))?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .ok_or_else(|| Error::Bounds("member size overflow".into()))?;
        if total > declared || total > MAX_MEMBER_BYTES {
            return Err(Error::Bounds(
                "member decompressed past its declared bound".into(),
            ));
        }
        hasher.update(&buffer[..read]);
    }
    if total != declared {
        return Err(Error::Archive(
            "member ended before its declared size".into(),
        ));
    }
    Ok(SeenMember {
        bytes: total,
        sha256: crate::lower_hex(&hasher.finalize()),
    })
}

pub(crate) fn validate_member_path(path: &str) -> Result<()> {
    if path.is_empty() || path.len() > 240 || path.contains('\\') || path.starts_with('/') {
        return Err(Error::Archive(format!(
            "unsafe archive member path {path:?}"
        )));
    }
    let components: Vec<_> = path.split('/').collect();
    if components.len() < 2
        || components
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(Error::Archive(format!(
            "member path is not relative single-root: {path}"
        )));
    }
    let root = components[0];
    if !root.is_ascii()
        || !root.as_bytes()[0].is_ascii_alphanumeric()
        || !root.as_bytes()[root.len() - 1].is_ascii_alphanumeric()
        || root.contains("..")
        || !root
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    {
        return Err(Error::Archive(format!("unsafe archive root {root:?}")));
    }
    Ok(())
}

pub(crate) fn ensure_one_root(paths: impl Iterator<Item = String>) -> Result<()> {
    let mut roots = BTreeSet::new();
    let mut folded = BTreeSet::new();
    for path in paths {
        validate_member_path(&path)?;
        let root = path
            .split('/')
            .next()
            .ok_or_else(|| Error::Archive("missing archive root".into()))?;
        roots.insert(root.to_owned());
        if !folded.insert(path.to_ascii_lowercase()) {
            return Err(Error::Archive(
                "member paths collide under ASCII case folding".into(),
            ));
        }
    }
    if roots.len() != 1 {
        return Err(Error::Archive(
            "archive members do not share exactly one root".into(),
        ));
    }
    Ok(())
}

pub(crate) fn package(
    version: &str,
    target: &str,
    executable: &Path,
    licenses: &[PathBuf],
    documentation: Option<&Path>,
    output_directory: &Path,
) -> Result<Asset> {
    crate::policy::parse_stable_version(version)?;
    Platform::from_target(target)?;
    let executable_bytes = read_bounded_file(executable, MAX_MEMBER_BYTES)?;
    if licenses.len() != 2 {
        return Err(Error::Schema(
            "package requires exactly two --license inputs".into(),
        ));
    }
    let mut supporting = Vec::new();
    for license in licenses {
        let basename = license
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| Error::Schema("license has no UTF-8 basename".into()))?;
        if !matches!(basename, "LICENSE-APACHE" | "LICENSE-MIT") {
            return Err(Error::Schema(format!(
                "unadmitted license basename {basename:?}"
            )));
        }
        let bytes = read_bounded_file(license, MAX_MEMBER_BYTES)?;
        supporting.push((basename.to_owned(), MemberKind::License, bytes));
    }
    if let Some(documentation) = documentation {
        if documentation.file_name().and_then(|name| name.to_str()) != Some("README.md") {
            return Err(Error::Schema(
                "documentation basename must be README.md".into(),
            ));
        }
        let bytes = read_bounded_file(documentation, MAX_MEMBER_BYTES)?;
        supporting.push(("README.md".into(), MemberKind::Documentation, bytes));
    }
    supporting.sort_by(|left, right| left.0.cmp(&right.0));
    for (name, bytes) in std::iter::once(("executable", &executable_bytes)).chain(
        supporting
            .iter()
            .map(|(name, _, bytes)| (name.as_str(), bytes)),
    ) {
        if bytes.is_empty() || bytes.len() as u64 > MAX_MEMBER_BYTES {
            return Err(Error::Bounds(format!("{name} is outside 1..=256 MiB")));
        }
    }
    let root = format!("flux-exchange-{version}-{target}");
    crate::policy::validate_basename(&root)?;
    let executable_basename = "flux-exchange";
    let executable_path = format!("{root}/{executable_basename}");
    let archive = format!("flux-exchange-{version}-{target}.tar.zst");
    let format = "tar.zst";
    std::fs::create_dir_all(output_directory)
        .map_err(|error| Error::Io(output_directory.to_owned(), error))?;
    let output = output_directory.join(&archive);
    let mut members = vec![(executable_path.clone(), executable_bytes.as_slice(), 0o755)];
    let supporting_paths: Vec<_> = supporting
        .iter()
        .map(|(name, _, bytes)| (format!("{root}/{name}"), bytes.as_slice(), 0o644))
        .collect();
    members.extend(
        supporting_paths
            .iter()
            .map(|(path, bytes, mode)| (path.clone(), *bytes, *mode)),
    );
    let bytes = deterministic_tar_zst(
        members
            .iter()
            .map(|(path, bytes, mode)| (path, *bytes, *mode)),
    )?;
    if output.exists() {
        let existing = read_bounded_file(&output, MAX_ARCHIVE_BYTES)?;
        if existing != bytes {
            return Err(Error::Archive(format!(
                "refusing to replace different deterministic archive {}",
                output.display()
            )));
        }
    } else {
        std::fs::write(&output, &bytes).map_err(|error| Error::Io(output.clone(), error))?;
    }
    let mut other_members: Vec<_> = supporting
        .into_iter()
        .map(|(name, kind, bytes)| crate::OtherMember {
            path: format!("{root}/{name}"),
            kind,
            bytes: bytes.len() as u64,
            sha256: digest_hex(&bytes),
        })
        .collect();
    other_members.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(Asset {
        target: target.into(),
        archive,
        format: format.into(),
        archive_bytes: bytes.len() as u64,
        archive_sha256: digest_hex(&bytes),
        executable: crate::Member {
            path: executable_path,
            bytes: executable_bytes.len() as u64,
            sha256: digest_hex(&executable_bytes),
        },
        other_members,
    })
}

fn deterministic_tar_zst<'a>(
    members: impl IntoIterator<Item = (&'a String, &'a [u8], u32)>,
) -> Result<Vec<u8>> {
    let mut tar_bytes = Vec::new();
    {
        let mut builder = tar::Builder::new(&mut tar_bytes);
        for (path, bytes, mode) in members {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(mode);
            header.set_uid(0);
            header.set_gid(0);
            header.set_mtime(0);
            header.set_cksum();
            builder
                .append_data(&mut header, path, Cursor::new(bytes))
                .map_err(|error| Error::Archive(error.to_string()))?;
        }
        builder
            .finish()
            .map_err(|error| Error::Archive(error.to_string()))?;
    }
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), 19)
        .map_err(|error| Error::Archive(error.to_string()))?;
    encoder
        .include_checksum(true)
        .map_err(|error| Error::Archive(error.to_string()))?;
    encoder
        .write_all(&tar_bytes)
        .map_err(|error| Error::Archive(error.to_string()))?;
    encoder
        .finish()
        .map_err(|error| Error::Archive(error.to_string()))
}
