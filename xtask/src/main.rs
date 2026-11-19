mod baseline;
mod baseline_verdict;
mod capture_actions;
#[cfg(not(test))]
mod capture_data;
mod capture_http;
mod decide;
mod depvec;
mod inventory;
mod measure;
mod noise_floor;
mod normalize;
mod observer_probe;
mod resources;
mod sites;
mod toolchain_identity;
mod trial_plan;
mod vecdriver;
mod verify;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args == ["migration", "self-test"] {
        let passed = baseline_verdict::controls::run()
            + trial_plan::controls::run()
            + decide::controls::run()
            + depvec::controls::run()
            + vecdriver::controls::run();
        println!("migration self-test: {passed} controls passed");
        return ExitCode::SUCCESS;
    }
    if args == ["migration", "inventory", "--check"] {
        return match inventory::check().and_then(|count| Ok((count, sites::check()?))) {
            Ok((count, sites)) => {
                println!("migration inventory: {count} assigned paths, 0 unassigned direct paths");
                println!("migration inventory: {sites} assigned embedded sites and entry points, 0 unassigned");
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("migration inventory: {reason}");
                ExitCode::FAILURE
            }
        };
    }
    if args == ["migration", "inventory", "--sites-check"] {
        return match sites::check() {
            Ok(count) => {
                println!("migration inventory: {count} assigned embedded sites and entry points, 0 unassigned");
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("migration inventory: {reason}");
                ExitCode::FAILURE
            }
        };
    }
    if args == ["migration", "inventory", "--sites-list"] {
        return match sites::list() {
            Ok(_) => ExitCode::SUCCESS,
            Err(reason) => {
                eprintln!("migration inventory: {reason}");
                ExitCode::FAILURE
            }
        };
    }
    if args == ["migration", "inventory", "--embedded-check"] {
        return match inventory::check_embedded() {
            Ok(count) => {
                println!("migration inventory: {count} reviewed embedded anchors verified");
                ExitCode::SUCCESS
            }
            Err(reason) => {
                eprintln!("migration inventory: {reason}");
                ExitCode::FAILURE
            }
        };
    }
    if let [migration, action, flag, command, rest @ ..] = args.as_slice() {
        if migration == "migration" && action == "resources" && flag == "--cmd" {
            let program = std::path::Path::new(command);
            if !program.is_absolute() {
                eprintln!("migration resources: --cmd must be an absolute path");
                return ExitCode::from(2);
            }
            return match resources::measure(program, rest) {
                Ok(usage) => {
                    println!("{}", usage.to_json());
                    if usage.passed() {
                        ExitCode::SUCCESS
                    } else {
                        ExitCode::FAILURE
                    }
                }
                Err(reason) => {
                    // Exit 3 is a measurement error, never the child's exit code.
                    println!("{}", serde_json::json!({"error": reason, "pass": false}));
                    ExitCode::from(3)
                }
            };
        }
    }
    if args.len() >= 2 && args[0] == "migration" && args[1] == "baseline" {
        return baseline_command(&args[2..]);
    }
    if args.len() >= 2 && args[0] == "migration" && args[1] == "decide" {
        return decide_command(&args[2..]);
    }
    if let [migration, action, flag, file] = args.as_slice() {
        if migration == "migration" && action == "noise-floor" && flag == "--baseline" {
            return match noise_floor::measure(std::path::Path::new(file)) {
                Ok(record) => {
                    println!("{record}");
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("migration noise-floor: {reason}");
                    ExitCode::from(2)
                }
            };
        }
    }
    if args.len() >= 2 && args[0] == "migration" && args[1] == "measure" {
        return measure_command(&args[2..]);
    }
    if args.len() >= 2 && args[0] == "migration" && args[1] == "vector-check" {
        return vector_check_command(&args[2..]);
    }
    if args.len() >= 2 && args[0] == "migration" && args[1] == "verify" {
        return verify_command(&args[2..]);
    }
    if let [migration, action, flag, compiler] = args.as_slice() {
        if migration == "migration" && action == "toolchain-identity" && flag == "--rustc" {
            return match toolchain_identity::run(std::path::Path::new(compiler)) {
                Ok(identity) => {
                    println!("migration toolchain identity: {identity}");
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("migration toolchain identity: {reason}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    if let [migration, action, python_flag, python, output_flag, output] = args.as_slice() {
        if migration == "migration"
            && action == "capture-actions"
            && python_flag == "--python"
            && output_flag == "--out"
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap();
            return match capture_actions::run(
                root,
                std::path::Path::new(python),
                std::path::Path::new(output),
            ) {
                Ok(()) => {
                    println!("action contract: two fresh Python executions agree");
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("action contract: {reason}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    if let [migration, action, python_flag, python, output_flag, output] = args.as_slice() {
        if migration == "migration"
            && action == "capture-http"
            && python_flag == "--python"
            && output_flag == "--out"
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap();
            return match capture_http::run(
                root,
                std::path::Path::new(python),
                std::path::Path::new(output),
            ) {
                Ok(()) => {
                    println!(
                        "HTTP contract: two fresh handler observations match the frozen reference"
                    );
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("HTTP candidate: {reason}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    #[cfg(not(test))]
    if let [migration, action, python_flag, python, output_flag, output] = args.as_slice() {
        if migration == "migration"
            && action == "capture-data"
            && python_flag == "--python"
            && output_flag == "--out"
        {
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .unwrap();
            return match capture_data::run(
                root,
                std::path::Path::new(python),
                std::path::Path::new(output),
            ) {
                Ok(()) => {
                    println!("DATA contract: two bounded observations match the frozen reference");
                    ExitCode::SUCCESS
                }
                Err(reason) => {
                    eprintln!("DATA candidate: {reason}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    if let [host, endpoint, action, request] = args.as_slice() {
        if host == "--host"
            && endpoint.to_string_lossy().starts_with("unix:///")
            && action == "observer-fault-client"
        {
            return match observer_probe::fault_client(
                std::path::Path::new(request),
                &endpoint.to_string_lossy(),
            ) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => {
                    eprintln!("fault client: {error}");
                    ExitCode::FAILURE
                }
            };
        }
    }
    if args.len() >= 4
        && args[0] == "migration"
        && args[1] == "observer-probe"
        && args[2] == "--image"
    {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap();
        let parsed = match args.as_slice() {
            [_, _, _, image] => {
                let nonce = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos();
                Some((
                    image,
                    root.join(format!(".metrics/rust-linux-observer/run-{nonce}")),
                    false,
                    false,
                ))
            }
            [_, _, _, image, out_flag, out] if out_flag == "--out" => {
                Some((image, std::path::PathBuf::from(out), false, false))
            }
            [_, _, _, image, out_flag, out, mode]
                if out_flag == "--out"
                    && (mode == "--freeze-only" || mode == "--fault-controls-only") =>
            {
                Some((
                    image,
                    std::path::PathBuf::from(out),
                    mode == "--freeze-only",
                    mode == "--fault-controls-only",
                ))
            }
            _ => None,
        };
        if let Some((image, out, freeze, faults)) = parsed {
            return match observer_probe::run(root, &image.to_string_lossy(), &out, freeze, faults) {
                Ok(()) => {
                    println!("observer probe receipt: {}", out.display());
                    ExitCode::SUCCESS
                }
                Err(error) => {
                    eprintln!("observer probe: {error}; receipt: {}", out.display());
                    ExitCode::FAILURE
                }
            };
        }
    }
    eprintln!("usage: cargo xtask migration self-test");
    eprintln!("       cargo xtask migration inventory --check");
    eprintln!("       cargo xtask migration inventory --embedded-check");
    eprintln!("       cargo xtask migration inventory --sites-check");
    eprintln!("       cargo xtask migration inventory --sites-list");
    eprintln!("       cargo xtask migration baseline --step <id> --python <absolute-path> --out <new-file> [--runs <n>] [--test-allow-few-runs]");
    eprintln!("       cargo xtask migration measure --step <id> --python <absolute-path> --out <new-file> [--pairs <n>] [--test-allow-few-pairs] [--test-candidate <absolute-path>]");
    eprintln!("       cargo xtask migration decide --step <id> --baseline <file> --verify <file> --measure <file> --out <new-file> [--test-noise-floor <absolute-file>]");
    eprintln!("       cargo xtask migration noise-floor --baseline <absolute-file>");
    eprintln!("       cargo xtask migration vector-check --python <absolute-path> --generator <repo-path> --driver <repo-path> (--candidate <absolute-path> | --test-candidate-script <repo-path>) --out <new-file> [--test-plant <remove-case|change-byte|unicode-mismatch>]");
    eprintln!("       cargo xtask migration verify --step <id> --python <absolute-path> --out <new-file> [--test-candidate <absolute-path>] [--test-oracle <absolute-path>] [--test-wrong-vector <set>] [--test-plant-home-write <file-name>] [--test-scope <full|allow-python>]");
    eprintln!("       cargo xtask migration toolchain-identity --rustc <absolute-path>");
    eprintln!(
        "       cargo xtask migration capture-actions --python <absolute-path> --out <new-file>"
    );
    eprintln!(
        "       cargo xtask migration capture-http --python <absolute-path> --out <new-file>"
    );
    eprintln!(
        "       cargo xtask migration capture-data --python <absolute-path> --out <new-file>"
    );
    eprintln!("       cargo xtask migration observer-probe --image <immutable-id> [--out <new-directory> [--freeze-only|--fault-controls-only]]");
    ExitCode::from(2)
}

/// `migration baseline --step <id> --python <path> --out <new-file> [--runs <n>]`.
fn baseline_command(rest: &[std::ffi::OsString]) -> ExitCode {
    let (mut step, mut python, mut output, mut runs) = (None, None, None, None);
    // Test-only: allow fewer runs than the protocol minimum.
    let allow_few_runs = rest.iter().any(|arg| arg == "--test-allow-few-runs");
    let rest = rest
        .iter()
        .filter(|arg| *arg != "--test-allow-few-runs")
        .cloned()
        .collect::<Vec<_>>();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("migration baseline: every flag needs a value");
            return ExitCode::from(2);
        };
        let slot = match flag.to_str() {
            Some("--step") => &mut step,
            Some("--python") => &mut python,
            Some("--out") => &mut output,
            Some("--runs") => &mut runs,
            _ => {
                eprintln!("migration baseline: unknown flag");
                return ExitCode::from(2);
            }
        };
        if slot.replace(value.clone()).is_some() {
            eprintln!("migration baseline: a flag is repeated");
            return ExitCode::from(2);
        }
    }
    let (Some(step), Some(python), Some(output)) = (step, python, output) else {
        eprintln!("migration baseline: --step, --python and --out are required");
        return ExitCode::from(2);
    };
    let runs = match runs.map(|value| value.to_string_lossy().parse::<usize>()) {
        None => baseline::DEFAULT_RUNS,
        Some(Ok(runs)) => runs,
        Some(Err(_)) => {
            eprintln!("migration baseline: --runs must be a number");
            return ExitCode::from(2);
        }
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    match baseline::run(
        root,
        &step.to_string_lossy(),
        std::path::Path::new(&python),
        runs,
        allow_few_runs,
        std::path::Path::new(&output),
    ) {
        Ok(record) => {
            let summary = &record["summary"];
            println!(
                "migration baseline: step={} os={} runs={} cpu_us_p50={} cpu_us_p95={} tree_max_rss_bytes_p50={} tree_max_rss_bytes_p95={}",
                record["step"].as_str().unwrap_or(""),
                record["os"].as_str().unwrap_or(""),
                record["runs"],
                summary["cpu_us_p50"],
                summary["cpu_us_p95"],
                summary["tree_max_rss_bytes_p50"],
                summary["tree_max_rss_bytes_p95"],
            );
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("migration baseline: {reason}");
            ExitCode::FAILURE
        }
    }
}

/// `migration decide --step <id> --baseline <f> --verify <f> --measure <f> --out <new-file>`.
/// Exit 0: activate. Exit 1: keep-python (verdict written). Exit 2: usage or
/// refused input (nothing written).
fn decide_command(rest: &[std::ffi::OsString]) -> ExitCode {
    let mut slots: [Option<std::ffi::OsString>; 6] = Default::default();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("migration decide: every flag needs a value");
            return ExitCode::from(2);
        };
        let index = match flag.to_str() {
            Some("--step") => 0,
            Some("--baseline") => 1,
            Some("--verify") => 2,
            Some("--measure") => 3,
            Some("--out") => 4,
            // Test-only: a noise floor in place of the registry entry.
            Some("--test-noise-floor") => 5,
            _ => {
                eprintln!("migration decide: unknown flag");
                return ExitCode::from(2);
            }
        };
        if slots[index].replace(value.clone()).is_some() {
            eprintln!("migration decide: a flag is repeated");
            return ExitCode::from(2);
        }
    }
    let [Some(step), Some(base), Some(verify), Some(measure), Some(output), test_floor] = slots
    else {
        eprintln!(
            "migration decide: --step, --baseline, --verify, --measure and --out are required"
        );
        return ExitCode::from(2);
    };
    let path = std::path::Path::new;
    match decide::run(
        &step.to_string_lossy(),
        path(&base),
        path(&verify),
        path(&measure),
        test_floor.as_deref().map(std::path::Path::new),
        path(&output),
    ) {
        Ok(record) => {
            let decision = record["decision"].as_str().unwrap_or("");
            let reasons = record["reasons"]
                .as_array()
                .map(|reasons| {
                    reasons
                        .iter()
                        .filter_map(|reason| reason.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            println!(
                "migration decide: step={} decision={decision} reasons={reasons}",
                record["step"].as_str().unwrap_or("")
            );
            if decision == "activate" {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Err(reason) => {
            eprintln!("migration decide: {reason}");
            ExitCode::from(2)
        }
    }
}

/// `migration measure --step <id> --python <path> --out <new-file> [--pairs <n>]`.
fn measure_command(rest: &[std::ffi::OsString]) -> ExitCode {
    let (mut step, mut python, mut output, mut pairs, mut candidate) =
        (None, None, None, None, None);
    // Test-only: allow fewer pairs than the protocol minimum.
    let allow_few_pairs = rest.iter().any(|arg| arg == "--test-allow-few-pairs");
    let rest = rest
        .iter()
        .filter(|arg| *arg != "--test-allow-few-pairs")
        .cloned()
        .collect::<Vec<_>>();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("migration measure: every flag needs a value");
            return ExitCode::from(2);
        };
        let slot = match flag.to_str() {
            Some("--step") => &mut step,
            Some("--python") => &mut python,
            Some("--out") => &mut output,
            Some("--pairs") => &mut pairs,
            // Test-only: measure this executable instead of the registered candidate.
            Some("--test-candidate") => &mut candidate,
            Some("--runs") => {
                eprintln!("migration measure: --runs is replaced by --pairs");
                return ExitCode::from(2);
            }
            _ => {
                eprintln!("migration measure: unknown flag");
                return ExitCode::from(2);
            }
        };
        if slot.replace(value.clone()).is_some() {
            eprintln!("migration measure: a flag is repeated");
            return ExitCode::from(2);
        }
    }
    let (Some(step), Some(python), Some(output)) = (step, python, output) else {
        eprintln!("migration measure: --step, --python and --out are required");
        return ExitCode::from(2);
    };
    let pairs = match pairs.map(|value| value.to_string_lossy().parse::<usize>()) {
        None => measure::DEFAULT_PAIRS,
        Some(Ok(pairs)) => pairs,
        Some(Err(_)) => {
            eprintln!("migration measure: --pairs must be a number");
            return ExitCode::from(2);
        }
    };
    let source = match &candidate {
        Some(path) => measure::Source::TestOverride(std::path::Path::new(path)),
        None => measure::Source::Registered,
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    match measure::run(
        root,
        &step.to_string_lossy(),
        std::path::Path::new(&python),
        &source,
        pairs,
        allow_few_pairs,
        std::path::Path::new(&output),
    ) {
        Ok(record) => {
            let mut line = format!(
                "migration measure: step={} os={} pairs={}",
                record["step"].as_str().unwrap_or(""),
                record["os"].as_str().unwrap_or(""),
                record["pairs"],
            );
            for regime in record["regimes"].as_array().into_iter().flatten() {
                let state = regime["cache_state"].as_str().unwrap_or("");
                for side in ["oracle", "candidate"] {
                    let summary = &regime["summary"][side];
                    line.push_str(&format!(
                        " {state}_{side}_cpu_us_p95={} {state}_{side}_tree_max_rss_bytes_p95={}",
                        summary["cpu_us_p95"], summary["tree_max_rss_bytes_p95"],
                    ));
                }
            }
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(reason) => {
            eprintln!("migration measure: {reason}");
            ExitCode::FAILURE
        }
    }
}

/// `migration verify --step <id> --python <path> --out <new-file> [--test-candidate <path>]`.
/// Exit 0 when every case matches; 1 on a refusal or a failed case.
fn verify_command(rest: &[std::ffi::OsString]) -> ExitCode {
    let (mut step, mut python, mut output, mut candidate, mut oracle, mut wrong, mut plant) =
        (None, None, None, None, None, None, None);
    let mut scope = None;
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("migration verify: every flag needs a value");
            return ExitCode::from(2);
        };
        let slot = match flag.to_str() {
            Some("--step") => &mut step,
            Some("--python") => &mut python,
            Some("--out") => &mut output,
            // Test-only: point at a candidate binary before a port registers one.
            Some("--test-candidate") => &mut candidate,
            // Test-only: a script in place of the Python oracle.
            Some("--test-oracle") => &mut oracle,
            // Test-only: plant a wrong vector in this vector set.
            Some("--test-wrong-vector") => &mut wrong,
            // Test-only: write this file into CAREERBREW_HOME during verify
            // (a step that counts home writes must count it).
            Some("--test-plant-home-write") => &mut plant,
            // Test-only: `full` or `allow-python` (see `verify::Plants`).
            Some("--test-scope") => &mut scope,
            _ => {
                eprintln!("migration verify: unknown flag");
                return ExitCode::from(2);
            }
        };
        if slot.replace(value.clone()).is_some() {
            eprintln!("migration verify: a flag is repeated");
            return ExitCode::from(2);
        }
    }
    let (Some(step), Some(python), Some(output)) = (step, python, output) else {
        eprintln!("migration verify: --step, --python and --out are required");
        return ExitCode::from(2);
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    match verify::run(
        root,
        &step.to_string_lossy(),
        std::path::Path::new(&python),
        candidate.as_deref().map(std::path::Path::new),
        oracle.as_deref().map(std::path::Path::new),
        verify::Plants {
            wrong: wrong.as_deref().and_then(std::ffi::OsStr::to_str),
            home_write: plant.as_deref().and_then(std::ffi::OsStr::to_str),
            scope: scope.as_deref().and_then(std::ffi::OsStr::to_str),
        },
        std::path::Path::new(&output),
    ) {
        Ok(record) => {
            let failed = record["failed_cases"]
                .as_array()
                .map(|cases| {
                    cases
                        .iter()
                        .filter_map(|case| case.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            println!(
                "migration verify: step={} cases={} passed={} failed={}",
                record["step"].as_str().unwrap_or(""),
                record["case_count"],
                record["passed_count"],
                if failed.is_empty() { "none" } else { &failed },
            );
            if record["passed"] == true {
                ExitCode::SUCCESS
            } else {
                eprintln!("migration verify: case(s) differ from the oracle: {failed}");
                ExitCode::FAILURE
            }
        }
        Err(reason) => {
            eprintln!("migration verify: {reason}");
            ExitCode::FAILURE
        }
    }
}

/// `migration vector-check`: the vector-driver kind (`rust-04a`). One
/// generated vector file feeds the Python and the Rust driver. Exit 0 when
/// every case matches, 1 when a case fails, 2 on a refusal.
fn vector_check_command(rest: &[std::ffi::OsString]) -> ExitCode {
    let mut slots: [Option<std::ffi::OsString>; 7] = Default::default();
    for pair in rest.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("migration vector-check: every flag needs a value");
            return ExitCode::from(2);
        };
        let index = match flag.to_str() {
            Some("--python") => 0,
            Some("--generator") => 1,
            Some("--driver") => 2,
            Some("--candidate") => 3,
            // Test-only: run a Python script as the Rust side.
            Some("--test-candidate-script") => 4,
            Some("--out") => 5,
            // Test-only: plant a defect in the Rust answer.
            Some("--test-plant") => 6,
            _ => {
                eprintln!("migration vector-check: unknown flag");
                return ExitCode::from(2);
            }
        };
        if slots[index].replace(value.clone()).is_some() {
            eprintln!("migration vector-check: a flag is repeated");
            return ExitCode::from(2);
        }
    }
    let [Some(python), Some(generator), Some(driver), binary, script, Some(output), plant] = slots
    else {
        eprintln!("migration vector-check: --python, --generator, --driver and --out are required");
        return ExitCode::from(2);
    };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let script = script.map(|script| root.join(script));
    let candidate = match (&binary, &script) {
        (Some(binary), None) => vecdriver::Candidate::Binary(std::path::Path::new(binary)),
        (None, Some(script)) => vecdriver::Candidate::PythonScript(script),
        _ => {
            eprintln!("migration vector-check: give one of --candidate or --test-candidate-script");
            return ExitCode::from(2);
        }
    };
    let plant = match vecdriver::Plant::parse(plant.as_deref().and_then(std::ffi::OsStr::to_str)) {
        Ok(plant) => plant,
        Err(reason) => {
            eprintln!("migration vector-check: {reason}");
            return ExitCode::from(2);
        }
    };
    let output = std::path::PathBuf::from(output);
    let result = vecdriver::record(
        root,
        std::path::Path::new(&python),
        std::path::Path::new(&generator),
        std::path::Path::new(&driver),
        candidate,
        plant,
    )
    .and_then(|record| vecdriver::write_new(&output, &record).map(|()| record));
    match result {
        Ok(record) => {
            let failed = record["failed_cases"]
                .as_array()
                .map(|cases| {
                    cases
                        .iter()
                        .filter_map(|case| case.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
                .unwrap_or_default();
            println!(
                "migration vector-check: cases={} passed={} failed={}",
                record["case_count"],
                record["passed_count"],
                if failed.is_empty() { "none" } else { &failed },
            );
            if record["passed"] == true {
                ExitCode::SUCCESS
            } else {
                eprintln!("migration vector-check: case(s) differ: {failed}");
                ExitCode::FAILURE
            }
        }
        Err(reason) => {
            eprintln!("migration vector-check: {reason}");
            ExitCode::from(2)
        }
    }
}
