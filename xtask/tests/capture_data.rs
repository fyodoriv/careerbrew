#[path = "support/actions_controls.rs"]
mod original_cli_controls;
#[path = "support/http_controls.rs"]
mod original_http_controls;
#[path = "support/owner_controls.rs"]
mod original_owner_controls;
mod capture_actions {
    include!("../src/capture_actions.rs");
}
use std::{path::Path, process::Command};

#[test]
fn current_binding_metadata_exact_current_acceptance() {
    capture_data::assert_current_binding_acceptance();
}

#[test]
fn current_binding_metadata_obsolete_reader_refusal() {
    capture_data::assert_obsolete_binding_refusal();
}

#[test]
fn current_binding_metadata_binding_negative_preservation() {
    capture_data::assert_binding_negative_preservation();
}

#[test]
fn current_binding_metadata_semantic_preservation() {
    capture_data::assert_binding_semantic_preservation();
}

#[test]
fn current_binding_metadata_manifest_contract() {
    capture_data::assert_current_binding_manifest_contract();
}

#[test]
fn shared_reference_inputs_refuse_changes_before_python() {
    original_http_controls::rejects_changed_reference_inputs_before_python();
}

#[test]
fn current_binding_metadata_preflight_before_bootstrap() {
    capture_data::assert_preflight_before_bootstrap();
}

