//! Owned process-tree resource accounting: spawn one child in its own
//! process group, reap it with `wait4`, and report its CPU time and peak RSS
//! in bytes. Descendants in the same process group are counted too: on Linux
//! the measuring process becomes a child subreaper and reaps them with
//! `wait4(-pgid)`; on macOS they are sampled with `proc_pid_rusage` until the
//! group is empty. A descendant that leaves the group (`setsid`, `setpgid`)
//! is not counted, and a descendant that never exits keeps the call waiting.
//! Each call owns its child and process group for the call's lifetime and
//! keeps no state between calls. The Linux subreaper flag is process-scope
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
                "{{\"scope\":\"process-tree\",\"tree_method\":\"{}\",\"descendants\":{},",
                "\"tree_user_cpu_us\":{},\"tree_system_cpu_us\":{},\"tree_max_rss_bytes\":{},",
                "\"os\":\"{}\",\"rss_unit_raw\":\"{}\",\"outcome\":\"{}\",\"{}\":{},",
                "\"pass\":{},\"user_cpu_us\":{},\"system_cpu_us\":{},",
                "\"max_rss_raw\":{},\"max_rss_bytes\":{},\"wall_us\":{},",
                "\"parent_max_rss_bytes\":{},\"rss_may_include_parent\":{}}}"
            ),
            self.tree_method,
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
#[cfg(target_os = "macos")]
const TREE_METHOD: &str = "sampled-proc-pid-rusage";
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
const TREE_METHOD: &str = "unsupported";

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
            Some(libc::ECHILD) => return Ok(tree),
            _ => {
                return Err(format!(
                    "wait4 on the process group failed: {}",
                    error.kind()
                ))
            }
        }
    }
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
            let peak = info
                .ri_lifetime_max_phys_footprint
                .max(info.ri_resident_size);
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
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .map_err(|error| format!("spawn failed: {}", error.kind()))?;
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
        (status, usage, collect_tree(pid, unit)?)
    };
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
        tree_method: TREE_METHOD,
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
