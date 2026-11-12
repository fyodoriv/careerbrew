const MIB: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mode {
    Small,
    Large,
}

impl Mode {
    fn payload(self) -> u64 {
        match self {
            Self::Small => 16 * MIB,
            Self::Large => 128 * MIB,
        }
    }
    fn checksum(self) -> u64 {
        match self {
            Self::Small => 0x3fba67b5df100000,
            Self::Large => 0x941c6daef8800000,
        }
    }
}

fn schedule() -> [Mode; 6] {
    [
        Mode::Small,
        Mode::Large,
        Mode::Small,
        Mode::Large,
        Mode::Small,
        Mode::Large,
    ]
}

#[derive(Debug)]
struct Identity {
    pid: u32,
    mode: Mode,
    fd: i32,
    device: u64,
    inode: u64,
    uid: u32,
    file_mode: u32,
}

#[derive(Clone, Debug, serde::Serialize)]
struct InvocationBinding {
    program_bytes: Vec<u8>,
    argument_bytes: Vec<Vec<u8>>,
    configured_environment_keys: Vec<Vec<u8>>,
    environment_scope: &'static str,
}

#[derive(Clone, Debug, serde::Serialize)]
struct OwnerFacts {
    pid: u32,
    code: Option<i32>,
    signal: Option<i32>,
    terminal: bool,
    reaped: bool,
    cleanup: bool,
    absent: bool,
    consumes: usize,
    attempts: usize,
    raw_valid: bool,
    raw_rss: Option<i64>,
    raw_usage: Option<String>,
    failure: Option<String>,
    invocation: Option<InvocationBinding>,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
struct UnitMatch {
    bytes: bool,
    kib: bool,
    oracle: u64,
    raw: u64,
}

#[derive(Debug, PartialEq, Eq)]
enum Interpretation {
    Bytes,
    KiB,
}

fn validate_record(bytes: &[u8], id: &Identity, owner: &OwnerFacts) -> Result<UnitMatch, String> {
    if bytes.len() != 96 {
        return Err("record length".into());
    }
    let u32_at = |n| u32::from_le_bytes(bytes[n..n + 4].try_into().unwrap());
    let u64_at = |n| u64::from_le_bytes(bytes[n..n + 8].try_into().unwrap());
    if &bytes[..8] != b"CBRSSU01" || u32_at(8) != 1 || u64_at(88) != 0 {
        return Err("record schema".into());
    }
    if id.pid == 0
        || id.fd < 3
        || id.uid == 0
        || id.file_mode != 0o100600
        || u32_at(12) != id.pid
        || u32_at(16) != id.mode as u32
        || u32_at(20) != id.fd as u32
        || u64_at(24) != id.mode.payload()
        || u64_at(64) != id.device
        || u64_at(72) != id.inode
        || u32_at(80) != id.uid
        || u32_at(84) != id.file_mode
    {
        return Err("record identity".into());
    }
    let page = u64_at(32);
    if ![4096, 16384].contains(&page)
        || !id.mode.payload().is_multiple_of(page)
        || u64_at(40) != id.mode.checksum()
        || u64_at(48) != id.mode.checksum()
    {
        return Err("page or checksum".into());
    }
    if owner.pid != id.pid
        || owner.code != Some(0)
        || owner.signal.is_some()
        || !owner.terminal
        || !owner.reaped
        || !owner.cleanup
        || !owner.absent
        || owner.consumes != 1
        || owner.attempts < 1
        || !owner.raw_valid
        || owner.raw_usage.is_none()
        || owner.failure.is_some()
    {
        return Err("owner outcome".into());
    }
    let raw =
        u64::try_from(owner.raw_rss.ok_or("missing raw usage")?).map_err(|_| "negative raw RSS")?;
    let kib = raw.checked_mul(1024).ok_or("KiB multiplication overflow")?;
    let oracle = u64_at(56);
    if oracle < id.mode.payload()
        || oracle
            > id.mode
                .payload()
                .checked_add(32 * MIB)
                .ok_or("oracle bound overflow")?
    {
        return Err("oracle bound".into());
    }
    let upper = oracle.checked_add(8 * MIB).ok_or("unit bound overflow")?;
    Ok(UnitMatch {
        bytes: (oracle..=upper).contains(&raw),
        kib: (oracle..=upper).contains(&kib),
        oracle,
        raw,
    })
}

fn classify_cohort(values: &[UnitMatch]) -> Result<Interpretation, String> {
    if values.len() != 6 {
        return Err("requires six outcomes".into());
    }
    let bytes = values.iter().all(|v| v.bytes && !v.kib);
    let kib = values.iter().all(|v| v.kib && !v.bytes);
    if bytes == kib {
        return Err("nonunique or mixed interpretation".into());
    }
    for pair in values.as_chunks::<2>().0 {
        let small = pair[0];
        let large = pair[1];
        let factor = if bytes { 1 } else { 1024 };
        let small_raw = small.raw.checked_mul(factor).ok_or("pair unit overflow")?;
        let large_raw = large.raw.checked_mul(factor).ok_or("pair unit overflow")?;
        for (s, l) in [(small.oracle, large.oracle), (small_raw, large_raw)] {
            let difference = l.checked_sub(s).ok_or("pair decreases")?;
            if !(80 * MIB..=144 * MIB).contains(&difference)
                || l < s.checked_mul(2).ok_or("ratio overflow")?
            {
                return Err("pair difference or ratio".into());
            }
        }
    }
    Ok(if bytes {
        Interpretation::Bytes
    } else {
        Interpretation::KiB
    })
}

#[cfg(target_os = "macos")]
pub mod capture_actions {
    include!("../src/capture_actions.rs");

