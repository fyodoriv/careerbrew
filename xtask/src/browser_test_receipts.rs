//! rust-27b slice 4: native Playwright discovery/execution identity checks.
//! This is a component port of `scripts/browser-test-receipts.py`; US-72 still
//! calls the Python oracle until a later measured activation change.

use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

pub type Identities = BTreeMap<(String, String), usize>;

fn list_or_empty(value: Option<&Value>) -> Result<&[Value], ()> {
    match value {
        None => Ok(&[]),
        Some(value) => value.as_array().map(Vec::as_slice).ok_or(()),
    }
}

/// Python's `retry != 0` accepts numeric zero and `False` as equal to zero.
fn python_zero(value: &Value) -> bool {
    value == false || value.as_f64().is_some_and(|number| number == 0.0)
}

pub fn identities(report: &Value, executed: bool) -> Result<Identities, ()> {
    let root = report.as_object().ok_or(())?;
    if !root
        .get("errors")
        .and_then(Value::as_array)
        .is_some_and(Vec::is_empty)
    {
        return Err(());
    }
    let suites = root.get("suites").and_then(Value::as_array).ok_or(())?;
    let mut found = Identities::new();
    let mut pending: Vec<&Value> = suites.iter().collect();
    while let Some(suite) = pending.pop() {
        if !suite.is_object() {
            return Err(());
        }
        let specs = list_or_empty(suite.get("specs"))?;
        let children = list_or_empty(suite.get("suites"))?;
        for spec in specs {
            let id = spec.get("id").and_then(Value::as_str).ok_or(())?;
            if id.is_empty() {
                return Err(());
            }
            let tests = spec
                .get("tests")
                .and_then(Value::as_array)
                .filter(|tests| !tests.is_empty())
                .ok_or(())?;
            for test in tests {
                if !test.is_object()
                    || test.get("expectedStatus").and_then(Value::as_str) != Some("passed")
                {
                    return Err(());
                }
                let project = test.get("projectId").and_then(Value::as_str).ok_or(())?;
                if executed {
                    let results = test.get("results").and_then(Value::as_array).ok_or(())?;
                    let [result] = results.as_slice() else {
                        return Err(());
                    };
                    if test.get("status").and_then(Value::as_str) != Some("expected")
                        || !result.is_object()
                        || result.get("status").and_then(Value::as_str) != Some("passed")
                        || result.get("retry").is_none_or(|retry| !python_zero(retry))
                    {
                        return Err(());
                    }
                }
                let count = found
                    .entry((id.to_owned(), project.to_owned()))
                    .or_insert(0);
                *count += 1;
            }
        }
        pending.extend(children.iter());
    }
    if found.is_empty() || found.values().any(|count| *count != 1) {
        return Err(());
    }
    if found.len() > 10_000
        || found
            .keys()
            .any(|(spec, project)| spec.chars().count() > 256 || project.chars().count() > 256)
    {
        return Err(());
    }
    Ok(found)
}

fn ascii_json_string(text: &str) -> String {
    let encoded = serde_json::to_string(text).expect("serializing a string cannot fail");
    let mut out = String::new();
    for ch in encoded.chars() {
        if ch == '\x7f' || !ch.is_ascii() {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// Match json.dump's default compact-object spacing and ensure_ascii output.
fn receipt_bytes(identities: &Identities) -> String {
    let rows = identities
        .keys()
        .map(|(spec, project)| {
            format!(
                "[{}, {}]",
                ascii_json_string(spec),
                ascii_json_string(project)
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!("{{\"version\": 1, \"identities\": [{rows}]}}")
}

pub fn write_receipt(destination: &Path, identities: &Identities) -> Result<(), ()> {
    let parent = destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut temporary = tempfile::Builder::new()
        .prefix(".browser-receipt-")
        .tempfile_in(parent)
        .map_err(|_| ())?;
    temporary
        .as_file_mut()
        .write_all(receipt_bytes(identities).as_bytes())
        .map_err(|_| ())?;
    std::fs::hard_link(temporary.path(), destination).map_err(|_| ())?;
    temporary.close().map_err(|_| ())
}

pub fn command(args: &[OsString]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("browser test receipts: need discovery and execution reports");
        return ExitCode::FAILURE;
    }
    let result = (|| {
        let discovery: Value =
            serde_json::from_str(&std::fs::read_to_string(Path::new(&args[0])).map_err(|_| ())?)
                .map_err(|_| ())?;
        let execution: Value =
            serde_json::from_str(&std::fs::read_to_string(Path::new(&args[1])).map_err(|_| ())?)
                .map_err(|_| ())?;
        let identities = identities(&discovery, false)?;
        if identities != self::identities(&execution, true)? {
            return Err(());
        }
        if let Some(destination) =
            std::env::var_os("CAREERBREW_BROWSER_RECEIPT").filter(|value| !value.is_empty())
        {
            write_receipt(Path::new(&destination), &identities)?;
        }
        Ok(identities)
    })();
    if result.is_err() {
        eprintln!("browser test receipts: incomplete or mismatched reports");
        return ExitCode::FAILURE;
    }
    println!("browser test receipts: all discovered tests passed once");
    ExitCode::SUCCESS
}
