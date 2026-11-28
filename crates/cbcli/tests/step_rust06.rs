//! Parity cases for the rust-06 native handlers: `"sync.export"` and
//! `"sync.import"` (the import cases are at the end of this file).
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

// ---------- sync import ----------

/// A synthetic Fernet pairing key (bytes 0 to 31), never a real secret.
/// Each bundle below is a Fernet token of a `json.dumps` bundle made with
/// it by the `cryptography` package, as Python's `sync.export` writes one.
const PAIR_KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

/// `{"ladder": "generic-swe", "target_level": "senior"}` from personal.
const FULL: &str = "gAAAAABrCk8VrawBtXrKv-XKzm3zpEi_Xj1BviNvXm_pum1ysLSlcpkScYs1zgYKO2Efwxa8iD1nIrChIY4sJtFOHtNdAf6sCfbHWqfhem5xsrhevOH5ouA5lFMxCREiB3Qx40nYPiryZOQFOr-AFlQr_HM-7LwdaYemQwD_WZDrvVuEI95Ibse-InDPNcB9NzLSvO01J6zN_tX3xvF7uLGwnblwmpwXbSbcpeW_Vt94NNS08DW_QoJjMNReSnEELnJhycIZnpOE";
/// `FULL` with bytes outside the base64 alphabet appended, which Python's
/// lenient decoder skips.
const FULL_JUNK: &str = "gAAAAABrCk8VrawBtXrKv-XKzm3zpEi_Xj1BviNvXm_pum1ysLSlcpkScYs1zgYKO2Efwxa8iD1nIrChIY4sJtFOHtNdAf6sCfbHWqfhem5xsrhevOH5ouA5lFMxCREiB3Qx40nYPiryZOQFOr-AFlQr_HM-7LwdaYemQwD_WZDrvVuEI95Ibse-InDPNcB9NzLSvO01J6zN_tX3xvF7uLGwnblwmpwXbSbcpeW_Vt94NNS08DW_QoJjMNReSnEELnJhycIZnpOE!!\n";
/// `FULL` with one character changed: it no longer decrypts.
const TAMPERED: &str = "gAAAAABrCk8VrawBtXrKv-XKzm3zpEi_Xj1BvjNvXm_pum1ysLSlcpkScYs1zgYKO2Efwxa8iD1nIrChIY4sJtFOHtNdAf6sCfbHWqfhem5xsrhevOH5ouA5lFMxCREiB3Qx40nYPiryZOQFOr-AFlQr_HM-7LwdaYemQwD_WZDrvVuEI95Ibse-InDPNcB9NzLSvO01J6zN_tX3xvF7uLGwnblwmpwXbSbcpeW_Vt94NNS08DW_QoJjMNReSnEELnJhycIZnpOE";
/// `{"ladder": "synthetic-ladder", "target_level": "principal"}`.
const UNKNOWN: &str = "gAAAAABrCk8V23J79tcQJXoDAvbn1CGN3KIb4RUyDMT6mFGFa_rw0uZAUZAQNP0tM0Qwtutjz4npXVIwgGAuUUL05qXnLOZ1tmK9uAx2apYBNv2ufmHuHPCeIMImNNlvDmYK-8maxV9401g8ZSKFTZeDCTl74M6aGXE5KXde83EgYssHnpVkAmaZM0cx6x9CSQTTGs2wvyTp0d3uFo8qRPu7py2B1hCdmYFtEJEsLYR22eD1WseBW8ck3RxBILjCh2DU_dOrjZBO";
/// `{"target_level": "staff"}`.
const LEVEL_ONLY: &str = "gAAAAABrCk8Vk89E_5z_ssVIo9mSnGeQ5eNjKoQdPHMql8wB__5cYOCAxw576LYxqjqgCYxrN9i8xTcxYIgLwjrKugs7YZWWXYO44YSp6RrOfE63FkF7oPHZL3jKlhAy9FIFhsYgBjBuvOEyhN7m6wNZzHEbAqugWolMo55zjBICk5B2yv3HyuPoSf7yWDXFEUlvf0kaEvc4ADPoMbL8xAHeQdmhQZ1LSA==";
/// `{"ladder": "generic-swe", "target_level": "principal"}`.
const MIXED: &str = "gAAAAABrCk8VQKNcEIq9o7r2G76M3kQtYL4ayUMBF5P5y0xe-Dxz4C2ZQzBge4exZvupMefj3tYnu8Ow2_tqlT63cK4yXCOp6EcL-m9N6_Ybwi6AWHon7o41UAFhdo9ikVimoyca79paeQGGDvXYBNvgMcLs03uUkuzXRnhRI-vLeoz27Z8R0pawVPpDT8Z2XNmMeEfKLnftOfMLUtLfXjFbOJTEEFHCvbqx_3wMfB9d8yYy5eMhmfXT4r0EAjkhWQZHt6rdTh-d";
/// `{}`.
const EMPTY: &str = "gAAAAABrCk8VpKkPdOn9VBhtZxVbon_CZC2WgTXHc4miRm4JmC2W7AwD4oHr0ZV4Nn4-fVpuBgqm2mdK7wHzGw_CD6jEEPYwz6RdNUGOW7H8U9DfUx_baQyCJukF7tEgIOQHu7b-AckkvQoZD0-g87XBUKyDkBkUD_RNjjzR67wXnXi6QRWioleOfk5fu1IesDM-V4JHehm1";
/// `{"target_level": <70 characters>, "ladder": 5}`: both ignored.
const ODD: &str = "gAAAAABrCk8VgMJzpCUKpCiMxyq6M4fL4u1focoUk5MQlEZra6YSk-FHZuY2AOPIn6lL02CVPUEgqrUWjD-G5l9l_pXJ0Zd7gMkETqvCH5EEG-HROok5IYDiFqyKXvabcU1iMeBPzFQpnwrS8SJcZuQHpvBZb9nxOBLhihzIIl8dWGp61t3Ng0v8oW9hB4uimg0sqc9FgAi6SQIBXlXgyC-LjntD-9ScGGBFkecJMpWNvDoJrdSvMX6l8BQicz4t2qH9T9n3EAx_B8aL24w74haHXs6Z4qKPDUsR1VM9hrY91lHzox-w9htCxXgcsGZGdc6htXfObc-l";
/// From another corporate machine.
const SAME_ROLE: &str = "gAAAAABrCk8VrgcYeq2WiyKBK0JcYjfr-Rd64MJwqpcmRlGlmUSecRo8R4h1-GcxbgrtwaphK4gVPijyxh_uyDeUsmyvliSspD0rEZoxgTRtASkhaPcNJCUapBo9AjkAlCsae29sAw7-oHfSbwVUpOrRjkKOE_qq9sqtlQTQpdl_xiw7G-fuuUiwhIBPPPskzM7wGszp_gVU2HteSy2sgAgGMMnM-UeN4A==";
/// A payload with a `jobs` key besides settings.
const EXTRA: &str = "gAAAAABrCk8VgxORTqnYkVq_UrsmPDvCjhxdvXf1CC9PjOzjv6vwMolyWsmfeZNojhfk0apS1OaWhjQSl98V2WXekgmKjuLwk-7twuQ43eF9Z3hQ_AndJvDYGVL5NNjeCVAc7m6TWD3FTZsF6W7o5o6q5b2Lm7DPEGHf0ile-s-zQiXqRpTNNR5is-NygyEEY3ueuK9nTX2VlVBPxz_w75yfX1q1VGN7Zw==";
/// Made with another synthetic key.
const WRONG_KEY: &str = "gAAAAABrCk8VEsIy2qoCqh2vyBlWwziMPQMlzaWfSqAu-NcFWV0taqVFlDpwtQbEBVxN0JCcok1wYMLZ8WBEnnm5ZJJSKlgow0Q4Km5YKgpRwUaR9ET_nBWh_rGqypWDLwmqZHvavVXNgEpKqWrvIQL_hi6_hoEeHQ==";

