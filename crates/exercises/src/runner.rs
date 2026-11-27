//! The sandboxed child lifecycle of one exercise run, the tests-then-scaling
//! run, and reference validation.
//!
//! Each run starts the language runner in a fresh empty temporary folder
//! (`cb-run-*`) with a cleared environment (`PATH` and `PYTHONHASHSEED=0`
//! only), CPU and address-space limits set in the child before `exec`, and a
//! wall-clock deadline after which the child is killed and reaped. The
//! payload goes on stdin; the last stdout line is the JSON result. Only the
//! last stderr line of a failed run is kept, so earlier stderr never leaves.

use crate::{decide, py};
use serde_json::{json, Map, Value};
use std::io::{Read, Write};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How to start one language's runner. The program reads the payload JSON on
/// stdin and prints one JSON result line (`results`, `scale`, or `error`).
#[derive(Clone, Debug)]
pub struct Language {
    pub program: PathBuf,
    pub args: Vec<String>,
}

fn set_limits(cpu: u64) -> std::io::Result<()> {
    let cpu_limit = libc::rlimit {
        rlim_cur: cpu as libc::rlim_t,
        rlim_max: cpu as libc::rlim_t,
    };
    // SAFETY: setrlimit is async-signal-safe and only reads the struct.
    if unsafe { libc::setrlimit(libc::RLIMIT_CPU, &cpu_limit) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    let memory = libc::rlimit {
        rlim_cur: decide::MEMORY_BYTES as libc::rlim_t,
        rlim_max: decide::MEMORY_BYTES as libc::rlim_t,
    };
    // SAFETY: as above; a refusal (macOS) is ignored like the Python runner does.
    unsafe {
        libc::setrlimit(libc::RLIMIT_AS, &memory);
    }
    Ok(())
}

fn annotate(
    mut result: Map<String, Value>,
    payload: &Value,
    category: Option<&str>,
    returncode: Option<i32>,
    wall: u64,
    cpu: u64,
) -> Value {
    result.remove("diagnostic");
    if let Some(category) = category {
        let stage = if py::truthy(payload.get("scale")) {
            "scale"
        } else {
            "tests"
        };
        result.insert(
            "diagnostic".into(),
            json!({
                "category": category,
                "stage": stage,
                "exit_code": returncode.filter(|code| *code >= 0),
                "signal": returncode.filter(|code| *code < 0).map(|code| -code),
                "wall_seconds": wall,
                "cpu_seconds": cpu,
                "memory_bytes": decide::MEMORY_BYTES,
            }),
        );
    }
    Value::Object(result)
}

fn failure(error: String, timeout: Option<bool>) -> Map<String, Value> {
    let mut out = Map::new();
    out.insert("error".into(), Value::String(error));
    out.insert("results".into(), json!([]));
    out.insert("scale".into(), json!([]));
    if let Some(timeout) = timeout {
        out.insert("timeout".into(), Value::Bool(timeout));
    }
    out
}

fn truncate(text: &str, limit: usize) -> String {
    text.chars().take(limit).collect()
}

struct Finished {
    returncode: i32,
    stdout: String,
    stderr: String,
}

fn spawn_and_wait(
    language: &Language,
    payload: &Value,
    wall: u64,
    cpu: u64,
) -> Result<Option<Finished>, String> {
    let folder = tempfile::Builder::new()
        .prefix("cb-run-")
        .tempdir()
        .map_err(|_| "cannot create the run folder")?;
    let mut command = Command::new(&language.program);
    command
        .args(&language.args)
        .current_dir(folder.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("PYTHONHASHSEED", "0")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: the closure only calls setrlimit between fork and exec.
    unsafe {
        command.pre_exec(move || set_limits(cpu));
    }
    let mut child = command.spawn().map_err(|_| "cannot start the runner")?;
    let input = serde_json::to_vec(payload).map_err(|_| "cannot encode the payload")?;
    let mut stdin = child.stdin.take().ok_or("the runner has no stdin")?;
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(&input);
    });
    let mut stdout = child.stdout.take().ok_or("the runner has no stdout")?;
    let mut stderr = child.stderr.take().ok_or("the runner has no stderr")?;
    let out_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout.read_to_end(&mut buffer);
        buffer
    });
    let err_reader = std::thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr.read_to_end(&mut buffer);
        buffer
    });
    let deadline = Instant::now() + Duration::from_secs(wall);
    let status = loop {
        match child.try_wait().map_err(|_| "cannot wait for the runner")? {
            Some(status) => break Some(status),
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            None => std::thread::sleep(Duration::from_millis(5)),
        }
    };
    let _ = writer.join();
    let stdout = out_reader.join().unwrap_or_default();
    let stderr = err_reader.join().unwrap_or_default();
    drop(folder);
    let Some(status) = status else {
        return Ok(None);
    };
    let returncode = status
        .signal()
        .map(|signal| -signal)
        .or_else(|| status.code())
        .unwrap_or(-1);
    Ok(Some(Finished {
        returncode,
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    }))
}

