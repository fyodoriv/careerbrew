//! First bounded synthetic child observation; not a qualified supervisor.

#[allow(dead_code)]
mod capture_actions {
    include!("../src/capture_actions.rs");
}
#[path = "support/store_supervisor.rs"]
mod supervisor;

use std::process::Command;
use std::time::Duration;

#[test]
fn normal_exit_is_observed_once() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "supervisor_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("CAREERBREW_SUPERVISOR_CONTROL", "normal");
    let run =
        capture_actions::execute_during_observed(command, Duration::from_secs(5), |_, _| Ok(()))
            .expect("outer owner failed; worker cleanup is unconfirmed");
    assert_eq!(
        run.observation.wait_code,
        Some(0),
        "fixture failed; worker cleanup is unconfirmed"
    );
    assert!(
        run.observation.wait_observed
            && run.observation.reaped
            && run.observation.cleanup_succeeded
    );
    assert!(
        run.stdout.contains("SYNTHETIC_CHILD_REAPED_ONCE"),
        "inner wait not confirmed"
    );
}

#[test]
fn nonzero_exit_is_observed_once() {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "supervisor_fixture",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env_clear()
        .env("CAREERBREW_SUPERVISOR_CONTROL", "nonzero");
    let run =
        capture_actions::execute_during_observed(command, Duration::from_secs(5), |_, _| Ok(()))
            .expect("outer owner failed; worker cleanup is unconfirmed");
    assert_eq!(
        run.observation.wait_code,
        Some(0),
        "fixture failed; worker cleanup is unconfirmed"
    );
    assert!(
        run.observation.wait_observed
            && run.observation.reaped
            && run.observation.cleanup_succeeded
    );
    assert!(
        run.stdout.contains("SYNTHETIC_CHILD_REAPED_ONCE"),
        "inner wait not confirmed"
    );
}

#[test]
fn live_child_is_observed_before_release() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_store_supervisor_fixture"));
    let before = supervisor::Receipt::capture(binary).expect("fixture evidence unavailable");
    let mut command = Command::new(binary);
    const CANARY: &str = "synthetic-fixture-canary-71df";
    command
        .env_clear()
        .env("CAREERBREW_FIXTURE_CANARY", CANARY)
        .arg("supervisor");
    let run =
        capture_actions::execute_during_observed(command, Duration::from_secs(15), |_, _| Ok(()))
            .expect("outer owner failed; worker cleanup is unconfirmed");
    assert_eq!(
        run.observation.wait_code,
        Some(0),
        "fixture failed; worker cleanup is unconfirmed"
    );
    assert!(
        run.observation.wait_observed
            && run.observation.reaped
            && run.observation.cleanup_succeeded
    );
    assert!(
        run.stdout.contains("SYNTHETIC_LIVE_WAIT_CONFIRMED"),
        "live wait was not confirmed"
    );
    assert!(
        supervisor::output_is_safe(&run.stdout, &run.stderr, CANARY),
        "fixture output was not content-free"
    );
    let after = supervisor::Receipt::capture(binary).expect("fixture evidence unavailable");
    assert!(before.matches(&after), "fixture evidence changed");
    after.write_private().expect("fixture receipt unavailable");
}

#[test]
fn incompatible_sigchld_refuses_before_worker_start() {
    let binary = std::path::Path::new(env!("CARGO_BIN_EXE_store_supervisor_fixture"));
    let before = supervisor::Receipt::capture(binary).expect("fixture evidence unavailable");
    for (mode, expected) in [
        ("sigchld-ignored", "SYNTHETIC_SIGCHLD_REFUSED_BEFORE_SPAWN"),
        ("sigchld-handler", "SYNTHETIC_SIGCHLD_REFUSED_BEFORE_SPAWN"),
        (
            "sigchld-no-cldwait",
            "SYNTHETIC_SIGCHLD_REFUSED_BEFORE_SPAWN",
        ),
        ("sigchld-bypass", "SYNTHETIC_START_DETECTOR_REJECTED"),
    ] {
        let mut command = Command::new(binary);
        const CANARY: &str = "synthetic-fixture-canary-71df";
        command
            .env_clear()
            .env("CAREERBREW_FIXTURE_CANARY", CANARY)
            .arg(mode);
        let run =
            capture_actions::execute_during_observed(command, Duration::from_secs(15), |_, _| {
                Ok(())
            })
            .expect("outer owner failed; worker cleanup is unconfirmed");
        assert!(
            supervisor::output_is_safe(&run.stdout, &run.stderr, CANARY),
            "{mode}: unsafe output"
        );
        let diagnostic = match run.stderr.trim() {
            "SYNTHETIC_START_ATTEMPTED" => "start attempted",
            "SYNTHETIC_SIGNAL_INSTALL_FAILED" => "signal installation failed",
            "SYNTHETIC_SIGNAL_READBACK_FAILED" => "signal readback failed",
            "SYNTHETIC_SIGNAL_HANDLER_MISMATCH" => "signal handler mismatch",
            "SYNTHETIC_SIGNAL_FLAGS_MISMATCH" => "signal flags mismatch",
            _ => "unknown",
        };
        assert_eq!(
            run.observation.wait_code,
            Some(0),
            "{mode}: control failed: {diagnostic}"
        );
        assert!(
            run.observation.wait_observed
                && run.observation.reaped
                && run.observation.cleanup_succeeded
        );
        assert!(
            run.stdout.trim() == expected,
            "{mode}: wrong control boundary"
        );
    }
    let after = supervisor::Receipt::capture(binary).expect("fixture evidence unavailable");
    assert!(before.matches(&after), "fixture evidence changed");
    after.write_private().expect("fixture receipt unavailable");
}

#[test]
#[ignore]
fn supervisor_fixture() {
    let mode = std::env::var("CAREERBREW_SUPERVISOR_CONTROL").unwrap();
    let (program, expected) = match mode.as_str() {
        "normal" => ("/usr/bin/true", 0),
        "nonzero" => ("/usr/bin/false", 1),
        _ => panic!("unknown synthetic control"),
    };
    supervisor::verify_sigchld().expect("SIGCHLD is incompatible");
    let child = Command::new(program).spawn().unwrap();
    let pid = child.id() as libc::pid_t;
    let (status, waits) = supervisor::observe(child).expect("unconfirmed child status");
    assert_eq!(status.code(), Some(expected));
    assert_eq!(waits, 1);
    let mut duplicate_status = 0;
    assert_eq!(
        unsafe { libc::waitpid(pid, &mut duplicate_status, libc::WNOHANG) },
        -1
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::ECHILD)
    );
    println!("SYNTHETIC_CHILD_REAPED_ONCE");
}