/// A corporate machine paired with `PAIR_KEY`.
const CORPORATE: &[&[&str]] = &[
    &["pair", "join", PAIR_KEY],
    &["machine", "set", "corporate"],
];

fn import_case(id: &'static str, files: &'static [(&'static str, &'static str)], must_serve: bool) {
    let home = prepare_setup(&Setup {
        id,
        cli: CORPORATE,
        files,
    });
    let file = home.data.join("in.cbsync").display().to_string();
    let served = assert_same(&home, &["sync", "import", &file], must_serve, "sync.import");
    assert_eq!(served, must_serve, "{id}");
}

#[test]
fn sync_import_matches_python_on_a_paired_corporate_machine() {
    strict(NOW);
    import_case("import-full", &[("in.cbsync", FULL)], true);
    import_case(
        "import-full-plain",
        &[(".plaintext", ""), ("in.cbsync", FULL)],
        true,
    );
    import_case(
        "import-junk",
        &[(".plaintext", ""), ("in.cbsync", FULL_JUNK)],
        true,
    );
}

#[test]
fn sync_import_counts_ignored_settings() {
    strict(NOW);
    import_case(
        "import-unknown",
        &[(".plaintext", ""), ("in.cbsync", UNKNOWN)],
        true,
    );
    import_case(
        "import-level",
        &[(".plaintext", ""), ("in.cbsync", LEVEL_ONLY)],
        true,
    );
    import_case(
        "import-odd",
        &[(".plaintext", ""), ("in.cbsync", ODD)],
        true,
    );
    import_case(
        "import-empty",
        &[(".plaintext", ""), ("in.cbsync", EMPTY)],
        true,
    );
}

