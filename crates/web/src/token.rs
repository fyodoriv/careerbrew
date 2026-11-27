//! The web token (issue #141), the pinned clock and the few settings the
//! native front reads. Mirrors `server.TokenSource`, `_stored_token`,
//! `_expires`, `_token_days` and `calendar.now`.

use careerbrew_vault::canon::Canon;
use careerbrew_vault::transfer::Context;
use chrono::{DateTime, Duration, FixedOffset, NaiveDateTime, TimeZone, Utc};
use num_like::to_i64;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub const DEFAULT_TOKEN_DAYS: i64 = 30;

mod num_like {
    use careerbrew_vault::canon::Canon;

    /// Python `int(value)` for a settings value; None where Python raises.
    pub fn to_i64(value: &Canon) -> Option<i64> {
        match value {
            Canon::Bool(v) => Some(i64::from(*v)),
            Canon::Int(v) => v.to_string().parse().ok(),
            Canon::Float(v) if v.is_finite() => Some(v.trunc() as i64),
            Canon::Str(v) => super::py_int(v),
            _ => None,
        }
    }
}

/// Python `int(text)` for base 10: surrounding whitespace, one sign, digits
/// with single underscores between them. None where Python raises ValueError.
pub fn py_int(text: &str) -> Option<i64> {
    let t = text.trim();
    let (neg, digits) = match t.as_bytes().first() {
        Some(b'-') => (true, &t[1..]),
        Some(b'+') => (false, &t[1..]),
        _ => (false, t),
    };
    if digits.is_empty()
        || digits.starts_with('_')
        || digits.ends_with('_')
        || digits.contains("__")
        || !digits.chars().all(|c| c.is_ascii_digit() || c == '_')
    {
        return None;
    }
    let value: i64 = digits.replace('_', "").parse().ok()?;
    Some(if neg { -value } else { value })
}

fn map_get<'a>(value: &'a Canon, name: &str) -> Option<&'a Canon> {
    let Canon::Map(items) = value else {
        return None;
    };
    items
        .iter()
        .find(|(k, _)| matches!(k, Canon::Str(k) if k == name))
        .map(|(_, v)| v)
}

/// The saved settings (`settings.yaml`), or an empty map when unreadable.
pub fn settings(ctx: &Context) -> Option<Canon> {
    careerbrew_vault::settings::read_settings(&ctx.store()).ok()
}

pub fn setting<'a>(settings: &'a Option<Canon>, key: &str) -> Option<&'a Canon> {
    settings.as_ref().and_then(|s| map_get(s, key))
}

/// `_token_days()`: `max(1, int(web_token_max_days))`, default on a bad value.
pub fn token_days(ctx: &Context) -> i64 {
    let saved = settings(ctx);
    match setting(&saved, "web_token_max_days") {
        None => DEFAULT_TOKEN_DAYS,
        Some(value) => to_i64(value).map_or(DEFAULT_TOKEN_DAYS, |v| v.max(1)),
    }
}

/// `config.get("web_tab_cache") is not False`.
pub fn tab_cache_on(ctx: &Context) -> bool {
    let saved = settings(ctx);
    !matches!(setting(&saved, "web_tab_cache"), Some(Canon::Bool(false)))
}

/// `datetime.fromisoformat` for the forms careerbrew writes and tests pin.
pub fn parse_iso(text: &str) -> Option<(NaiveDateTime, Option<FixedOffset>)> {
    if let Ok(dt) = DateTime::parse_from_rfc3339(text) {
        return Some((dt.naive_local(), Some(*dt.offset())));
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f%:z",
        "%Y-%m-%dT%H:%M%:z",
        "%Y-%m-%d %H:%M:%S%.f%:z",
        "%Y-%m-%d %H:%M%:z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(text, fmt) {
            return Some((dt.naive_local(), Some(*dt.offset())));
        }
    }
    for fmt in [
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(text, fmt) {
            return Some((dt, None));
        }
    }
    None
}

fn zone_name(ctx: &Context) -> String {
    let saved = settings(ctx);
    if let Some(Canon::Str(zone)) = setting(&saved, "timezone") {
        if !zone.is_empty() {
            return zone.clone();
        }
    }
    let name = std::env::var("TZ").unwrap_or_default();
    let name = name.trim_start_matches(':').to_owned();
    if !name.is_empty() {
        return name;
    }
    std::fs::canonicalize("/etc/localtime")
        .ok()
        .and_then(|p| {
            p.to_string_lossy()
                .split_once("/zoneinfo/")
                .map(|(_, z)| z.to_owned())
        })
        .unwrap_or_else(|| "UTC".into())
}

