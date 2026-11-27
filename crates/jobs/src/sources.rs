//! `careerbrew.jobs.sources`: public ATS boards normalized to one Posting shape.

use crate::{criteria, float, get_text, int, py_str, sub, truthy, JobsError, Result};
use careerbrew_tutor::{read_json, read_yaml, read_yaml_abs};
use regex::{Captures, Regex};
use serde_json::{json, Map, Value};
use std::path::Path;
use std::sync::OnceLock;

pub const ENDPOINTS: &[(&str, &str)] = &[
    (
        "greenhouse",
        "https://boards-api.greenhouse.io/v1/boards/{board}/jobs?content=true",
    ),
    (
        "lever",
        "https://api.lever.co/v0/postings/{board}?mode=json",
    ),
    (
        "ashby",
        "https://api.ashbyhq.com/posting-api/job-board/{board}?includeCompensation=true",
    ),
];
/// `platforms.KINDS`: the job platforms, ported in a later rust-19 slice.
pub const PLATFORM_KINDS: &[&str] = &["adzuna", "remoteok", "remotive", "himalayas", "hn", "wwr"];
const SENIORITY: &[(&str, i64)] = &[
    ("principal", 5),
    ("staff", 4),
    ("lead", 4),
    ("senior", 3),
    ("sr.", 3),
    ("sr ", 3),
    ("junior", 1),
    ("intern", 0),
];
pub const REGIONS: &[(&str, &[&str])] = &[
    (
        "us",
        &[
            "us only",
            "usa only",
            "united states only",
            "us-only",
            "remote - us",
            "remote, us",
            "remote (us",
            "(usa)",
            "u.s. only",
        ],
    ),
    (
        "americas",
        &[
            "americas",
            "north america",
            "latam",
            "na timezones",
            "est",
            "eastern time",
        ],
    ),
    ("europe", &["europe", "emea", "uk only", "eu only", "cet"]),
    ("asia", &["apac", "asia", "india"]),
    ("global", &["worldwide", "anywhere", "global", "remote"]),
];

/// The user's own region labels with their words, from `criteria::custom_regions`.
pub type CustomRegions = Vec<(String, Vec<String>)>;
/// `skill_vocab()`: skill name to its lower-case aliases (the name last).
pub type Vocab = Vec<(String, Vec<String>)>;

#[derive(Clone, Debug, PartialEq)]
pub struct Posting {
    pub id: String,
    pub company: String,
    pub title: String,
    pub location: String,
    pub remote: bool,
    pub url: String,
    pub description: String,
    pub salary_min: Option<i64>,
    pub salary_max: Option<i64>,
    pub seniority: i64,
    pub skills: Vec<String>,
    pub origin: String,
    pub tier_hint: Option<String>,
    pub region: String,
    pub source_name: String,
}

impl Posting {
    /// The dataclass with its defaults for the optional fields.
    pub fn new(
        id: &str,
        company: &str,
        title: &str,
        location: &str,
        remote: bool,
        url: &str,
        description: &str,
    ) -> Self {
        Self {
            id: id.into(),
            company: company.into(),
            title: title.into(),
            location: location.into(),
            remote,
            url: url.into(),
            description: description.into(),
            salary_min: None,
            salary_max: None,
            seniority: 2,
            skills: Vec::new(),
            origin: "base".into(),
            tier_hint: None,
            region: String::new(),
            source_name: String::new(),
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "company": self.company,
            "title": self.title,
            "location": self.location,
            "remote": self.remote,
            "url": self.url,
            "description": self.description,
            "salary_min": self.salary_min,
            "salary_max": self.salary_max,
            "seniority": self.seniority,
            "skills": self.skills,
            "origin": self.origin,
            "tier_hint": self.tier_hint,
            "region": self.region,
            "source_name": self.source_name,
        })
    }

    pub fn from_json(value: &Value) -> Result<Self> {
        let opt_int = |key: &str| -> Result<Option<i64>> {
            match value.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(other) => int(other).map(Some),
            }
        };
        let required = |key: &str| -> Result<String> {
            value
                .get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    JobsError::new(
                        "TypeError",
                        format!("Posting.__init__() missing required argument: '{key}'"),
                    )
                })
        };
        Ok(Self {
            id: required("id")?,
            company: required("company")?,
            title: required("title")?,
            location: required("location")?,
            remote: truthy(value.get("remote")),
            url: required("url")?,
            description: required("description")?,
            salary_min: opt_int("salary_min")?,
            salary_max: opt_int("salary_max")?,
            seniority: value.get("seniority").map(int).transpose()?.unwrap_or(2),
            skills: careerbrew_tutor::strings(value.get("skills")),
            origin: value
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or("base")
                .into(),
            tier_hint: value
                .get("tier_hint")
                .and_then(Value::as_str)
                .map(str::to_owned),
            region: get_text(value, "region"),
            source_name: get_text(value, "source_name"),
        })
    }
}