    pub(super) fn configured_environment_keys(command: &Command) -> Vec<Vec<u8>> {
        use std::os::unix::ffi::OsStrExt;
        let mut keys: Vec<_> = command
            .get_envs()
            .filter(|(_, value)| value.is_some())
            .map(|(key, _)| key.as_bytes().to_vec())
            .collect();
        keys.sort();
        keys
    }

    pub(super) fn reduce_owner_observation(
        mut facts: super::OwnerFacts,
        observation: Option<OwnerObservation>,
        environment_keys: Vec<Vec<u8>>,
    ) -> super::OwnerFacts {
        use std::os::unix::ffi::OsStrExt;
        facts.invocation = observation
            .as_ref()
            .map(|observed| super::InvocationBinding {
                program_bytes: observed.program.as_bytes().to_vec(),
                argument_bytes: observed
                    .args
                    .iter()
                    .map(|argument| argument.as_bytes().to_vec())
                    .collect(),
                configured_environment_keys: environment_keys,
                environment_scope:
                    "parent Command after env_clear; not independently observed child environment",
            });
        if let Some(observed) = observation {
            facts.pid = observed.pid;
            facts.code = observed.wait_code;
            facts.signal = observed.wait_signal;
            facts.terminal = observed.wait_observed;
            facts.reaped = observed.reaped;
            facts.cleanup = observed.cleanup_succeeded;
        }
        facts
    }

