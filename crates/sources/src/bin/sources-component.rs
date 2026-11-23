//! Synthetic-only rust-08 component transport. No production route selects it.

use careerbrew_sources::{cli, synthetic};
use std::path::Path;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("sources-component: --all | --case <id> | connect ...");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--all"] || args.len() == 2 && args[0] == "--case" {
        let selected = if args[0] == "--case" {
            Some(args[1].as_str())
        } else {
            None
        };
        if selected.is_some_and(|case| !synthetic::CASES.contains(&case)) {
            return usage();
        }
        return match synthetic::run_all(selected) {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("sources-component: {}", error.class);
                ExitCode::FAILURE
            }
        };
    }
    if args.first().is_some_and(|arg| arg == "connect") {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/http");
        let (output, code) = cli::connect(&args[1..], &root, cli::ConnectControl::default());
        print!("{output}");
        return ExitCode::from(code.clamp(0, 255) as u8);
    }
    usage()
}
