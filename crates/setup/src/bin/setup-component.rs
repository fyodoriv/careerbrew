//! Synthetic-only rust-07 component transport. No production route selects it.

use careerbrew_setup::{cli, synthetic, Result, SetupError};
use std::io::Read;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant};

fn usage() -> ExitCode {
    eprintln!("setup-component: --all | --case <id> | init | plugin ... | machine ... | connectors list | pair init");
    ExitCode::from(2)
}

/// Connection-time credential helper. Only the invoking client receives the
/// header; stderr stays empty and the value never enters a fixture or log.
fn github_header_helper(gh: &Path) -> Result<String> {
    let mut child = Command::new(gh)
        .args(["auth", "token", "--hostname", "github.com"])
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| SetupError::io())?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SetupError::io());
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(SetupError::io());
            }
        }
    };
    if !status.success() {
        return Err(SetupError::io());
    }
    let mut bytes = Vec::new();
    child
        .stdout
        .take()
        .ok_or_else(SetupError::io)?
        .take(4097)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4096 {
        return Err(SetupError::io());
    }
    let token = String::from_utf8(bytes).map_err(|_| SetupError::io())?;
    let token = token.trim();
    if token.is_empty() || token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err(SetupError::io());
    }
    serde_json::to_string(&serde_json::json!({"Authorization": format!("Bearer {token}")}))
        .map_err(|_| SetupError::io())
}

fn run(args: &[String]) -> Result<(String, i32)> {
    match args {
        [command] if command == "init" => Ok((cli::cmd_init()?, 0)),
        [command, action, source] if command == "plugin" && action == "add" => {
            Ok((cli::cmd_plugin_add(Path::new(source), false)?, 0))
        }
        [command, action, source, flag]
            if command == "plugin" && action == "add" && flag == "--allow-exec" =>
        {
            Ok((cli::cmd_plugin_add(Path::new(source), true)?, 0))
        }
        [command, action] if command == "plugin" && action == "list" => {
            Ok((cli::cmd_plugin_list()?, 0))
        }
        [command, action] if command == "plugin" && action == "refresh" => {
            Ok((cli::cmd_plugin_refresh()?, 0))
        }
        [command, action] if command == "plugin" && action == "verify" => cli::cmd_plugin_verify(),
        [command, action, name] if command == "plugin" && action == "remove" => {
            Ok((cli::cmd_plugin_remove(name, false)?, 0))
        }
        [command, action, name, flag]
            if command == "plugin" && action == "remove" && flag == "--purge-data" =>
        {
            Ok((cli::cmd_plugin_remove(name, true)?, 0))
        }
        [command, action, role] if command == "machine" && action == "set" => {
            Ok((cli::cmd_machine_set(role, "")?, 0))
        }
        [command, action, role, label] if command == "machine" && action == "set" => {
            Ok((cli::cmd_machine_set(role, label)?, 0))
        }
        [command, action] if command == "machine" && action == "show" => {
            Ok((cli::cmd_machine_show()?, 0))
        }
        [command, action] if command == "machine" && action == "split-plugin-evidence" => {
            cli::cmd_machine_split_plugin_evidence(false)
        }
        [command, action, flag]
            if command == "machine" && action == "split-plugin-evidence" && flag == "--dry-run" =>
        {
            cli::cmd_machine_split_plugin_evidence(true)
        }
        [command, action] if command == "connectors" && action == "list" => {
            Ok((cli::cmd_connectors_list()?, 0))
        }
        [command, action] if command == "pair" && action == "init" => {
            Ok((cli::cmd_pair_init()?, 0))
        }
        [command, gh] if command == "github-header-helper" => {
            Ok((github_header_helper(Path::new(gh))? + "\n", 0))
        }
        _ => Err(SetupError::new("UsageError", "invalid component arguments")),
    }
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--all"] {
        return match synthetic::run_all() {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("setup-component: {}", error.class);
                ExitCode::FAILURE
            }
        };
    }
    if args.len() == 2 && args[0] == "--case" {
        return match synthetic::run_case(&args[1]) {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("setup-component: {}", error.class);
                ExitCode::FAILURE
            }
        };
    }
    match run(&args) {
        Ok((output, code)) => {
            print!("{output}");
            ExitCode::from(code.clamp(0, 255) as u8)
        }
        Err(error) if error.class == "UsageError" => usage(),
        Err(error) => {
            eprintln!("{}", error.class);
            ExitCode::FAILURE
        }
    }
}
