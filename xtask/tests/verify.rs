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
//! fails the CLI case with side_effect or missing_writes counts; an HTTP
//! candidate that writes fails the `http:side-effects` case; a dead HTTP
//! candidate fails every HTTP case and counts the denied ones. Each test
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
        .contains("step=rust-03 cases=23 passed=23 failed=none"));
    let value = read(&out);
    assert_eq!(value["schema"], "careerbrew-migration-verify-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["case_count"], 23);
    assert_eq!(value["passed"], true);
    assert_eq!(value["cases"][0]["case"], "actions-json");
    assert_eq!(value["cases"][0]["kind"], "cli");
    assert_eq!(value["cases"][0]["passed"], true);
    assert_eq!(value["cases"][0]["writes_match"], true);
    assert_eq!(value["cases"][1]["case"], "http:actions");
    assert_eq!(value["cases"][1]["kind"], "http");
    assert_eq!(value["cases"][1]["body_matches"], true);
    // The record is decide's verify input: checks and every violation count.
    assert_eq!(value["checks"].as_array().unwrap().len(), 23);
    // The HTTP side effects are a case of their own.
    let last = value["cases"].as_array().unwrap().last().unwrap().clone();
    assert_eq!(last["case"], "http:side-effects");
    assert_eq!(last["passed"], true);
    // The DATA contract is not verified yet: decide must see it is missing.
    assert_eq!(value["missing_contracts"], serde_json::json!(["data"]));
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
    // The oracle itself writes a file in its home; the candidate does not.
    // Setup entries are the same on both sides, so only the oracle's own
    // write can make this case fail.
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let wrapper = scratch.path().join("oracle-python");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\nif [ \"$2\" != '-m' ]; then exec '{}' \"$@\"; fi\n'{}' \"$@\"\nstatus=$?\n: > \"$CAREERBREW_HOME/oracle-made\"\nexit $status\n",
            python().display(),
            python().display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o700)).unwrap();
    let same = oracle_candidate(scratch.path(), "");
    let result = Command::new(XTASK)
        .args(["migration", "verify", "--step", "rust-03", "--python"])
        .arg(&wrapper)
        .arg("--out")
        .arg(&out)
        .arg("--test-candidate")
        .arg(&same)
        .env_remove("CAREERBREW_HOME")
        .output()
        .unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert!(out.exists(), "{}", String::from_utf8_lossy(&result.stderr));
    let value = read(&out);
    assert_eq!(value["failed_cases"], serde_json::json!(["actions-json"]));
    let case = &value["cases"][0];
    assert_eq!(case["writes_match"], false);
    assert_eq!(case["missing_writes"], 1);
    assert_eq!(case["extra_writes"], 0);
    assert_eq!(value["violations"]["missing_writes"], 1);
    assert_eq!(value["violations"]["side_effect"], 0);
    let text = std::fs::read_to_string(&out).unwrap();
    assert!(!text.contains("oracle-made"));
}

#[test]
fn a_planted_http_write_fails_the_http_side_effect_case() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let writer = candidate(
        scratch.path(),
        &format!(
            "if [ \"$1\" = '{HTTP_FLAG}' ]; then : > \"$CAREERBREW_HOME/planted-http\"; exec '{}' -B '{}'; fi\nexec '{}' -B -m careerbrew \"$@\"",
            python().display(),
            root().join("xtask/tests/fixtures/capture/http_driver.py").display(),
            python().display(),
        ),
    );
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&writer)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&result.stderr).contains("http:side-effects"));
    let value = read(&out);
    assert_eq!(
        value["failed_cases"],
        serde_json::json!(["http:side-effects"])
    );
    assert_eq!(value["violations"]["side_effect"], 1);
    assert_eq!(value["violations"]["contract"], 0);
    let check = value["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|check| check["id"] == "http:side-effects")
        .unwrap()
        .clone();
    assert_eq!(check["passed"], false);
}

/// A planted server that publishes readiness, then exits at once.
const DEAD_SERVER: &str = r#"
import json, os, pathlib
control = json.loads(pathlib.Path(os.environ["CAREERBREW_CAPTURE_INPUT"]).read_text())
protocol = pathlib.Path(control["protocol"])
ready = protocol / ".ready.tmp"
ready.write_text(json.dumps({"pid": os.getpid(), "host": "127.0.0.1", "port": 9}))
os.replace(ready, protocol / "ready.json")
"#;

#[test]
fn a_dead_http_candidate_fails_every_case_and_counts_denied_ones() {
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let server = scratch.path().join("dead_server.py");
    std::fs::write(&server, DEAD_SERVER).unwrap();
    let dead = candidate(
        scratch.path(),
        &format!(
            "if [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\nexec '{}' -B -m careerbrew \"$@\"",
            python().display(),
            server.display(),
            python().display(),
        ),
    );
    let started = std::time::Instant::now();
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(&dead)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1));
    let value = read(&out);
    let http: Vec<_> = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["kind"] == "http")
        .cloned()
        .collect();
    assert_eq!(http.len(), 21);
    assert!(http.iter().all(|case| case["candidate_answered"] == false));
    // Each case the oracle refuses (401 or 403) is also an authorization
    // violation when the candidate gives no answer.
    assert!(value["violations"]["authorization"].as_u64().unwrap() > 0);
    // A dead server stops the requests at once, not one timeout per case.
    assert!(started.elapsed() < std::time::Duration::from_secs(120));
}

/// A candidate body that serves the Python HTTP driver for the HTTP flag and
/// runs `body` otherwise.
fn http_then(body: &str) -> String {
    format!(
        "if [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\n{body}",
        python().display(),
        root()
            .join("xtask/tests/fixtures/capture/http_driver.py")
            .display(),
    )
}

/// Writes an executable shell script named `name` with the given body.
fn script(dir: &Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn a_registered_rule_normalizes_the_synthetic_home() {
    // rust-03 actions-json registers `synthetic-home`: each side prints its
    // own fresh home, and both read `<home>` after the rule.
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let body = "printf 'home=%s\\n' \"$CAREERBREW_HOME\"";
    let oracle = script(scratch.path(), "oracle", body);
    // The candidate serves the real HTTP oracle: --test-oracle replaces
    // only the CLI oracle, and rust-03 also has the HTTP contract.
    let same = script(scratch.path(), "candidate", &http_then(body));
    let result = verify("rust-03", &out)
        .arg("--test-oracle")
        .arg(&oracle)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(0), "{result:?}");
    let value = read(&out);
    assert_eq!(value["passed"], true);
    assert_eq!(value["test_oracle"], true);
    assert_eq!(
        value["cases"][0]["normalization"],
        serde_json::json!(["synthetic-home"])
    );
}

#[test]
fn an_unregistered_local_path_is_refused() {
    // The repo root has no rule for actions-json: verify refuses, no record.
    let scratch = tempfile::tempdir().unwrap();
    let out = scratch.path().join("verify.json");
    let body = "printf 'root=%s\\n' \"$PWD\"";
    let oracle = script(scratch.path(), "oracle", body);
    let same = script(scratch.path(), "candidate", body);
    let result = verify("rust-03", &out)
        .arg("--test-oracle")
        .arg(&oracle)
        .arg("--test-candidate")
        .arg(&same)
        .output()
        .unwrap();
    assert_eq!(result.status.code(), Some(1), "{result:?}");
    assert!(String::from_utf8_lossy(&result.stderr)
        .contains("case actions-json prints a local path with no registered normalization rule"));
    assert!(!out.exists());
}
