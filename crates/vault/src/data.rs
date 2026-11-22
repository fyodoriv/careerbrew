//! Synthetic DATA transport. Every ledger row is built from the actual native
//! invocation: canonical provider id, purpose, observed target, bound arguments
//! and the executed return projection. No fixture declaration is a substitute.

use crate::canon::Canon;
use crate::store::Store;
use crate::synthetic::sha;
use num_bigint::BigInt;
use serde_json::{json, Map, Value};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

fn decode(value: &Value) -> Result<Canon, String> {
    Ok(match value["kind"].as_str().ok_or("typed-kind")? {
        "null" => Canon::Null,
        "bool" => Canon::Bool(value["value"].as_bool().ok_or("typed-bool")?),
        "int" => Canon::Int(
            value["decimal"]
                .as_str()
                .ok_or("typed-int")?
                .parse::<BigInt>()
                .map_err(|_| "typed-int")?,
        ),
        "float64" => Canon::Float(f64::from_bits(
            u64::from_str_radix(value["bits"].as_str().ok_or("typed-float")?, 16)
                .map_err(|_| "typed-float")?,
        )),
        "str" if value["encoding"] == "literal" => {
            Canon::Str(value["value"].as_str().ok_or("typed-str")?.into())
        }
        "date" => Canon::Date(value["iso"].as_str().ok_or("typed-date")?.into()),
        "datetime" => Canon::DateTime(value["iso"].as_str().ok_or("typed-datetime")?.into()),
        "list" | "tuple" => Canon::List(
            value["items"]
                .as_array()
                .ok_or("typed-list")?
                .iter()
                .map(decode)
                .collect::<Result<_, _>>()?,
        ),
        "map" => Canon::Map(
            value["entries"]
                .as_array()
                .ok_or("typed-map")?
                .iter()
                .map(|entry| Ok((decode(&entry["key"])?, decode(&entry["value"])?)))
                .collect::<Result<_, String>>()?,
        ),
        _ => return Err("typed-unsupported".into()),
    })
}

fn encode(value: &Canon) -> Value {
    match value {
        Canon::Null => json!({"kind": "null"}),
        Canon::Bool(value) => json!({"kind": "bool", "value": value}),
        Canon::Int(value) => json!({"kind": "int", "decimal": value.to_string()}),
        Canon::Float(value) => {
            json!({"kind": "float64", "bits": format!("{:016x}", value.to_bits())})
        }
        Canon::Str(value) => json!({"kind": "str", "encoding": "literal", "value": value}),
        Canon::Date(value) => json!({"kind": "date", "iso": value}),
        Canon::DateTime(value) => json!({"kind": "datetime", "iso": value}),
        Canon::List(items) => {
            json!({"kind": "list", "items": items.iter().map(encode).collect::<Vec<_>>()})
        }
        Canon::Map(items) => {
            let mut entries: Vec<(&str, &Canon)> = items
                .iter()
                .filter_map(|(key, value)| match key {
                    Canon::Str(key) => Some((key.as_str(), value)),
                    _ => None,
                })
                .collect();
            entries.sort_by(|(a, _), (b, _)| a.as_bytes().cmp(b.as_bytes()));
            json!({"kind": "map", "entries": entries.into_iter().map(|(key, value)| json!({"key": encode(&Canon::Str(key.into())), "value": encode(value)})).collect::<Vec<_>>()})
        }
    }
}

fn digest_text(text: &str) -> Value {
    json!({"kind": "str", "encoding": "sha256", "utf8_bytes": text.len().to_string(), "sha256": sha(text.as_bytes())})
}