#[test]
fn actual_data_oracle_executes_missing_read_phase() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let owned = scratch.path().join("owned");
    let owned = owned
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap()
        .join("owned");
    let mut environment = capture_actions::owned_environment(root, &owned).unwrap();
    environment.insert(
        "CAREERBREW_DATA_OWNED".into(),
        owned.to_string_lossy().into_owned(),
    );
    std::fs::create_dir(owned.join("data/contract")).unwrap();
    std::fs::create_dir(owned.join("protocol")).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(
        owned.join("data/contract"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    std::fs::set_permissions(
        owned.join("protocol"),
        std::fs::Permissions::from_mode(0o700),
    )
    .unwrap();
    let fixture = root.join("docs/plans/2026-11-05-rust-data-fixture-v1.json");
    for (name, value) in [
        (
            "PYTHONPATH",
            root.join("xtask/tests/fixtures/capture/observer"),
        ),
        ("CAREERBREW_CAPTURE_INPUT", fixture.clone()),
        ("CAREERBREW_DATA_FIXTURE", fixture),
        ("CAREERBREW_CAPTURE_ROOT", root.to_owned()),
        (
            "CAREERBREW_CAPTURE_OBSERVER",
            owned.join("protocol/01-reader-observer.json"),
        ),
        (
            "CAREERBREW_DATA_REPORT",
            owned.join("protocol/01-reader-api.json"),
        ),
    ] {
        environment.insert(name.into(), value.to_string_lossy().into_owned());
    }
    let mut command = Command::new(root.join(".venv/bin/python"));
    command
        .args(["-B", "-S"])
        .arg(root.join("xtask/tests/fixtures/capture/data_driver.py"))
        .args(["first", "1", "reader"])
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let evidence = root.join(".metrics/data-first-phase");
    std::fs::create_dir_all(&evidence).unwrap();
    let input = evidence.join("inputs.json");
    if !input.exists() {
        let names = [
            "xtask/tests/capture_data.rs",
            "xtask/tests/fixtures/capture/data_driver.py",
            "docs/plans/2026-11-05-rust-data-fixture-v1.json",
            "docs/plans/2026-11-05-rust-data-value-schema.json",
            "docs/plans/2026-11-05-rust-data-phase-schema.json",
        ];
        let pins = names
            .into_iter()
            .map(|name| {
                (
                    name,
                    capture_actions::digest(&std::fs::read(root.join(name)).unwrap()),
                )
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        let version = capture_actions::version(&root.join(".venv/bin/python")).unwrap();
        std::fs::write(&input,serde_json::to_vec_pretty(&serde_json::json!({"scope":"unqualified diagnostic missing-read phase, not corpus reference","pins":pins,"provider_sources":capture_actions::sources(root).unwrap(),"python_version":version})).unwrap()).unwrap();
    }
    let (pid, exit, stdout, stderr) =
        capture_actions::execute_during(command, |_, _| Ok(())).unwrap();
    assert_eq!(exit,Some(0),"actual data oracle must execute the registered read phase; stdout={stdout:?}; stderr={stderr:?}");
    if !evidence.join("api.json").exists() {
        std::fs::copy(
            owned.join("protocol/01-reader-api.json"),
            evidence.join("api.json"),
        )
        .unwrap();
        std::fs::copy(
            owned.join("protocol/01-reader-observer.json"),
            evidence.join("observer.json"),
        )
        .unwrap();
        std::fs::write(
            evidence.join("execution.json"),
            serde_json::to_vec_pretty(
                &serde_json::json!({"pid":pid,"exit":exit,"stdout":stdout,"stderr":stderr}),
            )
            .unwrap(),
        )
        .unwrap();
    }
    let report: serde_json::Value =
        serde_json::from_slice(&std::fs::read(owned.join("protocol/01-reader-api.json")).unwrap())
            .unwrap();
    assert_eq!(report["case"], "1");
    assert_eq!(report["role"], "reader");
    assert_eq!(report["api_calls"].as_array().unwrap().len(), 1);
}

mod capture_data {
    include!("../src/capture_data.rs");

    pub fn assert_preflight_before_bootstrap() {
        use std::cell::Cell;
        let launches = Cell::new(0);
        let refused = with_current_binding_preflight("obsolete", || {
            launches.set(launches.get() + 1);
            Ok(())
        });
        assert!(refused.is_err());
        assert_eq!(launches.get(), 0);
        let accepted = with_current_binding_preflight("data-current-bindings-v2", || {
            launches.set(launches.get() + 1);
            Ok(())
        });
        assert!(accepted.is_ok());
        assert_eq!(launches.get(), 1);
        assert!(reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE).is_ok());
    }

    const CURRENT_BINDING_ROWS: [(&str, &str, &str, &str); 16] = [
        (
            "1",
            "primary",
            "f5d1974587e1108785745a7bdcc6750cc141a46f62e2418c85ced4b5f16c5314",
            "67e83d88d94036c680d467c7f5f55758027665451fabc8753b5310cc3b8561eb",
        ),
        (
            "2",
            "primary",
            "56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714",
            "ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff",
        ),
        (
            "3",
            "primary",
            "5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c",
            "365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715",
        ),
        (
            "3",
            "serialized-bytes",
            "5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c",
            "365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715",
        ),
        (
            "4",
            "primary",
            "1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303",
            "ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0",
        ),
        (
            "4",
            "serialized-bytes",
            "1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303",
            "ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0",
        ),
        (
            "5",
            "primary",
            "88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10",
            "b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e",
        ),
        (
            "5",
            "serialized-bytes",
            "66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e",
            "bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749",
        ),
        (
            "6",
            "primary",
            "88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10",
            "b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e",
        ),
        (
            "6",
            "serialized-bytes",
            "66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e",
            "bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749",
        ),
        (
            "7",
            "primary",
            "88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10",
            "b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e",
        ),
        (
            "7",
            "serialized-bytes",
            "66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e",
            "bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749",
        ),
        (
            "8",
            "primary",
            "56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714",
            "ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff",
        ),
        (
            "9",
            "primary",
            "5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c",
            "365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715",
        ),
        (
            "9",
            "serialized-bytes",
            "5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c",
            "365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715",
        ),
        (
            "10",
            "primary",
            "56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714",
            "ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff",
        ),
    ];

    fn binding_wire_hash(value: &Value) -> String {
        let encoded = serde_json::to_string(value).unwrap();
        let mut ascii = String::new();
        for ch in encoded.chars() {
            if ch.is_ascii() {
                ascii.push(ch);
            } else {
                for unit in ch.encode_utf16(&mut [0; 2]) {
                    ascii.push_str(&format!("\\u{unit:04x}"));
                }
            }
        }
        shared::digest(ascii.as_bytes())
    }

    fn binding_reference(current: bool) -> Value {
        assert_eq!(shared::digest(FIXTURE.as_bytes()), FIXTURE_SHA);
        let historical = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE).unwrap();
        if !current {
            return historical;
        }
        let mut text = REFERENCE_SEMANTIC.to_owned();
        for (_, _, old, new) in CURRENT_BINDING_ROWS {
            text = text.replace(old, new);
        }
        assert_eq!(
            shared::digest(text.as_bytes()),
            "c131738a8ac0f7df93328041d26d30883dfef70ff1a0bbf668a907cb6e741344"
        );
        strict_json(text.as_bytes()).unwrap()
    }

    fn binding_report(case: &Value, phase: &Value, current: bool) -> Value {
        let reference = binding_reference(current);
        let projection = reference
            .as_array()
            .unwrap()
            .iter()
            .find(|row| row["case"] == case["case"] && row["role"] == phase["role"])
            .unwrap();
        let source = "/synthetic-capture-root/src/careerbrew/store.py";
        let calls = phase["ledger"].as_array().unwrap().iter().zip(
            projection["calls"].as_array().unwrap()
        ).map(|(spec, observed)| json!({
            "sequence":spec["sequence"],"purpose":spec["purpose"],
            "function":spec["function"],"arguments_sha256":observed["arguments_sha256"],
            "target":spec["observed_target"],"module_path":source,
            "loader_origin":source,"function_source":source,
            "source_sha256":"1111111111111111111111111111111111111111111111111111111111111111",
            "started":true,"returned":observed["returned"],"completed":true
        })).collect::<Vec<_>>();
        json!({"schema":"careerbrew-data-phase-v1","trial":"first",
            "case":case["case"],"role":phase["role"],"pid":7,
            "fixture_sha256":FIXTURE_SHA,"api_calls":calls})
    }

    fn check_binding_report(
        report: &Value,
        case: &Value,
        phase: &Value,
        current: bool,
    ) -> Result<(), String> {
        let root = Path::new("/synthetic-capture-root");
        let sources = json!({"src/careerbrew/store.py":
            "synthetic:1111111111111111111111111111111111111111111111111111111111111111"});
        if current {
            verify_current_report_structure(report, "first", case, phase, 7, root, &sources)
        } else {
            verify_report_structure(report, "first", case, phase, 7, root, &sources)
        }
    }

    pub fn assert_current_binding_acceptance() {
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let mut failures = Vec::new();
        let mut readers = 0;
        let mut writers = 0;
        for case in fixture["cases"].as_array().unwrap() {
            for phase in case["phases"].as_array().unwrap() {
                let report = binding_report(case, phase, true);
                let count = report["api_calls"].as_array().unwrap().len();
                if phase["role"] == "reader" {
                    readers += count;
                } else {
                    writers += count;
                }
                if let Err(error) = check_binding_report(&report, case, phase, true) {
                    failures.push(format!("{}:{}:{error}", case["case"], phase["role"]));
                }
            }
        }
        assert_eq!((readers, writers), (16, 9));
        let current = binding_reference(true);
        if let Err(error) = compare_current_data_semantics(&current, &current) {
            failures.push(error);
        }
        assert!(
            failures.is_empty(),
            "current binding acceptance: {failures:?}"
        );
    }

    pub fn assert_obsolete_binding_refusal() {
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let historical = binding_reference(false);
        let current = binding_reference(true);
        let mut failures = Vec::new();
        if compare_current_data_semantics(&historical, &historical).is_ok() {
            failures.push("complete historical projection accepted".to_owned());
        }
        for case in fixture["cases"].as_array().unwrap() {
            for phase in case["phases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["role"] == "reader")
            {
                if check_binding_report(&binding_report(case, phase, false), case, phase, true)
                    .is_ok()
                {
                    failures.push(format!("historical reader accepted: {}", case["case"]));
                }
            }
        }
        for (case_id, purpose, old, _) in CURRENT_BINDING_ROWS {
            let case = fixture["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["case"] == case_id)
                .unwrap();
            let phase = case["phases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["role"] == "reader")
                .unwrap();
            let mut report = binding_report(case, phase, true);
            let call = report["api_calls"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|c| c["purpose"] == purpose)
                .unwrap();
            call["arguments_sha256"] = json!(old);
            if check_binding_report(&report, case, phase, true).is_ok() {
                failures.push(format!("missing default accepted: {case_id}:{purpose}"));
            }
            let mut incomplete = current.clone();
            let row = incomplete
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["case"] == case_id && p["role"] == "reader")
                .unwrap();
            row["calls"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|c| c["purpose"] == purpose)
                .unwrap()["arguments_sha256"] = json!(old);
            if compare_current_data_semantics(&incomplete, &incomplete).is_ok() {
                failures.push(format!(
                    "incomplete projection accepted: {case_id}:{purpose}"
                ));
            }
        }
        assert!(
            failures.is_empty(),
            "obsolete binding refusal: {failures:?}"
        );
    }

    pub fn assert_binding_negative_preservation() {
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let historical = binding_reference(false);
        assert!(compare_projection_against(&historical, &historical, &historical).is_ok());
        let mut writers = 0;
        for case in fixture["cases"].as_array().unwrap() {
            for phase in case["phases"].as_array().unwrap() {
                let baseline = binding_report(case, phase, false);
                assert!(check_binding_report(&baseline, case, phase, false).is_ok());
                if phase["role"] != "reader" {
                    writers += baseline["api_calls"].as_array().unwrap().len();
                    continue;
                }
                for (index, spec) in phase["ledger"].as_array().unwrap().iter().enumerate() {
                    let bindings = spec["expected_bound_arguments"].clone();
                    let old = binding_wire_hash(&bindings);
                    let row = CURRENT_BINDING_ROWS
                        .iter()
                        .find(|r| case["case"] == r.0 && spec["purpose"] == r.1)
                        .unwrap();
                    assert_eq!(old, row.2);
                    let mut true_bindings = bindings.clone();
                    true_bindings["create_key"] = json!({"kind":"bool","value":true});
                    assert_eq!(binding_wire_hash(&true_bindings), row.3);
                    let mut mutations = Vec::new();
                    for value in [
                        json!({"kind":"bool","value":false}),
                        json!({"kind":"int","decimal":"1"}),
                        json!({"kind":"str","encoding":"literal","value":"true"}),
                    ] {
                        let mut changed = bindings.clone();
                        changed["create_key"] = value;
                        mutations.push(binding_wire_hash(&changed));
                    }
                    let mut extra = true_bindings;
                    extra["unexpected"] = json!({"kind":"null"});
                    mutations.push(binding_wire_hash(&extra));
                    mutations.push(if row.0 == "1" {
                        CURRENT_BINDING_ROWS[1].3.to_owned()
                    } else {
                        CURRENT_BINDING_ROWS[0].3.to_owned()
                    });
                    for digest in mutations {
                        let mut report = baseline.clone();
                        report["api_calls"][index]["arguments_sha256"] = json!(digest);
                        assert_eq!(
                            check_binding_report(&report, case, phase, false).unwrap_err(),
                            "data actual bound arguments differ"
                        );
                        let mut wrong = historical.clone();
                        let projected = wrong
                            .as_array_mut()
                            .unwrap()
                            .iter_mut()
                            .find(|p| p["case"] == case["case"] && p["role"] == phase["role"])
                            .unwrap();
                        projected["calls"][index]["arguments_sha256"] = json!(digest);
                        assert_eq!(
                            compare_projection_against(&wrong, &wrong, &historical).unwrap_err(),
                            "semantic_reference_mismatch"
                        );
                    }
                }
            }
        }
        assert_eq!(writers, 9);
    }

    pub fn assert_binding_semantic_preservation() {
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let historical = binding_reference(false);
        assert_eq!(
            shared::digest(REFERENCE_SEMANTIC.as_bytes()),
            REFERENCE_SEMANTIC_SHA
        );
        assert_eq!(
            shared::digest(REFERENCE_PROVENANCE.as_bytes()),
            REFERENCE_PROVENANCE_SHA
        );
        assert!(compare_projection_against(&historical, &historical, &historical).is_ok());
        let case = fixture["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["case"] == "3")
            .unwrap();
        let phase = case["phases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["role"] == "reader")
            .unwrap();
        let baseline = binding_report(case, phase, false);
        assert!(check_binding_report(&baseline, case, phase, false).is_ok());
        for (field, value, expected) in [
            (
                "target",
                json!({"kind":"path","root":"owned","relative":"data/contract/text.txt"}),
                "data actual target differs",
            ),
            (
                "function",
                json!("careerbrew.store.read_text"),
                "data actual API differs",
            ),
            (
                "purpose",
                json!("serialized-bytes"),
                "data actual API differs",
            ),
            ("sequence", json!("2"), "data actual API differs"),
        ] {
            let mut report = baseline.clone();
            report["api_calls"][0][field] = value;
            assert_eq!(
                check_binding_report(&report, case, phase, false).unwrap_err(),
                expected
            );
        }
        let mut reordered = baseline.clone();
        reordered["api_calls"].as_array_mut().unwrap().swap(0, 1);
        assert_eq!(
            check_binding_report(&reordered, case, phase, false).unwrap_err(),
            "data actual API differs"
        );
        let mut shortened = baseline.clone();
        shortened["api_calls"].as_array_mut().unwrap().pop();
        assert_eq!(
            check_binding_report(&shortened, case, phase, false).unwrap_err(),
            "data API evidence missing"
        );
        let projected_index = historical
            .as_array()
            .unwrap()
            .iter()
            .position(|p| p["case"] == "3" && p["role"] == "reader")
            .unwrap();
        let no_op_index = historical
            .as_array()
            .unwrap()
            .iter()
            .position(|p| p["case"] == "9" && p["role"] == "writer")
            .unwrap();
        let mut mutations = Vec::new();
        for index in [0, 1] {
            let mut wrong = historical.clone();
            wrong[projected_index]["calls"][index]["returned"] = json!({"kind":"null"});
            mutations.push(wrong);
        }
        for (field, value) in [
            (
                "target",
                json!({"kind":"path","root":"owned","relative":"data/contract/text.txt"}),
            ),
            ("function", json!("careerbrew.store.read_text")),
            ("purpose", json!("serialized-bytes")),
        ] {
            let mut wrong = historical.clone();
            wrong[projected_index]["calls"][0][field] = value;
            mutations.push(wrong);
        }
        let mut reordered = historical.clone();
        reordered[projected_index]["calls"]
            .as_array_mut()
            .unwrap()
            .swap(0, 1);
        mutations.push(reordered);
        let mut shortened = historical.clone();
        shortened[projected_index]["calls"]
            .as_array_mut()
            .unwrap()
            .pop();
        mutations.push(shortened);
        let mut no_op = historical.clone();
        assert_eq!(no_op[no_op_index]["no_op_identity_preserved"], true);
        no_op[no_op_index]["no_op_identity_preserved"] = json!(false);
        mutations.push(no_op);
        let mut same_wrong = historical.clone();
        for case_id in ["3", "9"] {
            let row = same_wrong
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|p| p["case"] == case_id && p["role"] == "reader")
                .unwrap();
            let one = row["calls"][0]["returned"]["entries"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
                .find(|e| e["key"]["value"] == "one")
                .unwrap();
            one["value"]["decimal"] = json!("2");
            row["calls"][1]["returned"]["sha256"] = json!("0".repeat(64));
        }
        assert_eq!(same_wrong[no_op_index]["no_op_identity_preserved"], true);
        mutations.push(same_wrong);
        for wrong in mutations {
            assert_eq!(
                compare_projection_against(&wrong, &wrong, &historical).unwrap_err(),
                "semantic_reference_mismatch"
            );
            assert_eq!(
                compare_projection_against(&wrong, &historical, &historical).unwrap_err(),
                "fresh data corpus projections differ"
            );
        }
        let mut writers = 0;
        for case in fixture["cases"].as_array().unwrap() {
            for phase in case["phases"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["role"] == "writer")
            {
                assert!(check_binding_report(
                    &binding_report(case, phase, false),
                    case,
                    phase,
                    false
                )
                .is_ok());
                writers += phase["ledger"].as_array().unwrap().len();
            }
        }
        assert_eq!(writers, 9);
    }

    fn binding_manifest_rows() -> Vec<Value> {
        vec![
            json!({"identity":"1:reader:1","case":"1","role":"reader","sequence":"1","function":"careerbrew.store.read_json","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/missing.json"},"fixture_pointer":"/cases/0/phases/0/ledger/0/expected_bound_arguments/create_key","old_sha256":"f5d1974587e1108785745a7bdcc6750cc141a46f62e2418c85ced4b5f16c5314","new_sha256":"67e83d88d94036c680d467c7f5f55758027665451fabc8753b5310cc3b8561eb"}),
            json!({"identity":"2:reader:1","case":"2","role":"reader","sequence":"1","function":"careerbrew.store.read_text","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/text.txt"},"fixture_pointer":"/cases/1/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff"}),
            json!({"identity":"3:reader:1","case":"3","role":"reader","sequence":"1","function":"careerbrew.store.read_json","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/record.json"},"fixture_pointer":"/cases/2/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715"}),
            json!({"identity":"3:reader:2","case":"3","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/record.json"},"fixture_pointer":"/cases/2/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715"}),
            json!({"identity":"4:reader:1","case":"4","role":"reader","sequence":"1","function":"careerbrew.store.read_yaml","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/record.yaml"},"fixture_pointer":"/cases/3/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303","new_sha256":"ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0"}),
            json!({"identity":"4:reader:2","case":"4","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/record.yaml"},"fixture_pointer":"/cases/3/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"1b7286687aad552f7c382de162bab2b2a9e581938fe4c403e4549c84bb21a303","new_sha256":"ab8f342ea8ca9517b9bdc68d735f51fe937db6f38f060ad103ff31d6a9a604e0"}),
            json!({"identity":"5:reader:1","case":"5","role":"reader","sequence":"1","function":"careerbrew.store.read_jsonl","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/4/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e"}),
            json!({"identity":"5:reader:2","case":"5","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/4/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749"}),
            json!({"identity":"6:reader:1","case":"6","role":"reader","sequence":"1","function":"careerbrew.store.read_jsonl","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/5/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e"}),
            json!({"identity":"6:reader:2","case":"6","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/5/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749"}),
            json!({"identity":"7:reader:1","case":"7","role":"reader","sequence":"1","function":"careerbrew.store.read_jsonl","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/6/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"88ee744c26aa0fe19e324b51d739302f2d5fdd0257e4815f6b41ba2e1156ec10","new_sha256":"b4fc26fcf8feba69e9d6fe38dfd65d49b2584da70c3de065bf000ef8a856d85e"}),
            json!({"identity":"7:reader:2","case":"7","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/rows.jsonl"},"fixture_pointer":"/cases/6/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"66759aebf94ca4989113186f469b0b6524f151973d056ff8e5254d0de72cb47e","new_sha256":"bb106cb5ed8355687dbdf71203be1e8c8d69bd3c517d95a4d49ee9c1c2b0c749"}),
            json!({"identity":"8:reader:1","case":"8","role":"reader","sequence":"1","function":"careerbrew.store.read_text","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/text.txt"},"fixture_pointer":"/cases/7/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff"}),
            json!({"identity":"9:reader:1","case":"9","role":"reader","sequence":"1","function":"careerbrew.store.read_json","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/record.json"},"fixture_pointer":"/cases/8/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715"}),
            json!({"identity":"9:reader:2","case":"9","role":"reader","sequence":"2","function":"careerbrew.store.read_text","purpose":"serialized-bytes","target":{"kind":"path","root":"owned","relative":"data/contract/record.json"},"fixture_pointer":"/cases/8/phases/1/ledger/1/expected_bound_arguments/create_key","old_sha256":"5b42ca47a2f68a05bcf4e9a7e1ee44730e7282042c9c4f03f977cbce426a096c","new_sha256":"365f52e129ca2fcbe22f0f6a6ac6aa979e5e81741de5b10ded4a01133b4dc715"}),
            json!({"identity":"10:reader:1","case":"10","role":"reader","sequence":"1","function":"careerbrew.store.read_text","purpose":"primary","target":{"kind":"path","root":"owned","relative":"data/contract/text.txt"},"fixture_pointer":"/cases/9/phases/1/ledger/0/expected_bound_arguments/create_key","old_sha256":"56226d1c113186180854ffc7c6fd2af916d032970547817a219cfdc9e7819714","new_sha256":"ee5a2ed07f3d38ca03d747d7da7879e7d1a287e6864d3744b8768ed4fd3a7fff"}),
        ]
    }

    pub fn assert_current_binding_manifest_contract() {
        let validate: fn(&[Value]) -> Result<Value, String> = validate_current_binding_manifest;
        let manifest = binding_manifest_rows();
        assert_eq!(manifest.len(), 16);
        assert_eq!(validate(&manifest).unwrap(), binding_reference(true));

        let mut cases = Vec::new();
        let mut duplicate = manifest.clone();
        duplicate[1] = duplicate[0].clone();
        cases.push(("duplicate identity", duplicate, "DuplicateSlot"));

        let mut extra = manifest.clone();
        extra.push(extra[0].clone());
        cases.push(("extra row", extra, "ExtraSlot"));

        let mut missing = manifest.clone();
        missing.pop();
        cases.push(("missing row", missing, "MissingSlot"));

        let mut unknown = manifest.clone();
        unknown[0]["identity"] = json!("unregistered:reader:1");
        cases.push(("unknown identity", unknown, "UnknownSlot"));

        let mut displaced = manifest.clone();
        displaced[0]["fixture_pointer"] = manifest[1]["fixture_pointer"].clone();
        cases.push(("displaced pointer", displaced, "DisplacedSlot"));

        for (field, value) in [
            ("case", json!("2")),
            ("role", json!("writer")),
            ("sequence", json!("2")),
            ("function", json!("careerbrew.store.read_text")),
            ("purpose", json!("serialized-bytes")),
            (
                "target",
                json!({"kind":"path","root":"owned","relative":"data/contract/text.txt"}),
            ),
        ] {
            let mut wrong = manifest.clone();
            wrong[0][field] = value;
            cases.push((field, wrong, "IdentityMismatch"));
        }

        let mut old_digest = manifest.clone();
        old_digest[0]["old_sha256"] =
            json!("0000000000000000000000000000000000000000000000000000000000000000");
        cases.push(("old digest", old_digest, "OldDigestMismatch"));

        let mut new_digest = manifest.clone();
        new_digest[0]["new_sha256"] =
            json!("1111111111111111111111111111111111111111111111111111111111111111");
        cases.push(("new digest", new_digest, "NewDigestMismatch"));

        assert_eq!(cases.len(), 13);
        assert_eq!(cases[0].1.len(), manifest.len());
        assert_eq!(cases[1].1.len(), 17);
        assert_eq!(cases[2].1.len(), 15);
        for (label, wrong, expected) in cases {
            assert_eq!(validate(&wrong).unwrap_err(), expected, "{label}");
        }
    }

    pub fn assert_executable_fault_roundtrip() {
        let schema = strict_json(FAULT_SCHEMA.as_bytes()).unwrap();
        let ids = schema["properties"]["control"]["enum"].as_array().unwrap();
        assert_eq!(ids.len(), 15);
        let variants = [
            ContractFault::JsonOne,
            ContractFault::JsonBool,
            ContractFault::JsonLarge,
            ContractFault::JsonZero,
            ContractFault::YamlDateMap,
            ContractFault::YamlMapDate,
            ContractFault::SameWrong,
            ContractFault::Plaintext,
            ContractFault::Stdout,
            ContractFault::Stderr,
            ContractFault::ExtraWrite,
            ContractFault::Empty,
            ContractFault::Nonzero,
            ContractFault::Hang,
            ContractFault::Rewrite,
        ];
        let actual = variants
            .iter()
            .map(|v| v.id())
            .collect::<std::collections::BTreeSet<_>>();
        let expected = ids
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(actual, expected);
        for id in ids {
            let id = id.as_str().unwrap();
            assert_eq!(ContractFault::from_id(id).unwrap().id(), id);
        }
    }
    pub fn assert_executable_fault_equivalence(invalid: bool) {
        let schema = strict_json(FAULT_SCHEMA.as_bytes()).unwrap();
        let ids = schema["properties"]["control"]["enum"].as_array().unwrap();
        let values = if invalid {
            vec![
                json!(null),
                json!(true),
                json!(1),
                json!(""),
                json!("unknown"),
                json!("JSON-ONE"),
                json!("json-one "),
            ]
        } else {
            ids.clone()
        };
        assert_eq!(values.len(), if invalid { 7 } else { 15 });
        for value in values {
            let old = ids.contains(&value);
            assert_eq!(fault_control_known(&value, &schema), old);
            assert_eq!(old, !invalid);
        }
    }

    pub fn assert_phase_owner_record() {
        let observation = crate::capture_actions::OwnerObservation {
            pid: 123,
            program: std::ffi::OsString::from("synthetic-python"),
            args: vec![std::ffi::OsString::from("-B")],
            wait_code: Some(0),
            wait_signal: None,
            wait_observed: true,
            reaped: true,
            cleanup_succeeded: true,
        };
        let record = phase_owner_record(
            Some(&observation),
            Path::new("/synthetic/root"),
            Path::new("/synthetic/owned"),
        );
        assert_eq!(record["observation"]["pid"], 123);
        assert_eq!(record["observation"]["wait_code"], 0);
        assert_eq!(record["observation"]["reaped"], true);
        let mut unknown = observation.clone();
        unknown.wait_code = None;
        unknown.wait_observed = false;
        unknown.reaped = false;
        unknown.cleanup_succeeded = false;
        let unconfirmed = phase_owner_record(
            Some(&unknown),
            Path::new("/synthetic/root"),
            Path::new("/synthetic/owned"),
        );
        for flag in ["wait_observed", "reaped", "cleanup_succeeded"] {
            assert_eq!(unconfirmed["observation"][flag], false);
        }
        assert!(unconfirmed["observation"]["wait_code"].is_null());
        assert!(phase_owner_record(
            None,
            Path::new("/synthetic/root"),
            Path::new("/synthetic/owned")
        )["observation"]
            .is_null());
    }
    pub fn assert_phase_owner_retention() {
        let scratch = tempfile::tempdir().unwrap();
        let owned = scratch.path().join("owned");
        std::fs::create_dir_all(owned.join("protocol")).unwrap();
        let allowed = BTreeSet::from(["/synthetic/env".to_owned(), "/synthetic/python".to_owned()]);
        let observer=json!({"catalog_calls":0,"modules":{},"external_modules":{},"sys_path":[],"prefix":"/synthetic/env","base_prefix":"/synthetic/python","site_inputs":{},"distributions":[],"descendant":null,"dependency_scope":"synthetic","unresolved":[]}).to_string();
        let api=json!({"schema":"careerbrew-data-phase-v1","trial":"first","case":"1","role":"reader","pid":7,"fixture_sha256":FIXTURE_SHA,"api_calls":[]}).to_string();
        let records = [
            ("api.json", "protocol/api.json"),
            ("observer.json", "protocol/observer.json"),
        ];
        for (name, raw, terminal) in [
            ("missing-identity", api.clone(), true),
            (
                "wrong-identity",
                api.replace(
                    "\"api_calls\":[]",
                    "\"api_calls\":[],\"driver_provenance\":{}",
                ),
                true,
            ),
            ("malformed", "not-json".into(), true),
            ("unconfirmed", "SYNTHETIC-CANARY".into(), false),
        ] {
            std::fs::write(owned.join("protocol/api.json"), raw).unwrap();
            std::fs::write(owned.join("protocol/observer.json"), &observer).unwrap();
            let output = scratch.path().join(name);
            std::fs::create_dir(&output).unwrap();
            retain_protocols(&output,&owned,&json!({"protocol_scope":"phase","primary_failure":{"category":"original-primary"}}),&allowed,"SYNTHETIC-CANARY",("original-primary",terminal),(&records,phase_protocol_shape)).unwrap();
            let inventory =
                strict_json(&std::fs::read(output.join("protocol-inventory.json")).unwrap())
                    .unwrap();
            assert_eq!(inventory["category"], "original-primary");
            if !terminal {
                for entry in inventory["expected"].as_array().unwrap() {
                    assert_eq!(entry["state"], "owner-terminal-unconfirmed");
                    assert!(entry["bytes"].is_null());
                    assert!(entry["sha256"].is_null());
                }
                assert!(!output.join("retained/api.json").exists());
                assert!(!output.join("retained/observer.json").exists());
            } else {
                assert!(output.join("retained/observer.json").is_file());
                assert_eq!(
                    output.join("retained/api.json").is_file(),
                    name != "malformed"
                );
                if name != "malformed" {
                    assert!(verify_driver_provenance(
                        &strict_json(&std::fs::read(output.join("retained/api.json")).unwrap())
                            .unwrap(),
                        7,
                        Path::new("/synthetic/source")
                    )
                    .is_err());
                }
            }
        }
    }
    pub fn assert_phase_primary_failure() {
        let scratch = tempfile::tempdir().unwrap();
        let owned = scratch.path().join("owned");
        std::fs::create_dir(&owned).unwrap();
        let mut actual_primary = Vec::new();
        for name in ["execution", "inventory", "secondary"] {
            let output = scratch.path().join(name);
            std::fs::create_dir(&output).unwrap();
            std::fs::create_dir(output.join(if name == "execution" {
                "execution.json"
            } else {
                "protocol-inventory.json"
            }))
            .unwrap();
            if name == "secondary" {
                std::fs::create_dir(output.join("retention-error.json")).unwrap();
            }
            let execution = json!({"primary_failure":{"category":"phase_deadline","cause":"Deadline"},"observation":null,"protocol_scope":"phase"});
            actual_primary.push(
                persist_phase_evidence(
                    &output,
                    &owned,
                    &execution,
                    &BTreeSet::new(),
                    "SYNTHETIC-CANARY",
                    ("protocol/api.json", "protocol/observer.json", None),
                )
                .unwrap_err(),
            );
        }
        assert_eq!(actual_primary, vec!["phase_deadline"; 3]);
        for name in ["execution", "inventory"] {
            let output = scratch.path().join(format!("valid-{name}"));
            std::fs::create_dir(&output).unwrap();
            std::fs::create_dir(output.join(if name == "execution" {
                "execution.json"
            } else {
                "protocol-inventory.json"
            }))
            .unwrap();
            assert!(persist_phase_evidence(
                &output,
                &owned,
                &json!({"primary_failure":null,"observation":{"pid":7,"wait_code":0,"wait_signal":null,"wait_observed":true,"reaped":true,"cleanup_succeeded":true},"protocol_scope":"phase"}),
                &BTreeSet::new(),
                "SYNTHETIC-CANARY",
                ("protocol/api.json", "protocol/observer.json",None)
            )
            .is_err());
        }
    }
    pub fn assert_phase_external_hash() {
        let mut observer = json!({"catalog_calls":0,"modules":{},"external_modules":{"synthetic":{"path":"/synthetic/module.py","sha256":"a".repeat(64)}},"sys_path":[],"prefix":"/synthetic/env","base_prefix":"/synthetic/python","site_inputs":{},"distributions":[],"descendant":null,"dependency_scope":"synthetic","unresolved":[]});
        assert!(phase_protocol_shape("observer.json", &observer).is_ok());
        for hash in ["a".repeat(3), "A".repeat(64), "z".repeat(64), String::new()] {
            observer["external_modules"]["synthetic"]["sha256"] = json!(hash);
            assert!(
                phase_protocol_shape("observer.json", &observer).is_err(),
                "{hash:?}"
            );
        }
    }
    pub fn assert_phase_cohort() {
        let identity=(1..=3).map(|pid|json!({"observation":{"pid":pid,"wait_observed":true,"reaped":true,"cleanup_succeeded":true},"os_reap_absence_verified":true})).collect::<Vec<_>>();
        let mut phases=(4..=10).map(|pid|json!({"observation":{"pid":pid,"wait_observed":true,"reaped":true,"cleanup_succeeded":true},"os_reap_absence_verified":true})).collect::<Vec<_>>();
        assert!(validate_phase_cohort(&identity, &phases).is_ok());
        phases[0]["observation"]["pid"] = json!(1);
        assert!(validate_phase_cohort(&identity, &phases).is_err());
        phases[0]["observation"]["pid"] = Value::Null;
        assert!(validate_phase_cohort(&identity, &phases).is_err());
    }
    pub fn assert_mandatory_observer_qualification() {
        let root = Path::new("/synthetic/source");
        let path = "/synthetic/source/src/careerbrew/store.py";
        let sources = json!({"src/careerbrew/store.py":format!("file:644:{}","b".repeat(64))});
        let mut observer = json!({"catalog_calls":0,"modules":{"careerbrew.store":{"path":path,"loader_origin":path,"sha256":"b".repeat(64)}},"external_modules":{"synthetic":{"path":"/synthetic/module.py","sha256":"a".repeat(64)}},"sys_path":["/synthetic/env"],"prefix":"/synthetic/env","base_prefix":"/synthetic/python","site_inputs":{"/synthetic/env/package/METADATA":"c".repeat(64)},"distributions":[{"name":"synthetic"}],"descendant":null,"dependency_scope":"synthetic","unresolved":["synthetic-native-boundary"]});
        assert!(qualify_phase_observer(&observer, root, &sources).is_ok());
        let mut accepted = Vec::new();
        for hash in ["a".repeat(3), "A".repeat(64), "z".repeat(64), String::new()] {
            observer["external_modules"]["synthetic"]["sha256"] = json!(hash);
            assert!(phase_protocol_shape("observer.json", &observer).is_err());
            accepted.push(qualify_phase_observer(&observer, root, &sources).is_ok());
        }
        assert_eq!(accepted, vec![false; 4]);
    }
    pub fn assert_candidate_pre_provider_failure() {
        let scratch = tempfile::tempdir().unwrap();
        let output = scratch.path().join("candidate.json");
        assert_eq!(
            record_candidate_attempt(&output, || Err("synthetic_before_provider".into()))
                .unwrap_err(),
            "synthetic_before_provider"
        );
        let start =
            strict_json(&std::fs::read(output.with_extension("attempt.json")).unwrap()).unwrap();
        let outcome =
            strict_json(&std::fs::read(output.with_extension("outcome.json")).unwrap()).unwrap();
        assert_eq!(start["provider_execution"], "unobserved");
        assert_eq!(outcome["rust_result"], "err");
        assert_eq!(outcome["safe_error"], "synthetic_before_provider");
        for record in [&start, &outcome] {
            for name in [
                "pid",
                "wait_code",
                "wait_observed",
                "reaped",
                "cleanup_succeeded",
            ] {
                assert!(record.get(name).is_none());
            }
        }
        for path in [
            &output,
            &output.with_extension("raw"),
            &output.with_extension("runtime"),
        ] {
            assert!(!path.exists());
        }
        use std::os::unix::fs::PermissionsExt;
        for path in [
            output.with_extension("attempt.json"),
            output.with_extension("outcome.json"),
        ] {
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    pub fn assert_candidate_private_error() {
        let scratch = tempfile::tempdir().unwrap();
        let output = scratch.path().join("candidate.json");
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let canary = fixture["canary"].as_str().unwrap();
        assert_eq!(
            record_candidate_attempt(&output, || Err(canary.to_owned())).unwrap_err(),
            canary
        );
        let raw = std::fs::read(output.with_extension("outcome.json")).unwrap();
        assert!(!String::from_utf8_lossy(&raw).contains(canary));
        let outcome = strict_json(&raw).unwrap();
        assert!(outcome["safe_error"].is_null());
        assert_eq!(outcome["error_sha256"], shared::digest(canary.as_bytes()));
    }
    pub fn assert_candidate_outcome_write_failure(success: bool) {
        let scratch = tempfile::tempdir().unwrap();
        let output = scratch.path().join("candidate.json");
        let called = std::cell::Cell::new(false);
        let result = record_candidate_attempt(&output, || {
            called.set(true);
            std::fs::create_dir(output.with_extension("outcome.json")).unwrap();
            if success {
                Ok(())
            } else {
                Err("synthetic_primary_refusal".into())
            }
        });
        assert!(called.get());
        assert_eq!(
            result.unwrap_err(),
            if success {
                "candidate_outcome_unavailable"
            } else {
                "synthetic_primary_refusal"
            }
        );
    }
    pub fn assert_data_contract_calendar() {
        for (kind, iso) in [
            ("date", "2026-03-01"),
            ("datetime", "2026-03-01T09:30:00+00:00"),
        ] {
            assert!(node(&json!({"kind":kind,"iso":iso}), 0).is_ok());
        }
        let rejected = [
            ("date", "2026-02-30"),
            ("date", "2023-02-29"),
            ("date", "2026-13-01"),
            ("datetime", "2026-03-01T25:30:00+00:00"),
            ("datetime", "2026-03-01T09:30:00+99:00"),
        ]
        .iter()
        .map(|(kind, iso)| node(&json!({"kind":kind,"iso":iso}), 0).is_err())
        .collect::<Vec<_>>();
        assert_eq!(rejected, vec![true; 5]);
    }
    pub fn assert_data_contract_priority() {
        assert_eq!(
            phase_completion(Err("data typed return differs".into()), || Err(
                "no_op_identity_mismatch".into()
            ))
            .unwrap_err(),
            "no_op_identity_mismatch"
        );
    }
    pub fn assert_data_contract_golden() {
        let golden = strict_json(include_bytes!(
            "fixtures/capture/data_reference_v1.semantic.json"
        ))
        .unwrap();
        assert!(compare_data_semantics(&golden, &golden).is_ok());
        let mut wrong = golden.clone();
        let primary = &mut wrong
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|phase| phase["case"] == "3" && phase["role"] == "reader")
            .unwrap()["calls"][0]["returned"]["entries"];
        primary
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|entry| entry["key"]["value"] == "one")
            .unwrap()["value"]["decimal"] = json!("2");
        assert!(compare_data_semantics(&wrong, &wrong).is_err());
    }
    pub fn assert_data_contract_provenance() {
        let reference = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE).unwrap();
        assert_eq!(reference.as_array().unwrap().len(), 19);
        let mut changed = strict_json(REFERENCE_PROVENANCE.as_bytes()).unwrap();
        changed["origin"]["source_tree"] = json!("0000000000000000000000000000000000000000");
        assert!(reference_parts(REFERENCE_SEMANTIC, &changed.to_string()).is_err());
        assert!(
            reference_parts(&(REFERENCE_SEMANTIC.to_owned() + " "), REFERENCE_PROVENANCE).is_err()
        );
    }
    pub fn assert_contract_fault_protocol() {
        let valid = json!({"schema":"careerbrew-data-fault-v1","control":"empty","trial":"first","case":"2","role":"writer","pid":7,"driver_sha256":shared::digest(DRIVER.as_bytes()),"stage":"api-complete","direct_calls_observed":0,"hook":null,"write_bytes":null});
        assert!(
            validate_fault_intent(&valid, ContractFault::Empty, "2", "writer", "first", 7).is_ok()
        );
        let mut accepted = Vec::new();
        for (key, value) in [
            ("control", json!("stdout")),
            ("pid", json!(8)),
            ("driver_sha256", json!("0".repeat(64))),
            ("stage", json!("invented")),
            ("case", json!("3")),
            ("role", json!("reader")),
            ("unknown", json!(true)),
        ] {
            let mut changed = valid.clone();
            changed[key] = value;
            accepted.push(
                validate_fault_intent(&changed, ContractFault::Empty, "2", "writer", "first", 7)
                    .is_ok(),
            );
        }
        assert_eq!(accepted, vec![false; 7]);
    }
    pub fn assert_contract_named_deferral() {
        let typed = ReturnDifference {
            purpose: "primary".into(),
            dimension: ReturnDimension::Typed,
        };
        assert!(may_defer(
            Some(ContractFault::SameWrong),
            "3",
            "reader",
            &typed
        ));
        for (fault, case, role) in [
            (None, "3", "reader"),
            (Some(ContractFault::JsonOne), "3", "reader"),
            (Some(ContractFault::SameWrong), "4", "reader"),
            (Some(ContractFault::SameWrong), "3", "writer"),
        ] {
            assert!(!may_defer(fault, case, role, &typed));
        }
    }
    pub fn assert_contract_completion_shape() {
        let golden = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE).unwrap();
        let mut trials = Vec::new();
        for trial in 0..2 {
            let phases=golden.as_array().unwrap().iter().enumerate().map(|(index,row)|json!({"case":row["case"],"role":row["role"],"report":{"api_calls":row["calls"]},"deferred_differences":[],"owner_observation":{"pid":trial*19+index+1,"wait_code":0,"wait_signal":null,"wait_observed":true,"reaped":true,"cleanup_succeeded":true}})).collect::<Vec<_>>();
            trials.push(json!({"phases":phases}));
        }
        assert!(completed_trial_shape(&trials).is_ok());
        let mut incomplete = trials.clone();
        incomplete[1]["phases"].as_array_mut().unwrap().pop();
        assert!(completed_trial_shape(&incomplete).is_err());
        let mut reused = trials.clone();
        reused[1]["phases"][0]["owner_observation"]["pid"] = json!(1);
        assert!(completed_trial_shape(&reused).is_err());
        let mut missing = trials.clone();
        missing[1]["phases"][0]["report"]["api_calls"] = json!([]);
        assert!(completed_trial_shape(&missing).is_err());
        let mut bad = trials;
        bad[1]["phases"][0]["deferred_differences"] =
            json!([{"purpose":"primary","dimension":"source"}]);
        assert!(completed_trial_shape(&bad).is_err());
    }
    pub fn assert_contract_sidecar_reads() {
        let scratch = tempfile::tempdir().unwrap();
        let owned = scratch.path().join("owned");
        std::fs::create_dir_all(owned.join("protocol")).unwrap();
        let valid = json!({"schema":"careerbrew-data-fault-v1","control":"empty","trial":"first","case":"2","role":"writer","pid":7,"driver_sha256":shared::digest(DRIVER.as_bytes()),"stage":"api-complete","direct_calls_observed":0,"hook":null,"write_bytes":null});
        let path = owned.join(fault_relative("2", "writer"));
        std::fs::write(&path, valid.to_string()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let owner = json!({"observation":{"pid":7,"wait_observed":true,"reaped":true,"cleanup_succeeded":true}});
        assert!(read_fault(
            &owned,
            &owner,
            ContractFault::Empty,
            ("2", "writer", "first")
        )
        .is_ok());
        assert_eq!(
            read_fault(
                Path::new("/synthetic/not-read"),
                &json!({"observation":null}),
                ContractFault::Empty,
                ("2", "writer", "first")
            )
            .unwrap_err(),
            "fault owner unconfirmed"
        );
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_fault(
            &owned,
            &owner,
            ContractFault::Empty,
            ("2", "writer", "first")
        )
        .is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(
            &path,
            strict_json(FIXTURE.as_bytes()).unwrap()["canary"]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            read_fault(
                &owned,
                &owner,
                ContractFault::Empty,
                ("2", "writer", "first")
            )
            .unwrap_err(),
            "fault protocol privacy refusal"
        );
        std::fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(scratch.path().join("unread-target"), &path).unwrap();
        assert!(read_fault(
            &owned,
            &owner,
            ContractFault::Empty,
            ("2", "writer", "first")
        )
        .is_err());
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        let spec = &fixture["cases"][1]["phases"][0];
        let regular = effect_spec(spec, None, "2", "writer");
        assert_eq!(regular, *spec);
        let fault = effect_spec(spec, Some(ContractFault::Empty), "2", "writer");
        let extra = fault["allowed_instrumentation_delta"]
            .as_array()
            .unwrap()
            .last()
            .unwrap();
        assert_eq!(extra["entry"]["path"], "protocol/02-writer-fault.json");
        assert_eq!(extra["entry"]["mode"], "0600");
    }
    fn aggregate_fixture(bytes: usize) -> Vec<u8> {
        let semantic = reference_parts(REFERENCE_SEMANTIC, REFERENCE_PROVENANCE).unwrap();
        let mut value = json!({"schema":"careerbrew-data-candidate-v1","capture_executable_sha256":"0".repeat(64),"qualification":"reference-review-pending","inputs":{"unresolved":[""]},"semantic_projection":semantic,"trials":[{"trial":"first","phases":semantic},{"trial":"second","phases":semantic}],"raw_outputs":"synthetic-parser-only","scope":"synthetic JSON parser envelope only; no process or qualification claim"});
        let base = serde_json::to_vec(&value).unwrap().len();
        value["inputs"]["unresolved"][0] = json!("p".repeat(bytes.checked_sub(base).unwrap()));
        let wire = serde_json::to_vec(&value).unwrap();
        assert_eq!(wire.len(), bytes);
        wire
    }
    pub fn assert_aggregate_observed_size() {
        let raw = aggregate_fixture(4_493_719);
        assert!(strict_aggregate_json(&raw).is_ok());
        assert_eq!(
            strict_json(&raw).unwrap_err(),
            "data protocol exceeds limit"
        );
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("aggregate.json");
        std::fs::write(&path, &raw).unwrap();
        assert!(aggregate_candidate(&path).is_ok());
    }
    pub fn assert_aggregate_exact_bound() {
        assert!(strict_aggregate_json(&aggregate_fixture(AGGREGATE_LIMIT)).is_ok());
    }
    pub fn assert_aggregate_oversize() {
        let raw = aggregate_fixture(AGGREGATE_LIMIT + 1);
        assert_eq!(
            strict_aggregate_json(&raw).unwrap_err(),
            "data aggregate exceeds limit"
        );
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("aggregate.json");
        std::fs::write(&path, raw).unwrap();
        assert_eq!(
            aggregate_candidate(&path).unwrap_err(),
            "data aggregate exceeds limit"
        );
    }
    pub fn assert_aggregate_phase_bound() {
        assert!(strict_json(&aggregate_fixture(LIMIT)).is_ok());
        let raw = aggregate_fixture(LIMIT + 1);
        assert_eq!(
            strict_json(&raw).unwrap_err(),
            "data protocol exceeds limit"
        );
        let scratch = tempfile::tempdir().unwrap();
        let path = scratch.path().join("phase.json");
        std::fs::write(&path, aggregate_fixture(AGGREGATE_LIMIT + 1)).unwrap();
        assert_eq!(protocol_bytes(&path).unwrap().len(), LIMIT + 1);
    }
    pub fn assert_aggregate_duplicates() {
        for raw in [
            br#"{"x":1,"x":2}"#.as_slice(),
            br#"{"x":1,"\u0078":2}"#.as_slice(),
        ] {
            assert_eq!(strict_json(raw).unwrap_err(), "duplicate data JSON member");
            assert_eq!(
                strict_aggregate_json(raw).unwrap_err(),
                "duplicate data JSON member"
            );
        }
        for raw in [b"{".as_slice(), b"\xff".as_slice()] {
            assert_eq!(strict_json(raw).unwrap_err(), "data JSON invalid");
            assert_eq!(strict_aggregate_json(raw).unwrap_err(), "data JSON invalid");
        }
    }
    pub fn assert_aggregate_depth() {
        for depth in [8, 64, 65, 129] {
            let raw = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
            for parsed in [
                strict_json(raw.as_bytes()),
                strict_aggregate_json(raw.as_bytes()),
            ] {
                if depth <= 64 {
                    assert!(parsed.is_ok());
                } else {
                    assert_eq!(
                        parsed.unwrap_err(),
                        if depth == 65 {
                            "data JSON depth exceeds limit"
                        } else {
                            "data JSON invalid"
                        }
                    );
                }
            }
        }
    }
    pub fn assert_diagnostic_retention() {
        let scratch = tempfile::tempdir().unwrap();
        let output = scratch.path().join("retained");
        let execution = serde_json::json!({"pid":12,"wait_exit":0,"validation_category":"driver_identity_mismatch"});
        let paths = std::collections::BTreeSet::from(["/synthetic/driver.py".to_owned()]);
        let safe = r#"{"path":"/synthetic/driver.py","schema":"safe"}"#;
        retain_diagnostic(
            &output,
            &execution,
            &[("api.json", safe)],
            &paths,
            "SYNTHETIC-CANARY",
        )
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(output.join("api.json")).unwrap(),
            safe
        );
        assert!(std::fs::read_to_string(output.join("outcome.json"))
            .unwrap()
            .contains("driver_identity_mismatch"));
        assert!(retain_diagnostic(
            &output,
            &execution,
            &[("api.json", safe)],
            &paths,
            "SYNTHETIC-CANARY"
        )
        .is_err());
        let privacy = scratch.path().join("privacy");
        assert_eq!(
            retain_diagnostic(
                &privacy,
                &execution,
                &[("api.json", r#"{"value":"SYNTHETIC-CANARY"}"#)],
                &paths,
                "SYNTHETIC-CANARY"
            )
            .unwrap_err(),
            "diagnostic privacy refusal"
        );
        assert!(!privacy.join("api.json").exists());
        for item in std::fs::read_dir(&privacy).unwrap() {
            assert!(!std::fs::read_to_string(item.unwrap().path())
                .unwrap()
                .contains("SYNTHETIC-CANARY"));
        }
        let unknown = scratch.path().join("unknown");
        retain_diagnostic(&unknown,&serde_json::json!({"pid":null,"wait_exit":null,"validation_category":"shared_owner_failure"}),&[],&paths,"SYNTHETIC-CANARY").unwrap();
        let observed: serde_json::Value =
            serde_json::from_slice(&std::fs::read(unknown.join("outcome.json")).unwrap()).unwrap();
        assert!(observed["execution"]["wait_exit"].is_null());
        assert!(observed["reaped"].is_null());
        assert!(observed["cleanup_succeeded"].is_null());
        let outside = scratch.path().join("outside");
        assert!(retain_diagnostic(
            &outside,
            &execution,
            &[("api.json", r#"{"path":"/untrusted/outside"}"#)],
            &paths,
            "SYNTHETIC-CANARY"
        )
        .is_err());
        assert!(!outside.join("api.json").exists());
    }
    pub fn assert_diagnostic_metadata() {
        let refusal = diagnostic_refusal(
            &Err("unexpected retained change".to_owned()),
            "SYNTHETIC-CANARY",
        );
        assert_eq!(refusal["category"], "diagnostic_other_refusal");
        assert_eq!(refusal["message"], "unexpected retained change");
        let redacted = diagnostic_refusal(&Err("SYNTHETIC-CANARY".to_owned()), "SYNTHETIC-CANARY");
        assert!(redacted["message"].is_null());
        assert!(!redacted.to_string().contains("SYNTHETIC-CANARY"));
        assert_eq!(
            diagnostic_refusal(
                &Err("diagnostic unexpected identity acceptance".to_owned()),
                "SYNTHETIC-CANARY"
            )["category"],
            "unexpected_identity_acceptance"
        );
    }
    fn valid_driver_report(root: &Path) -> serde_json::Value {
        let path = root.join("xtask/tests/fixtures/capture/data_driver.py");
        let sample = serde_json::json!({"stage":"main-entry","globals_file":path,"globals_file_sha256":shared::digest(DRIVER.as_bytes()),"globals_name":"__main__","globals_spec_name":null,"globals_spec_origin":null,"main_code_filename":path,"main_code_sha256":shared::digest(DRIVER.as_bytes()),"sys_modules_main_present":true,"sys_modules_main_file":path,"sys_modules_main_file_sha256":shared::digest(DRIVER.as_bytes()),"sys_modules_main_name":"__main__","sys_modules_main_spec_name":null,"sys_modules_main_spec_origin":null});
        let mut complete = sample.clone();
        complete["stage"] = serde_json::json!("api-complete");
        serde_json::json!({"driver_provenance":{"schema":"careerbrew-data-driver-provenance-v1","pid":7,"entry":sample,"api_complete":complete}})
    }
    pub fn assert_current_driver_provenance() {
        let root = Path::new("/synthetic/source");
        let report = valid_driver_report(root);
        verify_driver_provenance(&report, 7, root).unwrap();
        assert_eq!(
            verify_driver_provenance(&serde_json::json!({}), 7, root).unwrap_err(),
            "driver_identity_missing"
        );
        for stage in ["entry", "api_complete"] {
            for field in [
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
            ] {
                let mut wrong = report.clone();
                wrong["driver_provenance"][stage][field] = serde_json::json!("wrong");
                assert_eq!(
                    verify_driver_provenance(&wrong, 7, root).unwrap_err(),
                    "driver_identity_mismatch",
                    "{stage}.{field}"
                );
            }
        }
        // The observed exit-file removal cannot weaken mandatory entry provenance.
        let exit = serde_json::json!({"external_modules":{},"__main__":{"__file__":null}});
        assert!(exit["__main__"]["__file__"].is_null());
        verify_driver_provenance(&report, 7, root).unwrap();
    }
    pub fn assert_explicit_runtime_without_history() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let module = root.join("runtime.py");
        let site = root.join("site.pth");
        std::fs::write(&module, b"synthetic runtime").unwrap();
        std::fs::write(&site, b"synthetic site").unwrap();
        let observation = serde_json::json!({"external_modules":{"synthetic":{"path":module,"sha256":shared::digest(b"synthetic runtime")}},"site_inputs":{site.to_str().unwrap():shared::digest(b"synthetic site")}});
        let allowed = std::collections::BTreeSet::from([
            module.to_string_lossy().into_owned(),
            site.to_string_lossy().into_owned(),
        ]);
        assert!(!root.join(".metrics").exists());
        runtime_files(root, &observation, &allowed).unwrap();
    }
    pub fn assert_refusal_retention() {
        let scratch = tempfile::tempdir().unwrap();
        let root = Path::new("/synthetic/source");
        let driver = root.join("xtask/tests/fixtures/capture/data_driver.py");
        let provenance = valid_driver_report(root)["driver_provenance"].clone();
        let api=serde_json::json!({"schema":"careerbrew-data-runtime-bootstrap-v1","pid":7,"driver_provenance":provenance,"store_calls":[]}).to_string();
        let observer=serde_json::json!({"catalog_calls":0,"modules":{},"external_modules":{},"sys_path":[],"prefix":"/synthetic/env","base_prefix":"/synthetic/python","site_inputs":{},"distributions":[],"descendant":null,"dependency_scope":"synthetic","unresolved":[]}).to_string();
        let execution = serde_json::json!({"source_root":root,"observation":{"pid":7,"wait_code":0,"wait_observed":true,"reaped":true,"cleanup_succeeded":true},"stdout_bytes":16,"stdout_sha256":shared::digest(b"SYNTHETIC-CANARY")});
        let allowed = std::collections::BTreeSet::from([
            driver.to_string_lossy().into_owned(),
            "/synthetic/env".to_owned(),
            "/synthetic/python".to_owned(),
        ]);
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let receipts = super::evidence_output(project, "data-bootstrap-repair-structural-v1");
        std::fs::create_dir_all(&receipts).unwrap();
        for name in [
            "safe",
            "absent",
            "malformed",
            "unknown",
            "canary",
            "unconfirmed",
            "symlink",
            "oversized",
            "unreadable",
        ] {
            let owned = scratch.path().join(name).join("owned");
            let output = scratch.path().join(name).join("evidence");
            std::fs::create_dir_all(owned.join("protocol")).unwrap();
            std::fs::create_dir(&output).unwrap();
            if name != "absent" {
                let raw = match name {
                    "malformed" => "not JSON".to_owned(),
                    "unknown" => api.replace(
                        "/synthetic/source/xtask/tests/fixtures/capture/data_driver.py",
                        "/unknown/never-read.py",
                    ),
                    "canary" => "SYNTHETIC-CANARY".to_owned(),
                    _ => api.clone(),
                };
                std::fs::write(owned.join("protocol/bootstrap-api.json"), raw).unwrap();
                std::fs::write(owned.join("protocol/bootstrap-observer.json"), &observer).unwrap();
                if name == "symlink" {
                    std::fs::remove_file(owned.join("protocol/bootstrap-api.json")).unwrap();
                    std::os::unix::fs::symlink(
                        scratch.path().join("unread-target"),
                        owned.join("protocol/bootstrap-api.json"),
                    )
                    .unwrap();
                }
                if name == "oversized" {
                    std::fs::write(
                        owned.join("protocol/bootstrap-api.json"),
                        vec![b'x'; LIMIT + 1],
                    )
                    .unwrap();
                }
                if name == "unreadable" {
                    use std::os::unix::fs::PermissionsExt;
                    std::fs::set_permissions(
                        owned.join("protocol/bootstrap-api.json"),
                        std::fs::Permissions::from_mode(0o0),
                    )
                    .unwrap();
                }
            }
            retain_bootstrap_protocols(
                &output,
                &owned,
                &execution,
                &allowed,
                "SYNTHETIC-CANARY",
                "bootstrap_privacy_refused",
                name != "unconfirmed",
            )
            .unwrap();
            let inventory: serde_json::Value = serde_json::from_slice(
                &std::fs::read(output.join("protocol-inventory.json")).unwrap(),
            )
            .unwrap();
            let unreadable = name == "unreadable"
                && std::fs::File::open(owned.join("protocol/bootstrap-api.json")).is_err();
            let expected = match name {
                "safe" => "safe",
                "absent" => "absent",
                "malformed" => "malformed-json",
                "unknown" => "unknown-path",
                "canary" => "privacy-refused",
                "symlink" => "nonregular",
                "oversized" => "oversized",
                "unreadable" if unreadable => "read-unavailable",
                "unreadable" => "safe",
                _ => "owner-terminal-unconfirmed",
            };
            assert_eq!(inventory["expected"][0]["state"], expected);
            assert_eq!(
                output.join("retained/api.json").exists(),
                expected == "safe"
            );
            if matches!(
                expected,
                "read-unavailable" | "nonregular" | "absent" | "owner-terminal-unconfirmed"
            ) {
                assert!(inventory["expected"][0]["sha256"].is_null());
            }
            std::fs::copy(
                output.join("protocol-inventory.json"),
                receipts.join(format!("{name}.json")),
            )
            .unwrap();
            assert_eq!(
                output.join("retained/observer.json").exists(),
                !matches!(name, "absent" | "unconfirmed")
            );
            for entry in std::fs::read_dir(output.join("retained")).unwrap() {
                assert!(!std::fs::read_to_string(entry.unwrap().path())
                    .unwrap()
                    .contains("SYNTHETIC-CANARY"));
            }
        }
    }
    pub fn assert_active_site_policy() {
        let scratch = tempfile::tempdir().unwrap();
        let venv = scratch.path().join("venv");
        let base = scratch.path().join("python");
        for prefix in [&venv, &base] {
            let dir = prefix.join("lib/python3.12/site-packages/pip-fixture.dist-info");
            std::fs::create_dir_all(&dir).unwrap();
            for name in ["METADATA", "RECORD"] {
                std::fs::write(dir.join(name), b"synthetic metadata").unwrap();
            }
        }
        let mut rows = Vec::new();
        for (policy, base_count) in [("false", 0), ("true", 2)] {
            let prefixes = active_site_prefixes(
                &venv,
                &base,
                format!("include-system-site-packages = {policy}\n").as_bytes(),
            )
            .unwrap();
            let metadata = prefixes
                .iter()
                .flat_map(|prefix| {
                    [
                        prefix.join("lib/python3.12/site-packages/pip-fixture.dist-info/METADATA"),
                        prefix.join("lib/python3.12/site-packages/pip-fixture.dist-info/RECORD"),
                    ]
                })
                .collect::<Vec<_>>();
            assert_eq!(
                metadata.iter().filter(|p| p.starts_with(&base)).count(),
                base_count
            );
            rows.push(serde_json::json!({"policy":policy,"base_metadata_count":base_count,"venv_metadata_count":2,"scope":"synthetic static site-policy fixture, not a provider execution"}));
        }
        assert!(
            active_site_prefixes(&venv, &base, b"include-system-site-packages = unknown\n")
                .is_err()
        );
        assert!(active_site_prefixes(&venv, &base, b"").is_err());
        let project = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let output = super::evidence_output(project, "data-bootstrap-site-policy-structural-v1");
        std::fs::create_dir_all(&output).unwrap();
        std::fs::write(
            output.join("results.json"),
            serde_json::to_vec_pretty(&rows).unwrap(),
        )
        .unwrap();
    }
    pub fn assert_interpreter_alias_binding() {
        let scratch = tempfile::tempdir().unwrap();
        let base = scratch.path().join("python");
        let alias = scratch.path().join("python-alias");
        std::fs::create_dir_all(base.join("bin")).unwrap();
        std::fs::create_dir_all(base.join("lib/python3.12/lib-dynload")).unwrap();
        std::fs::write(base.join("bin/python3.12"), b"synthetic interpreter").unwrap();
        std::os::unix::fs::symlink(&base, &alias).unwrap();
        let search = alias.join("lib/python3.12/lib-dynload");
        let binary = base.join("bin/python3.12").canonicalize().unwrap();
        let frozen = serde_json::json!({"python":{"binary":binary},"runtime_static":{"allowed_files":[base.join("bin/python3.12")],"interpreter_search_paths":[search]}});
        let paths =
            bootstrap_record_paths(scratch.path(), &scratch.path().join("owned"), &frozen).unwrap();
        assert!(
            paths.contains(search.to_str().unwrap()),
            "pinned logical interpreter path is missing"
        );
        let config = format!("home = {}\n", alias.join("bin").display());
        let derived =
            configured_interpreter_paths(config.as_bytes(), &binary, &["python3.12".to_owned()])
                .unwrap();
        assert!(derived.contains(search.to_str().unwrap()));
        let wrong = scratch.path().join("other/bin");
        std::fs::create_dir_all(&wrong).unwrap();
        assert!(configured_interpreter_paths(
            format!("home = {}\n", wrong.display()).as_bytes(),
            &binary,
            &["python3.12".to_owned()]
        )
        .is_err());
    }
    pub fn assert_wire_guard() {
        assert!(strict_json(br#"{"kind":"null","kind":"bool"}"#).is_err());
        assert!(node(
            &serde_json::json!({"kind":"int","decimal":"1208925819614629174706179"}),
            0
        )
        .is_ok());
        assert!(node(&serde_json::json!({"kind":"int","decimal":"-0"}), 0).is_err());
        assert!(node(&serde_json::json!({"kind":"bool","value":1}), 0).is_err());
    }
    pub fn assert_preparation_primary() {
        let primary = Err("unexpected retained change".to_owned());
        let seen = std::cell::RefCell::new(None);
        let outcome: Result<Value, String> =
            control_proof(&primary, Err("control observer missing".into()), |value| {
                *seen.borrow_mut() = Some(value.clone());
                Ok(())
            });
        assert_eq!(outcome.unwrap_err(), "unexpected retained change");
        assert_eq!(
            seen.borrow().as_ref().unwrap()["secondary_sha256"],
            shared::digest(b"control observer missing")
        );
    }
    pub fn assert_preparation_secondary() {
        let accepted = Ok(Value::Null);
        let outcome: Result<Value, String> = control_proof(
            &accepted,
            Err("control observer missing".into()),
            |_| Ok(()),
        );
        assert_eq!(outcome.unwrap_err(), "control observer missing");
        let primary = Err("phase_deadline".to_owned());
        let outcome: Result<Value, String> =
            control_proof(&primary, Err("control proof write failed".into()), |_| {
                Err("receipt write unavailable".into())
            });
        assert_eq!(outcome.unwrap_err(), "phase_deadline");
        let seen = std::cell::RefCell::new(None);
        let outcome: Result<Value, String> =
            control_proof(&primary, Err("SYNTHETIC-CANARY".into()), |value| {
                *seen.borrow_mut() = Some(value.to_string());
                Ok(())
            });
        assert_eq!(outcome.unwrap_err(), "phase_deadline");
        assert!(!seen.borrow().as_ref().unwrap().contains("SYNTHETIC-CANARY"));
    }
    pub fn assert_preparation_schedule() {
        for fault in [
            ContractFault::JsonOne,
            ContractFault::JsonBool,
            ContractFault::JsonLarge,
            ContractFault::JsonZero,
            ContractFault::YamlDateMap,
            ContractFault::YamlMapDate,
        ] {
            let steps = mutation_schedule(fault);
            assert_eq!(steps.len(), 3);
            assert_eq!(steps[0], (1, 0, None, "preparation"));
            let case = if matches!(
                fault,
                ContractFault::YamlDateMap | ContractFault::YamlMapDate
            ) {
                3
            } else {
                2
            };
            assert_eq!(steps[1], (case, 0, Some(fault), "writer"));
            assert_eq!(steps[2], (case, 1, None, "reader"));
        }
    }
    pub fn assert_preparation_imports() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let script = r#"import ast,sys
m=ast.parse(open(sys.argv[1]).read())
assert any(isinstance(n,ast.Import) and any(a.name=='copy' for a in n.names) for n in m.body), 'common copy import missing'
assert any(isinstance(n,ast.ImportFrom) and n.module=='careerbrew' and any(a.name=='backup' for a in n.names) for n in m.body), 'common backup import missing'
f=next(n for n in m.body if isinstance(n,ast.FunctionDef) and n.name=='controlled_invoke')
assert not any(isinstance(n,ast.Import) and any(a.name=='copy' for a in n.names) for n in ast.walk(f))
profile=next(n for n in m.body if isinstance(n,ast.If) and any(isinstance(x,ast.Call) and isinstance(x.func,ast.Attribute) and x.func.attr=='setprofile' for x in ast.walk(n)))
site=next(n for n in m.body if isinstance(n,ast.Import) and any(a.name=='site' for a in n.names))
backup=next(n for n in m.body if isinstance(n,ast.ImportFrom) and n.module=='careerbrew' and any(a.name=='backup' for a in n.names))
main=next(n for n in m.body if isinstance(n,ast.FunctionDef) and n.name=='main')
assert profile.lineno < site.lineno < backup.lineno < main.lineno
bootstrap=next(n for n in m.body if isinstance(n,ast.FunctionDef) and n.name=='bootstrap_main')
store_calls=[n for n in ast.walk(bootstrap) if isinstance(n,ast.Call) and isinstance(n.func,ast.Attribute) and isinstance(n.func.value,ast.Name) and n.func.value.id=='store']
assert len(store_calls)==1 and store_calls[0].func.attr=='digest'
"#;
        let status = std::process::Command::new("python3")
            .arg("-c")
            .arg(script)
            .arg(root.join("xtask/tests/fixtures/capture/data_driver.py"))
            .status()
            .unwrap();
        assert!(status.success());
    }

    pub fn assert_preparation_processes() {
        let rows: Vec<Value> = (1..=3).map(|pid| json!({"execution":{"observation":{"pid":pid,"wait_observed":true,"reaped":true,"cleanup_succeeded":true}},"os_reap_absence_verified":true})).collect();
        mutation_process_proof(&rows.iter().collect::<Vec<_>>()).unwrap();
        for index in [1, 2] {
            let mut bad = rows.clone();
            bad[index]["execution"]["observation"]["pid"] = json!(1);
            assert!(mutation_process_proof(&bad.iter().collect::<Vec<_>>()).is_err());
        }
        for flag in ["wait_observed", "reaped", "cleanup_succeeded"] {
            let mut bad = rows.clone();
            bad[0]["execution"]["observation"][flag] = json!(false);
            assert!(mutation_process_proof(&bad.iter().collect::<Vec<_>>()).is_err());
        }
        let mut bad = rows.clone();
        bad[0]["os_reap_absence_verified"] = json!(false);
        assert!(mutation_process_proof(&bad.iter().collect::<Vec<_>>()).is_err());
        assert!(mutation_process_proof(&rows[..2].iter().collect::<Vec<_>>()).is_err());
    }
    pub fn assert_preparation_boundaries() {
        let fixture = strict_json(FIXTURE.as_bytes()).unwrap();
        assert_eq!(
            fixture["cases"][1]["phases"][0]["allowed_retained_delta"][0]["entry"]["path"],
            "keys/backup.key"
        );
        assert_eq!(
            fixture["cases"][1]["phases"][0]["allowed_retained_delta"][0]["entry"]["mode"],
            "0600"
        );
        let owned = tempfile::tempdir().unwrap();
        for case in [2, 3] {
            let phase = &fixture["cases"][case]["phases"][0];
            assert_eq!(phase["allowed_retained_delta"].as_array().unwrap().len(), 1);
            assert_eq!(
                verify_delta(
                    owned.path(),
                    &BTreeMap::new(),
                    &BTreeMap::from([("keys/backup.key".into(), "file:600:synthetic".into())]),
                    phase
                )
                .unwrap_err(),
                "unexpected retained change"
            );
        }
        let root = Path::new("/synthetic/source");
        let declared: Vec<String> = ["agent_workflows.py", "backup.py", "events.py", "plugins.py"]
            .iter()
            .map(|name| {
                root.join("src/careerbrew")
                    .join(name)
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        let mut observed = json!({"catalog_calls":0,"modules":{},"external_modules":{},"sys_path":[],"prefix":"/synthetic/env","base_prefix":"/synthetic/python","site_inputs":{},"distributions":[],"descendant":null,"dependency_scope":"synthetic","unresolved":[]});
        for path in declared {
            observed["modules"].as_object_mut().unwrap().insert(
                path.clone(),
                json!({"path":path,"loader_origin":path,"sha256":"a".repeat(64)}),
            );
        }
        for name in ["copy", "dataclasses", "tarfile"] {
            observed["external_modules"][name] =
                json!({"path":format!("/synthetic/python/{name}.py"),"sha256":"b".repeat(64)});
        }
        let allowed = diagnostic_allowed_paths(root, owned.path(), &observed).unwrap();
        diagnostic_paths(&observed, &allowed).unwrap();
        assert!(diagnostic_paths(&json!({"path":"/unknown/never-read.py"}), &allowed).is_err());
        std::fs::create_dir(owned.path().join("protocol")).unwrap();
        let output = owned.path().join("evidence");
        std::fs::create_dir(&output).unwrap();
        let path = owned.path().join("protocol/observer.json");
        std::fs::write(&path, observed.to_string()).unwrap();
        retain_protocols(
            &output,
            owned.path(),
            &json!({"scope":"synthetic structural record; no process executed"}),
            &allowed,
            "SYNTHETIC-CANARY",
            ("structural-declared-closure", true),
            (
                &[("observer.json", "protocol/observer.json")],
                phase_protocol_shape,
            ),
        )
        .unwrap();
        assert!(output.join("retained/observer.json").exists());
        assert_refusal_retention();
    }
    pub fn assert_source_copy_metadata_order() {
        let expected = json!({"src/careerbrew/store.py":"file:644:abc"});
        let actual = json!({"src/careerbrew/store.py":"file:664:abc"});
        let output = std::cell::RefCell::new(None);
        let result = source_copy_comparison(&expected, &actual, |value| {
            *output.borrow_mut() = Some(value.clone());
            Ok(())
        });
        assert_eq!(
            result.unwrap_err(),
            "copied pre-mutation source does not equal frozen original"
        );
        assert!(
            output.borrow().is_some(),
            "comparison evidence erased by equality refusal"
        );
    }
    pub fn assert_source_copy_metadata_primary() {
        let expected = json!({"src/careerbrew/store.py":"file:644:abc"});
        let actual = json!({"src/careerbrew/store.py":"file:664:abc"});
        let called = std::cell::Cell::new(false);
        let result = source_copy_comparison(&expected, &actual, |_| {
            called.set(true);
            Err("comparison receipt missing".into())
        });
        assert_eq!(
            result.unwrap_err(),
            "copied pre-mutation source does not equal frozen original"
        );
        assert!(called.get(), "secondary write not attempted before primary");
        assert_eq!(
            source_copy_comparison(&expected, &expected, |_| Err(
                "comparison receipt missing".into()
            ))
            .unwrap_err(),
            "comparison receipt missing"
        );
        let actual = json!({"bindings":actual,"unknown":[],"forbidden":[]});
        let result = persist_source_copy(
            &expected,
            &actual,
            |_| Err("actual write failed".into()),
            |_| Err("comparison write failed".into()),
        );
        assert_eq!(
            result.unwrap_err(),
            "copied pre-mutation source does not equal frozen original"
        );
        let equal = json!({"bindings":expected,"unknown":[],"forbidden":[]});
        assert_eq!(
            persist_source_copy(
                &expected,
                &equal,
                |_| Err("actual write failed".into()),
                |_| Ok(())
            )
            .unwrap_err(),
            "actual write failed"
        );
    }

    pub fn assert_source_copy_metadata_fields() {
        let expected = json!({"src/careerbrew/store.py":"file:644:abc"});
        let before = expected.to_string();
        source_copy_comparison(&expected, &expected, |v| {
            assert_eq!(v["differences"], json!([]));
            Ok(())
        })
        .unwrap();
        for (actual, field) in [
            (json!({"src/careerbrew/store.py":"file:664:abc"}), "mode"),
            (json!({"src/careerbrew/store.py":"file:644:def"}), "sha256"),
            (json!({}), "missing"),
            (
                json!({"src/careerbrew/store.py":"file:644:abc","unknown":"x"}),
                "extra",
            ),
        ] {
            let seen = std::cell::RefCell::new(None);
            assert!(source_copy_comparison(&expected, &actual, |v| {
                *seen.borrow_mut() = Some(v.clone());
                Ok(())
            })
            .is_err());
            assert_eq!(
                seen.borrow().as_ref().unwrap()["differences"][0]["field"],
                field
            );
        }
        assert_eq!(expected.to_string(), before);
    }
    pub fn assert_source_copy_metadata_snapshot() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        std::fs::create_dir_all(root.join("src/careerbrew")).unwrap();
        let file = root.join("src/careerbrew/store.py");
        std::fs::write(&file, b"synthetic").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).unwrap();
        let expected =
            json!({"src/careerbrew/store.py":format!("file:644:{}",shared::digest(b"synthetic"))});
        let fact = copy_file_fact(root, "src/careerbrew/store.py").unwrap();
        assert_eq!(fact["mode"], "0644");
        assert_eq!(fact["bytes"], 9);
        assert_eq!(fact["sha256"], shared::digest(b"synthetic"));
        std::os::unix::fs::symlink(
            "/unknown/never-read",
            root.join("src/careerbrew/unknown.py"),
        )
        .unwrap();
        let snapshot = copy_source_snapshot(root, &expected).unwrap();
        assert_eq!(snapshot["unknown"][0]["category"], "symlink");
        assert!(!snapshot.to_string().contains("unknown.py"));
        assert_eq!(snapshot["bindings"], expected);
        std::fs::remove_file(root.join("src/careerbrew/unknown.py")).unwrap();
        std::fs::write(&file, vec![b'x'; LIMIT + 1]).unwrap();
        assert_eq!(
            copy_file_fact(root, "src/careerbrew/store.py").unwrap_err(),
            "source file exceeds limit"
        );
        std::fs::remove_file(&file).unwrap();
        std::os::unix::fs::symlink("/unknown/never-read", &file).unwrap();
        assert_eq!(
            copy_file_fact(root, "src/careerbrew/store.py").unwrap_err(),
            "source symlink refused"
        );
        std::fs::remove_file(&file).unwrap();
        std::fs::create_dir(&file).unwrap();
        assert!(copy_file_fact(root, "src/careerbrew/store.py").is_err());
        std::fs::remove_dir_all(root.join("src")).unwrap();
        std::os::unix::fs::symlink("/unknown/never-read", root.join("src")).unwrap();
        assert_eq!(
            copy_file_fact(root, "src/careerbrew/store.py").unwrap_err(),
            "source symlink refused"
        );
    }
    pub fn assert_source_copy_metadata_tools() {
        use std::ffi::OsString;
        let plan = source_copy_tools(
            Path::new("/source"),
            Path::new("/owned"),
            Path::new("/owned/checkout"),
            Path::new("/owned/source.tar"),
            &"a".repeat(40),
        );
        let expected: Vec<Vec<OsString>> = vec![
            vec!["write-tree".into()],
            vec![
                "ls-tree".into(),
                "-rz".into(),
                "--full-tree".into(),
                "a".repeat(40).into(),
            ],
            vec![
                "archive".into(),
                "--format=tar".into(),
                "--output=/owned/source.tar".into(),
                "a".repeat(40).into(),
            ],
            vec![
                "-xf".into(),
                "/owned/source.tar".into(),
                "-C".into(),
                "/owned/checkout".into(),
            ],
        ];
        for (index, tool) in plan.iter().enumerate() {
            assert_eq!(tool.args, expected[index]);
            assert_eq!(
                tool.program,
                if index == 3 {
                    "/usr/bin/tar"
                } else {
                    "/usr/bin/git"
                }
            );
            assert_eq!(
                tool.cwd,
                if index == 3 {
                    Path::new("/owned")
                } else {
                    Path::new("/source")
                }
            );
        }
        let called = std::cell::Cell::new(false);
        assert!(inspect_copy_after_owner(false, || {
            called.set(true);
            Ok(Value::Null)
        })
        .is_err());
        assert!(!called.get());
        let source = include_str!("../src/capture_data.rs");
        for function in ["fn source_control(", "pub fn source_copy_metadata_probe("] {
            let body = source
                .split(function)
                .nth(1)
                .unwrap()
                .split("\n}\n")
                .next()
                .unwrap();
            assert!(body.contains("prepare_source_copy("));
            if function.starts_with("pub") {
                assert!(!body.contains("runtime_bootstrap("));
                assert!(!body.contains("Command::"));
                assert!(!body.contains("phase("));
            }
        }
        assert_eq!(
            setup_safe_text("unknown-filename.py", "SYNTHETIC-CANARY", false),
            None
        );
        assert_eq!(
            setup_safe_text("safe", "SYNTHETIC-CANARY", true),
            Some("safe")
        );
        assert_eq!(
            setup_safe_text("SYNTHETIC-CANARY", "SYNTHETIC-CANARY", true),
            None
        );
    }
    pub fn assert_source_copy_metadata_index() {
        let expected = json!({"src/careerbrew/store.py":"file:644:abc"});
        let row = format!("100644 blob {}\tsrc/careerbrew/store.py\0", "a".repeat(40));
        let index = source_copy_index(&row, &expected).unwrap();
        assert_eq!(index["known"]["src/careerbrew/store.py"]["mode"], "100644");
        assert!(source_copy_index("", &expected).is_err());
        assert!(source_copy_index(
            &(row
                + &format!(
                    "100644 blob {}\tsrc/careerbrew/unfrozen.py\0",
                    "b".repeat(40)
                )),
            &expected
        )
        .is_err());
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path();
        std::fs::create_dir_all(root.join("src/careerbrew")).unwrap();
        std::fs::create_dir(root.join(".venv")).unwrap();
        let snapshot = copy_source_snapshot(root, &json!({})).unwrap();
        assert_eq!(snapshot["forbidden"][0]["name"], ".venv");
    }
    fn mode_fixture() -> (tempfile::TempDir, Value, Value) {
        let root = tempfile::tempdir().unwrap();
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::create_dir_all(root.path().join("src/careerbrew")).unwrap();
        std::fs::write(root.path().join("src/careerbrew/store.py"), b"synthetic").unwrap();
        std::fs::set_permissions(
            root.path().join("src/careerbrew/store.py"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let expected =
            json!({"src/careerbrew/store.py":format!("file:644:{}",shared::digest(b"synthetic"))});
        let original = copy_source_snapshot(root.path(), &expected).unwrap();
        (root, expected, original)
    }
    pub fn assert_source_construction_modes() {
        let (root, expected, original) = mode_fixture();
        let path = root.path().join("src/careerbrew/store.py");
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let raw = copy_source_snapshot(root.path(), &expected).unwrap();
        assert!(source_copy_comparison(&expected, &raw["bindings"], |_| Ok(())).is_err());
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(
            restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(()))
                .unwrap_err(),
            "source mode target root not private directory"
        );
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let restored =
            restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(())).unwrap();
        source_copy_comparison(&expected, &restored["bindings"], |_| Ok(())).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic");
    }
    pub fn assert_source_construction_runtime() {
        let runtime = json!({"static_inputs":{"runtime_static":{"allowed_files":["/frozen/runtime.py"],"site_inputs":{},"interpreter_search_paths":[],"scope":"synthetic frozen inventory"}}});
        assert_eq!(
            trusted_source_runtime_inventory(&runtime).unwrap(),
            BTreeSet::from(["/frozen/runtime.py".to_owned()])
        );
        for field in [
            "missing",
            "empty",
            "alias",
            "relative",
            "duplicate",
            "unknown",
            "nonarray",
            "nonstring",
        ] {
            let mut bad = runtime.clone();
            let inventory = &mut bad["static_inputs"]["runtime_static"];
            match field {
                "missing" => {
                    inventory.as_object_mut().unwrap().remove("allowed_files");
                }
                "empty" => inventory["allowed_files"] = json!([]),
                "alias" => {
                    inventory["files"] = inventory["allowed_files"].clone();
                    inventory.as_object_mut().unwrap().remove("allowed_files");
                }
                "relative" => inventory["allowed_files"] = json!(["relative.py"]),
                "duplicate" => {
                    inventory["allowed_files"] = json!(["/frozen/runtime.py", "/frozen/runtime.py"])
                }
                "unknown" => inventory["new_inventory"] = json!([]),
                "nonarray" => inventory["allowed_files"] = json!(false),
                _ => inventory["allowed_files"] = json!([1]),
            }
            assert!(trusted_source_runtime_inventory(&bad).is_err(), "{field}");
        }
        for (field, value) in [
            ("site_inputs", json!(false)),
            ("interpreter_search_paths", json!(null)),
            ("scope", json!(1)),
        ] {
            let mut bad = runtime.clone();
            bad["static_inputs"]["runtime_static"][field] = value;
            assert!(trusted_source_runtime_inventory(&bad).is_err());
        }
    }

    pub fn assert_source_construction_unsafe() {
        for variant in [
            "tampered",
            "missing",
            "unknown",
            "symlink",
            "parent",
            "directory",
            "oversized",
            "hardlink",
        ] {
            let (root, mut expected, _) = mode_fixture();
            let a = root.path().join("src/careerbrew/a.py");
            std::fs::write(&a, b"peer").unwrap();
            std::fs::set_permissions(&a, std::fs::Permissions::from_mode(0o644)).unwrap();
            expected["src/careerbrew/a.py"] =
                json!(format!("file:644:{}", shared::digest(b"peer")));
            let original = copy_source_snapshot(root.path(), &expected).unwrap();
            let file = root.path().join("src/careerbrew/store.py");
            for path in [&a, &file] {
                std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
            }
            match variant {
                "tampered" => std::fs::write(&file, b"changed").unwrap(),
                "missing" => std::fs::remove_file(&file).unwrap(),
                "unknown" => {
                    std::fs::write(root.path().join("src/careerbrew/unknown.py"), b"unknown")
                        .unwrap()
                }
                "symlink" => {
                    std::fs::remove_file(&file).unwrap();
                    std::os::unix::fs::symlink("/unknown/never-read", &file).unwrap();
                }
                "parent" => {
                    std::fs::rename(
                        root.path().join("src/careerbrew"),
                        root.path().join("saved"),
                    )
                    .unwrap();
                    std::os::unix::fs::symlink(
                        root.path().join("saved"),
                        root.path().join("src/careerbrew"),
                    )
                    .unwrap();
                }
                "directory" => {
                    std::fs::remove_file(&file).unwrap();
                    std::fs::create_dir(&file).unwrap();
                }
                "oversized" => std::fs::write(&file, vec![b'x'; LIMIT + 1]).unwrap(),
                _ => {
                    std::fs::hard_link(&file, root.path().join("alias")).unwrap();
                }
            }
            let raw = copy_source_snapshot(root.path(), &expected).unwrap();
            assert!(
                restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(()))
                    .is_err(),
                "{variant}"
            );
            let peer = if variant == "parent" {
                root.path().join("saved/a.py")
            } else {
                a
            };
            assert_eq!(
                std::fs::metadata(peer).unwrap().permissions().mode() & 0o777,
                0o600,
                "{variant}"
            );
        }
    }
    pub fn assert_source_construction_special() {
        for variant in [
            "invalid-mode",
            "binding-mode",
            "original-special",
            "copied-special",
        ] {
            let (root, expected, mut original) = mode_fixture();
            let file = root.path().join("src/careerbrew/store.py");
            std::fs::set_permissions(
                &file,
                std::fs::Permissions::from_mode(if variant == "copied-special" {
                    0o4600
                } else {
                    0o600
                }),
            )
            .unwrap();
            match variant {
                "invalid-mode" => {
                    original["entries"]["src/careerbrew/store.py"]["mode"] = json!("4644")
                }
                "binding-mode" => {
                    original["entries"]["src/careerbrew/store.py"]["mode"] = json!("0600")
                }
                "original-special" => {
                    original["entries"]["src/careerbrew/store.py"]["special_bits"] = json!(0o4000)
                }
                _ => {}
            }
            let before = std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777;
            let raw = copy_source_snapshot(root.path(), &expected).unwrap();
            assert!(
                restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(()))
                    .is_err(),
                "{variant}"
            );
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o7777,
                before
            );
        }
        let (root, mut expected, _) = mode_fixture();
        let file = root.path().join("src/careerbrew/store.py");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o755)).unwrap();
        expected["src/careerbrew/store.py"] =
            json!(format!("file:755:{}", shared::digest(b"synthetic")));
        let original = copy_source_snapshot(root.path(), &expected).unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let raw = copy_source_snapshot(root.path(), &expected).unwrap();
        restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(())).unwrap();
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
    pub fn assert_source_construction_receipts() {
        for stage in ["raw", "actions", "restored"] {
            let (root, expected, original) = mode_fixture();
            let file = root.path().join("src/careerbrew/store.py");
            std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
            let raw = copy_source_snapshot(root.path(), &expected).unwrap();
            assert!(
                restore_source_modes(root.path(), &expected, &original, &raw, |name, _| {
                    if name == stage {
                        Err("required mode evidence unavailable".into())
                    } else {
                        Ok(())
                    }
                })
                .is_err()
            );
            if stage == "raw" {
                assert_eq!(
                    std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                    0o600
                );
            }
        }
        let (root, expected, original) = mode_fixture();
        std::fs::write(root.path().join("src/careerbrew/store.py"), b"tampered").unwrap();
        let raw = copy_source_snapshot(root.path(), &expected).unwrap();
        assert_eq!(
            restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Err(
                "secondary failure".into()
            ))
            .unwrap_err(),
            "source copied bytes or frozen mode binding differs"
        );
        let original_root = tempfile::tempdir().unwrap();
        let twin = original_root.path().join("original.py");
        std::fs::write(&twin, b"original").unwrap();
        std::fs::set_permissions(&twin, std::fs::Permissions::from_mode(0o644)).unwrap();
        let called = std::cell::Cell::new(false);
        assert!(inspect_copy_after_owner(false, || {
            called.set(true);
            Ok(Value::Null)
        })
        .is_err());
        assert!(!called.get());
        assert_eq!(std::fs::read(&twin).unwrap(), b"original");
        assert_eq!(
            std::fs::metadata(&twin).unwrap().permissions().mode() & 0o777,
            0o644
        );
        let (original_root, expected, original) = mode_fixture();
        let (copied, _, _) = mode_fixture();
        let source = original_root.path().join("src/careerbrew/store.py");
        let copy = copied.path().join("src/careerbrew/store.py");
        let before = std::fs::metadata(&source).unwrap().modified().unwrap();
        std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o600)).unwrap();
        let raw = copy_source_snapshot(copied.path(), &expected).unwrap();
        restore_source_modes(copied.path(), &expected, &original, &raw, |_, _| Ok(())).unwrap();
        assert_eq!(
            std::fs::metadata(&copy).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            std::fs::metadata(&source).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            std::fs::metadata(&source).unwrap().modified().unwrap(),
            before
        );
        assert_eq!(std::fs::read(&source).unwrap(), b"synthetic");
    }
    pub fn assert_source_construction_guards() {
        assert_source_copy_metadata_tools();
        let source = include_str!("../src/capture_data.rs");
        let body = source.split("fn source_control(").nth(1).unwrap();
        assert!(body.contains("imported module hash differs"));
        assert!(body.contains("imported Careerbrew module is outside checkout"));
        assert!(source.contains("if shared::sources(copied)? != *expected"));
        let (root, expected, original) = mode_fixture();
        let file = root.path().join("src/careerbrew/store.py");
        std::fs::write(&file, b"changed").unwrap();
        let raw = copy_source_snapshot(root.path(), &expected).unwrap();
        assert!(
            restore_source_modes(root.path(), &expected, &original, &raw, |_, _| Ok(())).is_err()
        );
        assert_eq!(
            source_copy_probe_outcome(
                Err("copied pre-mutation source does not equal frozen original".into()),
                Ok(())
            )
            .unwrap_err(),
            "copied pre-mutation source does not equal frozen original"
        );
        assert!(
            source_copy_probe_outcome(Ok(()), Err("missing mandatory probe receipt".into()))
                .is_err()
        );
    }
    fn observer_scope_context<'a>(
        root: &'a Path,
        source: Option<&'a Path>,
        observer: Option<&'a Path>,
        empty: &'a Value,
        env: &'a BTreeMap<String, String>,
    ) -> TrialContext<'a> {
        TrialContext {
            root,
            python: Path::new("/unused/python"),
            owned: root,
            environment: env,
            fixture: empty,
            name: "first",
            sources: empty,
            runtime: empty,
            raw_dir: root,
            evidence: None,
            driver: None,
            omit_driver_provenance: false,
            fault: None,
            observer_root: observer,
            source_import: source,
            diagnostic_extra: None,
        }
    }
    pub fn assert_observer_scope_ordinary() {
        let root = tempfile::tempdir().unwrap();
        let empty = Value::Null;
        let env = BTreeMap::new();
        let context = observer_scope_context(root.path(), None, None, &empty, &env);
        assert_eq!(
            phase_pythonpath(&context),
            root.path()
                .join("xtask/tests/fixtures/capture/observer")
                .to_string_lossy()
        );
        assert_eq!(context.root, root.path());
        assert!(context.driver.is_none());
    }
    pub fn assert_observer_scope_wrong_root() {
        let trusted = tempfile::tempdir().unwrap();
        let copied = tempfile::tempdir().unwrap();
        let source = copied.path().join("src");
        let empty = Value::Null;
        let env = BTreeMap::new();
        let context = observer_scope_context(
            trusted.path(),
            Some(&source),
            Some(trusted.path()),
            &empty,
            &env,
        );
        let paths: Vec<_> = std::env::split_paths(&phase_pythonpath(&context)).collect();
        assert_eq!(
            paths,
            vec![
                source.clone(),
                trusted.path().join("xtask/tests/fixtures/capture/observer")
            ]
        );
        assert_eq!(context.root, trusted.path());
    }
    fn observer_scope_files(root: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
        let path = root.join("xtask/tests/fixtures/capture/observer/sitecustomize.py");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"synthetic observer bytes").unwrap();
        let site = root.join("synthetic.METADATA");
        std::fs::write(&site, b"synthetic installed metadata").unwrap();
        (path, site)
    }
    fn observer_scope_record(path: &Path, site: &Path) -> Value {
        json!({"external_modules":{"sitecustomize":{"path":path,"sha256":shared::digest(&std::fs::read(path).unwrap())}},"site_inputs":{site.to_str().unwrap():shared::digest(&std::fs::read(site).unwrap())}})
    }
    pub fn assert_observer_scope_changed_root() {
        let trusted = tempfile::tempdir().unwrap();
        let copied = tempfile::tempdir().unwrap();
        let (observer, site) = observer_scope_files(trusted.path());
        observer_scope_files(copied.path());
        let source = copied.path().join("src");
        std::fs::create_dir(&source).unwrap();
        let empty = Value::Null;
        let env = BTreeMap::new();
        let context = observer_scope_context(
            copied.path(),
            Some(&source),
            Some(trusted.path()),
            &empty,
            &env,
        );
        let paths: Vec<_> = std::env::split_paths(&phase_pythonpath(&context)).collect();
        assert_eq!(paths[0], source);
        let allowed = BTreeSet::from([
            observer.to_str().unwrap().to_owned(),
            site.to_str().unwrap().to_owned(),
        ]);
        let before = allowed.clone();
        runtime_files(
            trusted.path(),
            &observer_scope_record(&paths[1].join("sitecustomize.py"), &site),
            &allowed,
        )
        .unwrap();
        assert_eq!(paths[1].join("sitecustomize.py"), observer);
        assert_eq!(allowed, before);
        assert_eq!(context.root, copied.path());
    }
    pub fn assert_observer_scope_refusals() {
        let trusted = tempfile::tempdir().unwrap();
        let copied = tempfile::tempdir().unwrap();
        let (observer, site) = observer_scope_files(trusted.path());
        let (copied_observer, _) = observer_scope_files(copied.path());
        let allowed = BTreeSet::from([
            observer.to_str().unwrap().to_owned(),
            site.to_str().unwrap().to_owned(),
        ]);
        let before = allowed.clone();
        let empty = Value::Null;
        let env = BTreeMap::new();
        let source = copied.path().join("src");
        let context = observer_scope_context(
            copied.path(),
            Some(&source),
            Some(copied.path()),
            &empty,
            &env,
        );
        let selected: Vec<_> = std::env::split_paths(&phase_pythonpath(&context)).collect();
        assert_eq!(selected[1].join("sitecustomize.py"), copied_observer);

        assert_eq!(
            runtime_files(
                trusted.path(),
                &observer_scope_record(&copied_observer, &site),
                &allowed
            )
            .unwrap_err(),
            "runtime module path not allowed"
        );
        let mut unknown = observer_scope_record(&observer, &site);
        unknown["external_modules"]["sitecustomize"]["path"] = json!("/unknown/never-read.py");
        assert_eq!(
            runtime_files(trusted.path(), &unknown, &allowed).unwrap_err(),
            "runtime module path not allowed"
        );
        let mut wrong_hash = observer_scope_record(&observer, &site);
        wrong_hash["external_modules"]["sitecustomize"]["sha256"] = json!("0".repeat(64));
        assert_eq!(
            runtime_files(trusted.path(), &wrong_hash, &allowed).unwrap_err(),
            "runtime module changed"
        );
        assert_eq!(allowed, before);
    }
}
#[test]
fn preflight_pins_registered_data_inputs_without_store_calls() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let frozen = capture_data::preflight(root, &root.join(".venv/bin/python")).unwrap();
    std::fs::write(
        root.join(".metrics/data-preflight.json"),
        serde_json::to_vec_pretty(&frozen).unwrap(),
    )
    .unwrap();
    capture_data::assert_wire_guard();
}

