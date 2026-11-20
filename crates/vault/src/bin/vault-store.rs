//! `vault-store`: synthetic-only transport for the rust-05d cross-engine
//! checks (`tests/unit/test_rust05d_key_interop.py`). No production dispatch.
//!
//! - `vault-store read <key-file> <data-root> <file>` loads the key without
//!   creating it, opens the file and prints its text.
//! - `vault-store write <key-file> <data-root> <file> <seal|plain>` loads or
//!   creates the key and writes stdin to the file atomically.
//! - `vault-store write-if-changed ...` (same arguments) prints `wrote` or
//!   `unchanged`.
//!
//! It never prints the key. Exit 1 on a refusal (the message names the
//! error kind only), 2 on a usage error.
use careerbrew_vault::{atomic, keyfile};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

fn key(path: &Path, root: &Path, create: bool) -> Result<String, String> {
    let no_sealed = || false;
    let facts = keyfile::KeyFacts {
        env_key_file: None,
        home: Path::new("/synthetic-home"),
        data_root: root,
        has_sealed_data: &no_sealed,
    };
    let bytes = keyfile::load_key(create, Some(path), &facts).map_err(|e| match e {
        keyfile::KeyError::InsideDataRoot => "inside-data-root".to_string(),
        keyfile::KeyError::Missing(_) => "missing-key".to_string(),
        _ => "key-error".to_string(),
    })?;
    String::from_utf8(bytes).map_err(|_| "key-error".to_string())
}

fn run(args: &[String]) -> Result<String, String> {
    match args {
        [op, key_file, root, file] if op == "read" => {
            let key = key(Path::new(key_file), Path::new(root), false)?;
            let stored = std::fs::read(file).map_err(|_| "io".to_string())?;
            let plain =
                careerbrew_vault::open(Some(&key), &stored).map_err(|_| "vault".to_string())?;
            String::from_utf8(plain).map_err(|_| "utf8".to_string())
        }
        [op, key_file, root, file, mode]
            if (op == "write" || op == "write-if-changed")
                && (mode == "seal" || mode == "plain") =>
        {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .map_err(|_| "io".to_string())?;
            let key = key(Path::new(key_file), Path::new(root), true)?;
            let seal = mode == "seal";
            let path = Path::new(file);
            if op == "write" {
                atomic::write_text(path, Some(&key), seal, &text)
                    .map_err(|_| "write".to_string())?;
                Ok(String::new())
            } else {
                let wrote = atomic::write_text_if_changed(path, Some(&key), seal, &text)
                    .map_err(|_| "write".to_string())?;
                Ok(if wrote { "wrote" } else { "unchanged" }.to_string())
            }
        }
        _ => Err("usage".to_string()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(out) => {
            let _ = std::io::stdout().write_all(out.as_bytes());
            ExitCode::SUCCESS
        }
        Err(kind) if kind == "usage" => {
            eprintln!("usage: vault-store read|write|write-if-changed <key-file> <data-root> <file> [seal|plain]");
            ExitCode::from(2)
        }
        Err(kind) => {
            eprintln!("refused: {kind}");
            ExitCode::from(1)
        }
    }
}
