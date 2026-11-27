"""rust-23 native model routes, eval judge, labels and voice assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust23.py"
CASES = [
    "routes-config",
    "allowed-routes",
    "complete-routing",
    "judge-text",
    "judge-cache",
    "http-transports",
    "labels",
    "agreement-run",
    "voice-transcribe",
    "voice-answer",
]
MARKERS = (b"RUST23CANARYTERM", b"rust23-synthetic-token")
# The autouse home fixture changes HOME between tests. Keep Cargo's real registry
# instead of resolving a new, empty $HOME/.cargo for the build.
CARGO_HOME = os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-voice", "--bin", "voice-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
        env={**os.environ, "CARGO_HOME": CARGO_HOME},
    )
    return ROOT / "target" / "debug" / "voice-component"


def synthetic_env(root: Path) -> dict[str, str]:
    for name in ("data", "home", "tmp", "keys"):
        (root / name).mkdir(parents=True)
    (root / "data").chmod(0o700)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(root / "data"),
        "TMPDIR": str(root / "tmp"),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_LLM": "off",
        "CAREERBREW_NOW": "2026-10-19T09:00:00",
        "CAREERBREW_RUST23_REPO": str(ROOT),
        "LC_ALL": "C",
        "PYTHONHASHSEED": "0",
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def count_floats(value) -> int:
    if isinstance(value, dict):
        if set(value) == {"float"}:
            return 1
        return sum(count_floats(item) for item in value.values())
    if isinstance(value, list):
        return sum(count_floats(item) for item in value)
    return 0


def test_routes_judge_and_voice_contracts_match_python(candidate, tmp_path):
    texts = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=300)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
        report = json.loads(done.stdout)
        assert [row["id"] for row in report["cases"]] == CASES
        assert all("error" not in row["result"] for row in report["cases"])
        texts.append(done.stdout)

        # Local route first: the first route that answers wins, and it is local.
        first = case(report, "complete-routing")["first-answers"]
        assert first["result"] == {"ok": ["local answer", "local"]}
        assert [row["url"] for row in first["seen"]] == ["http://localhost:11434/v1/chat/completions"]

        # Ollama judge routes send reasoning_effort and max_tokens.
        ollama = case(report, "judge-cache")["errors"]["http-404-ollama"]
        assert {"reasoning_effort", "max_tokens"} <= set(ollama["seen_keys"])

        # A judge answer counts only with finish_reason: stop.
        results = case(report, "http-transports")["results"]
        assert results[0] == {"ok": "HTTP answer one"}
        assert all("incomplete or has no completion reason" in row["message"] for row in results[1:3])

        # Audio and transcripts stay temporary.
        assert case(report, "voice-transcribe")["temp-dirs-left"] == {"int": "0"}
        assert count_floats(report) == 12

    assert texts[1] == texts[0], "the native component prints the Python oracle's bytes"


def test_native_unit_tests_pass():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    done = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "-p", "careerbrew-voice"],
        cwd=ROOT,
        capture_output=True,
        timeout=300,
        env={**os.environ, "CARGO_HOME": CARGO_HOME},
    )
    assert done.returncode == 0, done.stdout.decode(errors="replace") + done.stderr.decode(errors="replace")