fn assert_command_metadata(
    phase: &serde_json::Value,
    root: &Path,
    trial: &str,
    case: &str,
    role: &str,
) {
    use std::os::unix::ffi::OsStrExt;
    let expected = |value: &std::ffi::OsStr| serde_json::json!({"Unix":value.as_bytes()});
    assert_eq!(
        phase["program"],
        expected(root.join(".venv/bin/python").as_os_str())
    );
    assert_eq!(phase["cwd"], root.to_str().unwrap());
    let driver = root.join("xtask/tests/fixtures/capture/data_driver.py");
    let arguments = [
        std::ffi::OsStr::new("-B"),
        std::ffi::OsStr::new("-S"),
        driver.as_os_str(),
        std::ffi::OsStr::new(trial),
        std::ffi::OsStr::new(case),
        std::ffi::OsStr::new(role),
    ];
    assert_eq!(phase["args"], serde_json::json!(arguments.map(expected)));
}

#[test]
fn command_metadata_preserves_os_bytes() {
    let root = Path::new("/synthetic/source");
    let mut command = Command::new(root.join(".venv/bin/python"));
    command
        .args(["-B", "-S"])
        .arg(root.join("xtask/tests/fixtures/capture/data_driver.py"))
        .args(["first", "1", "reader"])
        .current_dir(root);
    let actual = serde_json::json!({"program":command.get_program(),"args":command.get_args().collect::<Vec<_>>(),"cwd":command.get_current_dir()});
    assert_command_metadata(&actual, root, "first", "1", "reader");
}
#[test]
fn driver_provenance_structural_contract() {
    capture_data::assert_current_driver_provenance();
}

