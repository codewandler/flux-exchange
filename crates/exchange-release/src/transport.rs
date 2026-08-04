//! Closed GitHub release redirect validation.

use std::collections::BTreeSet;
use url::Url;

use crate::{Error, Result};

/// The three immutable/fixed GitHub release namespaces admitted by the provider protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InitialResource<'a> {
    Trust,
    Channel,
    Immutable { version: &'a str, basename: &'a str },
}

/// Validate an initial GitHub request before any connection is opened.
pub fn validate_initial_url(url: &str, resource: InitialResource<'_>) -> Result<()> {
    if url.len() > 8192 || !url.is_ascii() {
        return Err(Error::Bounds("initial URL exceeds 8192 ASCII bytes".into()));
    }
    let prefix = "https://github.com/codewandler/flux-exchange/releases/download/";
    let suffix = url
        .strip_prefix(prefix)
        .ok_or_else(|| Error::Transport("initial URL origin/path prefix is not exact".into()))?;
    if suffix.contains(['?', '#', '\\']) || suffix.contains("//") {
        return Err(Error::Transport(
            "initial URL contains query, fragment or noncanonical path syntax".into(),
        ));
    }
    let expected = match resource {
        InitialResource::Trust => "exchange-trust-v1/flux-exchange-release-trust.json".to_owned(),
        InitialResource::Channel => {
            "exchange-stable-v1/flux-exchange-release-channel.json".to_owned()
        }
        InitialResource::Immutable { version, basename } => {
            let parsed = semver::Version::parse(version)
                .map_err(|_| Error::Transport("immutable release version is invalid".into()))?;
            if !parsed.pre.is_empty() || !parsed.build.is_empty() {
                return Err(Error::Transport(
                    "immutable release version must be stable SemVer".into(),
                ));
            }
            if basename.is_empty()
                || basename.contains('/')
                || basename.contains('\\')
                || basename == "."
                || basename == ".."
                || !basename.is_ascii()
            {
                return Err(Error::Transport(
                    "immutable release basename is unsafe".into(),
                ));
            }
            format!("v{version}/{basename}")
        }
    };
    if suffix != expected {
        return Err(Error::Transport(
            "initial URL is not the exact fixed or immutable release asset".into(),
        ));
    }
    Ok(())
}

/// Validate the initial URL and prove that no ambient/request credential is attached.
pub fn validate_initial_request(
    url: &str,
    resource: InitialResource<'_>,
    credentials_present: bool,
) -> Result<()> {
    if credentials_present {
        return Err(Error::Transport(
            "authorization, proxy authorization and cookies are forbidden".into(),
        ));
    }
    validate_initial_url(url, resource)
}

const QUERY_NAMES: &[&str] = &[
    "jwt",
    "response-content-disposition",
    "response-content-type",
    "rscd",
    "rsct",
    "se",
    "sig",
    "ske",
    "skoid",
    "sks",
    "skt",
    "sktid",
    "skv",
    "sp",
    "spr",
    "sr",
    "sv",
];

