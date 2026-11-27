//! Direct native controls for rust-17's synthetic component boundary.

use careerbrew_day::promises::{bullets, LockOnlyPromises, PromiseRecorder};
use careerbrew_day::{difflib, handoff, pm_learn, results, Result};
use serde_json::json;
use std::cell::RefCell;
use std::path::Path;
use std::sync::Mutex;

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn setup(root: &Path) -> std::path::PathBuf {
    std::fs::create_dir_all(root.join("home")).unwrap();
    std::env::set_var("HOME", root.join("home"));
    std::env::set_var("CAREERBREW_HOME", root.join("data"));
    std::env::set_var("CAREERBREW_KEY_FILE", root.join("keys/backup.key"));
    std::env::set_var("CAREERBREW_EVENTS", "off");
    std::env::set_var("CAREERBREW_ENCRYPT", "on");
    std::env::set_var("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00");
    std::env::set_var("TZ", "UTC");
    let data = root.join("data");
    std::fs::create_dir_all(&data).unwrap();
    data
}

/// Records each call and whether the sent-day lock was held at that time.
/// The probe is a non-blocking try-lock, so it can never wait on the lock.
struct Probe {
    calls: RefCell<Vec<(String, bool)>>,
}

impl PromiseRecorder for Probe {
    fn record(&self, root: &Path, day: &str, _text: &str, revision: &str) -> Result<()> {
        let lock = root.join(format!("day/.pm-sent-{day}.lock"));
        let file = std::fs::File::open(&lock).unwrap();
        let held = matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock));
        self.calls.borrow_mut().push((revision.to_owned(), held));
        Ok(())
    }
}

#[test]
fn difflib_matches_python_reference_values() {
    assert_eq!(difflib::ratio("", ""), 1.0);
    assert_eq!(difflib::ratio("abc", ""), 0.0);
    assert_eq!(difflib::ratio("abcd", "bcda"), 0.75);
    let blocks = difflib::blocks("abxcd", "abcd");
    let found = blocks
        .iter()
        .map(|m| (m.a, m.b, m.size))
        .collect::<Vec<_>>();
    assert_eq!(found, vec![(0, 0, 2), (3, 2, 2), (5, 4, 0)]);
    let a = ["a", "b"].map(String::from);
    let b = ["a", "c"].map(String::from);
    let ops = difflib::line_opcodes(&a, &b);
    assert_eq!(
        ops.iter().map(|op| op.tag).collect::<Vec<_>>(),
        ["equal", "replace"]
    );
}

#[test]
fn results_and_handoff_are_sealed_idempotent_and_bounded() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let root = setup(scratch.path());
    let day = careerbrew_day::iso_day("2026-11-02").unwrap();
    let row = json!({"id": "pr:1", "at": "2026-11-02T08:00:00+00:00", "kind": "pr", "title": "SYNTHCANARY-rust17-direct"});
    assert_eq!(
        results::append(&root, std::slice::from_ref(&row), day).unwrap(),
        1
    );
    assert_eq!(results::append(&root, &[row], day).unwrap(), 0);
    let bytes = std::fs::read(root.join(results::relative(day))).unwrap();
    assert!(bytes.starts_with(b"CBV1\n"));
    assert!(!bytes.windows(9).any(|part| part == b"SYNTHCANA"));
    let error = handoff::item("prs", &"x".repeat(201), "", &[]).unwrap_err();
    assert_eq!(error.class, "ValueError");
    let item = handoff::add(&root, "prs", "Review one", "", day).unwrap();
    assert!(handoff::done(&root, item["id"].as_str().unwrap(), day).unwrap());
    assert!(!handoff::done(&root, "h-missing", day).unwrap());
}

#[test]
fn sent_text_calls_the_promise_seam_inside_the_sent_day_lock() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let root = setup(scratch.path());
    let probe = Probe {
        calls: RefCell::new(Vec::new()),
    };
    let receipt = pm_learn::Receipt {
        channel: "C0SYN",
        ts: "1.5",
    };
    pm_learn::save_sent(
        &root,
        "2026-10-27",
        "Billing: shipped.",
        Some(&receipt),
        &probe,
    )
    .unwrap();
    pm_learn::save_sent(&root, "2026-10-27", "Billing: manual.", None, &probe).unwrap();
    let calls = probe.calls.borrow();
    assert_eq!(calls.len(), 2);
    assert!(calls.iter().all(|(_, held)| *held), "{calls:?}");
    assert_eq!(calls[0].0.len(), 32);
    assert_eq!(calls[1].0, "");
    assert!(!root.join(pm_learn::receipt_relative("2026-10-27")).exists());
    // The project lock is taken after the sent-day lock, and nothing is written.
    pm_learn::save_sent(&root, "2026-10-28", "Plain text.", None, &LockOnlyPromises).unwrap();
    assert!(root.join("work/.projects.yaml.lock").exists());
    assert!(!root.join("work/projects.yaml").exists());
}

#[test]
fn the_synthetic_promise_provider_refuses_tomorrow_bullets() {
    let _guard = ENV_LOCK.lock().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let root = setup(scratch.path());
    let text = "Today: done.\nTomorrow\n- Ship the export\n- Ship the export\n  with tests";
    assert_eq!(bullets(text), vec!["Ship the export with tests".to_owned()]);
    let error = LockOnlyPromises
        .record(&root, "2026-10-27", text, "")
        .unwrap_err();
    assert_eq!(error.class, "PromiseSeamUnsupported");
}