fn candidate_output_attempt(
    root: &Path,
    operation: impl FnOnce(&Path) -> Result<(), String>,
) -> (std::path::PathBuf, Result<(), String>) {
    let directory = evidence_output(root, "data-corpus-candidate-v1");
    std::fs::create_dir(&directory).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = directory.join("candidate.json");
    let outcome = operation(&output);
    (output, outcome)
}

#[test]
fn actual_ten_case_data_candidate_uses_fresh_writer_reader_processes() {
    fn archive(from: &Path, to: &Path) {
        std::fs::create_dir(to).unwrap();
        for item in std::fs::read_dir(from).unwrap() {
            let item = item.unwrap();
            let source = item.path();
            let target = to.join(item.file_name());
            if source.is_dir() {
                archive(&source, &target);
            } else {
                std::fs::copy(source, target).unwrap();
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let (_, outcome) = candidate_output_attempt(root, |output| {
        capture_data::run(root, &root.join(".venv/bin/python"), output)?;
        let receipt: serde_json::Value =
            serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
        assert_eq!(receipt["qualification"], "reference-review-pending");
        let trials = receipt["trials"].as_array().unwrap();
        assert_eq!(trials.len(), 2);
        assert_ne!(trials[0]["owned_root"], trials[1]["owned_root"]);
        for trial in trials {
            assert!(Path::new(trial["owned_root"].as_str().unwrap()).is_absolute());
            let phases = trial["phases"].as_array().unwrap();
            assert_eq!(phases.len(), 19);
            assert_eq!(
                phases
                    .iter()
                    .map(|p| p["report"]["api_calls"].as_array().unwrap().len())
                    .sum::<usize>(),
                25
            );
            for phase in phases {
                assert_command_metadata(
                    phase,
                    root,
                    trial["trial"].as_str().unwrap(),
                    phase["case"].as_str().unwrap(),
                    phase["role"].as_str().unwrap(),
                );
                assert_eq!(phase["exit"], 0);
                assert!(phase["pid"].as_u64().unwrap() > 0);
            }
        }
        let preserved = root.join(".metrics/data-first-reference.json");
        if !preserved.exists() {
            let raw = root.join(".metrics/data-first-reference.raw");
            archive(&output.with_extension("raw"), &raw);
            std::fs::copy(output, &preserved).unwrap();
            std::fs::write(root.join(".metrics/data-first-reference-archive.json"),serde_json::to_vec_pretty(&serde_json::json!({"original_raw_outputs":receipt["raw_outputs"],"archived_raw_outputs":raw,"receipt_sha256":capture_actions::digest(&std::fs::read(output).unwrap()),"scope":"unqualified first actual corpus candidate; no planted controls qualified"})).unwrap()).unwrap();
        }
        Ok(())
    });
    outcome.unwrap();
}

#[test]
fn existing_cli_capture_still_runs_with_the_shared_owner() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    capture_actions::run(
        root,
        &root.join(".venv/bin/python"),
        &scratch.path().join("actions.json"),
    )
    .unwrap();
}

#[test]
fn diagnostic_retention_structural_controls() {
    capture_data::assert_diagnostic_retention();
    capture_data::assert_diagnostic_metadata();
}

#[test]
fn driver_provenance_acceptance_and_refusals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    capture_data::driver_controls(
        root,
        &root.join(".venv/bin/python"),
        &evidence_output(root, "data-driver-controls-v1"),
    )
    .unwrap();
}

#[test]
fn runtime_bootstrap_structural_contract() {
    capture_data::assert_explicit_runtime_without_history();
}

fn evidence_output(root: &Path, name: &str) -> std::path::PathBuf {
    let base = root.join(".metrics");
    std::fs::create_dir_all(&base).unwrap();
    let first = base.join(name);
    if !first.exists() {
        return first;
    }
    tempfile::Builder::new()
        .prefix(&format!("{name}-"))
        .tempdir_in(base)
        .unwrap()
        .keep()
        .join("evidence")
}
#[test]
fn runtime_bootstrap_contracts() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    capture_data::bootstrap_contracts(
        root,
        &root.join(".venv/bin/python"),
        &evidence_output(root, "data-runtime-bootstrap-controls-v1"),
    )
    .unwrap();
}
#[test]
fn fresh_runtime_bootstrap_has_no_history_dependency() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    capture_data::fresh_bootstrap(
        root,
        &root.join(".venv/bin/python"),
        &evidence_output(root, "data-runtime-fresh-v1"),
    )
    .unwrap();
}

