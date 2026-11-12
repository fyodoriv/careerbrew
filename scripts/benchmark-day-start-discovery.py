#!/usr/bin/env python3
"""Measure update discovery in isolated, synthetic checkout worlds."""

import hashlib
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
import types
from pathlib import Path
from unittest.mock import patch

from careerbrew import config, paths, plugins
from careerbrew.workday import self_update, start


def seed(repo: Path, remote: str) -> None:
    def git(*args: str) -> None:
        subprocess.run(
            ["git", "-C", str(repo), "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgSign=false", "-c", "user.name=Synthetic Fixture", "-c", "user.email=fixture@example.com", *args],
            check=True,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            timeout=15,
        )

    git("init", "--quiet", "--template=")
    git("config", "remote.origin.url", remote)
    git("add", ".")
    git("commit", "--quiet", "--no-gpg-sign", "-m", "synthetic source")


def world(root: Path) -> tuple[Path, Path]:
    own, other = root / "self", root / "other"
    (own / "commands").mkdir(parents=True)
    (other / "commands").mkdir(parents=True)
    (own / "Agentfile.yaml").write_text("commands: [./commands]\n")
    for name in ("alpha", "beta"):
        (other / "commands" / f"{name}.md").write_text("synthetic\n")
    seed(own, str(root / "self-origin.git"))
    seed(other, str(root / "other-origin.git"))
    return own, other


def run(root: Path, candidate: bool, baseline_run) -> tuple[float, str, list[tuple[str, ...]]]:
    own, other = root / "self", root / "other"
    declared = [{"step": f"/{name}", "path": str(other / "commands" / f"{name}.md"), "uses": []} for name in ("alpha", "beta")]
    calls: list[tuple[str, ...]] = []
    refreshes: list[str] = []
    repairs: list[list[str]] = []
    real_git = start._git

    def observe(where, *argv, **kwargs):
        assert Path(where) in (own, other, other / "commands")
        assert argv in (("rev-parse", "--show-toplevel"), ("rev-parse", "--short=12", "HEAD"), ("config", "--get", "remote.origin.url"))
        calls.append(argv)
        return real_git(where, *argv, **kwargs)

    with (
        patch.object(paths, "REPO_ROOT", own),
        patch.object(config, "get", side_effect=lambda name: ["careerbrew"] if name == "work_checkouts" else None),
        patch.object(plugins, "installed", return_value=[]),
        patch.object(self_update, "declared_steps", return_value=declared),
        patch.object(start, "_git", side_effect=observe),
        patch.object(start, "refresh_one", side_effect=lambda top: refreshes.append(top) or {"status": "current", "top": top}),
        patch.object(self_update, "repair", side_effect=lambda argv, cwd: repairs.append(argv) or True),
        patch.object(self_update, "launcher_ready", return_value=True),
        patch.object(self_update, "primary_agent_roots", return_value={"synthetic": own}),
        patch.object(self_update.shutil, "which", return_value="/synthetic/agentbrew"),
    ):
        before = time.perf_counter_ns()
        result = self_update.run() if candidate else baseline_run()
        elapsed_ms = (time.perf_counter_ns() - before) / 1_000_000
    evidence = json.dumps([result, refreshes, repairs], sort_keys=True).replace(str(root), "<world>")
    return elapsed_ms, evidence, calls


