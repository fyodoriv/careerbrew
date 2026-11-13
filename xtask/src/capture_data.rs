// Observe the bounded data corpus without replacing its Python provider.
use crate::capture_actions as shared;
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
};

const FIXTURE: &str = include_str!("../../docs/plans/2026-11-05-rust-data-fixture-v1.json");
const VALUE_SCHEMA: &str = include_str!("../../docs/plans/2026-11-05-rust-data-value-schema.json");
const PHASE_SCHEMA: &str = include_str!("../../docs/plans/2026-11-05-rust-data-phase-schema.json");
const BOOTSTRAP_SCHEMA: &str =
    include_str!("../../docs/plans/2026-11-05-rust-data-bootstrap-schema.json");
const DRIVER: &str = include_str!("../tests/fixtures/capture/data_driver.py");
const OBSERVER: &str = include_str!("../tests/fixtures/capture/observer/sitecustomize.py");
const FIXTURE_SHA: &str = "6025c63de77ed2ed9fb6a69a95860a33b38b675840edbf187405918c23222fe2";
const REFERENCE_SEMANTIC: &str =
    include_str!("../tests/fixtures/capture/data_reference_v1.semantic.json");
const REFERENCE_PROVENANCE: &str =
    include_str!("../tests/fixtures/capture/data_reference_v1.provenance.json");
const REFERENCE_SEMANTIC_SHA: &str =
    "d752043a3dad4a940ad7955289e1ca79490454cca25881f0940abe0bc79a20b2";
const REFERENCE_PROVENANCE_SHA: &str =
    "b020b2b3d1d38a48ceeeb41eba781e66cd0732446b2a9f337bdc06d536b51fba";
const FAULT_SCHEMA: &str = include_str!("../../docs/plans/2026-11-05-rust-data-fault-schema.json");
const LIMIT: usize = 4 * 1024 * 1024;

#[cfg(test)]
const AGGREGATE_LIMIT: usize = 8 * 1024 * 1024;
#[cfg(test)]
fn strict_aggregate_json(bytes: &[u8]) -> Result<Value, String> {
    strict_json_bounded(bytes, AGGREGATE_LIMIT, "data aggregate exceeds limit")
}
#[cfg(test)]
fn aggregate_candidate(path: &Path) -> Result<Value, String> {
    strict_aggregate_json(
        &bounded_file_bytes(path, AGGREGATE_LIMIT)
            .map_err(|_| "aggregate candidate read refused")?,
    )
}

fn strict_json(bytes: &[u8]) -> Result<Value, String> {
    strict_json_bounded(bytes, LIMIT, "data protocol exceeds limit")
}
fn strict_json_bounded(bytes: &[u8], budget: usize, over_limit: &str) -> Result<Value, String> {
    if bytes.len() > budget {
        return Err(over_limit.into());
    }
    let value: Value = serde_json::from_slice(bytes).map_err(|_| "data JSON invalid")?;
    // serde_json owns parsing; this lexical pass detects duplicate decoded keys.
    let mut frames: Vec<Option<(BTreeSet<String>, bool)>> = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                frames.push(Some((BTreeSet::new(), true)));
                i += 1;
            }
            b'[' => {
                frames.push(None);
                i += 1;
            }
            b'}' | b']' => {
                frames.pop();
                i += 1;
            }
            b',' => {
                if let Some(Some((_, key))) = frames.last_mut() {
                    *key = true;
                }
                i += 1;
            }
            b'"' => {
                let start = i;
                i += 1;
                while i < bytes.len() {
                    if bytes[i] == b'\\' {
                        i += 2;
                    } else if bytes[i] == b'"' {
                        i += 1;
                        break;
                    } else {
                        i += 1;
                    }
                }
                if let Some(Some((keys, expected))) = frames.last_mut() {
                    if *expected {
                        let key: String = serde_json::from_slice(&bytes[start..i])
                            .map_err(|_| "data JSON key invalid")?;
                        if !keys.insert(key) {
                            return Err("duplicate data JSON member".into());
                        }
                        *expected = false;
                    }
                }
            }
            _ => i += 1,
        }
        if frames.len() > 64 {
            return Err("data JSON depth exceeds limit".into());
        }
    }
    Ok(value)
}

fn keys(value: &Value, expected: &[&str]) -> Result<(), String> {
    let object = value.as_object().ok_or("data object missing")?;
    if object.len() != expected.len() || expected.iter().any(|name| !object.contains_key(*name)) {
        return Err("data object fields differ".into());
    }
    Ok(())
}
fn decimal(value: &Value) -> bool {
    let Some(s) = value.as_str() else {
        return false;
    };
    if s == "0" {
        return true;
    }
    let digits = s.strip_prefix('-').unwrap_or(s);
    !digits.is_empty() && !digits.starts_with('0') && digits.bytes().all(|b| b.is_ascii_digit())
}
fn hex(value: &Value, length: usize) -> bool {
    value.as_str().is_some_and(|s| {
        s.len() == length
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn registered_calendar(kind: &str, value: &str) -> bool {
    use std::ffi::{CStr, CString};
    let (length, format) = match kind {
        "date" => (10, c"%Y-%m-%d"),
        "datetime" => (25, c"%Y-%m-%dT%H:%M:%S+00:00"),
        _ => return false,
    };
    if value.len() != length || !value.is_ascii() {
        return false;
    }
    let Ok(input) = CString::new(value) else {
        return false;
    };
    let mut parsed: libc::tm = unsafe { std::mem::zeroed() };
    let end = unsafe { libc::strptime(input.as_ptr(), format.as_ptr(), &mut parsed) };
    if end.is_null() || unsafe { *end } != 0 || parsed.tm_year < -1899 || parsed.tm_year > 8099 {
        return false;
    }
    // UTC normalization makes impossible dates differ from their canonical wire form.
    unsafe { libc::timegm(&mut parsed) };
    let mut output = [0_i8; 64];
    let count =
        unsafe { libc::strftime(output.as_mut_ptr(), output.len(), format.as_ptr(), &parsed) };
    count == length && unsafe { CStr::from_ptr(output.as_ptr()) }.to_bytes() == value.as_bytes()
}

fn node(value: &Value, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("data node depth exceeds limit".into());
    }
    match value["kind"].as_str().ok_or("data node kind missing")? {
        "null" => keys(value, &["kind"]),
        "bool" => {
            keys(value, &["kind", "value"])?;
            if value["value"].is_boolean() {
                Ok(())
            } else {
                Err("data bool invalid".into())
            }
        }
        "int" => {
            keys(value, &["kind", "decimal"])?;
            if decimal(&value["decimal"]) {
                Ok(())
            } else {
                Err("data int invalid".into())
            }
        }
        "float64" => {
            keys(value, &["kind", "bits"])?;
            if hex(&value["bits"], 16) {
                Ok(())
            } else {
                Err("data float bits invalid".into())
            }
        }
        "str" => match value["encoding"].as_str() {
            Some("literal") => {
                keys(value, &["kind", "encoding", "value"])?;
                if value["value"].is_string() {
                    Ok(())
                } else {
                    Err("data string invalid".into())
                }
            }
            Some("sha256") => {
                keys(value, &["kind", "encoding", "utf8_bytes", "sha256"])?;
                if decimal(&value["utf8_bytes"])
                    && !value["utf8_bytes"].as_str().unwrap().starts_with('-')
                    && hex(&value["sha256"], 64)
                {
                    Ok(())
                } else {
                    Err("data string digest invalid".into())
                }
            }
            _ => Err("data string encoding invalid".into()),
        },
        kind @ ("date" | "datetime") => {
            keys(value, &["kind", "iso"])?;
            if value["iso"]
                .as_str()
                .is_some_and(|iso| registered_calendar(kind, iso))
            {
                Ok(())
            } else {
                Err("data calendar node invalid".into())
            }
        }
        "list" | "tuple" => {
            keys(value, &["kind", "items"])?;
            for item in value["items"].as_array().ok_or("data sequence invalid")? {
                node(item, depth + 1)?;
            }
            Ok(())
        }
        "map" => {
            keys(value, &["kind", "entries"])?;
            let mut previous: Option<&str> = None;
            for entry in value["entries"].as_array().ok_or("data map invalid")? {
                keys(entry, &["key", "value"])?;
                node(&entry["key"], depth + 1)?;
                if entry["key"]["kind"] != "str" || entry["key"]["encoding"] != "literal" {
                    return Err("data map key invalid".into());
                }
                let key = entry["key"]["value"].as_str().unwrap();
                if previous.is_some_and(|old| old >= key) {
                    return Err("data map key order or identity differs".into());
                }
                previous = Some(key);
                node(&entry["value"], depth + 1)?;
            }
            Ok(())
        }
        "path" => {
            keys(value, &["kind", "root", "relative"])?;
            if value["root"] != "owned"
                || !matches!(
                    value["relative"].as_str(),
                    Some(
                        "data/contract/missing.json"
                            | "data/contract/text.txt"
                            | "data/contract/record.json"
                            | "data/contract/record.yaml"
                            | "data/contract/rows.jsonl"
                    )
                )
            {
                return Err("data path invalid".into());
            }
            Ok(())
        }
        _ => Err("data node kind invalid".into()),
    }
}

fn static_inputs(root: &Path, python: &Path) -> Result<Value, String> {
    let current_binding = current_binding_receipt()?;
    let mut pins = BTreeMap::new();
    for (name, expected) in [
        ("docs/plans/2026-11-05-rust-data-fixture-v1.json", FIXTURE),
        (
            "docs/plans/2026-11-05-rust-data-value-schema.json",
            VALUE_SCHEMA,
        ),
        (
            "docs/plans/2026-11-05-rust-data-phase-schema.json",
            PHASE_SCHEMA,
        ),
        (
            "docs/plans/2026-11-05-rust-data-bootstrap-schema.json",
            BOOTSTRAP_SCHEMA,
        ),
        (
            "xtask/tests/fixtures/capture/data_reference_v1.semantic.json",
            REFERENCE_SEMANTIC,
        ),
        (
            "xtask/tests/fixtures/capture/data_reference_v1.provenance.json",
            REFERENCE_PROVENANCE,
        ),
        (
            "docs/plans/2026-11-05-rust-data-fault-schema.json",
            FAULT_SCHEMA,
        ),
        ("xtask/tests/fixtures/capture/data_driver.py", DRIVER),
        (
            "xtask/tests/fixtures/capture/observer/sitecustomize.py",
            OBSERVER,
        ),
    ] {
        let raw = fs::read(root.join(name)).map_err(|_| "data input missing")?;
        if raw != expected.as_bytes() {
            return Err("data input differs from compiled pin".into());
        }
        pins.insert(name.to_owned(),json!({"sha256":shared::digest(&raw),"mode":fs::metadata(root.join(name)).map_err(|_|"data input mode missing")?.permissions().mode()&0o777}));
    }
    for name in [
        "xtask/src/capture_data.rs",
        "xtask/tests/capture_data.rs",
        "xtask/tests/support/actions_controls.rs",
        "xtask/tests/support/http_controls.rs",
        "xtask/tests/support/owner_controls.rs",
        "xtask/tests/capture_actions.rs",
        "xtask/tests/capture_http.rs",
        "xtask/tests/observer_deadline.rs",
    ] {
        pins.insert(name.to_owned(),json!({"sha256":shared::digest(&fs::read(root.join(name)).map_err(|_|"data source missing")?),"mode":fs::metadata(root.join(name)).map_err(|_|"data source mode missing")?.permissions().mode()&0o777}));
    }
    if shared::digest(FIXTURE.as_bytes()) != FIXTURE_SHA {
        return Err("data fixture pin differs".into());
    }
    let binary = python
        .canonicalize()
        .map_err(|_| "Python executable missing")?;
    let runtime_static = runtime_static_inputs(root, python, &binary)?;
    Ok(
        json!({"pins":pins,"provider_sources":shared::sources(root)?,"current_binding":current_binding,"runtime_static":runtime_static,"python":{"path":python,"binary":binary,"sha256":shared::digest(&fs::read(&binary).map_err(|_|"cannot hash Python")?),"version":shared::version(python)?},"unresolved":["external loaded file contents before initial load are not all prehashed","unloaded dependency files","transient/outside writes","escaped descendants","non-Mac platform matrix"]}),
    )
}

fn mode(entry: &Value) -> Result<u32, String> {
    u32::from_str_radix(entry["mode"].as_str().ok_or("data mode missing")?, 8)
        .map_err(|_| "data mode invalid".into())
}
fn prepare(root: &Path, owned: &Path, fixture: &Value) -> Result<BTreeMap<String, String>, String> {
    let environment = shared::owned_environment(root, owned)?;
    for entry in fixture["baseline"]
        .as_array()
        .ok_or("data baseline missing")?
    {
        if entry["kind"] != "directory" {
            return Err("data baseline kind invalid".into());
        }
        let path = owned.join(entry["path"].as_str().ok_or("data baseline path missing")?);
        fs::create_dir_all(&path).map_err(|_| "cannot prepare data baseline")?;
        fs::set_permissions(path, fs::Permissions::from_mode(mode(entry)?))
            .map_err(|_| "cannot set baseline mode")?;
    }
    Ok(environment)
}
fn identity(owned: &Path, case: &Value) -> Result<Value, String> {
    if case["no_op_identity"].is_null() {
        return Ok(Value::Null);
    }
    use std::os::unix::fs::MetadataExt;
    let target = owned.join(
        case["phases"][0]["target"]
            .as_str()
            .ok_or("no-op target missing")?,
    );
    let meta = fs::metadata(&target).map_err(|_| "no-op file missing")?;
    Ok(
        json!({"inode":meta.ino(),"mtime_seconds":meta.mtime(),"mtime_nanoseconds":meta.mtime_nsec(),"ciphertext_sha256":shared::digest(&fs::read(target).map_err(|_|"no-op bytes missing")?)}),
    )
}
fn verify_delta(
    owned: &Path,
    before: &BTreeMap<String, String>,
    after: &BTreeMap<String, String>,
    phase: &Value,
) -> Result<(), String> {
    let mut expected = BTreeMap::new();
    for field in ["allowed_retained_delta", "allowed_instrumentation_delta"] {
        for change in phase[field]
            .as_array()
            .ok_or("data delta declaration missing")?
        {
            let entry = &change["entry"];
            let path = entry["path"].as_str().ok_or("data delta path missing")?;
            if expected.insert(path, change).is_some() {
                return Err("duplicate data delta declaration".into());
            }
        }
    }
    for path in before.keys() {
        if !after.contains_key(path) {
            return Err("unexpected retained deletion".into());
        }
    }
    for (path, value) in after {
        if before.get(path) == Some(value) {
            continue;
        }
        let change = expected
            .remove(path.as_str())
            .ok_or("unexpected retained change")?;
        if (change["operation"] == "add") != (!before.contains_key(path)) {
            return Err("data delta operation differs".into());
        }
        let entry = &change["entry"];
        let metadata = fs::metadata(owned.join(path)).map_err(|_| "data delta metadata missing")?;
        if !metadata.is_file() || metadata.permissions().mode() & 0o777 != mode(entry)? {
            return Err("data delta kind/mode differs".into());
        }
        let raw = fs::read(owned.join(path)).map_err(|_| "data delta bytes missing")?;
        match entry["content_rule"]
            .as_str()
            .ok_or("data content rule missing")?
        {
            "encrypted-sealed" if !raw.starts_with(b"CBV1\n") => {
                return Err("data sealed boundary differs".into());
            }
            "empty exactly zero bytes" if !raw.is_empty() => {
                return Err("data lock is not empty".into());
            }
            "generated Fernet key; raw bytes never reported" if raw.len() != 44 => {
                return Err("data key boundary differs".into());
            }
            _ => {}
        }
    }
    if !expected.is_empty() {
        return Err("declared data delta missing".into());
    }
    Ok(())
}

#[cfg(test)]
fn verify_report_contract(
    report: &Value,
    trial: &str,
    case: &Value,
    phase: &Value,
    pid: u32,
    root: &Path,
    sources: &Value,
) -> Result<(), String> {
    verify_current_report_structure(report, trial, case, phase, pid, root, sources)?;
    verify_report_semantics(report, phase)
}

fn semantic_differences(report: &Value, phase: &Value) -> Result<Vec<ReturnDifference>, String> {
    let calls = report["api_calls"].as_array().ok_or("data calls missing")?;
    let ledger = phase["ledger"].as_array().ok_or("data ledger missing")?;
    if calls.len() != ledger.len() {
        return Err("data API evidence missing".into());
    }
    let mut differences = Vec::new();
    for (call, spec) in calls.iter().zip(ledger) {
        let dimension = if spec["purpose"] == "primary" {
            (call["returned"] != phase["expected_return"]).then_some(ReturnDimension::Typed)
        } else if !spec["independent_expected_return"].is_null() {
            (call["returned"] != spec["independent_expected_return"])
                .then_some(ReturnDimension::Serialized)
        } else {
            if call["returned"]["kind"] != "str" || call["returned"]["encoding"] != "sha256" {
                return Err("data YAML digest shape differs".into());
            }
            None
        };
        if let Some(dimension) = dimension {
            differences.push(ReturnDifference {
                purpose: spec["purpose"]
                    .as_str()
                    .ok_or("semantic purpose missing")?
                    .into(),
                dimension,
            });
        }
    }
    Ok(differences)
}
#[cfg(test)]
fn verify_report_semantics(report: &Value, phase: &Value) -> Result<(), String> {
    if let Some(d) = semantic_differences(report, phase)?.first() {
        return Err(d.refusal().into());
    }
    Ok(())
}

fn verify_current_report_structure(
    report: &Value,
    trial: &str,
    case: &Value,
    phase: &Value,
    pid: u32,
    root: &Path,
    sources: &Value,
) -> Result<(), String> {
    let manifest = strict_json(CURRENT_BINDING_MANIFEST)?;
    let (fixture, _) = current_binding_view(manifest.as_array().ok_or("IdentityMismatch")?)?;
    let expected_case = fixture["cases"]
        .as_array()
        .ok_or("IdentityMismatch")?
        .iter()
        .find(|item| item["case"] == case["case"])
        .ok_or("IdentityMismatch")?;
    let expected_phase = expected_case["phases"]
        .as_array()
        .ok_or("IdentityMismatch")?
        .iter()
        .find(|item| item["role"] == phase["role"])
        .ok_or("IdentityMismatch")?;
    let mut current_phase = phase.clone();
    for (call, expected) in current_phase["ledger"]
        .as_array_mut()
        .ok_or("IdentityMismatch")?
        .iter_mut()
        .zip(
            expected_phase["ledger"]
                .as_array()
                .ok_or("IdentityMismatch")?,
        )
    {
        if call["sequence"] != expected["sequence"]
            || call["function"] != expected["function"]
            || call["purpose"] != expected["purpose"]
            || call["observed_target"] != expected["observed_target"]
        {
            return Err("IdentityMismatch".into());
        }
        let mut historical_bindings = expected["expected_bound_arguments"].clone();
        if phase["role"] == "reader" {
            historical_bindings
                .as_object_mut()
                .ok_or("IdentityMismatch")?
                .remove("create_key");
        }
        if call["expected_bound_arguments"] != historical_bindings {
            return Err("IdentityMismatch".into());
        }
        call["expected_bound_arguments"] = expected["expected_bound_arguments"].clone();
    }
    if current_phase["ledger"].as_array().map(Vec::len)
        != expected_phase["ledger"].as_array().map(Vec::len)
    {
        return Err("IdentityMismatch".into());
    }
    verify_report_structure(report, trial, case, &current_phase, pid, root, sources)
}

fn verify_report_structure(
    report: &Value,
    trial: &str,
    case: &Value,
    phase: &Value,
    pid: u32,
    root: &Path,
    sources: &Value,
) -> Result<(), String> {
    let mut fields = vec![
        "schema",
        "trial",
        "case",
        "role",
        "pid",
        "fixture_sha256",
        "api_calls",
    ];
    if report.get("driver_provenance").is_some() {
        fields.push("driver_provenance");
    }
    keys(report, &fields)?;
    if report["schema"] != "careerbrew-data-phase-v1"
        || report["trial"] != trial
        || report["case"] != case["case"]
        || report["role"] != phase["role"]
        || report["pid"] != pid
        || report["fixture_sha256"] != FIXTURE_SHA
    {
        return Err("data phase identity differs".into());
    }
    let calls = report["api_calls"].as_array().ok_or("data calls missing")?;
    let ledger = phase["ledger"].as_array().ok_or("data ledger missing")?;
    if calls.len() != ledger.len() {
        return Err("data API evidence missing".into());
    }
    let source = root.join("src/careerbrew/store.py");
    for (call, spec) in calls.iter().zip(ledger) {
        keys(
            call,
            &[
                "sequence",
                "purpose",
                "function",
                "arguments_sha256",
                "target",
                "module_path",
                "loader_origin",
                "function_source",
                "source_sha256",
                "started",
                "returned",
                "completed",
            ],
        )?;
        for field in ["sequence", "purpose", "function"] {
            if call[field] != spec[field] {
                return Err("data actual API differs".into());
            }
        }
        for field in ["module_path", "loader_origin", "function_source"] {
            if call[field].as_str() != source.to_str() {
                return Err("data original source identity differs".into());
            }
        }
        let expected_source = sources["src/careerbrew/store.py"]
            .as_str()
            .ok_or("store source pin missing")?
            .rsplit(':')
            .next()
            .unwrap();
        if call["source_sha256"] != expected_source
            || call["started"] != true
            || call["completed"] != true
        {
            return Err("data original API completion differs".into());
        }
        node(&call["target"], 0)?;
        if call["target"] != spec["observed_target"] {
            return Err("data actual target differs".into());
        }
        if call["arguments_sha256"] != bound_argument_digest(&spec["expected_bound_arguments"])? {
            return Err("data actual bound arguments differ".into());
        }
        node(&call["returned"], 0)?;
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContractFault {
    JsonOne,
    JsonBool,
    JsonLarge,
    JsonZero,
    YamlDateMap,
    YamlMapDate,
    SameWrong,
    Plaintext,
    Stdout,
    Stderr,
    ExtraWrite,
    Empty,
    Nonzero,
    Hang,
    Rewrite,
}
impl ContractFault {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "json-one" => Some(Self::JsonOne),
            "json-bool" => Some(Self::JsonBool),
            "json-large" => Some(Self::JsonLarge),
            "json-zero" => Some(Self::JsonZero),
            "yaml-date-map" => Some(Self::YamlDateMap),
            "yaml-map-date" => Some(Self::YamlMapDate),
            "same-wrong" => Some(Self::SameWrong),
            "plaintext" => Some(Self::Plaintext),
            "stdout" => Some(Self::Stdout),
            "stderr" => Some(Self::Stderr),
            "extra-write" => Some(Self::ExtraWrite),
            "empty" => Some(Self::Empty),
            "nonzero" => Some(Self::Nonzero),
            "hang" => Some(Self::Hang),
            "rewrite" => Some(Self::Rewrite),
            _ => None,
        }
    }
    fn id(self) -> &'static str {
        match self {
            Self::JsonOne => "json-one",
            Self::JsonBool => "json-bool",
            Self::JsonLarge => "json-large",
            Self::JsonZero => "json-zero",
            Self::YamlDateMap => "yaml-date-map",
            Self::YamlMapDate => "yaml-map-date",
            Self::SameWrong => "same-wrong",
            Self::Plaintext => "plaintext",
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
            Self::ExtraWrite => "extra-write",
            Self::Empty => "empty",
            Self::Nonzero => "nonzero",
            Self::Hang => "hang",
            Self::Rewrite => "rewrite",
        }
    }
    fn applies(self, case: &str, role: &str) -> bool {
        match self {
            Self::JsonOne | Self::JsonBool | Self::JsonLarge | Self::JsonZero => {
                case == "3" && role == "writer"
            }
            Self::YamlDateMap | Self::YamlMapDate => case == "4" && role == "writer",
            Self::SameWrong => matches!(case, "3" | "9") && role == "writer",
            Self::Rewrite => case == "8" && role == "writer",
            Self::Empty | Self::Nonzero | Self::Hang => {
                (case == "2" && role == "writer") || (case == "1" && role == "reader")
            }
            _ => case == "2" && role == "writer",
        }
    }
}

fn fault_control_known(value: &Value, schema: &Value) -> bool {
    schema["properties"]["control"]["enum"]
        .as_array()
        .is_some_and(|ids| ids.contains(value))
        && value.as_str().and_then(ContractFault::from_id).is_some()
}
fn fault_protocol_shape(value: &Value) -> Result<(), String> {
    let schema = strict_json(FAULT_SCHEMA.as_bytes())?;
    if !fault_control_known(&value["control"], &schema)
        || !matches!(
            value["case"].as_str(),
            Some("1" | "2" | "3" | "4" | "8" | "9")
        )
    {
        return Err("fault control/case shape differs".into());
    }
    keys(
        value,
        &[
            "schema",
            "control",
            "trial",
            "case",
            "role",
            "pid",
            "driver_sha256",
            "stage",
            "direct_calls_observed",
            "hook",
            "write_bytes",
        ],
    )?;
    if value["schema"] != "careerbrew-data-fault-v1"
        || !hex(&value["driver_sha256"], 64)
        || value["pid"]
            .as_u64()
            .is_none_or(|p| p == 0 || p > i32::MAX as u64)
        || value["direct_calls_observed"]
            .as_u64()
            .is_none_or(|n| n > 1)
        || !matches!(
            value["stage"].as_str(),
            Some("started" | "applied" | "api-complete")
        )
        || !matches!(value["trial"].as_str(), Some("first" | "second"))
        || !matches!(value["role"].as_str(), Some("writer" | "reader"))
        || ["control", "case"].iter().any(|k| !value[k].is_string())
    {
        return Err("fault protocol root differs".into());
    }
    if !value["hook"].is_null() {
        keys(
            &value["hook"],
            &["name", "entered", "completed", "restored"],
        )?;
        if !matches!(
            value["hook"]["name"].as_str(),
            Some("store._json_text" | "yaml.safe_dump" | "store.read_text")
        ) || ["entered", "completed", "restored"]
            .iter()
            .any(|k| !value["hook"][k].is_boolean())
        {
            return Err("fault hook shape differs".into());
        }
    }
    if !value["write_bytes"].is_null()
        && value["write_bytes"]
            .as_u64()
            .is_none_or(|n| n == 0 || n > LIMIT as u64)
    {
        return Err("fault write size differs".into());
    }
    Ok(())
}
fn validate_fault_intent(
    v: &Value,
    f: ContractFault,
    case: &str,
    role: &str,
    trial: &str,
    pid: u32,
) -> Result<(), String> {
    fault_protocol_shape(v)?;
    if !f.applies(case, role)
        || v["control"] != f.id()
        || v["case"] != case
        || v["role"] != role
        || v["trial"] != trial
        || v["pid"] != pid
        || v["driver_sha256"] != shared::digest(DRIVER.as_bytes())
    {
        return Err("fault intent binding differs".into());
    }
    let early = matches!(f, ContractFault::Nonzero | ContractFault::Hang);
    let empty = f == ContractFault::Empty;
    if v["stage"]
        != if early {
            "started"
        } else if empty {
            "api-complete"
        } else {
            "applied"
        }
        || v["direct_calls_observed"] != if early || empty { 0 } else { 1 }
    {
        return Err("fault reached stage differs".into());
    }
    let hook = match f {
        ContractFault::JsonOne
        | ContractFault::JsonBool
        | ContractFault::JsonLarge
        | ContractFault::JsonZero
        | ContractFault::SameWrong => Some("store._json_text"),
        ContractFault::YamlDateMap | ContractFault::YamlMapDate => Some("yaml.safe_dump"),
        ContractFault::Rewrite => Some("store.read_text"),
        _ => None,
    };
    if let Some(name) = hook {
        if v["hook"]["name"] != name
            || ["entered", "completed", "restored"]
                .iter()
                .any(|k| v["hook"][k] != true)
        {
            return Err("fault hook proof differs".into());
        }
    } else if !v["hook"].is_null() {
        return Err("unexpected fault hook".into());
    }
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let bytes = match f {
        ContractFault::Plaintext | ContractFault::Stdout | ContractFault::Stderr => Some(
            fixture["canary"]
                .as_str()
                .ok_or("fault canary missing")?
                .len(),
        ),
        ContractFault::ExtraWrite => Some(b"synthetic-unexpected-write\n".len()),
        _ => None,
    };
    if v["write_bytes"] != json!(bytes) {
        return Err("fault descriptor evidence differs".into());
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ReturnDimension {
    Typed,
    Serialized,
}
#[derive(Debug)]
struct ReturnDifference {
    purpose: String,
    dimension: ReturnDimension,
}
impl ReturnDifference {
    fn refusal(&self) -> &'static str {
        match self.dimension {
            ReturnDimension::Typed => "data typed return differs",
            ReturnDimension::Serialized => "data serialized bytes differ",
        }
    }
    fn record(&self) -> Value {
        json!({"purpose":self.purpose,"dimension":match self.dimension{ReturnDimension::Typed=>"typed-return",ReturnDimension::Serialized=>"serialized-bytes"}})
    }
}
fn may_defer(fault: Option<ContractFault>, case: &str, role: &str, d: &ReturnDifference) -> bool {
    fault == Some(ContractFault::SameWrong)
        && matches!(case, "3" | "9")
        && role == "reader"
        && matches!(
            (d.purpose.as_str(), d.dimension),
            ("primary", ReturnDimension::Typed) | ("serialized-bytes", ReturnDimension::Serialized)
        )
}

// These borrowed inputs live for one synthetic trial.
struct TrialContext<'a> {
    root: &'a Path,
    python: &'a Path,
    owned: &'a Path,
    environment: &'a BTreeMap<String, String>,
    fixture: &'a Value,
    name: &'a str,
    sources: &'a Value,
    runtime: &'a Value,
    raw_dir: &'a Path,
    evidence: Option<&'a Path>,
    driver: Option<&'a Path>,
    omit_driver_provenance: bool,
    fault: Option<ContractFault>,
    observer_root: Option<&'a Path>,
    source_import: Option<&'a Path>,
    diagnostic_extra: Option<&'a BTreeSet<String>>,
}

fn phase_owner_record(
    observation: Option<&shared::OwnerObservation>,
    cwd: &Path,
    owned: &Path,
) -> Value {
    json!({"observation":observation.map(owner_observation),"cwd":cwd,"owned_root":owned})
}

fn phase(context: &TrialContext<'_>, case: &Value, spec: &Value) -> Result<Value, String> {
    let result = phase_observe(context, case, spec);
    let output = match context.evidence {
        Some(path) => path.parent().map(Path::to_owned),
        None => case["case"]
            .as_str()
            .zip(spec["role"].as_str())
            .map(|(id, role)| context.raw_dir.join(format!("{id:0>2}-{role}-evidence"))),
    };
    if let Some(output) = output.filter(|path| path.is_dir()) {
        let category = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.as_str())
            .to_owned();
        let safe = category.len() <= 4096
            && context.fixture["canary"]
                .as_str()
                .is_some_and(|canary| !category.contains(canary));
        if shared::write_receipt(&json!({"category":if safe {Some(category.as_str())}else{None},"category_sha256":shared::digest(category.as_bytes()),"scope":"phase outcome; retained owned changes only"}),&output.join("phase-result.json")).is_err() {
            return match result {Err(primary)=>Err(primary),Ok(_)=>Err("phase result retention unavailable".into())};
        }
    }
    result
}