#[test]
fn runtime_bootstrap_retention_and_site_policy_retention() {
    capture_data::assert_refusal_retention();
}
#[test]
fn runtime_bootstrap_retention_and_site_policy_sites() {
    capture_data::assert_active_site_policy();
}

#[test]
fn runtime_bootstrap_retention_and_site_policy_alias() {
    capture_data::assert_interpreter_alias_binding();
}

#[test]
fn phase_owner_structural_contract() {
    capture_data::assert_phase_owner_record();
    capture_data::assert_phase_owner_retention();
}

#[test]
fn phase_owner_acceptance_and_refusals() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    capture_data::phase_owner_controls(
        root,
        &root.join(".venv/bin/python"),
        &evidence_output(root, "data-phase-owner-controls-v1"),
    )
    .unwrap();
}

#[test]
fn phase_owner_review_structural_primary() {
    capture_data::assert_phase_primary_failure();
}
#[test]
fn phase_owner_review_structural_hash() {
    capture_data::assert_phase_external_hash();
}
#[test]
fn phase_owner_review_structural_cohort() {
    capture_data::assert_phase_cohort();
}

#[test]
fn phase_observer_qualification_structural() {
    capture_data::assert_mandatory_observer_qualification();
}

#[test]
fn candidate_failure_retention_structural_allocation() {
    let synthetic = tempfile::tempdir().unwrap();
    let (output, outcome) = candidate_output_attempt(synthetic.path(), |output| {
        std::fs::write(
            output.with_extension("safe-marker"),
            b"synthetic-safe-parent-evidence",
        )
        .unwrap();
        Err("synthetic_refusal".into())
    });
    assert_eq!(outcome.unwrap_err(), "synthetic_refusal");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(output.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    assert!(
        output.with_extension("safe-marker").is_file(),
        "safe failure evidence disappeared with caller TempDir"
    );
}

#[test]
fn candidate_failure_retention_structural_pre_provider() {
    capture_data::assert_candidate_pre_provider_failure();
}
#[test]
fn candidate_failure_retention_structural_unknown_owner() {
    capture_data::assert_phase_owner_retention();
}
#[test]
fn candidate_failure_retention_structural_privacy() {
    capture_data::assert_candidate_private_error();
    capture_data::assert_refusal_retention();
}
#[test]
fn candidate_failure_retention_structural_primary() {
    capture_data::assert_candidate_outcome_write_failure(false);
}
#[test]
fn candidate_failure_retention_structural_missing_outcome() {
    capture_data::assert_candidate_outcome_write_failure(true);
}

#[test]
fn data_contract_structural_calendar() {
    capture_data::assert_data_contract_calendar();
}
#[test]
fn data_contract_structural_priority() {
    capture_data::assert_data_contract_priority();
}
#[test]
fn data_contract_structural_golden() {
    capture_data::assert_data_contract_golden();
}

#[test]
fn data_contract_structural_provenance() {
    capture_data::assert_data_contract_provenance();
}

#[test]
fn data_contract_structural_fault_protocol() {
    capture_data::assert_contract_fault_protocol();
}
#[test]
fn data_contract_structural_named_deferral() {
    capture_data::assert_contract_named_deferral();
}

#[test]
fn data_contract_control_groups() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    capture_data::contract_control_groups(root,&root.join(".venv/bin/python"),&evidence_output(root,"data-contract-groups-v1"),||{
        original_cli_controls::captures_real_reference_schema_and_module_provenance();
        original_cli_controls::rejects_same_removed_action_in_both_real_children();
        original_cli_controls::rejects_same_argument_and_delivery_changes_in_real_children();
        original_cli_controls::rejects_malformed_action_and_argument_types_in_real_children();
        original_cli_controls::rejects_actual_stdout_canary();
        original_cli_controls::rejects_actual_stderr_canary();
        original_cli_controls::scans_actual_retained_canary_content_before_hashing();
        original_cli_controls::rejects_actual_extra_retained_file();
        original_cli_controls::rejects_actual_skipped_cli_entry();
        original_cli_controls::rejects_observed_nonzero_exit();
        original_cli_controls::cleans_up_owned_descendant_process_group();
        original_cli_controls::rejects_wrong_loader_root_and_modified_input_evidence();
        original_cli_controls::retains_leader_ownership_until_group_cleanup();
        original_cli_controls::distinguishes_owned_zombie_group_from_live_descendant();
        original_http_controls::captures_actual_twenty_one_case_handler_corpus();
        original_http_controls::rejects_real_catalog_removal_in_both_servers();
        original_http_controls::rejects_real_weakened_header();
        original_http_controls::rejects_real_authentication_bypass();
        original_http_controls::rejects_real_wrong_loader_root();
        original_http_controls::rejects_real_served_source_change();
        original_http_controls::rejects_real_stdout_canary();
        original_http_controls::rejects_real_stderr_canary();
        original_http_controls::rejects_real_retained_canary();
        original_http_controls::rejects_real_extra_write();
        original_http_controls::retains_real_skipped_and_nonzero_exit_results();
        original_http_controls::rejects_real_wire_defects_and_reaps_hanging_handler();
        original_http_controls::preserves_shared_cli_capture_contract();
        original_http_controls::publishes_complete_readiness_after_paused_real_write();
        original_http_controls::rejects_fully_published_malformed_readiness();
        original_owner_controls::invalid_budget_never_spawns();
        original_owner_controls::positive_budget_preserves_actual_exit();
        original_owner_controls::short_budget_kills_and_reaps_actual_child();
        Ok(serde_json::json!({"cli":14,"http":15,"owner":3,"scope":"unchanged shared original control bodies executed synchronously"}))
 }).unwrap();
}