/// Validate the sole admitted GitHub-to-CDN response transition.
pub fn validate_redirect(status: u16, location: &str, forwarded_credentials: bool) -> Result<()> {
    if status != 302 {
        return Err(Error::Transport(format!(
            "initial response is {status}, expected 302"
        )));
    }
    if forwarded_credentials {
        return Err(Error::Transport(
            "credentials, cookies and proxy authorization are forbidden".into(),
        ));
    }
    if location.len() > 8192 || !location.is_ascii() {
        return Err(Error::Bounds(
            "redirect Location exceeds 8192 ASCII bytes".into(),
        ));
    }
    let raw_authority = location
        .strip_prefix("https://")
        .and_then(|rest| rest.split('/').next())
        .ok_or_else(|| Error::Transport("redirect must use lowercase https syntax".into()))?;
    if !matches!(
        raw_authority,
        "release-assets.githubusercontent.com" | "release-assets.githubusercontent.com:443"
    ) {
        return Err(Error::Transport(
            "redirect authority spelling is not exact".into(),
        ));
    }
    let url = Url::parse(location).map_err(|error| Error::Transport(error.to_string()))?;
    if url.scheme() != "https"
        || url.host_str() != Some("release-assets.githubusercontent.com")
        || url.port().is_some_and(|port| port != 443)
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(Error::Transport(
            "redirect authority is outside the closed CDN origin".into(),
        ));
    }
    validate_path(url.path())?;
    let query = url
        .query()
        .ok_or_else(|| Error::Transport("redirect query is required".into()))?;
    if query.len() > 6144 {
        return Err(Error::Bounds("redirect query exceeds 6144 bytes".into()));
    }
    let mut names = BTreeSet::new();
    for pair in query.split('&') {
        let (name, value) = pair.split_once('=').ok_or_else(|| {
            Error::Transport("query values must be explicit and non-empty".into())
        })?;
        if name.is_empty() || value.is_empty() || value.len() > 2048 {
            return Err(Error::Bounds(
                "redirect query value is empty or exceeds 2048 bytes".into(),
            ));
        }
        if name.contains('%') || !QUERY_NAMES.contains(&name) || !names.insert(name) {
            return Err(Error::Transport(format!(
                "query name {name:?} is encoded, unknown or repeated"
            )));
        }
        validate_percent_value(value)?;
    }
    Ok(())
}

/// Validate that the second response is final and successful.
pub fn validate_final(status: u16, has_location: bool) -> Result<()> {
    if status != 200 || has_location {
        return Err(Error::Transport(
            "CDN response must be final HTTP 200".into(),
        ));
    }
    Ok(())
}

fn validate_path(path: &str) -> Result<()> {
    let prefix = "/github-production-release-asset/";
    let rest = path
        .strip_prefix(prefix)
        .ok_or_else(|| Error::Transport("redirect path has the wrong prefix".into()))?;
    let (repository, uuid) = rest
        .split_once('/')
        .ok_or_else(|| Error::Transport("redirect path is incomplete".into()))?;
    if repository.is_empty()
        || repository.len() > 20
        || repository.starts_with('0')
        || !repository.bytes().all(|byte| byte.is_ascii_digit())
        || !is_lower_uuid(uuid)
    {
        return Err(Error::Transport(
            "redirect path is outside the release-asset grammar".into(),
        ));
    }
    Ok(())
}

fn is_lower_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte),
        })
}

fn validate_percent_value(value: &str) -> Result<()> {
    let bytes = value.as_bytes();
    let mut decoded = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%' {
            if index + 2 >= bytes.len() {
                return Err(Error::Transport("truncated percent encoding".into()));
            }
            let high = hex(bytes[index + 1])?;
            let low = hex(bytes[index + 2])?;
            decoded.push((high << 4) | low);
            index += 3;
        } else {
            if !raw_query_value_byte(bytes[index]) {
                return Err(Error::Transport(
                    "query value contains a raw byte outside RFC 3986 policy".into(),
                ));
            }
            decoded.push(bytes[index]);
            index += 1;
        }
    }
    if decoded.iter().any(|byte| *byte < 0x20 || *byte == 0x7f) {
        return Err(Error::Transport(
            "decoded query value contains a control byte".into(),
        ));
    }
    Ok(())
}

fn raw_query_value_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric()
        || matches!(
            byte,
            b'.' | b'_'
                | b'~'
                | b'!'
                | b'$'
                | b'\''
                | b'('
                | b')'
                | b'*'
                | b'+'
                | b','
                | b';'
                | b'='
                | b':'
                | b'@'
                | b'/'
                | b'?'
                | b'-'
        )
}

fn hex(byte: u8) -> Result<u8> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(Error::Transport("invalid percent encoding".into())),
    }
}
