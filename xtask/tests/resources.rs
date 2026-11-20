#[path = "../src/resources.rs"]
mod resources;

use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

const FIXTURE: &str = env!("CARGO_BIN_EXE_resource_fixture");
const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");
const ALLOC: u64 = 64 * 1024 * 1024;

/// Prints one measurement line and, when `CAREERBREW_RESOURCES_RECORD` names
/// a file, appends it there. A passing `cargo test` hides stderr, and the gate's
/// pinned arguments refuse `--nocapture`; the hosted gate prints the file.
fn record(line: String) {
    eprintln!("{line}");
    if let Some(path) = std::env::var_os("CAREERBREW_RESOURCES_RECORD") {
        use std::io::Write;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .expect("open resources record");
        writeln!(file, "{line}").expect("write resources record");
    }
}

fn run(args: &[&str]) -> resources::Usage {
    let args = args.iter().map(OsString::from).collect::<Vec<_>>();
    resources::measure(Path::new(FIXTURE), &args).expect("measure")
}

/// The qualified path is the CLI: a small parent, so a Linux child never
/// reports this test process's RSS floor (another test holds 128 MiB here).
fn cli_usage(args: &[&str]) -> resources::Usage {
    let value = cli_json(args);
    let (os, unit) = resources::host_unit().expect("host unit");
    resources::Usage {
        os,
        unit,
        outcome: resources::Outcome::Exited(0),
        user_cpu_us: field(&value, "user_cpu_us"),
        system_cpu_us: field(&value, "system_cpu_us"),
        max_rss_raw: field(&value, "max_rss_raw"),
        max_rss_bytes: field(&value, "max_rss_bytes"),
        wall_us: field(&value, "wall_us"),
        parent_max_rss_bytes: field(&value, "parent_max_rss_bytes"),
        descendants: field(&value, "descendants"),
        tree_user_cpu_us: field(&value, "tree_user_cpu_us"),
        tree_system_cpu_us: field(&value, "tree_system_cpu_us"),
        tree_max_rss_bytes: field(&value, "tree_max_rss_bytes"),
        tree_method: "cli",
    }
}

fn raw_pair() -> (resources::Usage, resources::Usage) {
    let idle = cli_usage(&["alloc", "0"]);
    let loaded = cli_usage(&["alloc", &ALLOC.to_string()]);
    assert!(idle.passed() && loaded.passed());
    (idle, loaded)
}

#[test]
fn known_allocation_is_within_tolerance_on_this_os() {
    let (idle, loaded) = raw_pair();
    let delta = resources::check_calibration(idle.max_rss_bytes, loaded.max_rss_bytes, ALLOC)
        .unwrap_or_else(|reason| panic!("{} calibration: {reason}", idle.os));
    record(format!(
        "calibration {}: unit={} idle={} loaded={} delta={} ratio={:.3}",
        idle.os,
        idle.unit.name(),
        idle.max_rss_bytes,
        loaded.max_rss_bytes,
        delta,
        delta as f64 / ALLOC as f64
    ));
}

#[test]
fn planted_wrong_unit_fails_calibration() {
    let (idle, loaded) = raw_pair();
    let wrong = idle.unit.other();
    let result = resources::check_calibration(
        wrong.to_bytes(idle.max_rss_raw),
        wrong.to_bytes(loaded.max_rss_raw),
        ALLOC,
    );
    assert!(
        result.is_err(),
        "wrong unit {} passed calibration",
        wrong.name()
    );
}

#[test]
fn cpu_burn_child_reports_cpu_time() {
    let usage = run(&["burn", "300"]);
    assert!(usage.passed());
    assert!(usage.user_cpu_us + usage.system_cpu_us > 0, "{usage:?}");
}