fn ascii_json(value: &Value) -> String {
    let text = value.to_string();
    let mut out = String::new();
    for ch in text.chars() {
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

fn target_value(relative: &str) -> Value {
    json!({"kind": "path", "root": "owned", "relative": relative})
}

fn bindings(
    api: &str,
    arguments: &Map<String, Value>,
    relative: &str,
) -> Result<Map<String, Value>, String> {
    let mut out = Map::new();
    out.insert("path".into(), target_value(relative));
    match api {
        "read_text" | "read_yaml" => {
            out.insert("default".into(), encode(&Canon::Null));
            out.insert("create_key".into(), json!(true));
        }
        "read_json" => {
            out.insert(
                "default".into(),
                arguments
                    .get("default")
                    .map(decode)
                    .transpose()?
                    .map(|v| encode(&v))
                    .unwrap_or_else(|| encode(&Canon::Null)),
            );
            out.insert("create_key".into(), json!(true));
        }
        "read_jsonl" => {
            out.insert("create_key".into(), json!(true));
        }
        _ => {
            for (name, value) in arguments {
                out.insert(name.clone(), encode(&decode(value)?));
            }
        }
    }
    Ok(out)
}

fn actual_target(owned: &Path, relative: &str) -> Result<PathBuf, String> {
    let path = Path::new(relative);
    if relative.is_empty()
        || path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("target-path".into());
    }
    let target = owned.join(path);
    let mut current = owned.to_path_buf();
    for part in path.components() {
        current.push(part.as_os_str());
        if current.is_symlink() {
            return Err("target-symlink".into());
        }
    }
    Ok(target)
}

fn write_report(path: &Path, report: &Value) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| "report-open")?;
    file.write_all(report.to_string().as_bytes())
        .map_err(|_| "report-write")?;
    file.write_all(b"\n").map_err(|_| "report-write".to_owned())
}

