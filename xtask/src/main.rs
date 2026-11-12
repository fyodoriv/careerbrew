mod baseline_verdict;
mod capture_actions;
#[cfg(not(test))]
mod capture_data;
mod capture_http;
mod inventory;
mod observer_probe;
mod toolchain_identity;
mod trial_plan;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args == ["migration", "self-test"] {
        let passed = baseline_verdict::controls::run() + trial_plan::controls::run();
        println!("migration self-test: {passed} controls passed");
        return ExitCode::SUCCESS;
    }
    if args == ["migration", "inventory", "--check"] {
        return match inventory::check() {
            Ok(count) => {
                println!("migration inventory: {count} assigned paths, 0 unassigned direct paths");
                ExitCode::SUCCESS
            }
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
