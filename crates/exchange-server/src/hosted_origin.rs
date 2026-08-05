//! Startup-only canonical origin binding for hosted local management.

use std::net::{IpAddr, SocketAddr};

pub const SETTING: &str = "FLUX_EXCHANGE_CONSOLE_ORIGIN";

pub fn configured(development: bool, bind: SocketAddr) -> Result<Option<String>, String> {
    match std::env::var(SETTING) {
        Ok(value) if value.is_empty() => Err(format!("{SETTING} is explicitly empty")),
        Ok(value) => canonical(&value, development).map(Some),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{SETTING} is not Unicode")),
        Err(std::env::VarError::NotPresent) if development => derive_loopback(bind).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
    }
}

fn canonical(value: &str, development: bool) -> Result<String, String> {
    if !value.is_ascii() || value.trim() != value {
        return Err(refusal());
    }
    let (scheme, authority) = value.split_once("://").ok_or_else(refusal)?;
    if !matches!(scheme, "http" | "https") || authority.is_empty() {
        return Err(refusal());
    }
    if authority
        .bytes()
        .any(|byte| matches!(byte, b'/' | b'?' | b'#' | b'@'))
    {
        return Err(refusal());
    }
    let (host, port) = split_authority(authority)?;
    admit_host(host)?;
    let port = port.map(parse_port).transpose()?;
    if matches!((scheme, port), ("http", Some(80)) | ("https", Some(443))) {
        return Err(refusal());
    }
    if scheme == "http" && (!development || !literal_loopback(host)) {
        return Err(refusal());
    }
    Ok(value.to_owned())
}

fn split_authority(authority: &str) -> Result<(&str, Option<&str>), String> {
    if authority.starts_with('[') {
        let end = authority.find(']').ok_or_else(refusal)?;
        let host = &authority[..=end];
        let rest = &authority[end + 1..];
        return match rest.strip_prefix(':') {
            Some(port) if !port.is_empty() => Ok((host, Some(port))),
            None if rest.is_empty() => Ok((host, None)),
            _ => Err(refusal()),
        };
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') && !port.is_empty() => Ok((host, Some(port))),
        Some(_) => Err(refusal()),
        None => Ok((authority, None)),
    }
}

fn admit_host(host: &str) -> Result<(), String> {
    if host.starts_with('[') {
        let inner = host
            .strip_prefix('[')
            .and_then(|value| value.strip_suffix(']'))
            .ok_or_else(refusal)?;
        let address = inner.parse::<std::net::Ipv6Addr>().map_err(|_| refusal())?;
        if address.to_string() != inner {
            return Err(refusal());
        }
        return Ok(());
    }
    if let Ok(address) = host.parse::<std::net::Ipv4Addr>() {
        return (address.to_string() == host)
            .then_some(())
            .ok_or_else(refusal);
    }
    if host
        .bytes()
        .all(|byte| byte.is_ascii_digit() || byte == b'.')
        || host.is_empty()
        || host.ends_with('.')
        || host.to_ascii_lowercase() != host
        || host.split('.').any(|label| {
            label.is_empty()
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        })
    {
        return Err(refusal());
    }
    Ok(())
}

fn parse_port(port: &str) -> Result<u16, String> {
    if port.starts_with('0') || !port.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(refusal());
    }
    port.parse::<u16>()
        .ok()
        .filter(|port| *port != 0)
        .ok_or_else(refusal)
}

fn literal_loopback(host: &str) -> bool {
    let unwrapped = host
        .strip_prefix('[')
        .and_then(|value| value.strip_suffix(']'))
        .unwrap_or(host);
    unwrapped
        .parse::<IpAddr>()
        .is_ok_and(|address| address.is_loopback())
}

fn derive_loopback(bind: SocketAddr) -> Result<String, String> {
    if !bind.ip().is_loopback() {
        return Err("--dev cannot derive a hosted origin from a non-loopback bind".to_owned());
    }
    let host = match bind.ip() {
        IpAddr::V4(address) => address.to_string(),
        IpAddr::V6(address) => format!("[{address}]"),
    };
    if bind.port() == 80 {
        Ok(format!("http://{host}"))
    } else {
        Ok(format!("http://{host}:{}", bind.port()))
    }
}

fn refusal() -> String {
    format!(
        "{SETTING} must be one canonical ASCII origin with no userinfo, path, query, fragment or explicit default port"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_canonical_origins_are_closed() {
        for admitted in [
            "https://example.com",
            "https://example.com:8443",
            "http://127.0.0.1",
            "http://[::1]:3000",
        ] {
            assert_eq!(canonical(admitted, true).as_deref(), Ok(admitted));
        }
        for refused in [
            "https://example.com/",
            "https://example.com:443",
            "http://127.0.0.1:80",
            "HTTPS://example.com",
            "https://Example.com",
            "https://user@example.com",
            "https://example.com:080",
            "https://127.00.0.1",
            "https://[0:0:0:0:0:0:0:1]",
        ] {
            assert!(canonical(refused, true).is_err(), "admitted {refused}");
        }
        assert!(canonical("http://example.com", true).is_err());
        assert!(canonical("http://127.0.0.1", false).is_err());
    }

    #[test]
    fn absent_dev_origin_derives_only_from_the_literal_loopback_listener() {
        assert_eq!(
            derive_loopback("127.0.0.1:3000".parse().expect("bind")).as_deref(),
            Ok("http://127.0.0.1:3000")
        );
        assert_eq!(
            derive_loopback("[::1]:80".parse().expect("bind")).as_deref(),
            Ok("http://[::1]")
        );
        assert!(derive_loopback("0.0.0.0:3000".parse().expect("bind")).is_err());
    }
}
