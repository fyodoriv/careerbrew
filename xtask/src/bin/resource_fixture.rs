//! Synthetic child for the resource-accounting controls; never used in production.
//! Modes: `alloc <bytes>` touches every page, `burn <ms>` spins the CPU,
//! `sigkill` kills itself, `hold <bytes> <ms>` touches memory and keeps it,
//! `orphan <mode> <args>...` starts this binary as a grandchild in that mode
//! and exits at once without waiting for it.
//! `split <ms> <mode> <args>...` starts a grandchild in `detach` mode, waits
//! 100 ms and exits; `detach <ms> <mode> <args>...` starts a member in that
//! mode (still in the group), calls `setsid` to leave the group, sleeps
//! `<ms>` and exits without waiting for the member.
//! `escape <mode> <args>...` starts a grandchild that calls `setsid` before
//! it runs that mode, and exits at once without waiting for it.
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn touch(size: usize) -> Vec<u8> {
    let mut buffer = vec![0u8; size];
    for (index, byte) in buffer.iter_mut().enumerate() {
        *byte = (index % 251) as u8 | 1;
    }
    buffer
}

// The `orphan` mode leaves its grandchild unwaited on purpose.
#[allow(clippy::zombie_processes)]
fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .as_slice()
    {
        ["alloc", bytes] => {
            let size: usize = bytes.parse().expect("byte count");
            std::hint::black_box(&touch(size));
        }
        ["hold", bytes, millis] => {
            let buffer = touch(bytes.parse().expect("byte count"));
            std::thread::sleep(Duration::from_millis(millis.parse().expect("millis")));
            std::hint::black_box(&buffer);
        }
        ["orphan", rest @ ..] if !rest.is_empty() => {
            Command::new(std::env::current_exe().expect("own path"))
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("grandchild");
        }
        ["escape", rest @ ..] if !rest.is_empty() => {
            let mut command = Command::new(std::env::current_exe().expect("own path"));
            command
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            // SAFETY: setsid is async-signal-safe; it leaves the process group.
            unsafe {
                command.pre_exec(|| {
                    if libc::setsid() < 0 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                })
            };
            command.spawn().expect("grandchild");
        }
        ["split", millis, rest @ ..] if !rest.is_empty() => {
            Command::new(std::env::current_exe().expect("own path"))
                .arg("detach")
                .arg(millis)
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("grandchild");
            std::thread::sleep(Duration::from_millis(100));
        }
        ["detach", millis, rest @ ..] if !rest.is_empty() => {
            Command::new(std::env::current_exe().expect("own path"))
                .args(rest)
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("member");
            // SAFETY: setsid only moves this process to a new session.
            if unsafe { libc::setsid() } < 0 {
                std::process::exit(3);
            }
            std::thread::sleep(Duration::from_millis(millis.parse().expect("millis")));
        }
        ["burn", millis] => {
            let until = Instant::now() + Duration::from_millis(millis.parse().expect("millis"));
            let mut value = 1u64;
            while Instant::now() < until {
                value =
                    std::hint::black_box(value.wrapping_mul(6364136223846793005).wrapping_add(1));
            }
        }
        ["sigkill"] => unsafe {
            libc::kill(libc::getpid(), libc::SIGKILL);
        },
        _ => std::process::exit(2),
    }
}