fn regex(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("a fixed pattern compiles"))
}

/// `html.unescape` for the named and numeric references board APIs use.
/// rust-followup: the full HTML5 entity table and references without `;`.
pub fn unescape(text: &str) -> String {
    static ENTITY: OnceLock<Regex> = OnceLock::new();
    if !text.contains('&') {
        return text.to_owned();
    }
    let entity = regex(
        &ENTITY,
        r"&(#[0-9]+;|#[xX][0-9a-fA-F]+;|[A-Za-z][A-Za-z0-9]*;)",
    );
    entity
        .replace_all(text, |captures: &Captures| {
            let body = &captures[1];
            let inner = &body[..body.len() - 1];
            let code = if let Some(hex) = inner
                .strip_prefix("#x")
                .or_else(|| inner.strip_prefix("#X"))
            {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(decimal) = inner.strip_prefix('#') {
                decimal.parse::<u32>().ok()
            } else {
                None
            };
            if let Some(code) = code {
                return char::from_u32(code)
                    .filter(|c| *c != '\0')
                    .unwrap_or('\u{fffd}')
                    .to_string();
            }
            match inner {
                "amp" => "&".into(),
                "lt" => "<".into(),
                "gt" => ">".into(),
                "quot" => "\"".into(),
                "apos" => "'".into(),
                "nbsp" => "\u{a0}".into(),
                _ => captures[0].to_owned(),
            }
        })
        .into_owned()
}

pub fn strip_html(text: &str) -> String {
    static TAG: OnceLock<Regex> = OnceLock::new();
    static SPACE: OnceLock<Regex> = OnceLock::new();
    let once = unescape(text);
    let tagless = regex(&TAG, r"<[^>]+>").replace_all(&once, " ");
    regex(&SPACE, r"\s+")
        .replace_all(&unescape(&tagless), " ")
        .trim()
        .to_owned()
}

pub fn parse_salary(text: &str) -> (Option<i64>, Option<i64>) {
    static SALARY: OnceLock<Regex> = OnceLock::new();
    let salary = regex(
        &SALARY,
        r"(?i)\$\s?(\d{2,3})(?:,(\d{3})|k)\s*(?:-|–|to)\s*\$?\s?(\d{2,3})(?:,(\d{3})|k)",
    );
    let Some(found) = salary.captures(text) else {
        return (None, None);
    };
    let num = |a: usize, b: usize| -> i64 {
        let high = found[a].parse::<i64>().unwrap_or(0) * 1000;
        found
            .get(b)
            .map_or(high, |low| high + low.as_str().parse::<i64>().unwrap_or(0))
    };
    (Some(num(1, 2)), Some(num(3, 4)))
}

pub fn seniority_of(title: &str) -> i64 {
    let low = format!("{} ", title.to_lowercase());
    SENIORITY
        .iter()
        .find(|(word, _)| low.contains(word))
        .map_or(2, |(_, level)| *level)
}

pub fn skill_vocab(repo: &Path) -> Result<Vocab> {
    let data = read_yaml_abs(&repo.join("knowledge/skills.yaml"), json!({}))?;
    let mut vocab = Vec::new();
    for (name, aliases) in data
        .get("skills")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let mut words = careerbrew_tutor::strings(Some(aliases))
            .iter()
            .map(|alias| alias.to_lowercase())
            .collect::<Vec<_>>();
        words.push(name.to_lowercase());
        vocab.push((name.clone(), words));
    }
    Ok(vocab)
}