#[test]
fn signaled_child_is_reported_and_never_a_pass() {
    let usage = run(&["sigkill"]);
    assert_eq!(usage.outcome, resources::Outcome::Signaled(libc::SIGKILL));
    assert!(!usage.passed());
    let output = Command::new(XTASK)
        .args(["migration", "resources", "--cmd", FIXTURE, "sigkill"])
        .output()
        .expect("xtask");
    assert_eq!(output.status.code(), Some(1));
    let text = String::from_utf8(output.stdout).unwrap();
    assert!(text.contains("\"outcome\":\"signaled\""), "{text}");
    assert!(text.contains("\"pass\":false"), "{text}");
}

#[test]
fn command_prints_json_and_refuses_a_relative_path() {
    let output = Command::new(XTASK)
        .args(["migration", "resources", "--cmd", FIXTURE, "alloc", "0"])
        .output()
        .expect("xtask");
    assert_eq!(output.status.code(), Some(0));
    let text = String::from_utf8(output.stdout).unwrap();
    let value: serde_json::Value = serde_json::from_str(text.trim()).expect("json");
    assert_eq!(value["pass"], true);
    assert_eq!(value["scope"], "process-tree");
    assert_eq!(value["descendants"], 0);
    let relative = Command::new(XTASK)
        .args(["migration", "resources", "--cmd", "resource_fixture"])
        .output()
        .expect("xtask");
    assert_eq!(relative.status.code(), Some(2));
}

#[test]
fn malformed_rusage_fields_are_refused() {
    let clean: libc::rusage = unsafe { std::mem::zeroed() };
    assert!(resources::validate_rusage(&clean).is_ok());
    let mut negative_time = clean;
    negative_time.ru_utime.tv_sec = -1;
    let mut big_usec = clean;
    big_usec.ru_stime.tv_usec = 1_000_000;
    let mut negative_rss = clean;
    negative_rss.ru_maxrss = -1;
    for bad in [negative_time, big_usec, negative_rss] {
        assert!(resources::validate_rusage(&bad).is_err());
    }
}

#[test]
fn idle_baseline_from_the_cli_does_not_inherit_a_large_parent() {
    // Hold about 128 MiB of touched memory in this process.
    let mut held = vec![0u8; 128 * 1024 * 1024];
    for (index, byte) in held.iter_mut().enumerate() {
        *byte = (index % 251) as u8 | 1;
    }
    std::hint::black_box(&held);
    let direct = run(&["alloc", "0"]);
    let output = Command::new(XTASK)
        .args(["migration", "resources", "--cmd", FIXTURE, "alloc", "0"])
        .output()
        .expect("xtask");
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    let via_cli = value["max_rss_bytes"].as_u64().unwrap();
    record(format!(
        "inheritance {}: parent={} direct_idle={} direct_may_include_parent={} cli_idle={}",
        direct.os,
        direct.parent_max_rss_bytes,
        direct.max_rss_bytes,
        direct.rss_may_include_parent(),
        via_cli
    ));
    assert!(
        via_cli < 32 * 1024 * 1024,
        "CLI idle child inherited {via_cli} bytes"
    );
    if direct.max_rss_bytes >= 64 * 1024 * 1024 {
        assert!(direct.rss_may_include_parent(), "{direct:?}");
    }
}

#[test]
fn a_spawn_failure_is_exit_3_with_a_json_error() {
    let output = Command::new(XTASK)
        .args([
            "migration",
            "resources",
            "--cmd",
            "/nonexistent/careerbrew-fixture",
        ])
        .output()
        .expect("xtask");
    assert_eq!(output.status.code(), Some(3));
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).expect("json");
    assert!(value["error"].is_string());
    assert_eq!(value["pass"], false);
}

fn cli_json(args: &[&str]) -> serde_json::Value {
    let mut full = vec!["migration", "resources", "--cmd", FIXTURE];
    full.extend_from_slice(args);
    let output = Command::new(XTASK).args(&full).output().expect("xtask");
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    serde_json::from_slice(&output.stdout).expect("json")
}

fn field(value: &serde_json::Value, name: &str) -> u64 {
    value[name]
        .as_u64()
        .unwrap_or_else(|| panic!("missing {name}: {value}"))
}

