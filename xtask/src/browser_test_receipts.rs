//! rust-27b slice 4: native Playwright discovery/execution identity checks.
//! This is a component port of `scripts/browser-test-receipts.py`; US-72 still
//! calls the Python oracle until a later measured activation change.

use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::io::Write;
use std::path::Path;
use std::process::ExitCode;

// CPython's default recursion limit is 1,000. Stay below it and parse on a
// dedicated stack; the lexical depth check rejects hostile inputs iteratively.
const MAX_JSON_DEPTH: usize = 900;
// Hosted debug builds exhausted 16 MiB below this cap; 64 MiB covers depth 900.
const PARSER_STACK_BYTES: usize = 64 * 1024 * 1024;
const SENTINEL: char = '\u{e000}';
const ENCODED_SENTINEL: &str = "\\ue000";
const LITERAL_SENTINEL: &str = "\\ue000\\u0000";
const SURROGATE_TAG_BASE: u32 = 0x10000;
const SURROGATE_TAG_START: u32 = SURROGATE_TAG_BASE + 0xd800;
const SURROGATE_TAG_END: u32 = SURROGATE_TAG_BASE + 0xdfff;

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
struct PythonString(Vec<u32>);

type Identities = BTreeMap<(PythonString, PythonString), usize>;

fn hex_unit(bytes: &[u8], start: usize) -> Option<u16> {
    let digits = bytes.get(start..start + 4)?;
    if !digits.iter().all(|digit| digit.is_ascii_hexdigit()) {
        return None;
    }
    u16::from_str_radix(std::str::from_utf8(digits).ok()?, 16).ok()
}

fn push_lone_surrogate(out: &mut String, unit: u16) {
    out.push_str(ENCODED_SENTINEL);
    out.push(
        char::from_u32(SURROGATE_TAG_BASE + u32::from(unit))
            .expect("a surrogate tag is a valid non-BMP scalar"),
    );
}

/// Protect only string tokens, leaving JSON grammar and valid pairs to serde_json.
/// Original sentinel characters use a distinct tagged literal, so no input string
/// can collide with a lone-surrogate tag. Work and memory are linear in input size.
fn adapt_json(source: &str) -> Result<String, ()> {
    let bytes = source.as_bytes();
    let mut out = String::with_capacity(source.len());
    let mut index = 0;
    let mut depth = 0usize;
    let mut in_string = false;
    while index < bytes.len() {
        if !in_string {
            match bytes[index] {
                b'"' => {
                    in_string = true;
                    out.push('"');
                    index += 1;
                }
                b'[' | b'{' => {
                    depth += 1;
                    if depth > MAX_JSON_DEPTH {
                        return Err(());
                    }
                    out.push(bytes[index] as char);
                    index += 1;
                }
                b']' | b'}' => {
                    depth = depth.checked_sub(1).ok_or(())?;
                    out.push(bytes[index] as char);
                    index += 1;
                }
                _ => {
                    let ch = source[index..].chars().next().ok_or(())?;
                    out.push(ch);
                    index += ch.len_utf8();
                }
            }
            continue;
        }
        match bytes[index] {
            b'"' => {
                in_string = false;
                out.push('"');
                index += 1;
            }
            b'\\' => {
                if bytes.get(index + 1) == Some(&b'u') {
                    if let Some(unit) = hex_unit(bytes, index + 2) {
                        if (0xd800..=0xdbff).contains(&unit)
                            && bytes.get(index + 6) == Some(&b'\\')
                            && bytes.get(index + 7) == Some(&b'u')
                            && hex_unit(bytes, index + 8)
                                .is_some_and(|next| (0xdc00..=0xdfff).contains(&next))
                        {
                            out.push_str(&source[index..index + 12]);
                            index += 12;
                            continue;
                        }
                        if (0xd800..=0xdfff).contains(&unit) {
                            push_lone_surrogate(&mut out, unit);
                        } else if unit == SENTINEL as u16 {
                            out.push_str(LITERAL_SENTINEL);
                        } else {
                            out.push_str(&source[index..index + 6]);
                        }
                        index += 6;
                        continue;
                    }
                }
                out.push('\\');
                index += 1;
                if index < bytes.len() {
                    let ch = source[index..].chars().next().ok_or(())?;
                    out.push(ch);
                    index += ch.len_utf8();
                }
            }
            _ => {
                let ch = source[index..].chars().next().ok_or(())?;
                if ch == SENTINEL {
                    out.push_str(LITERAL_SENTINEL);
                } else {
                    out.push(ch);
                }
                index += ch.len_utf8();
            }
        }
    }
    Ok(out)
}

