"""rust-18 native evidence, promotion, ladder grade and LinkedIn assertions."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust18.py"
CASES = {"evidence-tags", "promo-assess", "ladder-grade", "linkedin-block"}
MARKERS = (b"SYNTHCANARY",)


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-career", "--bin", "career-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "career-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    (root / "home").mkdir()
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def written(root: Path) -> dict[str, str]:
    """Every file the run left, by relative path: sealed files by kind, plain files by text."""
    out = {}
    for path in sorted((root / "data").rglob("*")):
        if not path.is_file() or path.name == "writes.log":
            continue
        raw = path.read_bytes()
        rel = str(path.relative_to(root / "data"))
        out[rel] = "key" if rel.endswith("keys/backup.key") else "sealed" if raw.startswith(b"CBV1\n") else raw.decode()
    return out


def operations(root: Path) -> list[str]:
    rows = []
    for log in sorted((root / "data").rglob("writes.log")):
        rows += [line.replace(str(root), "<root>") for line in log.read_text().splitlines()]
    return rows


def test_evidence_promotion_ladder_and_linkedin_match_python(candidate, tmp_path):
    answers, trees, ops = [], [], []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS), engine
        report = json.loads(done.stdout)
        assert {row["id"] for row in report["cases"]} == CASES

        ev = case(report, "evidence-tags")
        assert ev["tags"][:3] == [["delivery"], ["quality"], ["impact", "quality"]]
        assert ev["first"]["tags"] == ["collaboration", "custom", "ownership"]
        assert ev["first"]["id"] == ev["again_id"]
        assert ev["second"]["links"] == ["https://example.com/doc/1"]
        assert ev["second"]["at"] == "2026-11-03T11:00:00+00:00"
        assert ev["loaded"] == {"int": "3"} and ev["forgot"] == {"int": "2"} and ev["after"] == [ev["second"]["id"]]

        promo = case(report, "promo-assess")
        assert promo["assessed"]["overall"] == {"float": "0.348"} and promo["staff_overall"] == {"float": "0.175"}
        assert promo["gaps"][0] == "delivery"
        assert promo["assessed"]["warnings"] == ["competency delivery matches 3 of 9 evidence items (over 30%); narrow its signals"]
        assert promo["keywords"][0] == [True, False, False, False, False, False]
        assert promo["errors"][-1] == "LadderError: level principal not in ['mid', 'senior', 'staff']"
        assert promo["doc_path"] == "out/promo-generic-swe-senior.md"
        assert promo["doc"].startswith("# Promotion case: senior\n")
        assert "summary by fallback" in promo["doc"]

        ladder = case(report, "ladder-grade")
        assert ladder["target"] == ["generic-swe", "senior"]
        assert ladder["added"] == {"int": "5"} and ladder["again"] == {"int": "0"}
        assert all(row["at"] == "2026-11-02T18:00:00-05:00" for row in ladder["stored"])
        assert all(any(tag.startswith("dim:") for tag in row["tags"]) for row in ladder["stored"])
        assert ladder["unknown"] == []

        block = case(report, "linkedin-block")
        assert block["terms_count"] == {"int": "1"}
        assert block["built"]["bullets"] == {"int": "4"} and block["since"]["bullets"] == {"int": "1"}
        assert block["built"]["text"].startswith("Senior engineer. Focused on impact, design, ownership.\n\n• ")
        assert block["file"] == block["since"]["text"] + "\n"
        assert block["built"]["path"] == "out/linkedin.md"
        answers.append(done.stdout)
        trees.append(written(root))
        ops.append(operations(root))
    assert answers[0] == answers[1]
    assert trees[0] == trees[1]
    assert ops[0] == ops[1] and ops[0]
