//! The rust-02 pure value vector candidate: maps each synthetic vector of a
//! set to an output, in the typed value form of `crates/depvec/src/canon.rs`.
//! It has no sets yet; the setting, path, clock and hash sets come one per
//! pull request. A set is one line in [`SETS`].
use serde_json::{json, Value};

pub mod hash;

/// One vector set: its name and the function that maps one vector's `op`
/// and `input` to an output value.
type Set = (&'static str, fn(&str, &Value) -> Result<Value, String>);

/// The registry: one line per vector set, sorted by name.
pub const SETS: &[Set] = &[("hash", hash::run)];

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
