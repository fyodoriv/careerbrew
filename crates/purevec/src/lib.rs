//! The rust-02 pure value vector candidate: maps each synthetic vector of a
//! set to an output, in the typed value form of `crates/depvec/src/canon.rs`.
//! Sets: `paths` and `settings`. A set is one module and one
//! line in [`SETS`].
use serde_json::{json, Value};

mod paths;
mod settings;

/// One vector set: its name and the function that maps one vector's `op`
/// and `input` to an output value.
type Set = (&'static str, fn(&str, &Value) -> Result<Value, String>);

/// The registry: one line per vector set, sorted by name, so parallel set
/// pull requests merge cleanly.
#[rustfmt::skip]
pub const SETS: &[Set] = &[
    ("paths", paths::map),
    ("settings", settings::run),
];

/// Runs every vector of `fixture` through the named set and returns
/// `{"set", "results": [{"id", "output"}]}`. An unknown set is refused.
pub fn run(set: &str, fixture: &Value) -> Result<Value, String> {
    let (_, map) = SETS
        .iter()
        .find(|(name, _)| *name == set)
        .ok_or("unknown vector set")?;
    let vectors = fixture["vectors"].as_array().ok_or("no vectors")?;
    let results: Vec<Value> = vectors
        .iter()
        .map(|vector| {
            let output = match map(vector["op"].as_str().unwrap_or(""), &vector["input"]) {
                Ok(value) => json!({"ok": value}),
                Err(_) => json!({"error": true}),
            };
            json!({"id": vector["id"], "output": output})
        })
        .collect();
    Ok(json!({"set": set, "results": results}))
}

/// The `--all` answer: the registered set names, for `migration baseline`
/// and `migration measure`, which time one whole process.
pub fn all() -> Value {
    json!({"sets": SETS.iter().map(|(name, _)| *name).collect::<Vec<_>>()})
}