#[test]
fn sync_import_keeps_other_settings_in_order() {
    strict(NOW);
    import_case(
        "import-merge",
        &[
            (".plaintext", ""),
            (
                "settings.yaml",
                "theme: dark\nladder: generic-swe\ntimezone: Etc/UTC\n",
            ),
            ("in.cbsync", MIXED),
        ],
        true,
    );
    import_case(
        "import-merge-sealed",
        &[
            ("settings.yaml", "target_level: mid\ntheme: dark\n"),
            ("in.cbsync", FULL),
        ],
        true,
    );
    // The level is checked against the ladder already set here.
    import_case(
        "import-level-unknown-ladder",
        &[
            (".plaintext", ""),
            ("settings.yaml", "ladder: synthetic-ladder\n"),
            ("in.cbsync", LEVEL_ONLY),
        ],
        true,
    );
}

/// Every Python error case, and every input this port does not write the
/// way PyYAML would, refuses before any write.
#[test]
fn sync_import_refuses_what_python_must_run() {
    strict(NOW);
    // A bundle from the same role, a payload besides settings, another
    // pairing key, and a changed token: Python prints `error:`.
    import_case(
        "import-same-role",
        &[(".plaintext", ""), ("in.cbsync", SAME_ROLE)],
        false,
    );
    import_case(
        "import-extra",
        &[(".plaintext", ""), ("in.cbsync", EXTRA)],
        false,
    );
    import_case(
        "import-wrong-key",
        &[(".plaintext", ""), ("in.cbsync", WRONG_KEY)],
        false,
    );
    import_case(
        "import-tampered",
        &[(".plaintext", ""), ("in.cbsync", TAMPERED)],
        false,
    );
    // A setting PyYAML writes as a number, a quoted word, a user ladder
    // folder, and a redaction input.
    import_case(
        "import-int-setting",
        &[
            (".plaintext", ""),
            ("settings.yaml", "work_hours: 8\n"),
            ("in.cbsync", FULL),
        ],
        false,
    );
    import_case(
        "import-quoted-setting",
        &[
            (".plaintext", ""),
            ("settings.yaml", "theme: 'yes'\n"),
            ("in.cbsync", FULL),
        ],
        false,
    );
    import_case(
        "import-user-ladders",
        &[
            (".plaintext", ""),
            ("ladders/readme.txt", "synthetic\n"),
            ("in.cbsync", FULL),
        ],
        false,
    );
    import_case(
        "import-terms",
        &[
            ("sensitive-terms.txt", "synthetic-codename\n"),
            ("in.cbsync", FULL),
        ],
        false,
    );
    // A personal machine refuses every bundle; an unpaired one cannot read it.
    let personal = prepare_setup(&Setup {
        id: "import-personal",
        cli: &[&["pair", "join", PAIR_KEY]],
        files: &[("in.cbsync", FULL)],
    });
    let file = personal.data.join("in.cbsync").display().to_string();
    assert!(!assert_same(
        &personal,
        &["sync", "import", &file],
        false,
        "sync.import"
    ));
    let unpaired = prepare_setup(&Setup {
        id: "import-unpaired",
        cli: &[&["machine", "set", "corporate"]],
        files: &[("in.cbsync", FULL)],
    });
    let file = unpaired.data.join("in.cbsync").display().to_string();
    assert!(!assert_same(
        &unpaired,
        &["sync", "import", &file],
        false,
        "sync.import"
    ));
    let missing = unpaired.data.join("missing.cbsync").display().to_string();
    assert!(!assert_same(
        &unpaired,
        &["sync", "import", &missing],
        false,
        "sync.import"
    ));
}

/// Negative controls: two bundles give different settings files and
/// lines, and a refusal writes nothing.
#[test]
fn sync_import_negative_controls() {
    strict(NOW);
    let full = prepare_setup(&Setup {
        id: "import-control-full",
        cli: CORPORATE,
        files: &[(".plaintext", ""), ("in.cbsync", FULL)],
    });
    let unknown = prepare_setup(&Setup {
        id: "import-control-unknown",
        cli: CORPORATE,
        files: &[(".plaintext", ""), ("in.cbsync", UNKNOWN)],
    });
    let path = |home: &Home| home.data.join("in.cbsync").display().to_string();
    let a = observe(&full, &["sync", "import", &path(&full)], &Side::Native);
    let b = observe(
        &unknown,
        &["sync", "import", &path(&unknown)],
        &Side::Python,
    );
    assert_eq!(
        a.stdout,
        "imported bundle from personal machine: 2 new record(s)\n"
    );
    assert!(b.stdout.contains("ignored 2 setting(s)"), "{b:?}");
    assert_ne!(a.files, b.files);
    let refused = prepare_setup(&Setup {
        id: "import-control-refused",
        cli: CORPORATE,
        files: &[(".plaintext", ""), ("in.cbsync", TAMPERED)],
    });
    let before = observe(
        &refused,
        &["sync", "import", &path(&refused), "extra"],
        &Side::Native,
    );
    assert_eq!(before.code, Some(64));
    let after = observe(
        &refused,
        &["sync", "import", &path(&refused)],
        &Side::Native,
    );
    assert_eq!(after.code, Some(64));
    assert_eq!(before.files, after.files);
    assert!(!after
        .files
        .iter()
        .any(|line| line.starts_with("settings.yaml")));
    assert!(after.events.is_empty());
}
