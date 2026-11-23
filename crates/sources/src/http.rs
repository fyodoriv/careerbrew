//! Bounded recorded HTTP transport and host/redirect authorization.

use crate::{Result, SourceError};
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub const TIMEOUT_SECONDS: u64 = 30;
pub const MAX_RESPONSE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_REDIRECTS: usize = 10;
const PLAIN_HEADERS: &[&str] = &[
    "user-agent",
    "accept",
    "accept-language",
    "content-type",
    "content-length",
    "host",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub path: String,
}

pub fn parse_url(url: &str) -> Result<Url> {
    let (scheme, rest) = url
        .split_once("://")
        .ok_or_else(|| SourceError::value("invalid URL"))?;
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(SourceError::value("unsupported URL scheme"));
    }
    let split = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..split];
    let path = if split == rest.len() {
        "/"
    } else {
        &rest[split..]
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.contains(']') && port.chars().all(|c| c.is_ascii_digit()) => (
            host,
            port.parse::<u16>()
                .map_err(|_| SourceError::value("invalid URL port"))?,
        ),
        _ => (authority, if scheme == "https" { 443 } else { 80 }),
    };
    if host.is_empty() || authority.contains('@') || authority.contains(['?', '#']) {
        return Err(SourceError::value("invalid URL authority"));
    }
    Ok(Url {
        scheme,
        host: host.to_ascii_lowercase(),
        port,
        path: path.to_owned(),
    })
}

pub fn host_allowed(host: &str, allowed: Option<&[String]>) -> bool {
    if matches!(host, "127.0.0.1" | "localhost" | "::1") {
        return true;
    }
    allowed.is_none_or(|hosts| {
        hosts
            .iter()
            .any(|allowed| host == allowed || host.ends_with(&format!(".{allowed}")))
    })
}

pub fn check_host(url_or_host: &str, allowed: Option<&[String]>) -> Result<()> {
    let host = if url_or_host.contains("://") {
        parse_url(url_or_host)?.host
    } else {
        url_or_host.to_ascii_lowercase()
    };
    if host_allowed(&host, allowed) {
        Ok(())
    } else {
        Err(SourceError::connector(format!(
            "host {host} is not in the plugin policy allowed_hosts"
        )))
    }
}

/// Authorize one redirect before any credential can move. The caller owns the
/// request scope; this pure decision reads no network or process state.
pub fn redirect(
    source: &str,
    target: &str,
    headers: &[String],
    allowed: Option<&[String]>,
) -> Result<String> {
    check_host(target, allowed)?;
    let old = parse_url(source)?;
    let new = parse_url(target)?;
    if old.scheme == "https" && new.scheme != "https" {
        return Err(SourceError::connector(format!(
            "redirect from HTTPS to plain HTTP refused ({})",
            new.host
        )));
    }
    let another_origin = (&old.scheme, &old.host, old.port) != (&new.scheme, &new.host, new.port);
    if another_origin
        && headers
            .iter()
            .any(|name| !PLAIN_HEADERS.contains(&name.to_ascii_lowercase().as_str()))
    {
        return Err(SourceError::connector(format!(
            "redirect to another address refused for a request that carries a token ({})",
            new.host
        )));
    }
    Ok(target.to_owned())
}

pub fn fixture_path(root: &Path, url: &str, extension: &str) -> Result<PathBuf> {
    let parsed = parse_url(url)?;
    let raw = parsed
        .path
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_matches('/');
    let mut key = if raw.is_empty() {
        "root".to_owned()
    } else {
        raw.to_owned()
    };
    key = key
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    Ok(root.join(parsed.host).join(format!("{key}{extension}")))
}

fn bounded_read(path: &Path) -> Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_RESPONSE_BYTES {
        return Err(SourceError::new(
            "OSError",
            "recorded response exceeds its bound",
        ));
    }
    Ok(std::fs::read(path)?)
}

pub fn get_bytes(root: &Path, url: &str, allowed: Option<&[String]>) -> Result<Vec<u8>> {
    check_host(url, allowed)?;
    for extension in [".json", ".xml", ".html", ".csv"] {
        let path = fixture_path(root, url, extension)?;
        if path.exists() {
            return bounded_read(&path);
        }
    }
    Err(SourceError::missing())
}

pub fn get_json(root: &Path, url: &str, allowed: Option<&[String]>) -> Result<Value> {
    check_host(url, allowed)?;
    let bytes = bounded_read(&fixture_path(root, url, ".json")?)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    if let Some(status) = value.get("__status").and_then(Value::as_u64) {
        return Err(SourceError::http(status.clamp(100, 599) as u16));
    }
    Ok(value)
}

/// A request's allowed header names, used by synthetic credential controls.
pub fn secret_header_names(headers: &[String]) -> BTreeSet<String> {
    headers
        .iter()
        .filter(|name| !PLAIN_HEADERS.contains(&name.to_ascii_lowercase().as_str()))
        .cloned()
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_suffix_and_cross_origin_credentials_fail() {
        let allowed = Some(vec!["example.com".to_owned()]);
        assert!(!host_allowed("badexample.com", allowed.as_deref()));
        assert!(redirect(
            "https://api.example.com/a",
            "https://api.example.org/b",
            &["authorization".into()],
            None
        )
        .is_err());
    }

    #[test]
    fn fixture_identity_ignores_query_and_bounds_pages() {
        let path = fixture_path(
            Path::new("/synthetic"),
            "https://api.example.com/a/b?x=1",
            ".json",
        )
        .unwrap();
        assert_eq!(path, Path::new("/synthetic/api.example.com/a_b.json"));
        assert_eq!(MAX_REDIRECTS, 10);
    }
}
