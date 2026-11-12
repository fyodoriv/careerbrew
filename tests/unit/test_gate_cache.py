"""The gate cache: a push right after a green gate reuses it (TASKS slow-pre-push, prepush-reuse-gate).

Each test runs the real scripts/check.sh in a tiny repo with stub steps, so it is fast and offline.
"""

import json
import os
import shlex
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
CONTENT_STEPS = {"docs", "sources", "stories"}
# A fixed past second for file and index mtimes, so "the same second as git add" is exact, not timing luck.
SECOND_NS = 1_700_000_000 * 10**9


def _stub(path: Path, body: str = "exit 0") -> None:
    path.write_text(f"#!/bin/sh\n{body}\n")
    path.chmod(0o755)


class Repo:
    def __init__(self, root: Path, bin_dir: Path):
        self.root = root
        # Shell startup files can replace fixture tools before the child gate runs.
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_") and k not in {"NO_CACHE", "CHECK_EVALS", "BASH_ENV", "ENV"}}
        self.env["PATH"] = f"{bin_dir}{os.pathsep}{self.env['PATH']}"

    def git(self, *args: str) -> None:
        subprocess.run([*GIT, *args], cwd=self.root, env=self.env, check=True, capture_output=True, text=True)

    def run(self, mode: str = "full") -> subprocess.CompletedProcess[str]:
        args = [] if mode == "full" else [mode]
        return subprocess.run(["bash", "scripts/check.sh", *args], cwd=self.root, env=self.env, capture_output=True, text=True, check=False)

    def gate(self, mode: str = "full") -> dict[str, bool]:
        """Run the gate; return {step: cached} for this run."""
        log = self.root / ".metrics" / "events.jsonl"
        seen = len(log.read_text().splitlines()) if log.exists() else 0
        r = self.run(mode)
        assert r.returncode == 0, r.stdout[-500:] + r.stderr[-500:]
        events = [json.loads(line) for line in log.read_text().splitlines()[seen:]]
        return {e["name"]: bool(e.get("cached")) for e in events}


