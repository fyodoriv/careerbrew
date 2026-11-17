//! Synthetic child for the resource-accounting controls; never used in production.
//! Modes: `alloc <bytes>` touches every page, `burn <ms>` spins the CPU,
//! `sigkill` kills itself, `hold <bytes> <ms>` touches memory and keeps it,
//! `orphan <mode> <args>...` starts this binary as a grandchild in that mode
//! and exits at once without waiting for it.
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