#[test]
fn data_contract_structural_complete_two_trials() {
    capture_data::assert_contract_completion_shape();
}
#[test]
fn data_contract_structural_sidecar_read_guards() {
    capture_data::assert_contract_sidecar_reads();
}

#[test]
fn data_contract_aggregate_structural_observed() {
    capture_data::assert_aggregate_observed_size();
}
#[test]
fn data_contract_aggregate_structural_exact() {
    capture_data::assert_aggregate_exact_bound();
}
#[test]
fn data_contract_aggregate_structural_oversize() {
    capture_data::assert_aggregate_oversize();
}
#[test]
fn data_contract_aggregate_structural_phase() {
    capture_data::assert_aggregate_phase_bound();
}
#[test]
fn data_contract_aggregate_structural_duplicates() {
    capture_data::assert_aggregate_duplicates();
}
#[test]
fn data_contract_aggregate_structural_depth() {
    capture_data::assert_aggregate_depth();
}

#[test]
fn data_contract_preparation_structural_primary() {
    capture_data::assert_preparation_primary();
}
#[test]
fn data_contract_preparation_structural_secondary() {
    capture_data::assert_preparation_secondary();
}
#[test]
fn data_contract_preparation_structural_schedule() {
    capture_data::assert_preparation_schedule();
}
#[test]
fn data_contract_preparation_structural_imports() {
    capture_data::assert_preparation_imports();
}