def main() -> None:
    git_path = Path(shutil.which("git") or "").resolve(strict=True)
    python_path = Path(sys.executable).resolve(strict=True)
    repo = Path(__file__).resolve().parent.parent
    sources = {"candidate": repo / "src/careerbrew/workday/self_update.py", "discovery": repo / "src/careerbrew/workday/start.py", "benchmark": Path(__file__).resolve()}
    assert Path(self_update.__file__).resolve() == sources["candidate"]
    assert Path(start.__file__).resolve() == sources["discovery"]
    identities = {"git_sha256": hashlib.sha256(git_path.read_bytes()).hexdigest(), "python_sha256": hashlib.sha256(python_path.read_bytes()).hexdigest()}
    identities.update({f"{name}_sha256": hashlib.sha256(path.read_bytes()).hexdigest() for name, path in sources.items()})

    def verify_identity():
        assert hashlib.sha256(git_path.read_bytes()).hexdigest() == identities["git_sha256"]
        assert hashlib.sha256(python_path.read_bytes()).hexdigest() == identities["python_sha256"]
        for name, path in sources.items():
            assert hashlib.sha256(path.read_bytes()).hexdigest() == identities[f"{name}_sha256"]

    original = subprocess.run([str(git_path), "show", "b9217cf36263c0a880dcc32441cae00d86bf2cd4:src/careerbrew/workday/self_update.py"], check=True, capture_output=True, timeout=15).stdout
    namespace = dict(vars(self_update))
    exec(compile(original, "<registered-baseline>", "exec"), namespace)  # noqa: S102
    baseline_run = types.FunctionType(namespace["run"].__code__, vars(self_update))
    Path(".metrics").mkdir(exist_ok=True)
    fd, receipt = tempfile.mkstemp(prefix="day-start-discovery-", suffix=".jsonl", dir=".metrics")
    print(f"local receipt: {receipt}", file=sys.stderr, flush=True)
    with os.fdopen(fd, "w") as log:

        def record(value):
            log.write(json.dumps(value, sort_keys=True) + "\n")
            log.flush()
            os.fsync(log.fileno())

        record({"event": "start", "baseline_revision": "b9217cf36263c0a880dcc32441cae00d86bf2cd4", "baseline_sha256": hashlib.sha256(original).hexdigest(), **identities})
        try:
            with tempfile.TemporaryDirectory(prefix="careerbrew-discovery-") as name:
                root = Path(name).resolve()
                clean: dict[str, str] = {}
                clean.update(
                    {
                        "HOME": str(root),
                        "PATH": str(git_path.parent) + os.pathsep + "/usr/bin:/bin",
                        "GIT_CONFIG_GLOBAL": os.devnull,
                        "GIT_CONFIG_NOSYSTEM": "1",
                        "GIT_ATTR_NOSYSTEM": "1",
                        "GIT_TERMINAL_PROMPT": "0",
                        "GIT_ALLOW_PROTOCOL": "",
                        "GIT_AUTHOR_DATE": "2026-11-03T09:00:00-04:00",
                        "GIT_COMMITTER_DATE": "2026-11-03T09:00:00-04:00",
                        "GIT_CEILING_DIRECTORIES": str(root),
                    }
                )
                with patch.dict(os.environ, clean, clear=True):
                    assert Path(shutil.which("git") or "").resolve(strict=True) == git_path
                    for label in ("baseline", "candidate"):
                        world(root / label)
                    samples = []
                    for pair in range(-2, 20):
                        results = {}
                        record({"event": "pair_start", "pair": pair + 1, "warmup": pair < 0})
                        for label in ("baseline", "candidate") if pair % 2 == 0 else ("candidate", "baseline"):
                            record({"event": "member_start", "pair": pair + 1, "variant": label})
                            verify_identity()
                            results[label] = run(root / label, label == "candidate", baseline_run)
                            verify_identity()
                            record({"event": "member", "pair": pair + 1, "variant": label, "elapsed_ms": results[label][0]})
                        before, after = results["baseline"], results["candidate"]
                        assert before[1] == after[1]
                        assert before[2].count(("rev-parse", "--show-toplevel")) == 7 and len(before[2]) == 11
                        assert after[2].count(("rev-parse", "--show-toplevel")) == 3 and len(after[2]) == 7
                        sample = {"event": "pair", "pair": pair + 1, "warmup": pair < 0, "baseline_ms": before[0], "candidate_ms": after[0], "saved_ms": before[0] - after[0], "saved_pct": 100 * (before[0] - after[0]) / before[0]}
                        record(sample)
                        if pair >= 0:
                            samples.append(sample)
                    report = {"event": "summary", "median_saved_ms": statistics.median(s["saved_ms"] for s in samples), "median_saved_pct": statistics.median(s["saved_pct"] for s in samples)}
                    record(report)
                    print(json.dumps(report, indent=2))
                    if report["median_saved_ms"] < 5 or report["median_saved_pct"] < 10:
                        raise SystemExit("registered timing threshold missed")
        except BaseException as error:
            record({"event": "failure", "error_type": type(error).__name__})
            raise


if __name__ == "__main__":
    main()