/// One whole-word occurrence of `needle`: `is_word` is false on both sides.
pub(crate) fn bounded(hay: &str, needle: &str, is_word: fn(char) -> bool) -> bool {
    bounded_after(hay, needle, is_word, |_| true)
}

fn bounded_after(
    hay: &str,
    needle: &str,
    is_word: fn(char) -> bool,
    allowed: impl Fn(&str) -> bool,
) -> bool {
    let mut start = 0;
    loop {
        let Some(offset) = hay[start..].find(needle) else {
            return false;
        };
        let at = start + offset;
        let end = at + needle.len();
        let before = hay[..at].chars().next_back();
        let after = hay[end..].chars().next();
        if !before.is_some_and(is_word) && !after.is_some_and(is_word) && allowed(&hay[..at]) {
            return true;
        }
        match hay[at..].chars().next() {
            Some(c) => start = at + c.len_utf8(),
            None => return false,
        }
    }
}

pub(crate) fn ascii_word(c: char) -> bool {
    c.is_ascii_lowercase() || c.is_ascii_digit()
}

fn unicode_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

pub fn extract_skills(text: &str, vocab: &Vocab) -> Vec<String> {
    let low = format!(" {} ", text.to_lowercase());
    let mut found = vocab
        .iter()
        .filter(|(_, aliases)| aliases.iter().any(|a| bounded(&low, a, ascii_word)))
        .map(|(skill, _)| skill.clone())
        .collect::<Vec<_>>();
    found.sort();
    found
}

/// `_US_ZONES`: a long US zone name, never "Australian Central Time".
fn us_zone(low: &str) -> bool {
    ["central time", "mountain time", "pacific time"]
        .iter()
        .any(|zone| {
            bounded_after(low, zone, unicode_word, |before| {
                !before.ends_with("australian ")
            })
        })
}

pub fn region_of(text: &str, custom: &CustomRegions) -> String {
    let low = text
        .to_lowercase()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    for (label, words) in custom {
        if words.iter().any(|w| bounded(&low, w, unicode_word)) {
            return label.clone();
        }
    }
    for (index, (label, words)) in REGIONS.iter().enumerate() {
        if index == REGIONS.len() - 1 && us_zone(&low) {
            return "americas".into();
        }
        if words.iter().any(|w| bounded(&low, w, unicode_word)) {
            return (*label).into();
        }
    }
    String::new()
}

pub fn configured_sources(root: &Path) -> Result<Vec<Value>> {
    // rust-followup: plugin `job_sources` resources join the user's list here.
    let mine = read_yaml(root, "jobs/sources.yaml", json!({}))?;
    let mut out = Vec::new();
    for source in mine
        .get("sources")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let mut row = source.as_object().cloned().unwrap_or_default();
        row.insert("origin".into(), json!("user"));
        out.push(Value::Object(row));
    }
    Ok(out)
}

pub fn fetch_json(url: &str) -> Result<Value> {
    let Some(path) = url.strip_prefix("file://") else {
        // rust-followup: the cached HTTPS fetch (`store.cached`, six hours).
        return Err(JobsError::new(
            "OSError",
            "the synthetic component reads only file:// board fixtures",
        ));
    };
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn opt_number(value: Option<&Value>) -> Result<Option<f64>> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(other) => float(other).map(Some),
    }
}

fn object_or_empty(value: Option<&Value>) -> Value {
    match value {
        Some(found) if truthy(Some(found)) => found.clone(),
        _ => json!({}),
    }
}

