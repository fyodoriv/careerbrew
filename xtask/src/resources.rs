//! Owned-child resource accounting: spawn one child, reap it with `wait4`,
//! and report its CPU time and peak RSS in bytes. Scope is the owned child
//! only; grandchildren are not qualified. Each call owns its child for the
//! call's lifetime and keeps no state between calls.
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
}

impl Usage {
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
                "{{\"scope\":\"owned-child\",\"grandchildren\":\"not-qualified\",",
                "\"os\":\"{}\",\"rss_unit_raw\":\"{}\",\"outcome\":\"{}\",\"{}\":{},",
                "\"pass\":{},\"user_cpu_us\":{},\"system_cpu_us\":{},",
                "\"max_rss_raw\":{},\"max_rss_bytes\":{},\"wall_us\":{}}}"
            ),
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
            self.wall_us
        )
    }
}

fn micros(time: libc::timeval) -> u64 {
    (time.tv_sec as u64)
        .saturating_mul(1_000_000)
        .saturating_add(time.tv_usec as u64)
}

/// Run `program` with `args` as an owned child and reap it with `wait4`.
pub fn measure(program: &Path, args: &[std::ffi::OsString]) -> Result<Usage, String> {
    let (os, unit) = host_unit()?;
    if !program.is_absolute() {
        return Err("--cmd must be an absolute path".to_string());
    }
    let started = Instant::now();
    let child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|error| format!("spawn failed: {}", error.kind()))?;
    let pid = child.id() as libc::pid_t;
    let mut status: libc::c_int = 0;
    // SAFETY: rusage is plain data; wait4 fills it for our own child.
    let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
    loop {
        let reaped = unsafe { libc::wait4(pid, &mut status, 0, &mut usage) };
        if reaped == pid {
            break;
        }
        let error = std::io::Error::last_os_error();
        if reaped < 0 && error.kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(format!("wait4 failed: {}", error.kind()));
    }
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
    let max_rss_raw = usage.ru_maxrss.max(0) as u64;
    Ok(Usage {
        os,
        unit,
        outcome,
        user_cpu_us: micros(usage.ru_utime),
        system_cpu_us: micros(usage.ru_stime),
        max_rss_raw,
        max_rss_bytes: unit.to_bytes(max_rss_raw),
        wall_us,
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
