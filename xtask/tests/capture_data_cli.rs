use sha2::{Digest, Sha256};
use std::{
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

#[allow(dead_code)]
mod capture_actions {
    include!("../src/capture_actions.rs");
}
#[allow(dead_code)]
mod production_reference {
    include!("../src/capture_data.rs");
    pub fn metadata() -> Result<serde_json::Value, String> {
        current_binding_receipt()
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

const CLI_BINDING_PAIRS: [(&str, &str, usize); 6] = [
    (
        "f5d1974587e1108785745a7bdcc6750cc141a46f62e2418c85ced4b5f16c5314",
        "67e83d88d94036c680d467c7f5f55758027665451fabc8753b5310cc3b8561eb",
        1,
    ),
    (
        "56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714",
        "ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff",
        3,
    ),
    (
        "5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c",
        "365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715",
        4,
    ),
    (
        "1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303",
        "ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0",
        2,
    ),
    (
        "88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10",
        "b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e",
        3,
    ),
    (
        "66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e",
        "bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749",
        3,
    ),
];

fn current_cli_expected_view() -> serde_json::Value {
    let historical = include_str!("fixtures/capture/data_reference_v1.semantic.json");
    assert_eq!(historical.len(), 29056);
    assert_eq!(
        digest(historical.as_bytes()),
        "d752043a3dad4a940ad7955289e1ca79490454cca25881f0940abe0bc79a20b2"
    );
    let mut current = historical.to_owned();
    let mut replacements = 0;
    for (old, new, count) in CLI_BINDING_PAIRS {
        assert_eq!(current.matches(old).count(), count);
        current = current.replace(old, new);
        replacements += count;
    }
    assert_eq!(replacements, 16);
    for (old, _, _) in CLI_BINDING_PAIRS {
        assert!(!current.contains(old));
    }
    assert_eq!(current.len(), 29056);
    assert_eq!(
        digest(current.as_bytes()),
        "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344"
    );
    serde_json::from_str(&current).unwrap()
}

fn cli_expectation_declarations() -> [(&'static str, &'static str, &'static str); 2] {
    [
        (
            "cli-current",
            "data-current-bindings-v2",
            "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344",
        ),
        (
            "cli-historical",
            "data-historical-v1",
            "d752043a3dad4a940ad7955289e1ca79490454cca25881f0940abe0bc79a20b2",
        ),
    ]
}

fn with_cli_expectation_preflight<T>(
    declarations: &[(&str, &str, &str)],
    production: &serde_json::Value,
    launch: impl FnOnce() -> T,
) -> Result<(serde_json::Value, T), String> {
    let mut seen = std::collections::BTreeSet::new();
    for &(name, revision, expected_sha) in declarations {
        if !seen.insert(name) {
            return Err("duplicate expectation consumer".into());
        }
        let expected = match name {
            "cli-current" => (
                "data-current-bindings-v2",
                "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344",
            ),
            "cli-historical" => (
                "data-historical-v1",
                "d752043a3dad4a940ad7955289e1ca79490454cca25881f0940abe0bc79a20b2",
            ),
            _ => return Err("unknown expectation consumer".into()),
        };
        if (revision, expected_sha) != expected {
            return Err("expectation revision differs".into());
        }
    }
    if seen.len() != 2 {
        return Err("missing expectation consumer".into());
    }
    let current = declarations
        .iter()
        .find(|row| row.0 == "cli-current")
        .unwrap();
    let historical = declarations
        .iter()
        .find(|row| row.0 == "cli-historical")
        .unwrap();
    if production["revision"] != current.1
        || production["expected_semantic_sha256"] != current.2
        || production["historical_semantic_sha256"] != historical.2
    {
        return Err("production and CLI expectations differ".into());
    }
    Ok((current_cli_expected_view(), launch()))
}

#[test]
fn data_expectation_preflight_rejects_stale_before_launch() {
    let mut declarations = cli_expectation_declarations();
    declarations[0].1 = "obsolete";
    let mut launches = 0;
    let result = with_cli_expectation_preflight(
        &declarations,
        &production_reference::metadata().unwrap(),
        || {
            launches += 1;
        },
    );
    assert!(result.is_err());
    assert_eq!(launches, 0);
}

#[test]
fn data_expectation_preflight_accepts_current_and_rejects_bad_inventory() {
    let declarations = cli_expectation_declarations();
    let production = production_reference::metadata().unwrap();
    assert!(with_cli_expectation_preflight(&declarations, &production, || ()).is_ok());
    assert!(with_cli_expectation_preflight(&declarations[..1], &production, || ()).is_err());
    let mut duplicate = declarations.to_vec();
    duplicate.push(declarations[0]);
    assert!(with_cli_expectation_preflight(&duplicate, &production, || ()).is_err());
    let mut unknown = declarations;
    unknown[0].0 = "unknown";
    assert!(with_cli_expectation_preflight(&unknown, &production, || ()).is_err());
    for field in ["revision", "expected_semantic_sha256"] {
        let mut stale = production.clone();
        stale[field] = serde_json::json!("obsolete");
        let mut launches = 0;
        assert!(with_cli_expectation_preflight(&declarations, &stale, || {
            launches += 1;
        })
        .is_err());
        assert_eq!(launches, 0);
    }
}

#[test]
fn data_cli_current_expected_view_integrity() {
    let historical: serde_json::Value = serde_json::from_slice(include_bytes!(
        "fixtures/capture/data_reference_v1.semantic.json"
    ))
    .unwrap();
    let current = current_cli_expected_view();
    assert_ne!(historical, current);
    let old_phases = historical.as_array().unwrap();
    let new_phases = current.as_array().unwrap();
    assert_eq!(old_phases.len(), 19);
    assert_eq!(new_phases.len(), old_phases.len());
    let mut reader_slots = Vec::new();
    let mut writers = 0;
    let mut calls = 0;
    for (old_phase, new_phase) in old_phases.iter().zip(new_phases) {
        let old_calls = old_phase["calls"].as_array().unwrap();
        let new_calls = new_phase["calls"].as_array().unwrap();
        assert_eq!(old_calls.len(), new_calls.len());
        let mut expected_phase = old_phase.clone();
        for (index, (old_call, new_call)) in old_calls.iter().zip(new_calls).enumerate() {
            calls += 1;
            if old_phase["role"] == "reader" {
                reader_slots.push((old_phase["case"].as_str().unwrap(), index + 1));
                let old_digest = old_call["arguments_sha256"].as_str().unwrap();
                let (old, new, _) = CLI_BINDING_PAIRS
                    .iter()
                    .find(|(old, _, _)| *old == old_digest)
                    .unwrap();
                assert_eq!(old_digest, *old);
                assert_eq!(new_call["arguments_sha256"], *new);
                let mut expected_call = old_call.clone();
                expected_call["arguments_sha256"] = serde_json::json!(new);
                assert_eq!(new_call, &expected_call);
                expected_phase["calls"][index] = expected_call;
            } else {
                assert_eq!(old_phase["role"], "writer");
                assert_eq!(old_call, new_call);
                writers += 1;
            }
        }
        assert_eq!(new_phase, &expected_phase);
    }
    assert_eq!(
        reader_slots,
        vec![
            ("1", 1),
            ("2", 1),
            ("3", 1),
            ("3", 2),
            ("4", 1),
            ("4", 2),
            ("5", 1),
            ("5", 2),
            ("6", 1),
            ("6", 2),
            ("7", 1),
            ("7", 2),
            ("8", 1),
            ("9", 1),
            ("9", 2),
            ("10", 1),
        ]
    );
    assert_eq!(writers, 9);
    assert_eq!(calls, 25);
}

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
fn scope(case: &str) -> PathBuf {
    let base = root().join(".metrics/data-executable-wiring-cli");
    fs::create_dir_all(&base).unwrap();
    let directory = tempfile::Builder::new()
        .prefix(&format!("{case}-"))
        .tempdir_in(base)
        .unwrap()
        .keep();
    fs::write(
        directory.join("owner.json"),
        serde_json::to_vec(
            &serde_json::json!({"scope":"one synthetic CLI test invocation","case":case}),
        )
        .unwrap(),
    )
    .unwrap();
    directory
}
fn invoke(directory: &Path, args: &[OsString]) -> Output {
    let binary = Path::new(env!("CARGO_BIN_EXE_careerbrew-xtask"));
    let output = Command::new(binary)
        .args(args)
        .current_dir(root())
        .output()
        .unwrap();
    let receipt = serde_json::json!({"scope":"actual default executable CLI invocation only","binary":binary,"binary_sha256":digest(&fs::read(binary).unwrap()),"args":args,"exit":output.status.code(),"stdout":String::from_utf8(output.stdout.clone()).unwrap(),"stderr":String::from_utf8(output.stderr.clone()).unwrap()});
    fs::write(
        directory.join("command.json"),
        serde_json::to_vec_pretty(&receipt).unwrap(),
    )
    .unwrap();
    output
}
fn arguments(python: &Path, output: &Path) -> Vec<OsString> {
    vec![
        "migration".into(),
        "capture-data".into(),
        "--python".into(),
        python.as_os_str().to_owned(),
        "--out".into(),
        output.as_os_str().to_owned(),
    ]
}
fn no_capture(output: &Path) {
    for path in [
        output.to_owned(),
        output.with_extension("raw"),
        output.with_extension("runtime"),
        output.with_extension("attempt.json"),
        output.with_extension("outcome.json"),
    ] {
        assert!(
            !path.exists(),
            "unexpected capture output {}",
            path.display()
        );
    }
}
#[test]
fn data_cli_rejects_invalid_grammar() {
    for case in ["missing-out", "reordered", "unknown-root", "extra-token"] {
        let directory = scope(case);
        let output = directory.join("candidate.json");
        let python = directory.join("absent-python");
        let args = match case {
            "missing-out" => vec![
                "migration".into(),
                "capture-data".into(),
                "--python".into(),
                python.as_os_str().to_owned(),
            ],
            "reordered" => vec![
                "migration".into(),
                "capture-data".into(),
                "--out".into(),
                output.as_os_str().to_owned(),
                "--python".into(),
                python.as_os_str().to_owned(),
            ],
            "unknown-root" => vec![
                "migration".into(),
                "capture-data".into(),
                "--source-root".into(),
                directory.as_os_str().to_owned(),
            ],
            "extra-token" => {
                let mut args = arguments(&python, &output);
                args.push("extra".into());
                args
            }
            _ => unreachable!(),
        };
        let observed = invoke(&directory, &args);
        assert_eq!(observed.status.code(), Some(2));
        let usage = String::from_utf8(observed.stderr).unwrap();
        assert!(usage.contains("usage: cargo xtask migration self-test"));
        assert!(usage.contains("migration capture-actions --python"));
        assert!(usage.contains("migration capture-http --python"));
        no_capture(&output);
    }
}
#[test]
fn data_cli_refuses_existing_output() {
    let directory = scope("existing-output");
    let output = directory.join("candidate.json");
    let marker = b"synthetic existing receipt";
    fs::write(&output, marker).unwrap();
    fs::set_permissions(&output, fs::Permissions::from_mode(0o600)).unwrap();
    let observed = invoke(
        &directory,
        &arguments(&root().join(".venv/bin/python"), &output),
    );
    assert_eq!(observed.status.code(), Some(1));
    assert_eq!(fs::read(&output).unwrap(), marker);
    assert_eq!(
        fs::metadata(&output).unwrap().permissions().mode() & 0o777,
        0o600
    );
}
#[test]
fn data_cli_refuses_missing_interpreter() {
    let directory = scope("missing-interpreter");
    let output = directory.join("candidate.json");
    let observed = invoke(
        &directory,
        &arguments(&directory.join("absent-python"), &output),
    );
    assert_eq!(observed.status.code(), Some(1));
    assert!(!output.exists());
}
#[test]
fn data_cli_captures_frozen_data_contract() {
    let directory = scope("positive");
    let output = directory.join("candidate.json");
    let (golden, observed) = with_cli_expectation_preflight(
        &cli_expectation_declarations(),
        &production_reference::metadata().unwrap(),
        || {
            invoke(
                &directory,
                &arguments(&root().join(".venv/bin/python"), &output),
            )
        },
    )
    .unwrap();
    assert_eq!(observed.status.code(), Some(0));
    let candidate: serde_json::Value = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
    assert_eq!(candidate["schema"], "careerbrew-data-candidate-v1");
    assert_eq!(candidate["qualification"], "reference-review-pending");
    assert_eq!(
        candidate["inputs"]["current_binding"],
        serde_json::json!({
            "revision": "data-current-bindings-v2",
            "manifest_sha256": "05974343395faf40eb5112aad4381d417ca8ef57a730eee53ec7b2e14b1447fb",
            "expected_semantic_sha256": "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344",
            "historical_fixture_sha256": "6025c63de77ed2ed9fb6a69a95860a33b38b675840edbf187405918c23222fe2",
            "historical_semantic_sha256": "d752043a3dad4a940ad7955289e1ca79490454cca25881f0940abe0bc79a20b2",
            "historical_provenance_sha256": "b020b2b3d1d38a48ceeeb41eba781e66cd0732446b2a9f337bdc06d536b51fba",
            "scope": "expected-side comparison metadata; no new observed reference"
        })
    );
    assert_eq!(candidate["semantic_projection"], golden);
    assert_eq!(
        candidate["capture_executable_sha256"],
        digest(&fs::read(env!("CARGO_BIN_EXE_careerbrew-xtask")).unwrap())
    );
    let trials = candidate["trials"].as_array().unwrap();
    assert_eq!(trials.len(), 2);
    let mut pids = std::collections::BTreeSet::new();
    let mut count = 0;
    for trial in trials {
        let phases = trial["phases"].as_array().unwrap();
        assert_eq!(phases.len(), 19);
        for phase in phases {
            let owner = &phase["owner_observation"];
            assert!(pids.insert(owner["pid"].as_u64().unwrap()));
            assert_eq!(owner["wait_code"], 0);
            assert!(owner["wait_signal"].is_null());
            for flag in ["wait_observed", "reaped", "cleanup_succeeded"] {
                assert_eq!(owner[flag], true);
            }
            assert_eq!(owner["program"], phase["program"]);
            assert_eq!(owner["args"], phase["args"]);
            assert!(phase["deferred_differences"].as_array().unwrap().is_empty());
            count += phase["report"]["api_calls"].as_array().unwrap().len();
        }
    }
    assert_eq!(pids.len(), 38);
    assert_eq!(count, 50);
}
#[test]
fn data_cli_refuses_missing_python_flag() {
    let directory = scope("missing-python");
    let output = directory.join("candidate.json");
    let observed = invoke(
        &directory,
        &[
            "migration".into(),
            "capture-data".into(),
            "--out".into(),
            output.as_os_str().to_owned(),
        ],
    );
    assert_eq!(observed.status.code(), Some(2));
    assert!(String::from_utf8(observed.stderr)
        .unwrap()
        .contains("usage:"));
    no_capture(&output);
}
