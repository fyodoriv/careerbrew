//! Initial synthetic wait control only; not a general process supervisor.

use std::process::{Child, ExitStatus};

pub fn verify_sigchld() -> Result<(), ()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0
        || action.sa_sigaction != libc::SIG_DFL
        || action.sa_flags & libc::SA_NOCLDWAIT != 0
    {
        return Err(());
    }
    Ok(())
}

// This runs only in the fixed single-control fixture. The outer owner bounds
// the fixture; its timeout does not prove that this child was reaped.
pub fn observe(mut child: Child) -> Result<(ExitStatus, usize), ()> {
    child.wait().map(|status| (status, 1)).map_err(|_| ())
}

use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    fixture: String,
    source: String,
    manifest: String,
    lock: String,
    toolchain: String,
    compiler: String,
    compiler_metadata: String,
}

fn digest_file(path: &Path, limit: u64) -> Result<String, ()> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| ())?;
    if !meta.is_file() || meta.len() > limit {
        return Err(());
    }
    let mut file = std::fs::File::open(path).map_err(|_| ())?;
    let mut hasher = Sha256::new();
    let mut buf = [0; 8192];
    let mut count = 0_u64;
    loop {
        let read = file.read(&mut buf).map_err(|_| ())?;
        if read == 0 {
            break;
        }
        count = count.checked_add(read as u64).ok_or(())?;
        if count > limit {
            return Err(());
        }
        hasher.update(&buf[..read]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn compiler_identity(root: &Path) -> Result<(String, String), ()> {
    for key in [
        "RUSTC",
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTUP_TOOLCHAIN",
    ] {
        if std::env::var_os(key).is_some_and(|v| !v.is_empty()) {
            return Err(());
        }
    }
    let mut command = Command::new("/bin/bash");
    command.args(["-c", r#"set -euo pipefail; source "$1/scripts/rust-readiness.sh"; rust_readiness >/dev/null; printf '%s\n' "$RUST_READY_BIN/rustc"; RUSTUP_AUTO_INSTALL=0 "$RUST_READY_BIN/rustc" -Vv"#, "fixture-compiler"])
        .arg(root)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").ok_or(())?)
        .env("HOME", std::env::var_os("HOME").ok_or(())?)
        .env("RUSTUP_AUTO_INSTALL", "0");
    if let Some(home) = std::env::var_os("RUSTUP_HOME") {
        command.env("RUSTUP_HOME", home);
    }
    let run = super::capture_actions::execute_during_observed(
        command,
        Duration::from_secs(15),
        |_, _| Ok(()),
    )
    .map_err(|_| ())?;
    if run.observation.wait_code != Some(0) || !run.stderr.is_empty() || run.stdout.len() > 8192 {
        return Err(());
    }
    let (path, metadata) = run.stdout.split_once('\n').ok_or(())?;
    let path = PathBuf::from(path);
    if !path.is_absolute() || metadata.is_empty() {
        return Err(());
    }
    validate_compiler_metadata(metadata)?;
    Ok((
        digest_file(&path, 64 * 1024 * 1024)?,
        format!("{:x}", Sha256::digest(metadata.as_bytes())),
    ))
}

fn validate_compiler_metadata(metadata: &str) -> Result<(), ()> {
    let releases: Vec<_> = metadata
        .lines()
        .filter_map(|line| line.strip_prefix("release: "))
        .collect();
    let hosts: Vec<_> = metadata
        .lines()
        .filter_map(|line| line.strip_prefix("host: "))
        .collect();
    if releases != ["1.99.1"] || hosts.len() != 1 || hosts[0].is_empty() {
        return Err(());
    }
    Ok(())
}

pub fn output_is_safe(stdout: &str, stderr: &str, canary: &str) -> bool {
    !canary.is_empty() && !stdout.contains(canary) && !stderr.contains(canary)
}

impl Receipt {
    fn valid(&self) -> bool {
        [
            &self.fixture,
            &self.source,
            &self.manifest,
            &self.lock,
            &self.toolchain,
            &self.compiler,
            &self.compiler_metadata,
        ]
        .iter()
        .all(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.valid() && other.valid() && self == other
    }

    pub fn capture(binary: &Path) -> Result<Self, ()> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().ok_or(())?;
        let (compiler, compiler_metadata) = compiler_identity(root)?;
        Ok(Self {
            fixture: digest_file(binary, 64 * 1024 * 1024)?,
            source: digest_file(
                &root.join("xtask/src/bin/store_supervisor_fixture.rs"),
                1024 * 1024,
            )?,
            manifest: digest_file(&root.join("xtask/Cargo.toml"), 1024 * 1024)?,
            lock: digest_file(&root.join("Cargo.lock"), 16 * 1024 * 1024)?,
            toolchain: digest_file(&root.join("rust-toolchain.toml"), 1024 * 1024)?,
            compiler,
            compiler_metadata,
        })
    }

    pub fn write_private(&self) -> Result<(), ()> {
        // Owner-only on purpose: under a 022 file mask a default scratch folder is group-readable.
        let dir = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir()
            .map_err(|_| ())?;
        if std::fs::metadata(dir.path())
            .map_err(|_| ())?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(());
        }
        let path = dir.path().join("receipt.json");
        let mut out = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&path)
            .map_err(|_| ())?;
        if !self.valid() {
            return Err(());
        }
        let bytes = serde_json::to_vec(self).map_err(|_| ())?;
        out.write_all(&bytes).map_err(|_| ())?;
        out.flush().map_err(|_| ())?;
        if out.metadata().map_err(|_| ())?.permissions().mode() & 0o077 != 0 {
            return Err(());
        }
        out.seek(SeekFrom::Start(0)).map_err(|_| ())?;
        let mut readback = Vec::new();
        out.take(4097).read_to_end(&mut readback).map_err(|_| ())?;
        if readback.len() > 4096 {
            return Err(());
        }
        let parsed: Receipt = serde_json::from_slice(&readback).map_err(|_| ())?;
        if !self.matches(&parsed) {
            return Err(());
        }
        Ok(())
    }
}

#[cfg(test)]
mod receipt_controls {
    use super::*;

    fn sample() -> Receipt {
        Receipt {
            fixture: "a".repeat(64),
            source: "b".repeat(64),
            manifest: "c".repeat(64),
            lock: "d".repeat(64),
            toolchain: "e".repeat(64),
            compiler: "f".repeat(64),
            compiler_metadata: "0".repeat(64),
        }
    }

    #[test]
    fn changed_or_invalid_receipt_is_rejected() {
        let baseline = sample();
        assert!(baseline.matches(&sample()));
        for field in 0..7 {
            let mut changed = sample();
            match field {
                0 => changed.fixture = "1".repeat(64),
                1 => changed.source = "1".repeat(64),
                2 => changed.manifest = "1".repeat(64),
                3 => changed.lock = "1".repeat(64),
                4 => changed.toolchain = "1".repeat(64),
                5 => changed.compiler = "1".repeat(64),
                _ => changed.compiler_metadata = "1".repeat(64),
            }
            assert!(!baseline.matches(&changed));
        }
        let mut malformed = sample();
        malformed.fixture = "missing".into();
        assert!(!malformed.matches(&malformed));
        assert!(malformed.write_private().is_err());
        assert!(serde_json::from_str::<Receipt>("{}").is_err());
        assert!(baseline.write_private().is_ok());
    }

    #[test]
    fn compiler_metadata_and_output_refuse_bad_evidence() {
        assert!(validate_compiler_metadata("release: 1.99.1\nhost: example-host\n").is_ok());
        for bad in [
            "",
            "release: 1.99.1\n",
            "release: 1.99.1\nhost: \n",
            "release: 1.99.1\nrelease: 1.99.1\nhost: host\n",
            "release: 1.99.1\nhost: one\nhost: two\n",
        ] {
            assert!(validate_compiler_metadata(bad).is_err());
        }
        assert!(output_is_safe("ok", "", "synthetic-canary"));
        assert!(!output_is_safe("synthetic-canary", "", "synthetic-canary"));
        assert!(!output_is_safe("", "synthetic-canary", "synthetic-canary"));
        assert!(!output_is_safe("", "", ""));
    }
}
