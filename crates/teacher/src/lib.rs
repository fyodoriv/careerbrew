//! Component-only teacher, goals, practice and mistake tracking (rust-21).
//!
//! No production route selects this crate. It reuses the rust-20 tutor crate
//! for cards, FSRS, mistake records and storage. Steps that are not merged yet
//! sit behind the typed seams in [`seams`]: the exercise runner (rust-22),
//! interview loops (rust-19) and the day re-plan. The synthetic providers
//! there mirror the Python oracle's providers exactly.

pub mod goals;
pub mod seams;
pub mod synthetic;
pub mod teacher;

pub use careerbrew_tutor::{field, text, truthy, Result, TutorError};

use chrono::{DateTime, FixedOffset, NaiveDate, TimeZone, Utc};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The paths one invocation works in: the data root, the public deck folder
/// and the repository root (for `knowledge/teaching.yaml`).
pub struct Ctx {
    pub root: PathBuf,
    pub public: PathBuf,
    pub repo: PathBuf,
    pub seams: seams::Seams,
}

impl Ctx {
    pub fn from_env(repo: &Path, seams: seams::Seams) -> Result<Self> {
        let root = careerbrew_tutor::root()?;
        let public = std::env::var_os("CAREERBREW_PUBLIC_CARDS")
            .map(PathBuf::from)
            .unwrap_or_else(|| repo.join("knowledge/cards"));
        Ok(Self {
            root,
            public,
            repo: repo.to_path_buf(),
            seams,
        })
    }
}

/// `calendar.now()`: the pinned clock keeps its own UTC offset, like Python's
/// `datetime.fromisoformat`; a naive pin takes the settings zone.
pub fn now_local(root: &Path) -> DateTime<FixedOffset> {
    let zone = careerbrew_tutor::local_zone(root);
    let Ok(value) = std::env::var("CAREERBREW_NOW") else {
        return Utc::now().with_timezone(&zone).fixed_offset();
    };
    if let Ok(at) = DateTime::parse_from_rfc3339(&value) {
        return at;
    }
    let naive = chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| chrono::NaiveDateTime::parse_from_str(&value, "%Y-%m-%dT%H:%M"))
        .expect("invalid CAREERBREW_NOW");
    zone.from_local_datetime(&naive)
        .earliest()
        .expect("invalid CAREERBREW_NOW")
        .fixed_offset()
}

/// `calendar.now().date()`.
pub fn today(root: &Path) -> NaiveDate {
    now_local(root).date_naive()
}

/// `calendar.now_utc()`.
pub fn now_utc(root: &Path) -> DateTime<FixedOffset> {
    now_local(root).with_timezone(&Utc).fixed_offset()
}

/// `now_utc().isoformat(timespec="seconds")`.
pub fn stamp(root: &Path) -> String {
    careerbrew_tutor::stamp_at(now_utc(root))
}

/// Python's `round(x)` for a float: ties go to the even integer.
pub fn round_even(value: f64) -> i64 {
    value.round_ties_even() as i64
}

/// Python's `str(value)` for a scalar read from a record.
pub fn py_str(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => "None".into(),
        Some(Value::Bool(true)) => "True".into(),
        Some(Value::Bool(false)) => "False".into(),
        other => text(other),
    }
}

/// Python truthiness of `value or default`: the value when truthy.
pub fn or(value: Option<&Value>) -> Option<&Value> {
    value.filter(|item| truthy(Some(item)))
}

/// `paths.sub(...)`: Python creates the parent folder even for a read.
pub fn sub(root: &Path, relative: &str) -> Result<()> {
    if let Some(parent) = root.join(relative).parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

/// `store.read_json(paths.sub(...), default)`.
pub fn read_json(root: &Path, relative: &str, default: Value) -> Result<Value> {
    sub(root, relative)?;
    careerbrew_tutor::read_json(root, relative, default)
}

/// `store.read_jsonl(paths.sub(...))`.
pub fn read_jsonl(root: &Path, relative: &str) -> Result<Vec<Value>> {
    sub(root, relative)?;
    careerbrew_tutor::read_jsonl(root, relative)
}

/// `mistakes.load()`, whose path goes through `paths.sub`.
pub fn load_mistakes(root: &Path) -> Result<Vec<Value>> {
    sub(root, "tutor/mistakes.jsonl")?;
    careerbrew_tutor::mistakes::load(root)
}

pub fn err(class: &'static str, message: impl Into<String>) -> TutorError {
    TutorError::new(class, message)
}