#[test]
fn data_contract_preparation_structural_processes() {
    capture_data::assert_preparation_processes();
}
#[test]
fn data_contract_preparation_structural_boundaries() {
    capture_data::assert_preparation_boundaries();
}

#[test]
fn source_copy_metadata_structural_order() {
    capture_data::assert_source_copy_metadata_order();
}
#[test]
fn source_copy_metadata_structural_primary() {
    capture_data::assert_source_copy_metadata_primary();
}

#[test]
fn source_copy_metadata_structural_fields() {
    capture_data::assert_source_copy_metadata_fields();
}
#[test]
fn source_copy_metadata_structural_snapshot() {
    capture_data::assert_source_copy_metadata_snapshot();
}
#[test]
fn source_copy_metadata_structural_tools() {
    capture_data::assert_source_copy_metadata_tools();
}
#[test]
fn source_copy_metadata_structural_index() {
    capture_data::assert_source_copy_metadata_index();
}
#[test]
fn source_copy_metadata_probe_once() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .unwrap();
    let output = evidence_output(&root, "data-source-copy-metadata-v1");
    capture_data::source_copy_metadata_probe(&root, &output).unwrap();
}

#[test]
fn source_copy_construction_structural_modes() {
    capture_data::assert_source_construction_modes();
}
#[test]
fn source_copy_construction_structural_runtime() {
    capture_data::assert_source_construction_runtime();
}

