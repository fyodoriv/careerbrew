//! Synthetic child for the resource-accounting controls; never used in production.
//! Modes: `alloc <bytes>` touches every page, `burn <ms>` spins the CPU,
//! `sigkill` kills itself.
use std::time::{Duration, Instant};

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
            let mut buffer = vec![0u8; size];
            for (index, byte) in buffer.iter_mut().enumerate() {
                *byte = (index % 251) as u8 | 1;
            }
            std::hint::black_box(&buffer);
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
