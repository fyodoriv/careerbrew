"""Registered #327: real setup/Git/Lefthook wiring, synthetic check jobs only.

Lifecycle: each case owns a retained miniature checkout, HOME, Git config, PATH,
sentinels and process groups. No command executes in the source checkout. The
seven cases measure guidance and enforcement, never real scanner effectiveness.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import signal
import stat
import subprocess
import tempfile
import time
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
NATIVE = Path("/opt/homebrew/Cellar/lefthook/2.2.2/bin/lefthook")
NATIVE_SHA256 = "8dba2a2e5f3e1b5d8b8f27dfee454505730ed42ecc186779c5f45ce59ca94948"
PLAN_SHA256 = "110d7ba742090cccbea176527bb2577a10c0b98542456becb316ad3c4e8e2208"
SOURCE_PATHS = (
    "scripts/setup.sh",
    "scripts/install.sh",
    "scripts/doctor.sh",
    "hooks/pre-commit",
    "hooks/pre-push",
    "lefthook.yml",
    "INSTALL.md",
    "tests/unit/test_install_prompt.py",
)
TOOLS = {"bash": "/bin/bash", "sh": "/bin/sh", "env": "/usr/bin/env", "dirname": "/usr/bin/dirname", "mkdir": "/bin/mkdir", "chmod": "/bin/chmod", "cp": "/bin/cp"}


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _json(path: Path, value: object) -> None:
    raw = (json.dumps(value, indent=2) + "\n").encode()
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    if path.read_bytes() != raw:
        raise RuntimeError("BLOCKED: retained evidence read-back differs")


def _script(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("#!/bin/bash\nset -euo pipefail\n" + body)
    path.chmod(0o700)


class Mini:
    def __init__(self, case: str, *, route: str = "active", native: bool = True, secrets: int = 0, doctor: int = 0):
        # The source manifest is ignored and exact-hash reviewed before execution.
        # Root can later pin reviewed AFTER source without changing these tests.
        freeze_raw = (ROOT / ".metrics/issue327/source-freeze.json").read_bytes()
        freeze = json.loads(freeze_raw)
        if freeze["plan_sha256"] != PLAN_SHA256 or set(freeze["source_sha256"]) != set(SOURCE_PATHS):
            raise RuntimeError("BLOCKED: unregistered source manifest")
        copies = {name: (ROOT / name).read_bytes() for name in SOURCE_PATHS}
        if {name: _sha(raw) for name, raw in copies.items()} != freeze["source_sha256"]:
            raise RuntimeError("BLOCKED: source pin drift")
        if _sha(NATIVE.read_bytes()) != NATIVE_SHA256 or not stat.S_ISREG(NATIVE.stat().st_mode) or not os.access(NATIVE, os.X_OK):
            raise RuntimeError("BLOCKED: native Lefthook pin unavailable or changed")
        package = json.loads((NATIVE.parent.parent / "INSTALL_RECEIPT.json").read_text())
        if package["source"]["versions"]["stable"] != "2.2.2":
            raise RuntimeError("BLOCKED: native package version changed")
        retained = ROOT / ".metrics/issue327/retained"
        retained.mkdir(parents=True, exist_ok=True)
        self.root = Path(tempfile.mkdtemp(prefix=f"{case}-", dir=retained)).resolve()
        self.case, self.sequence = case, 0
        (self.root / ".owned-327").write_text(case)
        self.repo, self.home = self.root / "repo", self.root / "home"
        self.bin, self.calls, self.tags = self.root / "bin", self.root / "calls", self.root / "sentinels"
        self.config, self.global_hooks = self.home / "gitconfig", self.home / "global-hooks"
        for name in ("repo", "home", "bin", "calls", "sentinels", "tmp", "templates", "xdg-config", "xdg-state", "xdg-cache", "xdg-data"):
            (self.root / name).mkdir()
        self.global_hooks.mkdir()
        self.config.write_text(f"[core]\n\thooksPath = {self.global_hooks}\n" if route != "absent" else "")
        self.env = {
            "HOME": str(self.home),
            "PATH": str(self.bin),
            "LANG": "C",
            "LC_ALL": "C",
            "TZ": "UTC",
            "TMPDIR": str(self.root / "tmp"),
            "XDG_CONFIG_HOME": str(self.root / "xdg-config"),
            "XDG_STATE_HOME": str(self.root / "xdg-state"),
            "XDG_CACHE_HOME": str(self.root / "xdg-cache"),
            "XDG_DATA_HOME": str(self.root / "xdg-data"),
            "GIT_CONFIG_GLOBAL": str(self.config),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_TEMPLATE_DIR": str(self.root / "templates"),
            "GIT_AUTHOR_NAME": "Synthetic Fixture",
            "GIT_AUTHOR_EMAIL": "fixture@example.com",
            "GIT_COMMITTER_NAME": "Synthetic Fixture",
            "GIT_COMMITTER_EMAIL": "fixture@example.com",
            "GIT_AUTHOR_DATE": "2026-11-02T09:00:00+00:00",
            "GIT_COMMITTER_DATE": "2026-11-02T09:00:00+00:00",
            "SETUP_SKIP_DEPS": "1",
            "ISSUE327_CALLS": str(self.calls),
            "ISSUE327_TAGS": str(self.tags),
            "ISSUE327_REPO": str(self.repo),
        }
        for name, target in TOOLS.items():
            (self.bin / name).symlink_to(target)
        # Only reviewed local Git verbs; config queries are read-only. No remote,
        # bypass, force, hook-path write or inherited Git routing is permitted.
        _script(
            self.bin / "git",
            """printf '%s\\n' "$@" >"$ISSUE327_CALLS/git-$$"