    pub(super) fn invoke(
        command: Command,
        restore: impl FnOnce() -> Result<(), String>,
    ) -> super::OwnerFacts {
        let environment_keys = configured_environment_keys(&command);
        let probe = OwnerProbe::default();
        let result = execute_during_observed_inner(
            command,
            Duration::from_secs(30),
            |_, _| restore(),
            Some(probe.clone()),
        );
        let (observation, failure) = match result {
            Ok(value) => (Some(value.observation), None),
            Err(value) => (value.observation, Some(format!("{:?}", value.cause))),
        };
        let terminal = probe.0.terminal.borrow();
        let raw_valid = terminal
            .as_ref()
            .is_some_and(|(_, _, usage)| usage.1.is_ok());
        let raw_rss = terminal
            .as_ref()
            .and_then(|(_, _, usage)| usage.0.as_ref())
            .map(|raw| {
                let RawWaitUsage::MacOSWait4BytesUncalibrated { max_rss, .. } = raw;
                *max_rss
            });
        let raw_usage = terminal
            .as_ref()
            .and_then(|(_, _, usage)| usage.0.as_ref())
            .map(|raw| format!("{raw:?}"));
        let facts = super::OwnerFacts {
            pid: 0,
            code: None,
            signal: None,
            terminal: false,
            reaped: false,
            cleanup: false,
            absent: false,
            consumes: probe.0.native_reaps.get(),
            attempts: probe.0.wait_attempts.get(),
            raw_valid,
            raw_rss,
            raw_usage,
            failure,
            invocation: None,
        };
        let mut facts = reduce_owner_observation(facts, observation, environment_keys);
        if facts.reaped && facts.cleanup && facts.pid > 0 {
            let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
            let wait = unsafe {
                libc::waitid(
                    libc::P_PID,
                    facts.pid,
                    &mut info,
                    libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
                )
            };
            let wait_errno = std::io::Error::last_os_error().raw_os_error();
            let signal = unsafe { libc::kill(facts.pid as i32, 0) };
            let signal_errno = std::io::Error::last_os_error().raw_os_error();
            facts.absent = wait == -1
                && wait_errno == Some(libc::ECHILD)
                && signal == -1
                && signal_errno == Some(libc::ESRCH);
        }
        facts
    }
}

#[test]
#[ignore = "requires reviewed exclusive experiment"]
fn fixed_darwin_leaf_units() {
    #[cfg(target_os = "macos")]
    actual::run().unwrap();
    #[cfg(not(target_os = "macos"))]
    panic!("Darwin unit calibration unavailable on this platform");
}

#[cfg(target_os = "macos")]
mod actual {
    use super::*;
    use sha2::{Digest, Sha256};
    use std::{
        collections::BTreeMap,
        fs::{self, File, OpenOptions},
        io::{Read, Seek, SeekFrom, Write},
        os::{
            fd::{AsRawFd, RawFd},
            unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        },
        path::{Path, PathBuf},
        process::Command,
    };

    const PREDICTION: &str = "210105c190d8b0a16779cf0267ae3448536631f3";
    const SOURCES: &[&str] = &[
        ".gitignore",
        "AGENTS.md",
        "docs/privacy.md",
        "docs/plans/2026-11-01-rust-code-inventory.csv",
        "docs/plans/2026-11-06-rust-leaf-unit-calibration.md",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "xtask/Cargo.toml",
        "xtask/src/capture_actions.rs",
        "xtask/examples/wait4_leaf.rs",
        "xtask/tests/wait4_unit_calibration.rs",
    ];

    #[derive(serde::Deserialize, serde::Serialize)]
    #[serde(deny_unknown_fields)]
    struct Release {
        schema: u32,
        approval: String,
        prediction_commit: String,
        source_tree: String,
        sources: BTreeMap<String, String>,
        binary_sha256: String,
        test_binary_sha256: String,
        system_version_sha256: String,
        sdk_settings_sha256: String,
        rust_unix_sha256: String,
        runtime_identity_sha256: String,
        kernel_release: String,
        kernel_version: String,
        machine: String,
    }

    fn digest(path: &Path) -> Result<String, String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "missing pinned file")?;
        if !metadata.is_file() || metadata.len() > 128 * MIB {
            return Err("invalid pinned file".into());
        }
        let bytes = fs::read(path).map_err(|_| "cannot read pinned file")?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    fn check_pin(path: &Path, expected: &str) -> Result<(), String> {
        if expected.len() != 64
            || !expected
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || digest(path)? != expected
        {
            return Err("pinned identity mismatch".into());
        }
        Ok(())
    }

