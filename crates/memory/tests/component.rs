//! Native rust-11 identity and owner-managed memory-file controls.

use careerbrew_memory::{memory_sync, project_import, research};

#[test]
fn item_identity_stays_exact_and_repository_identity_stays_separate() {
    let item = research::detect("https://github.com/acme/billing/pull/41/files?x=1#note")
        .expect("item URL");
    assert_eq!(
        item,
        (
            "url".into(),
            "https://github.com/acme/billing/pull/41".into()
        )
    );
    assert_eq!(
        research::github_item("https://github.com/acme/billing/pull/41"),
        Some(vec![
            "acme".into(),
            "billing".into(),
            "pull".into(),
            "41".into()
        ])
    );
    assert_eq!(
        research::detect("https://github.com/acme/billing/tree/main").unwrap(),
        ("repo".into(), "acme/billing".into())
    );
    assert_eq!(
        project_import::slug("Checkout Analytics"),
        "checkout-analytics"
    );
    assert_eq!(
        project_import::sheet_csv_url("https://docs.google.com/spreadsheets/d/abc_1-x/edit#gid=42")
            .unwrap(),
        "https://docs.google.com/spreadsheets/d/abc_1-x/export?format=csv&gid=42"
    );
}

#[test]
fn exact_item_urls_do_not_match_numeric_prefixes_or_repository_scope() {
    let item = serde_json::json!({
        "id": "pr:acme/billing#410",
        "title": "other item",
        "at": "2026-10-21T14:00:00Z",
        "url": "https://github.com/acme/billing/pull/410"
    });
    let project = serde_json::json!({
        "sources": [{"kind": "url", "ref": "https://github.com/acme/billing/pull/41"}]
    });
    assert_eq!(memory_sync::score_item(&item, &project, &[]), 0);
}

#[test]
fn external_rewrite_preserves_mode_and_carries_an_owner_append() {
    let scratch = tempfile::tempdir().unwrap();
    let data = scratch.path().join("data");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(scratch.path().join("external")).unwrap();
    std::env::set_var("HOME", scratch.path().join("home"));
    std::env::set_var("CAREERBREW_HOME", &data);
    std::env::set_var(
        "CAREERBREW_KEY_FILE",
        scratch.path().join("keys/backup.key"),
    );
    std::env::set_var("CAREERBREW_ENCRYPT", "off");
    let path = scratch.path().join("external/notes.md");
    std::fs::write(&path, "# notes\n- old\n").unwrap();
    let mut permissions = std::fs::metadata(&path).unwrap().permissions();
    use std::os::unix::fs::PermissionsExt;
    permissions.set_mode(0o600);
    std::fs::set_permissions(&path, permissions).unwrap();
    let old = std::fs::read_to_string(&path).unwrap();
    assert!(memory_sync::rewrite_external(&data, &path, &old, "# notes\n").unwrap());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "# notes\n");
    assert_eq!(
        std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert!(!memory_sync::rewrite_external(&data, &path, "different", "bad").unwrap());
}