if [ "$#" -eq 3 ] && [ "$1" = config ] && [ "$3" = core.hooksPath ]; then
  case "$2" in --local|--global) exec /usr/bin/git "$@";; esac
fi
for arg in "$@"; do
  case "$arg" in --no-verify|--force*|--unset*|--remove-section|--replace-all|--add|--global|--system|--file|-c|*hooksPath=*) exit 97;; esac
done
case "${1:-}" in
  config)
    case " $* " in *" --get "*|*" --get-all "*|*" --get-regexp "*|*" --list "*) ;; *) exit 97;; esac;;
  init|add|commit|rev-parse|diff|ls-files|status|check-attr|version|--version) ;;
  *) exit 97;;
esac
exec /usr/bin/git "$@"
""",
        )
        if native:
            # Record the real hook's argv, then exec the unchanged pinned binary.
            # This wrapper never invents, filters or suppresses native output.
            _script(self.bin / "lefthook", f'printf \'%s\\n\' "$@" >"$ISSUE327_CALLS/lefthook-$$"\nexec {NATIVE} "$@"\n')
        for name in ("scripts/setup.sh", "hooks/pre-commit", "lefthook.yml"):
            target = self.repo / name
            target.parent.mkdir(parents=True, exist_ok=True)
            target.write_bytes(copies[name])
            if name != "lefthook.yml":
                target.chmod(0o700)
            if target.read_bytes() != copies[name]:
                raise RuntimeError("BLOCKED: copied source differs")
        # Only the unrelated launcher/doctor and scanner jobs are sentinels.
        for file, tag, code, args in (
            ("scripts/install-launcher.sh", "LAUNCHER_327", 0, ""),
            ("scripts/doctor.sh", "DOCTOR_327", doctor, ""),
            ("scripts/leak-check.sh", "LEAKS_327", 0, "--staged"),
            ("scripts/check-docs.sh", "DOCS_327", 0, ""),
            ("../bin/gitleaks", "SECRETS_327", secrets, "git --staged --no-banner --redact -l warn"),
        ):
            _script(self.repo / file, f'[ "$#" -eq {len(args.split())} ] && [ "$*" = "{args}" ] || exit 96\nprintf \'{tag}\\n\' >>"$ISSUE327_TAGS/{tag}"\nexit {code}\n')
        if route == "active":
            _script(self.global_hooks / "pre-commit", 'printf \'GLOBAL_ROUTE_327\\n\' >>"$ISSUE327_TAGS/GLOBAL_ROUTE_327"\nexec "$ISSUE327_REPO/hooks/pre-commit" "$@"\n')
        fixture_files = [p for folder in (self.bin, self.repo / "scripts", self.repo / "hooks", self.global_hooks) for p in folder.iterdir() if p.is_file() and not p.is_symlink()]
        _json(
            self.root / "inputs.json",
            {
                "case": case,
                "source_manifest_sha256": _sha(freeze_raw),
                "source_manifest": freeze,
                "native": {"path": str(NATIVE), "version": "2.2.2", "sha256": NATIVE_SHA256, "version_source": "installed package metadata; no version probe"},
                "native_on_PATH": native,
                "route": route,
                "sentinel_exit": {"secrets": secrets, "doctor": doctor},
                "fixture_sha256": {str(p.relative_to(self.root)): _sha(p.read_bytes()) for p in fixture_files},
            },
        )
        if self.run("init", [str(self.bin / "git"), "init", "-q", "--initial-branch=main"]).returncode:
            raise RuntimeError("BLOCKED: synthetic Git initialization failed")
        # Fixture-owned identity/signing policy only. The active hook is retained
        # for the very first commit; no unhooked initial commit exists.
        with (self.repo / ".git/config").open("a") as stream:
            stream.write("[user]\n\tname = Synthetic Fixture\n\temail = fixture@example.com\n[commit]\n\tgpgsign = false\n")
        (self.repo / "synthetic.txt").write_text("Synthetic issue327 payload.\n")
        if self.run("stage", [str(self.bin / "git"), "add", "synthetic.txt"]).returncode:
            raise RuntimeError("BLOCKED: synthetic Git staging failed")

    def run(self, label: str, argv: list[str]) -> subprocess.CompletedProcess[str]:
        self.sequence += 1
        stem = self.root / f"{self.sequence:02}-{label}"
        child = None
        code, blocked = None, []
        group_gone = False

        def group_alive() -> bool | None:
            if child is None:
                return False
            try:
                os.killpg(child.pid, 0)
                return True
            except ProcessLookupError:
                return False
            except OSError as error:
                blocked.append(f"owned group observation: {error}")
                return None

        # Files and owners exist before spawn. All evidence/root state is retained.
        with stem.with_suffix(".stdout").open("xb") as stdout, stem.with_suffix(".stderr").open("xb") as stderr:
            try:
                child = subprocess.Popen(argv, cwd=self.repo, env=self.env, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, start_new_session=True)
                try:
                    code = child.wait(timeout=30)
                except subprocess.TimeoutExpired:
                    blocked.append("command timeout; never semantic RED")
            except (OSError, ValueError) as error:
                blocked.append(str(error))
            finally:
                if child is not None and child.returncode is None:
                    blocked.append("owned command did not produce a reaped terminal leader")
                    group_owned = False
                    try:
                        group_owned = os.getpgid(child.pid) == child.pid and os.getsid(child.pid) == child.pid and child.pid != os.getpgrp()
                    except OSError as error:
                        blocked.append(f"owned group identity: {error}")
                    direct = not group_owned
                    if group_owned:
                        # Direct Popen methods may reap internally. Finish group signals first.
                        try:
                            os.killpg(child.pid, signal.SIGTERM)
                            until = time.monotonic() + 3
                            alive = group_alive()
                            while alive is True and time.monotonic() < until:
                                time.sleep(0.05)
                                alive = group_alive()
                            if alive is True:
                                os.killpg(child.pid, signal.SIGKILL)
                            direct = alive is None
                        except ProcessLookupError:
                            pass
                        except OSError as error:
                            blocked.append(f"owned group signal: {error}")
                            direct = True
                    if not direct:
                        try:
                            code = child.wait(timeout=3)
                        except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                            blocked.append(f"owned group reap: {error}")
                            direct = True
                    if direct:
                        for kind in (signal.SIGTERM, signal.SIGKILL):
                            if child.returncode is not None:
                                break
                            try:
                                child.send_signal(kind)
                            except (OSError, ValueError) as error:
                                blocked.append(f"owned handle signal: {error}")
                            try:
                                code = child.wait(timeout=3)
                            except (OSError, ValueError, subprocess.TimeoutExpired) as error:
                                blocked.append(f"owned handle reap: {error}")
                if child is not None:
                    code = child.returncode
                    group_gone = group_alive() is False
                    if not group_gone and child.returncode is not None:
                        blocked.append("owned group absence unproved after reaping; no signal authority")
                for stream in (stdout, stderr):
                    stream.flush()
                    os.fsync(stream.fileno())
        out, err = stem.with_suffix(".stdout").read_text(), stem.with_suffix(".stderr").read_text()
        terminal = {
            "argv": argv,
            "pid": child.pid if child else None,
            "owned_group": child.pid if child else None,
            "code": code,
            "leader_reaped": child is not None and child.returncode is not None,
            "owned_group_gone": group_gone,
            "blocked": blocked,
            "stdout_sha256": _sha(out.encode()),
            "stderr_sha256": _sha(err.encode()),
        }
        _json(stem.with_suffix(".json"), terminal)
        if blocked or code is None or code < 0 or not group_gone:
            raise RuntimeError(f"BLOCKED: owned command lifecycle; evidence {stem}")
        return subprocess.CompletedProcess(argv, code, out, err)

    def snapshot(self) -> dict[str, object]:
        hooks = {p.name: {"bytes": p.read_bytes().hex(), "mode": stat.S_IMODE(p.stat().st_mode)} for p in self.global_hooks.iterdir() if p.is_file()}
        config = self.run("effective-hooks", [str(self.bin / "git"), "config", "--get", "core.hooksPath"])
        local = self.run("local-hooks", [str(self.bin / "git"), "config", "--local", "--get", "core.hooksPath"])
        assert config.returncode in (0, 1) and local.returncode == 1, "BLOCKED: unexpected configuration boundary"
        return {
            "global_config": self.config.read_bytes().hex(),
            "global_config_mode": stat.S_IMODE(self.config.stat().st_mode),
            "global_hook_files": hooks,
            "repo_config": (self.repo / ".git/config").read_bytes().hex(),
            "repo_config_mode": stat.S_IMODE((self.repo / ".git/config").stat().st_mode),
            "effective": config.stdout.strip(),
            "effective_exit": config.returncode,
            "no_local_override": local.returncode,
        }

    def measure(self, label: str, argv: list[str]) -> subprocess.CompletedProcess[str]:
        before = self.snapshot()
        result = self.run(label, argv)
        after = self.snapshot()
        ledger = {p.name: p.read_text().splitlines() for p in self.calls.iterdir()}
        tags = {p.name: p.read_text().splitlines() for p in self.tags.iterdir()}
        _json(self.root / f"{label}-preservation.json", {"before": before, "after": after, "sentinels": tags, "call_ledger": ledger})
        assert before == after, "BLOCKED: hook/config preservation changed before output oracle"
        for name, lines in ledger.items():
            if name.startswith("git-"):
                assert lines and lines[0] in {"init", "add", "commit", "config", "rev-parse", "diff", "ls-files", "status", "check-attr", "version", "--version"}, "BLOCKED: unexpected Git command"
                assert lines in (["config", "--local", "core.hooksPath"], ["config", "--global", "core.hooksPath"]) or not any(
                    arg in {"--no-verify", "--remove-section", "--replace-all", "--add", "--global", "--system", "--file", "-c"} or arg.startswith(("--force", "--unset")) or "hooksPath=" in arg for arg in lines
                ), "BLOCKED: forbidden Git route/bypass command attempted"
            else:
                assert name.startswith("lefthook-") and lines in (["-h"], ["install"], ["run", "pre-commit"], ["run", "--no-auto-install", "pre-commit"], ["run", "pre-commit", "--no-auto-install"]), "BLOCKED: unreviewed native argv"
        return result

    def tag(self, name: str, expected: int = 1) -> None:
        path = self.tags / name
        assert (path.read_text().splitlines() if path.exists() else []) == [name] * expected, f"BLOCKED: missing/duplicate {name}; retained {self.root}"

    def checks(self, *, global_route: bool = True) -> None:
        self.tag("GLOBAL_ROUTE_327", int(global_route))
        for name in ("SECRETS_327", "LEAKS_327", "DOCS_327"):
            self.tag(name)

    def native_run(self) -> None:
        calls = [p.read_text().splitlines() for p in self.calls.glob("lefthook-*")]
        assert sum(lines and lines[0] == "run" for lines in calls) == 1, "BLOCKED: native runner not observed exactly once"

    def commit(self) -> subprocess.CompletedProcess[str]:
        return self.measure("commit", [str(self.bin / "git"), "commit", "-m", "Synthetic issue327 check"])

    def setup(self) -> subprocess.CompletedProcess[str]:
        return self.measure("setup", [str(self.bin / "bash"), "scripts/setup.sh"])


def _no_reset_advice(text: str) -> None:
    assert not re.search(r"reset-hooks-path|config[^\n]*--unset[^\n]*core\.hookspath|disabl\w*[^\n]*hooks|hooks[^\n]*disabled", text, re.I), text


@pytest.mark.parametrize("case", ["setup-preserved-route", "native-commit-no-reset-advice"])
def test_global_hooks_scope(case):
    """Catch ambiguous setup or native autosync advice while checks still execute."""
    mini = Mini(case)
    if case == "setup-preserved-route":
        result = mini.setup()
        assert result.returncode == 0, f"BLOCKED: setup failed; {mini.root}"
        mini.tag("LAUNCHER_327")
        mini.tag("DOCTOR_327")
        text = result.stdout + result.stderr
        # Enforcement/preservation witnesses above precede the predicted output RED.
        lines = [line for line in text.splitlines() if str(mini.global_hooks) in line]
        assert any("retained" in line and "hooks/pre-commit" in line and "delegation not verified" in line for line in lines), text
        assert "lefthook install skipped" not in text
        _no_reset_advice(text)
    else:
        result = mini.commit()
        assert result.returncode == 0, f"BLOCKED: native commit failed; {mini.root}"
        mini.checks()
        mini.native_run()
        assert mini.run("head", [str(mini.bin / "git"), "rev-parse", "--verify", "HEAD"]).returncode == 0
        _no_reset_advice(result.stdout + result.stderr)


@pytest.mark.parametrize("case", ["failed-check-stays-failed", "no-global-normal-install", "no-lefthook-fallback", "unverified-global-not-overclaimed", "doctor-failure-propagates"])
def test_global_hooks_guard(case):
    """Catch weakened failure propagation, normal installation, fallback or truth."""
    mini = Mini(
        case,
        route="absent" if case == "no-global-normal-install" else "unverified" if case == "unverified-global-not-overclaimed" else "active",
        native=case != "no-lefthook-fallback",
        secrets=7 if case == "failed-check-stays-failed" else 0,
        doctor=9 if case == "doctor-failure-propagates" else 0,
    )
    if case == "failed-check-stays-failed":
        result = mini.commit()
        mini.tag("GLOBAL_ROUTE_327")
        mini.tag("SECRETS_327")
        mini.native_run()
        assert result.returncode != 0
        assert mini.run("no-head", [str(mini.bin / "git"), "rev-parse", "--verify", "HEAD"]).returncode != 0
    elif case == "no-global-normal-install":
        result = mini.setup()
        assert result.returncode == 0
        mini.tag("LAUNCHER_327")
        mini.tag("DOCTOR_327")
        installed = mini.repo / ".git/hooks/pre-commit"
        assert installed.is_file() and os.access(installed, os.X_OK)
        assert {p.name for p in installed.parent.iterdir()} == {"pre-commit", "pre-push"}
        assert mini.commit().returncode == 0
        mini.checks(global_route=False)
        mini.native_run()
    elif case == "no-lefthook-fallback":
        assert not (mini.bin / "lefthook").exists()
        assert mini.commit().returncode == 0
        mini.checks()
    elif case == "unverified-global-not-overclaimed":
        result = mini.setup()
        assert result.returncode == 0
        mini.tag("LAUNCHER_327")
        mini.tag("DOCTOR_327")
        mini.tag("GLOBAL_ROUTE_327", 0)
        text = (result.stdout + result.stderr).lower().replace("delegation not verified", "").replace("unverified", "")
        assert not re.search(r"(?:delegation|hooks|checks)[^\n]{0,50}\b(?:verified|active)\b|\b(?:verified|active)\b[^\n]{0,50}(?:delegation|hooks|checks)", text)
    else:
        result = mini.setup()
        mini.tag("DOCTOR_327")
        assert result.returncode == 9
        assert not re.search(r"^Ready\.", result.stdout, re.M)


def _config_query_snapshot(mini: Mini) -> dict[str, object]:
    """Snapshot this case's config/hook trees, excluding its growing evidence."""
    out = {}
    roots = {"home": mini.home, "repo_config": mini.repo / ".git/config", "repo_hooks": mini.repo / "hooks", "default_hooks": mini.repo / ".git/hooks"}
    for label, root in roots.items():
        if not root.exists():
            out[label] = {"absent": True}
            continue
        entries = [root, *sorted(root.rglob("*"))] if root.is_dir() else [root]
        tree = {}
        for entry in entries:
            assert not entry.is_symlink(), "BLOCKED: unexpected config/hook symlink"
            info = entry.stat()
            assert stat.S_ISDIR(info.st_mode) or stat.S_ISREG(info.st_mode), "BLOCKED: unexpected config/hook file type"
            tree[str(entry.relative_to(root))] = {
                "kind": "directory" if entry.is_dir() else "file",
                "mode": stat.S_IMODE(info.st_mode),
                "mtime_ns": info.st_mtime_ns,
                "bytes": entry.read_bytes().hex() if entry.is_file() else None,
            }
        out[label] = tree
    return out


