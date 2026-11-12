// Capture the Python action catalog in invocation-owned synthetic homes.

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

const CASE: &str = include_str!("../tests/fixtures/capture/actions-v1.json");
const HISTORICAL_REFERENCE: &str =
    include_str!("../tests/fixtures/capture/actions-reference-v1.json");
const HISTORICAL_REFERENCE_SHA256: &str =
    "99cc30068c2264989d166acc38f114494b6252d0a4860a32506af25a53ccbba0";
const REFERENCE: &str =
    include_str!("../tests/fixtures/capture/actions-reference-rules-promise-v3.json");
const OBSERVER: &str = include_str!("../tests/fixtures/capture/observer/sitecustomize.py");
const REFERENCE_SHA256: &str = "e5069cb03701d43d14e8ccfe9a66cc42a6d710d51ae86cc76b5ee292a1dae6e9";
const LIMIT: u64 = 4 * 1024 * 1024;

pub(crate) fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

pub(crate) fn files(
    root: &Path,
    folder: &Path,
    canary: Option<&str>,
) -> Result<BTreeMap<String, String>, String> {
    let mut found = BTreeMap::new();
    for entry in fs::read_dir(folder).map_err(|_| "cannot list owned files")? {
        let path = entry.map_err(|_| "cannot list owned entry")?.path();
        let metadata = fs::symlink_metadata(&path).map_err(|_| "cannot inspect owned entry")?;
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "entry escaped root")?
            .to_string_lossy()
            .into_owned();
        if canary.is_some_and(|needle| relative.contains(needle)) {
            return Err("capture canary leaked in retained path".into());
        }
        if metadata.is_dir() {
            found.insert(
                relative,
                format!("directory:{:o}", metadata.permissions().mode() & 0o777),
            );
            found.extend(files(root, &path, canary)?);
        } else if metadata.is_file() {
            let bytes = fs::read(path).map_err(|_| "cannot read owned file")?;
            if canary.is_some_and(|needle| {
                bytes
                    .windows(needle.len())
                    .any(|window| window == needle.as_bytes())
            }) {
                return Err("capture canary leaked in retained content".into());
            }
            found.insert(
                relative,
                format!(
                    "file:{:o}:{}",
                    metadata.permissions().mode() & 0o777,
                    digest(&bytes)
                ),
            );
        } else {
            return Err("unsupported owned entry".into());
        }
    }
    Ok(found)
}

pub(crate) fn sources(root: &Path) -> Result<Value, String> {
    let mut hashes = BTreeMap::new();
    for (path, hash) in files(root, &root.join("src/careerbrew"), None)? {
        if path.ends_with(".py") {
            hashes.insert(path, hash);
        }
    }
    for name in [
        "pyproject.toml",
        "uv.lock",
        "Cargo.lock",
        "xtask/Cargo.toml",
        "xtask/src/capture_actions.rs",
        "xtask/src/main.rs",
        "xtask/src/capture_http.rs",
        "xtask/tests/fixtures/capture/http_driver.py",
        "xtask/tests/fixtures/capture/http-v1.json",
        "xtask/tests/fixtures/capture/http-reference-v1.json",
        "xtask/tests/fixtures/capture/actions-reference-v1.json",
        "xtask/tests/fixtures/capture/actions-reference-promise-v2.json",
        "xtask/tests/fixtures/capture/http-reference-promise-v2.json",
        "docs/plans/2026-11-08-promise-action-delta.json",
        "docs/plans/2026-11-08-rules-action-delta.json",
        "xtask/tests/fixtures/capture/actions-reference-rules-v2.json",
        "xtask/tests/fixtures/capture/http-reference-rules-v2.json",
        "xtask/tests/fixtures/capture/actions-reference-rules-promise-v3.json",
        "xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json",
    ] {
        hashes.insert(
            name.to_owned(),
            digest(&fs::read(root.join(name)).map_err(|_| "source input missing")?),
        );
    }
    for (name, pin) in [
        (
            "docs/plans/2026-11-08-rules-action-delta.json",
            "e776df411b7f03ec602d8bbf57289d17280e24aeab1f118b3f352bd2e83f65db",
        ),
        (
            "xtask/tests/fixtures/capture/actions-reference-rules-v2.json",
            "173921bfce50fb42c6f667e3b4c9e51cc9a86b963f8cafc9ced1f67012a065a2",
        ),
        (
            "xtask/tests/fixtures/capture/http-reference-rules-v2.json",
            "cea13248b3e4fd0a72aead0d580b11829473c704568aad26d0567fbe385079a6",
        ),
        (
            "xtask/tests/fixtures/capture/actions-reference-rules-promise-v3.json",
            REFERENCE_SHA256,
        ),
        (
            "xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json",
            "bddc10911112bb79118ff109e0466426f912121996c4d7027276ee97ccdf8e64",
        ),
        (
            "docs/plans/2026-11-08-promise-action-delta.json",
            "320f56dc37908b55b21a01d119cb676844018c140652e1d93c8811f908e6671f",
        ),
        (
            "xtask/tests/fixtures/capture/actions-reference-v1.json",
            HISTORICAL_REFERENCE_SHA256,
        ),
        (
            "xtask/tests/fixtures/capture/actions-reference-promise-v2.json",
            "f8f8787727c3cb588692b8737fb2f618461eca7b63917694c32a2c7468468921",
        ),
        (
            "xtask/tests/fixtures/capture/http-reference-v1.json",
            "e58db0db976b3205bc206437141e3877a440a3a8a46f9da713e4a215fdb9eaf3",
        ),
        (
            "xtask/tests/fixtures/capture/http-reference-promise-v2.json",
            "c37759bd5a280e3abf21923117b554228ede773823c90e9e584cec302b8331b0",
        ),
    ] {
        if hashes.get(name).map(String::as_str) != Some(pin) {
            return Err("reference input differs".into());
        }
    }
    Ok(json!(hashes))
}

