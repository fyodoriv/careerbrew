//! Synthetic rust-22 component cases, mirroring `tests/oracles/rust22.py`
//! over `tests/fixtures/rust22/cases.json`. No production route selects it.

use crate::runner::{self, Language};
use crate::{compare, decide, plant, py};
use regex::Regex;
use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};

pub const CASES: &[&str] = &[
    "decisions",
    "comparison-modes",
    "grading",
    "child-lifecycle",
    "scaling-runs",
    "reference-validation",
];
const FIXTURE: &str = "tests/fixtures/rust22/cases.json";

fn typed(value: &Value) -> Value {
    match value {
        Value::Number(number) if py::is_float(number) => {
            json!({"float": py::float_repr(py::number_f64(number))})
        }
        Value::Array(items) => Value::Array(items.iter().map(typed).collect()),
        Value::Object(items) => Value::Object(
            items
                .iter()
                .map(|(key, item)| (key.clone(), typed(item)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn timed(value: &Value, slope_text: &Regex) -> Value {
    match value {
        Value::Array(items) => {
            Value::Array(items.iter().map(|item| timed(item, slope_text)).collect())
        }
        Value::Object(items) => {
            let mut out = Map::new();
            for (key, item) in items {
                let shown = match (key.as_str(), item) {
                    ("ms", _) => json!("<timed>"),
                    ("runs", Value::Array(rows)) => json!(rows.len()),
                    ("slope", Value::Null) => Value::Null,
                    ("slope", _) => json!("<timed>"),
                    _ => timed(item, slope_text),
                };
                out.insert(key.clone(), shown);
            }
            Value::Object(out)
        }
        Value::String(text) => Value::String(
            slope_text
                .replace_all(text, "growth n^<timed>")
                .into_owned(),
        ),
        other => other.clone(),
    }
}

fn language() -> Result<Language, String> {
    let program = match std::env::var_os("CAREERBREW_RUST22_RUNNER") {
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => std::env::current_exe()
            .map_err(|_| "cannot find the component")?
            .with_file_name("exercise-fixture-runner"),
    };
    if !program.is_file() {
        return Err("the synthetic exercise runner is not built".into());
    }
    Ok(Language {
        program,
        args: vec!["-I".into(), "-c".into(), "synthetic-harness".into()],
    })
}

fn ints(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

fn decisions(data: &Value) -> Result<Value, String> {
    let slopes: Map<String, Value> = decide::COMPLEXITY_SLOPE
        .iter()
        .map(|(name, limit)| ((*name).to_owned(), py::float_value(*limit)))
        .collect();
    let sizes: Vec<Value> = data["scale_sizes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|sizes| json!(decide::scale_sizes(&ints(sizes))))
        .collect();
    let fitted: Vec<Value> = data["slopes"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|points| {
            let points: Vec<(f64, f64)> = points
                .as_array()
                .into_iter()
                .flatten()
                .map(|pair| {
                    let n = pair[0].as_number().map_or(f64::NAN, py::number_f64);
                    let ms = pair[1].as_number().map_or(f64::NAN, py::number_f64);
                    (n, ms)
                })
                .collect();
            py::float_value(decide::growth_slope(&points))
        })
        .collect();
    let decided: Vec<Value> = data["complexity"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|row| {
            let rows = row["rows"].as_array().cloned().unwrap_or_default();
            decide::complexity(Some(&row["spec"]), &rows).unwrap_or(Value::Null)
        })
        .collect();
    Ok(json!({
        "limits": {
            "cpu_seconds": decide::CPU_SECONDS,
            "memory_bytes": decide::MEMORY_BYTES,
            "wall_seconds": decide::WALL_SECONDS,
            "scale_wall_seconds": decide::SCALE_WALL_SECONDS,
            "scale_cpu_seconds": decide::SCALE_CPU_SECONDS,
            "scale_repeats": decide::SCALE_REPEATS,
            "complexity_slope": slopes,
        },
        "scale_sizes": sizes,
        "slopes": fitted,
        "complexity": decided,
    }))
}

fn comparisons(data: &Value) -> Result<Value, String> {
    Ok(Value::Array(
        data["compare"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                match compare::same(&row["got"], &row["exp"], row["mode"].as_str().unwrap_or("")) {
                    Ok(flag) => Value::Bool(flag),
                    Err(error) => json!({"error": error.0}),
                }
            })
            .collect(),
    ))
}

fn grading(data: &Value) -> Result<Value, String> {
    Ok(Value::Array(
        data["grade"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|row| {
                let count = row["tests"].as_u64().unwrap_or(0) as usize;
                let ex = json!({"tests": vec![json!({}); count], "scale": row["scale"]});
                let style: Vec<String> = row["style"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|item| item.as_str().map(str::to_owned))
                    .collect();
                decide::grade(&ex, &row["outcome"], &style)
            })
            .collect(),
    ))
}

fn optional_u64(value: &Value) -> Option<u64> {
    value.as_u64()
}

fn tests_payload(ex: &Value, code: &str) -> Value {
    json!({
        "code": code,
        "function": ex["function"],
        "tests": ex["tests"],
        "compare": ex.get("compare").cloned().unwrap_or_else(|| json!("exact")),
        "scale": null,
    })
}

fn child_lifecycle(data: &Value, slope_text: &Regex) -> Result<Value, String> {
    let language = language()?;
    let ex = &data["exercises"]["linear"];
    let mut out = Vec::new();
    for row in data["child"].as_array().into_iter().flatten() {
        let code = row["code"].as_str().unwrap_or_default();
        let result = runner::child(
            &language,
            &tests_payload(ex, code),
            optional_u64(&row["wall"]),
            optional_u64(&row["cpu"]),
        )?;
        out.push(json!({"label": row["label"], "result": timed(&result, slope_text)}));
    }
    Ok(Value::Array(out))
}

fn runs(data: &Value, slope_text: &Regex) -> Result<Value, String> {
    let language = language()?;
    let mut out = Vec::new();
    for row in data["runs"].as_array().into_iter().flatten() {
        let name = row["exercise"].as_str().unwrap_or_default();
        let code = row["code"].as_str().unwrap_or_default();
        let with_scale = row
            .get("with_scale")
            .and_then(Value::as_bool)
            .unwrap_or(true);
        let result = runner::run(&language, &data["exercises"][name], code, with_scale)?;
        out.push(json!({"exercise": name, "code": code, "result": timed(&result, slope_text)}));
    }
    Ok(Value::Array(out))
}

fn validation(data: &Value, slope_text: &Regex) -> Result<Value, String> {
    let language = language()?;
    let mut out = Vec::new();
    for name in data["validate"].as_array().into_iter().flatten() {
        let name = name.as_str().unwrap_or_default();
        let problems = runner::validate(&language, &data["exercises"][name])?;
        out.push(json!({"exercise": name, "problems": timed(&json!(problems), slope_text)}));
    }
    Ok(Value::Array(out))
}

fn apply_plant() -> Result<(), String> {
    match std::env::var("CAREERBREW_RUST22_PLANT").as_deref() {
        Ok("accept-wrong-class") => plant::accept_wrong_class(),
        Ok("extra") => {
            let home = std::env::var_os("CAREERBREW_HOME").ok_or("no synthetic home")?;
            std::fs::write(Path::new(&home).join("rust22-planted.txt"), "planted\n")
                .map_err(|_| "cannot write the planted file")?;
        }
        _ => {}
    }
    Ok(())
}

/// Run every case (or one) and return the report line.
pub fn run_all(root: &Path, selected: Option<&str>) -> Result<String, String> {
    apply_plant()?;
    let text =
        std::fs::read_to_string(root.join(FIXTURE)).map_err(|_| "cannot read the rust-22 cases")?;
    let data: Value = serde_json::from_str(&text).map_err(|_| "cannot parse the rust-22 cases")?;
    let slope_text = Regex::new(r"growth n\^-?[0-9.e+-]+").map_err(|_| "bad pattern")?;
    let mut cases = Vec::new();
    for name in CASES {
        if selected.is_some_and(|case| case != *name) {
            continue;
        }
        let value = match *name {
            "decisions" => decisions(&data),
            "comparison-modes" => comparisons(&data),
            "grading" => grading(&data),
            "child-lifecycle" => child_lifecycle(&data, &slope_text),
            "scaling-runs" => runs(&data, &slope_text),
            _ => validation(&data, &slope_text),
        };
        let value = value.unwrap_or_else(|_| json!({"error": "RuntimeError"}));
        cases.push(json!({"id": name, "result": typed(&value)}));
    }
    Ok(py::dumps_with(&json!({"cases": cases}), true, ",", ":") + "\n")
}