def _config_query_world(case: str) -> tuple[Mini, Path]:
    mini = Mini(case)
    assert mini.root.is_relative_to((ROOT / ".metrics/issue327/retained").resolve())
    assert (mini.root / ".owned-327").read_text() == case
    assert mini.env["HOME"] == str(mini.home) and mini.env["PATH"] == str(mini.bin)
    assert mini.env["GIT_CONFIG_GLOBAL"] == str(mini.config) and mini.env["GIT_CONFIG_NOSYSTEM"] == "1"
    local_hooks = mini.home / "local-hooks"
    local_hooks.mkdir()
    assert local_hooks != mini.global_hooks
    assert local_hooks.is_relative_to(mini.root) and mini.global_hooks.is_relative_to(mini.root)
    with (mini.repo / ".git/config").open("a") as stream:
        stream.write(f"[core]\n\thooksPath = {local_hooks}\n")
    _json(mini.root / "fidelity-inputs.json", {"global_path": str(mini.global_hooks), "local_path": str(local_hooks), "seeded": _config_query_snapshot(mini)})
    return mini, local_hooks


def _config_query_run(mini: Mini, label: str, argv: list[str]) -> subprocess.CompletedProcess[str]:
    before = _config_query_snapshot(mini)
    prior_calls = {p.name for p in mini.calls.iterdir()}
    unused = mini.home / "unused-new-hooks"
    assert not unused.exists(), "BLOCKED: denial target already exists"
    result = mini.run(label, [str(mini.bin / "git"), *argv])
    after = _config_query_snapshot(mini)
    new_calls = sorted(p for p in mini.calls.iterdir() if p.name not in prior_calls)
    ledger = {p.name: p.read_text().splitlines() for p in new_calls}
    terminal_path = mini.root / f"{mini.sequence:02}-{label}.json"
    terminal = json.loads(terminal_path.read_text())
    _json(
        mini.root / f"{label}-fidelity.json",
        {
            "argv": argv,
            "code": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
            "before": before,
            "after": after,
            "changed_snapshot_keys": sorted(k for k in before.keys() | after.keys() if before.get(k) != after.get(k)),
            "call_ledger": ledger,
            "terminal_record": terminal_path.name,
            "terminal_sha256": _sha(terminal_path.read_bytes()),
            "unused_target_absent": not unused.exists(),
        },
    )
    assert before == after and not unused.exists(), "BLOCKED: config/hook preservation changed"
    assert len(ledger) == 1 and next(iter(ledger)).startswith("git-") and next(iter(ledger.values())) == argv, "BLOCKED: wrong or missing recorded query"
    assert terminal["leader_reaped"] and terminal["owned_group_gone"] and not terminal["blocked"], "BLOCKED: query cleanup failed"
    assert not list(mini.tags.iterdir()), "BLOCKED: a hook or sentinel ran during a config query"
    assert result.returncode in (0, 97) and result.stderr == "", "BLOCKED: unexpected Git/query execution error"
    return result


