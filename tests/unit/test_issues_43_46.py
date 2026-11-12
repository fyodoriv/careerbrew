"""Issues #43 (no duplicate log items), #44 (reviews today only), #46 (who added a task and why)."""

import pytest

from careerbrew import connectors
from careerbrew.cli import main
from careerbrew.day import inbox, results
from careerbrew.day import log as daylog
from careerbrew.work import priorities


def test_a_change_from_pr_and_commits_is_listed_once(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    from datetime import date

    day = date(2026, 10, 16)
    results.append(
        [
            {"id": "commit:a", "at": "2026-10-16T10:00", "kind": "commit", "title": "feat: invoice export", "source": "evidence", "origin": "user"},
            {"id": "pr:1", "at": "2026-10-16T11:00", "kind": "pr", "title": "feat: invoice export (#41)", "source": "evidence", "origin": "user"},
            {"id": "commit:b", "at": "2026-10-16T11:05", "kind": "commit", "title": "Feat:  Invoice export", "source": "evidence", "origin": "user"},
        ],
        day,
    )
    d = daylog.build(day)
    assert d["done"]["Shipped"] == ["feat: invoice export (#41)"], "one line, the PR's"


def test_review_count_is_for_today_and_keyed_by_day(home, monkeypatch):
    monkeypatch.setenv("CB_T", "x")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T12:00+00:00")
    rev_day = "2026-10-16T09:00:00Z"
    seen = []

    def fake(url, headers, body=None):
        seen.append(url)
        if "reviewed-by" in url:
            return {"items": [{"number": 1, "repository_url": "https://api.github.com/repos/o/r"}], "total_count": 1}
        if "/reviews" in url:
            return [{"user": {"login": "me"}, "submitted_at": rev_day}]
        return {"items": []}

    monkeypatch.setattr(connectors, "http_json", fake)
    items = connectors.github({"token": "env:CB_T", "user": "me"}, "user")
    review_query = next(u for u in seen if "reviewed-by" in u)
    assert "updated%3A%3E%3D" in review_query, "the review count has a date qualifier"
    (rev,) = [i for i in items if i["id"].startswith("github:reviews-")]
    assert rev["title"] == "Reviewed 1 pull requests" and rev["id"].endswith(rev["at"][:10])


def test_agent_added_task_needs_why_and_waits_for_acceptance(home, monkeypatch, capsys):
    monkeypatch.delenv("CAREERBREW_TEST_OWNER", raising=False)
    assert main(["work", "task", "Refactor billing"]) != 0, "no why"
    assert main(["work", "task", "Refactor billing", "--source", "user"]) == 3, "an agent cannot claim to be the user"
    assert main(["work", "task", "Refactor billing", "--why", "PR review asked for it", "--link", "https://github.example.com/o/r/pull/9"]) == 0
    assert "waits in the inbox" in capsys.readouterr().out
    t = priorities.load()[0]
    assert t["status"] == "proposed" and t["source"] == "agent" and t["why"] == "PR review asked for it" and t["added_at"]
    assert priorities.rank({}) == [], "not planned before acceptance"
    inbox.regenerate()
    item = next(i for i in inbox.open_items() if i["kind"] == "agent-task")
    assert "added by agent: PR review asked for it" in item["text"]
    inbox.accept(item["id"])
    assert [x["id"] for x in priorities.rank({})] == [t["id"]]


def test_owner_task_is_planned_at_once_and_log_names_agent_tasks(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T17:45")  # after work hours: both tasks are Monday's
    assert main(["work", "task", "Write the design doc", "--hours", "1"]) == 0
    assert priorities.load()[0]["status"] == "open" and priorities.load()[0]["source"] == "user"
    t = priorities.add("Fix flaky test", 3, 3, 1, [], source="agent", why="CI failed twice")
    from careerbrew.day.inbox import _set_task_status

    _set_task_status(t["id"], "open")
    from datetime import date

    planned = " ".join(daylog.build(date(2026, 10, 16))["planned"])
    assert "added by agent: CI failed twice" in planned and "Write the design doc" in planned


def test_dismissed_agent_task_is_never_planned(home):
    t = priorities.add("Rewrite everything", 3, 3, 1, [], source="agent", why="seemed nice")
    inbox.regenerate()
    inbox.dismiss(next(i for i in inbox.open_items() if i["kind"] == "agent-task")["id"])
    assert next(x for x in priorities.load() if x["id"] == t["id"])["status"] == "dismissed"
    assert priorities.rank({}) == []


def test_missing_why_for_agents_in_the_core(home):
    with pytest.raises(ValueError, match="--why"):
        priorities.add("x", 3, 3, 1, [], source="agent")


def test_doctor_catches_venv_scripts_from_an_old_folder(tmp_path):
    """Issue #45: after the repo moves, .venv/bin scripts keep shebangs to the old folder."""
    import shutil
    import subprocess
    from pathlib import Path

    root = Path(__file__).resolve().parents[2]
    repo = tmp_path / "moved"
    (repo / "scripts").mkdir(parents=True)
    shutil.copy(root / "scripts" / "doctor.sh", repo / "scripts" / "doctor.sh")
    (repo / ".venv" / "bin").mkdir(parents=True)
    (repo / ".venv" / "bin" / "pytest").write_text("#!/old/place/.venv/bin/python\n")
    out = subprocess.run(["bash", "scripts/doctor.sh"], cwd=repo, capture_output=True, text=True, check=False).stdout
    assert "FAIL  venv scripts point at another folder (/old/place/.venv/bin/python)" in out and "uv sync --reinstall" in out
    (repo / ".venv" / "bin" / "pytest").write_text(f"#!{repo.resolve()}/.venv/bin/python\n")
    out = subprocess.run(["bash", "scripts/doctor.sh"], cwd=repo, capture_output=True, text=True, check=False).stdout
    assert "ok    venv scripts point at this repo" in out
