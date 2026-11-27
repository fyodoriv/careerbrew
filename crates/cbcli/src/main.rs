//! `careerbrew-native`: see the crate docs in `lib.rs`.
use careerbrew_cli::{events, run, Env};
use std::io::{IsTerminal, Write};

fn main() {
    let started = std::time::Instant::now();
    let args: Vec<String> = match std::env::args_os()
        .skip(1)
        .map(|a| a.into_string())
        .collect()
    {
        Ok(args) => args,
        Err(_) => {
            eprintln!("native: unsupported argv");
            std::process::exit(careerbrew_cli::UNSUPPORTED);
        }
    };
    let get = |key: &str| std::env::var(key).ok();
    let env = Env {
        columns: get("COLUMNS"),
        stdout_is_terminal: std::io::stdout().is_terminal(),
        caps: events::Caps::from_env(get),
    };
    careerbrew_cli::routed::export_home(&args, &env.caps);
    let outcome = run(&args, &env);
    let mut stdout = std::io::stdout().lock();
    if stdout
        .write_all(outcome.stdout.as_bytes())
        .and_then(|()| stdout.flush())
        .is_err()
    {
        std::process::exit(1);
    }
    drop(stdout);
    let _ = std::io::stderr().write_all(outcome.stderr.as_bytes());
    if outcome.emit_event {
        let ms = started.elapsed().as_millis() as u64;
        match careerbrew_cli::routed::command_of(&args) {
            Some(command) => events::emit_command(&env.caps, command, outcome.code == 0, ms),
            None => events::emit(&env.caps, ms),
        }
    }
    std::process::exit(outcome.code);
}