fn cpu_killed(returncode: i32) -> bool {
    matches!(returncode, -9 | -24 | 152)
}

/// One child run with its limits. `wall` and `cpu` default to the test
/// limits. The result is the runner's JSON object plus a `diagnostic` when
/// the run failed or the scaling decision rejected it.
pub fn child(
    language: &Language,
    payload: &Value,
    wall: Option<u64>,
    cpu: Option<u64>,
) -> Result<Value, String> {
    let wall = wall.unwrap_or(decide::WALL_SECONDS);
    let cpu = cpu.unwrap_or(decide::CPU_SECONDS);
    let Some(done) = spawn_and_wait(language, payload, wall, cpu)? else {
        let error = format!(
            "time limit: the code ran longer than {wall} s (an infinite loop, or too slow)"
        );
        return Ok(annotate(
            failure(error, Some(true)),
            payload,
            Some("wall_timeout"),
            None,
            wall,
            cpu,
        ));
    };
    let code = done.returncode;
    let status_category = if code < 0 {
        Some("signal_exit")
    } else if code != 0 {
        Some("nonzero_exit")
    } else {
        None
    };
    if code != 0 && done.stdout.trim().is_empty() {
        let reason = if cpu_killed(code) {
            "CPU time limit (too slow, or an infinite loop)".to_owned()
        } else {
            done.stderr
                .trim()
                .lines()
                .last()
                .unwrap_or("the run crashed")
                .to_owned()
        };
        return Ok(annotate(
            failure(truncate(&reason, 300), Some(cpu_killed(code))),
            payload,
            status_category,
            Some(code),
            wall,
            cpu,
        ));
    }
    let parsed = done
        .stdout
        .trim()
        .lines()
        .last()
        .and_then(|line| serde_json::from_str::<Value>(line).ok());
    let Some(Value::Object(result)) = parsed else {
        return Ok(annotate(
            failure(
                "the run printed something unexpected; remove print statements and try again"
                    .into(),
                None,
            ),
            payload,
            Some("malformed_result"),
            Some(code),
            wall,
            cpu,
        ));
    };
    let mut category = status_category;
    if category.is_none() && py::truthy(payload.get("scale")) {
        let scale = result
            .get("scale")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let decision = crate::plant::complexity(payload.get("scale"), &scale);
        if decision
            .as_ref()
            .is_some_and(|cx| cx.get("ok") != Some(&Value::Bool(true)))
        {
            category = Some(if scale.iter().any(|row| row.get("error").is_some()) {
                "scale_error"
            } else {
                "complexity_rejected"
            });
        }
    }
    Ok(annotate(result, payload, category, Some(code), wall, cpu))
}

fn tests_payload(ex: &Value, code: &str) -> Value {
    json!({
        "code": code,
        "function": ex["function"],
        "tests": ex["tests"],
        "compare": ex.get("compare").cloned().unwrap_or_else(|| json!("exact")),
        "scale": null,
    })
}

fn sizes(spec: &Value) -> Vec<i64> {
    spec.get("sizes")
        .and_then(Value::as_array)
        .map(|sizes| sizes.iter().filter_map(Value::as_i64).collect())
        .unwrap_or_default()
}