fn persist_phase_evidence(
    output: &Path,
    owned: &Path,
    execution: &Value,
    allowed: &BTreeSet<String>,
    canary: &str,
    paths: (&str, &str, Option<&str>),
) -> Result<(), String> {
    let primary = execution["primary_failure"]["category"].as_str();
    let terminal = execution["observation"]["wait_observed"] == true
        && execution["observation"]["reaped"] == true
        && execution["observation"]["cleanup_succeeded"] == true;
    let written = shared::write_receipt(execution, &output.join("execution.json"));
    let mut records = vec![("api.json", paths.0), ("observer.json", paths.1)];
    if let Some(path) = paths.2 {
        records.push(("fault.json", path));
    }
    let retained = retain_protocols(
        output,
        owned,
        execution,
        allowed,
        canary,
        (primary.unwrap_or("phase_validation_pending"), terminal),
        (&records, phase_protocol_shape),
    );
    if written.is_err() || retained.is_err() {
        let _secondary = shared::write_receipt(
            &json!({"primary_failure":execution["primary_failure"],"execution_receipt_error_sha256":written.as_ref().err().map(|error|shared::digest(error.as_bytes())),"retention_error_sha256":retained.as_ref().err().map(|error|shared::digest(error.as_bytes()))}),
            &output.join("retention-error.json"),
        );
    }
    if let Some(primary) = primary {
        return Err(primary.to_owned());
    }
    if written.is_err() || retained.is_err() {
        return Err("phase_evidence_unavailable".into());
    }
    Ok(())
}

fn qualify_phase_observer(provenance: &Value, root: &Path, sources: &Value) -> Result<(), String> {
    phase_protocol_shape("observer.json", provenance)?;
    shared::validate_loaded_modules(provenance, root, sources)?;
    if provenance["catalog_calls"] != 0
        || !provenance["descendant"].is_null()
        || provenance["modules"]["careerbrew.store"].is_null()
    {
        return Err("data provider observation differs".into());
    }
    Ok(())
}

fn phase_completion(
    semantic: Result<(), String>,
    physical: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    physical()?;
    semantic
}

fn reference_parts(semantic: &str, provenance: &str) -> Result<Value, String> {
    if semantic.len() != 29056
        || shared::digest(semantic.as_bytes()) != REFERENCE_SEMANTIC_SHA
        || shared::digest(provenance.as_bytes()) != REFERENCE_PROVENANCE_SHA
    {
        return Err("data portable reference pin differs".into());
    }
    let reference = strict_json(semantic.as_bytes())?;
    let metadata = strict_json(provenance.as_bytes())?;
    keys(
        &metadata,
        &[
            "schema",
            "semantic",
            "observation_pins",
            "provider_sources",
            "origin",
            "scope",
        ],
    )?;
    if metadata["schema"] != "careerbrew-data-reference-provenance-v1"
        || metadata["semantic"]["sha256"] != REFERENCE_SEMANTIC_SHA
        || metadata["semantic"]["bytes"] != 29056
        || metadata["observation_pins"]
            .as_object()
            .is_none_or(|pins| pins.len() != 8)
        || metadata["provider_sources"]
            .as_object()
            .is_none_or(|pins| pins.len() != 126)
    {
        return Err("data portable reference metadata differs".into());
    }
    fn safe(value: &Value) -> bool {
        match value {
            Value::String(text) => !text.starts_with('/'),
            Value::Array(items) => items.iter().all(safe),
            Value::Object(items) => items
                .iter()
                .all(|(key, value)| !key.starts_with('/') && safe(value)),
            _ => true,
        }
    }
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let canary = fixture["canary"].as_str().ok_or("data canary missing")?;
    if !safe(&metadata)
        || !safe(&reference)
        || semantic.contains(canary)
        || provenance.contains(canary)
    {
        return Err("data portable reference privacy refusal".into());
    }
    let phases = reference
        .as_array()
        .filter(|phases| phases.len() == 19)
        .ok_or("data reference phases differ")?;
    let mut count = 0;
    for phase in phases {
        keys(
            phase,
            &["case", "role", "calls", "no_op_identity_preserved"],
        )?;
        for call in phase["calls"]
            .as_array()
            .ok_or("data reference calls missing")?
        {
            keys(
                call,
                &[
                    "function",
                    "purpose",
                    "target",
                    "arguments_sha256",
                    "returned",
                ],
            )?;
            if !hex(&call["arguments_sha256"], 64) {
                return Err("data reference argument digest invalid".into());
            }
            node(&call["target"], 0)?;
            node(&call["returned"], 0)?;
            count += 1;
        }
    }
    if count != 25 {
        return Err("data reference direct calls differ".into());
    }
    Ok(reference)
}

#[cfg(test)]
fn compare_data_semantics(first: &Value, second: &Value) -> Result<Value, String> {
    if first != second {
        return Err("fresh data corpus projections differ".into());
    }
    let reference = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE)?;
    compare_projection_against(first, second, &reference)
}

fn compare_current_data_semantics(first: &Value, second: &Value) -> Result<Value, String> {
    if first != second {
        return Err("fresh data corpus projections differ".into());
    }
    let manifest = strict_json(CURRENT_BINDING_MANIFEST)?;
    let reference =
        validate_current_binding_manifest(manifest.as_array().ok_or("IdentityMismatch")?)?;
    compare_projection_against(first, second, &reference)
}

const CURRENT_BINDING_REVISION: &str = "data-current-bindings-v2";
const CURRENT_BINDING_MANIFEST: &[u8] = br#"[{"case":"1","fixture_pointer":"/cases/0/phases/0/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_json","identity":"1:reader:1","new_sha256":"67e83d88d94036c680d467c7f5f55758027665451fabc8753b5310cc3b8561eb","old_sha256":"f5d1974587e1108785745a7bdcc6750cc141a46f62e2418c85ced4b5f16c5314","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/missing.json","root":"owned"}},{"case":"2","fixture_pointer":"/cases/1/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"2:reader:1","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/text.txt","root":"owned"}},{"case":"3","fixture_pointer":"/cases/2/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_json","identity":"3:reader:1","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/record.json","root":"owned"}},{"case":"3","fixture_pointer":"/cases/2/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"3:reader:2","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/record.json","root":"owned"}},{"case":"4","fixture_pointer":"/cases/3/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_yaml","identity":"4:reader:1","new_sha256":"ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0","old_sha256":"1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/record.yaml","root":"owned"}},{"case":"4","fixture_pointer":"/cases/3/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"4:reader:2","new_sha256":"ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0","old_sha256":"1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/record.yaml","root":"owned"}},{"case":"5","fixture_pointer":"/cases/4/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_jsonl","identity":"5:reader:1","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"5","fixture_pointer":"/cases/4/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"5:reader:2","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"6","fixture_pointer":"/cases/5/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_jsonl","identity":"6:reader:1","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"6","fixture_pointer":"/cases/5/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"6:reader:2","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"7","fixture_pointer":"/cases/6/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_jsonl","identity":"7:reader:1","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"7","fixture_pointer":"/cases/6/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"7:reader:2","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/rows.jsonl","root":"owned"}},{"case":"8","fixture_pointer":"/cases/7/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"8:reader:1","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/text.txt","root":"owned"}},{"case":"9","fixture_pointer":"/cases/8/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_json","identity":"9:reader:1","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/record.json","root":"owned"}},{"case":"9","fixture_pointer":"/cases/8/phases/1/ledger/1/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"9:reader:2","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","purpose":"serialized-bytes","role":"reader","sequence":"2","target":{"kind":"path","relative":"data/contract/record.json","root":"owned"}},{"case":"10","fixture_pointer":"/cases/9/phases/1/ledger/0/expected_bound_arguments/create_key","function":"careerbrew.store.read_text","identity":"10:reader:1","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","purpose":"primary","role":"reader","sequence":"1","target":{"kind":"path","relative":"data/contract/text.txt","root":"owned"}}]"#;
const CURRENT_BINDING_MANIFEST_SHA: &str =
    "05974343395faf40eb5112aad4381d417ca8ef57a730eee53ec7b2e14b1447fb";
const CURRENT_BINDING_SEMANTIC_SHA: &str =
    "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344";