fn localize(ctx: &Context, naive: NaiveDateTime) -> DateTime<FixedOffset> {
    let zone: chrono_tz::Tz = zone_name(ctx).parse().unwrap_or(chrono_tz::UTC);
    zone.from_local_datetime(&naive)
        .earliest()
        .unwrap_or_else(|| zone.from_utc_datetime(&naive))
        .fixed_offset()
}

/// `calendar.now()`: `CAREERBREW_NOW` pins it for tests and e2e runs.
pub fn now(ctx: &Context) -> DateTime<FixedOffset> {
    if let Some((naive, offset)) = std::env::var("CAREERBREW_NOW")
        .ok()
        .filter(|v| !v.is_empty())
        .and_then(|v| parse_iso(&v))
    {
        return match offset {
            Some(offset) => offset.from_local_datetime(&naive).unwrap(),
            None => localize(ctx, naive),
        };
    }
    Utc::now().fixed_offset()
}

pub fn token_shape(token: &str) -> bool {
    token.len() >= 20
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

pub fn web_token_path(root: &Path) -> PathBuf {
    root.join("day").join("web-token.json")
}

/// `_stored_token()`: (token, created), None when missing or unreadable.
pub fn stored_token(ctx: &Context) -> Option<(String, DateTime<FixedOffset>)> {
    let value = ctx
        .store()
        .read_json(&web_token_path(&ctx.root), Canon::Map(Vec::new()), true)
        .ok()?;
    let token = match map_get(&value, "token") {
        Some(Canon::Str(t)) => t.clone(),
        _ => String::new(),
    };
    let created = match map_get(&value, "created")? {
        Canon::Str(c) | Canon::DateTime(c) => c.clone(),
        _ => return None,
    };
    let (naive, offset) = parse_iso(&created)?;
    let created = offset?.from_local_datetime(&naive).single()?;
    token_shape(&token).then_some((token, created))
}

pub fn expires(ctx: &Context, created: DateTime<FixedOffset>) -> DateTime<FixedOffset> {
    created + Duration::days(token_days(ctx))
}

/// `TokenSource`: a pinned token never changes or expires; otherwise the file
/// is read again whenever its inode, size or mtime changes.
/// Lifecycle: server-instance scope, owned by one `Server`.
pub struct TokenSource {
    pub pinned: String,
    cached: Mutex<Cached>,
}

/// The token file's (inode, size, mtime) and the token with its expiry read from it.
type Cached = (
    Option<(u64, u64, i128)>,
    Option<(String, DateTime<FixedOffset>)>,
);

impl TokenSource {
    pub fn new(pinned: Option<String>) -> Self {
        Self {
            pinned: pinned.unwrap_or_default(),
            cached: Mutex::new((None, None)),
        }
    }

    pub fn current(&self, ctx: &Context) -> (String, Option<DateTime<FixedOffset>>) {
        if !self.pinned.is_empty() {
            return (self.pinned.clone(), None);
        }
        let Ok(st) = std::fs::metadata(web_token_path(&ctx.root)) else {
            return (String::new(), None);
        };
        let sig = (
            st.ino(),
            st.size(),
            i128::from(st.mtime()) * 1_000_000_000 + i128::from(st.mtime_nsec()),
        );
        let stored = {
            let mut cached = self.cached.lock().unwrap_or_else(|e| e.into_inner());
            if cached.0 != Some(sig) {
                *cached = (Some(sig), stored_token(ctx));
            }
            cached.1.clone()
        };
        match stored {
            Some((token, created)) => (token, Some(expires(ctx, created))),
            None => (String::new(), None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn python_int_rules() {
        assert_eq!(py_int(" 12 "), Some(12));
        assert_eq!(py_int("-3"), Some(-3));
        assert_eq!(py_int("1_000"), Some(1000));
        assert_eq!(py_int("1__0"), None);
        assert_eq!(py_int("abc"), None);
        assert_eq!(py_int(""), None);
        assert_eq!(py_int("_1"), None);
    }

    #[test]
    fn iso_forms() {
        assert!(parse_iso("2026-11-20T10:00:00+00:00").unwrap().1.is_some());
        assert!(parse_iso("2026-11-20T10:00").unwrap().1.is_none());
        assert!(parse_iso("2026-11-20T10:00:00Z").unwrap().1.is_some());
        assert!(parse_iso("nope").is_none());
        assert!(token_shape(&"a".repeat(20)));
        assert!(!token_shape("short"));
        assert!(!token_shape(&format!("{}!", "a".repeat(20))));
    }
}
