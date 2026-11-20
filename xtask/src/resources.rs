//! Owned process-tree resource accounting: spawn one child in its own
//! process group, reap it with `wait4`, and report its CPU time and peak RSS
//! in bytes. Descendants in the same process group are counted too: on Linux
//! the measuring process becomes a child subreaper and reaps them with
//! `wait4(-pgid)`; on macOS they are sampled with `proc_pid_rusage` until the
//! group is empty. A descendant that leaves the group (`setsid`, `setpgid`)
//! is not counted, and a descendant that never exits keeps the call waiting.
//! On Linux a member whose parent left the group is waited for until that
//! parent exits and the member comes back to the subreaper. If that parent
//! never exits (a daemon), the call polls every 2 ms and never returns.
//! The parent that left the group is reaped (no zombie stays) but not counted.
//! On Linux with cgroup v2, each call also tries to start the child in its own
//! new cgroup under the measuring process's cgroup. When that works, the tree
//! CPU fields take the larger of the group sum and the cgroup's `cpu.stat`,
//! so a descendant that leaves the group (`setsid`) is counted too, and the
//! call waits until the cgroup is empty. When the cgroup cannot be created or
//! joined (no delegation), the call falls back to the group method alone, and
//! `tree_method` says which method ran. Peak RSS stays per process.
//! Each call owns its child, process group and cgroup for the call's lifetime
//! and keeps no state between calls. The Linux subreaper flag is process-scope
//! state of the measuring process: it is set once and stays set.
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Instant;

/// The unit the kernel uses for `ru_maxrss`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[allow(dead_code)]
pub enum RssUnit {
    Bytes,
    Kibibytes,
}

impl RssUnit {
    pub fn name(self) -> &'static str {
        match self {
            RssUnit::Bytes => "bytes",
            RssUnit::Kibibytes => "KiB",
        }
    }

    pub fn to_bytes(self, raw: u64) -> u64 {
        match self {
            RssUnit::Bytes => raw,
            RssUnit::Kibibytes => raw.saturating_mul(1024),
        }
    }

    #[allow(dead_code)]
    pub fn other(self) -> RssUnit {
        match self {
            RssUnit::Bytes => RssUnit::Kibibytes,
            RssUnit::Kibibytes => RssUnit::Bytes,
        }
    }
}

/// The `ru_maxrss` unit on this host. An unknown OS is refused.
#[cfg(target_os = "macos")]
pub fn host_unit() -> Result<(&'static str, RssUnit), String> {
    Ok(("macos", RssUnit::Bytes))
}

/// The `ru_maxrss` unit on this host. An unknown OS is refused.
#[cfg(target_os = "linux")]
pub fn host_unit() -> Result<(&'static str, RssUnit), String> {
    Ok(("linux", RssUnit::Kibibytes))
}

/// The `ru_maxrss` unit on this host. An unknown OS is refused.
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn host_unit() -> Result<(&'static str, RssUnit), String> {
    Err("unsupported OS: ru_maxrss unit is not calibrated here".to_string())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Exited(i32),
    Signaled(i32),
}

#[derive(Debug, Clone)]
pub struct Usage {
    pub os: &'static str,
    pub unit: RssUnit,
    pub outcome: Outcome,
    pub user_cpu_us: u64,
    pub system_cpu_us: u64,
    pub max_rss_raw: u64,
    pub max_rss_bytes: u64,
    pub wall_us: u64,
    /// The measuring process's own peak RSS when it spawned the child.
    pub parent_max_rss_bytes: u64,
    /// Descendants counted beyond the direct child.
    pub descendants: u64,
    /// CPU time of the child plus its counted descendants.
    pub tree_user_cpu_us: u64,
    pub tree_system_cpu_us: u64,
    /// The largest peak of any one process in the tree, in bytes.
    pub tree_max_rss_bytes: u64,
    pub tree_method: &'static str,
}

impl Usage {
    /// On Linux, exec can fold the spawning process's high-water RSS into the
    /// child's `ru_maxrss`. A child peak at or below the parent's own peak may
    /// be that floor, not the child's memory.
    pub fn rss_may_include_parent(&self) -> bool {
        self.os == "linux" && self.max_rss_bytes <= self.parent_max_rss_bytes
    }

    pub fn passed(&self) -> bool {
        self.outcome == Outcome::Exited(0)
    }