fn bound_argument_digest(value: &Value) -> Result<String, String> {
    let argument_bytes =
        serde_json::to_vec(value).map_err(|_| "data argument specification invalid")?;
    // Python's ASCII escaping differs from serde_json for non-ASCII strings.
    let mut ascii = String::new();
    for ch in std::str::from_utf8(&argument_bytes)
        .map_err(|_| "data argument encoding invalid")?
        .chars()
    {
        if ch.is_ascii() {
            ascii.push(ch);
        } else {
            for unit in ch.encode_utf16(&mut [0; 2]) {
                ascii.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    Ok(shared::digest(ascii.as_bytes()))
}

// The compiled table is immutable; derived expected values have invocation scope.
fn current_binding_view(manifest: &[Value]) -> Result<(Value, Value), String> {
    for (bytes, expected) in [
        (FIXTURE.as_bytes(), FIXTURE_SHA),
        (
            VALUE_SCHEMA.as_bytes(),
            "74c1acf9a81e0d80d45544079e1a01895838d18917fb1941ec390e17cfadaadd",
        ),
        (
            PHASE_SCHEMA.as_bytes(),
            "8a760cdf3adeabb03dec426b2c8e72ae45e219b4a59e197ee39356e62d82edaf",
        ),
        (CURRENT_BINDING_MANIFEST, CURRENT_BINDING_MANIFEST_SHA),
    ] {
        if shared::digest(bytes) != expected {
            return Err("data current binding input pin differs".into());
        }
        strict_json(bytes)?;
    }
    let mut reference = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE)?;
    let mut fixture = strict_json(FIXTURE.as_bytes())?;
    let canonical = strict_json(CURRENT_BINDING_MANIFEST)?;
    let slots = canonical.as_array().ok_or("IdentityMismatch")?;
    if slots.len() != 16 {
        return Err("data current binding table count differs".into());
    }
    if manifest.len() > slots.len() {
        return Err("ExtraSlot".into());
    }
    if manifest.len() < slots.len() {
        return Err("MissingSlot".into());
    }
    let mut seen = BTreeSet::new();
    for row in manifest {
        keys(
            row,
            &[
                "identity",
                "case",
                "role",
                "sequence",
                "function",
                "purpose",
                "target",
                "fixture_pointer",
                "old_sha256",
                "new_sha256",
            ],
        )
        .map_err(|_| "IdentityMismatch")?;
        let id = row["identity"].as_str().ok_or("UnknownSlot")?;
        if !seen.insert(id) {
            return Err("DuplicateSlot".into());
        }
        let expected = slots
            .iter()
            .find(|slot| slot["identity"] == id)
            .ok_or("UnknownSlot")?;
        if row["fixture_pointer"] != expected["fixture_pointer"] {
            return Err(if slots
                .iter()
                .any(|slot| slot["fixture_pointer"] == row["fixture_pointer"])
            {
                "DisplacedSlot"
            } else {
                "UnknownSlot"
            }
            .into());
        }
        for field in ["case", "role", "sequence", "function", "purpose", "target"] {
            if row[field] != expected[field] {
                return Err("IdentityMismatch".into());
            }
        }
        node(&row["target"], 0).map_err(|_| "IdentityMismatch")?;
        for (field, category) in [
            ("old_sha256", "OldDigestMismatch"),
            ("new_sha256", "NewDigestMismatch"),
        ] {
            if !hex(&row[field], 64) || row[field] != expected[field] {
                return Err(category.into());
            }
        }
    }
    let mut phase_count = 0;
    let mut call_count = 0;
    for case in fixture["cases"].as_array().ok_or("IdentityMismatch")? {
        for phase in case["phases"].as_array().ok_or("IdentityMismatch")? {
            phase_count += 1;
            node(&phase["expected_return"], 0)?;
            for call in phase["ledger"].as_array().ok_or("IdentityMismatch")? {
                call_count += 1;
                node(&call["observed_target"], 0)?;
                for value in call["expected_bound_arguments"]
                    .as_object()
                    .ok_or("IdentityMismatch")?
                    .values()
                {
                    node(value, 0)?;
                }
            }
        }
    }
    if fixture["cases"].as_array().map(Vec::len) != Some(10)
        || phase_count != 19
        || call_count != 25
    {
        return Err("data fixed corpus count differs".into());
    }
    let mut expected_text = REFERENCE_SEMANTIC.to_owned();
    for row in slots {
        let pointer = row["fixture_pointer"].as_str().ok_or("DisplacedSlot")?;
        let parts = pointer.split('/').collect::<Vec<_>>();
        let case_index: usize = parts
            .get(2)
            .ok_or("DisplacedSlot")?
            .parse()
            .map_err(|_| "DisplacedSlot")?;
        let phase_index: usize = parts
            .get(4)
            .ok_or("DisplacedSlot")?
            .parse()
            .map_err(|_| "DisplacedSlot")?;
        let call_index: usize = parts
            .get(6)
            .ok_or("DisplacedSlot")?
            .parse()
            .map_err(|_| "DisplacedSlot")?;
        let case = &fixture["cases"][case_index];
        let phase = &case["phases"][phase_index];
        let spec = &phase["ledger"][call_index];
        if case["case"] != row["case"]
            || phase["role"] != row["role"]
            || spec["sequence"] != row["sequence"]
            || spec["function"] != row["function"]
            || spec["purpose"] != row["purpose"]
            || spec["observed_target"] != row["target"]
        {
            return Err("IdentityMismatch".into());
        }
        let old = row["old_sha256"].as_str().ok_or("OldDigestMismatch")?;
        let new = row["new_sha256"].as_str().ok_or("NewDigestMismatch")?;
        if bound_argument_digest(&spec["expected_bound_arguments"])? != old {
            return Err("OldDigestMismatch".into());
        }
        let parent = pointer.strip_suffix("/create_key").ok_or("DisplacedSlot")?;
        let bindings = fixture
            .pointer_mut(parent)
            .and_then(Value::as_object_mut)
            .ok_or("DisplacedSlot")?;
        if bindings
            .insert("create_key".into(), json!({"kind":"bool","value":true}))
            .is_some()
        {
            return Err("IdentityMismatch".into());
        }
        if bound_argument_digest(&json!(bindings))? != new {
            return Err("NewDigestMismatch".into());
        }
        let projected_phase = reference
            .as_array_mut()
            .ok_or("IdentityMismatch")?
            .iter_mut()
            .find(|phase| phase["case"] == row["case"] && phase["role"] == row["role"])
            .ok_or("IdentityMismatch")?;
        let projected = projected_phase["calls"]
            .as_array_mut()
            .and_then(|calls| calls.get_mut(call_index))
            .ok_or("IdentityMismatch")?;
        if projected["function"] != row["function"]
            || projected["purpose"] != row["purpose"]
            || projected["target"] != row["target"]
            || projected["arguments_sha256"] != old
        {
            return Err("IdentityMismatch".into());
        }
        projected["arguments_sha256"] = json!(new);
        expected_text = expected_text.replace(old, new);
    }
    if expected_text.len() != 29056
        || shared::digest(expected_text.as_bytes()) != CURRENT_BINDING_SEMANTIC_SHA
        || strict_json(expected_text.as_bytes())? != reference
    {
        return Err("data current binding expected view differs".into());
    }
    Ok((fixture, reference))
}

fn validate_current_binding_manifest(manifest: &[Value]) -> Result<Value, String> {
    current_binding_view(manifest).map(|(_, reference)| reference)
}

fn current_binding_receipt() -> Result<Value, String> {
    let manifest = strict_json(CURRENT_BINDING_MANIFEST)?;
    validate_current_binding_manifest(manifest.as_array().ok_or("IdentityMismatch")?)?;
    Ok(json!({
        "revision":CURRENT_BINDING_REVISION,
        "manifest_sha256":CURRENT_BINDING_MANIFEST_SHA,
        "expected_semantic_sha256":CURRENT_BINDING_SEMANTIC_SHA,
        "historical_fixture_sha256":FIXTURE_SHA,
        "historical_semantic_sha256":REFERENCE_SEMANTIC_SHA,
        "historical_provenance_sha256":REFERENCE_PROVENANCE_SHA,
        "scope":"expected-side comparison metadata; no new observed reference"
    }))
}

fn with_current_binding_preflight<T>(
    expected_revision: &str,
    launch: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if expected_revision != CURRENT_BINDING_REVISION {
        return Err("data expected binding revision differs".into());
    }
    current_binding_receipt()?;
    launch()
}

fn compare_projection_against(
    first: &Value,
    second: &Value,
    reference: &Value,
) -> Result<Value, String> {
    if first != second {
        return Err("fresh data corpus projections differ".into());
    }
    if first != reference || second != reference {
        return Err("semantic_reference_mismatch".into());
    }
    Ok(first.clone())
}

fn phase_pythonpath(context: &TrialContext<'_>) -> String {
    let observer = context
        .observer_root
        .unwrap_or(context.root)
        .join("xtask/tests/fixtures/capture/observer");
    match context.source_import {
        Some(source) => format!("{}:{}", source.display(), observer.display()),
        None => observer.to_string_lossy().into_owned(),
    }
}
fn phase_observe(context: &TrialContext<'_>, case: &Value, spec: &Value) -> Result<Value, String> {
    let root = context.root;
    let python = context.python;
    let owned = context.owned;
    let environment = context.environment;
    let fixture = context.fixture;
    let trial = context.name;
    let sources = context.sources;
    let raw_dir = context.raw_dir;
    let id = case["case"].as_str().ok_or("data case missing")?;
    let role = spec["role"].as_str().ok_or("data role missing")?;
    let stem = format!("{id:0>2}-{role}");
    let api = owned.join("protocol").join(format!("{stem}-api.json"));
    let observer = owned.join("protocol").join(format!("{stem}-observer.json"));
    let mut env = environment.clone();
    for (key, value) in [
        (
            "CAREERBREW_CAPTURE_INPUT",
            root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json"),
        ),
        (
            "CAREERBREW_DATA_FIXTURE",
            root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json"),
        ),
        ("CAREERBREW_CAPTURE_ROOT", root.to_owned()),
        ("CAREERBREW_DATA_OWNED", owned.to_owned()),
        ("CAREERBREW_CAPTURE_OBSERVER", observer.clone()),
        ("CAREERBREW_DATA_REPORT", api.clone()),
    ] {
        env.insert(key.into(), value.to_string_lossy().into_owned());
    }
    if context.omit_driver_provenance {
        env.insert(
            "CAREERBREW_DATA_DRIVER_CONTROL".into(),
            "missing-provenance".into(),
        );
    }
    env.insert("PYTHONPATH".into(), phase_pythonpath(context));
    let active_fault = context.fault.filter(|f| f.applies(id, role));
    if let Some(f) = active_fault {
        env.insert("CAREERBREW_DATA_CONTRACT_CONTROL".into(), f.id().into());
    }
    let canary = fixture["canary"].as_str().ok_or("data canary missing")?;
    let before = shared::files(owned, owned, Some(canary))?;
    let identity_before = if role == "writer" {
        identity(owned, case)?
    } else {
        Value::Null
    };
    let mut command = Command::new(python);
    command
        .arg("-B")
        .arg("-S")
        .arg(
            context
                .driver
                .map(Path::to_owned)
                .unwrap_or_else(|| root.join("xtask/tests/fixtures/capture/data_driver.py")),
        )
        .args([trial, id, role])
        .current_dir(root)
        .env_clear()
        .envs(&env);
    let program = command.get_program().to_owned();
    let args = command
        .get_args()
        .map(std::ffi::OsStr::to_owned)
        .collect::<Vec<_>>();
    let cwd = command
        .get_current_dir()
        .ok_or("data command cwd missing")?
        .to_owned();
    let output = match context.evidence {
        Some(directory) => directory
            .parent()
            .ok_or("phase evidence parent missing")?
            .to_owned(),
        None => raw_dir.join(format!("{stem}-evidence")),
    };
    if context.evidence.is_none() {
        fs::create_dir(&output).map_err(|_| "phase evidence output unavailable")?;
        fs::set_permissions(&output, fs::Permissions::from_mode(0o700))
            .map_err(|_| "phase evidence mode unavailable")?;
    }
    let mut allowed = diagnostic_allowed_paths(root, owned, context.runtime)?;
    if let Some(extra) = context.diagnostic_extra {
        allowed.extend(extra.iter().cloned());
    }
    if let Some(driver) = context.driver {
        allowed.insert(
            driver
                .parent()
                .ok_or("control driver parent missing")?
                .to_string_lossy()
                .into_owned(),
        );
    }
    if let Some(f) = active_fault {
        shared::write_receipt(
            &json!({"schema":"careerbrew-data-fault-intent-v1","control":f.id(),"trial":trial,"case":id,"role":role,"relative":fault_relative(id,role),"driver_sha256":shared::digest(DRIVER.as_bytes()),"scope":"parent pre-execution intent only"}),
            &output.join("control-intent.json"),
        )?;
    }
    let actual =
        shared::execute_during_observed(command, std::time::Duration::from_secs(30), |_, _| Ok(()));
    let (observation, stdout, stderr, primary) = match actual {
        Ok(execution) => (
            Some(execution.observation),
            Some(execution.stdout),
            Some(execution.stderr),
            None,
        ),
        Err(failure) => {
            let category = if failure.cause == shared::OwnerFailureCause::Deadline {
                "phase_deadline"
            } else {
                "phase_owner_failure"
            };
            let primary = json!({"category":category,"cause":format!("{:?}",failure.cause),"diagnostic_sha256":shared::digest(failure.message.as_bytes())});
            (failure.observation, None, None, Some(primary))
        }
    };
    let primary = primary.or_else(|| {
        let category = if observation
            .as_ref()
            .is_none_or(|o| !o.wait_observed || !o.reaped || !o.cleanup_succeeded)
        {
            Some("phase_cleanup_unconfirmed")
        } else if observation
            .as_ref()
            .is_some_and(|o| o.program != program || o.args != args)
        {
            Some("phase command observation differs")
        } else if observation
            .as_ref()
            .is_some_and(|o| o.wait_code != Some(0) || o.wait_signal.is_some())
        {
            Some("phase_nonzero_exit")
        } else if stdout.as_ref().is_some_and(|text| text.contains(canary))
            || stderr.as_ref().is_some_and(|text| text.contains(canary))
        {
            Some("data stream canary")
        } else if stdout.as_ref().is_some_and(|text| !text.is_empty())
            || stderr.as_ref().is_some_and(|text| !text.is_empty())
        {
            Some("data child streams are not empty")
        } else {
            None
        };
        category.map(|category| json!({"category":category}))
    });
    let mut execution = phase_owner_record(observation.as_ref(), &cwd, owned);
    execution["source_root"] = json!(root);
    execution["protocol_scope"] = json!("phase");
    execution["expected_command"] = json!({"program":program,"args":args,"cwd":cwd});
    execution["command_matches"] = json!(observation
        .as_ref()
        .map(|actual| actual.program == program && actual.args == args));
    execution["primary_failure"] = primary.clone().unwrap_or(Value::Null);
    for (name, stream) in [("stdout", &stdout), ("stderr", &stderr)] {
        execution[format!("{name}_bytes")] = json!(stream.as_ref().map(String::len));
        execution[format!("{name}_sha256")] =
            json!(stream.as_ref().map(|text| shared::digest(text.as_bytes())));
    }
    let terminal = observation
        .as_ref()
        .is_some_and(|o| o.wait_observed && o.reaped && o.cleanup_succeeded);
    let api_relative = format!("protocol/{stem}-api.json");
    let observer_relative = format!("protocol/{stem}-observer.json");
    let fault_path = active_fault.map(|_| fault_relative(id, role));
    persist_phase_evidence(
        &output,
        owned,
        &execution,
        &allowed,
        canary,
        (&api_relative, &observer_relative, fault_path.as_deref()),
    )?;
    if !terminal {
        return Err("phase_cleanup_unconfirmed".into());
    }
    let observation = observation.ok_or("phase observation missing")?;
    if observation.program != program || observation.args != args {
        return Err("phase command observation differs".into());
    }
    let fault_evidence = active_fault
        .map(|f| read_fault(owned, &execution, f, (id, role, trial)))
        .transpose()?;
    if let Some(value) = &fault_evidence {
        shared::write_receipt(value, &output.join("fault-evidence.json"))?;
    }
    let pid = observation.pid;
    let exit = observation.wait_code;
    if exit != Some(0) || observation.wait_signal.is_some() {
        return Err("phase_nonzero_exit".into());
    }
    let stdout = stdout.ok_or("phase stdout unavailable")?;
    let stderr = stderr.ok_or("phase stderr unavailable")?;
    if stdout.contains(canary) || stderr.contains(canary) {
        return Err("data stream canary".into());
    }
    if !stdout.is_empty() || !stderr.is_empty() {
        return Err("data child streams are not empty".into());
    }
    for path in [&api, &observer] {
        if !fs::symlink_metadata(path)
            .map_err(|_| "phase_evidence_missing")?
            .file_type()
            .is_file()
        {
            return Err("data protocol entry is not a regular file".into());
        }
    }
    let api_raw = shared::bounded(&api)?;
    let observer_raw = shared::bounded(&observer)?;
    if api_raw.contains(canary) || observer_raw.contains(canary) {
        return Err("data protocol canary".into());
    }
    let report = strict_json(api_raw.as_bytes())?;
    let provenance = strict_json(observer_raw.as_bytes())?;
    qualify_phase_observer(&provenance, root, sources)?;
    let mut effects = None;
    verify_driver_provenance(&report, pid, root)?;
    verify_current_report_structure(&report, trial, case, spec, pid, root, sources)?;
    let differences = semantic_differences(&report, spec)?;
    let semantic = differences
        .iter()
        .find(|d| !may_defer(context.fault, id, role, d))
        .map_or(Ok(()), |d| Err(d.refusal().into()));
    let deferred = differences
        .iter()
        .filter(|d| may_defer(context.fault, id, role, d))
        .map(ReturnDifference::record)
        .collect::<Vec<_>>();
    phase_completion(semantic, || {
        let after = shared::files(owned, owned, Some(canary))?;
        let identity_after = if role == "writer" {
            identity(owned, case)?
        } else {
            Value::Null
        };
        if !identity_before.is_null() {
            let recorded = shared::write_receipt(
                &json!({"before":identity_before,"after":identity_after,"scope":"actual designated no-op file identity only"}),
                &output.join("no-op-proof.json"),
            );
            if identity_before != identity_after {
                return Err("no_op_identity_mismatch".into());
            }
            recorded?;
        }
        verify_delta(
            owned,
            &before,
            &after,
            &effect_spec(spec, active_fault, id, role),
        )?;
        effects = Some((after, identity_after));
        Ok(())
    })?;
    let (after, identity_after) = effects.ok_or("phase effects missing")?;
    fs::write(raw_dir.join(format!("{stem}-api.json")), api_raw.as_bytes())
        .map_err(|_| "cannot preserve raw API report")?;
    fs::write(
        raw_dir.join(format!("{stem}-observer.json")),
        observer_raw.as_bytes(),
    )
    .map_err(|_| "cannot preserve raw source report")?;
    Ok(
        json!({"deferred_differences":deferred,"fault_evidence":fault_evidence,"owner_observation":owner_observation(&observation),"case":id,"role":role,"program":program,"args":args,"cwd":cwd,"pid":pid,"exit":exit,"stdout":stdout,"stderr":stderr,"report":report,"observer":provenance,"before":before,"after":after,"no_op_before":identity_before,"no_op_after":identity_after,"api_raw_sha256":shared::digest(api_raw.as_bytes()),"observer_raw_sha256":shared::digest(observer_raw.as_bytes())}),
    )
}

fn trial(
    root: &Path,
    python: &Path,
    fixture: &Value,
    name: &str,
    sources: &Value,
    raw_dir: &Path,
    runtime_mode: (&Value, Option<ContractFault>),
) -> Result<Value, String> {
    let (runtime, fault_mode) = runtime_mode;
    let scratch = tempfile::tempdir().map_err(|_| "cannot create data trial")?;
    let owned = scratch
        .path()
        .canonicalize()
        .map_err(|_| "cannot canonicalize data trial")?
        .join("owned");
    let environment = prepare(root, &owned, fixture)?;
    let canonical_owned = owned
        .canonicalize()
        .map_err(|_| "data owned root missing after setup")?;
    if canonical_owned != owned {
        return Err("data owned root identity changed".into());
    }
    let baseline = shared::files(&owned, &owned, Some(fixture["canary"].as_str().unwrap()))?;
    let expected: BTreeMap<_, _> = fixture["baseline"]
        .as_array()
        .ok_or("data baseline missing")?
        .iter()
        .map(|entry| {
            Ok((
                entry["path"]
                    .as_str()
                    .ok_or("baseline path missing")?
                    .to_owned(),
                format!("directory:{:o}", mode(entry)?),
            ))
        })
        .collect::<Result<_, String>>()?;
    if baseline != expected {
        return Err("data baseline entries differ".into());
    }
    fs::create_dir(raw_dir).map_err(|_| "cannot preserve trial reports")?;
    let context = TrialContext {
        root,
        python,
        owned: &owned,
        environment: &environment,
        fixture,
        name,
        sources,
        runtime,
        raw_dir,
        evidence: None,
        driver: None,
        omit_driver_provenance: false,
        fault: fault_mode,
        observer_root: None,
        source_import: None,
        diagnostic_extra: None,
    };
    let mut phases = Vec::new();
    for case in fixture["cases"].as_array().ok_or("data cases missing")? {
        for spec in case["phases"].as_array().ok_or("data phases missing")? {
            phases.push(phase(&context, case, spec)?);
        }
    }
    Ok(
        json!({"trial":name,"owned_root":canonical_owned,"phases":phases,"baseline":baseline,"final":shared::files(&owned,&owned,Some(fixture["canary"].as_str().unwrap()))?}),
    )
}

fn projection(trial: &Value) -> Result<Value, String> {
    let phases = trial["phases"]
        .as_array()
        .ok_or("data trial phases missing")?;
    Ok(json!(phases.iter().map(|phase|{
        let calls=phase["report"]["api_calls"].as_array().unwrap().iter().map(|call|json!({"function":call["function"],"purpose":call["purpose"],"target":call["target"],"arguments_sha256":call["arguments_sha256"],"returned":call["returned"]})).collect::<Vec<_>>();
        json!({"case":phase["case"],"role":phase["role"],"calls":calls,"no_op_identity_preserved":if phase["no_op_before"].is_null(){Value::Null}else{json!(phase["no_op_before"]==phase["no_op_after"])}})
    }).collect::<Vec<_>>()))
}

#[cfg(test)]
pub fn preflight(root: &Path, python: &Path) -> Result<Value, String> {
    reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE)?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let mut phases = 0;
    let mut calls = 0;
    for case in fixture["cases"].as_array().ok_or("data cases missing")? {
        for phase in case["phases"].as_array().ok_or("data phases missing")? {
            phases += 1;
            node(&phase["expected_return"], 0)?;
            for call in phase["ledger"].as_array().ok_or("data ledger missing")? {
                calls += 1;
                node(&call["observed_target"], 0)?;
                for value in call["expected_bound_arguments"]
                    .as_object()
                    .ok_or("data bound arguments missing")?
                    .values()
                {
                    node(value, 0)?;
                }
            }
        }
    }
    if fixture["cases"].as_array().unwrap().len() != 10 || phases != 19 || calls != 25 {
        return Err("data fixed corpus count differs".into());
    }
    static_inputs(root, python)
}

// This synchronous wrapper owns parent evidence only; the shared child owner owns execution.
fn record_candidate_attempt(
    output: &Path,
    operation: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    let start = output.with_extension("attempt.json");
    let outcome_path = output.with_extension("outcome.json");
    for path in [
        output.to_owned(),
        output.with_extension("raw"),
        output.with_extension("runtime"),
        start.clone(),
        outcome_path.clone(),
    ] {
        match fs::symlink_metadata(&path) {
            Ok(_) => return Err("candidate attempt output already exists".into()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err("candidate attempt output metadata unavailable".into()),
        }
    }
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let canary = fixture["canary"]
        .as_str()
        .ok_or("candidate canary missing")?;
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-candidate-attempt-v1","state":"started","scope":"Rust function invocation only; no provider process execution inferred","provider_execution":"unobserved"}),
        &start,
    )?;
    let result = operation();
    let category = result.as_ref().err().map(String::as_str);
    let safe = category.is_none_or(|error| error.len() <= 4096 && !error.contains(canary));
    let recorded = shared::write_receipt(
        &json!({"schema":"careerbrew-data-candidate-outcome-v1","rust_result":if result.is_ok(){"ok"}else{"err"},"safe_error":category.filter(|_|safe),"error_sha256":category.map(|error|shared::digest(error.as_bytes())),"qualification":"unqualified","scope":"actual Rust Result only; subprocess wait/cleanup belongs to separate typed owner receipts"}),
        &outcome_path,
    );
    match result {
        Err(primary) => Err(primary),
        Ok(()) => recorded.map_err(|_| "candidate_outcome_unavailable".into()),
    }
}

pub fn run(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    record_candidate_attempt(output, || run_candidate(root, python, output))
}

fn run_candidate(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("data candidate output already exists".into());
    }
    let root = root
        .canonicalize()
        .map_err(|_| "data source root missing")?;
    let runtime = with_current_binding_preflight("data-current-bindings-v2", || {
        runtime_bootstrap(&root, python, &output.with_extension("runtime"), "none")
    })?;
    let before = observed_inputs(&root, python, &runtime)?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let raw = output.with_extension("raw");
    fs::create_dir(&raw).map_err(|_| "data raw-output directory already exists or unavailable")?;
    shared::write_receipt(
        &json!({"scope":"pre-execution data input freeze; no corpus execution claimed","inputs":before}),
        &raw.join("inputs-before.json"),
    )?;
    let first = trial(
        &root,
        python,
        &fixture,
        "first",
        &before["provider_sources"],
        &raw.join("first"),
        (&runtime["observer"], None),
    )?;
    let second = trial(
        &root,
        python,
        &fixture,
        "second",
        &before["provider_sources"],
        &raw.join("second"),
        (&runtime["observer"], None),
    )?;
    let semantic = compare_current_data_semantics(&projection(&first)?, &projection(&second)?)?;
    if before != observed_inputs(&root, python, &runtime)? {
        return Err("data source/runtime inputs changed".into());
    }
    let executable = std::env::current_exe().map_err(|_| "data capture executable missing")?;
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-candidate-v1","qualification":"reference-review-pending","scope":"bounded10-case real data API oracle; no resource, concurrent-lock, startup, full-format or fullRust00 qualification","inputs":before,"capture_executable_sha256":shared::digest(&fs::read(executable).map_err(|_|"cannot hash data capture executable")?),"semantic_projection":semantic,"trials":[first,second],"raw_outputs":raw}),
        output,
    )
}

fn diagnostic_paths(value: &Value, allowed: &BTreeSet<String>) -> Result<(), String> {
    match value {
        Value::String(text) if text.starts_with('/') && !allowed.contains(text) => {
            Err("diagnostic untrusted path".into())
        }
        Value::Array(items) => items
            .iter()
            .try_for_each(|item| diagnostic_paths(item, allowed)),
        Value::Object(items) => items.iter().try_for_each(|(key, value)| {
            if key.starts_with('/') && !allowed.contains(key) {
                return Err("diagnostic untrusted path".into());
            }
            diagnostic_paths(value, allowed)
        }),
        _ => Ok(()),
    }
}