    fn owned_mode(path: &Path, uid: u32, mode: u32, directory: bool) -> Result<(), String> {
        let metadata = fs::symlink_metadata(path).map_err(|_| "missing owned input")?;
        if metadata.uid() != uid
            || metadata.permissions().mode() & 0o7777 != mode
            || (if directory {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            })
        {
            return Err("owned input mode or type".into());
        }
        Ok(())
    }

    fn validate_release(root: &Path, area: &Path) -> Result<Release, String> {
        let uid = unsafe { libc::getuid() };
        if uid == 0
            || uid != unsafe { libc::geteuid() }
            || unsafe { libc::getgid() } != unsafe { libc::getegid() }
        {
            return Err("credentials unavailable".into());
        }
        owned_mode(area, uid, 0o700, true)?;
        let release_path = area.join("release.json");
        owned_mode(&release_path, uid, 0o600, false)?;
        if fs::metadata(&release_path)
            .map_err(|_| "release metadata")?
            .len()
            > 32 * 1024
        {
            return Err("release size".into());
        }
        let release: Release =
            serde_json::from_slice(&fs::read(&release_path).map_err(|_| "release read")?)
                .map_err(|_| "release schema")?;
        if release.schema != 1
            || release.approval != "root-reviewed-source-and-binary-exclusive-six"
            || release.prediction_commit != PREDICTION
            || release.source_tree.len() != 40
            || !release.source_tree.bytes().all(|b| b.is_ascii_hexdigit())
            || release.sources.len() != SOURCES.len()
            || release.machine != "arm64"
        {
            return Err("missing later root release".into());
        }
        for path in SOURCES {
            check_pin(
                &root.join(path),
                release.sources.get(*path).ok_or("missing source pin")?,
            )?;
        }
        check_pin(
            &root.join("docs/plans/2026-11-06-rust-leaf-unit-calibration.md"),
            "a86a70aa0fac883aaea9beb23953c9fa771a54349afc744ebc847372516b655c",
        )?;
        let binary = area.join("frozen/wait4_leaf");
        owned_mode(&area.join("frozen"), uid, 0o700, true)?;
        owned_mode(&binary, uid, 0o700, false)?;
        check_pin(&binary, &release.binary_sha256)?;
        check_pin(
            &std::env::current_exe().map_err(|_| "test executable identity")?,
            &release.test_binary_sha256,
        )?;
        check_pin(
            Path::new("/System/Library/CoreServices/SystemVersion.plist"),
            &release.system_version_sha256,
        )?;
        check_pin(Path::new("/Applications/Xcode.app/Contents/Developer/Platforms/MacOSX.platform/Developer/SDKs/MacOSX.sdk/SDKSettings.json"), &release.sdk_settings_sha256)?;
        check_pin(Path::new("/opt/homebrew/Cellar/rust/1.99.1/lib/rustlib/src/rust/library/std/src/sys/process/unix/unix.rs"), &release.rust_unix_sha256)?;
        owned_mode(&area.join("runtime-identity.json"), uid, 0o600, false)?;
        check_pin(
            &area.join("runtime-identity.json"),
            &release.runtime_identity_sha256,
        )?;
        let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
        if unsafe { libc::uname(&mut uts) } != 0 {
            return Err("kernel identity unavailable".into());
        }
        for (actual, expected) in [
            (&uts.release[..], &release.kernel_release),
            (&uts.version[..], &release.kernel_version),
            (&uts.machine[..], &release.machine),
        ] {
            let actual = actual
                .iter()
                .take_while(|b| **b != 0)
                .map(|b| *b as u8)
                .collect::<Vec<_>>();
            if expected.is_empty() || actual != expected.as_bytes() {
                return Err("kernel identity changed".into());
            }
        }
        Ok(release)
    }

    // One invocation owns this File through restoration; no stale fd or foreign slot is retained.
    struct Inherited {
        file: Option<File>,
        original: i32,
        active: bool,
    }

