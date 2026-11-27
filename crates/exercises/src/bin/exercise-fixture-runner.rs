//! Synthetic language runner for the rust-22 differential gate.
//!
//! It speaks the learner-harness protocol (payload JSON on stdin, one JSON
//! result line on stdout) but runs no learner code: `payload.code` names a
//! fixed behavior. Solutions spend a calibrated amount of process CPU time
//! per call (linear, quadratic or cubic in `n`), so the scaling decision is
//! the same in debug and release builds and on a loaded machine. Both the
//! Python oracle and the Rust candidate start this same program.

use serde_json::{json, Value};
use std::io::Read;
use std::time::Duration;

fn cpu_ms() -> f64 {
    let mut now = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes one timespec we own.
    unsafe {
        libc::clock_gettime(libc::CLOCK_PROCESS_CPUTIME_ID, &mut now);
    }
    now.tv_sec as f64 * 1000.0 + now.tv_nsec as f64 / 1_000_000.0
}

fn burn_us(micros: f64) {
    let until = cpu_ms() + micros / 1000.0;
    let mut spin = 0_u64;
    while cpu_ms() < until {
        spin = std::hint::black_box(spin.wrapping_add(1));
    }
}

fn emit(value: &Value) -> ! {
    println!("{value}");
    std::process::exit(0);
}

fn cost_us(code: &str, n: f64) -> f64 {
    match code {
        "quadratic" => 40.0 * n * n,
        "cubic" => 2.0 * n * n * n,
        _ => 1600.0 * n,
    }
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn probe() -> String {
    let cwd = std::env::current_dir().unwrap_or_default();
    let prefix = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with("cb-run-"));
    let empty = std::fs::read_dir(&cwd).is_ok_and(|mut entries| entries.next().is_none());
    let mut keys: Vec<String> = std::env::vars_os()
        .map(|(key, _)| key.to_string_lossy().into_owned())
        .collect();
    keys.sort();
    let seed = std::env::var("PYTHONHASHSEED").unwrap_or_default();
    let mut cpu = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    let mut memory = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: getrlimit writes structs we own.
    unsafe {
        libc::getrlimit(libc::RLIMIT_CPU, &mut cpu);
        libc::getrlimit(libc::RLIMIT_AS, &mut memory);
    }
    let memory_limited = memory.rlim_cur == 768 * 1024 * 1024;
    let memory_unlimited = memory.rlim_cur == libc::RLIM_INFINITY;
    format!(
        "cwd_prefix={prefix};cwd_empty={empty};env={};hashseed={seed};cpu={}/{};as={}",
        keys.join(","),
        cpu.rlim_cur,
        cpu.rlim_max,
        if memory_limited {
            "limited"
        } else if memory_unlimited {
            "unlimited"
        } else {
            "other"
        }
    )
}

fn tests(code: &str, payload: &Value) -> Vec<Value> {
    let rows = payload["tests"].as_array().cloned().unwrap_or_default();
    rows.iter()
        .enumerate()
        .map(|(i, test)| {
            let nums: Vec<i64> = test["args"][0]
                .as_array()
                .map(|items| items.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            if code == "probe" {
                return json!({"i": i, "ok": true, "got": probe(), "ms": 0.01});
            }
            if code == "raises" && nums.is_empty() {
                return json!({"i": i, "ok": false, "error": "ValueError: synthetic failure", "line": 2, "ms": 0.01});
            }
            let mut got: i64 = nums.iter().sum();
            if code == "wrong" {
                got += 1;
            }
            let ok = test["expected"].as_i64() == Some(got);
            json!({"i": i, "ok": ok, "got": got.to_string(), "ms": 0.01})
        })
        .collect()
}

fn scale(code: &str, spec: &Value) -> Vec<Value> {
    if code == "scaleexit" {
        std::process::exit(152);
    }
    let sizes: Vec<i64> = spec["sizes"]
        .as_array()
        .map(|items| items.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default();
    let repeats = spec["repeats"].as_u64().unwrap_or(1) as usize;
    let mut out = Vec::new();
    for (index, n) in sizes.iter().enumerate() {
        if code == "scalecrash" && index == 1 {
            out.push(json!({"n": n, "error": "ValueError"}));
            break;
        }
        let mut runs: Vec<f64> = Vec::new();
        let mut k = 1_usize;
        while runs.len() < repeats {
            let start = cpu_ms();
            for _ in 0..k {
                burn_us(cost_us(code, *n as f64));
            }
            runs.push((cpu_ms() - start) / k as f64);
            let last = runs[runs.len() - 1];
            if k == 1 && last < 20.0 {
                k = 50.min((20.0 / last.max(0.5)) as usize + 1);
                runs.clear();
            }
        }
        let fastest = runs.iter().copied().fold(f64::INFINITY, f64::min);
        out.push(json!({"n": n, "ms": round2(fastest), "runs": runs.iter().map(|ms| round2(*ms)).collect::<Vec<_>>()}));
    }
    out
}

fn main() {
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let payload: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    let code = payload["code"].as_str().unwrap_or_default().to_owned();
    match code.as_str() {
        "spin" => {
            // Pure user-space work: a loop of clock reads is mostly system
            // time, which macOS does not stop at the CPU limit promptly.
            let mut spin = 0_u64;
            loop {
                spin = std::hint::black_box(spin.wrapping_add(1));
            }
        }
        "sleep" => loop {
            std::thread::sleep(Duration::from_secs(60));
        },
        "crash" => {
            eprintln!("EXERCISECANARY private detail on an early stderr line");
            eprintln!("RuntimeError: synthetic crash");
            std::process::exit(3);
        }
        "garbage" => {
            println!("hello from the synthetic runner");
            std::process::exit(0);
        }
        "list" => {
            println!("[1, 2]");
            std::process::exit(0);
        }
        "term" => {
            // SAFETY: the process signals itself.
            unsafe {
                libc::kill(libc::getpid(), libc::SIGTERM);
            }
            std::thread::sleep(Duration::from_secs(5));
            std::process::exit(0);
        }
        "exit152" => std::process::exit(152),
        "syntax" => {
            emit(&json!({"error": "SyntaxError: invalid syntax (solution.py, line 1)", "line": 1}))
        }
        "linear" | "quadratic" | "cubic" | "wrong" | "raises" | "probe" | "scalecrash"
        | "scaleexit" => {
            let results = tests(&code, &payload);
            let rows = if payload["scale"].is_object() {
                scale(&code, &payload["scale"])
            } else {
                Vec::new()
            };
            emit(&json!({"results": results, "scale": rows}));
        }
        _ => emit(&json!({"error": "define total (see the signature)"})),
    }
}