@pytest.fixture
def repo(tmp_path):
    root = tmp_path / "repo"
    (root / "scripts").mkdir(parents=True)
    for name in ("check.sh", "input-key.sh"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    for name in ("check-docs.sh", "leak-check.sh", "e2e.sh"):
        _stub(root / "scripts" / name)
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    for tool in ("uv", "gitleaks"):
        _stub(bin_dir / tool)
    (root / ".gitignore").write_text(".metrics/\n")
    (root / "notes.txt").write_text("one\n")
    r = Repo(root, bin_dir)
    r.git("init", "-q", "-b", "main")
    r.git("add", "-A")
    r.git("commit", "-qm", "init")
    return r


def test_fast_gate_reuses_a_green_full_gate(repo):
    full = repo.gate("full")
    assert "e2e" in full
    assert not any(full.values()), "first run computes every step"
    fast = repo.gate("fast")
    assert fast["leaks"] is False
    assert all(cached for name, cached in fast.items() if name != "leaks"), f"the push hook's fast gate re-ran steps: {fast}"


def test_python_format_lint_and_unit_rerun(repo):
    (repo.root / "pyproject.toml").write_text("# synthetic Python project\n")
    repo.gate("fast")
    after = repo.gate("fast")
    assert after["format"] is False
    assert after["lint"] is False
    assert after["unit"] is False
    assert not (repo.root / ".metrics/cache/unit").exists()
    assert not (repo.root / ".metrics/cache/lint").exists()


def test_javascript_unit_also_reruns(repo):
    (repo.root / "package.json").write_text("{}\n")
    _stub(Path(repo.env["PATH"].split(os.pathsep)[0]) / "npm")
    repo.gate("fast")
    after = repo.gate("fast")
    assert after["unit"] is False
    assert after["lint"] is False and after["types"] is False
    assert not (repo.root / ".metrics/cache/unit").exists()


def test_a_term_only_change_cannot_reuse_a_clean_leak_scan(repo):
    _stub(repo.root / "scripts" / "leak-check.sh", "test ! -f .metrics/synthetic-term-present")
    repo.git("commit", "-qam", "synthetic leak check")
    repo.gate("fast")
    (repo.root / ".metrics" / "synthetic-term-present").write_text("synthetic")
    result = repo.run("fast")
    assert result.returncode != 0
    assert "step 'leaks' failed" in result.stderr


def test_real_scanner_reads_changed_terms_without_a_repo_edit(repo):
    private_root = repo.root.parent / "synthetic-data"
    private_root.mkdir()
    repo.env["CAREERBREW_HOME"] = str(private_root)
    (repo.root / "notes.txt").write_text("SyntheticGateCanary\n")
    _stub(repo.root / "scripts" / "leak-check.sh", f'exec {shlex.quote(sys.executable)} -m careerbrew.cli privacy scan --repo "$PWD"')
    repo.git("commit", "-qam", "synthetic scanner")
    repo.gate("fast")
    (private_root / "sensitive-terms.txt").write_text("SyntheticGateCanary\n")
    result = repo.run("fast")
    assert result.returncode != 0
    assert "step 'leaks' failed" in result.stderr
    assert "SyntheticGateCanary" not in result.stdout + result.stderr


def test_leak_scan_repeats_while_other_clean_steps_are_cached(repo):
    repo.gate("fast")
    after = repo.gate("fast")
    assert after["leaks"] is False
    assert all(cached for name, cached in after.items() if name != "leaks")


def test_committing_the_checked_tree_keeps_the_cache(repo):
    (repo.root / "notes.txt").write_text("two\n")
    assert not any(repo.gate("full").values())
    repo.git("add", "-A")
    repo.git("commit", "-qm", "change")
    after = repo.gate("fast")
    assert all(after[name] for name in CONTENT_STEPS), f"same content, new commit: {after}"


def test_a_newly_tracked_file_reruns_only_the_leak_scan(repo):
    (repo.root / "new.txt").write_text("new\n")
    assert not any(repo.gate("full").values())
    repo.git("add", "-A")
    repo.git("commit", "-qm", "add")
    after = repo.gate("fast")
    assert after["leaks"] is False, "leak-check scans tracked files only, so a newly tracked file must be scanned"
    assert all(after[name] for name in CONTENT_STEPS - {"leaks"}), f"same content: {after}"


def test_a_leak_in_a_file_tracked_after_a_green_gate_is_caught(repo):
    # Like privacy.scan_repo, the stub scans tracked and staged files only (scripts/ holds the pattern itself).
    _stub(repo.root / "scripts" / "leak-check.sh", "! git ls-files -z -- . ':!:scripts/' | xargs -0 grep -l SYNTHETIC-LEAK")
    repo.git("commit", "-qam", "leak-check stub")
    (repo.root / "new.txt").write_text("SYNTHETIC-LEAK\n")
    repo.gate("full")  # green: new.txt is untracked, so the scan does not read it
    repo.git("add", "new.txt")
    repo.git("commit", "-qm", "add the file")
    r = repo.run("fast")
    assert r.returncode != 0, f"a cached pass let a tracked leak through: {r.stdout[-500:]}"
    assert "step 'leaks' failed" in r.stderr


def test_a_same_size_edit_in_the_second_of_git_add_is_seen(repo):
    """Git trusts a file's size and mtime unless the file is as new as the index (racy git).

    A same-size edit made in the same second as `git add` is then seen only through the
    index file's own mtime. Fixed mtimes and core.trustctime=false make that second exact.
    """
    _stub(repo.root / "scripts" / "leak-check.sh", "! grep -q BAD notes.txt")
    repo.git("config", "core.trustctime", "false")
    repo.git("commit", "-qam", "leak-check stub")
    notes = repo.root / "notes.txt"

    def write(text: str) -> None:
        notes.write_text(text)
        os.utime(notes, ns=(SECOND_NS, SECOND_NS))

    write("GOOD\n")
    repo.git("add", "notes.txt")
    os.utime(repo.root / ".git" / "index", ns=(SECOND_NS, SECOND_NS))
    repo.gate("fast")
    write("BAD\n\n")  # same 5 bytes, same second
    r = repo.run("fast")
    assert r.returncode != 0, f"a cached pass hid a same-second edit: {r.stdout[-500:]}"
    assert "step 'leaks' failed" in r.stderr


def test_a_new_commit_reruns_the_history_scan(repo):
    repo.gate("fast")
    repo.git("commit", "-q", "--allow-empty", "-m", "message only")
    after = repo.gate("fast")
    assert after["secrets"] is False, "gitleaks reads commits, so a new commit must be scanned"
    assert all(after[name] for name in CONTENT_STEPS)


@pytest.mark.parametrize(("jobs", "flag"), [(None, "-n auto"), ("0", "-n 0")])
def test_the_unit_step_runs_the_tests_in_parallel(repo, tmp_path, jobs, flag):
    # One by one the unit step took about 140 s at load ~160 (task action-smoke-speed).
    args = tmp_path / "uv-args.txt"
    _stub(tmp_path / "bin" / "uv", f'echo "$*" >> "{args}"')
    (repo.root / "pyproject.toml").write_text("[project]\nname = 'stub'\n")
    repo.env.pop("UNIT_JOBS", None)
    if jobs is not None:
        repo.env["UNIT_JOBS"] = jobs
    assert "unit" in repo.gate("fast")
    assert f"run pytest -q {flag}" in args.read_text().splitlines()


@pytest.mark.parametrize("change", ["edit", "untracked", "delete", "stage-then-edit"])
def test_changed_content_reruns_every_step(repo, change):
    repo.gate("fast")
    notes = repo.root / "notes.txt"
    if change == "edit":
        notes.write_text("changed\n")
    elif change == "untracked":
        (repo.root / "extra.txt").write_text("extra\n")
    elif change == "delete":
        notes.unlink()
    else:
        notes.write_text("staged\n")
        repo.git("add", "notes.txt")
        repo.gate("fast")
        notes.write_text("edited after staging\n")
    after = repo.gate("fast")
    assert not any(after.values()), f"{change}: a changed tree must miss the cache: {after}"


@pytest.mark.parametrize("note", ["TASKS.md", "RECURRING.md", "docs/learnings/" + "2026-10-28-x.md"])
def test_a_notes_only_change_keeps_safe_scope_and_reruns_e2e(repo, note):
    # E2E and exercise runtime inputs are not qualified for cross-run reuse.
    repo.env["CHECK_OFFLINE"] = "1"
    (repo.root / "pyproject.toml").write_text("[project]\nname = 'stub'\n")
    path = repo.root / note
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("one\n")
    repo.git("add", "-f", note)  # tracked, as in the repo: a global gitignore may hide an untracked TASKS.md
    assert not any(repo.gate("full").values())
    path.write_text("two\n")
    after = repo.gate("full")
    assert after["exercises"] is False, f"exercise runtime inputs are not qualified: {after}"
    assert after["e2e"] is False, f"e2e runtime inputs are not qualified: {after}"
    assert after["format"] is False
    assert after["lint"] is False
    assert not any(after[name] for name in ("docs", "learnings", "leaks", "unit")), f"these read every file: {after}"


def test_a_code_change_still_reruns_the_steps_with_inputs(repo):
    (repo.root / "pyproject.toml").write_text("[project]\nname = 'stub'\n")
    assert not any(repo.gate("full").values())
    (repo.root / "pyproject.toml").write_text("[project]\nname = 'stub2'\n")
    after = repo.gate("full")
    assert not any(after.values()), f"a changed input must miss every cache: {after}"


def test_inherited_shell_startup_keeps_fixture_tools(repo, tmp_path, monkeypatch):
    import shlex

    wrong_bin = tmp_path / "wrong-tools"
    wrong_bin.mkdir()
    marker = tmp_path / "wrong-tool-ran"
    _stub(wrong_bin / "uv", f"printf invoked > {shlex.quote(str(marker))}\nexit 91")
    startup = tmp_path / "synthetic-shell-startup.sh"
    startup.write_text(f'export PATH={shlex.quote(str(wrong_bin))}:"$PATH"\n')
    monkeypatch.setenv("BASH_ENV", str(startup))
    child = Repo(repo.root, tmp_path / "bin")
    result = child.run("fast")
    assert result.returncode == 0, result.stdout[-500:] + result.stderr[-500:]
    assert not marker.exists(), "shell startup replaced the fixture's declared tool"


# Rust preparation uses the real gate and input-key scripts, with local tool
# programs. Each case owns its environment, process groups and retained evidence.
RUST_PRIMARY = {"G01": ("format", "rust-fmt"), "G02": ("lint", "rust-clippy"), "G03": ("unit", "rust-test")}
RUST_CARGO_VECTORS = {
    ("fmt", "--all", "--check"): "format",
    ("clippy", "--all-targets", "--", "-D", "warnings"): "lint",
    ("clippy", "--workspace", "--all-targets", "--locked", "--offline", "--", "-D", "warnings"): "lint",
    ("test", "--all"): "unit",
    ("test", "--workspace", "--locked", "--offline"): "unit",
}
RUST_UV_VECTORS = (
    ("run", "--quiet", "python", "scripts/learnings-table.py", "--check"),
    ("run", "--quiet", "python", "scripts/check-sources.py"),
    ("run", "--quiet", "python", "scripts/check-stories.py"),
    ("run", "ruff", "format", "--check", "."),
    ("run", "ruff", "check", "."),
    ("run", "--quiet", "pyright", "--version"),
    ("run", "pyright"),
    ("run", "pytest", "-q", "-n", "0"),
    ("run", "careerbrew", "cards", "public-check"),
    ("run", "python", "scripts/check-card-code.py"),
    ("run", "careerbrew", "exercises", "check"),
)


class RustCacheRepo(Repo):
    """One invocation owns all fake tools, local Git state and real child handles."""

    def __init__(self, root, bin_dir, env, evidence, nonce, tools):
        super().__init__(root, bin_dir)
        self.env = env
        self.evidence, self.nonce, self.tools = evidence, nonce, tools
        self.commands = []

    def execute(self, label, argv):
        import signal
        import time

        child = None
        interrupted = None
        errors = []
        number = len(self.commands) + 1
        record = {
            "command": label,
            "started_ns": time.time_ns(),
            "ended_ns": None,
            "pid": None,
            "queried_pgid": None,
            "queried_sid": None,
            "requested_new_session": True,
            "exit": None,
            "signal": None,
            "timeout": False,
            "term": False,
            "kill": False,
            "leader_reaped": False,
            "group_absent": None,
            "errors": errors,
        }
        streams = []

        def alive():
            try:
                os.killpg(child.pid, 0)
            except ProcessLookupError:
                return False
            return True

        try:
            # Descriptors are acquired before Popen. Any acquisition failure
            # reaches finally, with no missing child cleanup.
            streams.append((self.evidence / f"command-{number:02}.stdout").open("xb"))
            streams.append((self.evidence / f"command-{number:02}.stderr").open("xb"))
            child = subprocess.Popen(argv, cwd=self.root, env=self.env, stdin=subprocess.DEVNULL, stdout=streams[0], stderr=streams[1], start_new_session=True)
            record["pid"] = child.pid
            try:
                record["queried_pgid"], record["queried_sid"] = os.getpgid(child.pid), os.getsid(child.pid)
                if record["queried_pgid"] != child.pid or record["queried_sid"] != child.pid:
                    raise RuntimeError("fixture child escaped requested session")
            except ProcessLookupError:
                record["identity_unavailable"] = "already-exited"
                raise RuntimeError("fixture child identity unavailable")
            try:
                child.wait(timeout=600)
            except subprocess.TimeoutExpired:
                record["timeout"] = True
        except BaseException as error:
            errors.append({"stage": "launch-or-wait", "type": type(error).__name__})
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                interrupted = error
        finally:
            if child is not None:
                try:
                    if child.returncode is not None:
                        # Reaped leaders cannot keep a numeric group identity owned.
                        # A surviving group blocks; this branch never signals it.
                        if alive():
                            errors.append({"stage": "cleanup", "type": "group-outlived-reaped-leader"})
                    elif record["queried_pgid"] == record["queried_sid"] == child.pid:
                        errors.append({"stage": "cleanup", "type": "group-outlived-command"})
                        for kind, field in ((signal.SIGTERM, "term"), (signal.SIGKILL, "kill")):
                            if not alive():
                                break
                            record[field] = True
                            try:
                                os.killpg(child.pid, kind)
                            except ProcessLookupError:
                                pass
                            until = time.monotonic() + 3
                            while alive() and time.monotonic() < until:
                                time.sleep(0.02)
                        # No poll/wait runs between the group signals and this reap.
                        child.wait(timeout=3)
                    else:
                        errors.append({"stage": "cleanup", "type": "unverified-group"})
                except BaseException as error:
                    errors.append({"stage": "cleanup", "type": type(error).__name__})
                # Identity or group-observation failures cannot bypass direct cleanup.
                # This handle remains owned only while its leader is unreaped.
                if child.returncode is None:
                    try:
                        child.terminate()
                    except BaseException as error:
                        errors.append({"stage": "direct-terminate", "type": type(error).__name__})
                    try:
                        child.wait(timeout=3)
                    except BaseException as error:
                        errors.append({"stage": "direct-wait", "type": type(error).__name__})
                    if child.returncode is None:
                        try:
                            child.kill()
                        except BaseException as error:
                            errors.append({"stage": "direct-kill", "type": type(error).__name__})
                        try:
                            child.wait(timeout=3)
                        except BaseException as error:
                            errors.append({"stage": "direct-reap", "type": type(error).__name__})
                if child.returncode is not None:
                    record.update(exit=child.returncode if child.returncode >= 0 else None, signal=-child.returncode if child.returncode < 0 else None, leader_reaped=True)
                try:
                    record["group_absent"] = not alive()
                except BaseException as error:
                    errors.append({"stage": "group-absence", "type": type(error).__name__})
            for stream in streams:
                try:
                    stream.flush()
                    os.fsync(stream.fileno())
                except BaseException as error:
                    errors.append({"stage": "flush", "type": type(error).__name__})
                finally:
                    try:
                        stream.close()
                    except BaseException as error:
                        errors.append({"stage": "close", "type": type(error).__name__})
            record["ended_ns"] = time.time_ns()
            self.commands.append(record)
            self.write_record(f"command-{number:02}.json", record)
        if interrupted is not None:
            raise interrupted
        assert not errors and not record["timeout"] and record["leader_reaped"] and record["group_absent"], "BLOCKED: fixture process/evidence/cleanup failed"
        output = (self.evidence / f"command-{number:02}.stdout").read_text()
        error = (self.evidence / f"command-{number:02}.stderr").read_text()
        return subprocess.CompletedProcess(argv, child.returncode, output, error)

    def write_record(self, name, value):
        raw = json.dumps(value, sort_keys=True, indent=2) + "\n"
        with (self.evidence / name).open("x") as stream:
            stream.write(raw)
            stream.flush()
            os.fsync(stream.fileno())
        assert (self.evidence / name).read_text() == raw, "BLOCKED: evidence read-back differs"

    def git(self, *args):
        assert tuple(args) in (("init", "-q", "-b", "main"), ("add", "-A"), ("commit", "-qm", "init")), "BLOCKED: unregistered fixture Git request"
        options = ["-c", "maintenance.auto=false"] if args[0] == "commit" else []
        run = self.execute("git:" + args[0], [str(self.tools["git"]), *GIT[1:], *options, *args])
        assert run.returncode == 0, "BLOCKED: owned Git setup failed"

    def run(self, mode="fast"):
        assert mode == "fast", "BLOCKED: primary uses only the registered fast gate"
        return self.execute("gate:fast", [str(self.tools["bash"]), "scripts/check.sh", "fast"])


@pytest.fixture
def rust_cache_repo(request):
    import hashlib
    import stat
    import tempfile
    import uuid

    case = request.node.callspec.params["case"]
    assert case in RUST_PRIMARY
    retained = ROOT / ".metrics/rust00-gate-primary/retained"
    retained.mkdir(parents=True, exist_ok=True)
    owned = Path(tempfile.mkdtemp(prefix=f"{case}-", dir=retained)).resolve()
    owned.chmod(0o700)
    nonce = uuid.uuid4().hex
    (owned / ".owned-rust-gate").write_text(nonce)
    root, bin_dir, evidence = owned / "repo", owned / "bin", owned / "evidence"
    for path in (
        root / "scripts",
        bin_dir,
        evidence,
        owned / "home",
        owned / "config",
        owned / "state",
        owned / "cache",
        owned / "tmp",
        owned / "keys",
        owned / "clients",
        owned / "cargo",
        owned / "rustup",
        owned / "target",
        owned / "git-templates",
        owned / "runtime",
        owned / "data-home",
        owned / "backups",
        owned / "commands",
    ):
        path.mkdir(parents=True, exist_ok=True)
    (owned / "git-global").write_text("")
    tools, tool_metadata = {}, {}
    # Discover trusted installed POSIX tools from explicit system locations;
    # execute only their owned aliases, never the parent's PATH or user tools.
    search = os.defpath + os.pathsep + "/opt/homebrew/bin:/usr/local/bin"
    for name in ("bash", "sh", "env", "git", "dirname", "mkdir", "cat", "date", "cut", "paste", "shasum", "mktemp", "cp", "rm"):
        found = shutil.which(name, path=search)
        assert found is not None, "BLOCKED: required installed POSIX tool is absent"
        actual = Path(found).resolve(strict=True)
        assert stat.S_ISREG(actual.stat().st_mode) and os.access(actual, os.X_OK), "BLOCKED: fixture tool is not a regular executable"
        tools[name] = actual
        (bin_dir / name).symlink_to(actual)
        tool_metadata[name] = {"resolved": str(actual), "sha256": hashlib.sha256(actual.read_bytes()).hexdigest(), "basis": "observed-current"}
    env = {
        "HOME": str(owned / "home"),
        "PATH": str(bin_dir),
        "TMPDIR": str(owned / "tmp"),
        "LANG": "C",
        "LC_ALL": "C",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "XDG_CONFIG_HOME": str(owned / "config"),
        "XDG_STATE_HOME": str(owned / "state"),
        "XDG_CACHE_HOME": str(owned / "cache"),
        "XDG_RUNTIME_DIR": str(owned / "runtime"),
        "XDG_DATA_HOME": str(owned / "data-home"),
        "PYTHONDONTWRITEBYTECODE": "1",
        "CAREERBREW_HOME": str(owned / "data"),
        "CAREERBREW_KEY_FILE": str(owned / "keys/key"),
        "CAREERBREW_PAIR_KEY_FILE": str(owned / "keys/pair"),
        "CAREERBREW_BACKUP_DIRS": str(owned / "backups"),
        "CAREERBREW_COMMANDS_DIR": str(owned / "commands"),
        "CAREERBREW_CODEX_CONFIG": str(owned / "clients/codex"),
        "CAREERBREW_CLAUDE_CONFIG": str(owned / "clients/claude"),
        "CAREERBREW_LLM": "off",
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_NOTIFY": "off",
        "CAREERBREW_POSTHOG_KEY": "",
        "DO_NOT_TRACK": "1",
        "GIT_CONFIG_GLOBAL": str(owned / "git-global"),
        "GIT_CONFIG_NOSYSTEM": "1",
        "GIT_CONFIG_COUNT": "0",
        "GIT_TEMPLATE_DIR": str(owned / "git-templates"),
        "GIT_ATTR_NOSYSTEM": "1",
        "GIT_TERMINAL_PROMPT": "0",
        "GIT_ALLOW_PROTOCOL": "",
        "GIT_PROTOCOL_FROM_USER": "0",
        "CARGO_HOME": str(owned / "cargo"),
        "CARGO_TARGET_DIR": str(owned / "target"),
        "CARGO_NET_OFFLINE": "true",
        "RUSTUP_HOME": str(owned / "rustup"),
        "RUSTUP_AUTO_INSTALL": "0",
        "RUSTUP_NO_UPDATE_CHECK": "1",
        "UV_OFFLINE": "1",
        "UV_NO_SYNC": "1",
        "UV_PYTHON_DOWNLOADS": "never",
        "UNIT_JOBS": "0",
        "CHECK_OFFLINE": "1",
        "RUST_CACHE_NONCE": nonce,
        "RUST_CACHE_FAULT": RUST_PRIMARY[case][0],
        "RUST_CACHE_CALLS": str(owned / "calls.tsv"),
    }
    world = RustCacheRepo(root, bin_dir, env, evidence, nonce, tools)

    def finish():
        if not (evidence / "observation.json").exists():
            world.write_record("unobserved-or-blocked.json", {"case": case, "status": "unobserved-or-blocked", "commands": world.commands})

    request.addfinalizer(finish)
    for name in ("check.sh", "input-key.sh"):
        source = ROOT / "scripts" / name
        assert source.is_file() and not source.is_symlink()
        shutil.copy(source, root / "scripts" / name)
    for name in ("check-docs.sh", "leak-check.sh", "e2e.sh"):
        _stub(root / "scripts" / name)
    journal = 'printf "%s\\t%s\\t%s" "$RUST_CACHE_NONCE" "TOOL" "$#" >> "$RUST_CACHE_CALLS"\nfor value in "$@"; do printf "\\t%s" "$value" >> "$RUST_CACHE_CALLS"; done\nprintf "\\n" >> "$RUST_CACHE_CALLS"\n'
    uv_cases = "|".join('"' + " ".join(argv) + '"' for argv in RUST_UV_VECTORS)
    _stub(bin_dir / "uv", journal.replace("TOOL", "uv") + f'case "$*" in {uv_cases}) exit 0;; *) printf "unregistered-uv-vector\\n" >&2; exit 96;; esac')
    _stub(bin_dir / "gitleaks", journal.replace("TOOL", "gitleaks") + 'case "$*" in "git --no-banner --redact -l warn --log-opts=-20") exit 0;; *) printf "unregistered-gitleaks-vector\\n" >&2; exit 96;; esac')
    cargo_cases = []
    for argv, step in RUST_CARGO_VECTORS.items():
        cargo_cases.append('"' + " ".join(argv) + f'") step={step};;')
    _stub(
        bin_dir / "cargo",
        journal.replace("TOOL", "cargo")
        + 'case "$*" in\n'
        + "\n".join(cargo_cases)
        + '\n*) printf "unregistered-cargo-vector\\n" >&2; exit 96;; esac\nif [ "$step" = "$RUST_CACHE_FAULT" ]; then printf "native-failure\\t%s\\t%s\\n" "$RUST_CACHE_NONCE" "$step"; exit 73; fi\nexit 0',
    )
    (root / ".gitignore").write_text(".metrics/\n")
    (root / "notes.txt").write_text("one\n")
    (root / "pyproject.toml").write_text("[project]\nname = 'native-cache-fixture'\nversion = '0.0.0'\n")
    (root / "Cargo.toml").write_text("[workspace]\nmembers = []\n")
    world.write_record(
        "inputs.json",
        {
            "case": case,
            "nonce": nonce,
            "source_basis": "observed-current",
            "scripts": {name: hashlib.sha256((ROOT / "scripts" / name).read_bytes()).hexdigest() for name in ("check.sh", "input-key.sh")},
            "tools": tool_metadata,
            "fault_step": RUST_PRIMARY[case][0],
            "native_step_name": RUST_PRIMARY[case][1],
            "environment_keys": sorted(env),
            "environment_scope": "fresh explicit child values; owned HOME/config/data/key/client/Git/Cargo/temp roots",
            "intended_scope": "real gate/input-key/Git/POSIX tools and owned tool programs; no compiler or application route",
        },
    )
    world.git("init", "-q", "-b", "main")
    world.git("add", "-A")
    world.git("commit", "-qm", "init")
    return world


@pytest.mark.parametrize("case", ["G01", "G02", "G03"])
def test_rust_cache_primary(rust_cache_repo, case):
    import hashlib

    world = rust_cache_repo

    def public_bytes():
        return {str(p.relative_to(world.root)): (p.read_bytes(), p.stat().st_mode) for p in world.root.rglob("*") if p.is_file() and not set(p.relative_to(world.root).parts) & {".git", ".metrics"}}

    before = public_bytes()
    run = world.run("fast")
    assert public_bytes() == before, "BLOCKED: the fixture gate changed its public source inputs"
    assert "unregistered-" not in run.stderr, "BLOCKED: source requested a vector outside the frozen tool contract"
    assert run.returncode in (0, 1), "BLOCKED: gate did not return its normal functional status"
    calls = []
    for line in (world.root.parent / "calls.tsv").read_text().splitlines():
        nonce, tool, argc, *argv = line.split("\t")
        assert nonce == world.nonce and int(argc) == len(argv), "BLOCKED: malformed command correspondence"
        if tool == "uv":
            assert tuple(argv) in RUST_UV_VECTORS
        elif tool == "cargo":
            assert tuple(argv) in RUST_CARGO_VECTORS
        else:
            assert tool == "gitleaks" and argv == ["git", "--no-banner", "--redact", "-l", "warn", "--log-opts=-20"]
        calls.append((tool, tuple(argv)))
    for argv in (("run", "ruff", "format", "--check", "."), ("run", "ruff", "check", "."), ("run", "pytest", "-q", "-n", "0")):
        assert calls.count(("uv", argv)) == 1, "BLOCKED: Python's real dispatch was not observed"
    python_keys = {}  # no Python step is eligible for a source-only cache
    assert not (world.root / ".metrics/cache/format").exists(), "BLOCKED: format success must not be cached"
    assert not (world.root / ".metrics/cache/unit").exists(), "BLOCKED: unit success must not be cached"
    assert not (world.root / ".metrics/cache/lint").exists(), "BLOCKED: lint success must not be cached"
    events = [json.loads(line) for line in (world.root / ".metrics/events.jsonl").read_text().splitlines()]
    target, native_name = RUST_PRIMARY[case]
    target_calls = sum(tool == "cargo" and RUST_CARGO_VECTORS[argv] == target for tool, argv in calls if tool == "cargo")
    marker = f"native-failure\t{world.nonce}\t{target}\n"
    marker_count = run.stdout.count(marker)
    assert target_calls in (0, 1) and marker_count == target_calls, "BLOCKED: actual argv/output witnesses disagree"
    if run.returncode == 1:
        assert marker_count == 1, "BLOCKED: unexpected gate failure before the planted native command"
    native_failed = any(event["name"] == native_name and event.get("ok") is False for event in events)
    observation = {
        "case": case,
        "gate_exit": run.returncode,
        "native_target_calls": target_calls,
        "actual_failure_markers": marker_count,
        "native_failed_step_observed": native_failed,
        "native_success_cache_absent": not (world.root / ".metrics/cache" / native_name).exists(),
        "python_cache_keys": python_keys,
        "legacy_cache_hits": sum(event["name"] in {"format", "lint", "unit"} and event.get("cached") is True for event in events),
        "source_inputs_preserved": True,
        "commands": world.commands,
        "gate_output_sha256": hashlib.sha256(run.stdout.encode()).hexdigest(),
        "gate_error_sha256": hashlib.sha256(run.stderr.encode()).hexdigest(),
        "tool_calls": [{"tool": tool, "argv": argv} for tool, argv in calls],
    }
    world.write_record("observation.json", observation)
    assert target_calls == marker_count == 1 and run.returncode == 1 and native_failed and observation["native_success_cache_absent"], {
        "case": case,
        "native_failure_was_skipped": target_calls == 0,
        "gate_exit": run.returncode,
        "native_failure_markers": marker_count,
    }


def test_exercises_step_rechecks_external_runner(repo):
    (repo.root / "pyproject.toml").write_text("[project]\nname = 'stub'\n")
    sentinel = repo.root.parent / "exercise-runner-fails"
    uv = Path(repo.env["PATH"].split(os.pathsep)[0]) / "uv"
    _stub(uv, f'if [ "$*" = "run careerbrew exercises check" ]; then test ! -e {shlex.quote(str(sentinel))}; fi')
    first = repo.gate("fast")
    assert first["exercises"] is False
    sentinel.touch()

    ordinary = repo.run("fast")
    assert ordinary.returncode != 0, ordinary.stdout + ordinary.stderr
    assert "step 'exercises' failed" in ordinary.stderr
    repo.env["NO_CACHE"] = "1"
    uncached = repo.run("fast")
    assert uncached.returncode != 0, uncached.stdout + uncached.stderr
    assert "step 'exercises' failed" in uncached.stderr
