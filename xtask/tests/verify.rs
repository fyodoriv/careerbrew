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
//! candidate fails every HTTP case and counts the denied ones. DATA and role
//! controls: a candidate that writes a wrong data value fails its data case
//! with a contract count; one that writes a data path again fails with a
//! duplicate_writes count; one that runs a job-hunt command on a corporate
//! machine fails the role case with a role count. Each test owns its temp
//! folder.
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

/// The flag verify passes when it runs one phase of the DATA contract.
const DATA_FLAG: &str = "--careerbrew-data-contract";

/// Shell lines that run a DATA contract phase with the Python data driver.
/// `before` runs first (it may export a control); `after` runs after the
/// driver, with `$2` the case id and `$3` the role.
fn data_dispatch(before: &str, after: &str) -> String {
    format!(
        "if [ \"$1\" = '{DATA_FLAG}' ]; then shift\n{before}\n'{}' -B -S '{}' \"$@\"\nstatus=$?\n{after}\nexit $status\nfi",
        python().display(),
        root()
            .join("xtask/tests/fixtures/capture/data_driver.py")
            .display(),
    )
}

/// A candidate that runs the Python oracle for every contract, then the
/// given extra shell lines after a CLI run.
fn oracle_candidate(dir: &Path, extra: &str) -> PathBuf {
    let python = python();
    let driver = root().join("xtask/tests/fixtures/capture/http_driver.py");
    candidate(
        dir,
        &format!(
            "{}\nif [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\n'{}' -B -m careerbrew \"$@\"\nstatus=$?\n{extra}\nexit $status",
            data_dispatch("", ""),
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
        .contains("step=rust-03 cases=34 passed=34 failed=none"));
    let value = read(&out);
    assert_eq!(value["schema"], "careerbrew-migration-verify-v1");
    assert_eq!(value["step"], "rust-03");
    assert_eq!(value["case_count"], 34);
    assert_eq!(value["passed"], true);
    assert_eq!(value["cases"][0]["case"], "actions-json");
    assert_eq!(value["cases"][0]["kind"], "cli");
    assert_eq!(value["cases"][0]["passed"], true);
    assert_eq!(value["cases"][0]["writes_match"], true);
    assert_eq!(value["cases"][1]["case"], "http:actions");
    assert_eq!(value["cases"][1]["kind"], "http");
    assert_eq!(value["cases"][1]["body_matches"], true);
    // The record is decide's verify input: checks and every violation count.
    assert_eq!(value["checks"].as_array().unwrap().len(), 34);
    let find = |id: &str| {
        value["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|case| case["case"] == id)
            .unwrap_or_else(|| panic!("{id}"))
            .clone()
    };
    // The HTTP side effects are a case of their own.
    assert_eq!(find("http:side-effects")["passed"], true);
    // The DATA contract: one case per corpus case, every phase compared.
    for id in 1..=10 {
        let case = find(&format!("data:{id}"));
        assert_eq!(case["kind"], "data");
        assert_eq!(case["passed"], true, "data:{id}");
        assert_eq!(case["calls_match"], true);
        assert_eq!(case["duplicate_writes"], 0);
    }
    assert_eq!(find("data:3")["phases"], 2);
    // The corporate-role case: the job-hunt command is refused on both sides.
    let role = find("role:jobs-list");
    assert_eq!(role["kind"], "role");
    assert_eq!(role["passed"], true);
    assert_eq!(role["oracle_refused"], true);
    // Every contract is checked and every violation kind is observed.
    assert_eq!(value["missing_contracts"], serde_json::json!([]));
    assert_eq!(
        value["contracts"],
        serde_json::json!(["cli", "side-effect", "http", "data", "role"])
    );
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
    assert_eq!(value["unobserved_violations"], serde_json::json!([]));
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
    let extra = oracle_candidate(
        scratch.path(),
        "if [ \"$1\" = actions ]; then : > \"$CAREERBREW_HOME/planted-extra\"; fi",
    );
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
            "#!/bin/sh\nif [ \"$2\" != '-m' ] || [ \"$4\" != actions ]; then exec '{}' \"$@\"; fi\n'{}' \"$@\"\nstatus=$?\n: > \"$CAREERBREW_HOME/oracle-made\"\nexit $status\n",
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
            "{}\nif [ \"$1\" = '{HTTP_FLAG}' ]; then : > \"$CAREERBREW_HOME/planted-http\"; exec '{}' -B '{}'; fi\nexec '{}' -B -m careerbrew \"$@\"",
            data_dispatch("", ""),
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

/// A candidate body that serves the Python HTTP and DATA drivers for their
/// flags, runs the Python CLI for the role case, and runs `body` otherwise.
fn http_then(body: &str) -> String {
    format!(
        "{}\nif [ \"$1\" = jobs ]; then exec '{}' -B -m careerbrew \"$@\"; fi\nif [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\n{body}",
        data_dispatch("", ""),
        python().display(),
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

/// Runs verify with the given candidate and returns its exit code and record.
fn verify_with(dir: &Path, path: &Path) -> (Option<i32>, serde_json::Value) {
    let out = dir.join("verify.json");
    let result = verify("rust-03", &out)
        .arg("--test-candidate")
        .arg(path)
        .output()
        .unwrap();
    assert!(out.exists(), "{}", String::from_utf8_lossy(&result.stderr));
    (result.status.code(), read(&out))
}

/// A candidate with the given DATA dispatch first; HTTP and CLI go to Python.
fn data_candidate(dir: &Path, dispatch: &str) -> PathBuf {
    candidate(
        dir,
        &format!(
            "{dispatch}\nif [ \"$1\" = '{HTTP_FLAG}' ]; then exec '{}' -B '{}'; fi\nexec '{}' -B -m careerbrew \"$@\"",
            python().display(),
            root()
                .join("xtask/tests/fixtures/capture/http_driver.py")
                .display(),
            python().display(),
        ),
    )
}

#[test]
fn a_planted_wrong_data_value_fails_its_data_case() {
    // The writer of case 3 serializes a changed record; its reader then
    // returns another value than the oracle's.
    let scratch = tempfile::tempdir().unwrap();
    let wrong = data_candidate(
        scratch.path(),
        &data_dispatch(
            "if [ \"$2\" = 3 ] && [ \"$3\" = writer ]; then CAREERBREW_DATA_CONTRACT_CONTROL=json-one; export CAREERBREW_DATA_CONTRACT_CONTROL; fi",
            "",
        ),
    );
    let (code, value) = verify_with(scratch.path(), &wrong);
    assert_eq!(code, Some(1));
    // Case 9 rewrites the same record "if changed", so the wrong record also
    // fails it: the defect shows first in case 3 and carries on.
    assert_eq!(value["failed_cases"][0], "data:3");
    assert!(value["violations"]["contract"].as_u64().unwrap() > 0);
    let case = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["case"] == "data:3")
        .unwrap()
        .clone();
    assert_eq!(case["calls_match"], false);
}

#[test]
fn a_planted_duplicate_write_fails_its_data_case() {
    // The reader of case 2 writes its target again (same bytes, new mtime):
    // the final state matches, only the write trace sees it.
    let scratch = tempfile::tempdir().unwrap();
    let twice = data_candidate(
        scratch.path(),
        &data_dispatch(
            "",
            "if [ \"$2\" = 2 ] && [ \"$3\" = reader ]; then /usr/bin/touch \"$CAREERBREW_DATA_OWNED/data/contract/text.txt\"; fi",
        ),
    );
    let (code, value) = verify_with(scratch.path(), &twice);
    assert_eq!(code, Some(1));
    assert_eq!(value["failed_cases"], serde_json::json!(["data:2"]));
    assert!(value["violations"]["duplicate_writes"].as_u64().unwrap() > 0);
    assert_eq!(value["violations"]["contract"], 0);
    assert_eq!(value["violations"]["side_effect"], 0);
}

#[test]
fn a_planted_role_bypass_fails_the_role_case() {
    // On a corporate machine the candidate runs a job-hunt command and
    // exits 0 where the oracle refuses.
    let scratch = tempfile::tempdir().unwrap();
    let bypass = data_candidate(
        scratch.path(),
        &format!(
            "{}\nif [ \"$1\" = jobs ]; then exit 0; fi",
            data_dispatch("", "")
        ),
    );
    let (code, value) = verify_with(scratch.path(), &bypass);
    assert_eq!(code, Some(1));
    assert_eq!(value["failed_cases"], serde_json::json!(["role:jobs-list"]));
    assert_eq!(value["violations"]["role"], 1);
    assert_eq!(value["violations"]["contract"], 0);
}

#[test]
fn a_planted_canary_leak_counts_once_in_the_case_that_leaked() {
    // The reader of case 2 copies the synthetic canary into a new file. The
    // file stays for cases 3 to 10, but only case 2 leaked it.
    let scratch = tempfile::tempdir().unwrap();
    let leak = data_candidate(
        scratch.path(),
        &data_dispatch(
            "",
            "if [ \"$2\" = 2 ] && [ \"$3\" = reader ]; then printf 'SYNTHETIC-DATA-CANARY-8H3' > \"$CAREERBREW_DATA_OWNED/leak.txt\"; fi",
        ),
    );
    let (code, value) = verify_with(scratch.path(), &leak);
    assert_eq!(code, Some(1));
    assert_eq!(value["failed_cases"], serde_json::json!(["data:2"]));
    assert_eq!(value["violations"]["privacy"], 1);
    let leaked: Vec<_> = value["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|case| case["canary_leaked"] == true)
        .map(|case| case["case"].clone())
        .collect();
    assert_eq!(leaked, vec![serde_json::json!("data:2")]);
}