fn retain_diagnostic(
    directory: &Path,
    execution: &Value,
    records: &[(&str, &str)],
    allowed_paths: &BTreeSet<String>,
    canary: &str,
) -> Result<(), String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    fs::create_dir(directory).map_err(|_| "diagnostic output already exists or unavailable")?;
    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
        .map_err(|_| "diagnostic output mode unavailable")?;
    let mut category = execution["validation_category"]
        .as_str()
        .unwrap_or("unknown")
        .to_owned();
    let mut refusal = None;
    let mut inventory = Vec::new();
    let mut safe = Vec::new();
    for (name, raw) in records {
        if !matches!(
            *name,
            "api.json" | "observer.json" | "identity.json" | "fault.json"
        ) {
            return Err("diagnostic record name invalid".into());
        }
        let checked = if raw.contains(canary) {
            Err("diagnostic privacy refusal".into())
        } else {
            strict_json(raw.as_bytes()).and_then(|value| diagnostic_paths(&value, allowed_paths))
        };
        inventory.push(json!({"name":name,"bytes":raw.len(),"sha256":shared::digest(raw.as_bytes()),"retained":checked.is_ok()}));
        if let Err(error) = checked {
            category = error.clone();
            refusal = Some(error);
        } else {
            safe.push((*name, *raw));
        }
    }
    if refusal.is_some() {
        for item in &mut inventory {
            item["retained"] = json!(false);
        }
    }
    let outcome = json!({"schema":"careerbrew-data-diagnostic-retention-v1","execution":execution,"category":category,"records":inventory,"scope":"diagnostic only; no corpus reference or owner cleanup qualification"});
    let serialized = serde_json::to_string_pretty(&outcome)
        .map_err(|_| "diagnostic outcome serialization failed")?;
    if serialized.contains(canary) {
        return Err("diagnostic outcome privacy refusal".into());
    }
    let write = |name: &str, raw: &[u8]| -> Result<(), String> {
        let mut file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(directory.join(name))
            .map_err(|_| "diagnostic output unavailable")?;
        file.write_all(raw)
            .map_err(|_| "diagnostic write failed".into())
    };
    write("outcome.json", serialized.as_bytes())?;
    // A refused protocol set retains no raw member, including otherwise safe peers.
    if let Some(error) = refusal {
        return Err(error);
    }
    for (name, raw) in safe {
        write(name, raw.as_bytes())?;
    }
    Ok(())
}

fn diagnostic_allowed_paths(
    root: &Path,
    owned: &Path,
    observation: &Value,
) -> Result<BTreeSet<String>, String> {
    fn gather(value: &Value, paths: &mut BTreeSet<String>) {
        match value {
            Value::String(text) if text.starts_with('/') => {
                paths.insert(text.clone());
            }
            Value::Array(items) => {
                for item in items {
                    gather(item, paths);
                }
            }
            Value::Object(items) => {
                for (key, value) in items {
                    if key.starts_with('/') {
                        paths.insert(key.clone());
                    }
                    gather(value, paths);
                }
            }
            _ => {}
        }
    }
    let mut paths = BTreeSet::new();
    gather(observation, &mut paths);
    for path in [
        root.to_owned(),
        root.join("xtask/tests/fixtures/capture/data_driver.py"),
        owned.to_owned(),
    ] {
        paths.insert(path.to_string_lossy().into_owned());
    }
    Ok(paths)
}

#[cfg(test)]
fn diagnostic_refusal(result: &Result<Value, String>, canary: &str) -> Value {
    let (category, message) = match result {
        Err(error) if error == "data driver identity differs" => {
            ("driver_identity_mismatch", error.as_str())
        }
        Err(error) if error == "diagnostic unexpected identity acceptance" => {
            ("unexpected_identity_acceptance", error.as_str())
        }
        Err(error) => ("diagnostic_other_refusal", error.as_str()),
        Ok(_) => (
            "unexpected_identity_acceptance",
            "original identity guard did not refuse",
        ),
    };
    let safe = message.len() <= 4096 && !message.contains(canary);
    json!({"category":category,"message":if safe {Some(message)} else {None},"error_sha256":shared::digest(message.as_bytes()),"message_retention":if safe {"safe-original"} else {"privacy-or-bound-refusal"},"scope":"single missing-read diagnostic, not corpus reference"})
}

fn verify_driver_provenance(report: &Value, pid: u32, root: &Path) -> Result<(), String> {
    let identity = report
        .get("driver_provenance")
        .ok_or("driver_identity_missing")?;
    let validate = || -> Result<(), String> {
        keys(identity, &["schema", "pid", "entry", "api_complete"])?;
        if identity["schema"] != "careerbrew-data-driver-provenance-v1" || identity["pid"] != pid {
            return Err("driver identity header invalid".into());
        }
        let driver = root.join("xtask/tests/fixtures/capture/data_driver.py");
        for (field, stage) in [("entry", "main-entry"), ("api_complete", "api-complete")] {
            let sample = &identity[field];
            keys(
                sample,
                &[
                    "stage",
                    "globals_file",
                    "globals_file_sha256",
                    "globals_name",
                    "globals_spec_name",
                    "globals_spec_origin",
                    "main_code_filename",
                    "main_code_sha256",
                    "sys_modules_main_present",
                    "sys_modules_main_file",
                    "sys_modules_main_file_sha256",
                    "sys_modules_main_name",
                    "sys_modules_main_spec_name",
                    "sys_modules_main_spec_origin",
                ],
            )?;
            if sample["stage"] != stage
                || sample["globals_name"] != "__main__"
                || sample["sys_modules_main_name"] != "__main__"
                || sample["sys_modules_main_present"] != true
            {
                return Err("driver identity sample invalid".into());
            }
            for name in [
                "globals_spec_name",
                "globals_spec_origin",
                "sys_modules_main_spec_name",
                "sys_modules_main_spec_origin",
            ] {
                if !sample[name].is_null() {
                    return Err("driver script spec invalid".into());
                }
            }
            for name in [
                "globals_file",
                "main_code_filename",
                "sys_modules_main_file",
            ] {
                if sample[name].as_str() != driver.to_str() {
                    return Err("driver path invalid".into());
                }
            }
            for name in [
                "globals_file_sha256",
                "main_code_sha256",
                "sys_modules_main_file_sha256",
            ] {
                if sample[name] != shared::digest(DRIVER.as_bytes()) {
                    return Err("driver hash invalid".into());
                }
            }
        }
        Ok(())
    };
    validate().map_err(|_| "driver_identity_mismatch".into())
}

// These controls own one invocation directory and three independent child environments.
#[cfg(test)]
pub fn driver_controls(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|_| "driver controls output already exists or unavailable")?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o700))
        .map_err(|_| "driver controls mode unavailable")?;
    let root = root
        .canonicalize()
        .map_err(|_| "driver controls source root missing")?;
    let runtime = runtime_bootstrap(&root, python, &output.join("runtime"), "none")?;
    let frozen = observed_inputs(&root, python, &runtime)?;
    shared::write_receipt(&frozen, &output.join("inputs-before.json"))?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let mut results = Vec::new();
    for (name, expected) in [
        ("valid", "accepted"),
        ("wrong-driver", "driver_identity_mismatch"),
        ("missing-provenance", "driver_identity_missing"),
    ] {
        let directory = output.join(name);
        fs::create_dir(&directory).map_err(|_| "driver control directory unavailable")?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "driver control mode unavailable")?;
        let scratch = tempfile::tempdir().map_err(|_| "driver control owned root unavailable")?;
        let owned = scratch
            .path()
            .canonicalize()
            .map_err(|_| "driver control owned root not canonical")?
            .join("owned");
        let environment = prepare(&root, &owned, &fixture)?;
        let canary = fixture["canary"]
            .as_str()
            .ok_or("driver control canary missing")?;
        let before = shared::files(&owned, &owned, Some(canary))?;
        let copied = directory.join("driver.py");
        let driver = if name == "wrong-driver" {
            fs::write(&copied, DRIVER).map_err(|_| "cannot prepare exact driver copy")?;
            fs::set_permissions(&copied, fs::Permissions::from_mode(0o600))
                .map_err(|_| "driver copy mode unavailable")?;
            Some(copied.as_path())
        } else {
            None
        };
        let retained = directory.join("retained");
        let raw = directory.join("raw");
        fs::create_dir(&raw).map_err(|_| "driver control raw directory unavailable")?;
        let context = TrialContext {
            root: &root,
            python,
            owned: &owned,
            environment: &environment,
            fixture: &fixture,
            name: "first",
            sources: &frozen["provider_sources"],
            runtime: &runtime["observer"],
            raw_dir: &raw,
            evidence: Some(&retained),
            driver,
            omit_driver_provenance: name == "missing-provenance",
            fault: None,
            observer_root: None,
            source_import: None,
            diagnostic_extra: None,
        };
        let result = phase(
            &context,
            &fixture["cases"][0],
            &fixture["cases"][0]["phases"][0],
        );
        let category = match &result {
            Ok(_) => "accepted",
            Err(error) => error.as_str(),
        };
        let safe_category = if category.len() <= 4096 && !category.contains(canary) {
            Some(category)
        } else {
            None
        };
        shared::write_receipt(
            &json!({"control":name,"category":safe_category,"category_sha256":shared::digest(category.as_bytes()),"expected":expected,"scope":"single actual API identity control; no full corpus qualification"}),
            &directory.join("result.json"),
        )?;
        let execution = strict_json(
            &fs::read(directory.join("execution.json"))
                .map_err(|_| "driver control execution evidence missing")?,
        )?;
        let api = strict_json(
            &fs::read(retained.join("api.json"))
                .map_err(|_| "driver control raw API evidence missing")?,
        )?;
        if category != expected
            || execution["observation"]["wait_code"] != 0
            || execution["observation"]["pid"] != api["pid"]
            || execution["command_matches"] != true
            || api["api_calls"]
                .as_array()
                .is_none_or(|calls| calls.len() != 1)
        {
            return Err("driver control actual outcome differs".into());
        }
        for flag in ["wait_observed", "reaped", "cleanup_succeeded"] {
            if execution["observation"][flag] != true {
                return Err("driver control cleanup unconfirmed".into());
            }
        }
        let actual_pid = execution["observation"]["pid"]
            .as_u64()
            .and_then(|pid| u32::try_from(pid).ok())
            .ok_or("driver control PID invalid")?;
        verify_phase_reaped_absent(&execution["observation"])?;
        verify_report_contract(
            &api,
            "first",
            &fixture["cases"][0],
            &fixture["cases"][0]["phases"][0],
            actual_pid,
            &root,
            &frozen["provider_sources"],
        )?;
        let after = shared::files(&owned, &owned, Some(canary))?;
        verify_delta(&owned, &before, &after, &fixture["cases"][0]["phases"][0])?;
        shared::write_receipt(
            &json!({"before":before,"after":after,"scope":"retained owned changes only"}),
            &directory.join("snapshots.json"),
        )?;
        results.push(json!({"control":name,"category":category,"pid":execution["observation"]["pid"],"observation":execution["observation"],"api_calls":1,"os_reap_absence_verified":true,"owned_root":owned,"scope":"actual API identity control with typed owner observation"}));
    }
    let pids = results
        .iter()
        .filter_map(|row| row["pid"].as_u64())
        .collect::<BTreeSet<_>>();
    if pids.len() != 3 || observed_inputs(&root, python, &runtime)? != frozen {
        return Err("driver controls identity/source stability differs".into());
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-driver-controls-v1","results":results,"counts":{"actual_api_phase_processes":3,"runtime_bootstrap_preparation_processes":1,"accepted_valid":1,"refused_wrong_or_missing":2},"runtime_bootstrap_pid":runtime["observation"]["pid"],"tool_child_scope":"version/setup identity children have separate preparation scope, excluded from API-phase and bootstrap counts","scope":"current invocation-owned import-only runtime; no full corpus qualification"}),
        &output.join("results.json"),
    )
}

#[cfg(test)]
pub fn phase_owner_controls(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|_| "phase controls output unavailable")?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o700))
        .map_err(|_| "phase controls mode unavailable")?;
    let root = root
        .canonicalize()
        .map_err(|_| "phase controls root missing")?;
    let runtime = runtime_bootstrap(&root, python, &output.join("runtime"), "none")?;
    let frozen = observed_inputs(&root, python, &runtime)?;
    shared::write_receipt(&frozen, &output.join("inputs-before.json"))?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let canary = fixture["canary"]
        .as_str()
        .ok_or("phase control canary missing")?;
    let mut outcomes = Vec::new();
    for (name, case_index, fault, expected) in [
        ("writer-valid", 1, "none", "accepted"),
        ("writer-skip", 1, "skip", "phase_evidence_missing"),
        ("reader-skip", 0, "skip", "phase_evidence_missing"),
        ("writer-nonzero", 1, "nonzero", "phase_nonzero_exit"),
        ("reader-nonzero", 0, "nonzero", "phase_nonzero_exit"),
        ("writer-deadline", 1, "hang", "phase_deadline"),
        ("reader-deadline", 0, "hang", "phase_deadline"),
    ] {
        let directory = output.join(name);
        fs::create_dir(&directory).map_err(|_| "phase control directory unavailable")?;
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700))
            .map_err(|_| "phase control directory mode unavailable")?;
        let scratch = tempfile::tempdir().map_err(|_| "phase control scratch unavailable")?;
        let owned = scratch
            .path()
            .canonicalize()
            .map_err(|_| "phase control root unavailable")?
            .join("owned");
        let mut environment = prepare(&root, &owned, &fixture)?;
        environment.insert("CAREERBREW_DATA_PHASE_CONTROL".into(), fault.into());
        let before = shared::files(&owned, &owned, Some(canary))?;
        let retained = directory.join("retained");
        let raw = directory.join("raw");
        fs::create_dir(&raw).map_err(|_| "phase control raw unavailable")?;
        let context = TrialContext {
            root: &root,
            python,
            owned: &owned,
            environment: &environment,
            fixture: &fixture,
            name: "first",
            sources: &frozen["provider_sources"],
            runtime: &runtime["observer"],
            raw_dir: &raw,
            evidence: Some(&retained),
            driver: None,
            omit_driver_provenance: false,
            fault: None,
            observer_root: None,
            source_import: None,
            diagnostic_extra: None,
        };
        let case = &fixture["cases"][case_index];
        let spec = &case["phases"][0];
        let result = phase(&context, case, spec);
        let category = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.as_str())
            .to_owned();
        let execution = strict_json(
            &fs::read(directory.join("execution.json"))
                .map_err(|_| "phase control execution missing")?,
        )?;
        let observation = &execution["observation"];
        let inventory = strict_json(
            &fs::read(directory.join("protocol-inventory.json"))
                .map_err(|_| "phase control inventory missing")?,
        )?;
        shared::write_receipt(
            &json!({"control":name,"category":if category.contains(canary){None}else{Some(category.as_str())},"category_sha256":shared::digest(category.as_bytes()),"expected":expected,"execution":execution,"inventory":inventory}),
            &directory.join("result.json"),
        )?;
        if category != expected
            || execution["command_matches"] != true
            || ["wait_observed", "reaped", "cleanup_succeeded"]
                .iter()
                .any(|flag| observation[flag] != true)
        {
            return Err("phase control outcome or cleanup differs".into());
        }
        let pid = observation["pid"]
            .as_u64()
            .and_then(|pid| i32::try_from(pid).ok())
            .ok_or("phase control PID invalid")?;
        verify_phase_reaped_absent(observation)?;
        let after = shared::files(&owned, &owned, Some(canary))?;
        if fault == "none" {
            let result = result?;
            verify_report_contract(
                &result["report"],
                "first",
                case,
                spec,
                pid as u32,
                &root,
                &frozen["provider_sources"],
            )?;
            verify_delta(&owned, &before, &after, spec)?;
        } else {
            if before != after
                || inventory["expected"].as_array().is_none_or(|entries| {
                    entries.len() != 2 || entries.iter().any(|entry| entry["state"] != "absent")
                })
            {
                return Err("phase fault effects or protocol absence differs".into());
            }
            if fault == "hang" {
                if observation["wait_signal"] != libc::SIGKILL
                    || !observation["wait_code"].is_null()
                    || execution["primary_failure"]["cause"] != "Deadline"
                {
                    return Err("phase deadline evidence differs".into());
                }
            } else if observation["wait_code"] != if fault == "nonzero" { 17 } else { 0 }
                || !observation["wait_signal"].is_null()
            {
                return Err("phase fault exit differs".into());
            }
        }
        shared::write_receipt(
            &json!({"before":before,"after":after,"scope":"retained owned changes only"}),
            &directory.join("snapshots.json"),
        )?;
        outcomes.push(json!({"control":name,"category":category,"observation":observation,"os_reap_absence_verified":true}));
    }
    if outcomes
        .iter()
        .filter_map(|value| value["observation"]["pid"].as_u64())
        .collect::<BTreeSet<_>>()
        .len()
        != 7
        || observed_inputs(&root, python, &runtime)? != frozen
    {
        return Err("phase control identities or inputs changed".into());
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-phase-owner-controls-v1","outcomes":outcomes,"actual_api_phase_processes":7,"runtime_bootstrap_preparation_processes":1,"scope":"seven phase outcomes; existing three identity outcomes separate; no full corpus qualification"}),
        &output.join("results.json"),
    )
}

#[cfg(test)]
fn verify_phase_reaped_absent(observation: &Value) -> Result<(), String> {
    if ["wait_observed", "reaped", "cleanup_succeeded"]
        .iter()
        .any(|flag| observation[flag] != true)
    {
        return Err("phase control cleanup unconfirmed".into());
    }
    let pid = observation["pid"]
        .as_u64()
        .and_then(|pid| i32::try_from(pid).ok())
        .filter(|pid| *pid > 0)
        .ok_or("phase control PID invalid")?;
    let mut status = 0;
    if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } != -1
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD)
    {
        return Err("phase control direct child not reaped".into());
    }
    // Signal zero observes existence; only the shared owner may terminate a child.
    if unsafe { libc::kill(pid, 0) } != -1
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
    {
        return Err("phase control PID still exists".into());
    }
    Ok(())
}

#[cfg(test)]
fn validate_phase_cohort(identity: &[Value], phases: &[Value]) -> Result<(), String> {
    if identity.len() != 3 || phases.len() != 7 {
        return Err("phase cohort count differs".into());
    }
    let mut pids = BTreeSet::new();
    for row in identity.iter().chain(phases) {
        let pid = row["observation"]["pid"]
            .as_u64()
            .filter(|pid| *pid > 0 && *pid <= i32::MAX as u64)
            .ok_or("phase cohort PID missing or invalid")?;
        if !pids.insert(pid)
            || row["os_reap_absence_verified"] != true
            || ["wait_observed", "reaped", "cleanup_succeeded"]
                .iter()
                .any(|flag| row["observation"][flag] != true)
        {
            return Err("phase cohort PID proof differs".into());
        }
    }
    Ok(())
}