pub(crate) fn bounded(path: &Path) -> Result<String, String> {
    let file = File::open(path).map_err(|_| "capture stream missing")?;
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "cannot read capture stream")?;
    if bytes.len() as u64 > LIMIT {
        return Err("capture stream exceeds limit".into());
    }
    String::from_utf8(bytes).map_err(|_| "capture stream is not UTF-8".into())
}

#[cfg(target_os = "macos")]
fn owned_group_members(pgid: i32) -> std::io::Result<std::collections::BTreeSet<i32>> {
    let mut members = [0_i32; 1024];
    let capacity = std::mem::size_of_val(&members);
    let count = unsafe {
        libc::proc_listpgrppids(pgid, members.as_mut_ptr().cast(), capacity as libc::c_int)
    };
    // The wrapper returns a PID count, while its buffer size is in bytes.
    if count <= 0 || count as usize >= members.len() {
        return Err(std::io::Error::other("owned group listing is incomplete"));
    }
    let members: std::collections::BTreeSet<_> = members[..count as usize]
        .iter()
        .copied()
        .filter(|pid| *pid > 0)
        .collect();
    if !members.contains(&pgid) {
        return Err(std::io::Error::other(
            "unreaped group leader is absent from listing",
        ));
    }
    Ok(members)
}

#[cfg(target_os = "macos")]
fn zombie_only_group(pgid: i32) -> std::io::Result<bool> {
    let before = owned_group_members(pgid)?;
    for pid in &before {
        let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
        let size = std::mem::size_of_val(&info) as libc::c_int;
        let bytes = unsafe {
            libc::proc_pidinfo(
                *pid,
                libc::PROC_PIDT_SHORTBSDINFO,
                1,
                (&mut info as *mut libc::proc_bsdshortinfo).cast(),
                size,
            )
        };
        if bytes != size || info.pbsi_pid != *pid as u32 || info.pbsi_pgid != pgid as u32 {
            return Err(std::io::Error::other(
                "owned group member status is unreadable or changed",
            ));
        }
        if info.pbsi_status != libc::SZOMB {
            return Ok(false);
        }
    }
    if before != owned_group_members(pgid)? {
        return Err(std::io::Error::other(
            "owned group membership changed during status reads",
        ));
    }
    Ok(true)
}

