//! Synthetic-only rust-06 component transport. No production route selects it.

use careerbrew_vault::transfer::{Context, TransferError};
use std::io::{IsTerminal, Write};
use std::path::Path;
use std::process::ExitCode;

fn usage() -> ExitCode {
    eprintln!("vault-sync: --all | data status | backup create|verify|restore | pair join [key] | sync export|import <file>");
    ExitCode::from(2)
}

fn hidden_key() -> Result<String, TransferError> {
    if !std::io::stdin().is_terminal() {
        let mut value = String::new();
        std::io::stdin()
            .read_line(&mut value)
            .map_err(|_| TransferError::new("OSError", "cannot read pairing key"))?;
        return Ok(value.trim().to_owned());
    }
    let mut stderr = std::io::stderr().lock();
    stderr
        .write_all(b"pairing key (hidden): ")
        .and_then(|()| stderr.flush())
        .map_err(|_| TransferError::new("OSError", "terminal failed"))?;
    let mut saved: libc::termios;
    // SAFETY: the process owns its stdin descriptor for this invocation and
    // restores its exact terminal attributes before returning.
    unsafe {
        saved = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut saved) != 0 {
            return Err(TransferError::new("OSError", "terminal failed"));
        }
        let mut hidden = saved;
        hidden.c_lflag &= !libc::ECHO;
        if libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &hidden) != 0 {
            return Err(TransferError::new("OSError", "terminal failed"));
        }
    }
    let mut value = String::new();
    let read = std::io::stdin().read_line(&mut value);
    // SAFETY: restore the attributes read from this invocation's terminal.
    unsafe {
        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &saved);
    }
    std::io::stderr().write_all(b"\n").ok();
    read.map_err(|_| TransferError::new("OSError", "cannot read pairing key"))?;
    Ok(value.trim_end_matches(['\r', '\n']).to_owned())
}

fn run(args: &[String]) -> Result<String, TransferError> {
    let context = Context::from_env()?;
    match args {
        [data, status] if data == "data" && status == "status" => context.cmd_data_status(),
        [backup, create] if backup == "backup" && create == "create" => context.cmd_backup_create(),
        [backup, verify] if backup == "backup" && verify == "verify" => {
            context.cmd_backup_verify(None)
        }
        [backup, verify, file] if backup == "backup" && verify == "verify" => {
            context.cmd_backup_verify(Some(Path::new(file)))
        }
        [backup, restore, file, dest] if backup == "backup" && restore == "restore" => {
            context.cmd_backup_restore(Path::new(file), Path::new(dest))
        }
        [pair, join] if pair == "pair" && join == "join" => context.cmd_pair_join(&hidden_key()?),
        [pair, join, key] if pair == "pair" && join == "join" => context.cmd_pair_join(key),
        [sync, export, file] if sync == "sync" && export == "export" => {
            context.cmd_sync_export(Path::new(file))
        }
        [sync, import, file] if sync == "sync" && import == "import" => {
            context.cmd_sync_import(Path::new(file))
        }
        _ => Err(TransferError::new(
            "UsageError",
            "invalid component arguments",
        )),
    }
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    if args == ["--all"] {
        return match careerbrew_vault::synthetic_transfer::run_all() {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("vault-sync: {error}");
                ExitCode::from(1)
            }
        };
    }
    if args.len() == 2 && args[0] == "--case" {
        return match careerbrew_vault::synthetic_transfer::run_case(&args[1]) {
            Ok(output) => {
                print!("{output}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("vault-sync: {error}");
                ExitCode::from(1)
            }
        };
    }
    match run(&args) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(error) if error.class == "UsageError" => usage(),
        Err(error) => {
            eprintln!("{}", error.message);
            ExitCode::from(1)
        }
    }
}
