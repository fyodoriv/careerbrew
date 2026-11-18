//! Controls for `migration verify`: a registered step with no Rust candidate
//! is refused; with the test-only candidate flag, a candidate that matches the
//! oracle passes, one with a different stdout fails naming the case, and one
//! with the same stdout but a nonzero exit fails, and one that prints
//! invalid UTF-8 fails its case with the record written. Refusals: an unknown
//! step, a relative `--out`, an oracle that exits nonzero and an empty
//! CAREERBREW_HOME. The passing test also checks that a second run refuses
//! its existing output. Contract controls: a candidate that serves a wrong
//! HTTP body (and prints the canary) fails its HTTP cases with authorization
//! and privacy counts; an extra or a missing entry in the synthetic home
//! fails the CLI case with side_effect or missing_writes counts. Each test
//! owns its temp folder.
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const XTASK: &str = env!("CARGO_BIN_EXE_careerbrew-xtask");

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn python() -> PathBuf {
    root().join(".venv/bin/python")
}

fn verify(step: &str, out: &Path) -> Command {
    let mut command = Command::new(XTASK);
    command
        .args(["migration", "verify", "--step", step, "--python"])
        .arg(python())
        .arg("--out")
        .arg(out)
        .env_remove("CAREERBREW_HOME");
    command
}

/// Writes an executable shell candidate with the given body.
fn candidate(dir: &Path, body: &str) -> PathBuf {
    let path = dir.join("candidate");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

/// The flag verify passes when it starts a candidate's HTTP contract server.
const HTTP_FLAG: &str = "--careerbrew-http-contract";

/// A candidate that runs the Python oracle for both contracts, then the
/// given extra shell lines after a CLI run.
fn oracle_candidate(dir: &Path, extra: &str) -> PathBuf {
    let python = python();
    let driver = root().join("xtask/tests/fixtures/capture/http_driver.py");
    candidate(
        dir,
        &format!(
            "if [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\n'{}' -B -m careerbrew \"$@\"\nstatus=$?\n{extra}\nexit $status",
            python.display(),
            driver.display(),
            python.display(),
        ),
    )
}

fn read(out: &Path) -> serde_json::Value {
    serde_json::from_slice(&std::fs::read(out).unwrap()).unwrap()
}

#[test]
fn a_step_with_no_candidate_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-03", &out).output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("step rust-03 has no Rust candidate yet; verify refused"));
    assert!(!out.exists());
}

#[test]
fn a_candidate_identical_to_the_oracle_passes() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let same = oracle_candidate(scratch.path(), "");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(String::from_utf8_lossy(&result.stdout)
        .contains("step=rust-03 cases=22 passed=22 failed=none"));
    let value = read(&out);
    assert_eq!(value["schema"], "careerbrew-migration-verify-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["case_count"], 22);
    assert_eq!(value["passed"], true);
    assert_eq!(value["cases"][0]["case"], "actions-json");
    assert_eq!(value["cases"][0]["kind"], "cli");
    assert_eq!(value["cases"][0]["passed"], true);
    assert_eq!(value["cases"][0]["writes_match"], true);
    assert_eq!(value["cases"][1]["case"], "http:actions");
    assert_eq!(value["cases"][1]["kind"], "http");
    assert_eq!(value["cases"][1]["body_matches"], true);
    // The record is decide's verify input: checks and every violation count.
    assert_eq!(value["checks"].as_array().unwrap().len(), 22);
    assert_eq!(
        value["checks"][0],
        serde_json::json!({"id": "actions-json", "passed": true})
    );
    for name in [
        "contract",
        "privacy",
        "authorization",
        "role",
        "side_effect",
        "missing_writes",
        "duplicate_writes",
    ] {
        assert_eq!(value["violations"][name], 0, "{name}");
    }
    assert_eq!(
        value["unobserved_violations"],
        serde_json::json!(["duplicate_writes", "role"])
    );
    assert_eq!(value["source_revision"].as_str().unwrap().len(), 40);
    assert_eq!(value["sources_sha256"].as_str().unwrap().len(), 64);
    assert_eq!(value["test_candidate"], true);
    assert_eq!(value["candidate_sha256"].as_str().unwrap().len(), 64);
    // Content-free: no local path of this machine.
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains(scratch.path().to_str().unwrap()));
    assert!(!text.contains(root().to_str().unwrap()));
    // A second run never overwrites a record.
    let before = std::fs::read(&out).unwrap();
    let again = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert_eq!(again.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&again.stderr).contains("verify output already exists"));
    assert_eq!(std::fs::read(&out).unwrap(), before);
}

#[test]
fn a_planted_different_output_fails_naming_the_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let wrong = candidate(scratch.path(), "printf '[]'");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&wrong)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("actions-json"));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["failed_cases"][0], "actions-json");
    assert_eq!(value["cases"][0]["stdout_matches"], false);
    assert_eq!(value["cases"][0]["exit_matches"], true);
}

#[test]
fn a_planted_nonzero_candidate_fails() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let nonzero = candidate(
        scratch.path(),
        &format!("'{}' -B -m careerbrew \"$@\"\nexit 3", python().display()),
    );
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&nonzero)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["cases"][0]["exit_matches"], false);
    assert_eq!(value["cases"][0]["stdout_matches"], true);
}