#[cfg(not(target_os = "macos"))]
fn zombie_only_group(_pgid: i32) -> std::io::Result<bool> {
    Ok(false)
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[derive(Debug, Clone)]
enum RawWaitUsage {
    #[cfg(target_os = "macos")]
    MacOSWait4BytesUncalibrated {
        user_sec: libc::time_t,
        user_usec: libc::suseconds_t,
        system_sec: libc::time_t,
        system_usec: libc::suseconds_t,
        max_rss: libc::c_long,
    },
    #[cfg(target_os = "linux")]
    LinuxWait4KiBUncalibrated {
        user_sec: libc::time_t,
        user_usec: libc::suseconds_t,
        system_sec: libc::time_t,
        system_usec: libc::suseconds_t,
        max_rss: libc::c_long,
    },
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
type RawWaitUsage = ();

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RawUsageIssue {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    NegativeTime,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    InvalidTimeval,
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    NegativeRss,
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    CollectorUnsupported,
}

type RawUsageOutcome = (Option<RawWaitUsage>, Result<(), RawUsageIssue>);

#[derive(Debug)]
struct ReapedChild {
    status: std::process::ExitStatus,
    usage: RawUsageOutcome,
}

impl ReapedChild {
    fn into_parts(self) -> (std::process::ExitStatus, RawUsageOutcome) {
        (self.status, self.usage)
    }
}

#[cfg(test)]
impl std::ops::Deref for ReapedChild {
    type Target = std::process::ExitStatus;
    fn deref(&self) -> &Self::Target {
        &self.status
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn validate_raw(raw: &RawWaitUsage) -> Result<(), RawUsageIssue> {
    let (user_sec, user_usec, system_sec, system_usec, max_rss) = match *raw {
        #[cfg(target_os = "macos")]
        RawWaitUsage::MacOSWait4BytesUncalibrated {
            user_sec,
            user_usec,
            system_sec,
            system_usec,
            max_rss,
        } => (user_sec, user_usec, system_sec, system_usec, max_rss),
        #[cfg(target_os = "linux")]
        RawWaitUsage::LinuxWait4KiBUncalibrated {
            user_sec,
            user_usec,
            system_sec,
            system_usec,
            max_rss,
        } => (user_sec, user_usec, system_sec, system_usec, max_rss),
    };
    if user_sec < 0 || system_sec < 0 {
        Err(RawUsageIssue::NegativeTime)
    } else if !(0..1_000_000).contains(&user_usec) || !(0..1_000_000).contains(&system_usec) {
        Err(RawUsageIssue::InvalidTimeval)
    } else if max_rss < 0 {
        Err(RawUsageIssue::NegativeRss)
    } else {
        Ok(())
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
struct ReapContext<'a> {
    expected_pid: libc::pid_t,
    released: &'a mut bool,
    #[cfg(test)]
    raw_fault: Option<fn(&mut libc::rusage)>,
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
enum WaitAttempt {
    Interrupted,
    Failed(libc::c_int),
    Returned {
        pid: libc::pid_t,
        raw_status: libc::c_int,
        rusage: libc::rusage,
    },
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
enum ReapDecision {
    Retry,
    Done(std::io::Result<ReapedChild>),
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
fn finish_wait_attempt(context: &mut ReapContext<'_>, attempt: WaitAttempt) -> ReapDecision {
    use std::os::unix::process::ExitStatusExt;
    match attempt {
        WaitAttempt::Interrupted => ReapDecision::Retry,
        WaitAttempt::Failed(errno) => {
            *context.released = true;
            ReapDecision::Done(Err(std::io::Error::from_raw_os_error(errno)))
        }
        WaitAttempt::Returned {
            pid,
            raw_status,
            rusage,
        } => {
            *context.released = true;
            if pid != context.expected_pid {
                return ReapDecision::Done(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "wait4 returned a different PID",
                )));
            }
            if !libc::WIFEXITED(raw_status) && !libc::WIFSIGNALED(raw_status) {
                return ReapDecision::Done(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "wait4 returned a nonterminal status",
                )));
            }
            #[cfg(test)]
            let rusage = {
                let mut rusage = rusage;
                if let Some(fault) = context.raw_fault {
                    fault(&mut rusage);
                }
                rusage
            };
            #[cfg(target_os = "macos")]
            let raw = RawWaitUsage::MacOSWait4BytesUncalibrated {
                user_sec: rusage.ru_utime.tv_sec,
                user_usec: rusage.ru_utime.tv_usec,
                system_sec: rusage.ru_stime.tv_sec,
                system_usec: rusage.ru_stime.tv_usec,
                max_rss: rusage.ru_maxrss,
            };
            #[cfg(target_os = "linux")]
            let raw = RawWaitUsage::LinuxWait4KiBUncalibrated {
                user_sec: rusage.ru_utime.tv_sec,
                user_usec: rusage.ru_utime.tv_usec,
                system_sec: rusage.ru_stime.tv_sec,
                system_usec: rusage.ru_stime.tv_usec,
                max_rss: rusage.ru_maxrss,
            };
            let validation = validate_raw(&raw);
            ReapDecision::Done(Ok(ReapedChild {
                status: std::process::ExitStatus::from_raw(raw_status),
                usage: (Some(raw), validation),
            }))
        }
    }
}

#[cfg(test)]
type ProbeTerminal = (Option<i32>, Option<i32>, RawUsageOutcome);

// One owned invocation has one test probe; its last Rc handle releases the probe.
#[cfg(test)]
#[derive(Default)]
struct OwnerProbeState {
    signals: std::cell::Cell<usize>,
    wait_attempts: std::cell::Cell<usize>,
    native_reaps: std::cell::Cell<usize>,
    pid: std::cell::Cell<Option<u32>>,
    raw_fault: std::cell::Cell<Option<fn(&mut libc::rusage)>>,
    terminal: std::cell::RefCell<Option<ProbeTerminal>>,
}

#[cfg(test)]
#[derive(Clone, Default)]
struct OwnerProbe(std::rc::Rc<OwnerProbeState>);

#[cfg(test)]
impl OwnerProbe {
    fn get(&self) -> usize {
        self.0.signals.get()
    }
    fn set(&self, count: usize) {
        self.0.signals.set(count);
    }
}

// The command invocation owns this unreaped direct child and its process group.
struct RunningChild {
    child: Child,
    released: bool,
    #[cfg(test)]
    group_signals: OwnerProbe,
}

impl RunningChild {
    fn stop_and_reap(&mut self) -> std::io::Result<ReapedChild> {
        if self.released {
            return Err(std::io::Error::from_raw_os_error(libc::ECHILD));
        }
        // A successful waitid, including a running child, preserves ownership.
        // An error does not authorize signaling the former numeric group ID.
        if let Err(error) = leader_exited(&mut self.child) {
            self.released = true;
            return Err(error);
        }
        #[cfg(test)]
        self.group_signals.set(self.group_signals.get() + 1);
        // process_group(0) created this group. Its leader has not been reaped.
        let result = unsafe { libc::kill(-(self.child.id() as i32), libc::SIGKILL) };
        if result != 0 {
            let error = std::io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::EPERM) {
                match zombie_only_group(self.child.id() as i32) {
                    Ok(true) => {}
                    Ok(false) => return Err(error),
                    Err(reason) => {
                        return Err(std::io::Error::new(
                            error.kind(),
                            format!("{error}; zombie-group proof incomplete: {reason}"),
                        ))
                    }
                }
            } else if error.raw_os_error() != Some(libc::ESRCH) {
                return Err(error);
            }
        }
        #[cfg(any(target_os = "macos", target_os = "linux"))]
        loop {
            let expected_pid = self.child.id() as libc::pid_t;
            let mut raw_status = 0;
            let mut rusage: libc::rusage = unsafe { std::mem::zeroed() };
            let pid = unsafe { libc::wait4(expected_pid, &mut raw_status, 0, &mut rusage) };
            let errno = (pid == -1).then(|| {
                std::io::Error::last_os_error()
                    .raw_os_error()
                    .unwrap_or(libc::EIO)
            });
            // A final consuming outcome relinquishes ownership before any validation.
            if errno != Some(libc::EINTR) {
                self.released = true;
            }
            #[cfg(test)]
            {
                self.group_signals
                    .0
                    .wait_attempts
                    .set(self.group_signals.0.wait_attempts.get() + 1);
                if pid == expected_pid
                    && (libc::WIFEXITED(raw_status) || libc::WIFSIGNALED(raw_status))
                {
                    self.group_signals
                        .0
                        .native_reaps
                        .set(self.group_signals.0.native_reaps.get() + 1);
                }
            }
            let attempt = match errno {
                Some(libc::EINTR) => WaitAttempt::Interrupted,
                Some(error) => WaitAttempt::Failed(error),
                None => WaitAttempt::Returned {
                    pid,
                    raw_status,
                    rusage,
                },
            };
            let mut context = ReapContext {
                expected_pid,
                released: &mut self.released,
                #[cfg(test)]
                raw_fault: self.group_signals.0.raw_fault.get(),
            };
            match finish_wait_attempt(&mut context, attempt) {
                ReapDecision::Retry => continue,
                ReapDecision::Done(result) => {
                    #[cfg(test)]
                    if let Ok(reaped) = &result {
                        use std::os::unix::process::ExitStatusExt;
                        *self.group_signals.0.terminal.borrow_mut() = Some((
                            reaped.status.code(),
                            reaped.status.signal(),
                            reaped.usage.clone(),
                        ));
                    }
                    return result;
                }
            }
        }
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        {
            let status = self.child.wait();
            self.released = true;
            status.map(|status| ReapedChild {
                status,
                usage: (None, Err(RawUsageIssue::CollectorUnsupported)),
            })
        }
    }
}

impl Drop for RunningChild {
    fn drop(&mut self) {
        if !self.released {
            let _ = self.stop_and_reap();
        }
    }
}

fn retry_interrupted(mut poll: impl FnMut() -> std::io::Result<bool>) -> std::io::Result<bool> {
    loop {
        match poll() {
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            result => return result,
        }
    }
}

fn leader_exited(child: &mut Child) -> std::io::Result<bool> {
    retry_interrupted(|| {
        // Zeroed siginfo is valid input storage. WNOWAIT retains the direct child.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let result = unsafe {
            libc::waitid(
                libc::P_PID,
                child.id() as libc::id_t,
                &mut info,
                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
            )
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(unsafe { info.si_pid() } != 0)
    })
}

pub(crate) fn execute(command: Command) -> Result<(u32, Option<i32>, String, String), String> {
    execute_bounded(command, Duration::from_secs(30))
}

pub(crate) fn execute_during(
    command: Command,
    during: impl FnOnce(u32, &mut dyn FnMut() -> Result<bool, String>) -> Result<(), String>,
) -> Result<(u32, Option<i32>, String, String), String> {
    execute_during_bounded(command, Duration::from_secs(30), during)
}

#[cfg_attr(test, allow(dead_code))]
pub(crate) fn execute_bounded(
    command: Command,
    budget: Duration,
) -> Result<(u32, Option<i32>, String, String), String> {
    if budget == Duration::from_secs(30) {
        execute_during(command, |_, _| Ok(()))
    } else {
        execute_during_bounded(command, budget, |_, _| Ok(()))
    }
}

#[cfg_attr(test, allow(dead_code))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerFailureCause {
    InvalidBudget,
    Setup,
    Spawn,
    Deadline,
    StreamLimit,
    Ownership,
    Callback,
    Cleanup,
    Output,
}
#[cfg_attr(test, allow(dead_code))]
#[derive(Debug, Clone)]
pub(crate) struct OwnerObservation {
    pub(crate) pid: u32,
    pub(crate) program: std::ffi::OsString,
    pub(crate) args: Vec<std::ffi::OsString>,
    pub(crate) wait_code: Option<i32>,
    pub(crate) wait_signal: Option<i32>,
    pub(crate) wait_observed: bool,
    pub(crate) reaped: bool,
    pub(crate) cleanup_succeeded: bool,
}
#[derive(Debug)]
pub(crate) struct OwnerExecution {
    pub(crate) observation: OwnerObservation,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}
#[cfg_attr(test, allow(dead_code))]
#[derive(Debug)]
pub(crate) struct OwnerFailure {
    pub(crate) cause: OwnerFailureCause,
    pub(crate) observation: Option<OwnerObservation>,
    pub(crate) message: String,
}
impl OwnerObservation {
    fn terminal(mut self, status: std::process::ExitStatus) -> Self {
        use std::os::unix::process::ExitStatusExt;
        self.wait_code = status.code();
        self.wait_signal = status.signal();
        self.wait_observed = true;
        self.reaped = true;
        self.cleanup_succeeded = true;
        self
    }
}
fn execute_during_bounded(
    command: Command,
    budget: Duration,
    during: impl FnOnce(u32, &mut dyn FnMut() -> Result<bool, String>) -> Result<(), String>,
) -> Result<(u32, Option<i32>, String, String), String> {
    execute_during_observed(command, budget, during)
        .map(|run| {
            (
                run.observation.pid,
                run.observation.wait_code,
                run.stdout,
                run.stderr,
            )
        })
        .map_err(|failure| failure.message)
}

// One command invocation owns this child observation until actual cleanup/wait.
#[cfg_attr(test, allow(dead_code))]
pub(crate) fn execute_during_observed(
    command: Command,
    budget: Duration,
    during: impl FnOnce(u32, &mut dyn FnMut() -> Result<bool, String>) -> Result<(), String>,
) -> Result<OwnerExecution, OwnerFailure> {
    execute_during_observed_inner(
        command,
        budget,
        during,
        #[cfg(test)]
        None,
    )
}

fn execute_during_observed_inner(
    mut command: Command,
    budget: Duration,
    during: impl FnOnce(u32, &mut dyn FnMut() -> Result<bool, String>) -> Result<(), String>,
    #[cfg(test)] probe: Option<OwnerProbe>,
) -> Result<OwnerExecution, OwnerFailure> {
    let before_spawn = |cause, message: &str| OwnerFailure {
        cause,
        observation: None,
        message: message.into(),
    };
    if budget.is_zero() || budget > Duration::from_secs(30) {
        return Err(before_spawn(
            OwnerFailureCause::InvalidBudget,
            "capture command budget must be positive and at most 30 seconds",
        ));
    }
    let streams = tempfile::tempdir()
        .map_err(|_| before_spawn(OwnerFailureCause::Setup, "cannot create capture streams"))?;
    let stdout = streams.path().join("stdout");
    let stderr = streams.path().join("stderr");
    let program = command.get_program().to_owned();
    let args = command.get_args().map(std::ffi::OsStr::to_owned).collect();
    command.process_group(0);
    command
        .stdin(Stdio::null())
        .stdout(
            File::create(&stdout).map_err(|_| {
                before_spawn(OwnerFailureCause::Setup, "cannot create stdout stream")
            })?,
        )
        .stderr(
            File::create(&stderr).map_err(|_| {
                before_spawn(OwnerFailureCause::Setup, "cannot create stderr stream")
            })?,
        );
    let mut child = RunningChild {
        child: command
            .spawn()
            .map_err(|_| before_spawn(OwnerFailureCause::Spawn, "cannot execute capture child"))?,
        released: false,
        #[cfg(test)]
        group_signals: probe.unwrap_or_default(),
    };
    let pid = child.child.id();
    #[cfg(test)]
    child.group_signals.0.pid.set(Some(pid));
    let mut observation = OwnerObservation {
        pid,
        program,
        args,
        wait_code: None,
        wait_signal: None,
        wait_observed: false,
        reaped: false,
        cleanup_succeeded: false,
    };
    let deadline = Instant::now() + budget;
    let mut cause = OwnerFailureCause::Callback;
    let progress: Result<(), String> = (|| {
        let mut check = || {
            for path in [&stdout, &stderr] {
                let length = fs::metadata(path)
                    .map_err(|_| {
                        cause = OwnerFailureCause::Output;
                        "cannot inspect capture stream"
                    })?
                    .len();
                if length > LIMIT {
                    cause = OwnerFailureCause::StreamLimit;
                    return Err("capture stream exceeds limit".into());
                }
            }
            if Instant::now() >= deadline {
                cause = OwnerFailureCause::Deadline;
                return Err("capture child exceeded timeout".into());
            }
            leader_exited(&mut child.child).map_err(|error| {
                child.released = true;
                cause = OwnerFailureCause::Ownership;
                format!("cannot establish direct-child ownership: {error}")
            })
        };
        during(pid, &mut check)?;
        while !check()? {
            thread::sleep(Duration::from_millis(10));
        }
        Ok(())
    })();
    if let Err(primary) = progress {
        let message = match child.stop_and_reap() {
            Ok(reaped) => {
                let (status, _usage) = reaped.into_parts();
                let message = format!("{primary}; observed child exit {:?}", status.code());
                observation = observation.terminal(status);
                message
            }
            Err(error) => format!("{primary}; child cleanup failed: {error}"),
        };
        return Err(OwnerFailure {
            cause,
            observation: Some(observation),
            message,
        });
    }
    let (status, _usage) = child
        .stop_and_reap()
        .map_err(|error| OwnerFailure {
            cause: OwnerFailureCause::Cleanup,
            observation: Some(observation.clone()),
            message: format!("cannot clean capture child: {error}"),
        })?
        .into_parts();
    observation = observation.terminal(status);
    let stdout = bounded(&stdout).map_err(|message| OwnerFailure {
        cause: OwnerFailureCause::Output,
        observation: Some(observation.clone()),
        message,
    })?;
    let stderr = bounded(&stderr).map_err(|message| OwnerFailure {
        cause: OwnerFailureCause::Output,
        observation: Some(observation.clone()),
        message,
    })?;
    Ok(OwnerExecution {
        observation,
        stdout,
        stderr,
    })
}

pub(crate) fn version(python: &Path) -> Result<String, String> {
    let mut command = Command::new(python);
    command.arg("--version").env_clear();
    let (_, exit, stdout, stderr) = execute(command)?;
    if exit != Some(0) || !stderr.is_empty() {
        return Err("Python version command failed".into());
    }
    Ok(stdout)
}

pub(crate) fn owned_environment(
    root: &Path,
    owned: &Path,
) -> Result<BTreeMap<String, String>, String> {
    fs::create_dir(owned).map_err(|_| "cannot prepare capture home")?;
    fs::set_permissions(owned, fs::Permissions::from_mode(0o700))
        .map_err(|_| "cannot protect capture home")?;
    let mut environment = BTreeMap::new();
    for (name, suffix) in [
        ("HOME", "home"),
        ("CAREERBREW_HOME", "data"),
        ("TMPDIR", "tmp"),
        ("XDG_CONFIG_HOME", "config"),
        ("XDG_CACHE_HOME", "cache"),
        ("XDG_STATE_HOME", "state"),
        ("CAREERBREW_KEY_FILE", "keys/backup.key"),
        ("CAREERBREW_PAIR_KEY_FILE", "keys/pair.key"),
        ("CAREERBREW_COMMANDS_DIR", "commands"),
        ("CAREERBREW_BACKUP_DIRS", "backups"),
        ("CAREERBREW_CODEX_CONFIG", "no-codex.toml"),
        ("CAREERBREW_CLAUDE_CONFIG", "no-claude.json"),
    ] {
        environment.insert(
            name.to_owned(),
            owned.join(suffix).to_string_lossy().into_owned(),
        );
    }
    for folder in [
        "home", "data", "tmp", "config", "cache", "state", "keys", "commands", "backups",
    ] {
        fs::create_dir(owned.join(folder)).map_err(|_| "cannot prepare owned directory")?;
        fs::set_permissions(owned.join(folder), fs::Permissions::from_mode(0o700))
            .map_err(|_| "cannot protect owned directory")?;
    }
    for (key, value) in [
        ("CAREERBREW_EVENTS", "off"),
        ("CAREERBREW_LLM", "off"),
        ("CAREERBREW_NOTIFY", "off"),
        ("CAREERBREW_POSTHOG_KEY", ""),
        ("CAREERBREW_NOW", "2026-10-19T09:00:00"),
        ("PYTHONDONTWRITEBYTECODE", "1"),
        ("PYTHONHASHSEED", "0"),
        ("LC_ALL", "C"),
    ] {
        environment.insert(key.to_owned(), value.to_owned());
    }
    environment.insert(
        "CAREERBREW_HTTP_FIXTURES".into(),
        root.join("tests/fixtures/http")
            .to_string_lossy()
            .into_owned(),
    );
    Ok(environment)
}

fn observe(
    root: &Path,
    python: &Path,
    fixture: &Value,
    fault: &str,
    descendant: &Path,
) -> Result<Value, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create capture home")?;
    let owned = scratch.path().join("owned");
    let mut environment = owned_environment(root, &owned)?;
    let observer_root = root.join("xtask/tests/fixtures/capture/observer");
    let observer_source =
        fs::read(observer_root.join("sitecustomize.py")).map_err(|_| "observer source missing")?;
    if digest(&observer_source) != digest(OBSERVER.as_bytes()) {
        return Err("observer source changed".into());
    }
    let control = scratch.path().join("control.json");
    fs::write(
        &control,
        serde_json::to_vec(&json!({"fault": fault, "canary": fixture["canary"]})).unwrap(),
    )
    .map_err(|_| "cannot write synthetic control")?;
    let observer_path = scratch.path().join("observer.json");
    environment.insert(
        "CAREERBREW_CAPTURE_ROOT".into(),
        root.to_string_lossy().into_owned(),
    );
    environment.insert(
        "PYTHONPATH".into(),
        observer_root.to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_CAPTURE_INPUT".into(),
        control.to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_CAPTURE_OBSERVER".into(),
        observer_path.to_string_lossy().into_owned(),
    );
    environment.insert(
        "CAREERBREW_CAPTURE_DESCENDANT".into(),
        descendant.to_string_lossy().into_owned(),
    );
    let before = files(&owned, &owned, None)?;
    let args: Vec<_> = fixture["argv"]
        .as_array()
        .ok_or("missing fixture argv")?
        .iter()
        .map(|value| value.as_str().ok_or("invalid fixture argument"))
        .collect::<Result<_, _>>()?;
    let mut command = Command::new(python);
    command
        .args(&args)
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let (pid, exit, stdout, stderr) = execute(command)?;
    if exit != Some(0) {
        return Err(format!("action child exited unsuccessfully: {exit:?}"));
    }
    let canary = fixture["canary"].as_str().unwrap();
    if stdout.contains(canary) || stderr.contains(canary) {
        return Err("capture canary leaked in stdout or stderr".into());
    }
    if !observer_path.is_file() {
        return Err("action entry execution provenance missing".into());
    }
    let after = files(&owned, &owned, fixture["canary"].as_str())?;
    let observer: Value = serde_json::from_str(&bounded(&observer_path)?)
        .map_err(|_| "invalid execution provenance")?;
    let writes: BTreeMap<_, _> = after
        .iter()
        .filter(|(path, value)| before.get(*path) != Some(*value))
        .map(|(path, value)| (path.clone(), value.clone()))
        .collect();
    let removed: Vec<_> = before
        .keys()
        .filter(|path| !after.contains_key(*path))
        .cloned()
        .collect();
    Ok(json!({
        "pid": pid, "argv": std::iter::once(python.to_string_lossy().into_owned()).chain(args.into_iter().map(str::to_owned)).collect::<Vec<_>>(),
        "exit": exit, "stdout": stdout, "stderr": stderr,
        "writes": writes, "removed": removed,
        "environment_keys": environment.keys().collect::<Vec<_>>(),
        "observer": observer,
    }))
}

fn nullable_string(value: &Value) -> bool {
    value.is_null() || value.is_string()
}

fn validate_schema(text: &str) -> Result<(), String> {
    let fixture: Value = serde_json::from_str(CASE).unwrap();
    let actions: Vec<Value> =
        serde_json::from_str(text).map_err(|_| "action output is not JSON")?;
    let keys = |field: &str| {
        fixture[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect::<Vec<_>>()
    };
    let mut names = std::collections::BTreeSet::new();
    for action in &actions {
        let object = action.as_object().ok_or("invalid action record")?;
        let name = action["name"]
            .as_str()
            .filter(|name| !name.is_empty())
            .ok_or("invalid action name")?;
        if object.keys().map(String::as_str).collect::<Vec<_>>() != keys("action_keys")
            || !names.insert(name)
            || !action["help"].is_string()
            || action["command"] != format!("careerbrew {}", name.replace('.', " "))
            || !action["ui"].is_boolean()
            || !["agent_note", "confirm", "terminal_only"]
                .iter()
                .all(|field| nullable_string(&action[*field]))
            || action["ui"].as_bool() != Some(action["terminal_only"].is_null())
        {
            return Err("action schema differs".into());
        }
        let mut argument_names = std::collections::BTreeSet::new();
        for arg in action["args"].as_array().ok_or("invalid argument list")? {
            let object = arg.as_object().ok_or("invalid argument")?;
            let name = arg["name"]
                .as_str()
                .filter(|name| !name.is_empty())
                .ok_or("invalid argument name")?;
            let choices = &arg["choices"];
            let default = &arg["default"];
            if object.keys().map(String::as_str).collect::<Vec<_>>() != keys("argument_keys")
                || !argument_names.insert(name)
                || !arg["help"].is_string()
                || !nullable_string(&arg["flag"])
                || !arg["required"].is_boolean()
                || !arg["many"].is_boolean()
                || !matches!(
                    arg["kind"].as_str(),
                    Some("flag" | "choice" | "int" | "number" | "text")
                )
                || !(choices.is_null()
                    || choices
                        .as_array()
                        .is_some_and(|values| values.iter().all(Value::is_string)))
                || !(default.is_null()
                    || default.is_string()
                    || default.is_number()
                    || default.is_boolean()
                    || default.as_array().is_some_and(|values| {
                        values.iter().all(|v| {
                            v.is_string() || v.is_number() || v.is_boolean() || v.is_null()
                        })
                    }))
            {
                return Err("argument schema differs".into());
            }
        }
    }
    if actions.is_empty() {
        return Err("empty action contract".into());
    }
    Ok(())
}

fn validate_observer(observer: &Value, root: &Path, source_hashes: &Value) -> Result<(), String> {
    if observer["catalog_calls"] != 1 || !observer["descendant"].is_null() {
        return Err("catalog execution provenance failed".into());
    }
    let modules = observer["modules"]
        .as_object()
        .ok_or("missing imported modules")?;
    for required in [
        "__main__",
        "careerbrew",
        "careerbrew.actions",
        "careerbrew.cli",
    ] {
        if !modules.contains_key(required) {
            return Err("entry module was not observed".into());
        }
    }
    validate_loaded_modules(observer, root, source_hashes)
}

pub(crate) fn validate_loaded_modules(
    observer: &Value,
    root: &Path,
    source_hashes: &Value,
) -> Result<(), String> {
    let modules = observer["modules"]
        .as_object()
        .ok_or("missing imported modules")?;
    for module in modules.values() {
        let path = module["path"].as_str().ok_or("missing module path")?;
        let path = Path::new(path);
        if module["loader_origin"].as_str() != path.to_str() {
            return Err("module loader origin differs".into());
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| "imported Careerbrew module is outside checkout")?;
        let key = relative.to_string_lossy();
        let expected = source_hashes[key.as_ref()]
            .as_str()
            .ok_or("imported module is not source-pinned")?;
        let expected = expected.rsplit(':').next().unwrap();
        if module["sha256"] != expected {
            return Err("imported module hash differs".into());
        }
    }
    for field in ["external_modules", "site_inputs"] {
        if observer[field]
            .as_object()
            .is_none_or(|entries| entries.is_empty())
        {
            return Err("dependency evidence missing".into());
        }
    }
    for field in ["sys_path", "distributions", "unresolved"] {
        if observer[field]
            .as_array()
            .is_none_or(|entries| entries.is_empty())
        {
            return Err("dependency mapping missing".into());
        }
    }
    Ok(())
}

pub fn validate(receipt: &Value) -> Result<(), String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .canonicalize()
        .map_err(|_| "checkout missing")?;
    let fixture: Value = serde_json::from_str(CASE).unwrap();
    if receipt["fixture_sha256"] != digest(CASE.as_bytes())
        || digest(HISTORICAL_REFERENCE.as_bytes()) != HISTORICAL_REFERENCE_SHA256
        || fs::read(root.join("xtask/tests/fixtures/capture/actions-reference-v1.json"))
            .map_err(|_| "historical action reference missing")?
            != HISTORICAL_REFERENCE.as_bytes()
        || digest(REFERENCE.as_bytes()) != REFERENCE_SHA256
        || fs::read(
            root.join("xtask/tests/fixtures/capture/actions-reference-rules-promise-v3.json"),
        )
        .map_err(|_| "current action reference missing")?
            != REFERENCE.as_bytes()
        || receipt["reference_sha256"] != REFERENCE_SHA256
        || receipt["reference_revision"] != "rules-promise-v3"
        || receipt["historical_reference_sha256"] != HISTORICAL_REFERENCE_SHA256
        || receipt["observer_sha256"] != digest(OBSERVER.as_bytes())
    {
        return Err("fixture or observer identity differs".into());
    }
    let runs = receipt["executions"]
        .as_array()
        .ok_or("missing executions")?;
    if runs.len() != 2 || runs[0]["pid"] == runs[1]["pid"] {
        return Err("two fresh execution identities required".into());
    }
    for run in runs {
        if run["pid"].as_u64().filter(|pid| *pid > 0).is_none()
            || run["exit"] != 0
            || run["stderr"] != ""
            || run["writes"] != json!({})
            || run["removed"] != json!([])
        {
            return Err("execution, exit, stderr or retained change contract failed".into());
        }
        let argv = run["argv"].as_array().ok_or("missing observed argv")?;
        if argv.len() != 6 || argv[1..] != *fixture["argv"].as_array().unwrap() {
            return Err("observed argv differs".into());
        }
        if serde_json::to_string(run)
            .map_err(|_| "invalid execution record")?
            .contains(fixture["canary"].as_str().unwrap())
        {
            return Err("capture canary leaked".into());
        }
        let text = run["stdout"].as_str().ok_or("missing observed stdout")?;
        validate_schema(text)?;
        if text != REFERENCE {
            return Err("action output differs from frozen reference".into());
        }
        validate_observer(&run["observer"], &root, &receipt["sources_before"])?;
    }
    for key in [
        "argv",
        "stdout",
        "stderr",
        "exit",
        "writes",
        "removed",
        "environment_keys",
        "observer",
    ] {
        if runs[0][key] != runs[1][key] {
            return Err("fresh executions disagree".into());
        }
    }
    if receipt["capture_executable_sha256"]
        .as_str()
        .is_none_or(|hash| hash.len() != 64 || !hash.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        return Err("capture executable identity missing".into());
    }
    if !receipt["sources_before"].is_object()
        || receipt["sources_before"].as_object().unwrap().is_empty()
        || receipt["sources_before"] != receipt["sources_after"]
        || receipt["python_before"] != receipt["python_after"]
        || !receipt["python_before"]["sha256"].is_string()
        || !receipt["python_before"]["version"].is_string()
    {
        return Err("source or interpreter changed".into());
    }
    Ok(())
}

fn capture_mode(
    root: &Path,
    python: &Path,
    fault: &str,
    descendant: &Path,
) -> Result<Value, String> {
    if !python.is_absolute() || !python.is_file() {
        return Err("Python must be an absolute executable path".into());
    }
    let python_binary = python
        .canonicalize()
        .map_err(|_| "Python executable missing")?;
    let identity = || -> Result<Value, String> {
        Ok(
            json!({"sha256": digest(&fs::read(&python_binary).map_err(|_| "cannot hash Python")?), "version": version(python)?}),
        )
    };
    let source_before = sources(root)?;
    let python_before = identity()?;
    let fixture: Value = serde_json::from_str(CASE).map_err(|_| "invalid fixture")?;
    let first = observe(root, python, &fixture, fault, descendant)?;
    let second = observe(root, python, &fixture, fault, descendant)?;
    let mut revision = Command::new("/usr/bin/git");
    revision
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .env_clear();
    let (_, revision_exit, source_revision, revision_error) = execute(revision)?;
    if revision_exit != Some(0) || !revision_error.is_empty() {
        return Err("cannot observe source revision".into());
    }
    let observer_executable = std::env::current_exe().map_err(|_| "capture executable missing")?;
    let receipt = json!({
        "schema": fixture["schema"], "scope": "Python CLI action discovery",
        "source_revision": source_revision.trim(),
        "capture_executable_sha256": digest(&fs::read(observer_executable).map_err(|_| "cannot hash capture executable")?),
        "fixture_sha256": digest(CASE.as_bytes()),
        "reference_sha256": REFERENCE_SHA256, "reference_revision":"rules-promise-v3", "historical_reference_sha256":HISTORICAL_REFERENCE_SHA256, "observer_sha256": digest(OBSERVER.as_bytes()),
        "sources_before": source_before, "sources_after": sources(root)?,
        "python_before": python_before, "python_after": identity()?,
        "executions": [first, second],
    });
    validate(&receipt)?;
    Ok(receipt)
}

pub fn capture(root: &Path, python: &Path) -> Result<Value, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create capture control")?;
    capture_mode(root, python, "none", &scratch.path().join("descendant"))
}

pub fn run(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    if output.exists() {
        return Err("capture output already exists".into());
    }
    let receipt = capture(root, python)?;
    write_receipt(&receipt, output)
}

pub(crate) fn write_receipt(receipt: &Value, output: &Path) -> Result<(), String> {
    let parent = output
        .parent()
        .ok_or("capture output needs a parent directory")?;
    let mut temporary =
        tempfile::NamedTempFile::new_in(parent).map_err(|_| "cannot create receipt")?;
    serde_json::to_writer_pretty(&mut temporary, &receipt).map_err(|_| "cannot encode receipt")?;
    temporary
        .write_all(b"\n")
        .map_err(|_| "cannot finish receipt")?;
    temporary
        .as_file()
        .sync_all()
        .map_err(|_| "cannot sync receipt")?;
    temporary
        .persist_noclobber(output)
        .map_err(|_| "cannot publish new receipt")?;
    Ok(())
}
