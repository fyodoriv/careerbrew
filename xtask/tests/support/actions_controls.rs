mod capture_actions {
    include!("../../src/capture_actions.rs");

    pub fn capture_fault(
        root: &std::path::Path,
        python: &std::path::Path,
        fault: &str,
        descendant: &std::path::Path,
    ) -> Result<serde_json::Value, String> {
        capture_mode(root, python, fault, descendant)
    }
    pub fn assert_leader_is_unreaped() {
        let mut attempts = 0;
        let result = retry_interrupted(|| {
            attempts += 1;
            if attempts == 1 {
                Err(std::io::Error::from_raw_os_error(libc::EINTR))
            } else {
                Ok(false)
            }
        })
        .unwrap();
        assert!(!result);
        assert_eq!(attempts, 2);

        let mut command = std::process::Command::new("/usr/bin/true");
        command.process_group(0);
        let mut guard = RunningChild {
            child: command.spawn().unwrap(),
            released: false,
            group_signals: Default::default(),
        };
        let signal_count = guard.group_signals.clone();
        let pid = guard.child.id() as i32;
        let deadline = Instant::now() + Duration::from_secs(3);
        while !leader_exited(&mut guard.child).unwrap() {
            assert!(Instant::now() < deadline);
            thread::sleep(Duration::from_millis(10));
        }
        let mut status = 0;
        // This exact direct child must still be waitable after observation.
        let result = unsafe { libc::waitpid(pid, &mut status, libc::WNOHANG) };
        assert_eq!(
            result, pid,
            "terminal observation must leave the leader unreaped"
        );
        // Drop must revalidate ownership after an external reap, even with a stale flag.
        assert!(!guard.released);
        drop(guard);
        assert_eq!(
            signal_count.get(),
            0,
            "reaped child must not authorize a group signal"
        );
    }
    pub fn assert_owned_group_states() {
        #[cfg(target_os = "macos")]
        {
            for (script, expected) in [("exit 0", true), ("/bin/sleep 60 & exit 0", false)] {
                let mut command = std::process::Command::new("/bin/sh");
                command
                    .args(["-c", script])
                    .env_clear()
                    .process_group(0)
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
                let mut guard = RunningChild {
                    child: command.spawn().unwrap(),
                    released: false,
                    group_signals: Default::default(),
                };
                let deadline = Instant::now() + Duration::from_secs(3);
                while !leader_exited(&mut guard.child).unwrap() {
                    assert!(Instant::now() < deadline);
                    thread::sleep(Duration::from_millis(10));
                }
                assert_eq!(
                    zombie_only_group(guard.child.id() as i32).unwrap(),
                    expected
                );
                assert!(guard.stop_and_reap().unwrap().success());
                assert!(guard.released);
            }
        }
    }
}

use serde_json::{json, Value};
use std::{
    path::Path,
    sync::OnceLock,
    thread,
    time::{Duration, Instant},
};

// The observed pair is immutable and scoped to this integration-test process.
static OBSERVED: OnceLock<Value> = OnceLock::new();

fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn observed() -> Value {
    OBSERVED
        .get_or_init(|| capture_actions::capture(root(), &root().join(".venv/bin/python")).unwrap())
        .clone()
}

fn fault(name: &str) -> String {
    let controls = tempfile::tempdir().unwrap();
    capture_actions::capture_fault(
        root(),
        &root().join(".venv/bin/python"),
        name,
        &controls.path().join("descendant"),
    )
    .expect_err("actual negative execution must be refused")
}

pub fn captures_real_reference_schema_and_module_provenance() {
    let receipt = observed();
    capture_actions::validate(&receipt).unwrap();
    assert_eq!(receipt["executions"][0]["observer"]["catalog_calls"], 1);
    assert!(receipt["executions"][0]["observer"]["modules"]["careerbrew.cli"].is_object());
    assert!(
        receipt["executions"][0]["observer"]["site_inputs"]
            .as_object()
            .unwrap()
            .len()
            > 1
    );
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("contract.json");
    capture_actions::run(root(), &root().join(".venv/bin/python"), &path).unwrap();
    let first = std::fs::read(&path).unwrap();
    assert!(capture_actions::run(root(), &root().join(".venv/bin/python"), &path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), first);
}

pub fn rejects_same_removed_action_in_both_real_children() {
    assert!(fault("remove-action").contains("frozen reference"));
}

pub fn rejects_same_argument_and_delivery_changes_in_real_children() {
    for name in ["argument", "terminal-only", "confirmation"] {
        let error = fault(name);
        assert!(
            error.contains("frozen reference") || error.contains("schema"),
            "{name}: {error}"
        );
    }
}

pub fn rejects_malformed_action_and_argument_types_in_real_children() {
    for name in ["action-type", "argument-type"] {
        assert!(fault(name).contains("schema"));
    }
}

pub fn rejects_actual_stdout_canary() {
    assert!(fault("stdout-canary").contains("canary"));
}

pub fn rejects_actual_stderr_canary() {
    assert!(fault("stderr-canary").contains("canary"));
}

pub fn scans_actual_retained_canary_content_before_hashing() {
    assert!(fault("retained-canary").contains("canary leaked in retained content"));
}

pub fn rejects_actual_extra_retained_file() {
    assert!(fault("extra-write").contains("retained change"));
}

pub fn rejects_actual_skipped_cli_entry() {
    assert!(fault("skip").contains("entry execution provenance missing"));
}

pub fn rejects_observed_nonzero_exit() {
    assert!(fault("nonzero").contains("Some(17)"));
}

pub fn cleans_up_owned_descendant_process_group() {
    let controls = tempfile::tempdir().unwrap();
    let path = controls.path().join("descendant");
    let error = capture_actions::capture_fault(
        root(),
        &root().join(".venv/bin/python"),
        "descendant",
        &path,
    )
    .unwrap_err();
    assert!(error.contains("execution provenance"));
    let pid: i32 = std::fs::read_to_string(&path).unwrap().parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        // Signal zero observes only the exact PID reported by this owned fixture.
        if unsafe { libc::kill(pid, 0) } != 0 {
            break;
        }
        let output = std::process::Command::new("/bin/ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .unwrap();
        if String::from_utf8_lossy(&output.stdout)
            .trim()
            .starts_with('Z')
        {
            break;
        }
        assert!(Instant::now() < deadline, "owned descendant still executes");
        thread::sleep(Duration::from_millis(10));
    }
}

pub fn rejects_wrong_loader_root_and_modified_input_evidence() {
    assert!(fault("wrong-root").contains("outside checkout"));
    for field in [
        "fixture_sha256",
        "sources_after",
        "python_after",
        "observer_sha256",
        "reference_revision",
        "historical_reference_sha256",
    ] {
        let mut receipt = observed();
        receipt[field] = json!("changed-input");
        assert!(capture_actions::validate(&receipt).is_err());
    }
    for field in ["reference_revision", "historical_reference_sha256"] {
        let mut receipt = observed();
        receipt.as_object_mut().unwrap().remove(field);
        assert!(capture_actions::validate(&receipt).is_err());
    }
    let mut receipt = observed();
    receipt["executions"][1]["observer"]["site_inputs"] = json!({});
    assert!(capture_actions::validate(&receipt).is_err());
}

pub fn retains_leader_ownership_until_group_cleanup() {
    capture_actions::assert_leader_is_unreaped();
}

pub fn distinguishes_owned_zombie_group_from_live_descendant() {
    capture_actions::assert_owned_group_states();
}
