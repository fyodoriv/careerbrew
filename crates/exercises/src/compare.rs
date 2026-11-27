//! The harness comparison modes: `exact`, `sorted` (outer order free,
//! inner order exact), `set` (both orders free) and `float` (absolute 1e-6).
//! Any other mode name compares exactly.

use crate::py::{self, PyError};
use serde_json::Value;

/// The harness `same(got, exp, mode)`.
pub fn same(got: &Value, expected: &Value, mode: &str) -> Result<bool, PyError> {
    match (mode, got, expected) {
        ("sorted", Value::Array(got), Value::Array(expected)) => Ok(py::eq(
            &Value::Array(sort_by_dump(got)),
            &Value::Array(sort_by_dump(expected)),
        )),
        ("set", Value::Array(got), Value::Array(expected)) => Ok(norm(got)? == norm(expected)?),
        ("float", _, _) => Ok((py::to_float(got)? - py::to_float(expected)?).abs() < 1e-6),
        _ => Ok(py::eq(got, expected)),
    }
}

fn sort_by_dump(items: &[Value]) -> Vec<Value> {
    let mut keyed: Vec<(String, Value)> = items
        .iter()
        .map(|item| (py::dumps_sorted(item), item.clone()))
        .collect();
    keyed.sort_by(|a, b| a.0.cmp(&b.0));
    keyed.into_iter().map(|(_, item)| item).collect()
}

fn norm(items: &[Value]) -> Result<Vec<String>, PyError> {
    let mut out = items
        .iter()
        .map(|item| match item {
            Value::Array(inner) => Ok(py::dumps_sorted(&Value::Array(py::sorted(inner)?))),
            other => Ok(py::dumps_sorted(other)),
        })
        .collect::<Result<Vec<_>, PyError>>()?;
    out.sort();
    Ok(out)
}