/// Run code against every test, then (in a separate process, so a slow
/// solution keeps its test results) the scaling check.
pub fn run(language: &Language, ex: &Value, code: &str, with_scale: bool) -> Result<Value, String> {
    let mut out = child(language, &tests_payload(ex, code), None, None)?;
    let passed = !py::truthy(out.get("error"))
        && out
            .get("results")
            .and_then(Value::as_array)
            .is_none_or(|rows| rows.iter().all(|row| py::truthy(row.get("ok"))));
    let spec = ex.get("scale").filter(|spec| py::truthy(Some(spec)));
    if let (true, Some(spec), true) = (with_scale, spec, passed) {
        let mut scale = spec.as_object().cloned().unwrap_or_default();
        scale.insert("sizes".into(), json!(decide::scale_sizes(&sizes(spec))));
        scale.insert("repeats".into(), json!(decide::SCALE_REPEATS));
        let payload =
            json!({"code": code, "function": ex["function"], "tests": [], "scale": scale});
        let sc = child(
            language,
            &payload,
            Some(decide::SCALE_WALL_SECONDS),
            Some(decide::SCALE_CPU_SECONDS),
        )?;
        let rows = sc
            .get("scale")
            .filter(|rows| py::truthy(Some(rows)))
            .cloned();
        let rows = rows.unwrap_or_else(|| {
            let row = json!({
                "n": sizes(spec).last().copied(),
                "error": "too slow",
                "reason": sc.get("error").cloned().unwrap_or_else(|| json!("")),
            });
            json!([row.clone(), row])
        });
        let object = out
            .as_object_mut()
            .ok_or("the run result is not an object")?;
        object.insert("scale".into(), rows);
        if let Some(diagnostic) = sc.get("diagnostic") {
            object.insert("diagnostic".into(), diagnostic.clone());
        }
    }
    Ok(out)
}

/// The content check for the gate: fields, then the reference solution
/// passes every test and its own complexity class.
pub fn validate(language: &Language, ex: &Value) -> Result<Vec<String>, String> {
    let mut problems = match decide::structure(ex) {
        Ok(problems) => problems,
        Err(missing) => return Ok(missing),
    };
    let solution = ex["solution"].as_str().unwrap_or_default();
    let outcome = run(language, ex, solution, true)?;
    if py::truthy(outcome.get("error")) {
        let detail = match outcome.get("diagnostic") {
            Some(diagnostic) => py::dumps_sorted(diagnostic),
            None => outcome["error"].as_str().unwrap_or_default().to_owned(),
        };
        problems.push(format!("reference solution fails to run: {detail}"));
        return Ok(problems);
    }
    let bad: Vec<i64> = outcome
        .get("results")
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter(|row| !py::truthy(row.get("ok")))
                .filter_map(|row| row.get("i").and_then(Value::as_i64))
                .collect()
        })
        .unwrap_or_default();
    if !bad.is_empty() {
        problems.push(format!(
            "reference solution fails tests {}",
            py::repr_ints(&bad)
        ));
    }
    let scale = outcome
        .get("scale")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    if let Some(cx) = crate::plant::complexity(ex.get("scale"), &scale) {
        if cx.get("ok") != Some(&Value::Bool(true)) {
            let diagnostic = outcome.get("diagnostic");
            match diagnostic {
                Some(diagnostic) if diagnostic["category"] != "complexity_rejected" => {
                    problems.push(format!(
                        "reference solution scaling failed: {}",
                        py::dumps_sorted(diagnostic)
                    ));
                }
                _ => {
                    let suffix = if diagnostic.is_some() {
                        " [complexity_rejected]"
                    } else {
                        ""
                    };
                    let slope = match cx.get("slope") {
                        Some(Value::Number(number)) => py::float_repr(py::number_f64(number)),
                        _ => "None".to_owned(),
                    };
                    let expected = cx["expected"].as_str().unwrap_or_default();
                    problems.push(format!(
                        "reference solution misses its own complexity ({expected}, growth n^{slope}){suffix}"
                    ));
                }
            }
        }
    }
    Ok(problems)
}
