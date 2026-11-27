//! Synthetic-only rust-22 component transport. No production route selects it.

use careerbrew_exercises::synthetic;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("exercises-component: --all | --case <id>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let selected = match args.as_slice() {
        [all] if all == "--all" => None,
        [flag, case] if flag == "--case" && synthetic::CASES.contains(&case.as_str()) => {
            Some(case.as_str())
        }
        _ => return usage(),
    };
    let Ok(root) = std::env::current_dir() else {
        return usage();
    };
    match synthetic::run_all(&root, selected) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("exercises-component: {error}");
            ExitCode::FAILURE
        }
    }
}
