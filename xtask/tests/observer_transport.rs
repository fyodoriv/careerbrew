//! Local producer/controller controls. No Docker or kernel observations.
#[allow(dead_code)]
mod capture_actions {
    include!("../src/capture_actions.rs");
}
#[path = "../src/observer_probe/transport.rs"]
mod transport;
use serde_json::{json, Value};
use std::os::unix::{
    ffi::OsStrExt,
    fs::{symlink, PermissionsExt},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};
const CHUNK: usize = 131072;
const PRODUCER: &str = r#"
import importlib.util, json, sys
from pathlib import Path
spec = importlib.util.spec_from_file_location('fixed_export', sys.argv[1])
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
module.main(Path(sys.argv[2]), int(sys.argv[3]), json.loads(sys.argv[4]))
"#;
struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    output: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("work");
        fs::create_dir(&root).unwrap();
        for (name, bytes) in [
            ("stdout", b"out".as_slice()),
            ("stderr", b"error"),
            ("status.json", b"{}"),
            ("trace.7", b"synthetic raw trace"),
        ] {
            fs::write(root.join(name), bytes).unwrap();
        }
        let output = dir.path().join("snapshot");
        Self {
            _dir: dir,
            root,
            output,
        }
    }
    fn command(&self, script: &str, args: &[String]) -> Command {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let mut command = Command::new(repo.join(".venv/bin/python"));
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("HOME", self._dir.path())
            .env("TMPDIR", self._dir.path())
            .env("LC_ALL", "C")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .arg("-c")
            .arg(script)
            .arg(repo.join("xtask/tests/fixtures/linux-observer/export.py"))
            .arg(&self.root)
            .arg(unsafe { libc::geteuid() }.to_string())
            .arg(serde_json::to_string(args).unwrap());
        command
    }
    fn call(&self, args: &[String]) -> Result<String, String> {
        let (_, code, stdout, stderr) = capture_actions::execute(self.command(PRODUCER, args))?;
        if code != Some(0) {
            return Err(format!("exporter exit {code:?}: {stderr}"));
        }
        Ok(stdout)
    }
    fn manifest(&self) -> Value {
        serde_json::from_str(&self.call(&["--export-manifest".into()]).unwrap()).unwrap()
    }
    fn export(&self) -> Result<Value, String> {
        transport::export_with_uid(&self.output, unsafe { libc::geteuid() }, |args| {
            self.call(args)
        })
    }
}
fn refused(result: Result<Value, String>) {
    assert!(
        !result.is_ok_and(|value| value["raw_evidence_complete"] == true),
        "unsafe transport qualified"
    );
}
fn mutate_chunk(fixture: &Fixture, mut mutation: impl FnMut(&mut Value)) {
    refused(transport::export_with_uid(
        &fixture.output,
        unsafe { libc::geteuid() },
        |args| {
            let wire = fixture.call(args)?;
            if args[0] == "--export-chunk" {
                let mut value: Value = serde_json::from_str(&wire).unwrap();
                mutation(&mut value);
                Ok(value.to_string())
            } else {
                Ok(wire)
            }
        },
    ));
}
#[test]
fn t01_registered_maximum_local_roundtrip() {
    let fixture = Fixture::new();
    for (name, size) in [
        ("trace.7", 8 * 1048576),
        ("stdout", 1048576),
        ("stderr", 1048576),
        ("status.json", 1048576),
    ] {
        fs::write(
            fixture.root.join(name),
            (0..size).map(|i| (i % 256) as u8).collect::<Vec<_>>(),
        )
        .unwrap();
    }
    let mut calls = 0;
    let mut largest = 0;
    let result = transport::export_with_uid(&fixture.output, unsafe { libc::geteuid() }, |args| {
        let wire = fixture.call(args)?;
        if args[0] == "--export-chunk" {
            calls += 1;
            largest = largest.max(wire.len());
        }
        Ok(wire)
    })
    .unwrap();
    assert_eq!(result["raw_evidence_complete"], true);
    assert!(calls <= 128 && largest <= 768 * 1024);
    for name in ["trace.7", "stdout", "stderr", "status.json"] {
        assert_eq!(
            fs::read(fixture.output.join(name)).unwrap(),
            fs::read(fixture.root.join(name)).unwrap()
        );
        assert_eq!(
            fs::metadata(fixture.output.join(name))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
    assert_eq!(
        fs::metadata(&fixture.output).unwrap().permissions().mode() & 0o777,
        0o700
    );
}
#[test]
fn t02_incomplete_manifest_retains_available_safe_files() {
    for mode in ["missing", "empty", "unavailable"] {
        let fixture = Fixture::new();
        if mode == "empty" {
            for entry in fs::read_dir(&fixture.root).unwrap() {
                fs::remove_file(entry.unwrap().path()).unwrap();
            }
        } else {
            fs::remove_file(fixture.root.join("status.json")).unwrap();
        }
        let result =
            transport::export_with_uid(&fixture.output, unsafe { libc::geteuid() }, |args| {
                if mode == "unavailable" {
                    Err("owned exporter unavailable".into())
                } else {
                    fixture.call(args)
                }
            });
        refused(result);
        if mode == "missing" {
            assert_eq!(fs::read(fixture.output.join("stderr")).unwrap(), b"error");
        }
    }
}
#[test]
fn t03_names_counts_and_duplicate_manifest_entries() {
    for name in ["unexpected", "trace.0", "trace.bad", "trace.01"] {
        let fixture = Fixture::new();
        fs::write(fixture.root.join(name), b"unsafe").unwrap();
        refused(fixture.export());
        assert!(!fixture.output.join(name).exists());
    }
    for name in ["../outside", "/absolute", "trace.0"] {
        let fixture = Fixture::new();
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |args| {
                let mut value: Value = serde_json::from_str(&fixture.call(args)?).unwrap();
                if args[0] == "--export-manifest" {
                    value["files"][0]["name"] = json!(name);
                }
                Ok(value.to_string())
            },
        ));
    }
    let fixture = Fixture::new();
    refused(transport::export_with_uid(
        &fixture.output,
        unsafe { libc::geteuid() },
        |args| {
            let mut value: Value = serde_json::from_str(&fixture.call(args)?).unwrap();
            if args[0] == "--export-manifest" {
                let duplicate = value["files"][0].clone();
                value["files"].as_array_mut().unwrap().push(duplicate);
            }
            Ok(value.to_string())
        },
    ));
    let fixture = Fixture::new();
    for pid in 10..42 {
        fs::write(fixture.root.join(format!("trace.{pid}")), b"x").unwrap();
    }
    refused(fixture.export());
}
#[test]
fn t04_real_unsafe_file_types_and_host_destination_aliases() {
    for kind in ["symlink", "hardlink", "directory", "fifo"] {
        let fixture = Fixture::new();
        let peer = fixture.root.join("trace.8");
        match kind {
            "symlink" => symlink(fixture.root.join("stderr"), &peer).unwrap(),
            "hardlink" => fs::hard_link(fixture.root.join("stderr"), &peer).unwrap(),
            "directory" => fs::create_dir(&peer).unwrap(),
            _ => {
                let path = std::ffi::CString::new(peer.as_os_str().as_bytes()).unwrap();
                assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
            }
        }
        refused(fixture.export());
        assert!(!fixture.output.join("trace.8").exists());
    }
    let fixture = Fixture::new();
    let outside = fixture._dir.path().join("outside");
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"unchanged").unwrap();
    symlink(&outside, &fixture.output).unwrap();
    refused(fixture.export());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"unchanged");
    let fixture = Fixture::new();
    fs::create_dir(&fixture.output).unwrap();
    fs::write(fixture.output.join("stderr"), b"prior").unwrap();
    refused(fixture.export());
    assert_eq!(fs::read(fixture.output.join("stderr")).unwrap(), b"prior");
}
#[test]
fn t05_payload_and_wire_boundaries() {
    for name in ["stdout", "stderr", "status.json", "trace.7"] {
        let fixture = Fixture::new();
        let bound = if name == "trace.7" {
            8 * 1048576
        } else {
            1048576
        };
        fs::write(fixture.root.join(name), vec![255; bound]).unwrap();
        assert!(fixture.manifest()["issues"].as_array().unwrap().is_empty());
        fs::write(fixture.root.join(name), vec![255; bound + 1]).unwrap();
        refused(fixture.export());
    }
    let fixture = Fixture::new();
    fs::write(fixture.root.join("trace.7"), vec![1; 8 * 1048576]).unwrap();
    fs::write(fixture.root.join("trace.8"), b"x").unwrap();
    refused(fixture.export());
    for (which, cap) in [
        ("--export-manifest", 64 * 1024),
        ("--export-chunk", 768 * 1024),
    ] {
        for over in [false, true] {
            let fixture = Fixture::new();
            let result =
                transport::export_with_uid(&fixture.output, unsafe { libc::geteuid() }, |args| {
                    let mut wire = fixture.call(args)?;
                    if args[0] == which {
                        wire.extend(std::iter::repeat_n(
                            ' ',
                            cap + usize::from(over) - wire.len(),
                        ));
                    }
                    Ok(wire)
                });
            if over {
                refused(result);
            } else {
                assert_eq!(result.unwrap()["raw_evidence_complete"], true);
            }
        }
    }
    let fixture = Fixture::new();
    for index in 0..65 {
        fs::write(fixture.root.join(format!("unknown-{index}")), b"x").unwrap();
    }
    refused(fixture.export());
    let fixture = Fixture::new();
    mutate_chunk(&fixture, |v| {
        v["data"] = json!(vec![0; CHUNK + 1]);
        v["length"] = json!(CHUNK + 1);
    });
}
#[test]
fn t06_actual_source_mutation_replacement_and_during_read() {
    for replace in [false, true] {
        let fixture = Fixture::new();
        let mut changed = false;
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |args| {
                if !changed && args[0] == "--export-chunk" {
                    changed = true;
                    if replace {
                        fs::remove_file(fixture.root.join(&args[1])).unwrap();
                    }
                    fs::write(fixture.root.join(&args[1]), b"changed").unwrap();
                }
                fixture.call(args)
            },
        ));
    }
    let fixture = Fixture::new();
    let script = r#"