#[cfg(test)]
struct ContractContext<'a> {
    root: &'a Path,
    python: &'a Path,
    fixture: &'a Value,
    inputs: &'a Value,
    runtime: &'a Value,
}
#[cfg(test)]
struct ContractScope {
    _temporary: tempfile::TempDir,
    owned: std::path::PathBuf,
    environment: BTreeMap<String, String>,
}
#[cfg(test)]
impl ContractScope {
    fn new(context: &ContractContext<'_>) -> Result<Self, String> {
        let temporary = tempfile::tempdir().map_err(|_| "control scope unavailable")?;
        let owned = temporary
            .path()
            .canonicalize()
            .map_err(|_| "control root unavailable")?
            .join("owned");
        let environment = prepare(context.root, &owned, context.fixture)?;
        Ok(Self {
            _temporary: temporary,
            owned,
            environment,
        })
    }
}
#[cfg(test)]
struct ContractPhase {
    case: usize,
    phase: usize,
    fault: Option<ContractFault>,
    directory: std::path::PathBuf,
}
#[cfg(test)]
struct ContractObserved {
    result: Result<Value, String>,
    record: Value,
}
#[cfg(test)]
fn private_directory(path: &Path) -> Result<(), String> {
    fs::create_dir(path).map_err(|_| "control evidence directory unavailable")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|_| "control evidence directory mode unavailable".to_owned())
}
#[cfg(test)]
fn retained_protocol(directory: &Path, name: &str) -> Result<Option<Value>, String> {
    let path = directory.join("retained").join(name);
    match fs::symlink_metadata(&path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("control retained protocol metadata unavailable".into()),
        Ok(meta) => {
            if !meta.file_type().is_file() {
                return Err("control retained protocol kind differs".into());
            }
            Ok(Some(strict_json(
                &protocol_bytes(&path).map_err(|_| "control retained protocol unreadable")?,
            )?))
        }
    }
}
#[cfg(test)]
fn control_proof<T>(
    primary: &Result<Value, String>,
    proof: Result<T, String>,
    secondary: impl FnOnce(&Value) -> Result<(), String>,
) -> Result<T, String> {
    match proof {
        Ok(value) => Ok(value),
        Err(error) => {
            let _ = secondary(
                &json!({"category":"control proof unavailable","secondary_sha256":shared::digest(error.as_bytes()),"qualification":"refused"}),
            );
            Err(primary.as_ref().err().cloned().unwrap_or(error))
        }
    }
}
#[cfg(test)]
fn mutation_process_proof(records: &[&Value]) -> Result<(), String> {
    if records.len() != 3 {
        return Err("mutation process count differs".into());
    }
    let mut pids = BTreeSet::new();
    for row in records {
        let owner = &row["execution"]["observation"];
        let pid = owner["pid"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= i32::MAX as u64)
            .ok_or("mutation PID invalid")?;
        if !pids.insert(pid)
            || row["os_reap_absence_verified"] != true
            || ["wait_observed", "reaped", "cleanup_succeeded"]
                .iter()
                .any(|flag| owner[flag] != true)
        {
            return Err("mutation process proof differs".into());
        }
    }
    Ok(())
}
#[cfg(test)]
fn mutation_schedule(
    fault: ContractFault,
) -> Vec<(usize, usize, Option<ContractFault>, &'static str)> {
    let index = if matches!(
        fault,
        ContractFault::YamlDateMap | ContractFault::YamlMapDate
    ) {
        3
    } else {
        2
    };
    vec![
        (1, 0, None, "preparation"),
        (index, 0, Some(fault), "writer"),
        (index, 1, None, "reader"),
    ]
}
#[cfg(test)]
fn contract_phase(
    context: &ContractContext<'_>,
    scope: &ContractScope,
    plan: &ContractPhase,
) -> Result<ContractObserved, String> {
    private_directory(&plan.directory)?;
    let raw = plan.directory.join("raw");
    private_directory(&raw)?;
    let retained = plan.directory.join("retained");
    let case = &context.fixture["cases"][plan.case];
    let spec = &case["phases"][plan.phase];
    let id = case["case"].as_str().ok_or("control case missing")?;
    let role = spec["role"].as_str().ok_or("control role missing")?;
    let canary = context.fixture["canary"]
        .as_str()
        .ok_or("control canary missing")?;
    let before = shared::files(&scope.owned, &scope.owned, Some(canary))?;
    let trial = TrialContext {
        root: context.root,
        python: context.python,
        owned: &scope.owned,
        environment: &scope.environment,
        fixture: context.fixture,
        name: "first",
        sources: &context.inputs["provider_sources"],
        runtime: &context.runtime["observer"],
        raw_dir: &raw,
        evidence: Some(&retained),
        driver: None,
        omit_driver_provenance: false,
        fault: plan.fault,
        observer_root: None,
        source_import: None,
        diagnostic_extra: None,
    };
    let result = phase(&trial, case, spec);
    let proof = (|| -> Result<Value, String> {
        let execution = strict_json(
            &fs::read(plan.directory.join("execution.json"))
                .map_err(|_| "control execution receipt missing")?,
        )?;
        verify_phase_reaped_absent(&execution["observation"])?;
        if execution["command_matches"] != true {
            return Err("control actual command differs".into());
        }
        let fault = plan
            .fault
            .map(|f| read_fault(&scope.owned, &execution, f, (id, role, "first")))
            .transpose()?;
        let api = retained_protocol(&plan.directory, "api.json")?;
        let observer = retained_protocol(&plan.directory, "observer.json")?;
        let early = matches!(
            plan.fault,
            Some(ContractFault::Nonzero | ContractFault::Hang)
        );
        if early {
            if api.is_some() || observer.is_some() {
                return Err("early fault unexpectedly published API/observer".into());
            }
        } else {
            let observer = observer.as_ref().ok_or("control observer missing")?;
            qualify_phase_observer(observer, context.root, &context.inputs["provider_sources"])?;
            let api = api.as_ref().ok_or("control API proof missing")?;
            let pid = execution["observation"]["pid"]
                .as_u64()
                .and_then(|p| u32::try_from(p).ok())
                .ok_or("control PID invalid")?;
            verify_driver_provenance(api, pid, context.root)?;
            let checked = verify_current_report_structure(
                api,
                "first",
                case,
                spec,
                pid,
                context.root,
                &context.inputs["provider_sources"],
            );
            if plan.fault == Some(ContractFault::Empty) {
                if api["api_calls"]
                    .as_array()
                    .is_none_or(|calls| !calls.is_empty())
                    || checked.as_ref().err().map(String::as_str)
                        != Some("data API evidence missing")
                {
                    return Err("empty original API ledger not observed".into());
                }
            } else {
                checked?;
            }
        }
        let after = if plan.fault == Some(ContractFault::Plaintext) {
            let target = scope
                .owned
                .join(spec["target"].as_str().ok_or("plaintext target missing")?);
            let raw = protocol_bytes(&target).map_err(|_| "plaintext target unreadable")?;
            if raw != canary.as_bytes() {
                return Err("declared plaintext canary not observed".into());
            }
            json!({"scan":"plaintext-refused-before-target-hash","bytes":raw.len()})
        } else {
            let after = shared::files(&scope.owned, &scope.owned, Some(canary))?;
            let mut expected = effect_spec(spec, plan.fault, id, role);
            if early || plan.fault == Some(ContractFault::Empty) {
                expected["allowed_retained_delta"] = json!([]);
                if early {
                    expected["allowed_instrumentation_delta"] = json!([{"operation":"add","entry":{"path":fault_relative(id,role),"mode":"0600","content_rule":"fault protocol"}}]);
                }
            }
            let effects = verify_delta(&scope.owned, &before, &after, &expected);
            if matches!(
                plan.fault,
                Some(ContractFault::ExtraWrite | ContractFault::Rewrite)
            ) {
                if effects.as_ref().err().map(String::as_str) != Some("unexpected retained change")
                {
                    return Err("unexpected actual write not observed".into());
                }
            } else {
                effects?;
            }
            json!(after)
        };
        let category = result
            .as_ref()
            .map(|_| "accepted")
            .unwrap_or_else(|error| error.as_str());
        if category.contains(canary) {
            return Err("control diagnostic privacy refusal".into());
        }
        let record = json!({"case":id,"role":role,"control":plan.fault.map(ContractFault::id),"category":category,"execution":execution,"fault":fault,"api":api,"observer":observer,"before":before,"after":after,"os_reap_absence_verified":true,"scope":"real original phase and retained owned evidence only"});
        shared::write_receipt(&record, &plan.directory.join("judged.json"))?;
        Ok(record)
    })();
    let record = control_proof(&result, proof, |diagnostic| {
        shared::write_receipt(
            diagnostic,
            &plan.directory.join("control-proof-refusal.json"),
        )
    })?;
    Ok(ContractObserved { result, record })
}
#[cfg(test)]
fn require_refusal(observed: &ContractObserved, expected: &str) -> Result<(), String> {
    if observed.result.as_ref().err().map(String::as_str) != Some(expected) {
        return Err(format!("control expected exact refusal: {expected}"));
    }
    Ok(())
}
#[cfg(test)]
fn expected_mutation(expected: &Value, fault: ContractFault) -> Result<Value, String> {
    let mut value = expected.clone();
    let key = match fault {
        ContractFault::JsonOne => "one",
        ContractFault::JsonBool => "truth",
        ContractFault::JsonLarge => "large",
        ContractFault::JsonZero => "zeros",
        ContractFault::YamlDateMap => "date",
        ContractFault::YamlMapDate => "tag_map",
        _ => return Err("unknown mutation expectation".into()),
    };
    let entry = value["entries"]
        .as_array_mut()
        .ok_or("mutation expected map missing")?
        .iter_mut()
        .find(|entry| entry["key"]["value"] == key)
        .ok_or("mutation expected key missing")?;
    match fault {
        ContractFault::JsonOne => entry["value"] = json!({"kind":"int","decimal":"2"}),
        ContractFault::JsonBool => entry["value"] = json!({"kind":"int","decimal":"1"}),
        ContractFault::JsonLarge => entry["value"]["decimal"] = json!("1208925819614629174706180"),
        ContractFault::JsonZero => entry["value"]["items"][0]["bits"] = json!("0000000000000000"),
        ContractFault::YamlDateMap => {
            entry["value"] = json!({"kind":"map","entries":[{"key":{"kind":"str","encoding":"literal","value":"iso"},"value":{"kind":"str","encoding":"literal","value":"2026-03-01"}},{"key":{"kind":"str","encoding":"literal","value":"kind"},"value":{"kind":"str","encoding":"literal","value":"date"}}]})
        }
        ContractFault::YamlMapDate => entry["value"] = json!({"kind":"date","iso":"2026-03-01"}),
        _ => unreachable!(),
    };
    Ok(value)
}
#[cfg(test)]
fn mutation_pair(
    context: &ContractContext<'_>,
    fault: ContractFault,
    directory: &Path,
) -> Result<Value, String> {
    private_directory(directory)?;
    let scope = ContractScope::new(context)?;
    let schedule = mutation_schedule(fault);
    let mut observed = Vec::new();
    for &(case, phase, fault, label) in &schedule {
        let item = contract_phase(
            context,
            &scope,
            &ContractPhase {
                case,
                phase,
                fault,
                directory: directory.join(label),
            },
        )?;
        if label != "reader" {
            item.result.as_ref().map_err(Clone::clone)?;
        }
        observed.push(item);
    }
    let preparation = &observed[0];
    let writer = &observed[1];
    let reader = &observed[2];
    mutation_process_proof(&[&preparation.record, &writer.record, &reader.record])?;
    let index = schedule[1].0;
    writer.result.as_ref().map_err(Clone::clone)?;
    require_refusal(reader, "data typed return differs")?;
    let expected = expected_mutation(
        &context.fixture["cases"][index]["phases"][1]["expected_return"],
        fault,
    )?;
    if reader.record["api"]["api_calls"][0]["returned"] != expected {
        return Err("actual mutation differs from independent expected node".into());
    }
    if writer.record["execution"]["observation"]["pid"]
        == reader.record["execution"]["observation"]["pid"]
    {
        return Err("mutation reader did not use a fresh process".into());
    }
    Ok(
        json!({"control":fault.id(),"preparation":preparation.record,"writer":writer.record,"reader":reader.record,"counts":{"preparation_processes":1,"mutation_processes":2},"scope":"preparation excluded from positive corpus and mutation writer/reader counts"}),
    )
}
#[cfg(test)]
fn completed_trial_shape(trials: &[Value]) -> Result<Value, String> {
    if trials.len() != 2 {
        return Err("two completed trials required".into());
    }
    let mut pids = BTreeSet::new();
    let mut calls = 0;
    let mut deferred = 0;
    for trial in trials {
        let phases = trial["phases"]
            .as_array()
            .filter(|phases| phases.len() == 19)
            .ok_or("trial did not complete19phases")?;
        let mut trial_calls = 0;
        for (index, phase) in phases.iter().enumerate() {
            let golden = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE)?;
            if phase["case"] != golden[index]["case"] || phase["role"] != golden[index]["role"] {
                return Err("completed trial phase order differs".into());
            }
            for difference in phase["deferred_differences"]
                .as_array()
                .ok_or("completed differences missing")?
            {
                keys(difference, &["purpose", "dimension"])?;
                let dimension = match difference["dimension"].as_str() {
                    Some("typed-return") => ReturnDimension::Typed,
                    Some("serialized-bytes") => ReturnDimension::Serialized,
                    _ => return Err("unregistered deferred dimension".into()),
                };
                let named = ReturnDifference {
                    purpose: difference["purpose"]
                        .as_str()
                        .ok_or("deferred purpose missing")?
                        .into(),
                    dimension,
                };
                if !may_defer(
                    Some(ContractFault::SameWrong),
                    phase["case"].as_str().ok_or("deferred case missing")?,
                    phase["role"].as_str().ok_or("deferred role missing")?,
                    &named,
                ) {
                    return Err("unregistered deferred phase".into());
                }
            }
            let o = &phase["owner_observation"];
            if o["wait_code"] != 0
                || !o["wait_signal"].is_null()
                || ["wait_observed", "reaped", "cleanup_succeeded"]
                    .iter()
                    .any(|k| o[k] != true)
            {
                return Err("completed trial lifecycle differs".into());
            }
            let pid = o["pid"]
                .as_u64()
                .filter(|pid| *pid > 0)
                .ok_or("completed trial PID missing")?;
            if !pids.insert(pid) {
                return Err("completed trial reused a phase PID".into());
            }
            trial_calls += phase["report"]["api_calls"]
                .as_array()
                .ok_or("completed API calls missing")?
                .len();
            deferred += phase["deferred_differences"].as_array().map_or(0, Vec::len);
        }
        if trial_calls != 25 {
            return Err("completed trial direct calls differ".into());
        }
        calls += trial_calls;
    }
    if pids.len() != 38 || calls != 50 {
        return Err("two-trial completion proof differs".into());
    }
    Ok(
        json!({"trials":2,"cases":20,"phases":38,"direct_calls":50,"distinct_phase_pids":pids,"deferred_dimensions":deferred}),
    )
}
#[cfg(test)]
fn complete_trials(trials: &[Value]) -> Result<Value, String> {
    let counts = completed_trial_shape(trials)?;
    for trial in trials {
        for phase in trial["phases"]
            .as_array()
            .ok_or("completed phases missing")?
        {
            verify_phase_reaped_absent(&phase["owner_observation"])?;
        }
    }
    Ok(counts)
}

#[cfg(test)]
fn same_wrong_trials(context: &ContractContext<'_>, directory: &Path) -> Result<Value, String> {
    private_directory(directory)?;
    let first = trial(
        context.root,
        context.python,
        context.fixture,
        "first",
        &context.inputs["provider_sources"],
        &directory.join("first"),
        (&context.runtime["observer"], Some(ContractFault::SameWrong)),
    )?;
    let second = trial(
        context.root,
        context.python,
        context.fixture,
        "second",
        &context.inputs["provider_sources"],
        &directory.join("second"),
        (&context.runtime["observer"], Some(ContractFault::SameWrong)),
    )?;
    let trials = [first, second];
    let completion = complete_trials(&trials)?;
    shared::write_receipt(
        &json!({"completion":completion,"trials":trials,"scope":"both actual corpora complete before semantic judgment"}),
        &directory.join("completed.json"),
    )?;
    for trial in &trials {
        for phase in trial["phases"]
            .as_array()
            .ok_or("same-wrong phases missing")?
        {
            if matches!(phase["case"].as_str(), Some("3" | "9")) && phase["role"] == "reader" {
                let index = if phase["case"] == "3" { 2 } else { 8 };
                let expected = expected_mutation(
                    &context.fixture["cases"][index]["phases"][1]["expected_return"],
                    ContractFault::JsonOne,
                )?;
                if phase["report"]["api_calls"][0]["returned"] != expected {
                    return Err("same-wrong actual typed mutation differs".into());
                }
            }
        }
    }
    let left = projection(&trials[0])?;
    let right = projection(&trials[1])?;
    if left != right
        || completion["deferred_dimensions"] != 8
        || compare_current_data_semantics(&left, &right)
            .as_ref()
            .err()
            .map(String::as_str)
            != Some("semantic_reference_mismatch")
    {
        return Err("same-wrong did not produce two completed equal wrong corpora".into());
    }
    Ok(json!({"judgment":"semantic_reference_mismatch","completion":completion}))
}
#[cfg(test)]
fn simple_fault(
    context: &ContractContext<'_>,
    fault: ContractFault,
    reader: bool,
    expected: &str,
    directory: &Path,
) -> Result<Value, String> {
    let scope = ContractScope::new(context)?;
    let preparation = if fault == ContractFault::Rewrite {
        let setup = contract_phase(
            context,
            &scope,
            &ContractPhase {
                case: 1,
                phase: 0,
                fault: None,
                directory: directory.with_extension("prepare"),
            },
        )?;
        setup.result?;
        Some(setup.record)
    } else {
        None
    };
    let observed = contract_phase(
        context,
        &scope,
        &ContractPhase {
            case: if reader {
                0
            } else if fault == ContractFault::Rewrite {
                7
            } else {
                1
            },
            phase: 0,
            fault: Some(fault),
            directory: directory.to_owned(),
        },
    )?;
    require_refusal(&observed, expected)?;
    let o = &observed.record["execution"]["observation"];
    if fault == ContractFault::Nonzero {
        if o["wait_code"] != 17 || !o["wait_signal"].is_null() {
            return Err("nonzero exact wait differs".into());
        }
    } else if fault == ContractFault::Hang {
        if o["wait_signal"] != libc::SIGKILL
            || !o["wait_code"].is_null()
            || observed.record["execution"]["primary_failure"]["cause"] != "Deadline"
        {
            return Err("hang typed deadline evidence differs".into());
        }
    } else if o["wait_code"] != 0 || !o["wait_signal"].is_null() {
        return Err("fault zero wait differs".into());
    }
    if matches!(fault, ContractFault::Stdout | ContractFault::Stderr) {
        let channel = if fault == ContractFault::Stdout {
            "stdout"
        } else {
            "stderr"
        };
        let other = if channel == "stdout" {
            "stderr"
        } else {
            "stdout"
        };
        let canary = context.fixture["canary"]
            .as_str()
            .ok_or("control canary missing")?;
        if observed.record["execution"][format!("{channel}_bytes")] != canary.len()
            || observed.record["execution"][format!("{channel}_sha256")]
                != shared::digest(canary.as_bytes())
            || observed.record["execution"][format!("{other}_bytes")] != 0
        {
            return Err("actual canary stream channel differs".into());
        }
    }
    if fault == ContractFault::Rewrite {
        let proof = strict_json(
            &fs::read(directory.join("no-op-proof.json"))
                .map_err(|_| "actual no-op proof missing")?,
        )?;
        if proof["before"]["inode"] == proof["after"]["inode"]
            || proof["before"]["ciphertext_sha256"] == proof["after"]["ciphertext_sha256"]
            || (proof["before"]["mtime_seconds"] == proof["after"]["mtime_seconds"]
                && proof["before"]["mtime_nanoseconds"] == proof["after"]["mtime_nanoseconds"])
        {
            return Err("original no-op rewrite identity proof differs".into());
        }
        let fresh = contract_phase(
            context,
            &scope,
            &ContractPhase {
                case: 7,
                phase: 1,
                fault: None,
                directory: directory.with_extension("reader"),
            },
        )?;
        fresh.result?;
        if fresh.record["execution"]["observation"]["pid"] == o["pid"] {
            return Err("rewrite reader not fresh".into());
        }
        let prepared = preparation.as_ref().ok_or("rewrite preparation missing")?;
        unique_control_pids(&[
            prepared.clone(),
            observed.record.clone(),
            fresh.record.clone(),
        ])?;
        return Ok(
            json!({"preparation":preparation,"writer":observed.record,"fresh_reader":fresh.record,"identity":proof}),
        );
    }
    Ok(observed.record)
}

