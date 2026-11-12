"""Eight portable dispatch contracts; native qualification is in tests/safety.

Each case owns its miniature repo, environment, direct children and evidence.
Fake Lefthook proves argv/status dispatch only, never native hook effectiveness.
"""

from __future__ import annotations

import hashlib
import json
import os
import shlex
import shutil
import signal
import stat
import subprocess
import tempfile
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SOURCE_PATHS = ("scripts/setup.sh", "hooks/pre-commit", "lefthook.yml")
TOOL_NAMES = ("bash", "sh", "env", "dirname", "mkdir", "chmod", "cp", "git")
CASE_IDS = ("U01", "U02", "U03", "U04", "U05", "U06", "U07", "U08")
OWNER_SELECTOR = "CAREERBREW_TEST_DIAGNOSTIC_OWNER"


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _json(path: Path, value: object) -> None:
    raw = (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    if path.read_bytes() != raw:
        raise RuntimeError("BLOCKED: evidence read-back differs")


def _files(folder: Path) -> dict[str, object]:
    return {str(path.relative_to(folder)): {"sha256": _sha(path.read_bytes()), "mode": stat.S_IMODE(path.stat().st_mode), "mtime_ns": path.stat().st_mtime_ns} for path in sorted(folder.rglob("*")) if path.is_file()}


class PortableHooks:
    """One case owns its tools, command handles, route files and retained ledger."""

    def __init__(self, case: str):
        self.case = case
        self.root = None
        self.commands = []
        self.source = {}

    def prepare(self) -> None:
        # No unqualified adapter is allowed to select a fallback ownership mode.
        if OWNER_SELECTOR in os.environ:
            raise RuntimeError("BLOCKED: outer-owner adapter qualification pending")
        if os.name != "posix":
            raise RuntimeError("BLOCKED: this bounded process fixture requires POSIX")
        retained = ROOT / ".metrics" / "issue327-portable" / "retained"
        for path in (ROOT / ".metrics", retained.parent, retained):
            if path.is_symlink():
                raise RuntimeError("BLOCKED: evidence root is a symlink")
            path.mkdir(exist_ok=True)
        if not retained.resolve().is_relative_to(ROOT):
            raise RuntimeError("BLOCKED: evidence root escaped public checkout")
        self.root = Path(tempfile.mkdtemp(prefix=f"{self.case}-", dir=retained)).resolve()
        self.root.chmod(0o700)
        (self.root / ".owned-327-portable").write_text(self.case)
        self.repo, self.home, self.bin = self.root / "repo", self.root / "home", self.root / "bin"
        self.evidence, self.journal = self.root / "evidence", self.root / "calls.tsv"
        for name in ("repo", "home", "bin", "evidence", "tmp", "templates", "xdg-config", "xdg-cache", "xdg-state", "xdg-data"):
            (self.root / name).mkdir(mode=0o700)
        self.journal.write_text("")
        self.tools = {}
        for name in TOOL_NAMES:
            found = shutil.which(name)
            if found is None:
                raise RuntimeError("BLOCKED: required ordinary tool unavailable")
            resolved = Path(found).resolve(strict=True)
            if not resolved.is_file() or not os.access(resolved, os.X_OK):
                raise RuntimeError("BLOCKED: ordinary tool is not an executable file")
            self.tools[name] = {"resolved": str(resolved), "sha256": _sha(resolved.read_bytes())}
            if name != "git":
                (self.bin / name).symlink_to(resolved)
        self.global_config = self.home / "gitconfig"
        self.global_hooks, self.local_hooks = self.home / "global-hooks", self.home / "local-hooks"
        for path in (self.global_hooks, self.local_hooks):
            path.mkdir()
            self.script(path / "pre-commit", "exit 95\n")
        route = self.global_hooks if self.case in {"U01", "U02", "U08"} else None
        self.global_config.write_text(f"[core]\n\thooksPath = {route}\n" if route else "")
        self.env = {
            "HOME": str(self.home),
            "PATH": str(self.bin),
            "LANG": "C",
            "LC_ALL": "C",
            "TZ": "UTC",
            "TMPDIR": str(self.root / "tmp"),
            "XDG_CONFIG_HOME": str(self.root / "xdg-config"),
            "XDG_CACHE_HOME": str(self.root / "xdg-cache"),
            "XDG_STATE_HOME": str(self.root / "xdg-state"),
            "XDG_DATA_HOME": str(self.root / "xdg-data"),
            "GIT_CONFIG_GLOBAL": str(self.global_config),
            "GIT_CONFIG_SYSTEM": os.devnull,
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_COUNT": "0",
            "GIT_ATTR_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_OPTIONAL_LOCKS": "0",
            "GIT_TEMPLATE_DIR": str(self.root / "templates"),
            "GIT_ALLOW_PROTOCOL": "",
            "GIT_PROTOCOL_FROM_USER": "0",
            "SETUP_SKIP_DEPS": "1",
            "HOOK_PORTABLE_CALLS": str(self.journal),
        }
        self.script(
            self.bin / "git",
            self.log_shell("git")
            + 'case "$#:${1-}:${2-}:${3-}:${4-}" in\n'
            + '  "3:init:-q:--initial-branch=main:"|"3:config:--get:core.hooksPath:"|"4:config:--local:--get:core.hooksPath"|"4:config:--global:--get:core.hooksPath"|"2:rev-parse:--show-toplevel::") ;;\n'
            + "  *) exit 96;;\nesac\n"
            + f'exec {shlex.quote(self.tools["git"]["resolved"])} "$@"\n',
        )
        for name in SOURCE_PATHS:
            path = ROOT / name
            if path.is_symlink() or not path.resolve().is_relative_to(ROOT):
                raise RuntimeError("BLOCKED: copied source escaped its public path")
            raw = path.read_bytes()
            self.source[name] = _sha(raw)
            target = self.repo / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(raw)
            target.chmod(0o600 if name.endswith(".yml") else 0o700)
            if target.read_bytes() != raw:
                raise RuntimeError("BLOCKED: copied source differs")
        for relative, tag, args, code in (
            ("scripts/install-launcher.sh", "launcher", (), 0),
            ("scripts/doctor.sh", "doctor", (), 9 if self.case == "U08" else 0),
            ("scripts/leak-check.sh", "leaks", ("--staged",), 0),
            ("scripts/check-docs.sh", "docs", (), 0),
            ("../bin/gitleaks", "secrets", ("git", "--staged", "--no-banner", "--redact", "-l", "warn"), 9 if self.case == "U07" else 0),
        ):
            guard = f'[ "$#" -eq {len(args)} ] && [ "$*" = {shlex.quote(" ".join(args))} ] || exit 96\n'
            self.script(self.repo / relative, self.log_shell(tag) + guard + f"exit {code}\n")
        if self.case not in {"U06", "U07"}:
            self.script(
                self.bin / "lefthook",
                self.log_shell("lefthook") + 'case "$#:${1-}:${2-}:${3-}" in "1:install::"|"3:run:--no-auto-install:pre-commit") ;; *) exit 96;; esac\n' + "printf 'portable dispatch stub\\n'\n" + f"exit {7 if self.case == 'U05' else 0}\n",
            )
        _json(
            self.evidence / "inputs.json",
            {
                "schema": 1,
                "case": self.case,
                "source_basis": "observed-current",
                "reviewed": False,
                "source_sha256": self.source,
                "tools": self.tools,
                "env_keys": sorted(self.env),
                "env_sha256": _sha(json.dumps(self.env, sort_keys=True).encode()),
                "fixture_files": _files(self.bin),
                "lifecycle": "standalone-direct-owned-session",
                "native_qualification": False,
            },
        )
        result = self.execute("init", [str(self.bin / "git"), "init", "-q", "--initial-branch=main"])
        if result.returncode != 0:
            raise RuntimeError("BLOCKED: owned Git initialization failed")
        if self.case == "U02":
            with (self.repo / ".git/config").open("a") as stream:
                stream.write(f"[core]\n\thooksPath = {self.local_hooks}\n")
        self.effective = self.local_hooks if self.case == "U02" else route
        query = self.execute("effective", [str(self.bin / "git"), "config", "--get", "core.hooksPath"])
        if (query.returncode, query.stdout.strip()) != (0 if self.effective else 1, str(self.effective) if self.effective else ""):
            raise RuntimeError("BLOCKED: real Git effective route differs")

    @staticmethod
    def log_shell(tag: str) -> str:
        return f'printf \'{tag}\\t%s\' "$#" >>"$HOOK_PORTABLE_CALLS"\n' + 'if [ "$#" -gt 0 ]; then printf "\\t%s" "$@" >>"$HOOK_PORTABLE_CALLS"; fi\nprintf "\\n" >>"$HOOK_PORTABLE_CALLS"\n'

    def script(self, path: Path, body: str) -> None:
        path.write_text(f"#!{self.tools['bash']['resolved']}\nset -euo pipefail\n{body}")
        path.chmod(0o700)

    def execute(self, label: str, argv: list[str]) -> subprocess.CompletedProcess[str]:
        if argv not in (
            [str(self.bin / "git"), "init", "-q", "--initial-branch=main"],
            [str(self.bin / "git"), "config", "--get", "core.hooksPath"],
            [str(self.bin / "bash"), "scripts/setup.sh"],
            [str(self.bin / "bash"), "hooks/pre-commit"],
        ):
            raise RuntimeError("BLOCKED: unregistered command vector")
        child, interrupted = None, None
        streams, errors = [], []
        record = {
            "label": label,
            "argv": argv,
            "started_ns": time.time_ns(),
            "ended_ns": None,
            "pid": None,
            "pgid": None,
            "sid": None,
            "exit": None,
            "timeout": False,
            "leader_reaped": False,
            "group_absent": None,
            "signals": [],
            "errors": errors,
        }
        stem = self.evidence / f"{len(self.commands) + 1:02}-{label}"

        def alive() -> bool:
            try:
                os.killpg(child.pid, 0)
                return True
            except ProcessLookupError:
                return False

        try:
            streams.append(stem.with_suffix(".stdout").open("xb"))
            streams.append(stem.with_suffix(".stderr").open("xb"))
            child = subprocess.Popen(argv, cwd=self.repo, env=self.env, stdin=subprocess.DEVNULL, stdout=streams[0], stderr=streams[1], start_new_session=True)
            record["pid"] = child.pid
            record["pgid"], record["sid"] = os.getpgid(child.pid), os.getsid(child.pid)
            if record["pgid"] != child.pid or record["sid"] != child.pid or child.pid == os.getpgrp():
                raise RuntimeError("BLOCKED: owned child session identity differs")
            try:
                child.wait(timeout=10)
            except subprocess.TimeoutExpired:
                record["timeout"] = True
                errors.append("command-timeout")
        except BaseException as error:
            errors.append(f"launch-or-wait:{type(error).__name__}")
            if isinstance(error, (KeyboardInterrupt, SystemExit)):
                interrupted = error
        finally:
            if child is not None:
                try:
                    if child.returncode is not None:
                        # An already-reaped leader never authorizes a group signal.
                        if alive():
                            errors.append("group-outlived-reaped-leader")
                    elif record["pgid"] == record["sid"] == child.pid:
                        errors.append("unreaped-command-cleanup")
                        for kind in (signal.SIGTERM, signal.SIGKILL):
                            if not alive():
                                break
                            record["signals"].append(kind.name)
                            try:
                                os.killpg(child.pid, kind)
                            except ProcessLookupError:
                                pass
                            until = time.monotonic() + 3
                            while alive() and time.monotonic() < until:
                                time.sleep(0.02)
                        child.wait(timeout=3)
                    else:
                        errors.append("unverified-group")
                except BaseException as error:
                    errors.append(f"group-cleanup:{type(error).__name__}")
                # Unknown group identity still permits cleanup through the owned,
                # unreaped direct handle. No group number is guessed here.
                if child.returncode is None:
                    try:
                        child.terminate()
                    except BaseException as error:
                        errors.append(f"direct-terminate:{type(error).__name__}")
                    try:
                        child.wait(timeout=3)
                    except BaseException as error:
                        errors.append(f"direct-wait:{type(error).__name__}")
                    if child.returncode is None:
                        try:
                            child.kill()
                        except BaseException as error:
                            errors.append(f"direct-kill:{type(error).__name__}")
                        try:
                            child.wait(timeout=3)
                        except BaseException as error:
                            errors.append(f"direct-reap:{type(error).__name__}")
                record.update(exit=child.returncode, leader_reaped=child.returncode is not None)
                try:
                    record["group_absent"] = not alive()
                except BaseException as error:
                    errors.append(f"group-absence:{type(error).__name__}")
            for stream in streams:
                try:
                    stream.flush()
                    os.fsync(stream.fileno())
                except BaseException as error:
                    errors.append(f"output-flush:{type(error).__name__}")
                finally:
                    try:
                        stream.close()
                    except BaseException as error:
                        errors.append(f"output-close:{type(error).__name__}")
            record["ended_ns"] = time.time_ns()
            self.commands.append(record)
            _json(stem.with_suffix(".json"), record)
        if interrupted is not None:
            raise interrupted
        if errors or record["timeout"] or not record["leader_reaped"] or not record["group_absent"]:
            raise RuntimeError("BLOCKED: owned command lifecycle/evidence failed")
        return subprocess.CompletedProcess(argv, child.returncode, stem.with_suffix(".stdout").read_text(), stem.with_suffix(".stderr").read_text())

    def snapshot(self) -> dict[str, object]:
        return {"repo_files": _files(self.repo), "home_files": _files(self.home), "tool_files": _files(self.bin)}

    def measure(self) -> subprocess.CompletedProcess[str]:
        before = self.snapshot()
        earlier = self.journal.read_text().splitlines()
        command = "scripts/setup.sh" if self.case in {"U01", "U02", "U03", "U08"} else "hooks/pre-commit"
        result = self.execute("measured", [str(self.bin / "bash"), command])
        self.measured_calls = [tuple(line.split("\t")) for line in self.journal.read_text().splitlines()[len(earlier) :]]
        after = self.snapshot()
        _json(
            self.evidence / "preservation.json",
            {"before": before, "after": after, "actual_calls": self.measured_calls, "actual_exit": result.returncode, "stdout_sha256": _sha(result.stdout.encode()), "stderr_sha256": _sha(result.stderr.encode())},
        )
        assert before == after, "BLOCKED: configured routes or source fixture files changed"
        assert not (self.repo / ".env").exists(), "BLOCKED: source env unexpectedly appeared"
        if command == "scripts/setup.sh":
            assert (self.repo / "private").is_dir() and not any((self.repo / "private").iterdir())
            assert stat.S_IMODE((self.repo / "private").stat().st_mode) == 0o700
        return result

    def finish(self) -> None:
        if self.root is not None:
            _json(self.root / "retention.json", {"schema": 1, "case": self.case, "commands": self.commands, "evidence_retained": True, "cleanup_scope": "direct-owned-standalone-sessions", "outer_mode": "held-until-qualified-adapter"})


@pytest.fixture
def portable_hooks(case):
    fixture = PortableHooks(case)
    try:
        fixture.prepare()
        yield fixture
    finally:
        fixture.finish()


@pytest.mark.parametrize("case", ["U03"])
def test_setup_refuses_to_install_when_git_config_is_invalid(case, portable_hooks):
    fixture = portable_hooks
    direct_git = shutil.which("git", path=os.defpath)
    assert direct_git is not None, "direct system Git is unavailable"
    fixture.script(fixture.bin / "git", fixture.log_shell("git") + f'exec {shlex.quote(direct_git)} "$@"\n')
    fixture.global_config.write_text("[invalid\n")
    result = fixture.measure()
    assert result.returncode != 0
    assert ("lefthook", "1", "install") not in fixture.measured_calls
    assert "hook" in result.stderr.lower()


@pytest.mark.parametrize("case", CASE_IDS)
def test_portable_hook_contract(case, portable_hooks):
    fixture = portable_hooks
    result = fixture.measure()
    git_config = ("git", "3", "config", "--get", "core.hooksPath")
    git_root = ("git", "2", "rev-parse", "--show-toplevel")
    launcher, doctor = ("launcher", "0"), ("doctor", "0")
    native_run = ("lefthook", "3", "run", "--no-auto-install", "pre-commit")
    secrets = ("secrets", "6", "git", "--staged", "--no-banner", "--redact", "-l", "warn")
    expected = {
        "U01": [git_config, launcher, doctor],
        "U02": [git_config, launcher, doctor],
        "U03": [git_config, ("lefthook", "1", "install"), launcher, doctor],
        "U04": [git_root, native_run],
        "U05": [git_root, native_run],
        "U06": [git_root, secrets, ("leaks", "1", "--staged"), ("docs", "0")],
        "U07": [git_root, secrets],
        "U08": [git_config, launcher, doctor],
    }
    assert fixture.measured_calls == expected[case], "wrong dispatch, duplicate action, fallback or unexpected command"
    assert result.returncode == {"U05": 7, "U07": 9, "U08": 9}.get(case, 0)
    if case in {"U01", "U02", "U08"}:
        line = f"git hooks: existing path {fixture.effective} retained; repository entry: hooks/pre-commit (delegation not verified)"
        assert result.stdout.splitlines().count(line) == 1
        assert "lefthook install skipped" not in result.stdout
        assert "--unset" not in result.stdout and "--remove-section" not in result.stdout
        if case == "U02":
            assert str(fixture.global_hooks) not in result.stdout
    if case in {"U01", "U02", "U03"}:
        assert result.stdout.splitlines().count("Ready. Next: scripts/check.sh") == 1
    if case == "U08":
        assert "Ready. Next: scripts/check.sh" not in result.stdout
    if case in {"U06", "U07"}:
        assert not (fixture.bin / "lefthook").exists()