pub fn run_phase(
    owned: &Path,
    fixture_path: &Path,
    report_path: &Path,
    trial: &str,
    case_id: &str,
    role: &str,
) -> Result<(), String> {
    if !matches!(trial, "first" | "second") || !matches!(role, "reader" | "writer") {
        return Err("phase-identity".into());
    }
    let fixture: Value =
        serde_json::from_slice(&std::fs::read(fixture_path).map_err(|_| "fixture-read")?)
            .map_err(|_| "fixture-json")?;
    let case = fixture["cases"]
        .as_array()
        .ok_or("fixture-cases")?
        .iter()
        .find(|case| case["case"] == case_id)
        .ok_or("phase-case")?;
    let phase = case["phases"]
        .as_array()
        .ok_or("fixture-phases")?
        .iter()
        .find(|phase| phase["role"] == role)
        .ok_or("phase-role")?;
    let api = phase["api"].as_str().ok_or("phase-api")?;
    let relative = phase["target"].as_str().ok_or("phase-target")?;
    let target = actual_target(owned, relative)?;
    let data_root = owned.join("data");
    let key_file = owned.join("keys/backup.key");
    let keys = |create: bool| crate::synthetic::key_for_data(&data_root, &key_file, create);
    let store = Store::new(&data_root, true, &keys);
    let arguments = phase["arguments"].as_object().ok_or("phase-arguments")?;
    let mut calls = Vec::new();
    for spec in phase["ledger"].as_array().ok_or("phase-ledger")? {
        let purpose = spec["purpose"].as_str().ok_or("ledger-purpose")?;
        let serialized = purpose == "serialized-bytes";
        if purpose != "primary" && !serialized {
            return Err("ledger-purpose".into());
        }
        let actual_api = if serialized { "read_text" } else { api };
        let function = format!("careerbrew.store.{actual_api}");
        let empty = Map::new();
        let supplied = if serialized { &empty } else { arguments };
        let bound = bindings(actual_api, supplied, relative)?;
        let arguments_sha256 = sha(ascii_json(&Value::Object(bound)).as_bytes());
        let (returned, text): (Value, Option<String>) = match actual_api {
            "read_text" => {
                let value = store
                    .read_text(&target, None, true)
                    .map_err(|_| "store-read-text")?;
                let text = value.unwrap_or_default();
                (encode(&Canon::Str(text.clone())), Some(text))
            }
            "read_json" => {
                let default = arguments
                    .get("default")
                    .map(decode)
                    .transpose()?
                    .unwrap_or(Canon::Null);
                (
                    encode(
                        &store
                            .read_json(&target, default, true)
                            .map_err(|_| "store-read-json")?,
                    ),
                    None,
                )
            }
            "read_yaml" => {
                let default = arguments
                    .get("default")
                    .map(decode)
                    .transpose()?
                    .unwrap_or(Canon::Null);
                (
                    encode(
                        &store
                            .read_yaml(&target, default, true)
                            .map_err(|_| "store-read-yaml")?,
                    ),
                    None,
                )
            }
            "read_jsonl" => (
                encode(&Canon::List(
                    store
                        .read_jsonl(&target, true)
                        .map_err(|_| "store-read-jsonl")?,
                )),
                None,
            ),
            "write_text" => {
                let Canon::Str(text) = decode(arguments.get("text").ok_or("argument-text")?)?
                else {
                    return Err("argument-text".into());
                };
                store
                    .write_text(&target, &text)
                    .map_err(|_| "store-write-text")?;
                (json!({"kind": "null"}), None)
            }
            "write_json" => {
                store
                    .write_json(
                        &target,
                        &decode(arguments.get("data").ok_or("argument-data")?)?,
                    )
                    .map_err(|_| "store-write-json")?;
                (json!({"kind": "null"}), None)
            }
            "write_yaml" => {
                store
                    .write_yaml(
                        &target,
                        &decode(arguments.get("data").ok_or("argument-data")?)?,
                    )
                    .map_err(|_| "store-write-yaml")?;
                (json!({"kind": "null"}), None)
            }
            "write_jsonl" => {
                let Canon::List(rows) = decode(arguments.get("rows").ok_or("argument-rows")?)?
                else {
                    return Err("argument-rows".into());
                };
                store
                    .write_jsonl(&target, &rows)
                    .map_err(|_| "store-write-jsonl")?;
                (json!({"kind": "null"}), None)
            }
            "append_jsonl" => {
                let Canon::List(rows) = decode(arguments.get("rows").ok_or("argument-rows")?)?
                else {
                    return Err("argument-rows".into());
                };
                let count = store
                    .append_jsonl(&target, &rows)
                    .map_err(|_| "store-append-jsonl")?;
                (json!({"kind": "int", "decimal": count.to_string()}), None)
            }
            "upsert_jsonl" => {
                let Canon::List(rows) = decode(arguments.get("rows").ok_or("argument-rows")?)?
                else {
                    return Err("argument-rows".into());
                };
                let Canon::List(fields) =
                    decode(arguments.get("fields").ok_or("argument-fields")?)?
                else {
                    return Err("argument-fields".into());
                };
                let fields: Vec<&str> = fields
                    .iter()
                    .map(|field| match field {
                        Canon::Str(value) => Ok(value.as_str()),
                        _ => Err("argument-fields"),
                    })
                    .collect::<Result<_, _>>()?;
                let (added, replaced) = store
                    .upsert_jsonl(&target, &rows, &fields)
                    .map_err(|_| "store-upsert-jsonl")?;
                let items: Vec<Value> = [added, replaced]
                    .iter()
                    .map(|value| json!({"kind": "int", "decimal": value.to_string()}))
                    .collect();
                (json!({"kind": "tuple", "items": items}), None)
            }
            "write_text_if_changed" => {
                let Canon::Str(text) = decode(arguments.get("text").ok_or("argument-text")?)?
                else {
                    return Err("argument-text".into());
                };
                let wrote = store
                    .write_text_if_changed(&target, &text)
                    .map_err(|_| "store-write-if-changed")?;
                (json!({"kind": "bool", "value": wrote}), None)
            }
            "write_json_if_changed" => {
                let wrote = store
                    .write_json_if_changed(
                        &target,
                        &decode(arguments.get("data").ok_or("argument-data")?)?,
                    )
                    .map_err(|_| "store-write-json-if-changed")?;
                (json!({"kind": "bool", "value": wrote}), None)
            }
            _ => return Err("provider-identity".into()),
        };
        let use_digest = role == "reader"
            && (serialized || (purpose == "primary" && matches!(case_id, "2" | "8")));
        let returned = if use_digest {
            match text {
                Some(text) => digest_text(&text),
                None => returned,
            }
        } else {
            returned
        };
        calls.push(json!({
            "function": function,
            "purpose": purpose,
            "target": target_value(relative),
            "arguments_sha256": arguments_sha256,
            "returned": returned,
        }));
    }
    write_report(
        report_path,
        &json!({"schema": "careerbrew-data-phase-v1", "case": case_id, "role": role, "api_calls": calls}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writer_bindings_match_the_registered_typed_arguments() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .unwrap();
        let fixture: Value = serde_json::from_slice(
            &std::fs::read(root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json")).unwrap(),
        )
        .unwrap();
        let mut checked = 0;
        for case in fixture["cases"].as_array().unwrap() {
            for phase in case["phases"].as_array().unwrap() {
                if phase["role"] != "writer" {
                    continue;
                }
                let api = phase["api"].as_str().unwrap();
                let relative = phase["target"].as_str().unwrap();
                let arguments = phase["arguments"].as_object().unwrap();
                let actual = Value::Object(bindings(api, arguments, relative).unwrap());
                for call in phase["ledger"].as_array().unwrap() {
                    assert_eq!(
                        actual, call["expected_bound_arguments"],
                        "case {} api {api}",
                        case["case"]
                    );
                    checked += 1;
                }
            }
        }
        assert_eq!(checked, 9);
    }
}
