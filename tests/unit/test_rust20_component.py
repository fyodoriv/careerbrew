"""rust-20 native tutor, public-deck, Notion, and provider assertions."""

from __future__ import annotations

import json
import math
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust20.py"
CASES = {
    "depvec-fsrs",
    "fsrs-old-state",
    "cards-and-decks",
    "local-day-stats",
    "public-deck",
    "notion-idempotence",
    "notion-validation-and-partial-writes",
    "providers-and-mistakes",
}
MARKERS = (b"RUST20CANARYPERSON", b"RUST20CANARYEMPLOYER", b"RUST20CANARYANSWER")
# FSRS uses f32 in the maintained Rust crate and Python floats in py-fsrs. Compare
# the actual full-precision values; never replace them with placeholders.
FLOAT_ABS_TOLERANCE = 0.00001
FLOAT_REL_TOLERANCE = 0.000001


@pytest.fixture(scope="module")
def candidate():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-tutor", "--bin", "tutor-component"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "tutor-component"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)["result"]


def assert_same(actual, expected, path: str = "report", floats: list[str] | None = None):
    """Compare typed reports, with numeric tolerance only at actual float leaves."""
    if floats is None:
        floats = []
    if isinstance(expected, dict) and set(expected) == {"float"}:
        left = float(actual["float"])
        right = float(expected["float"])
        assert math.isfinite(left) and math.isfinite(right), path
        assert math.isclose(left, right, abs_tol=FLOAT_ABS_TOLERANCE, rel_tol=FLOAT_REL_TOLERANCE), (
            path,
            left,
            right,
        )
        floats.append(path)
        return floats
    assert type(actual) is type(expected), (path, type(actual), type(expected))
    if isinstance(expected, dict):
        assert set(actual) == set(expected), path
        for key in expected:
            assert_same(actual[key], expected[key], f"{path}.{key}", floats)
    elif isinstance(expected, list):
        assert len(actual) == len(expected), path
        for index, (left, right) in enumerate(zip(actual, expected, strict=True)):
            assert_same(left, right, f"{path}[{index}]", floats)
    else:
        assert actual == expected, (path, actual, expected)
    return floats


def test_tutor_public_notion_and_provider_contracts_match_python(candidate, tmp_path):
    reports = []
    compared_floats = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(candidate), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=180)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert all(marker not in done.stdout + done.stderr for marker in MARKERS)
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert set(ids) == CASES
        assert all("error" not in row["result"] for row in report["cases"])
        reports.append(report)

        vectors = case(report, "depvec-fsrs")
        assert vectors["vector_count"] == {"int": "18"}
        assert [row["id"] for row in vectors["results"][:5]] == ["parameters", "new-again", "new-hard", "new-good", "new-easy"]
        assert [row["output"] for row in vectors["results"][-2:]] == [{"error": True}, {"error": True}]
        assert all("error" not in row["output"] for row in vectors["results"][:-2])

        old = case(report, "fsrs-old-state")
        assert old["old_roundtrip"] is True
        assert old["old"] == old["old_file"], "old state is lossless before numeric tolerance applies"
        assert old["old"]["due"] == "2026-10-21T09:10:00.123456+00:00"
        assert old["old"]["card_id"] == old["new"]["card_id"] == {"int": "424242"}
        assert old["new"]["state"] == {"int": "2"} and old["new"]["step"] is None

        deck = case(report, "cards-and-decks")
        assert len(deck["first"]) == 4
        assert len({row["topic"] for row in deck["first"]}) >= 3
        assert deck["isolation"]["learn_ids"] == ["learn-c1"]
        assert "learn-c1" not in deck["isolation"]["interview_ids"]
        assert deck["isolation"]["learn_state"] is True
        assert deck["isolation"]["interview_has_learn_state"] is False
        assert deck["isolation"]["learn_has_interview_deck"] is False
        assert deck["leech"]["leech"] is True and deck["leech"]["lapses"] == {"int": "8"}
        assert deck["suspended_absent"] is True and deck["unsuspended_present"] is True

        local = case(report, "local-day-stats")
        assert local["first_count"] == {"int": "3"}
        assert local["later_new"] == []
        assert local["reviewed_today"] == {"int": "3"}
        assert local["streak_days"] == local["next_day_streak"] == {"int": "1"}

        public = case(report, "public-deck")
        assert public["initial_hints"]["Tell me about a conflict"] == ""
        assert public["owner_cards"] == ["n1", "n2"]
        assert public["private_sealed"] is True
        assert all(card_id.startswith("kb-") for card_id in public["public_ids"])
        assert public["partial_write"] == {"error": "PublishError", "unchanged": True}
        assert public["leak_refusal"] == "PublishError"
        assert public["check"] == [] and public["flags_cleared"] is True

        notion = case(report, "notion-idempotence")
        assert notion["before"] == notion["after"]
        assert notion["check"]["ok"] is True and notion["check"]["expected"] == {"int": "19"}
        assert notion["scheduled_kept"] is True and notion["reviewed_today"] == {"int": "1"}
        assert notion["deck_count"] == {"int": "4"} and notion["sealed"] is True
        assert notion["origins"]["apps"] == ["notion", "notion", "notion", "user"]
        assert notion["skills"]["python"] == {"int": "4"}

        partial = case(report, "notion-validation-and-partial-writes")
        assert partial["missing_kind"] == "ImportError_"
        assert not any(partial["missing_kind_writes"].values())
        assert partial["partial_error"] == "RuntimeError"
        assert partial["partial"]["library"] is True
        assert partial["partial"]["board"] is True
        assert partial["partial"]["applications"] is True
        assert partial["partial"]["deck"] is False
        assert partial["partial"]["profile"] is False
        assert partial["partial"]["bundle_sealed"] is False
        assert partial["recovered_check"]["ok"] is True
        assert partial["role_error"] == "RoleError" and not any(partial["role_writes"].values())

        providers = case(report, "providers-and-mistakes")
        assert providers["added"] == ["gap-rust-1"] and providers["repeated"] == []
        assert providers["item"]["origin"] == "generated"
        assert providers["cards"]["gap-rust-1"]["card_id"] == {"int": "424243"}
        assert providers["attempt_count"] == {"int": "1"}
        assert providers["next_review"]["mode"] == "review"
        assert providers["rating_thresholds"] == ["Again", "Hard", "Hard", "Good", "Good", "Easy", "Easy"]
        assert providers["mistakes"]["deck_count"] == {"int": "2"}
        assert providers["mistakes"]["second_repeat"] is True
        assert providers["mistakes"]["from_exercise"] == ["edge-case"]

    assert_same(reports[1], reports[0], floats=compared_floats)
    assert len(compared_floats) >= 8, "full-precision FSRS and mastery values were really compared"


def test_native_direct_controls():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    done = subprocess.run(
        ["cargo", "test", "--locked", "--offline", "-p", "careerbrew-tutor", "--test", "component"],
        cwd=ROOT,
        capture_output=True,
        timeout=180,
    )
    assert done.returncode == 0, done.stdout.decode(errors="replace") + done.stderr.decode(errors="replace")