#[test]
fn an_unknown_step_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-99", &out).output().unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("unknown step id"));
    assert!(!out.exists());
}

#[test]
fn a_relative_out_is_refused() {
    let result = verify("rust-03", Path::new("verify.json"))
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("--out must be an absolute path"));
}

#[test]
fn a_candidate_with_invalid_utf8_output_fails_its_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let bad = candidate(scratch.path(), "printf '\\377\\376'");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&bad)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("actions-json"));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(value["failed_cases"][0], "actions-json");
    assert_eq!(value["cases"][0]["candidate_ran"], false);
    assert_eq!(value["cases"][0]["passed"], false);
}

#[test]
fn a_nonzero_oracle_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let python = scratch.path().join("python");
    std::fs::write(&python, "#!/bin/sh\nexit 7\n").unwrap();
    std::fs::set_permissions(&python, std::fs::Permissions::from_mode(0o700)).unwrap();
    let same = candidate(scratch.path(), "exit 0");
    let out = scratch.path().join("verify.json");
    let result = Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-03", "--python"])
        .arg(&python)
        .arg("--out")
        .arg(&out)
        .arg("--test-candidate")
        .arg(&same)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("did not exit 0"));
    assert!(!out.exists());
}

#[test]
fn an_empty_careerbrew_home_is_refused() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let result = verify("rust-03", &out)
        .env("CAREERBREW_HOME", "")
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("CAREERBREW_HOME is set but empty"));
    assert!(!out.exists());
}

/// A planted HTTP server that answers 200 with a wrong body to every request
/// and prints the synthetic canary it was given.
const WRONG_SERVER: &str = r#"
import http.server, json, os, pathlib, threading, time
control = json.loads(pathlib.Path(os.environ["CAREERBREW_CAPTURE_INPUT"]).read_text())
protocol = pathlib.Path(control["protocol"])
print(control["canary"], flush=True)
class Handler(http.server.BaseHTTPRequestHandler):
    def answer(self):
        length = int(self.headers.get("Content-Length") or 0)
        if length <= 64000:
            self.rfile.read(length)
        body = b'{"ok":false}'
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)
    do_GET = answer
    do_POST = answer
    def log_message(self, *args):
        pass
server = http.server.HTTPServer(("127.0.0.1", 0), Handler)
threading.Thread(target=server.serve_forever, daemon=True).start()
ready = protocol / ".ready.tmp"
ready.write_text(json.dumps({"pid": os.getpid(), "host": "127.0.0.1", "port": server.server_address[1]}))
os.replace(ready, protocol / "ready.json")
while not (protocol / "stop").exists():
    time.sleep(0.01)
"#;

#[test]
fn a_planted_wrong_http_body_fails_its_http_cases() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let server = scratch.path().join("wrong_server.py");
    std::fs::write(&server, WRONG_SERVER).unwrap();
    let wrong = candidate(
        scratch.path(),
        &format!(
            "if [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\nexec '{}' -B -m careerbrew \"$@\"",
            python().display(),
            server.display(),
            python().display(),
        ),
    );
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&wrong)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    assert_eq!(value["passed"], false);
    assert_eq!(
        value["cases"][0]["passed"], true,
        "the CLI case still passes"
    );
    let ping = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case"] == "http:ping")
        .unwrap()
        .clone();
    assert_eq!(ping["status_matches"], true);
    assert_eq!(ping["body_matches"], false);
    assert_eq!(ping["passed"], false);
    assert!(value["failed_cases"]
        .as_array()
        .unwrap()
        .contains(&serde_json::json!("http:ping")));
    assert!(value["violations"]["contract"].as_u64().unwrap() > 0);
    // 401 and 403 cases answered 200: each is an authorization violation.
    assert!(value["violations"]["authorization"].as_u64().unwrap() > 0);
    assert_eq!(value["violations"]["privacy"], 1);
    // Content-free: the canary never reaches the record.
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("synthetic-http-capture-canary"));
}

#[test]
fn a_planted_extra_write_fails_the_cli_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let extra = oracle_candidate(scratch.path(), ": > \"$CAREERBREW_HOME/planted-extra\"");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&extra)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    assert_eq!(value["failed_cases"], serde_json::json!(["actions-json"]));
    let case = &value["cases"][0];
    assert_eq!(case["stdout_matches"], true);
    assert_eq!(case["writes_match"], false);
    assert_eq!(case["extra_writes"], 1);
    assert_eq!(case["missing_writes"], 0);
    assert_eq!(value["violations"]["side_effect"], 1);
    assert_eq!(value["violations"]["missing_writes"], 0);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("planted-extra"));
}

#[test]
fn a_planted_missing_write_fails_the_cli_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let missing = oracle_candidate(scratch.path(), "rmdir \"$XDG_CACHE_HOME\"");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&missing)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    assert_eq!(value["failed_cases"], serde_json::json!(["actions-json"]));
    let case = &value["cases"][0];
    assert_eq!(case["writes_match"], false);
    assert_eq!(case["missing_writes"], 1);
    assert_eq!(case["extra_writes"], 0);
    assert_eq!(value["violations"]["missing_writes"], 1);
    assert_eq!(value["violations"]["side_effect"], 0);
}