    #[allow(dead_code)]
    pub fn to_json(&self) -> String {
        let (kind, value) = match self.outcome {
            Outcome::Exited(code) => ("exited", code),
            Outcome::Signaled(signal) => ("signaled", signal),
        };
        let field = if kind == "exited" { "code" } else { "signal" };
        format!(
            concat!(
                "{{\"scope\":\"process-tree\",\"tree_method\":\"{}\",",
                "\"tree_rss_metric\":\"{}\",\"descendants\":{},",
                "\"tree_user_cpu_us\":{},\"tree_system_cpu_us\":{},\"tree_max_rss_bytes\":{},",
                "\"os\":\"{}\",\"rss_unit_raw\":\"{}\",\"outcome\":\"{}\",\"{}\":{},",
                "\"pass\":{},\"user_cpu_us\":{},\"system_cpu_us\":{},",
                "\"max_rss_raw\":{},\"max_rss_bytes\":{},\"wall_us\":{},",
                "\"parent_max_rss_bytes\":{},\"rss_may_include_parent\":{}}}"
            ),
            self.tree_method,
            TREE_RSS_METRIC,
            self.descendants,
            self.tree_user_cpu_us,
            self.tree_system_cpu_us,
            self.tree_max_rss_bytes,
            self.os,
            self.unit.name(),
            kind,
            field,
            value,
            self.passed(),
            self.user_cpu_us,
            self.system_cpu_us,
            self.max_rss_raw,
            self.max_rss_bytes,
            self.wall_us,
            self.parent_max_rss_bytes,
            self.rss_may_include_parent()
        )
    }
}

/// Refuse a malformed `rusage`: negative or out-of-range time, negative RSS.
pub fn validate_rusage(usage: &libc::rusage) -> Result<(), String> {
    for (name, time) in [("ru_utime", usage.ru_utime), ("ru_stime", usage.ru_stime)] {
        if time.tv_sec < 0 || time.tv_usec < 0 || time.tv_usec >= 1_000_000 {
            return Err(format!("{name} is out of range"));
        }
    }
    if usage.ru_maxrss < 0 {
        return Err("ru_maxrss is negative".to_string());
    }
    Ok(())
}

fn micros(time: libc::timeval) -> u64 {
    (time.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(time.tv_usec as u64)
}

/// What the descendants of the child used, beyond the child itself.
#[derive(Default)]
struct Tree {
    count: u64,
    user_cpu_us: u64,
    system_cpu_us: u64,
    max_rss_bytes: u64,
}

#[cfg(target_os = "linux")]
const TREE_METHOD: &str = "subreaper-wait4";
#[cfg(target_os = "linux")]
const TREE_METHOD_CGROUP: &str = "cgroup-v2-cpu+subreaper-wait4";
#[cfg(target_os = "macos")]
const TREE_METHOD: &str = "sampled-proc-pid-rusage";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const TREE_METHOD: &str = "unsupported";

/// What `tree_max_rss_bytes` compares. macOS has no lifetime resident peak
/// for another process, so a descendant's part is its sampled phys footprint
/// or resident size, while the child's part stays `ru_maxrss` (resident).
#[cfg(target_os = "macos")]
pub const TREE_RSS_METRIC: &str =
    "child=ru_maxrss;descendants=max(lifetime_phys_footprint,resident_size)";
#[cfg(target_os = "linux")]
pub const TREE_RSS_METRIC: &str = "child=ru_maxrss;descendants=ru_maxrss";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub const TREE_RSS_METRIC: &str = "unsupported";

/// Reap `pid` with `wait4`, retrying on EINTR. `None` means still running.
type Reaped = (libc::c_int, libc::rusage);
fn reap(pid: libc::pid_t, options: libc::c_int) -> Result<Option<Reaped>, String> {
    let mut status: libc::c_int = 0;
    // SAFETY: rusage is plain data; wait4 fills it for our own child.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    loop {
        let reaped = unsafe { libc::wait4(pid, &mut status, options, &mut usage) };
        if reaped == pid {
            return Ok(Some((status, usage)));
        }
        if reaped == 0 {
            return Ok(None);
        }
        let error = std::io::Error::last_os_error();
        if reaped < 0 && error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(format!("wait4 failed: {}", error.kind()));
    }
}

