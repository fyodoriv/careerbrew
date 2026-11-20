//! `privacy.scrub_public` (rust-04e): make text safe to publish over
//! supplied terms. Same steps and pattern text as Python, matched with the
//! `re`-compatible matcher in `pyre.rs`.

use crate::allow::{self, PyError};
use crate::canary::{self, Rules};
use crate::pyre::{self, Regex};
use crate::text;

/// `privacy.PUBLIC_PATTERNS`, in order: (pattern, `re.IGNORECASE`).
pub const PUBLIC_PATTERNS: &[(&str, bool)] = &[
    (r"(?:https?|hxxps?|ftp)s?:/+\S*|mailto:\S*", true),
    (r"[\w.+-]+@[\w-]+(\.[\w-]+)+", false),
    (
        r"\b\w+\s*(?:\[at\]|\(at\)|\sat\s)\s*\w+(?:\s*(?:\.|\[dot\]|\(dot\)|\sdot\s)\s*\w+)+",
        true,
    ),
    (r"\b[\w-]+/[\w.-]+#\d+\b", false),
    (r"\(?#[\w-]+\)?", false),
    (r"(?<![\w])!\d+\b", false),
    (r"(?<![\w])@[\w][\w.-]*", false),
    (r"\bAKIA[0-9A-Z]{16}\b|\b[A-Za-z0-9_\-]{32,}\b", false),
    (
        r"\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b",
        false,
    ),
    (r"\[?[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}\]?(?::\d+)?", true),
    (r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b", false),
    (r"\b[A-Za-z]:\\\S*|\\\\\S+", false),
    (r"(?:(?<=\s)|^)(?:~|\.{1,2})?/[\w.-]+(?:/[\w.-]+)*", false),
    (r"\bgo/[\w.-]+", true),
    (r"\b(?:[a-z0-9-]+\.){2,}[a-z]{2,}\b", true),
    (
        r"\b[a-z0-9-]+\.(?:com|net|org|io|dev|internal|corp|intra|local|lan|home|test|example|cloud|app|ai|co)\b",
        true,
    ),
    (r"\b[a-z]+-\d+\b", false),
    (r"\b[A-Z][A-Z0-9]{1,9}-\d+\b", false),
    (
        r"(?:\+\d{1,3}[\s.-]?)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}\b",
        false,
    ),
    (
        r"\b\d{1,6}\s+(?:[A-Z][a-z]+\s+){1,3}(?:St|Street|Ave|Avenue|Rd|Road|Blvd|Dr|Drive|Ln|Lane|Way)\b\.?",
        false,
    ),
    (
        r"\b(?:room|rm|bldg|building|floor|desk)\s+\w+(?:\s+(?:bldg|building|floor)\s+\w+)?",
        true,
    ),
    (
        r"\b(?:employee|badge|staff|personnel)\s+(?:id|number|no\.?|#)\s*\w+",
        true,
    ),
];

/// `privacy._DANGLING` (`re.IGNORECASE`).
const DANGLING: &str = r"(?:\s+|^)(?:and|or|see|to|in|on|at|from|via|with|for|use|ping|mail|thanks|host|server|key|commit|by|ipv6|ran|saved|posted|merged|closed|plus|hit|email|pr|mr|ticket|bucket|cluster|tracked|copied|pushed|deployed|renamed|bot|dm'd|old)\s*(?=[;,.:]|$)";

/// The default replacement of `scrub_public`.
pub const REPLACEMENT: &str = "an internal project";

fn compile(rules: &Rules, src: &str, icase: bool) -> Result<Regex, PyError> {
    Regex::new(src, icase, rules).map_err(|e| PyError::new("error", e))
}

/// `privacy.scrub_public` over supplied terms. A replacement with a
/// backslash (a `re` template) is out of contract and refused.
pub fn scrub_public(
    rules: &Rules,
    text_in: &str,
    terms: &[String],
    replacement: &str,
) -> Result<String, PyError> {
    if replacement.contains('\\') {
        return Err(PyError::new("TypeError", "unsupported input shape"));
    }
    let mut out = text::normalize(text_in);
    for t in terms {
        let src = format!(
            r"(?:\b(?:the|a|an)\s+)?\w*(?:{})\w*",
            canary::term_pattern_source(t)
        );
        out = compile(rules, &src, true)?.sub(&out, replacement);
    }
    for (src, icase) in PUBLIC_PATTERNS {
        out = compile(rules, src, *icase)?.sub(&out, "");
    }
    out = text::split(&out)
        .into_iter()
        .filter(|tok| !allow::identifier_shaped(tok))
        .collect::<Vec<_>>()
        .join(" ");
    out = compile(rules, r"\(\s*\)|\[\s*\]", false)?.sub(&out, "");
    let spaces = compile(rules, r"\s+", false)?;
    out = spaces.sub(&out, " ");
    let dangling = compile(rules, DANGLING, true)?;
    let empty_sep = compile(rules, r"\s*([;,:])\s*(?=[;,.:]|$)", false)?;
    let doubled = compile(rules, r"\b(and|or)\s+(and|or)\b", true)?;
    let before_punct = compile(rules, r"\s+([.,;:])", false)?;
    let dots = compile(rules, r"^[.]+|(?<=[\s;,:])[.]+$", false)?;
    for _ in 0..6 {
        let before = out.clone();
        out = dangling.sub(&out, "");
        out = empty_sep.sub(&out, "");
        out = doubled.sub_with(&out, &|t, caps| pyre::group(t, caps, 1));
        let joined = spaces.sub(&out, " ");
        let tight = before_punct.sub_with(&joined, &|t, caps| pyre::group(t, caps, 1));
        out = text::strip_chars(&tight, " ;,-:").to_owned();
        out = text::strip(&dots.sub(&out, "")).to_owned();
        if out == before {
            break;
        }
    }
    Ok(out)
}
