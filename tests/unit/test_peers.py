"""US-113: `/indie` and `/work` improve each other. Each side detects the other's lessons and source changes,
trials one change at a time on its own metric, and adopts or declines by the number. Synthetic text only."""

from __future__ import annotations

import json
import os
import subprocess
from pathlib import Path

import pytest

from careerbrew import cli, machine, peers
from careerbrew.workday import timing

NOW = "2026-11-19T10:00+00:00"
ROOT = Path(__file__).resolve().parents[2]


def _git(path: Path, *args: str) -> str:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    done = subprocess.run(["git", "-C", str(path), "-c", "user.name=t", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false", *args], check=True, capture_output=True, text=True, env=env)
    return done.stdout.strip()


def _write(root: Path, rel: str, text: str, message: str) -> str:
    path = root / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    _git(root, "add", "-f", rel)
    _git(root, "commit", "-q", "-m", message)
    return _git(root, "rev-parse", "--short", "HEAD")


@pytest.fixture(autouse=True)
def checkout(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    cli.main(["init"])
    root = tmp_path / "checkout"
    root.mkdir()
    _git(root, "init", "-q", "-b", "main")
    _write(root, "skills/careerbrew-indie/LESSONS.md", "# Lessons\n", "docs: indie lessons")
    _write(root, "skills/careerbrew-work-day/LESSONS.md", "# Lessons\n\n- 2026-10-28: an older free-form line\n", "docs: work lessons")
    monkeypatch.setenv("CAREERBREW_PEER_ROOT", str(root))
    return root


def _runs(monkeypatch, failed: int, runs: int = 10, median: int | None = 600) -> None:
    monkeypatch.setattr(timing, "summary", lambda today, n_days=7: {"steps": [{"step": "/a", "runs": runs, "done": runs - failed, "blocked": 1, "failed": failed, "median": median}], "days": []})


def test_each_side_has_its_own_personality_and_metrics():
    assert "founder" in peers.SIDES["indie"]["persona"] and "operator" in peers.SIDES["work"]["persona"]
    assert set(peers.SIDES["indie"]["metrics"]) == {"hours_per_outcome", "asks_per_outcome", "outcomes_per_week"}
    assert set(peers.SIDES["work"]["metrics"]) == {"step_fail_rate", "step_blocked_rate", "step_minutes"}
    for side in peers.SIDES.values():
        assert (ROOT / side["skill"] / "SKILL.md").is_file() and all((ROOT / s).exists() for s in side["sources"])


def test_a_side_detects_its_peers_new_lessons_and_source_commits(checkout):
    start = _git(checkout, "rev-parse", "--short", "HEAD")
    _write(checkout, "skills/careerbrew-work-day/LESSONS.md", "# Lessons\n\n- [L-work-1] Prove each step with its verify command (metric step_fail_rate: 0.2 -> 0.1, KEEP)\n", "docs: a work lesson")
    _write(checkout, "commands/work.md", "changed\n", "feat: a work change")
    _write(checkout, "README.md", "not a work source\n", "docs: elsewhere")
    got = peers.status("indie")
    assert got["unreviewed_lessons"] == ["L-work-1"] and len(got["peer_commits"]) == 3  # no marker yet: the last 30 days
    assert got["peer"] == "work" and "L-work-1" in got["next"][0] and "--lesson L-work-1" in got["next"][0]
    assert peers.status("work")["unreviewed_lessons"] == []  # indie has no lesson yet
    _write(checkout, "skills/careerbrew-indie/LESSONS.md", f"# Lessons\n\n- [L-work-1] declined: the indie day has no verify step yet\nPeer source reviewed at: {start}\n", "docs: indie reviews work")
    got = peers.status("indie")
    assert got["unreviewed_lessons"] == [] and got["declined"] == 1 and got["reviewed_at"] == start
    assert len(got["peer_commits"]) == 2  # the lesson and the command change; the README commit is not a work source
    head = _git(checkout, "rev-parse", "--short", "HEAD")
    _write(checkout, "skills/careerbrew-indie/LESSONS.md", f"# Lessons\n\n- [L-work-1] declined: no verify step yet\n- [L-indie-1] Try each new bet once before ranking it\nPeer source reviewed at: {head}\n", "docs: marker")
    assert peers.status("indie")["peer_commits"] == [] and peers.status("indie")["next"] == []
    assert peers.status("work")["unreviewed_lessons"] == ["L-indie-1"]  # and now work has one to review


def test_one_trial_at_a_time_and_not_before_three_days(monkeypatch, capsys):
    _runs(monkeypatch, failed=4)
    assert cli.main(["peer", "experiment", "--side", "work", "--start", "Give every step a verify command", "--metric", "step_fail_rate"]) == 0
    assert "baseline 0.4" in capsys.readouterr().out
    assert cli.main(["peer", "experiment", "--side", "work", "--start", "A second change at once", "--metric", "step_minutes"]) == 2
    assert "one change at a time" in capsys.readouterr().out
    assert cli.main(["peer", "experiment", "--side", "work", "--close"]) == 2
    assert "needs at least 3" in capsys.readouterr().out
    assert peers.start("indie", "Batch every ask at the end of the day", "asks_per_outcome")["id"] == "x-indie-1"  # the other side has its own slot


@pytest.mark.parametrize(("failed", "verdict"), [(2, "KEEP"), (6, "REVERT"), (4, "NEUTRAL")])
def test_a_trial_ends_keep_revert_or_neutral_by_a_ten_percent_margin(monkeypatch, failed, verdict):
    _runs(monkeypatch, failed=4)
    peers.start("work", "Give every step a verify command", "step_fail_rate")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-24T10:00+00:00")
    _runs(monkeypatch, failed=failed)
    got = peers.close("work")
    assert got["verdict"] == verdict and got["baseline"] == 0.4 and got["value"] == failed / 10
    if verdict == "KEEP":
        assert got["write"] == "- [L-work-1] Give every step a verify command (metric step_fail_rate: 0.4 -> 0.2, KEEP)"
    else:
        assert got["write"] == ""
    assert peers.open_experiment("work") is None


def test_a_peer_lesson_is_adopted_or_declined_with_its_number(monkeypatch, checkout, capsys):
    _write(checkout, "skills/careerbrew-indie/LESSONS.md", "# Lessons\n\n- [L-indie-1] Try each new thing once before ranking it\n", "docs: an indie lesson")
    _runs(monkeypatch, failed=4)
    with pytest.raises(peers.PeerError, match="names a lesson of indie"):
        peers.start("work", "Try each new step once", "step_fail_rate", "L-work-9")
    assert cli.main(["peer", "experiment", "--side", "work", "--start", "Try each new step once before ordering it", "--metric", "step_fail_rate", "--lesson", "L-indie-1"]) == 0
    assert peers.status("work")["open_trial"]["lesson"] == "L-indie-1"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-24T10:00+00:00")
    _runs(monkeypatch, failed=6)
    capsys.readouterr()
    assert cli.main(["peer", "experiment", "--side", "work", "--close"]) == 0
    out = capsys.readouterr().out
    assert "REVERT" in out and "- [L-indie-1] declined: Try each new step once before ordering it (metric step_fail_rate: 0.4 -> 0.6, REVERT)" in out
    assert cli.main(["peer", "experiment", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["experiments"][0]["verdict"] == "REVERT"


def test_no_data_is_never_a_keep(monkeypatch):
    _runs(monkeypatch, failed=0, runs=0)
    assert peers.measure("work", "step_fail_rate", 14) is None
    assert peers.verdict("work", "step_fail_rate", None, 0.1) == "NO-DATA" and peers.verdict("indie", "outcomes_per_week", 2.0, 2.5) == "KEEP"
    assert peers.verdict("indie", "outcomes_per_week", 2.0, 1.5) == "REVERT"
    _runs(monkeypatch, failed=1, median=None)
    assert peers.measure("work", "step_minutes", 14) is None and peers.measure("work", "step_blocked_rate", 14) == 0.1


def test_status_prints_both_sides_and_runs_on_a_corporate_machine(capsys):
    assert cli.main(["peer", "status", "--json"]) == 0
    sides = json.loads(capsys.readouterr().out)["sides"]
    assert [s["side"] for s in sides] == ["indie", "work"] and sides[0]["metrics"]["hours_per_outcome"] is None
    machine.set_role("corporate", "")
    assert cli.main(["peer", "status", "--for", "work"]) == 0  # /work runs there, so its side of the exchange must too
    assert "the operator" in capsys.readouterr().out


def test_bad_input_names_the_story(capsys):
    assert cli.main(["peer", "experiment", "--side", "indie", "--start", "short", "--metric", "hours_per_outcome"]) == 2
    assert cli.main(["peer", "experiment", "--side", "indie", "--start", "A long enough change", "--metric", "step_fail_rate"]) == 2
    assert cli.main(["peer", "experiment", "--start", "A long enough change"]) == 2
    assert capsys.readouterr().out.count("breaks US-113") == 3
