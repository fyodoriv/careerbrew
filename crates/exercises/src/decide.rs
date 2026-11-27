//! Limits, scaling sizes, the growth slope, the complexity decision and the
//! grade, as `careerbrew.tutor.exercises` defines them.

use crate::py;
use serde_json::{json, Map, Value};

/// One test run's CPU limit in seconds.
pub const CPU_SECONDS: u64 = 6;
/// The address-space limit (macOS ignores it; the wall limit still holds).
pub const MEMORY_BYTES: u64 = 768 * 1024 * 1024;
/// One test run's wall-clock limit in seconds.
pub const WALL_SECONDS: u64 = 15;
/// The scaling run's wall limit only stops a hang that uses no CPU.
pub const SCALE_WALL_SECONDS: u64 = 600;
/// The scaling run's timed runs share one process, so they share a larger CPU limit.
pub const SCALE_CPU_SECONDS: u64 = 60;
/// Timed runs per input size; the decision uses the fastest.
pub const SCALE_REPEATS: u64 = 5;
/// Maximum fitted growth exponent per expected class (halfway to the next class).
pub const COMPLEXITY_SLOPE: &[(&str, f64)] = &[
    ("O(1)", 0.5),
    ("O(log n)", 0.5),
    ("O(n)", 1.5),
    ("O(n log n)", 1.6),
    ("O(n^2)", 2.5),
];
/// A class the table does not name uses the linear limit.
pub const DEFAULT_SLOPE: f64 = 1.5;
/// Below this last-size time the decision accepts any slope (too small to judge).
pub const SMALL_MS: f64 = 50.0;

pub const REQUIRED: &[&str] = &[
    "id",
    "order",
    "pattern",
    "difficulty",
    "title",
    "statement",
    "function",
    "signature",
    "tests",
    "solution",
    "hints",
    "lesson",
];
pub const PATTERNS: &[&str] = &[
    "arrays and hashing",
    "two pointers",
    "sliding window",
    "stack",
    "binary search",
    "linked list",
    "trees",
    "tries",
    "heap",
    "backtracking",
    "graphs",
    "advanced graphs",
    "1-d dynamic programming",
    "2-d dynamic programming",
    "greedy",
    "intervals",
    "math and geometry",
    "bit manipulation",
];

pub fn slope_limit(expected: &str) -> f64 {
    COMPLEXITY_SLOPE
        .iter()
        .find(|(name, _)| *name == expected)
        .map_or(DEFAULT_SLOPE, |(_, limit)| *limit)
}

/// Add the geometric middle size when only two are given, so the fit uses three points.
pub fn scale_sizes(sizes: &[i64]) -> Vec<i64> {
    if sizes.len() == 2 {
        let middle = ((sizes[0] as f64) * (sizes[1] as f64)).sqrt();
        return vec![sizes[0], round_half_even(middle), sizes[1]];
    }
    sizes.to_vec()
}

fn round_half_even(x: f64) -> i64 {
    let floor = x.floor();
    let diff = x - floor;
    let base = floor as i64;
    if diff > 0.5 || (diff == 0.5 && base % 2 != 0) {
        base + 1
    } else {
        base
    }
}

/// Least-squares slope of log(ms) against log(n): the k in time ~ n^k.
pub fn growth_slope(points: &[(f64, f64)]) -> f64 {
    let xs: Vec<f64> = points.iter().map(|(n, _)| n.ln()).collect();
    let ys: Vec<f64> = points.iter().map(|(_, ms)| ms.max(0.05).ln()).collect();
    let mx = py::fsum(xs.iter().copied()) / xs.len() as f64;
    let my = py::fsum(ys.iter().copied()) / ys.len() as f64;
    let den = py::fsum(xs.iter().map(|x| (x - mx).powi(2)));
    let den = if den == 0.0 { 1.0 } else { den };
    py::fsum(xs.iter().zip(&ys).map(|(x, y)| (x - mx) * (y - my))) / den
}

fn row_number(row: &Value, key: &str) -> f64 {
    row.get(key)
        .and_then(Value::as_number)
        .map_or(f64::NAN, py::number_f64)
}