    impl Inherited {
        fn new(file: File) -> Result<Self, String> {
            let fd = file.as_raw_fd();
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if fd < 3 || flags < 0 || flags & libc::FD_CLOEXEC == 0 {
                return Err("owned descriptor flags".into());
            }
            let mut inherited = Self {
                file: Some(file),
                original: flags,
                active: true,
            };
            if unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } == -1 {
                inherited.active = false;
                return Err("cannot enable owned inheritance".into());
            }
            Ok(inherited)
        }

        fn restore(&mut self) -> Result<(), String> {
            if !self.active {
                return if self.file.is_some() {
                    Ok(())
                } else {
                    Err("owned descriptor closed".into())
                };
            }
            let fd = self
                .file
                .as_ref()
                .ok_or("owned descriptor missing")?
                .as_raw_fd();
            self.active = false;
            if unsafe { libc::fcntl(fd, libc::F_SETFD, self.original) } == -1 {
                self.file.take();
                return Err("owned descriptor restoration failed".into());
            }
            Ok(())
        }
    }

    impl Drop for Inherited {
        fn drop(&mut self) {
            let _ = self.restore();
        }
    }

    #[derive(serde::Serialize)]
    pub(super) struct Sample {
        pub(super) ordinal: usize,
        pub(super) mode: &'static str,
        pub(super) owner: Option<OwnerFacts>,
        pub(super) record: Vec<u8>,
        pub(super) matched: Option<UnitMatch>,
        pub(super) refusal: Option<String>,
        pub(super) resource_admitted: bool,
    }

    fn sample(binary: &Path, folder: &Path, ordinal: usize, mode: Mode) -> Sample {
        let mut row = Sample {
            ordinal,
            mode: if mode == Mode::Small { "S" } else { "L" },
            owner: None,
            record: Vec::new(),
            matched: None,
            refusal: None,
            resource_admitted: false,
        };
        let result = (|| -> Result<UnitMatch, String> {
            let file = OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(folder.join(format!("oracle-{ordinal}.bin")))
                .map_err(|_| "oracle creation")?;
            let metadata = file.metadata().map_err(|_| "oracle identity")?;
            let fd: RawFd = file.as_raw_fd();
            let mut id = Identity {
                pid: 0,
                mode,
                fd,
                device: u64::from(metadata.dev() as u32),
                inode: metadata.ino(),
                uid: metadata.uid(),
                file_mode: metadata.mode(),
            };
            let mut command = Command::new(binary);
            command.arg(row.mode).arg(fd.to_string()).env_clear();
            let mut inherited = Inherited::new(file)?;
            let facts = capture_actions::invoke(command, || inherited.restore());
            let restored = inherited.restore();
            id.pid = facts.pid;
            row.owner = Some(facts);
            restored?;
            let file = inherited.file.as_mut().ok_or("oracle closed")?;
            file.seek(SeekFrom::Start(0)).map_err(|_| "oracle rewind")?;
            file.take(97)
                .read_to_end(&mut row.record)
                .map_err(|_| "oracle read")?;
            validate_record(
                &row.record,
                &id,
                row.owner.as_ref().ok_or("missing owner outcome")?,
            )
        })();
        match result {
            Ok(value) => row.matched = Some(value),
            Err(reason) => row.refusal = Some(reason),
        }
        row
    }

    pub(super) fn encode_receipt(value: &impl serde::Serialize) -> Result<Vec<u8>, String> {
        let bytes = serde_json::to_vec_pretty(value).map_err(|_| "receipt encoding")?;
        if bytes.len() > 128 * 1024 {
            return Err("receipt bound".into());
        }
        Ok(bytes)
    }

    fn retain(path: &Path, value: &impl serde::Serialize) -> Result<(), String> {
        let bytes = encode_receipt(value)?;
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path)
            .map_err(|_| "receipt exists or unavailable")?
            .write_all(&bytes)
            .map_err(|_| "receipt write".into())
    }

    pub(super) fn run() -> Result<(), String> {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .ok_or("repository root")?
            .to_path_buf();
        let area = root.join(".metrics/wait4-phase-b-actual");
        let release = validate_release(&root, &area)?;
        // A fixed create-new directory seals this one experiment against an unnoticed second run.
        let folder = area.join("run");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&folder)
            .map_err(|_| "experiment already claimed or unavailable")?;
        retain(&folder.join("release.json"), &release)?;
        let binary = area.join("frozen/wait4_leaf");
        let mut rows = Vec::new();
        for (ordinal, mode) in schedule().into_iter().enumerate() {
            // Recheck the exact executable before each spawn; never accept a rebuilt Cargo stub.
            if let Err(error) = check_pin(&binary, &release.binary_sha256) {
                retain(
                    &folder.join("stopped.json"),
                    &serde_json::json!({"ordinal":ordinal,"refusal":error,"resource_admitted":false}),
                )?;
                break;
            }
            let row = sample(&binary, &folder, ordinal, mode);
            retain(&folder.join(format!("outcome-{ordinal}.json")), &row)?;
            let cleaned = row
                .owner
                .as_ref()
                .is_some_and(|facts| facts.cleanup && facts.reaped && facts.absent);
            rows.push(row);
            if !cleaned {
                break;
            }
        }
        let values: Vec<_> = rows.iter().filter_map(|row| row.matched).collect();
        let verdict = classify_cohort(&values);
        retain(
            &folder.join("summary.json"),
            &serde_json::json!({
            "scope":"fixed Darwin leaf unit interpretation only", "prediction_commit":PREDICTION,
            "attempted":rows.len(),"valid":values.len(),"byte_matches":values.iter().filter(|v|v.bytes).count(),
            "kib_matches":values.iter().filter(|v|v.kib).count(),"verdict":format!("{verdict:?}"),
            "resource_admitted":false,"rows":rows}),
        )?;
        println!("phase-B evidence: {}", folder.display());
        verdict.map(|_| ())
    }
}