#[allow(clippy::too_many_arguments)]
fn make(
    id: String,
    company: &str,
    title: &str,
    location: &str,
    url: &str,
    description: &str,
    salary: (Option<f64>, Option<f64>),
    vocab: &Vocab,
    origin: &str,
    tier_hint: Option<&str>,
    workplace: &str,
    regions: &CustomRegions,
) -> Posting {
    let (low, high) = if salary.0.is_none() {
        let (low, high) = parse_salary(description);
        (low.map(|v| v as f64), high.map(|v| v as f64))
    } else {
        salary
    };
    let some = |value: Option<f64>| value.filter(|v| *v != 0.0).map(|v| v.trunc() as i64);
    let mut posting = Posting::new(
        &id,
        company,
        title,
        location,
        format!("{location} {workplace} {title}")
            .to_lowercase()
            .contains("remote"),
        url,
        &careerbrew_tutor::chars(description, 6000),
    );
    posting.salary_min = some(low);
    posting.salary_max = some(high);
    posting.seniority = seniority_of(title);
    posting.skills = extract_skills(&format!("{title} {description}"), vocab);
    posting.origin = origin.into();
    posting.tier_hint = tier_hint.map(str::to_owned);
    posting.region = region_of(&format!("{location} {workplace}"), regions);
    posting
}

pub fn normalize(
    repo: &Path,
    kind: &str,
    company: &str,
    raw: &Value,
    origin: &str,
    tier_hint: Option<&str>,
    regions: &CustomRegions,
) -> Result<Vec<Posting>> {
    let vocab = skill_vocab(repo)?;
    let mut out = Vec::new();
    let rows = |value: Option<&Value>| value.and_then(Value::as_array).cloned().unwrap_or_default();
    match kind {
        "greenhouse" => {
            for job in rows(raw.get("jobs")) {
                let description = strip_html(&get_text(&job, "content"));
                let location = get_text(&object_or_empty(job.get("location")), "name");
                out.push(make(
                    format!("gh:{company}:{}", py_str(job.get("id"))),
                    company,
                    &get_text(&job, "title"),
                    &location,
                    &get_text(&job, "absolute_url"),
                    &description,
                    (None, None),
                    &vocab,
                    origin,
                    tier_hint,
                    "",
                    regions,
                ));
            }
        }
        "lever" => {
            for job in rows(Some(raw)) {
                let plain = [job.get("descriptionPlain"), job.get("description")]
                    .into_iter()
                    .flatten()
                    .find(|value| truthy(Some(value)))
                    .map(|value| py_str(Some(value)))
                    .unwrap_or_default();
                let lists = rows(job.get("lists"))
                    .iter()
                    .map(|item| strip_html(&get_text(item, "content")))
                    .collect::<Vec<_>>()
                    .join(" ");
                let description = strip_html(&format!("{plain} {lists}"));
                let categories = object_or_empty(job.get("categories"));
                let range = object_or_empty(job.get("salaryRange"));
                out.push(make(
                    format!("lever:{company}:{}", py_str(job.get("id"))),
                    company,
                    &get_text(&job, "text"),
                    &get_text(&categories, "location"),
                    &get_text(&job, "hostedUrl"),
                    &description,
                    (opt_number(range.get("min"))?, opt_number(range.get("max"))?),
                    &vocab,
                    origin,
                    tier_hint,
                    &get_text(&job, "workplaceType"),
                    regions,
                ));
            }
        }
        "ashby" => {
            for job in rows(raw.get("jobs")) {
                let html = [job.get("descriptionHtml"), job.get("descriptionPlain")]
                    .into_iter()
                    .flatten()
                    .find(|value| truthy(Some(value)))
                    .map(|value| py_str(Some(value)))
                    .unwrap_or_default();
                let description = strip_html(&html);
                let compensation = object_or_empty(job.get("compensation"));
                let (mut low, mut high) = (None, None);
                for part in rows(compensation.get("summaryComponents")) {
                    if part.get("compensationType").and_then(Value::as_str) == Some("Salary") {
                        low = opt_number(part.get("minValue"))?;
                        high = opt_number(part.get("maxValue"))?;
                    }
                }
                let workplace = if truthy(job.get("isRemote")) {
                    "remote"
                } else {
                    ""
                };
                out.push(make(
                    format!("ashby:{company}:{}", py_str(job.get("id"))),
                    company,
                    &get_text(&job, "title"),
                    &get_text(&job, "location"),
                    &get_text(&job, "jobUrl"),
                    &description,
                    (low, high),
                    &vocab,
                    origin,
                    tier_hint,
                    workplace,
                    regions,
                ));
            }
        }
        _ => {
            let mut kinds = ENDPOINTS.iter().map(|(k, _)| *k).collect::<Vec<_>>();
            kinds.sort();
            let listed = kinds
                .iter()
                .map(|k| careerbrew_policy::approvals::py_repr(k))
                .collect::<Vec<_>>()
                .join(", ");
            return Err(JobsError::value(format!(
                "unknown source kind {}; use one of [{listed}]",
                careerbrew_policy::approvals::py_repr(kind)
            )));
        }
    }
    Ok(out)
}