#[test]
fn an_unwaited_grandchild_cpu_burn_is_counted() {
    let value = cli_json(&["orphan", "burn", "300"]);
    let child = field(&value, "user_cpu_us") + field(&value, "system_cpu_us");
    let tree = field(&value, "tree_user_cpu_us") + field(&value, "tree_system_cpu_us");
    assert!(field(&value, "descendants") >= 1, "{value}");
    assert!(
        tree >= child + 100_000,
        "grandchild CPU not counted: {value}"
    );
}

#[test]
fn an_unwaited_grandchild_allocation_is_within_tolerance() {
    let idle = cli_json(&["orphan", "hold", "0", "200"]);
    let loaded = cli_json(&["orphan", "hold", &ALLOC.to_string(), "200"]);
    let delta = resources::check_calibration(
        field(&idle, "tree_max_rss_bytes"),
        field(&loaded, "tree_max_rss_bytes"),
        ALLOC,
    )
    .unwrap_or_else(|reason| panic!("tree calibration: {reason}"));
    record(format!(
        "tree calibration {}: method={} delta={} ratio={:.3}",
        idle["os"],
        idle["tree_method"],
        delta,
        delta as f64 / ALLOC as f64
    ));
}

/// Review gap 1: on macOS the child's peak is `ru_maxrss` (resident) and a
/// descendant's is the sampled phys footprint or resident size. The JSON
/// names the mix, so a reader never takes it for one metric.
#[test]
fn the_tree_peak_names_the_metric_of_each_part() {
    let value = cli_json(&["alloc", "0"]);
    let expected = if cfg!(target_os = "macos") {
        "child=ru_maxrss;descendants=max(lifetime_phys_footprint,resident_size)"
    } else if cfg!(target_os = "linux") {
        "child=ru_maxrss;descendants=ru_maxrss"
    } else {
        "unsupported"
    };
    assert_eq!(value["tree_rss_metric"], expected, "{value}");
}

/// Review gap 2 (Linux only): a group member whose parent left the group
/// (`setsid`) is not a child of the measuring process, so `wait4(-pgid)`
/// says `ECHILD` while it still runs. Its CPU must still be counted. macOS
/// samples the group and never uses `ECHILD`, so it proves nothing there.
#[cfg(target_os = "linux")]
#[test]
fn a_member_whose_parent_left_the_group_is_counted() {
    let value = cli_json(&["split", "500", "burn", "300"]);
    let child = field(&value, "user_cpu_us") + field(&value, "system_cpu_us");
    let tree = field(&value, "tree_user_cpu_us") + field(&value, "tree_system_cpu_us");
    assert!(
        tree >= child + 100_000,
        "member left behind by a setsid parent not counted: {value}"
    );
}

/// Task rust-00-process-tree-escapees (Linux only): a grandchild that calls
/// `setsid` leaves the group, so the group method never sees it. With a
/// delegated cgroup v2 scope its 300 ms burn is counted within 0.75x to 1.5x.
/// Without one the run falls back and records the gap as unsupported.
#[cfg(target_os = "linux")]
#[test]
fn a_descendant_that_calls_setsid_is_counted_by_the_cgroup() {
    let value = cli_json(&["escape", "burn", "300"]);
    let method = value["tree_method"].as_str().unwrap_or("").to_string();
    let child = field(&value, "user_cpu_us") + field(&value, "system_cpu_us");
    let tree = field(&value, "tree_user_cpu_us") + field(&value, "tree_system_cpu_us");
    let escaped = tree.saturating_sub(child);
    let supported = method.starts_with("cgroup-v2");
    record(format!(
        "escapee {}: method={method} escaped_cpu_us={escaped} supported={supported}",
        value["os"]
    ));
    if supported {
        assert!(
            (225_000..=450_000).contains(&escaped),
            "setsid descendant CPU outside 225..=450 ms: {value}"
        );
    }
}