#[test]
fn source_copy_construction_structural_unsafe() {
    capture_data::assert_source_construction_unsafe();
}
#[test]
fn source_copy_construction_structural_special() {
    capture_data::assert_source_construction_special();
}
#[test]
fn source_copy_construction_structural_receipts() {
    capture_data::assert_source_construction_receipts();
}
#[test]
fn source_copy_construction_structural_guards() {
    capture_data::assert_source_construction_guards();
}

#[test]
fn source_observer_scope_structural_ordinary() {
    capture_data::assert_observer_scope_ordinary();
}
#[test]
fn source_observer_scope_structural_wrong_root() {
    capture_data::assert_observer_scope_wrong_root();
}
#[test]
fn source_observer_scope_structural_changed_root() {
    capture_data::assert_observer_scope_changed_root();
}
#[test]
fn source_observer_scope_structural_refusals() {
    capture_data::assert_observer_scope_refusals();
}

#[test]
fn data_executable_fault_decoder_structural_roundtrip() {
    capture_data::assert_executable_fault_roundtrip();
}
#[test]
fn data_executable_fault_decoder_structural_known() {
    capture_data::assert_executable_fault_equivalence(false);
}
#[test]
fn data_executable_fault_decoder_structural_invalid() {
    capture_data::assert_executable_fault_equivalence(true);
}