#[cfg(test)]
fn source_copy_comparison(
    expected: &Value,
    actual: &Value,
    write: impl FnOnce(&Value) -> Result<(), String>,
) -> Result<(), String> {
    let mut differences = Vec::new();
    let before = expected.as_object().ok_or("source expected map missing")?;
    let after = actual.as_object().ok_or("source actual map missing")?;
    for (path, value) in before {
        match after.get(path) {
            None => differences.push(json!({"path":path,"field":"missing"})),
            Some(observed) if observed != value => {
                let original = value.as_str().ok_or("source expected binding invalid")?;
                let current = observed.as_str().ok_or("source actual binding invalid")?;
                let original: Vec<_> = original.split(':').collect();
                let current: Vec<_> = current.split(':').collect();
                if original.len() == 3 && current.len() == 3 {
                    if original[1] != current[1] {
                        differences.push(json!({"path":path,"field":"mode","expected":original[1],"actual":current[1]}));
                    }
                    if original[2] != current[2] {
                        differences.push(json!({"path":path,"field":"sha256","expected":original[2],"actual":current[2]}));
                    }
                } else {
                    differences.push(
                        json!({"path":path,"field":"sha256","expected":value,"actual":observed}),
                    );
                }
            }
            _ => {}
        }
    }
    for path in after.keys().filter(|path| !before.contains_key(*path)) {
        differences.push(json!({"path_sha256":shared::digest(path.as_bytes()),"field":"extra"}));
    }
    let safe_actual: serde_json::Map<String, Value> = after
        .iter()
        .filter(|(path, _)| before.contains_key(*path))
        .map(|(path, value)| (path.clone(), value.clone()))
        .collect();
    let written = write(
        &json!({"schema":"careerbrew-data-copy-comparison-v1","expected":expected,"actual":safe_actual,"differences":differences}),
    );
    if actual != expected {
        return Err("copied pre-mutation source does not equal frozen original".into());
    }
    written
}
#[cfg(test)]
fn copy_file_fact(root: &Path, relative: &str) -> Result<Value, String> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt, path::Component};
    let mut path = root.to_owned();
    for component in Path::new(relative).components() {
        let Component::Normal(component) = component else {
            return Err("source path is not lexical owned-relative".into());
        };
        path.push(component);
        if fs::symlink_metadata(&path)
            .map_err(|_| "source metadata unavailable")?
            .file_type()
            .is_symlink()
        {
            return Err("source symlink refused".into());
        }
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(&path)
        .map_err(|_| "source file unavailable")?;
    let metadata = file
        .metadata()
        .map_err(|_| "source opened metadata unavailable")?;
    if !metadata.is_file() {
        return Err("source nonregular refused".into());
    }
    let mut bytes = Vec::new();
    file.take((LIMIT + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|_| "source read unavailable")?;
    if bytes.len() > LIMIT {
        return Err("source file exceeds limit".into());
    }
    Ok(
        json!({"state":"regular","mode":format!("{:04o}",metadata.permissions().mode()&0o777),"special_bits":metadata.permissions().mode()&0o7000,"bytes":bytes.len(),"sha256":shared::digest(&bytes)}),
    )
}
#[cfg(test)]
fn copy_source_names(
    root: &Path,
    folder: &Path,
    expected: &Value,
    unknown: &mut Vec<Value>,
    depth: usize,
) -> Result<(), String> {
    if depth > 64 {
        return Err("source directory depth exceeds limit".into());
    }
    if fs::symlink_metadata(folder)
        .map_err(|_| "source directory unavailable")?
        .file_type()
        .is_symlink()
    {
        return Err("source parent symlink refused".into());
    }
    for entry in fs::read_dir(folder).map_err(|_| "source names unavailable")? {
        let path = entry.map_err(|_| "source name unavailable")?.path();
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "source name escaped copy")?
            .to_string_lossy()
            .into_owned();
        let metadata =
            fs::symlink_metadata(&path).map_err(|_| "source name metadata unavailable")?;
        if metadata.is_dir() {
            copy_source_names(root, &path, expected, unknown, depth + 1)?;
        } else if metadata.file_type().is_symlink()
            || (relative.ends_with(".py") && expected.get(&relative).is_none())
        {
            unknown.push(json!({"path_sha256":shared::digest(relative.as_bytes()),"category":if metadata.file_type().is_symlink(){"symlink"}else{"unknown-source"}}));
        }
        if unknown.len() > 1024 {
            return Err("source unknown-name count exceeds limit".into());
        }
    }
    Ok(())
}
#[cfg(test)]
fn copy_source_snapshot(root: &Path, expected: &Value) -> Result<Value, String> {
    let mut entries = serde_json::Map::new();
    let mut bindings = serde_json::Map::new();
    for (path, binding) in expected.as_object().ok_or("frozen sources missing")? {
        match copy_file_fact(root, path) {
            Ok(fact) => {
                let hash = fact["sha256"].as_str().ok_or("source hash missing")?;
                let value = if binding.as_str().is_some_and(|s| s.starts_with("file:")) {
                    format!(
                        "file:{}:{hash}",
                        fact["mode"]
                            .as_str()
                            .ok_or("source mode missing")?
                            .trim_start_matches('0')
                    )
                } else {
                    hash.to_owned()
                };
                bindings.insert(path.clone(), json!(value));
                entries.insert(path.clone(), fact);
            }
            Err(error) => {
                entries.insert(path.clone(), json!({"state":"refused","category":error}));
            }
        }
    }
    let mut unknown = Vec::new();
    if let Err(error) = copy_source_names(
        root,
        &root.join("src/careerbrew"),
        expected,
        &mut unknown,
        0,
    ) {
        unknown.push(json!({"category":error,"scope":"source-name enumeration incomplete; no inferred content"}));
    }
    let forbidden: Vec<_> = [".venv", ".metrics", ".git"]
        .iter()
        .filter(|name| fs::symlink_metadata(root.join(name)).is_ok())
        .map(|name| json!({"category":"excluded-runtime-history","name":name}))
        .collect();
    Ok(json!({"entries":entries,"bindings":bindings,"unknown":unknown,"forbidden":forbidden}))
}
#[cfg(test)]
fn persist_source_copy(
    expected: &Value,
    actual: &Value,
    write_actual: impl FnOnce(&Value) -> Result<(), String>,
    write_comparison: impl FnOnce(&Value) -> Result<(), String>,
) -> Result<(), String> {
    let retained = write_actual(actual);
    let comparison = source_copy_comparison(expected, &actual["bindings"], write_comparison);
    if expected != &actual["bindings"] {
        return comparison;
    }
    if actual["unknown"].as_array().is_none_or(|v| !v.is_empty())
        || actual["forbidden"].as_array().is_none_or(|v| !v.is_empty())
    {
        return Err("source copy unknown or excluded entry".into());
    }
    retained?;
    comparison
}
#[cfg(test)]
fn inspect_copy_after_owner<T>(
    confirmed: bool,
    inspect: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if !confirmed {
        return Err("source copy owner cleanup unconfirmed".into());
    }
    inspect()
}
#[cfg(test)]
struct SourceCopyTool {
    program: &'static str,
    args: Vec<std::ffi::OsString>,
    cwd: std::path::PathBuf,
}
#[cfg(test)]
fn source_copy_tools(
    root: &Path,
    base: &Path,
    copied: &Path,
    archive: &Path,
    tree: &str,
) -> [SourceCopyTool; 4] {
    use std::ffi::OsString;
    let mut output = OsString::from("--output=");
    output.push(archive);
    [
        SourceCopyTool {
            program: "/usr/bin/git",
            args: vec!["write-tree".into()],
            cwd: root.to_owned(),
        },
        SourceCopyTool {
            program: "/usr/bin/git",
            args: vec![
                "ls-tree".into(),
                "-rz".into(),
                "--full-tree".into(),
                tree.into(),
            ],
            cwd: root.to_owned(),
        },
        SourceCopyTool {
            program: "/usr/bin/git",
            args: vec!["archive".into(), "--format=tar".into(), output, tree.into()],
            cwd: root.to_owned(),
        },
        SourceCopyTool {
            program: "/usr/bin/tar",
            args: vec!["-xf".into(), archive.into(), "-C".into(), copied.into()],
            cwd: base.to_owned(),
        },
    ]
}
#[cfg(test)]
fn execute_copy_tool(
    tool: SourceCopyTool,
    env: &BTreeMap<String, String>,
    receipt: &Path,
) -> Result<(shared::OwnerObservation, String), String> {
    let mut command = Command::new(tool.program);
    command
        .args(tool.args)
        .current_dir(tool.cwd)
        .env_clear()
        .envs(env);
    execute_setup_output(command, receipt, false)
}
#[cfg(test)]
fn source_copy_index(raw: &str, expected: &Value) -> Result<Value, String> {
    let mut known = serde_json::Map::new();
    let mut extra = Vec::new();
    for row in raw.split('\0').filter(|s| !s.is_empty()) {
        let (header, path) = row.split_once('\t').ok_or("source index framing invalid")?;
        let fields: Vec<_> = header.split(' ').collect();
        if fields.len() != 3
            || fields[1] != "blob"
            || fields[2].len() != 40
            || !fields[2]
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            return Err("source index entry invalid".into());
        }
        if expected.get(path).is_some() {
            if !["100644", "100755"].contains(&fields[0])
                || known
                    .insert(path.to_owned(), json!({"mode":fields[0],"blob":fields[2]}))
                    .is_some()
            {
                return Err("source index regular mode or duplicate differs".into());
            }
        } else if path.starts_with("src/careerbrew/") && path.ends_with(".py") {
            extra.push(json!({"path_sha256":shared::digest(path.as_bytes()),"category":"unfrozen-index-source"}));
        }
    }
    if known.len()
        != expected
            .as_object()
            .ok_or("source index expected missing")?
            .len()
        || !extra.is_empty()
    {
        return Err("source index keyset differs".into());
    }
    Ok(json!({"known":known,"extra":extra}))
}
#[cfg(test)]
struct CopiedModeTarget {
    path: String,
    file: fs::File,
    mode: u32,
    before: u32,
}
#[cfg(test)]
fn validate_copied_mode_targets(
    copied: &Path,
    expected: &Value,
    original: &Value,
    raw: &Value,
) -> Result<Vec<CopiedModeTarget>, String> {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
        path::Component,
    };
    let root = fs::symlink_metadata(copied).map_err(|_| "source copy root unavailable")?;
    if !root.is_dir() || root.permissions().mode() & 0o777 != 0o700 {
        return Err("source mode target root not private directory".into());
    }
    let entries = expected
        .as_object()
        .ok_or("source frozen bindings missing")?;
    if raw["unknown"].as_array().is_none_or(|v| !v.is_empty())
        || raw["forbidden"].as_array().is_none_or(|v| !v.is_empty())
        || raw["entries"]
            .as_object()
            .is_none_or(|m| m.len() != entries.len())
    {
        return Err("source mode target inventory differs".into());
    }
    if original["bindings"] != *expected
        || original["entries"]
            .as_object()
            .is_none_or(|m| m.len() != entries.len())
    {
        return Err("source frozen original binding differs".into());
    }
    let mut targets = Vec::new();
    for (relative, binding) in entries {
        let frozen = &original["entries"][relative];
        let extracted = &raw["entries"][relative];
        let mode = frozen["mode"]
            .as_str()
            .filter(|s| {
                s.len() == 4 && s.starts_with('0') && s.bytes().all(|b| (b'0'..=b'7').contains(&b))
            })
            .ok_or("source frozen ordinary mode invalid")?;
        let mode = u32::from_str_radix(mode, 8).map_err(|_| "source frozen mode invalid")?;
        if frozen["special_bits"] != 0
            || extracted["special_bits"] != 0
            || frozen["state"] != "regular"
            || extracted["state"] != "regular"
        {
            return Err("source mode special or nonregular file refused".into());
        }
        let hash = frozen["sha256"]
            .as_str()
            .filter(|s| {
                s.len() == 64
                    && s.bytes()
                        .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            })
            .ok_or("source frozen hash invalid")?;
        let wanted = if binding.as_str().is_some_and(|s| s.starts_with("file:")) {
            format!("file:{mode:o}:{hash}")
        } else {
            hash.to_owned()
        };
        if *binding != wanted
            || extracted["sha256"] != hash
            || extracted["bytes"] != frozen["bytes"]
        {
            return Err("source copied bytes or frozen mode binding differs".into());
        }
        let mut path = copied.to_owned();
        for component in Path::new(relative).components() {
            let Component::Normal(part) = component else {
                return Err("source mode path invalid".into());
            };
            path.push(part);
            if fs::symlink_metadata(&path)
                .map_err(|_| "source mode metadata unavailable")?
                .file_type()
                .is_symlink()
            {
                return Err("source mode symlink refused".into());
            }
        }
        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(path)
            .map_err(|_| "source mode file unavailable")?;
        let metadata = file
            .metadata()
            .map_err(|_| "source mode opened metadata unavailable")?;
        if !metadata.is_file()
            || metadata.permissions().mode() & 0o7000 != 0
            || metadata.nlink() != 1
        {
            return Err("source mode regular unaliased file required".into());
        }
        let mut bytes = Vec::new();
        (&file)
            .take((LIMIT + 1) as u64)
            .read_to_end(&mut bytes)
            .map_err(|_| "source mode file read unavailable")?;
        if bytes.len() > LIMIT || shared::digest(&bytes) != hash || frozen["bytes"] != bytes.len() {
            return Err("source mode opened bytes differ".into());
        }
        targets.push(CopiedModeTarget {
            path: relative.clone(),
            file,
            mode,
            before: metadata.permissions().mode() & 0o777,
        });
    }
    Ok(targets)
}
#[cfg(test)]
fn restore_source_modes(
    copied: &Path,
    expected: &Value,
    original: &Value,
    raw: &Value,
    mut write: impl FnMut(&str, &Value) -> Result<(), String>,
) -> Result<Value, String> {
    let prepared = validate_copied_mode_targets(copied, expected, original, raw);
    let retained = write("raw", raw);
    let targets = prepared?;
    retained?;
    let mut actions = Vec::new();
    for target in targets {
        if target.before != target.mode
            && target
                .file
                .set_permissions(fs::Permissions::from_mode(target.mode))
                .is_err()
        {
            let _ = write(
                "actions",
                &json!({"actions":actions,"category":"source copied mode change failed"}),
            );
            return Err("source copied mode change failed".into());
        }
        actions.push(json!({"path":target.path,"before":format!("{:04o}",target.before),"mode":format!("{:04o}",target.mode),"changed":target.before!=target.mode,"completed":true}));
    }
    let restored = copy_source_snapshot(copied, expected)?;
    let primary = source_copy_comparison(expected, &restored["bindings"], |_| Ok(()));
    let actions_written = write(
        "actions",
        &json!({"scope":"constructor copied-file modes only; no loader normalization","actions":actions}),
    );
    let restored_written = write("restored", &restored);
    primary?;
    actions_written?;
    restored_written?;
    Ok(restored)
}
#[cfg(test)]
fn trusted_source_runtime_inventory(runtime: &Value) -> Result<BTreeSet<String>, String> {
    let inventory = &runtime["static_inputs"]["runtime_static"];
    keys(
        inventory,
        &[
            "allowed_files",
            "site_inputs",
            "interpreter_search_paths",
            "scope",
        ],
    )?;
    if !inventory["site_inputs"].is_object()
        || !inventory["interpreter_search_paths"].is_array()
        || !inventory["scope"].is_string()
    {
        return Err("trusted runtime inventory schema invalid".into());
    }
    let files = inventory["allowed_files"]
        .as_array()
        .filter(|v| !v.is_empty() && v.len() <= 16384)
        .ok_or("trusted runtime file inventory missing")?;
    let mut allowed = BTreeSet::new();
    for file in files {
        let file = file
            .as_str()
            .filter(|s| {
                s.len() <= 4096
                    && Path::new(s).is_absolute()
                    && !Path::new(s)
                        .components()
                        .any(|c| matches!(c, std::path::Component::ParentDir))
            })
            .ok_or("runtime path invalid")?;
        if !allowed.insert(file.to_owned()) {
            return Err("runtime path duplicate".into());
        }
    }
    Ok(allowed)
}

#[cfg(test)]
fn prepare_source_copy(
    root: &Path,
    env: &BTreeMap<String, String>,
    expected: &Value,
    directory: &Path,
    base: &Path,
    copied: &Path,
) -> Result<String, String> {
    let original = copy_source_snapshot(root, expected)?;
    shared::write_receipt(
        &json!({"expected":expected,"original":original,"scope":"frozen before native copy; no post-copy expectation refresh"}),
        &directory.join("copy-expected.json"),
    )?;
    source_copy_comparison(expected, &original["bindings"], |_| Ok(()))?;
    let archive = base.join("source.tar");
    let first = source_copy_tools(root, base, copied, &archive, "")
        .into_iter()
        .next()
        .ok_or("source tool plan empty")?;
    let (_, text) = execute_copy_tool(first, env, &directory.join("tree-command.json"))?;
    let tree = text.trim().to_owned();
    if tree.len() != 40
        || !tree
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("source index tree invalid".into());
    }
    let mut tools = source_copy_tools(root, base, copied, &archive, &tree)
        .into_iter()
        .skip(1);
    let (_, index) = execute_copy_tool(
        tools.next().ok_or("source index tool absent")?,
        env,
        &directory.join("index-command.json"),
    )?;
    let index = source_copy_index(&index, expected)?;
    shared::write_receipt(
        &json!({"tree":tree,"index":index}),
        &directory.join("copy-index.json"),
    )?;
    execute_copy_tool(
        tools.next().ok_or("source archive tool absent")?,
        env,
        &directory.join("archive-command.json"),
    )?;
    let (owner, _) = execute_copy_tool(
        tools.next().ok_or("source unpack tool absent")?,
        env,
        &directory.join("unpack-command.json"),
    )?;
    let actual = inspect_copy_after_owner(
        owner.wait_observed && owner.reaped && owner.cleanup_succeeded,
        || copy_source_snapshot(copied, expected),
    )?;
    let actual = restore_source_modes(copied, expected, &original, &actual, |stage, value| {
        shared::write_receipt(value, &directory.join(format!("copy-mode-{stage}.json")))
    })?;
    persist_source_copy(
        expected,
        &actual,
        |value| shared::write_receipt(value, &directory.join("copy-actual.json")),
        |value| shared::write_receipt(value, &directory.join("copy-comparison.json")),
    )?;
    if shared::sources(copied)? != *expected {
        return Err("copied pre-mutation source does not equal frozen original".into());
    }
    Ok(tree)
}
#[cfg(test)]
fn source_copy_probe_outcome(
    primary: Result<(), String>,
    retained: Result<(), String>,
) -> Result<(), String> {
    primary?;
    retained
}
#[cfg(test)]
pub fn source_copy_metadata_probe(root: &Path, output: &Path) -> Result<(), String> {
    private_directory(output)?;
    let temporary = tempfile::tempdir().map_err(|_| "source probe scope unavailable")?;
    let base = temporary
        .path()
        .canonicalize()
        .map_err(|_| "source probe base unavailable")?;
    let copied = base.join("checkout");
    private_directory(&copied)?;
    let owned = base.join("owned");
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let environment = prepare(root, &owned, &fixture)?;
    let expected = shared::sources(root)?;
    if expected.as_object().is_none_or(|m| m.len() != 138) {
        return Err("source probe expected count differs".into());
    }
    let result = prepare_source_copy(root, &environment, &expected, output, &base, &copied);
    let primary = result.as_ref().err().map(String::as_str);
    if primary.is_some_and(|s| s != "copied pre-mutation source does not equal frozen original") {
        return result.map(|_| ());
    }
    let manifest = strict_json(
        &protocol_bytes(&output.join("copy-comparison.json"))
            .map_err(|_| "source comparison evidence missing")?,
    )?;
    let retained = shared::write_receipt(
        &json!({"schema":"careerbrew-data-copy-probe-v1","primary":primary,"comparison":manifest,"scope":"native source construction only; zero Python/provider calls; equality is not parity"}),
        &output.join("probe-result.json"),
    );
    source_copy_probe_outcome(result.map(|_| ()), retained)
}

