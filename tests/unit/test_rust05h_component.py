"""rust-05h store helpers, key paths and data-show paths in synthetic homes."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from careerbrew import vault

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust05.py"
CANARY = "SYNTHETIC-RUST05-CANARY-7Q9"
SECRET = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE"
VALUE = "synthetic-outline-value-366"
PATH_GROUPS = [
    "P01-personal",
    "P02-cache-keys",
    "P03-cache-refusal",
    "P04-exact-once",
    "C01-personal",
    "C02-cache-keys",
    "C03-cache-refusal",
    "C04-missing",
    "C05-directory",
    "C06-traversal",
    "C07-symlink",
    "C08-absolute-inside",
    "C09-absolute-outside",
    "C11-lookalike",
    "C12-root-directory",
]


@pytest.fixture(scope="module")
def store_bin():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the cross-engine checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-vault", "--bin", "vault-store"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "vault-store"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "on",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str):
    return next(row for row in report["cases"] if row["id"] == case_id)


def assert_no_synthetic_values(root: Path, output: bytes) -> None:
    assert VALUE.encode() not in output
    for term in (CANARY.encode(), SECRET.encode()):
        assert term not in output
        for path in root.rglob("*"):
            if path.is_file() and not path.is_symlink() and path.name != "backup.key":
                raw = path.read_bytes()
                if not raw.startswith(vault.MAGIC):
                    assert term not in raw, path.relative_to(root)


def test_helpers_and_all_issue_366_groups_match_real_python(store_bin, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(store_bin), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        report = json.loads(done.stdout)
        ids = {row["id"] for row in report["cases"]}
        assert {
            "outline-projects",
            "outline-json",
            "outline-jsonl",
            "outline-text",
            "json-nonfinite-write",
            "json-nonfinite-read",
            "json-nonfinite-noop",
            "json-nonfinite-noop-identity",
            "outline-nonfinite",
            "outline-malformed-json",
            "outline-malformed-jsonl",
            "outline-malformed-yaml",
            "outline-key-48",
            "outline-key-48-newline",
            "outline-key-48-two-newlines",
            "outline-key-49",
            "outline-key-49-newline",
            "digest-values",
            "digest-empty",
            "cached-miss",
            "cached-hit",
            "cached-hit-identity",
            "cached-fresh",
            "cached-expired",
            "key-path-home",
            "key-path-home-file",
            "key-path-home-repeated-slash",
            "key-path-home-repeated-slashes",
            "key-path-home-traversal-lexical",
            "key-path-empty-default",
            "key-path-current-user",
            "key-path-current-user-file",
            "key-path-current-user-repeated-slash",
            "key-path-current-user-repeated-slashes",
            "key-path-current-user-traversal-lexical",
            "key-path-missing-user",
            "data-show-read-only",
        } <= ids
        assert case(report, "outline-projects")["result"]["keys"] == [
            "projects: map[2]",
            "projects.*: map",
            "projects.*.created_at: str",
            "projects.*.stale_after_days: int",
            "projects.*.sources: list",
            "projects.*.sources[]: map",
            "projects.*.sources[].kind: str",
            "projects.*.sources[].ref: str",
            "projects.*.owner: str",
            "people: map[1]",
            "people.*: int",
        ]
        assert case(report, "outline-json")["result"]["keys"] == [
            "cards: map[2]",
            "cards.*: map",
            "cards.*.due: str",
            "cards.*.reps: int",
            "cards.*.lapses: int",
            "by_id: map[40]",
            "by_id.*: int",
            "rows: list[2]",
            "rows[]: list",
            "rows[][]: int|str|null",
        ]
        assert case(report, "outline-jsonl")["result"]["keys"] == [
            "[]: map",
            "[].title: str",
            "[].tags: list",
            "[].tags[]: str",
            "[].at: int",
        ]
        assert case(report, "outline-text")["result"] == {
            "kind": "text",
            "sealed": True,
            "count": {"int": "3"},
            "keys": [],
        }
        expected_numbers = {
            "nan": {"float": "nan"},
            "infinity": {"float": "inf"},
            "negative_infinity": {"float": "-inf"},
            "negative_zero": {"float": (-0.0).hex()},
            "exponent": {"float": (1e-7).hex()},
            "high_scientific": {"float": (1e16).hex()},
            "literal": "NaN Infinity -Infinity",
            "big": {"int": "12089258196146291747061791208925819614629174706179"},
        }
        assert case(report, "json-nonfinite-write")["result"] is None
        assert case(report, "json-nonfinite-read")["result"] == expected_numbers
        assert case(report, "json-nonfinite-noop")["result"] is False
        assert case(report, "json-nonfinite-noop-identity")["result"] is True
        assert case(report, "outline-nonfinite")["result"]["keys"] == [
            "big: int",
            "exponent: float",
            "high_scientific: float",
            "infinity: float",
            "literal: str",
            "nan: float",
            "negative_infinity: float",
            "negative_zero: float",
        ]
        for case_id in ("outline-malformed-json", "outline-malformed-jsonl", "outline-malformed-yaml"):
            assert case(report, case_id)["result"] == {"error": "ValueError"}
        key_48 = "k" * 48
        assert case(report, "outline-key-48")["result"] == [f"{key_48}: int"]
        assert case(report, "outline-key-48-newline")["result"] == [f"{key_48}\n: int"]
        for case_id in ("outline-key-48-two-newlines", "outline-key-49", "outline-key-49-newline"):
            assert case(report, case_id)["result"] == ["*: int"]
        assert case(report, "digest-empty")["result"] == "e3b0c44298fc1c149afbf4c8"
        assert len(case(report, "digest-values")["result"]) == 24
        assert case(report, "cached-miss")["result"]["hit"] is False
        assert case(report, "cached-hit")["result"]["hit"] is True
        assert case(report, "cached-fresh")["result"]["hit"] is True
        assert case(report, "cached-expired")["result"]["hit"] is False
        for case_id in ("cached-miss", "cached-hit", "cached-fresh", "cached-expired"):
            assert case(report, case_id)["result"]["numbers"] == expected_numbers
        assert case(report, "cached-hit-identity")["result"] is True
        for case_id in (
            "key-path-home",
            "key-path-home-file",
            "key-path-home-repeated-slash",
            "key-path-home-repeated-slashes",
            "key-path-home-traversal-lexical",
            "key-path-empty-default",
            "key-path-current-user",
            "key-path-current-user-file",
            "key-path-current-user-repeated-slash",
            "key-path-current-user-repeated-slashes",
            "key-path-current-user-traversal-lexical",
        ):
            assert case(report, case_id)["result"] is True
        assert case(report, "key-path-missing-user")["result"] == {"error": "RuntimeError"}
        for group in PATH_GROUPS:
            result = case(report, f"data-show-{group}")["result"]
            assert result["passed"] is True, group
            assert VALUE not in json.dumps(result)
        assert case(report, "data-show-P01-personal")["result"]["alias"]["stdout"] == "current_level: 3\n"
        assert case(report, "data-show-P02-cache-keys")["result"]["alias"]["stdout"].splitlines()[1:] == [
            "count: int",
            "note: str",
        ]
        assert case(report, "data-show-P03-cache-refusal")["result"]["alias"]["exit"] == 3
        assert case(report, "data-show-read-only")["result"] is True
        assert case(report, "receipt-events")["result"] == [
            {"event": "approvals.request", "status": "pending"},
            {"event": "approvals.decide", "status": "approved"},
        ]
        assert_no_synthetic_values(root, done.stdout + done.stderr)
        answers.append(done.stdout)
    assert answers[0] == answers[1]