import importlib.util, json, sys
from pathlib import Path
spec=importlib.util.spec_from_file_location('fixed_export',sys.argv[1]); m=importlib.util.module_from_spec(spec); spec.loader.exec_module(m)
root=Path(sys.argv[2]); uid=int(sys.argv[3]); manifest=m.manifest(root,uid)
entry=next(e for e in manifest['files'] if e['name']=='trace.7')
def change(): (root/'trace.7').write_bytes(b'changed during actual read')
m.chunk(root,uid,'trace.7',0,entry['identity']['size'],entry['identity'],before_read=change)
"#;
    let (_, code, _, _) = capture_actions::execute(fixture.command(script, &[])).unwrap();
    assert_ne!(code, Some(0));
}
#[test]
fn t07_chunk_offsets_lengths_order_and_integer_bounds() {
    for field in ["offset", "length"] {
        for value in [
            json!(-1),
            json!(18446744073709551615u64),
            json!("overflow"),
            json!(1),
        ] {
            let fixture = Fixture::new();
            mutate_chunk(&fixture, |v| v[field] = value.clone());
        }
    }
    for offset in [0, 2 * CHUNK] {
        let fixture = Fixture::new();
        fs::write(fixture.root.join("trace.7"), vec![7; CHUNK + 1]).unwrap();
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |args| {
                let mut v: Value = serde_json::from_str(&fixture.call(args)?).unwrap();
                if args[0] == "--export-chunk" && args[1] == "trace.7" && args[2] != "0" {
                    v["offset"] = json!(offset);
                }
                Ok(v.to_string())
            },
        ));
    }
    let fixture = Fixture::new();
    mutate_chunk(&fixture, |v| {
        v["data"].as_array_mut().unwrap().pop();
    });
}
#[test]
fn t08_wire_schema_and_byte_values() {
    for bad in ["{", "[]", "null", "{} trailing"] {
        let fixture = Fixture::new();
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |_| Ok(bad.into()),
        ));
    }
    for which in ["--export-manifest", "--export-chunk"] {
        let fixture = Fixture::new();
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |args| {
                let mut v: Value = serde_json::from_str(&fixture.call(args)?).unwrap();
                if args[0] == which {
                    v["unexpected"] = json!(true);
                }
                Ok(v.to_string())
            },
        ));
    }
    for bad in [json!(256), json!(-1), json!(1.5), json!("byte")] {
        let fixture = Fixture::new();
        mutate_chunk(&fixture, |v| v["data"][0] = bad.clone());
    }
    let fixture = Fixture::new();
    mutate_chunk(&fixture, |v| v["name"] = json!("wrong"));
    let fixture = Fixture::new();
    mutate_chunk(&fixture, |v| v["identity"]["inode"] = json!(0));
}
#[test]
fn t09_chunk_and_complete_file_hashes() {
    let fixture = Fixture::new();
    mutate_chunk(&fixture, |v| v["sha256"] = json!("0".repeat(64)));
    let fixture = Fixture::new();
    refused(transport::export_with_uid(
        &fixture.output,
        unsafe { libc::geteuid() },
        |args| {
            let mut v: Value = serde_json::from_str(&fixture.call(args)?).unwrap();
            if args[0] == "--export-manifest" {
                for f in v["files"].as_array_mut().unwrap() {
                    f["sha256"] = json!("0".repeat(64));
                }
            }
            Ok(v.to_string())
        },
    ));
}
#[test]
fn t10_actual_owned_child_failures_and_unavailable_transport() {
    let fixture = Fixture::new();
    for (script, budget, deadline) in [
        (
            "import sys;print('{',end='',flush=True);sys.exit(7)",
            Duration::from_secs(2),
            false,
        ),
        (
            "import time;print('{',end='',flush=True);time.sleep(5)",
            Duration::from_millis(30),
            true,
        ),
    ] {
        let result = capture_actions::execute_during_observed(
            fixture.command(script, &[]),
            budget,
            |_, _| Ok(()),
        );
        let observation = if deadline {
            let failure = result.unwrap_err();
            assert_eq!(failure.cause, capture_actions::OwnerFailureCause::Deadline);
            failure.observation.unwrap()
        } else {
            let run = result.unwrap();
            assert_eq!(run.stdout, "{");
            assert_eq!(run.observation.wait_code, Some(7));
            run.observation
        };
        assert!(observation.wait_observed && observation.reaped && observation.cleanup_succeeded);
    }
    refused(transport::export_with_uid(
        &fixture.output,
        unsafe { libc::geteuid() },
        |_| Err("owned stopped exporter".into()),
    ));
}
#[test]
fn t11_safe_peer_retention_before_owned_cleanup() {
    let fixture = Fixture::new();
    symlink(fixture.root.join("stderr"), fixture.root.join("trace.8")).unwrap();
    fs::remove_file(fixture.root.join("status.json")).unwrap();
    let result = fixture.export().unwrap();
    assert_eq!(result["raw_evidence_complete"], false);
    assert!(!result["issues"].as_array().unwrap().is_empty());
    assert_eq!(fs::read(fixture.output.join("stderr")).unwrap(), b"error");
    fs::remove_dir_all(&fixture.root).unwrap();
    assert_eq!(fs::read(fixture.output.join("stderr")).unwrap(), b"error");
    assert!(!fixture.output.join("trace.8").exists());
}
#[test]
fn t12_final_manifest_drift_and_export_only_commands() {
    if unsafe { libc::geteuid() } != 65534 {
        let fixture = Fixture::new();
        refused(transport::export(&fixture.output, |args| {
            fixture.call(args)
        }));
    }
    for name in ["status.json", "trace.7"] {
        let fixture = Fixture::new();
        let mut manifests = 0;
        refused(transport::export_with_uid(
            &fixture.output,
            unsafe { libc::geteuid() },
            |args| {
                assert!(
                    matches!(args[0].as_str(), "--export-manifest" | "--export-chunk"),
                    "export attempted release or unrelated action"
                );
                if args[0] == "--export-manifest" {
                    manifests += 1;
                    if manifests == 2 {
                        fs::write(fixture.root.join(name), b"changed after chunks").unwrap();
                    }
                }
                fixture.call(args)
            },
        ));
        assert_eq!(manifests, 2);
    }
}