fn title_case(kind: &str) -> String {
    let mut chars = kind.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + &chars.as_str().to_lowercase()
    })
}

/// `scan(no_cache=True)` over the ATS boards. rust-followup: platform kinds.
pub fn scan(root: &Path, repo: &Path) -> Result<Vec<Posting>> {
    let regions = criteria::custom_regions(&criteria::load(root)?)?;
    let mut postings = load(root)?
        .into_iter()
        .filter(|p| p.source_name.starts_with("LinkedIn"))
        .collect::<Vec<_>>();
    for source in configured_sources(root)? {
        let kind = get_text(&source, "kind");
        if PLATFORM_KINDS.contains(&kind.as_str()) {
            return Err(JobsError::new(
                "NotImplementedError",
                "job platform sources are a later rust-19 slice",
            ));
        }
        let board = source
            .get("board")
            .map(|value| py_str(Some(value)))
            .ok_or_else(|| JobsError::new("KeyError", "'board'"))?;
        let url = match source.get("url").filter(|value| truthy(Some(value))) {
            Some(url) => py_str(Some(url)),
            None => ENDPOINTS
                .iter()
                .find(|(name, _)| *name == kind)
                .map(|(_, endpoint)| endpoint.replace("{board}", &board))
                .ok_or_else(|| {
                    JobsError::new("KeyError", careerbrew_policy::approvals::py_repr(&kind))
                })?,
        };
        let company = match source.get("company") {
            Some(value) => py_str(Some(value)),
            None => board.clone(),
        };
        let raw = fetch_json(&url)?;
        let tier = source.get("tier").and_then(Value::as_str);
        for mut posting in normalize(
            repo,
            &kind,
            &company,
            &raw,
            source
                .get("origin")
                .and_then(Value::as_str)
                .unwrap_or("user"),
            tier,
            &regions,
        )? {
            posting.source_name = title_case(&kind);
            postings.push(posting);
        }
        careerbrew_tutor::emit(
            root,
            "jobs.source",
            vec![
                ("source_kind", careerbrew_vault::canon::Canon::Str(kind)),
                ("status", careerbrew_vault::canon::Canon::Str("ok".into())),
            ],
        )?;
    }
    save(root, &postings)?;
    Ok(postings)
}

pub fn save(root: &Path, postings: &[Posting]) -> Result<()> {
    let path = sub(root, "jobs/postings.json")?;
    let map = postings
        .iter()
        .map(|p| (p.id.clone(), p.to_json()))
        .collect::<Map<_, _>>();
    careerbrew_tutor::write_json_abs(&path, &Value::Object(map))
}

pub fn load(root: &Path) -> Result<Vec<Posting>> {
    sub(root, "jobs/postings.json")?;
    let data = read_json(root, "jobs/postings.json", json!({}))?;
    data.as_object()
        .into_iter()
        .flatten()
        .map(|(_, value)| Posting::from_json(value))
        .collect()
}