/// The complexity decision on one scaling run: `None` when there is
/// nothing to judge, else `{expected, slope, ok}`.
pub fn complexity(spec: Option<&Value>, scale: &[Value]) -> Option<Value> {
    let spec = spec.filter(|spec| py::truthy(Some(spec)))?;
    if scale.is_empty() {
        return None;
    }
    let expected = spec.get("expected").cloned().unwrap_or(Value::Null);
    if scale.iter().any(|row| row.get("error").is_some()) {
        return Some(json!({"expected": expected, "slope": null, "ok": false}));
    }
    if scale.len() < 2 {
        return None;
    }
    let points: Vec<(f64, f64)> = scale
        .iter()
        .map(|row| (row_number(row, "n"), row_number(row, "ms")))
        .collect();
    let slope = growth_slope(&points);
    let limit = slope_limit(expected.as_str().unwrap_or(""));
    let last = row_number(&scale[scale.len() - 1], "ms");
    let ok = slope <= limit || last < SMALL_MS;
    Some(json!({"expected": expected, "slope": py::float_value(py::round2(slope)), "ok": ok}))
}

/// The quality score: correctness first, then complexity, then readability.
/// `style` is the readability finding list for correct code.
pub fn grade(ex: &Value, outcome: &Value, style: &[String]) -> Value {
    let tests = outcome
        .get("results")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let passed = tests
        .iter()
        .filter(|test| py::truthy(test.get("ok")))
        .count();
    let total = ex
        .get("tests")
        .and_then(Value::as_array)
        .map_or(0, Vec::len);
    let correct = !py::truthy(outcome.get("error")) && passed == total;
    let scale = outcome
        .get("scale")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let cx = if correct {
        complexity(ex.get("scale"), &scale)
    } else {
        None
    };
    let style: Vec<String> = if correct { style.to_vec() } else { Vec::new() };
    let mut score = 0.0_f64;
    if total > 0 {
        score = 0.7 * passed as f64 / total as f64;
    }
    if correct {
        let cx_ok = cx
            .as_ref()
            .is_none_or(|cx| cx.get("ok") == Some(&Value::Bool(true)));
        score += if cx_ok { 0.2 } else { 0.0 };
        score += if style.is_empty() { 0.1 } else { 0.05 };
    }
    let mut out = Map::new();
    out.insert("passed".into(), json!(passed));
    out.insert("total".into(), json!(total));
    out.insert("correct".into(), json!(correct));
    out.insert("complexity".into(), cx.unwrap_or(Value::Null));
    out.insert("style".into(), json!(style));
    out.insert("score".into(), py::float_value(py::round2(score)));
    Value::Object(out)
}

/// The content checks that need no run: fields, pattern, difficulty and counts.
/// `Err` holds the missing-field problems, which stop every later check.
pub fn structure(ex: &Value) -> Result<Vec<String>, Vec<String>> {
    let missing: Vec<String> = REQUIRED
        .iter()
        .filter(|key| ex.get(**key).is_none())
        .map(|key| format!("missing {key}"))
        .collect();
    if !missing.is_empty() {
        return Err(missing);
    }
    let mut problems = Vec::new();
    let pattern = &ex["pattern"];
    if !pattern
        .as_str()
        .is_some_and(|pattern| PATTERNS.contains(&pattern))
    {
        let shown = pattern
            .as_str()
            .map_or_else(|| py::dumps_sorted(pattern), py::repr_str);
        problems.push(format!("unknown pattern {shown}"));
    }
    if !ex["difficulty"]
        .as_str()
        .is_some_and(|level| ["easy", "medium", "hard"].contains(&level))
    {
        problems.push("difficulty must be easy, medium, or hard".into());
    }
    if ex["tests"].as_array().map_or(0, Vec::len) < 4 {
        problems.push("needs at least 4 tests (include edge cases)".into());
    }
    if ex["hints"].as_array().map_or(0, Vec::len) < 2 {
        problems.push("needs at least 2 hints".into());
    }
    Ok(problems)
}
