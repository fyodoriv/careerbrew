//! rust-01 dependency vectors: the Rust side of `migration verify --step
//! rust-01`. Each vector set maps one vector (an op and an input) to an
//! output with maintained crates; the Python driver
//! (`xtask/tests/fixtures/depvec/driver.py`) does the same with careerbrew's
//! own code, and verify compares the two per vector.
//!
//! To add a set: a module here, one line in `SETS`, one line in the driver's
//! `SETS`, and a fixture `xtask/tests/fixtures/depvec/<set>.json`.
pub mod calendar;
pub mod canon;
pub mod code_parse;
pub mod fernet;
pub mod json_yaml;

use serde_json::{json, Value};

/// A vector set: its name and the function that runs one op on one input.
pub type Run = fn(&str, &Value) -> Result<Value, String>;

/// The registry: one line per vector set.
pub const SETS: &[(&str, Run)] = &[("calendar", calendar::run), ("json-yaml", json_yaml::run)];
pub const SETS: &[(&str, Run)] = &[
    ("json-yaml", json_yaml::run),
    ("code-parse", code_parse::run),
];
pub const SETS: &[(&str, Run)] = &[("fernet", fernet::run), ("json-yaml", json_yaml::run)];

/// Runs every vector of a fixture. A vector that fails gives `{"error":
/// true}`; the reason stays out of the output, since only the outcome is
/// compared.
pub fn run(set: &str, fixture: &Value) -> Result<Value, String> {
    let (_, run) = SETS
        .iter()
        .find(|(name, _)| *name == set)
        .ok_or("unknown vector set")?;
    let vectors = fixture["vectors"]
        .as_array()
        .ok_or("fixture has no vectors")?;
    let results = vectors
        .iter()
        .map(|vector| {
            let output = match vector["op"].as_str() {
                Some(op) => match run(op, &vector["input"]) {
                    Ok(value) => json!({"ok": value}),
                    Err(_) => json!({"error": true}),
                },
                None => json!({"error": true}),
            };
            json!({"id": vector["id"], "output": output})
        })
        .collect::<Vec<_>>();
    Ok(json!({"set": set, "results": results}))
}