@pytest.mark.parametrize("scope", ["global", "local"], ids=["R01-global", "R02-local"])
def test_config_read_fidelity(scope):
    """The two native positional reads return their distinct owned values."""
    mini, local_hooks = _config_query_world(f"config-read-fidelity-{scope}")
    expected = mini.global_hooks if scope == "global" else local_hooks
    result = _config_query_run(mini, "read", ["config", f"--{scope}", "core.hooksPath"])
    assert result.returncode == 0 and result.stdout == f"{expected}\n", {"code": result.returncode, "stdout": result.stdout, "expected": f"{expected}\n"}


@pytest.mark.parametrize("case", ["D01-global-write", "D02-local-write", "D03-global-unset", "D04-local-unset-all", "D05-global-add", "D06-command-override"])
def test_config_write_denial(case):
    """Read fidelity cannot admit a configuration write or hook-path bypass."""
    mini, _local_hooks = _config_query_world(f"config-write-denial-{case}")
    unused = str(mini.home / "unused-new-hooks")
    argv = {
        "D01-global-write": ["config", "--global", "core.hooksPath", unused],
        "D02-local-write": ["config", "--local", "core.hooksPath", unused],
        "D03-global-unset": ["config", "--global", "--unset", "core.hooksPath"],
        "D04-local-unset-all": ["config", "--local", "--unset-all", "core.hooksPath"],
        "D05-global-add": ["config", "--global", "--add", "core.hooksPath", unused],
        "D06-command-override": ["-c", f"core.hooksPath={unused}", "config", "--get", "core.hooksPath"],
    }[case]
    result = _config_query_run(mini, "deny", argv)
    assert result.returncode == 97 and result.stdout == "" and result.stderr == "", {"code": result.returncode, "stdout": result.stdout, "stderr": result.stderr}
