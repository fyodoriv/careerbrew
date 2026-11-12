//! Fixed synthetic worker ownership experiment; never used for production.
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::ExitStatusExt;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn nonblocking(fd: libc::c_int) -> Result<(), ()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        return Err(());
    }
    Ok(())
}

fn ready(fd: libc::c_int, events: libc::c_short, deadline: Instant) -> Result<(), ()> {
    loop {
        let left = deadline.checked_duration_since(Instant::now()).ok_or(())?;
        let millis = left.as_millis().saturating_add(1).min(i32::MAX as u128) as i32;
        let mut descriptor = libc::pollfd {
            fd,
            events,
            revents: 0,
        };
        let result = unsafe { libc::poll(&mut descriptor, 1, millis) };
        if result > 0 && descriptor.revents & libc::POLLNVAL == 0 {
            return Ok(());
        }
        if result == 0 {
            continue;
        }
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(());
    }
}

fn put<W: Write + AsRawFd>(stream: &mut W, byte: u8, deadline: Instant) -> Result<(), ()> {
    loop {
        ready(stream.as_raw_fd(), libc::POLLOUT, deadline)?;
        match stream.write(&[byte]) {
            Ok(1) => return Ok(()),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            _ => return Err(()),
        }
    }
}

fn get<R: Read + AsRawFd>(stream: &mut R, deadline: Instant) -> Result<Option<u8>, ()> {
    loop {
        ready(stream.as_raw_fd(), libc::POLLIN, deadline)?;
        let mut byte = [0];
        match stream.read(&mut byte) {
            Ok(0) => return Ok(None),
            Ok(1) => return Ok(Some(byte[0])),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::Interrupted | std::io::ErrorKind::WouldBlock
                ) =>
            {
                continue
            }
            _ => return Err(()),
        }
    }
}

fn verify_sigchld() -> Result<(), ()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut action) } != 0
        || action.sa_sigaction != libc::SIG_DFL
        || action.sa_flags & libc::SA_NOCLDWAIT != 0
    {
        return Err(());
    }
    Ok(())
}

fn wait_once(pid: libc::pid_t, deadline: Instant) -> Result<Option<ExitStatus>, ()> {
    loop {
        if Instant::now() >= deadline {
            return Err(());
        }
        let mut status = 0;
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        if result == pid {
            return Ok(Some(ExitStatus::from_raw(status)));
        }
        if result == 0 {
            return Ok(None);
        }
        if result < 0 && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted {
            continue;
        }
        return Err(());
    }
}

fn wait_to_exit(pid: libc::pid_t, deadline: Instant) -> Result<ExitStatus, ()> {
    loop {
        if let Some(status) = wait_once(pid, deadline)? {
            return Ok(status);
        }
        // No descriptors are polled here; this wait is bounded by the same deadline.
        let until = deadline.checked_duration_since(Instant::now()).ok_or(())?;
        let timeout = until.as_millis().min(10) as i32;
        let rc = unsafe { libc::poll(std::ptr::null_mut(), 0, timeout) };
        if rc < 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
            return Err(());
        }
    }
}

fn worker() -> Result<(), ()> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let input_fd = unsafe { libc::dup(libc::STDIN_FILENO) };
    if input_fd < 0 {
        return Err(());
    }
    let output_fd = unsafe { libc::dup(libc::STDOUT_FILENO) };
    if output_fd < 0 {
        unsafe { libc::close(input_fd) };
        return Err(());
    }
    // Each File owns one duplicate, with no user-space buffering.
    let mut input = unsafe { std::fs::File::from_raw_fd(input_fd) };
    let mut output = unsafe { std::fs::File::from_raw_fd(output_fd) };
    nonblocking(input.as_raw_fd())?;
    nonblocking(output.as_raw_fd())?;
    put(&mut output, b'R', deadline)?;
    match get(&mut input, deadline)? {
        Some(b'G') | None => Ok(()),
        _ => Err(()),
    }
}

fn experiment(child: &mut Child, work_deadline: Instant, reaped: &mut bool) -> Result<(), ()> {
    let pid = i32::try_from(child.id()).map_err(|_| ())?;
    let input = child.stdin.as_mut().ok_or(())?;
    let output = child.stdout.as_mut().ok_or(())?;
    nonblocking(input.as_raw_fd())?;
    nonblocking(output.as_raw_fd())?;
    if get(output, work_deadline)? != Some(b'R') {
        return Err(());
    }
    if wait_once(pid, work_deadline)?.is_some() {
        *reaped = true;
        return Err(());
    }
    put(input, b'G', work_deadline)?;
    let status = wait_to_exit(pid, work_deadline)?;
    *reaped = true;
    if status.code() != Some(0) {
        return Err(());
    }
    let mut status = 0;
    if unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) } != -1
        || std::io::Error::last_os_error().raw_os_error() != Some(libc::ECHILD)
    {
        return Err(());
    }
    Ok(())
}