#[cfg(test)]
mod wire_controls {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn observed_invocation_survives_real_outcome_encoding() {
        use std::{ffi::OsString, os::unix::ffi::OsStringExt, process::Command};
        let program = b"/synthetic/leaf-\xff".to_vec();
        let argument = b"arg-\xfe".to_vec();
        let key = b"KEY_\xfd".to_vec();
        let mut command = Command::new(OsString::from_vec(program.clone()));
        command
            .arg(OsString::from_vec(argument.clone()))
            .env_clear()
            .env(OsString::from_vec(key.clone()), "ENV_VALUE_MUST_NOT_APPEAR")
            .env("A_KEY", "ANOTHER_VALUE_MUST_NOT_APPEAR")
            .env_remove("REMOVED");
        let keys = capture_actions::configured_environment_keys(&command);
        assert_eq!(keys, vec![b"A_KEY".to_vec(), key]);
        for state in ["observed", "absent", "failed"] {
            let mut facts = owner(17 * MIB as i64);
            facts.pid = 0;
            facts.code = None;
            facts.terminal = false;
            facts.reaped = false;
            facts.cleanup = false;
            facts.absent = false;
            if state == "failed" {
                facts.failure = Some("Callback".into());
            }
            let observation = (state != "absent").then(|| capture_actions::OwnerObservation {
                pid: 4242,
                program: OsString::from_vec(program.clone()),
                args: vec![OsString::from_vec(argument.clone())],
                wait_code: if state == "failed" {
                    Some(101)
                } else {
                    Some(0)
                },
                wait_signal: None,
                wait_observed: true,
                reaped: true,
                cleanup_succeeded: true,
            });
            let facts = capture_actions::reduce_owner_observation(facts, observation, keys.clone());
            let row = actual::Sample {
                ordinal: 0,
                mode: "S",
                owner: Some(facts),
                record: vec![],
                matched: None,
                refusal: None,
                resource_admitted: false,
            };
            let encoded = actual::encode_receipt(&row).unwrap();
            let text = std::str::from_utf8(&encoded).unwrap();
            assert!(
                !text.contains("ENV_VALUE_MUST_NOT_APPEAR")
                    && !text.contains("ANOTHER_VALUE_MUST_NOT_APPEAR")
            );
            let value: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
            let saved = value["owner"].as_object().unwrap();
            assert!(
                saved.contains_key("invocation"),
                "owner-observed invocation binding is missing"
            );
            if state == "absent" {
                assert!(saved["invocation"].is_null());
                assert_eq!(saved["pid"], 0);
            } else {
                assert_eq!(
                    saved["invocation"]["program_bytes"],
                    serde_json::json!(program)
                );
                assert_eq!(
                    saved["invocation"]["argument_bytes"],
                    serde_json::json!([argument])
                );
                assert_eq!(
                    saved["invocation"]["configured_environment_keys"],
                    serde_json::json!(keys)
                );
                assert_eq!(
                    saved["invocation"]["environment_scope"],
                    "parent Command after env_clear; not independently observed child environment"
                );
                assert_eq!(saved["pid"], 4242);
                assert_eq!(
                    saved["failure"],
                    if state == "failed" {
                        serde_json::json!("Callback")
                    } else {
                        serde_json::Value::Null
                    }
                );
            }
            assert_eq!(value["resource_admitted"], false);
            assert!(value["matched"].is_null());
        }
        let cleared = Command::new("/synthetic/never-executed");
        let mut cleared = cleared;
        cleared.env_clear();
        assert!(capture_actions::configured_environment_keys(&cleared).is_empty());
    }

