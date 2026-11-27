//! Synthetic-only rust-23 component transport. No production route selects it.

use careerbrew_voice::synthetic;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("voice-component: --all | --case <id>");
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args != ["--all"] && !(args.len() == 2 && args[0] == "--case") {
        return usage();
    }
    let selected = if args[0] == "--case" {
        Some(args[1].as_str())
    } else {
        None
    };
    if selected.is_some_and(|case| !synthetic::CASES.contains(&case)) {
        return usage();
    }
    match synthetic::run_all(selected) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("voice-component: {}", error.class);
            ExitCode::FAILURE
        }
    }
}
