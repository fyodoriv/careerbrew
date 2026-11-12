"""Issue #272: background agents report each step while they work, and Work's Agent activity shows it live.

`careerbrew work agent event <agent> "<title>"` appends one row to `work/agents/events.jsonl`; the file is under
`work/`, so `/api/changes` moves at once and the live block refetches. `/api/agents` groups runs by project,
each with its last event; a run silent past `agent_quiet_minutes` shows as quiet."""

import json

import pytest

from careerbrew import cli, config, paths, server
from careerbrew.work import agents


@pytest.fixture
def agenthome(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    return home


def _at(monkeypatch, stamp: str) -> None:
    monkeypatch.setenv("CAREERBREW_NOW", stamp)


def test_one_event_moves_the_change_version_at_once_so_live_views_refetch(agenthome):
    before = server.changes_view()["version"]
    agents.record("review-bot", "Reviewing PR 7", project="billing", url="https://github.example.com/org/shop/pull/7")
    assert server.changes_view()["version"] != before, "an agent event must move /api/changes, or the UI waits for the cycle's end"


def test_the_view_groups_runs_by_project_each_with_its_last_event_and_link(agenthome, monkeypatch):
    agents.record("review-bot", "Reviewing PR 7", project="billing", url="https://github.example.com/org/shop/pull/7")
    _at(monkeypatch, "2026-10-21T10:05")
    agents.record("fix-bot", "Fixing the flaky test", project="ledger")
    _at(monkeypatch, "2026-10-21T10:10")
    agents.record("review-bot", "Left 3 comments on PR 7", status="done", project="billing", url="https://github.example.com/org/shop/pull/7")
    agents.record("standup-bot", "Refreshed the standup rows")
    v = server.agents_view()
    assert v["counts"] == {"running": 2, "quiet": 0, "done": 1, "failed": 0}
    names = [g["project"] for g in v["groups"]]
    assert names == ["", "ledger", "billing"], "groups with a running run first, newest first; no project is the empty group"
    billing = v["groups"][2]["runs"]
    assert len(billing) == 1, "one run per agent: its last event wins"
    r = billing[0]
    assert (r["agent"], r["status"], r["title"], r["url"], r["events"]) == ("review-bot", "done", "Left 3 comments on PR 7", "https://github.example.com/org/shop/pull/7", 2)
    assert r["started_at"] == "2026-10-21T14:00:00+00:00" and r["at"] == "2026-10-21T14:10:00+00:00"


def test_a_running_run_that_goes_silent_shows_as_quiet_and_a_new_running_event_restarts_a_finished_run(agenthome, monkeypatch):
    agents.record("fix-bot", "Started", project="ledger")
    _at(monkeypatch, "2026-10-21T11:01")
    assert agents.runs()[0]["status"] == "quiet", "silent past agent_quiet_minutes (60): never running forever"
    config.set_value("agent_quiet_minutes", "120")
    assert agents.runs()[0]["status"] == "running", "the limit is a setting"
    agents.record("fix-bot", "Done", status="done")
    agents.record("fix-bot", "Started again", project="ledger")
    r = agents.runs()[0]
    assert (r["status"], r["events"], r["started_at"]) == ("running", 1, r["at"]), "running after done starts the run again"


def test_a_second_done_event_keeps_the_run_and_its_start(agenthome, monkeypatch):
    agents.record("fix-bot", "Started")
    _at(monkeypatch, "2026-10-21T10:05")
    agents.record("fix-bot", "Done", status="done")
    _at(monkeypatch, "2026-10-21T10:06")
    agents.record("fix-bot", "Done, and the PR is merged", status="done")
    r = agents.runs()[0]
    assert (r["events"], r["started_at"]) == (3, "2026-10-21T14:00:00+00:00"), "only a running event starts a finished run again"


def test_parallel_runs_of_one_agent_keep_apart_by_run_id(agenthome):
    agents.record("review-bot", "PR 7", run="pr-7", project="billing")
    agents.record("review-bot", "PR 8", run="pr-8", project="billing")
    assert sorted(r["run"] for r in agents.runs()) == ["pr-7", "pr-8"]


@pytest.mark.parametrize(
    ("kwargs", "fix"),
    [
        ({"agent": "", "title": "x"}, "bad agent name"),
        ({"agent": "a b", "title": "x"}, "bad agent name"),
        ({"agent": "bot", "title": "  "}, "title is empty"),
        ({"agent": "bot", "title": "x" * 201}, "limit is 200"),
        ({"agent": "bot", "title": "x", "status": "paused"}, "bad status"),
        ({"agent": "bot", "title": "x", "url": "javascript:alert(1)"}, r"full http\(s\) URL"),
        ({"agent": "bot", "title": "x", "url": "/local/path"}, r"full http\(s\) URL"),
        ({"agent": "bot", "title": "x", "project": "p" * 81}, "limit is 80"),
    ],
)
def test_bad_input_is_refused_names_the_fix_and_writes_nothing(agenthome, kwargs, fix):
    with pytest.raises(agents.AgentError, match=fix):
        agents.record(**kwargs)
    assert agents.load() == []


def test_a_multi_line_title_is_one_line(agenthome):
    assert agents.record("bot", "line one\nline two")["title"] == "line one line two"


def test_rows_older_than_agent_keep_days_are_dropped_and_never_shown(agenthome, monkeypatch):
    agents.record("old-bot", "long ago")
    _at(monkeypatch, "2026-10-29T10:00")
    assert agents.runs() == [], "older than agent_keep_days (7): not shown"
    agents.record("new-bot", "now")
    assert [r["agent"] for r in agents.load()] == ["new-bot"], "and dropped at the next write"


def test_events_are_encrypted_at_rest_and_the_event_log_gets_the_status_only(agenthome, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    agents.record("bot", "CANARYAGENTTITLE step", url="https://example.com/CANARYAGENTURL", project="CANARYPROJECT")
    raw = paths.sub("work", "agents", "events.jsonl").read_bytes()
    assert b"CANARYAGENT" not in raw and b"CANARYPROJECT" not in raw, "business text stays encrypted on disk"
    log = paths.sub("logs", "events.jsonl").read_text()
    assert "work.agent.event" in log and "CANARY" not in log


def test_the_cli_reports_one_step_and_prints_titles_only_where_business_data_may_show(agenthome, capsys):
    assert cli.main(["work", "agent", "show"]) == 0
    assert "no agent activity yet" in capsys.readouterr().out
    assert cli.main(["work", "agent", "event", "review-bot", "CANARYAGENT Reviewing PR 7", "--project", "billing", "--url", "https://github.example.com/org/shop/pull/7"]) == 0
    assert "event stored: review-bot running" in capsys.readouterr().out
    assert cli.main(["work", "agent", "show"]) == 0
    out = capsys.readouterr().out
    assert "1 run(s): 1 running, 0 quiet, 0 done, 0 failed" in out and "billing" in out
    assert "CANARYAGENT" not in out and "github.example.com" not in out, "an agent sees counts and names, never titles or links"
    assert cli.main(["work", "agent", "show", "--json"]) == 0
    d = json.loads(capsys.readouterr().out)
    assert d["counts"]["running"] == 1 and "title" not in d["runs"][0]


def test_a_bad_event_from_the_cli_names_the_story(agenthome, capsys):
    assert cli.main(["work", "agent", "event", "bot", "x", "--url", "ftp://x"]) == 2
    assert "breaks US-91" in capsys.readouterr().out


def test_the_web_view_is_a_cached_minute_view(agenthome):
    assert server.CACHED_VIEWS["/api/agents"] == "minute", "quiet depends on the minute; a write moves files_version"