/// Linux: make this process the child subreaper, so orphans of the child are
/// reparented here instead of to init.
#[cfg(target_os = "linux")]
fn prepare_tree() -> Result<(), String> {
    // SAFETY: PR_SET_CHILD_SUBREAPER only sets a flag on this process.
    let on: libc::c_ulong = 1;
    let set = unsafe { libc::prctl(libc::PR_SET_CHILD_SUBREAPER, on, 0, 0, 0) };
    if set != 0 {
        return Err("prctl(PR_SET_CHILD_SUBREAPER) failed".to_string());
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn prepare_tree() -> Result<(), String> {
    Ok(())
}

/// Linux: the child is reaped, so its orphans are ours now (subreaper).
/// Reap every member of its process group until none is left.
#[cfg(target_os = "linux")]
fn collect_tree(pgid: libc::pid_t, unit: RssUnit) -> Result<Tree, String> {
    let mut tree = Tree::default();
    loop {
        let mut status: libc::c_int = 0;
        // SAFETY: rusage is plain data; wait4 fills it for a reaped descendant.
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let reaped = unsafe { libc::wait4(-pgid, &mut status, 0, &mut usage) };
        if reaped > 0 {
            validate_rusage(&usage)?;
            tree.count += 1;
            tree.user_cpu_us = tree.user_cpu_us.saturating_add(micros(usage.ru_utime));
            tree.system_cpu_us = tree.system_cpu_us.saturating_add(micros(usage.ru_stime));
            tree.max_rss_bytes = tree
                .max_rss_bytes
                .max(unit.to_bytes(usage.ru_maxrss as u64));
            continue;
        }
        let error = std::io::Error::last_os_error();
        match error.raw_os_error() {
            Some(libc::EINTR) => continue,
            // No child of ours is left in the group. A member whose parent
            // left the group (`setsid`) is not our child yet: it comes back
            // to us when that parent exits. Wait while the group still has a
            // member; a failed signal-0 check (ESRCH) means it is empty.
            Some(libc::ECHILD) => {
                // A parent that left the group was reparented to us; reap it
                // so no zombie stays. It is not a member, so it is not counted.
                reap_strays(pgid);
                // SAFETY: signal 0 only checks that the group has a member.
                if unsafe { libc::kill(-pgid, 0) } != 0 {
                    let gone = std::io::Error::last_os_error();
                    if gone.raw_os_error() == Some(libc::ESRCH) {
                        return Ok(tree);
                    }
                    return Err(format!(
                        "kill(-pgid, 0) on the process group failed: {}",
                        gone.kind()
                    ));
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
            _ => {
                return Err(format!(
                    "wait4 on the process group failed: {}",
                    error.kind()
                ))
            }
        }
    }
}

/// Linux: reap, without blocking, every exited child of ours outside the
/// group (a parent that called `setsid`). Their usage is not counted; members
/// are only peeked (`WNOWAIT`) and stay for `wait4(-pgid)`.
#[cfg(target_os = "linux")]
fn reap_strays(pgid: libc::pid_t) {
    loop {
        // SAFETY: siginfo_t is plain data; waitid fills it. WNOWAIT only
        // peeks, so a group member is left for `wait4(-pgid)` to count.
        let mut info: libc::siginfo_t = unsafe { std::mem::zeroed() };
        let flags = libc::WEXITED | libc::WNOHANG | libc::WNOWAIT;
        let got = unsafe { libc::waitid(libc::P_ALL, 0, &mut info, flags) };
        if got != 0 {
            if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return;
        }
        // SAFETY: waitid filled si_pid for an exited child.
        let pid = unsafe { info.si_pid() };
        if pid <= 0 || unsafe { libc::getpgid(pid) } == pgid {
            return;
        }
        let mut status: libc::c_int = 0;
        // SAFETY: reaps the exited non-member peeked above.
        unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
    }
}

/// Linux: one cgroup v2 scope per call, a child of this process's cgroup.
/// It counts the CPU of every process started in it, also one that left the
/// process group. Memory needs the memory controller, which a cgroup that
/// holds processes cannot delegate, so only `cpu.stat` is read.
#[cfg(target_os = "linux")]
mod cgroup {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    const ROOT: &str = "/sys/fs/cgroup";
    static NEXT: AtomicU64 = AtomicU64::new(0);

    pub struct Scope {
        dir: PathBuf,
        procs: CString,
    }

    impl Scope {
        /// `None` when cgroup v2 is not mounted at `/sys/fs/cgroup` or this
        /// process may not create a cgroup under its own.
        pub fn create() -> Option<Scope> {
            if !Path::new(ROOT).join("cgroup.controllers").is_file() {
                return None;
            }
            let text = std::fs::read_to_string("/proc/self/cgroup").ok()?;
            let own = text.lines().find_map(|line| line.strip_prefix("0::"))?;
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = Path::new(ROOT)
                .join(own.trim_start_matches('/'))
                .join(format!("careerbrew-measure-{}-{n}", std::process::id()));
            std::fs::create_dir(&dir).ok()?;
            let path = dir.join("cgroup.procs").into_os_string().into_vec();
            match CString::new(path) {
                Ok(procs) => Some(Scope { dir, procs }),
                Err(_) => {
                    let _ = std::fs::remove_dir(&dir);
                    None
                }
            }
        }

        /// The path the child writes `0` to, after fork and before exec.
        pub fn procs(&self) -> CString {
            self.procs.clone()
        }

        /// Wait until no process is left in the scope. Return every pid
        /// seen in it while waiting, so the caller reaps only those.
        pub fn wait_empty(&self) -> Result<Vec<libc::pid_t>, String> {
            let events = self.dir.join("cgroup.events");
            let procs = self.dir.join("cgroup.procs");
            let mut seen = Vec::new();
            loop {
                if let Ok(text) = std::fs::read_to_string(&procs) {
                    for pid in text.lines().filter_map(|line| line.trim().parse().ok()) {
                        if !seen.contains(&pid) {
                            seen.push(pid);
                        }
                    }
                }
                let text = std::fs::read_to_string(&events)
                    .map_err(|error| format!("read cgroup.events failed: {}", error.kind()))?;
                if text.lines().any(|line| line.trim() == "populated 0") {
                    return Ok(seen);
                }
                std::thread::sleep(std::time::Duration::from_millis(2));
            }
        }

        /// User and system CPU of every process that ran in the scope.
        pub fn cpu(&self) -> Result<(u64, u64), String> {
            let text = std::fs::read_to_string(self.dir.join("cpu.stat"))
                .map_err(|error| format!("read cpu.stat failed: {}", error.kind()))?;
            parse_cpu_stat(&text)
        }
    }

    impl Drop for Scope {
        fn drop(&mut self) {
            // An empty cgroup is removed with rmdir; a busy one stays.
            let _ = std::fs::remove_dir(&self.dir);
        }
    }

    /// Write `0` (this process) to `procs`. Runs between fork and exec, so it
    /// only calls `open`, `write` and `close`.
    pub fn join(procs: &CString) -> std::io::Result<()> {
        // SAFETY: async-signal-safe calls on a valid C string and buffer.
        unsafe {
            let fd = libc::open(procs.as_ptr(), libc::O_WRONLY | libc::O_CLOEXEC);
            if fd < 0 {
                return Err(std::io::Error::last_os_error());
            }
            let wrote = libc::write(fd, b"0".as_ptr().cast(), 1);
            let error = std::io::Error::last_os_error();
            libc::close(fd);
            if wrote != 1 {
                return Err(error);
            }
        }
        Ok(())
    }

    pub fn parse_cpu_stat(text: &str) -> Result<(u64, u64), String> {
        let value = |name: &str| {
            text.lines()
                .find_map(|line| line.strip_prefix(name)?.strip_prefix(' '))
                .and_then(|value| value.trim().parse::<u64>().ok())
                .ok_or_else(|| format!("cpu.stat has no {name}"))
        };
        Ok((value("user_usec")?, value("system_usec")?))
    }
}

/// Linux: reap, without blocking, each pid seen in the cgroup that is an
/// exited child of ours (a descendant that left the group came back to the
/// subreaper). Only these pids, so another call's child is never reaped.
#[cfg(target_os = "linux")]
fn reap_seen(pids: &[libc::pid_t]) {
    for &pid in pids {
        let mut status: libc::c_int = 0;
        // SAFETY: reaps `pid` only if it is an exited child of ours.
        while unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } < 0
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::EINTR)
        {}
    }
}

/// macOS: a descendant's peak, as `TREE_RSS_METRIC` names it.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn descendant_peak(phys_footprint: u64, resident_size: u64) -> u64 {
    phys_footprint.max(resident_size)
}

/// macOS has no subreaper, so each group member is sampled while it lives.
/// Sampling every 2 ms can miss the last CPU slice and a process that lives
/// shorter than one interval.
#[cfg(target_os = "macos")]
struct Sampler {
    numer: u64,
    denom: u64,
    // Keyed by pid and start time, so a reused pid is a new entry.
    seen: std::collections::HashMap<(libc::pid_t, u64), (u64, u64, u64)>,
}

#[cfg(target_os = "macos")]
impl Sampler {
    #[allow(deprecated)]
    fn new() -> Result<Self, String> {
        let mut info = libc::mach_timebase_info { numer: 0, denom: 0 };
        // SAFETY: mach_timebase_info fills a plain struct.
        let got = unsafe { libc::mach_timebase_info(&mut info) };
        if got != 0 || info.denom == 0 {
            return Err("mach_timebase_info failed".to_string());
        }
        Ok(Sampler {
            numer: info.numer as u64,
            denom: info.denom as u64,
            seen: Default::default(),
        })
    }

    /// `rusage_info` times are Mach absolute ticks, not nanoseconds.
    fn to_us(&self, ticks: u64) -> u64 {
        (ticks as u128 * self.numer as u128 / self.denom as u128 / 1000) as u64
    }

    /// Sample every group member except `skip`; return how many answered.
    fn sample(&mut self, pgid: libc::pid_t, skip: libc::pid_t) -> usize {
        let mut pids = vec![0 as libc::pid_t; 4096];
        let size = (pids.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: the buffer holds `size` bytes of pids.
        let count = unsafe { libc::proc_listpgrppids(pgid, pids.as_mut_ptr().cast(), size) };
        let count = (count.max(0) as usize).min(pids.len());
        let mut alive = 0;
        for &pid in pids[..count]
            .iter()
            .filter(|pid| **pid > 0 && **pid != skip)
        {
            // SAFETY: rusage_info_v4 is plain data; the kernel fills it.
            let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
            let buffer = (&mut info as *mut libc::rusage_info_v4).cast();
            if unsafe { libc::proc_pid_rusage(pid, libc::RUSAGE_INFO_V4, buffer) } != 0 {
                continue;
            }
            alive += 1;
            let peak = descendant_peak(info.ri_lifetime_max_phys_footprint, info.ri_resident_size);
            let entry = self
                .seen
                .entry((pid, info.ri_proc_start_abstime))
                .or_insert((0, 0, 0));
            entry.0 = entry.0.max(info.ri_user_time);
            entry.1 = entry.1.max(info.ri_system_time);
            entry.2 = entry.2.max(peak);
        }
        alive
    }

    fn finish(&self) -> Tree {
        let mut tree = Tree::default();
        for (user, system, peak) in self.seen.values() {
            tree.count += 1;
            tree.user_cpu_us = tree.user_cpu_us.saturating_add(self.to_us(*user));
            tree.system_cpu_us = tree.system_cpu_us.saturating_add(self.to_us(*system));
            tree.max_rss_bytes = tree.max_rss_bytes.max(*peak);
        }
        tree
    }
}

/// Run `program` with `args` as an owned child in its own process group,
/// reap it with `wait4`, and count the descendants left in that group.
pub fn measure(program: &Path, args: &[std::ffi::OsString]) -> Result<Usage, String> {
    measure_in(program, args, None, None)
}

/// Like `measure`, but with `environment` the child gets only those variables,
/// and with `current_dir` it starts in that folder.
pub fn measure_in(
    program: &Path,
    args: &[std::ffi::OsString],
    environment: Option<&std::collections::BTreeMap<String, String>>,
    current_dir: Option<&Path>,
) -> Result<Usage, String> {
    let (os, unit) = host_unit()?;
    if !program.is_absolute() {
        return Err("--cmd must be an absolute path".to_string());
    }
    // SAFETY: rusage is plain data; getrusage fills it for this process.
    let mut own: libc::rusage = unsafe { std::mem::zeroed() };
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut own) } != 0 {
        return Err("getrusage failed".to_string());
    }
    let parent_max_rss_bytes = unit.to_bytes(own.ru_maxrss.max(0) as u64);
    prepare_tree()?;
    let started = Instant::now();
    let build = || {
        let mut command = Command::new(program);
        if let Some(environment) = environment {
            command.env_clear().envs(environment);
        }
        if let Some(folder) = current_dir {
            command.current_dir(folder);
        }
        command
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        command
    };
    #[cfg(target_os = "linux")]
    let mut scope = cgroup::Scope::create();
    #[cfg(target_os = "linux")]
    let spawned = match scope.as_ref().map(cgroup::Scope::procs) {
        Some(procs) => {
            let mut command = build();
            // SAFETY: the hook only calls async-signal-safe functions.
            unsafe { command.pre_exec(move || cgroup::join(&procs)) };
            match command.spawn() {
                Ok(child) => Ok(child),
                // The child could not join the cgroup and never ran:
                // fall back to the group method alone.
                Err(_) => {
                    scope = None;
                    build().spawn()
                }
            }
        }
        None => build().spawn(),
    };
    #[cfg(not(target_os = "linux"))]
    let spawned = build().spawn();
    let child = spawned.map_err(|error| format!("spawn failed: {}", error.kind()))?;
    let pid = child.id() as libc::pid_t;
    #[cfg(target_os = "macos")]
    let (status, usage, tree) = {
        let mut sampler = Sampler::new()?;
        let (status, usage) = loop {
            sampler.sample(pid, pid);
            if let Some(done) = reap(pid, libc::WNOHANG)? {
                break done;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        };
        while sampler.sample(pid, pid) > 0 {
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        (status, usage, sampler.finish())
    };
    #[cfg(target_os = "linux")]
    let (status, usage, tree) = {
        let (status, usage) = reap(pid, 0)?.ok_or("wait4 returned no child")?;
        let mut tree = collect_tree(pid, unit)?;
        if let Some(found) = &scope {
            let seen = found.wait_empty()?;
            reap_seen(&seen);
            let (user, system) = found.cpu()?;
            // The cgroup counts the child too; the tree fields add it later.
            let user = user.saturating_sub(micros(usage.ru_utime));
            let system = system.saturating_sub(micros(usage.ru_stime));
            tree.user_cpu_us = tree.user_cpu_us.max(user);
            tree.system_cpu_us = tree.system_cpu_us.max(system);
        }
        (status, usage, tree)
    };
    #[cfg(target_os = "linux")]
    let tree_method = if scope.is_some() {
        TREE_METHOD_CGROUP
    } else {
        TREE_METHOD
    };
    #[cfg(not(target_os = "linux"))]
    let tree_method = TREE_METHOD;
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let (status, usage, tree) = {
        let (status, usage) = reap(pid, 0)?.ok_or("wait4 returned no child")?;
        (status, usage, Tree::default())
    };
    let wall_us = started.elapsed().as_micros().min(u64::MAX as u128) as u64;
    // The child is reaped; dropping the handle does not wait again.
    drop(child);
    let outcome = if libc::WIFEXITED(status) {
        Outcome::Exited(libc::WEXITSTATUS(status))
    } else if libc::WIFSIGNALED(status) {
        Outcome::Signaled(libc::WTERMSIG(status))
    } else {
        return Err("child ended in an unknown state".to_string());
    };
    validate_rusage(&usage)?;
    let max_rss_raw = usage.ru_maxrss as u64;
    let max_rss_bytes = unit.to_bytes(max_rss_raw);
    let user_cpu_us = micros(usage.ru_utime);
    let system_cpu_us = micros(usage.ru_stime);
    Ok(Usage {
        os,
        unit,
        outcome,
        user_cpu_us,
        system_cpu_us,
        max_rss_raw,
        max_rss_bytes,
        wall_us,
        parent_max_rss_bytes,
        descendants: tree.count,
        tree_user_cpu_us: user_cpu_us.saturating_add(tree.user_cpu_us),
        tree_system_cpu_us: system_cpu_us.saturating_add(tree.system_cpu_us),
        tree_max_rss_bytes: max_rss_bytes.max(tree.max_rss_bytes),
        tree_method,
    })
}

#[allow(dead_code)]
/// Check that a peak-RSS delta matches a known allocation: 0.75x to 1.5x.
pub fn check_calibration(
    idle_bytes: u64,
    loaded_bytes: u64,
    allocated: u64,
) -> Result<u64, String> {
    let delta = loaded_bytes.saturating_sub(idle_bytes);
    let low = allocated / 4 * 3;
    let high = allocated / 2 * 3;
    if delta < low || delta > high {
        return Err(format!(
            "RSS delta {delta} bytes is outside {low}..={high} for a {allocated}-byte allocation"
        ));
    }
    Ok(delta)
}

#[cfg(test)]
mod peak_tests {
    use super::descendant_peak;

    #[cfg(target_os = "linux")]
    #[test]
    fn cpu_stat_user_and_system_are_read_and_a_missing_field_is_refused() {
        let text = "usage_usec 30\nuser_usec 20\nsystem_usec 10\n";
        assert_eq!(super::cgroup::parse_cpu_stat(text), Ok((20, 10)));
        assert!(super::cgroup::parse_cpu_stat("usage_usec 30\n").is_err());
    }

    #[test]
    fn the_descendant_peak_is_the_larger_of_footprint_and_resident() {
        assert_eq!(descendant_peak(10, 3), 10);
        assert_eq!(descendant_peak(3, 10), 10);
    }
}