    fn expected(mode: Mode) -> Identity {
        Identity {
            pid: 4242,
            mode,
            fd: 17,
            device: 7,
            inode: 91,
            uid: 501,
            file_mode: 0o100600,
        }
    }

    fn owner(rss: i64) -> OwnerFacts {
        OwnerFacts {
            pid: 4242,
            code: Some(0),
            signal: None,
            terminal: true,
            reaped: true,
            cleanup: true,
            absent: true,
            consumes: 1,
            attempts: 1,
            raw_valid: true,
            raw_rss: Some(rss),
            raw_usage: Some("synthetic control; no native usage".into()),
            failure: None,
            invocation: None,
        }
    }

    fn wire(mode: Mode) -> [u8; 96] {
        let id = expected(mode);
        let mut bytes = [0; 96];
        bytes[..8].copy_from_slice(b"CBRSSU01");
        for (offset, value) in [
            (8, 1),
            (12, id.pid),
            (16, mode as u32),
            (20, id.fd as u32),
            (80, id.uid),
            (84, id.file_mode),
        ] {
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (offset, value) in [
            (24, mode.payload()),
            (32, 16384),
            (40, mode.checksum()),
            (48, mode.checksum()),
            (56, mode.payload() + MIB),
            (64, id.device),
            (72, id.inode),
        ] {
            bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    #[test]
    fn exact_wire_accepts_both_fixed_payloads() {
        for mode in [Mode::Small, Mode::Large] {
            let bytes = wire(mode);
            let result = validate_record(
                &bytes,
                &expected(mode),
                &owner((mode.payload() + MIB) as i64),
            )
            .unwrap();
            assert!(result.bytes);
            assert!(!result.kib);
            assert_eq!(result.oracle, mode.payload() + MIB);
            assert_eq!(bytes, wire(mode));
        }
    }

    #[test]
    fn wire_rejects_every_identity_checksum_and_structural_mismatch() {
        let valid = wire(Mode::Small);
        for offset in [0, 8, 12, 16, 20, 24, 32, 40, 48, 64, 72, 80, 84, 88] {
            let mut changed = valid;
            changed[offset] ^= 1;
            assert!(
                validate_record(&changed, &expected(Mode::Small), &owner(17 * MIB as i64)).is_err(),
                "offset {offset}"
            );
        }
        for length in [95, 97] {
            let mut changed = valid.to_vec();
            changed.resize(length, 0);
            assert!(
                validate_record(&changed, &expected(Mode::Small), &owner(17 * MIB as i64)).is_err()
            );
        }
        for page in [0_u64, 8192, 32768] {
            let mut changed = valid;
            changed[32..40].copy_from_slice(&page.to_le_bytes());
            assert!(
                validate_record(&changed, &expected(Mode::Small), &owner(17 * MIB as i64)).is_err()
            );
        }
    }

    #[test]
    fn owner_failures_and_invalid_usage_never_match_units() {
        let valid = owner(17 * MIB as i64);
        let mut cases = Vec::new();
        let mut x = valid.clone();
        x.code = Some(17);
        cases.push(x);
        let mut x = valid.clone();
        x.code = None;
        x.signal = Some(15);
        cases.push(x);
        let mut x = valid.clone();
        x.failure = Some("Deadline".into());
        cases.push(x);
        let mut x = valid.clone();
        x.raw_rss = None;
        cases.push(x);
        let mut x = valid.clone();
        x.raw_usage = None;
        cases.push(x);
        let mut x = valid.clone();
        x.raw_rss = Some(-1);
        cases.push(x);
        let mut x = valid.clone();
        x.raw_rss = Some(i64::MAX);
        cases.push(x);
        let mut x = valid.clone();
        x.raw_valid = false;
        cases.push(x);
        let mut x = valid.clone();
        x.pid += 1;
        cases.push(x);
        let mut x = valid.clone();
        x.terminal = false;
        cases.push(x);
        let mut x = valid.clone();
        x.reaped = false;
        cases.push(x);
        let mut x = valid.clone();
        x.cleanup = false;
        cases.push(x);
        let mut x = valid.clone();
        x.absent = false;
        cases.push(x);
        let mut x = valid.clone();
        x.consumes = 2;
        cases.push(x);
        let mut x = valid;
        x.attempts = 0;
        cases.push(x);
        for facts in cases {
            assert!(
                validate_record(&wire(Mode::Small), &expected(Mode::Small), &facts).is_err(),
                "{facts:?}"
            );
        }
    }

    #[test]
    fn fixed_cohort_requires_all_six_unique_matches_and_pair_bounds() {
        let values: Vec<_> = schedule()
            .into_iter()
            .map(|mode| {
                validate_record(
                    &wire(mode),
                    &expected(mode),
                    &owner((mode.payload() + MIB) as i64),
                )
                .unwrap()
            })
            .collect();
        assert_eq!(classify_cohort(&values), Ok(Interpretation::Bytes));
        assert!(classify_cohort(&values[..5]).is_err());
        for bad in [
            UnitMatch {
                bytes: false,
                kib: false,
                ..values[0]
            },
            UnitMatch {
                bytes: true,
                kib: true,
                ..values[0]
            },
            UnitMatch {
                bytes: false,
                kib: true,
                ..values[0]
            },
            UnitMatch {
                oracle: 100 * MIB,
                ..values[0]
            },
            UnitMatch {
                raw: 100 * MIB,
                ..values[0]
            },
        ] {
            let mut changed = values.clone();
            changed[0] = bad;
            assert!(classify_cohort(&changed).is_err());
        }
        for oracle in [16 * MIB - 1, 48 * MIB + 1] {
            let mut changed = wire(Mode::Small);
            changed[56..64].copy_from_slice(&oracle.to_le_bytes());
            assert!(
                validate_record(&changed, &expected(Mode::Small), &owner(oracle as i64)).is_err()
            );
        }
        assert_eq!(
            schedule(),
            [
                Mode::Small,
                Mode::Large,
                Mode::Small,
                Mode::Large,
                Mode::Small,
                Mode::Large
            ]
        );
    }
}
