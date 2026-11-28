//! Parity cases for the rust-06 native handlers: `"sync.export"`.
//!
//! Each case builds a synthetic home with the Python CLI (`pair init` makes
//! the pairing key), then compares the native binary and the console script
//! with Python: stdout, exit code, data-root files and event rows. The
//! bundle itself lies outside the data root, so one more test decrypts both
//! sides' bundles with the pairing key and compares the plaintext bytes.
mod common;

use common::*;
use std::path::Path;
use std::process::Command;

const NOW: &str = "2026-11-20T09:00:00+00:00";

const SETTINGS: &str =
    "ladder: synthetic-ladder\ntarget_level: 4\ntheme: dark\nnote: synthetic text\n";

/// An absolute bundle path outside the data root, inside the scenario's
/// home (restored before every run).
fn bundle(home: &Home) -> String {
    home.home.join("bundle.enc").display().to_string()
}

fn export(home: &Home, file: &str, must_serve: bool) -> bool {
    assert_same(home, &["sync", "export", file], must_serve, "sync.export")
}

#[test]
fn sync_export_matches_python_in_a_paired_home() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "sync-paired",
        cli: &[&["pair", "init"]],
        files: &[("settings.yaml", SETTINGS)],
    });
    assert!(export(&home, &bundle(&home), true));
}

#[test]
fn sync_export_matches_python_with_no_settings() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "sync-no-settings",
        cli: &[&["machine", "set", "personal"], &["pair", "init"]],
        files: &[],
    });
    assert!(export(&home, &bundle(&home), true));
}

#[test]
fn sync_export_reads_sealed_settings() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "sync-encrypted",
        cli: &[&["pair", "init"], &["data", "encrypt"]],
        files: &[(
            "settings.yaml",
            "target_level: 2\nladder: \"synthetic \\u00e9 ladder\"\ntimezone: Etc/UTC\n",
        )],
    });
    assert!(export(&home, &bundle(&home), true));
}

/// Every Python error case refuses natively before any write, and the
/// console script then prints Python's `error:` line.
#[test]
fn sync_export_refuses_every_error_case() {
    strict(NOW);
    let unpaired = prepare_setup(&Setup {
        id: "sync-unpaired",
        cli: &[],
        files: &[("settings.yaml", SETTINGS)],
    });
    assert!(!export(&unpaired, &bundle(&unpaired), false));
    let corporate = prepare_setup(&Setup {
        id: "sync-corporate",
        cli: &[&["pair", "init"], &["machine", "set", "corporate"]],
        files: &[],
    });
    assert!(!export(&corporate, &bundle(&corporate), false));
    let paired = prepare_setup(&Setup {
        id: "sync-inside",
        cli: &[&["pair", "init"]],
        files: &[("settings.yaml", SETTINGS)],
    });
    let inside = paired.data.join("bundle.enc").display().to_string();
    assert!(!export(&paired, &inside, false));
    // A setting whose JSON form this port does not write goes to Python.
    let float = prepare_setup(&Setup {
        id: "sync-float",
        cli: &[&["pair", "init"]],
        files: &[("settings.yaml", "target_level: 2.5\n")],
    });
    assert!(!export(&float, &bundle(&float), false));
    // Redaction inputs present: the output line goes to Python.
    let terms = prepare_setup(&Setup {
        id: "sync-terms",
        cli: &[&["pair", "init"]],
        files: &[("sensitive-terms.txt", "synthetic-codename\n")],
    });
    assert!(!export(&terms, &bundle(&terms), false));
}

/// Decrypts a bundle with the scenario's pairing key through the repo's
/// Python (`cryptography` is a careerbrew dependency).
fn open_bundle(home: &Home, path: &str) -> Vec<u8> {
    let key = home.home.join(".config/careerbrew/pair.key");
    let done = Command::new(repo().join(".venv/bin/python"))
        .args([
            "-c",
            "import sys\nfrom cryptography.fernet import Fernet\nkey = open(sys.argv[1]).read().strip().encode()\nsys.stdout.buffer.write(Fernet(key).decrypt(open(sys.argv[2], 'rb').read()))",
        ])
        .arg(&key)
        .arg(path)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    assert!(done.status.success(), "{done:?}");
    done.stdout
}

#[test]
fn sync_export_bundle_plaintext_matches_python() {
    strict(NOW);
    let home = prepare_setup(&Setup {
        id: "sync-bundle",
        cli: &[&["pair", "init"]],
        files: &[("settings.yaml", SETTINGS)],
    });
    let path = bundle(&home);
    let args = ["sync", "export", path.as_str()];
    observe(&home, &args, &Side::Python);
    let want = open_bundle(&home, &path);
    let native = observe(&home, &args, &Side::Native);
    assert_eq!(native.code, Some(0));
    let got = open_bundle(&home, &path);
    assert_eq!(
        String::from_utf8(got).unwrap(),
        String::from_utf8(want).unwrap()
    );
    let mode = std::fs::metadata(Path::new(&path)).unwrap();
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(mode.permissions().mode() & 0o777, 0o600);
}

/// Negative controls: the comparison sees a planted difference (two homes
/// with different settings print different item counts), and an extra
/// argument is refused by the argument reader.
#[test]
fn sync_export_negative_controls() {
    strict(NOW);
    let full = prepare_setup(&Setup {
        id: "sync-control-full",
        cli: &[&["pair", "init"]],
        files: &[("settings.yaml", SETTINGS)],
    });
    let bare = prepare_setup(&Setup {
        id: "sync-control-bare",
        cli: &[&["pair", "init"]],
        files: &[],
    });
    let a = observe(&full, &["sync", "export", &bundle(&full)], &Side::Native);
    let b = observe(&bare, &["sync", "export", &bundle(&bare)], &Side::Python);
    assert!(a.stdout.contains("items=2"), "{a:?}");
    assert!(b.stdout.contains("items=0"), "{b:?}");
    assert_ne!(a, b);
    let extra = observe(
        &full,
        &["sync", "export", &bundle(&full), "extra"],
        &Side::Native,
    );
    assert_eq!(extra.code, Some(64));
    assert!(extra.events.is_empty());
}