#[cfg(test)]
fn source_control(
    context: &ContractContext<'_>,
    directory: &Path,
    changed: bool,
) -> Result<Value, String> {
    private_directory(directory)?;
    let temporary = tempfile::tempdir().map_err(|_| "source copy scope unavailable")?;
    let base = temporary
        .path()
        .canonicalize()
        .map_err(|_| "source copy base unavailable")?;
    let copied = base.join("checkout");
    private_directory(&copied)?;
    let scope = ContractScope::new(context)?;
    let expected = context.inputs["provider_sources"].clone();
    let tree_id = prepare_source_copy(
        context.root,
        &scope.environment,
        &expected,
        directory,
        &base,
        &copied,
    )?;
    let mut diagnostic = BTreeSet::new();
    for relative in expected
        .as_object()
        .ok_or("frozen sources missing")?
        .keys()
        .map(String::as_str)
        .chain([
            "xtask/tests/fixtures/capture/data_driver.py",
            "xtask/tests/fixtures/capture/observer/sitecustomize.py",
        ])
    {
        let path = copied.join(relative);
        let mut current = Some(path.as_path());
        while let Some(path) = current {
            if !path.starts_with(&copied) {
                break;
            }
            diagnostic.insert(path.to_string_lossy().into_owned());
            current = path.parent();
        }
    }
    let original =
        fs::read(copied.join("src/careerbrew/store.py")).map_err(|_| "copied store missing")?;
    let mut intended = original.clone();
    if changed {
        intended.extend_from_slice(b"\n# invocation-owned source mismatch control\n");
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-source-intent-v1","control":if changed{"changed-source"}else{"wrong-root"},"tree":tree_id,"copied_root":copied,"expected_pre_mutation":expected,"original_store_sha256":shared::digest(&original),"intended_executed_store_sha256":shared::digest(&intended),"trusted_runtime_sha256":shared::digest(&serde_json::to_vec(context.runtime).map_err(|_|"runtime encoding failed")?),"scope":"source-only tracked copy; current original interpreter/external runtime; no copied venv/history"}),
        &directory.join("source-intent.json"),
    )?;
    if changed {
        fs::write(copied.join("src/careerbrew/store.py"), &intended)
            .map_err(|_| "cannot plant owned source mutation")?;
    }
    let root = if changed {
        copied.as_path()
    } else {
        context.root
    };
    let import = copied.join("src");
    let raw = directory.join("raw");
    private_directory(&raw)?;
    let retained = directory.join("retained");
    let case = &context.fixture["cases"][0];
    let spec = &case["phases"][0];
    let before = shared::files(&scope.owned, &scope.owned, None)?;
    let trial = TrialContext {
        root,
        python: context.python,
        owned: &scope.owned,
        environment: &scope.environment,
        fixture: context.fixture,
        name: "first",
        sources: &expected,
        runtime: &context.runtime["observer"],
        raw_dir: &raw,
        evidence: Some(&retained),
        driver: None,
        omit_driver_provenance: false,
        fault: None,
        observer_root: Some(context.root),
        source_import: Some(&import),
        diagnostic_extra: Some(&diagnostic),
    };
    let actual = phase(&trial, case, spec);
    let refusal = if changed {
        "imported module hash differs"
    } else {
        "imported Careerbrew module is outside checkout"
    };
    if actual.as_ref().err().map(String::as_str) != Some(refusal) {
        return Err("source control exact guard refusal differs".into());
    }
    let execution = strict_json(
        &fs::read(directory.join("execution.json")).map_err(|_| "source execution missing")?,
    )?;
    verify_phase_reaped_absent(&execution["observation"])?;
    if execution["observation"]["wait_code"] != 0 || execution["command_matches"] != true {
        return Err("source control wait/command differs".into());
    }
    let api = retained_protocol(directory, "api.json")?
        .ok_or("actual copied-source API proof missing")?;
    let observer = retained_protocol(directory, "observer.json")?
        .ok_or("actual copied-source observer missing")?;
    let pid = execution["observation"]["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or("source PID missing")?;
    verify_driver_provenance(&api, pid, root)?;
    // This proves the planted execution; it never replaces the phase's frozen baseline.
    let mut executed_proof = expected.clone();
    if changed {
        let original_pin = expected["src/careerbrew/store.py"]
            .as_str()
            .ok_or("original store pin missing")?;
        let prefix = original_pin
            .rsplit_once(':')
            .ok_or("original store pin malformed")?
            .0;
        executed_proof["src/careerbrew/store.py"] =
            json!(format!("{prefix}:{}", shared::digest(&intended)));
    }
    verify_current_report_structure(&api, "first", case, spec, pid, &copied, &executed_proof)?;
    verify_report_semantics(&api, spec)?;
    let called = &api["api_calls"];
    if called.as_array().is_none_or(|calls| calls.len() != 1) {
        return Err("source control original call count differs".into());
    }
    let provider = copied.join("src/careerbrew/store.py");
    let digest = shared::digest(&intended);
    for field in ["module_path", "loader_origin", "function_source"] {
        if called[0][field].as_str() != provider.to_str() {
            return Err("source control did not execute copied provider".into());
        }
    }
    if called[0]["source_sha256"] != digest
        || called[0]["started"] != true
        || called[0]["completed"] != true
        || observer["modules"]["careerbrew.store"]["sha256"] != digest
    {
        return Err("source control actual executed bytes differ".into());
    }
    let allowed = trusted_source_runtime_inventory(context.runtime)?;
    runtime_files(context.root, &observer, &allowed)?;
    let after = shared::files(
        &scope.owned,
        &scope.owned,
        Some(
            context.fixture["canary"]
                .as_str()
                .ok_or("source canary missing")?,
        ),
    )?;
    verify_delta(&scope.owned, &before, &after, spec)?;
    let result = json!({"control":if changed{"changed-source"}else{"wrong-root"},"exact_refusal":refusal,"execution":execution,"api":api,"observer":observer,"before":before,"after":after,"os_reap_absence_verified":true});
    shared::write_receipt(&result, &directory.join("judged.json"))?;
    Ok(result)
}
#[cfg(test)]
fn unique_control_pids(records: &[Value]) -> Result<(), String> {
    let mut pids = BTreeSet::new();
    for record in records {
        let pid = record["execution"]["observation"]["pid"]
            .as_u64()
            .filter(|pid| *pid > 0)
            .ok_or("control PID missing")?;
        if !pids.insert(pid) {
            return Err("control reused an actual phase PID".into());
        }
    }
    Ok(())
}
#[cfg(test)]
fn record_group(output: &Path, id: u32, details: Value) -> Result<Value, String> {
    let judgment = match id {
        1 => "positive_reference_match",
        2 => "typed_value_mismatch",
        3 => "semantic_reference_mismatch",
        4 => "plaintext_canary",
        5 => "stream_canary_stdout",
        6 => "stream_canary_stderr",
        7 => "unexpected_retained_write",
        8 => "missing_api_evidence",
        9 => "nonzero_exit",
        10 => "source_identity_mismatch",
        11 => "no_op_identity_mismatch",
        12 => "deadline_exceeded",
        13 => "original_preservation",
        _ => return Err("unregistered contract group".into()),
    };
    let result = json!({"group":id,"status":"passed","judgment":judgment,"details":details});
    shared::write_receipt(&result, &output.join(format!("group-{id:02}.json")))?;
    Ok(result)
}
#[cfg(test)]
pub fn contract_control_groups(
    root: &Path,
    python: &Path,
    output: &Path,
    preservation: impl FnOnce() -> Result<Value, String>,
) -> Result<(), String> {
    private_directory(output)?;
    let root = root.canonicalize().map_err(|_| "contract root missing")?;
    let runtime = runtime_bootstrap(&root, python, &output.join("runtime"), "none")?;
    let inputs = observed_inputs(&root, python, &runtime)?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    shared::write_receipt(&inputs, &output.join("inputs-before.json"))?;
    let context = ContractContext {
        root: &root,
        python,
        fixture: &fixture,
        inputs: &inputs,
        runtime: &runtime,
    };
    let mut groups = Vec::new();
    let positive = output.join("positive");
    private_directory(&positive)?;
    let candidate = positive.join("candidate.json");
    run(&root, python, &candidate)?;
    let observed = aggregate_candidate(&candidate)?;
    let completion = complete_trials(
        observed["trials"]
            .as_array()
            .ok_or("positive trials missing")?,
    )?;
    groups.push(record_group(output, 1, completion)?);
    let mutations = output.join("mutations");
    private_directory(&mutations)?;
    let mut typed = Vec::new();
    for fault in [
        ContractFault::JsonOne,
        ContractFault::JsonBool,
        ContractFault::JsonLarge,
        ContractFault::JsonZero,
        ContractFault::YamlDateMap,
        ContractFault::YamlMapDate,
    ] {
        typed.push(mutation_pair(&context, fault, &mutations.join(fault.id()))?);
    }
    groups.push(record_group(output, 2, json!(typed))?);
    groups.push(record_group(
        output,
        3,
        same_wrong_trials(&context, &output.join("same-wrong"))?,
    )?);
    for (id, fault, expected) in [
        (
            4,
            ContractFault::Plaintext,
            "capture canary leaked in retained content",
        ),
        (5, ContractFault::Stdout, "data stream canary"),
        (6, ContractFault::Stderr, "data stream canary"),
        (7, ContractFault::ExtraWrite, "unexpected retained change"),
    ] {
        groups.push(record_group(
            output,
            id,
            simple_fault(&context, fault, false, expected, &output.join(fault.id()))?,
        )?);
    }
    for (id, fault, expected) in [
        (8, ContractFault::Empty, "data API evidence missing"),
        (9, ContractFault::Nonzero, "phase_nonzero_exit"),
    ] {
        let mut arms = Vec::new();
        for reader in [false, true] {
            arms.push(simple_fault(
                &context,
                fault,
                reader,
                expected,
                &output.join(format!(
                    "{}-{}",
                    fault.id(),
                    if reader { "reader" } else { "writer" }
                )),
            )?);
        }
        unique_control_pids(&arms)?;
        groups.push(record_group(output, id, json!(arms))?);
    }
    let source_arms = [
        source_control(&context, &output.join("wrong-root"), false)?,
        source_control(&context, &output.join("changed-source"), true)?,
    ];
    unique_control_pids(&source_arms)?;
    groups.push(record_group(output, 10, json!(source_arms))?);
    groups.push(record_group(
        output,
        11,
        simple_fault(
            &context,
            ContractFault::Rewrite,
            false,
            "no_op_identity_mismatch",
            &output.join("rewrite"),
        )?,
    )?);
    let mut hangs = Vec::new();
    for reader in [false, true] {
        hangs.push(simple_fault(
            &context,
            ContractFault::Hang,
            reader,
            "phase_deadline",
            &output.join(if reader { "hang-reader" } else { "hang-writer" }),
        )?);
    }
    unique_control_pids(&hangs)?;
    groups.push(record_group(output, 12, json!(hangs))?);
    let original = preservation()?;
    if original["cli"] != 14 || original["http"] != 15 || original["owner"] != 3 {
        return Err("original preservation counts differ".into());
    }
    groups.push(record_group(output, 13, original)?);
    if groups.len() != 13 || observed_inputs(&root, python, &runtime)? != inputs {
        return Err("contract groups or frozen inputs differ".into());
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-data-thirteen-groups-v1","groups":groups,"qualification":"bounded13groups-only","scope":"ten original data cases and exact registered faults; no resource/fullformat/startup/fullRust00 qualification"}),
        &output.join("results.json"),
    )
}

fn runtime_files(
    _root: &Path,
    observation: &Value,
    allowed: &BTreeSet<String>,
) -> Result<(), String> {
    let modules = observation["external_modules"]
        .as_object()
        .filter(|v| !v.is_empty())
        .ok_or("runtime modules missing")?;
    for record in modules.values() {
        keys(record, &["path", "sha256"])?;
        let path = record["path"]
            .as_str()
            .filter(|p| allowed.contains(*p))
            .ok_or("runtime module path not allowed")?;
        if shared::digest(&fs::read(path).map_err(|_| "runtime module unavailable")?)
            != record["sha256"]
        {
            return Err("runtime module changed".into());
        }
    }
    let sites = observation["site_inputs"]
        .as_object()
        .filter(|v| !v.is_empty())
        .ok_or("runtime sites missing")?;
    for (path, hash) in sites {
        if !allowed.contains(path) {
            return Err("runtime site path not allowed".into());
        }
        if shared::digest(&fs::read(path).map_err(|_| "runtime site unavailable")?) != *hash {
            return Err("runtime site changed".into());
        }
    }
    Ok(())
}

fn runtime_static_inputs(root: &Path, python: &Path, binary: &Path) -> Result<Value, String> {
    fn collect(path: &Path, files: &mut BTreeSet<String>) -> Result<(), String> {
        let Ok(meta) = fs::symlink_metadata(path) else {
            return Ok(());
        };
        if meta.file_type().is_symlink() {
            return Ok(());
        }
        if meta.is_dir() {
            for child in fs::read_dir(path).map_err(|_| "runtime input directory unavailable")? {
                collect(
                    &child.map_err(|_| "runtime input entry unavailable")?.path(),
                    files,
                )?;
            }
        } else if meta.is_file() {
            let name = path
                .file_name()
                .ok_or("runtime input name missing")?
                .to_string_lossy();
            if name.ends_with(".py")
                || name.ends_with(".so")
                || name.ends_with(".pth")
                || name.ends_with(".egg-link")
                || matches!(name.as_ref(), "METADATA" | "RECORD" | "pyvenv.cfg")
            {
                files.insert(path.to_string_lossy().into_owned());
            }
        }
        if files.len() > 100_000 {
            return Err("runtime input inventory exceeds bound".into());
        }
        Ok(())
    }
    let venv = python
        .parent()
        .and_then(Path::parent)
        .ok_or("runtime environment root missing")?;
    let base = binary
        .parent()
        .and_then(Path::parent)
        .ok_or("runtime interpreter root missing")?;
    let mut allowed = BTreeSet::new();
    for dir in [
        venv.to_owned(),
        base.to_owned(),
        root.join("src/careerbrew"),
        root.join("xtask/tests/fixtures/capture"),
    ] {
        collect(&dir, &mut allowed)?;
    }
    let library_names = fs::read_dir(base.join("lib"))
        .map_err(|_| "runtime interpreter libraries unavailable")?
        .map(|entry| {
            entry
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .map_err(|_| "runtime library name unavailable")
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut sites = BTreeSet::new();
    let config = venv.join("pyvenv.cfg");
    let config_key = config.to_string_lossy().into_owned();
    if !allowed.contains(&config_key) {
        return Err("runtime environment configuration not regular".into());
    }
    let config_raw =
        fs::read(&config).map_err(|_| "runtime environment configuration unavailable")?;
    for prefix in active_site_prefixes(venv, base, &config_raw)? {
        for entry in
            fs::read_dir(prefix.join("lib")).map_err(|_| "runtime library root unavailable")?
        {
            let path = entry
                .map_err(|_| "runtime library entry unavailable")?
                .path();
            if path
                .file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("python"))
                && path.join("site-packages").is_dir()
            {
                sites.insert(path.join("site-packages"));
            }
        }
    }
    for dir in [
        root.to_owned(),
        root.join("xtask/tests/fixtures/capture"),
        root.join("xtask/tests/fixtures/capture/observer"),
    ] {
        sites.insert(dir);
    }
    let mut hashes = BTreeMap::new();
    for file in &allowed {
        let path = Path::new(file);
        let parent = path.parent().ok_or("runtime site parent missing")?;
        let name = path
            .file_name()
            .ok_or("runtime site name missing")?
            .to_string_lossy();
        let direct =
            sites.contains(parent) && (name.ends_with(".pth") || name.ends_with(".egg-link"));
        let metadata = matches!(name.as_ref(), "METADATA" | "RECORD")
            && parent
                .file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".dist-info"))
            && parent.parent().is_some_and(|p| sites.contains(p));
        if direct || metadata {
            hashes.insert(
                file.clone(),
                shared::digest(&fs::read(path).map_err(|_| "runtime site input unavailable")?),
            );
        }
    }
    let interpreter_search_paths =
        configured_interpreter_paths(&config_raw, binary, &library_names)?;
    hashes.insert(config_key, shared::digest(&config_raw));
    Ok(
        json!({"allowed_files":allowed,"site_inputs":hashes,"interpreter_search_paths":interpreter_search_paths,"scope":"trusted source/interpreter/environment file inventory before execution; site inputs prehashed; other loaded file content first observed after import"}),
    )
}

fn observed_inputs(root: &Path, python: &Path, runtime: &Value) -> Result<Value, String> {
    let mut current = static_inputs(root, python)?;
    if current != runtime["static_inputs"] {
        return Err("runtime static inputs changed".into());
    }
    let allowed = current["runtime_static"]["allowed_files"]
        .as_array()
        .ok_or("runtime file inventory missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("runtime file path invalid")
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    runtime_files(root, &runtime["observer"], &allowed)?;
    if runtime["observer"]["site_inputs"] != current["runtime_static"]["site_inputs"] {
        return Err("runtime observed site inputs differ".into());
    }
    shared::validate_loaded_modules(&runtime["observer"], root, &current["provider_sources"])?;
    current["runtime_observation"] = runtime.clone();
    Ok(current)
}

fn owner_observation(observation: &shared::OwnerObservation) -> Value {
    json!({"pid":observation.pid,"program":observation.program,"args":observation.args,"wait_code":observation.wait_code,"wait_signal":observation.wait_signal,"wait_observed":observation.wait_observed,"reaped":observation.reaped,"cleanup_succeeded":observation.cleanup_succeeded})
}

fn runtime_bootstrap(
    root: &Path,
    python: &Path,
    output: &Path,
    fault: &str,
) -> Result<Value, String> {
    fs::create_dir(output).map_err(|_| "bootstrap output already exists or unavailable")?;
    fs::set_permissions(output, fs::Permissions::from_mode(0o700))
        .map_err(|_| "bootstrap output mode unavailable")?;
    let frozen = static_inputs(root, python)?;
    shared::write_receipt(&frozen, &output.join("static-before.json"))?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let canary = fixture["canary"]
        .as_str()
        .ok_or("bootstrap canary missing")?;
    let scratch = tempfile::tempdir().map_err(|_| "bootstrap owned root unavailable")?;
    let owned = scratch
        .path()
        .canonicalize()
        .map_err(|_| "bootstrap owned root not canonical")?
        .join("owned");
    let mut env = prepare(root, &owned, &fixture)?;
    let api = owned.join("protocol/bootstrap-api.json");
    let observer = owned.join("protocol/bootstrap-observer.json");
    for (key, path) in [
        (
            "PYTHONPATH",
            root.join("xtask/tests/fixtures/capture/observer"),
        ),
        (
            "CAREERBREW_CAPTURE_INPUT",
            root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json"),
        ),
        (
            "CAREERBREW_DATA_FIXTURE",
            root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json"),
        ),
        ("CAREERBREW_CAPTURE_ROOT", root.to_owned()),
        ("CAREERBREW_DATA_OWNED", owned.clone()),
        ("CAREERBREW_CAPTURE_OBSERVER", observer.clone()),
        ("CAREERBREW_DATA_REPORT", api.clone()),
    ] {
        env.insert(key.into(), path.to_string_lossy().into_owned());
    }
    env.insert("CAREERBREW_DATA_BOOTSTRAP_CONTROL".into(), fault.to_owned());
    let before = shared::files(&owned, &owned, Some(canary))?;
    let mut command = Command::new(python);
    command
        .args(["-B", "-S"])
        .arg(root.join("xtask/tests/fixtures/capture/data_driver.py"))
        .arg("--runtime-bootstrap")
        .current_dir(root)
        .env_clear()
        .envs(&env);
    let record_paths = bootstrap_record_paths(root, &owned, &frozen)?;
    let actual =
        shared::execute_during_observed(command, std::time::Duration::from_secs(30), |_, _| Ok(()));
    let (observed, stdout, stderr) = match actual {
        Ok(execution) => {
            let observed = owner_observation(&execution.observation);
            shared::write_receipt(
                &json!({"observation":observed,"cwd":root,"stdout_bytes":execution.stdout.len(),"stdout_sha256":shared::digest(execution.stdout.as_bytes()),"stderr_bytes":execution.stderr.len(),"stderr_sha256":shared::digest(execution.stderr.as_bytes())}),
                &output.join("execution.json"),
            )?;
            (execution.observation, execution.stdout, execution.stderr)
        }
        Err(failure) => {
            shared::write_receipt(
                &json!({"cause":format!("{:?}",failure.cause),"observation":failure.observation.as_ref().map(owner_observation),"diagnostic_sha256":shared::digest(failure.message.as_bytes())}),
                &output.join("execution.json"),
            )?;
            let terminal = failure
                .observation
                .as_ref()
                .is_some_and(|o| o.wait_observed && o.reaped && o.cleanup_succeeded);
            retain_bootstrap_protocols(
                output,
                &owned,
                &json!({"source_root":root,"observation":failure.observation.as_ref().map(owner_observation),"validation_category":"bootstrap_owner_failure"}),
                &record_paths,
                canary,
                "bootstrap_owner_failure",
                terminal,
            )?;
            return Err("bootstrap_owner_failure".into());
        }
    };
    let terminal = observed.wait_observed && observed.reaped && observed.cleanup_succeeded;
    let initial_category = if !terminal {
        "bootstrap_cleanup_unknown"
    } else if observed.wait_code != Some(0) {
        "bootstrap_nonzero_exit"
    } else if stdout.contains(canary) || stderr.contains(canary) {
        "bootstrap_privacy_refused"
    } else if !stdout.is_empty() || !stderr.is_empty() {
        "bootstrap_streams_not_empty"
    } else {
        "bootstrap_validation_pending"
    };
    if let Err(retention_error) = retain_bootstrap_protocols(
        output,
        &owned,
        &json!({"source_root":root,"observation":owner_observation(&observed),"validation_category":initial_category,"stdout_bytes":stdout.len(),"stdout_sha256":shared::digest(stdout.as_bytes()),"stderr_bytes":stderr.len(),"stderr_sha256":shared::digest(stderr.as_bytes())}),
        &record_paths,
        canary,
        initial_category,
        terminal,
    ) {
        let primary = if initial_category == "bootstrap_validation_pending" {
            "bootstrap_retention_failed"
        } else {
            initial_category
        };
        let secondary = shared::write_receipt(
            &json!({"primary_category":primary,"retention_error_sha256":shared::digest(retention_error.as_bytes()),"safe_retention_error":if retention_error.contains(canary){None}else{Some(retention_error.as_str())}}),
            &output.join("retention-error.json"),
        );
        if secondary.is_err() {
            return Err(format!("{primary}: retention receipt unavailable"));
        }
        return Err(primary.into());
    }
    if !observed.wait_observed || !observed.reaped || !observed.cleanup_succeeded {
        return Err("bootstrap_cleanup_unknown".into());
    }
    if observed.wait_code != Some(0) {
        return Err("bootstrap_nonzero_exit".into());
    }
    if stdout.contains(canary) || stderr.contains(canary) {
        return Err("bootstrap_privacy_refused".into());
    }
    if !stdout.is_empty() || !stderr.is_empty() {
        return Err("bootstrap_streams_not_empty".into());
    }
    for path in [&api, &observer] {
        if !fs::symlink_metadata(path)
            .map_err(|_| "bootstrap_evidence_missing")?
            .file_type()
            .is_file()
        {
            return Err("bootstrap_protocol_not_regular".into());
        }
    }
    let api_raw = shared::bounded(&api)?;
    let observer_raw = shared::bounded(&observer)?;
    if api_raw.contains(canary) || observer_raw.contains(canary) {
        return Err("bootstrap_privacy_refused".into());
    }
    let report = strict_json(api_raw.as_bytes())?;
    let provenance = strict_json(observer_raw.as_bytes())?;
    keys(
        &report,
        &["schema", "pid", "driver_provenance", "store_calls"],
    )?;
    if report["schema"] != "careerbrew-data-runtime-bootstrap-v1" || report["pid"] != observed.pid {
        return Err("bootstrap_report_identity_differs".into());
    }
    verify_driver_provenance(&report, observed.pid, root)?;
    shared::validate_loaded_modules(&provenance, root, &frozen["provider_sources"])?;
    let allowed = frozen["runtime_static"]["allowed_files"]
        .as_array()
        .ok_or("bootstrap inventory missing")?
        .iter()
        .map(|v| {
            v.as_str()
                .map(str::to_owned)
                .ok_or("bootstrap path invalid")
        })
        .collect::<Result<BTreeSet<_>, _>>()?;
    runtime_files(root, &provenance, &allowed)?;
    if provenance["site_inputs"] != frozen["runtime_static"]["site_inputs"]
        || provenance["catalog_calls"] != 0
        || !provenance["descendant"].is_null()
        || provenance["modules"]["careerbrew.store"].is_null()
    {
        return Err("bootstrap_source_or_site_differs".into());
    }
    let calls = report["store_calls"]
        .as_array()
        .ok_or("bootstrap call ledger missing")?;
    for (index, call) in calls.iter().enumerate() {
        keys(call, &["ordinal", "function"])?;
        if call["ordinal"] != index + 1
            || call["function"].as_str().is_none_or(|name| {
                name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            return Err("bootstrap call ledger invalid".into());
        }
    }
    if !calls.is_empty() {
        return Err("bootstrap_store_call_refused".into());
    }
    let after = shared::files(&owned, &owned, Some(canary))?;
    shared::write_receipt(
        &json!({"before":before,"after":after,"scope":"retained owned entries only"}),
        &output.join("snapshots.json"),
    )?;
    let spec = json!({"allowed_retained_delta":[],"allowed_instrumentation_delta":[{"operation":"add","entry":{"path":"protocol/bootstrap-api.json","mode":"0600","content_rule":"bootstrap API JSON"}},{"operation":"add","entry":{"path":"protocol/bootstrap-observer.json","mode":"0644","content_rule":"bootstrap observer JSON"}}]});
    verify_delta(&owned, &before, &after, &spec)
        .map_err(|_| "bootstrap_retained_change_refused")?;
    if static_inputs(root, python)? != frozen {
        return Err("bootstrap_static_inputs_changed".into());
    }
    let result = json!({"schema":"careerbrew-data-runtime-observation-v1","static_inputs":frozen,"observer":provenance,"observer_sha256":shared::digest(observer_raw.as_bytes()),"report_sha256":shared::digest(api_raw.as_bytes()),"observation":owner_observation(&observed),"store_calls":0,"scope":"current import-only invocation; unloaded dependencies and pre-initial-load external content not fully observed"});
    shared::write_receipt(&result, &output.join("runtime.json"))?;
    Ok(result)
}

#[cfg(test)]
fn execute_setup(
    command: Command,
    receipt: &Path,
) -> Result<(shared::OwnerObservation, String), String> {
    execute_setup_output(command, receipt, true)
}
#[cfg(test)]
fn setup_safe_text<'a>(text: &'a str, canary: &str, retain_text: bool) -> Option<&'a str> {
    (retain_text && !text.contains(canary)).then_some(text)
}
#[cfg(test)]
fn execute_setup_output(
    command: Command,
    receipt: &Path,
    retain_text: bool,
) -> Result<(shared::OwnerObservation, String), String> {
    let cwd = command.get_current_dir().map(Path::to_owned);
    let execution =
        shared::execute_during_observed(command, std::time::Duration::from_secs(30), |_, _| Ok(()));
    match execution {
        Ok(value) => {
            let fixture = strict_json(FIXTURE.as_bytes())?;
            let canary = fixture["canary"].as_str().ok_or("setup canary missing")?;
            shared::write_receipt(
                &json!({"observation":owner_observation(&value.observation),"cwd":cwd,"stdout_bytes":value.stdout.len(),"stdout_sha256":shared::digest(value.stdout.as_bytes()),"stderr_bytes":value.stderr.len(),"stderr_sha256":shared::digest(value.stderr.as_bytes()),"safe_stdout":setup_safe_text(&value.stdout,canary,retain_text),"safe_stderr":setup_safe_text(&value.stderr,canary,retain_text)}),
                receipt,
            )?;
            if value.observation.wait_code != Some(0)
                || !value.observation.wait_observed
                || !value.observation.reaped
                || !value.observation.cleanup_succeeded
            {
                return Err("bootstrap setup child failed".into());
            }
            Ok((value.observation, value.stdout))
        }
        Err(failure) => {
            shared::write_receipt(
                &json!({"cause":format!("{:?}",failure.cause),"observation":failure.observation.as_ref().map(owner_observation),"diagnostic_sha256":shared::digest(failure.message.as_bytes())}),
                receipt,
            )?;
            Err("bootstrap setup owner failed".into())
        }
    }
}

#[cfg(test)]
fn installed_tool(name: &str) -> Result<std::path::PathBuf, String> {
    for directory in std::env::split_paths(&std::env::var_os("PATH").ok_or("tool PATH missing")?) {
        let path = directory.join(name);
        if path.is_file() {
            return path
                .canonicalize()
                .map_err(|_| "tool path unavailable".into());
        }
    }
    Err("bootstrap setup tool unavailable".into())
}