fn preflight_and_start<T>(bypass: bool, start: impl FnOnce() -> Result<T, ()>) -> Result<T, ()> {
    if !bypass {
        verify_sigchld()?;
    }
    start()
}

extern "C" fn synthetic_sigchld_handler(_: libc::c_int) {}

fn signal_control(mode: &str) -> Result<(), ()> {
    let mut action: libc::sigaction = unsafe { std::mem::zeroed() };
    action.sa_sigaction = match mode {
        "sigchld-ignored" | "sigchld-bypass" => libc::SIG_IGN,
        "sigchld-handler" => synthetic_sigchld_handler as *const () as libc::sighandler_t,
        "sigchld-no-cldwait" => libc::SIG_DFL,
        _ => return Err(()),
    };
    if mode == "sigchld-no-cldwait" {
        action.sa_flags = libc::SA_NOCLDWAIT;
    }
    if unsafe { libc::sigemptyset(&mut action.sa_mask) } != 0
        || unsafe { libc::sigaction(libc::SIGCHLD, &action, std::ptr::null_mut()) } != 0
    {
        eprintln!("SYNTHETIC_SIGNAL_INSTALL_FAILED");
        return Err(());
    }
    let mut observed: libc::sigaction = unsafe { std::mem::zeroed() };
    if unsafe { libc::sigaction(libc::SIGCHLD, std::ptr::null(), &mut observed) } != 0 {
        eprintln!("SYNTHETIC_SIGNAL_READBACK_FAILED");
        return Err(());
    }
    if observed.sa_sigaction != action.sa_sigaction {
        eprintln!("SYNTHETIC_SIGNAL_HANDLER_MISMATCH");
        return Err(());
    }
    if mode == "sigchld-no-cldwait" && observed.sa_flags & libc::SA_NOCLDWAIT == 0 {
        eprintln!("SYNTHETIC_SIGNAL_FLAGS_MISMATCH");
        return Err(());
    }
    let starts = std::cell::Cell::new(0);
    let result: Result<(), ()> = preflight_and_start(mode == "sigchld-bypass", || {
        starts.set(starts.get() + 1);
        Err(())
    });
    let zero_start = starts.get() == 0 && result.is_err();
    if mode == "sigchld-bypass" && !zero_start && starts.get() == 1 && result.is_err() {
        println!("SYNTHETIC_START_DETECTOR_REJECTED");
        return Ok(());
    }
    if mode != "sigchld-bypass" && zero_start {
        println!("SYNTHETIC_SIGCHLD_REFUSED_BEFORE_SPAWN");
        return Ok(());
    }
    if mode != "sigchld-bypass" && starts.get() == 1 {
        eprintln!("SYNTHETIC_START_ATTEMPTED");
    }
    Err(())
}

fn supervisor() -> Result<(), ()> {
    let start = Instant::now();
    let overall = start + Duration::from_secs(8);
    let work = start + Duration::from_secs(6);
    let mut child = preflight_and_start(false, || {
        Command::new(std::env::current_exe().map_err(|_| ())?)
            .arg("worker")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| ())
    })?;
    let mut reaped = false;
    let result = experiment(&mut child, work, &mut reaped);
    drop(child.stdin.take());
    drop(child.stdout.take());
    if result.is_err() && !reaped {
        let cleanup = overall.min(Instant::now() + Duration::from_secs(2));
        // An error or deadline leaves cleanup unconfirmed; no signal is sent.
        let _ = wait_to_exit(child.id() as libc::pid_t, cleanup);
    }
    result
}

fn main() {
    // The fixture owns this process-wide disposition; the caller is unaffected.
    if unsafe { libc::signal(libc::SIGPIPE, libc::SIG_IGN) } == libc::SIG_ERR {
        std::process::exit(1);
    }
    let result = match std::env::args().nth(1).as_deref() {
        Some("supervisor") => supervisor().map(|()| println!("SYNTHETIC_LIVE_WAIT_CONFIRMED")),
        Some("worker") => worker(),
        Some(
            mode
            @ ("sigchld-ignored" | "sigchld-handler" | "sigchld-no-cldwait" | "sigchld-bypass"),
        ) => signal_control(mode),
        _ => Err(()),
    };
    if result.is_err() {
        std::process::exit(1);
    }
}