fn parse_report(path: &Path) -> Result<Value, ()> {
    let source = std::fs::read_to_string(path).map_err(|_| ())?;
    let adapted = adapt_json(&source)?;
    let mut parser = serde_json::Deserializer::from_str(&adapted);
    parser.disable_recursion_limit();
    let value = Value::deserialize(&mut parser).map_err(|_| ())?;
    parser.end().map_err(|_| ())?;
    Ok(value)
}

fn python_string(text: &str) -> Result<PythonString, ()> {
    let mut chars = text.chars();
    let mut codepoints = Vec::with_capacity(text.len());
    while let Some(ch) = chars.next() {
        if ch != SENTINEL {
            codepoints.push(ch as u32);
            continue;
        }
        let tag = chars.next().ok_or(())? as u32;
        if tag == 0 {
            codepoints.push(SENTINEL as u32);
        } else if (SURROGATE_TAG_START..=SURROGATE_TAG_END).contains(&tag) {
            codepoints.push(tag - SURROGATE_TAG_BASE);
        } else {
            return Err(());
        }
    }
    Ok(PythonString(codepoints))
}

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

fn identities(report: &Value, executed: bool) -> Result<Identities, ()> {
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
            let id = python_string(spec.get("id").and_then(Value::as_str).ok_or(())?)?;
            if id.0.is_empty() {
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
                let project =
                    python_string(test.get("projectId").and_then(Value::as_str).ok_or(())?)?;
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
                let count = found.entry((id.clone(), project.clone())).or_insert(0);
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
            .any(|(spec, project)| spec.0.len() > 256 || project.0.len() > 256)
    {
        return Err(());
    }
    Ok(found)
}

fn ascii_json_string(text: &PythonString) -> String {
    let mut out = String::from("\"");
    for &codepoint in &text.0 {
        match codepoint {
            0x08 => out.push_str("\\b"),
            0x09 => out.push_str("\\t"),
            0x0a => out.push_str("\\n"),
            0x0c => out.push_str("\\f"),
            0x0d => out.push_str("\\r"),
            0x22 => out.push_str("\\\""),
            0x5c => out.push_str("\\\\"),
            0x20..=0x7e => out.push(char::from_u32(codepoint).expect("ASCII is a scalar")),
            0x00..=0xffff => out.push_str(&format!("\\u{codepoint:04x}")),
            _ => {
                let offset = codepoint - 0x10000;
                let high = 0xd800 + (offset >> 10);
                let low = 0xdc00 + (offset & 0x3ff);
                out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
            }
        }
    }
    out.push('"');
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

fn write_receipt(destination: &Path, identities: &Identities) -> Result<(), ()> {
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

fn run(args: &[OsString]) -> Result<Identities, ()> {
    let discovery = parse_report(Path::new(&args[0]))?;
    let execution = parse_report(Path::new(&args[1]))?;
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
}

pub fn command(args: &[OsString]) -> ExitCode {
    if args.len() != 2 {
        eprintln!("browser test receipts: need discovery and execution reports");
        return ExitCode::FAILURE;
    }
    // serde_json's recursive Value parser and Value drop both stay on this
    // bounded-depth worker's large stack. Only flat identities cross the join.
    let owned_args = args.to_vec();
    let result = match std::thread::Builder::new()
        .stack_size(PARSER_STACK_BYTES)
        .spawn(move || run(&owned_args))
    {
        Ok(worker) => worker.join().unwrap_or(Err(())),
        Err(_) => Err(()),
    };
    if result.is_err() {
        eprintln!("browser test receipts: incomplete or mismatched reports");
        return ExitCode::FAILURE;
    }
    println!("browser test receipts: all discovered tests passed once");
    ExitCode::SUCCESS
}