#[cfg(test)]
pub fn bootstrap_contracts(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir(output)
        .map_err(|_| "bootstrap controls output already exists or unavailable")?;
    let root = root
        .canonicalize()
        .map_err(|_| "bootstrap control root unavailable")?;
    let mut outcomes = Vec::new();
    for (fault, expected) in [
        ("none", "accepted"),
        ("store-call", "bootstrap_store_call_refused"),
        ("extra-write", "bootstrap_retained_change_refused"),
        ("stdout-canary", "bootstrap_privacy_refused"),
        ("skip", "bootstrap_evidence_missing"),
        ("nonzero", "bootstrap_nonzero_exit"),
    ] {
        let directory = output.join(fault);
        let result = runtime_bootstrap(&root, python, &directory, fault);
        let category = match &result {
            Ok(_) => "accepted",
            Err(error) => error.as_str(),
        };
        let execution = strict_json(
            &fs::read(directory.join("execution.json"))
                .map_err(|_| "bootstrap execution missing")?,
        )?;
        let observed = &execution["observation"];
        shared::write_receipt(
            &json!({"category":category,"expected":expected,"scope":"real bootstrap control; no corpus qualification"}),
            &directory.join("result.json"),
        )?;
        let expected_exit = if fault == "nonzero" { 17 } else { 0 };
        if category != expected
            || observed["wait_code"] != expected_exit
            || observed["wait_observed"] != true
            || observed["reaped"] != true
            || observed["cleanup_succeeded"] != true
            || observed["pid"].as_u64().is_none_or(|pid| pid == 0)
        {
            return Err("bootstrap actual control differs".into());
        }
        let inventory = strict_json(
            &fs::read(directory.join("protocol-inventory.json"))
                .map_err(|_| "bootstrap protocol inventory missing")?,
        )?;
        if matches!(fault, "skip" | "nonzero") {
            if inventory["expected"].as_array().is_none_or(|items| {
                items.len() != 2 || items.iter().any(|item| item["state"] != "absent")
            }) {
                return Err("bootstrap absent evidence differs".into());
            }
        } else {
            if inventory["expected"].as_array().is_none_or(|items| {
                items.len() != 2 || items.iter().any(|item| item["state"] != "safe")
            }) || !directory.join("retained/api.json").is_file()
                || !directory.join("retained/observer.json").is_file()
            {
                return Err("bootstrap safe protocol retention differs".into());
            }
        }
        if fault == "store-call" {
            let api = strict_json(
                &fs::read(directory.join("retained/api.json"))
                    .map_err(|_| "bootstrap call raw evidence missing")?,
            )?;
            if api["store_calls"] != json!([{"ordinal":1,"function":"digest"}]) {
                return Err("bootstrap actual call evidence differs".into());
            }
        }
        if fault == "extra-write" {
            let snapshots = strict_json(
                &fs::read(directory.join("snapshots.json"))
                    .map_err(|_| "bootstrap write evidence missing")?,
            )?;
            if !snapshots["after"]["data/bootstrap-unexpected.txt"]
                .as_str()
                .is_some_and(|v| v.ends_with(&shared::digest(b"synthetic unexpected write")))
            {
                return Err("bootstrap actual write evidence differs".into());
            }
        }
        if fault == "none" {
            let runtime = result
                .as_ref()
                .map_err(|_| "bootstrap acceptance missing")?;
            observed_inputs(&root, python, runtime)?;
        }
        outcomes.push(json!({"fault":fault,"category":category,"pid":observed["pid"],"wait_code":observed["wait_code"],"wait_observed":observed["wait_observed"],"reaped":observed["reaped"],"cleanup_succeeded":observed["cleanup_succeeded"]}));
    }
    let pids = outcomes
        .iter()
        .filter_map(|v| v["pid"].as_u64())
        .collect::<BTreeSet<_>>();
    if pids.len() != 6 {
        return Err("bootstrap distinct execution evidence missing".into());
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-runtime-bootstrap-controls-v1","outcomes":outcomes,"scope":"six actual local controls; fresh checkout seventh control separate; no corpus qualification"}),
        &output.join("results.json"),
    )
}

#[cfg(test)]
pub fn fresh_bootstrap(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    fs::create_dir(output).map_err(|_| "fresh bootstrap output already exists or unavailable")?;
    let root = root
        .canonicalize()
        .map_err(|_| "fresh bootstrap source unavailable")?;
    let static_before = static_inputs(&root, python)?;
    let container =
        tempfile::tempdir().map_err(|_| "fresh bootstrap checkout container unavailable")?;
    let fresh = container
        .path()
        .canonicalize()
        .map_err(|_| "fresh checkout container not canonical")?
        .join("checkout");
    fs::create_dir(&fresh).map_err(|_| "fresh checkout unavailable")?;
    let setup_owned = container.path().join("setup-owned");
    let mut env = shared::owned_environment(&fresh, &setup_owned)?;
    env.insert("PATH".into(), "/opt/homebrew/bin:/usr/bin:/bin".into());
    env.insert("UV_PYTHON_DOWNLOADS".into(), "never".into());
    env.insert("UV_OFFLINE".into(), "1".into());
    env.insert("UV_NO_CONFIG".into(), "1".into());
    env.insert(
        "UV_PYTHON".into(),
        static_before["python"]["binary"]
            .as_str()
            .ok_or("fresh Python binding missing")?
            .to_owned(),
    );
    let git = installed_tool("git")?;
    let uv = installed_tool("uv")?;
    let tar = installed_tool("tar")?;
    shared::write_receipt(
        &json!({"git":{"path":git,"sha256":shared::digest(&fs::read(&git).map_err(|_|"git binary unavailable")?)},"uv":{"path":uv,"sha256":shared::digest(&fs::read(&uv).map_err(|_|"uv binary unavailable")?)},"tar":{"path":tar,"sha256":shared::digest(&fs::read(&tar).map_err(|_|"tar binary unavailable")?)}}),
        &output.join("setup-tools.json"),
    )?;
    let mut tree_command = Command::new(&git);
    tree_command
        .arg("write-tree")
        .current_dir(&root)
        .env_clear()
        .envs(&env);
    let (_, tree) = execute_setup(tree_command, &output.join("setup-tree.json"))?;
    let tree = tree.trim();
    if tree.len() != 40 || !tree.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("fresh archive tree invalid".into());
    }
    let archive = container.path().join("source.tar");
    let mut archive_command = Command::new(&git);
    archive_command
        .args(["archive", "--format=tar"])
        .arg(format!("--output={}", archive.display()))
        .arg(tree)
        .current_dir(&root)
        .env_clear()
        .envs(&env);
    execute_setup(archive_command, &output.join("setup-archive.json"))?;
    let mut extract = Command::new(&tar);
    extract
        .arg("-xf")
        .arg(&archive)
        .arg("-C")
        .arg(&fresh)
        .current_dir(&root)
        .env_clear()
        .envs(&env);
    execute_setup(extract, &output.join("setup-extract.json"))?;
    if fresh.join(".metrics").exists() || fresh.join(".venv").exists() {
        return Err("fresh checkout contains ignored history or environment".into());
    }
    for (path, pin) in static_before["pins"]
        .as_object()
        .ok_or("fresh expected pins missing")?
    {
        if shared::digest(&fs::read(fresh.join(path)).map_err(|_| "fresh archived source missing")?)
            != pin["sha256"]
        {
            return Err("fresh archive does not match compiled inputs".into());
        }
    }
    let mut cache = Command::new(&uv);
    cache
        .args(["--no-config", "cache", "dir"])
        .current_dir(&root)
        .env_clear()
        .env(
            "HOME",
            std::env::var_os("HOME").ok_or("tool cache HOME missing")?,
        )
        .env("PATH", "/opt/homebrew/bin:/usr/bin:/bin");
    let (_, cache_path) = execute_setup(cache, &output.join("setup-cache.json"))?;
    let cache_path = cache_path.trim();
    if !Path::new(cache_path).is_absolute() || !Path::new(cache_path).is_dir() {
        return Err("offline tool cache unavailable".into());
    }
    env.insert("UV_CACHE_DIR".into(), cache_path.to_owned());
    let mut sync = Command::new(&uv);
    sync.args(["sync", "--locked", "--offline"])
        .current_dir(&fresh)
        .env_clear()
        .envs(&env);
    execute_setup(sync, &output.join("setup-sync.json"))?;
    if fresh.join(".metrics").exists() {
        return Err("fresh setup unexpectedly created history".into());
    }
    let runtime = runtime_bootstrap(
        &fresh,
        &fresh.join(".venv/bin/python"),
        &output.join("runtime"),
        "none",
    )?;
    observed_inputs(&fresh, &fresh.join(".venv/bin/python"), &runtime)?;
    if runtime["store_calls"] != 0
        || runtime["observer"]["modules"]["careerbrew.store"]["path"].as_str()
            != fresh.join("src/careerbrew/store.py").to_str()
    {
        return Err("fresh observed provider identity differs".into());
    }
    shared::write_receipt(
        &json!({"schema":"careerbrew-fresh-runtime-bootstrap-v1","archive_tree":tree,"fresh_root":fresh,"history_absent_before_bootstrap":true,"copied_environment":false,"runtime_observation":runtime["observation"],"store_calls":0,"scope":"fresh offline editable environment; setup writes separate from protocol-only bootstrap"}),
        &output.join("result.json"),
    )
}

fn active_site_prefixes(
    venv: &Path,
    base: &Path,
    config: &[u8],
) -> Result<Vec<std::path::PathBuf>, String> {
    let config =
        std::str::from_utf8(config).map_err(|_| "runtime environment policy is not UTF8")?;
    let mut policy = None;
    for line in config.lines() {
        if let Some((key, value)) = line.split_once('=') {
            if key
                .trim()
                .eq_ignore_ascii_case("include-system-site-packages")
            {
                if policy.is_some() {
                    return Err("runtime environment policy repeated".into());
                }
                policy = Some(if value.trim().eq_ignore_ascii_case("true") {
                    true
                } else if value.trim().eq_ignore_ascii_case("false") {
                    false
                } else {
                    return Err("runtime environment policy invalid".into());
                });
            }
        }
    }
    let include = policy.ok_or("runtime environment policy missing")?;
    let mut prefixes = vec![venv.to_owned()];
    if include && base != venv {
        prefixes.push(base.to_owned());
    }
    Ok(prefixes)
}

fn bootstrap_record_paths(
    root: &Path,
    owned: &Path,
    frozen: &Value,
) -> Result<BTreeSet<String>, String> {
    let mut paths = BTreeSet::new();
    for file in frozen["runtime_static"]["allowed_files"]
        .as_array()
        .ok_or("bootstrap static inventory missing")?
    {
        let file = file.as_str().ok_or("bootstrap static path invalid")?;
        paths.insert(file.to_owned());
        for parent in Path::new(file).ancestors().skip(1) {
            paths.insert(parent.to_string_lossy().into_owned());
        }
    }
    for path in [root.to_owned(), owned.to_owned()] {
        paths.insert(path.to_string_lossy().into_owned());
    }
    for path in frozen["runtime_static"]["interpreter_search_paths"]
        .as_array()
        .ok_or("bootstrap interpreter search bindings missing")?
    {
        paths.insert(
            path.as_str()
                .ok_or("bootstrap interpreter search binding invalid")?
                .to_owned(),
        );
    }
    Ok(paths)
}

fn phase_protocol_shape(name: &str, value: &Value) -> Result<(), String> {
    if name == "fault.json" {
        return fault_protocol_shape(value);
    }
    if name == "api.json" {
        let mut fields = vec![
            "schema",
            "trial",
            "case",
            "role",
            "pid",
            "fixture_sha256",
            "api_calls",
        ];
        if value.get("driver_provenance").is_some() {
            fields.push("driver_provenance");
        }
        keys(value, &fields)?;
        if value["schema"] != "careerbrew-data-phase-v1"
            || !value["pid"].is_u64()
            || ["trial", "case", "role", "fixture_sha256"]
                .iter()
                .any(|field| !value[field].is_string())
            || value["api_calls"]
                .as_array()
                .is_none_or(|calls| calls.len() > 1024)
        {
            return Err("phase protocol shape invalid".into());
        }
        Ok(())
    } else {
        // Observer shape is shared; driver identity and provider qualification remain separate.
        bootstrap_protocol_shape(name, value, &Value::Null)
    }
}

fn bootstrap_protocol_shape(name: &str, value: &Value, execution: &Value) -> Result<(), String> {
    if name == "api.json" {
        keys(
            value,
            &["schema", "pid", "driver_provenance", "store_calls"],
        )?;
        let pid = execution["observation"]["pid"]
            .as_u64()
            .and_then(|p| u32::try_from(p).ok())
            .ok_or("bootstrap execution PID invalid")?;
        if value["schema"] != "careerbrew-data-runtime-bootstrap-v1" || value["pid"] != pid {
            return Err("bootstrap protocol identity invalid".into());
        }
        verify_driver_provenance(
            value,
            pid,
            Path::new(
                execution["source_root"]
                    .as_str()
                    .ok_or("bootstrap source root missing")?,
            ),
        )?;
        let calls = value["store_calls"]
            .as_array()
            .filter(|a| a.len() <= 1024)
            .ok_or("bootstrap calls invalid")?;
        for (index, call) in calls.iter().enumerate() {
            keys(call, &["ordinal", "function"])?;
            if call["ordinal"] != index + 1
                || call["function"].as_str().is_none_or(|s| {
                    s.is_empty() || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                })
            {
                return Err("bootstrap call shape invalid".into());
            }
        }
    } else {
        keys(
            value,
            &[
                "catalog_calls",
                "modules",
                "external_modules",
                "sys_path",
                "prefix",
                "base_prefix",
                "site_inputs",
                "distributions",
                "descendant",
                "dependency_scope",
                "unresolved",
            ],
        )?;
        for field in ["modules", "external_modules", "site_inputs"] {
            if !value[field].is_object() {
                return Err("bootstrap observer map invalid".into());
            }
        }
        for field in ["sys_path", "distributions", "unresolved"] {
            if !value[field].is_array() {
                return Err("bootstrap observer array invalid".into());
            }
        }
        if !value["catalog_calls"].is_u64()
            || !value["prefix"].is_string()
            || !value["base_prefix"].is_string()
            || !value["dependency_scope"].is_string()
            || !(value["descendant"].is_null() || value["descendant"].is_u64())
        {
            return Err("bootstrap observer fields invalid".into());
        }
        for entry in value["external_modules"].as_object().unwrap().values() {
            keys(entry, &["path", "sha256"])?;
            if !entry["path"].is_string()
                || entry["sha256"].as_str().is_none_or(|hash| {
                    hash.len() != 64
                        || !hash
                            .bytes()
                            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
                })
            {
                return Err("bootstrap module fields invalid".into());
            }
        }
        for entry in value["modules"].as_object().unwrap().values() {
            keys(entry, &["path", "sha256", "loader_origin"])?;
            if !entry["path"].is_string()
                || !entry["sha256"].is_string()
                || !(entry["loader_origin"].is_null() || entry["loader_origin"].is_string())
            {
                return Err("bootstrap provider fields invalid".into());
            }
        }
    }
    Ok(())
}

fn retain_bootstrap_protocols(
    output: &Path,
    owned: &Path,
    execution: &Value,
    allowed: &BTreeSet<String>,
    canary: &str,
    category: &str,
    terminal: bool,
) -> Result<(), String> {
    retain_protocols(
        output,
        owned,
        execution,
        allowed,
        canary,
        (category, terminal),
        (
            &[
                ("api.json", "protocol/bootstrap-api.json"),
                ("observer.json", "protocol/bootstrap-observer.json"),
            ],
            |name, value| bootstrap_protocol_shape(name, value, execution),
        ),
    )
}

fn protocol_bytes(path: &Path) -> std::io::Result<Vec<u8>> {
    bounded_file_bytes(path, LIMIT)
}
fn bounded_file_bytes(path: &Path, budget: usize) -> std::io::Result<Vec<u8>> {
    use std::{io::Read, os::unix::fs::OpenOptionsExt};
    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "protocol is not regular",
        ));
    }
    let mut raw = Vec::new();
    file.take((budget + 1) as u64).read_to_end(&mut raw)?;
    Ok(raw)
}
fn fault_relative(case: &str, role: &str) -> String {
    format!("protocol/{case:0>2}-{role}-fault.json")
}
fn read_fault(
    owned: &Path,
    execution: &Value,
    f: ContractFault,
    phase: (&str, &str, &str),
) -> Result<Value, String> {
    let o = &execution["observation"];
    if ["wait_observed", "reaped", "cleanup_succeeded"]
        .iter()
        .any(|k| o[k] != true)
    {
        return Err("fault owner unconfirmed".into());
    }
    let (case, role, trial) = phase;
    if !fs::symlink_metadata(owned.join("protocol"))
        .map_err(|_| "fault protocol directory missing")?
        .file_type()
        .is_dir()
    {
        return Err("fault protocol directory differs".into());
    }
    let path = owned.join(fault_relative(case, role));
    let meta = fs::symlink_metadata(&path).map_err(|_| "fault protocol missing")?;
    if !meta.file_type().is_file()
        || meta.permissions().mode() & 0o777 != 0o600
        || meta.len() > LIMIT as u64
    {
        return Err("fault protocol mode/kind/size differs".into());
    }
    let raw = protocol_bytes(&path).map_err(|_| "fault protocol read refused")?;
    let fixture = strict_json(FIXTURE.as_bytes())?;
    let canary = fixture["canary"].as_str().ok_or("fault canary missing")?;
    if raw
        .windows(canary.len())
        .any(|part| part == canary.as_bytes())
    {
        return Err("fault protocol privacy refusal".into());
    }
    let value = strict_json(&raw)?;
    let pid = o["pid"]
        .as_u64()
        .and_then(|p| u32::try_from(p).ok())
        .ok_or("fault owner PID missing")?;
    validate_fault_intent(&value, f, case, role, trial, pid)?;
    Ok(value)
}
fn effect_spec(spec: &Value, f: Option<ContractFault>, case: &str, role: &str) -> Value {
    let mut result = spec.clone();
    if f.is_some() {
        result["allowed_instrumentation_delta"].as_array_mut().expect("compiled effects").push(json!({"operation":"add","entry":{"path":fault_relative(case,role),"mode":"0600","content_rule":"intent-bound fault protocol"}}));
    }
    result
}

fn retain_protocols(
    output: &Path,
    owned: &Path,
    execution: &Value,
    allowed: &BTreeSet<String>,
    canary: &str,
    disposition: (&str, bool),
    protocol: (&[(&str, &str)], impl Fn(&str, &Value) -> Result<(), String>),
) -> Result<(), String> {
    if canary.is_empty() {
        return Err("bootstrap canary missing".into());
    }
    let (category, terminal) = disposition;
    let (records, shape) = protocol;
    let mut inventory = Vec::new();
    let mut safe = Vec::new();
    for &(name, relative) in records {
        let path = owned.join(relative);
        let mut state = "owner-terminal-unconfirmed";
        let mut bytes = None;
        let mut hash = None;
        if terminal {
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    state = "absent";
                }
                Err(_) => {
                    state = "metadata-unavailable";
                }
                Ok(meta) if !meta.file_type().is_file() => {
                    state = "nonregular";
                }
                Ok(meta) if meta.len() > LIMIT as u64 => {
                    state = "oversized";
                    bytes = Some(meta.len());
                }
                Ok(meta) => {
                    bytes = Some(meta.len());
                    let read = protocol_bytes(&path);
                    match read {
                        Err(_) => {
                            state = "read-unavailable";
                        }
                        Ok(raw) => {
                            bytes = Some(raw.len() as u64);
                            hash = Some(shared::digest(&raw));
                            if raw.len() > LIMIT {
                                state = "oversized";
                            } else if raw.windows(canary.len()).any(|w| w == canary.as_bytes()) {
                                state = "privacy-refused";
                            } else {
                                match strict_json(&raw) {
                                    Err(_) => {
                                        state = "malformed-json";
                                    }
                                    Ok(value) => {
                                        if diagnostic_paths(&value, allowed).is_err() {
                                            state = "unknown-path";
                                        } else if shape(name, &value).is_err() {
                                            state = "malformed-protocol";
                                        } else {
                                            match String::from_utf8(raw) {
                                                Ok(text) => {
                                                    state = "safe";
                                                    safe.push((name, text));
                                                }
                                                Err(_) => {
                                                    state = "malformed-utf8";
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        inventory.push(
            json!({"name":name,"relative":relative,"state":state,"bytes":bytes,"sha256":hash}),
        );
    }
    shared::write_receipt(
        &json!({"schema":if execution["protocol_scope"]=="phase" {"careerbrew-phase-protocol-inventory-v1"}else{"careerbrew-bootstrap-protocol-inventory-v1"},"category":category,"terminal_confirmed":terminal,"expected":inventory}),
        &output.join("protocol-inventory.json"),
    )?;
    let borrowed = safe
        .iter()
        .map(|(name, raw)| (*name, raw.as_str()))
        .collect::<Vec<_>>();
    retain_diagnostic(
        &output.join("retained"),
        execution,
        &borrowed,
        allowed,
        canary,
    )
}

fn configured_interpreter_paths(
    config: &[u8],
    binary: &Path,
    libraries: &[String],
) -> Result<BTreeSet<String>, String> {
    let mut home = None;
    for line in std::str::from_utf8(config)
        .map_err(|_| "interpreter configuration is not UTF8")?
        .lines()
    {
        if let Some((key, value)) = line.split_once('=') {
            if key.trim().eq_ignore_ascii_case("home") {
                if home.is_some() {
                    return Err("interpreter home repeated".into());
                }
                home = Some(Path::new(value.trim()));
            }
        }
    }
    let home = home
        .filter(|p| p.is_absolute())
        .ok_or("interpreter home binding missing")?;
    let bin = binary.parent().ok_or("selected interpreter bin missing")?;
    if home
        .canonicalize()
        .map_err(|_| "interpreter home unavailable")?
        != bin
    {
        return Err("configured interpreter home differs".into());
    }
    let logical = home.parent().ok_or("logical interpreter prefix missing")?;
    let canonical = bin.parent().ok_or("selected interpreter prefix missing")?;
    let mut paths = BTreeSet::new();
    for library in libraries {
        if let Some(version) = library.strip_prefix("python") {
            if !version.is_empty() && version.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
                for prefix in [logical, canonical] {
                    for path in [
                        prefix.join("lib").join(library),
                        prefix.join("lib").join(library).join("lib-dynload"),
                        prefix
                            .join("lib")
                            .join(format!("python{}.zip", version.replace('.', ""))),
                    ] {
                        paths.insert(path.to_string_lossy().into_owned());
                    }
                }
            }
        }
    }
    Ok(paths)
}
