"""Issue #274 (US-107): one long-lived session per project, talking through the store.

The main /work session runs only the work process. Each project session writes its steps (agent events), project
updates, and questions to careerbrew, claims a shared draft before it writes it, and ends the day with one handoff
per project. Synthetic names only."""

from __future__ import annotations

import json
from datetime import date
from pathlib import Path

import pytest

from careerbrew import cli, config, plugins, runs, server, store
from careerbrew.day import handoff
from careerbrew.work import agents, drafts, sessions
from careerbrew.workday import plan, questions, status, watch

ROOT = Path(__file__).resolve().parents[2]
CANARY = "zq-session-canary-7"
TARGET = "C0SYN/1700000000.000100"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    cli.main(["init"])
    for name in ("billing", "ledger", "search", "atlas"):
        cli.main(["work", "project", "add", name])


def _at(monkeypatch, stamp: str) -> None:
    monkeypatch.setenv("CAREERBREW_NOW", stamp)


def _business(monkeypatch, on: bool = True) -> None:
    monkeypatch.setattr(plugins, "effective_policy", lambda: {"agent_may_read_business": on})


# 1. One session per project, reused all day, under a cap.


def test_a_project_gets_one_session_reused_all_day(monkeypatch):
    s = sessions.open_session("billing", "sess-a1")
    assert (s["id"], s["project"], s["reused"]) == ("sess-a1", "billing", False)
    _at(monkeypatch, "2026-10-28T15:00+00:00")
    again = sessions.open_session("billing")
    assert (again["id"], again["reused"]) == ("sess-a1", True), "the same project later the same day gets its live session back"
    with pytest.raises(sessions.SessionError, match="already has a live session today, sess-a1"):
        sessions.open_session("billing", "sess-other")
    with pytest.raises(sessions.SessionError, match="belongs to another open session"):
        sessions.open_session("ledger", "sess-a1")
    with pytest.raises(sessions.SessionError, match="unknown project"):
        sessions.open_session("no-such-project", "sess-x")
    assert [r["id"] for r in sessions.listing()] == ["sess-a1"]


def test_conflicting_session_reports_recovery_commands(capsys):
    sessions.open_session("billing", "sess-old")
    with pytest.raises(sessions.SessionError) as error:
        sessions.open_session("billing", "sess-new")
    message = str(error.value)
    handoff = 'careerbrew work session handoff sess-old "<status and next step>"'
    end = "careerbrew work session end sess-old"
    assert handoff in message and end in message
    assert message.index(handoff) < message.index(end)
    assert cli.main(["work", "session", "handoff", "sess-old", "The old session is unavailable; next: resume work"]) == 0
    assert cli.main(["work", "session", "end", "sess-old"]) == 0
    assert sessions.open_session("billing", "sess-new")["id"] == "sess-new"
    capsys.readouterr()


def test_a_session_past_the_cap_is_refused_with_the_fix(capsys):
    config.set_value("work_session_cap", "2")
    sessions.open_session("billing", "s1")
    sessions.open_session("ledger", "s2")
    capsys.readouterr()
    assert cli.main(["work", "session", "open", "search", "--id", "s3"]) == 2
    out = capsys.readouterr().out
    assert "2 of 2 sessions are live (work_session_cap)" in out and "careerbrew config set work_session_cap" in out and "breaks US-107" in out
    assert len(sessions.listing()) == 2, "a refused open records nothing"


def test_opening_records_a_session_loop_that_day_loop_end_checks(capsys):
    sessions.open_session("billing", "sess-loop")
    running = watch.status()["running"]
    assert [(lp["kind"], lp["id"]) for lp in running] == [("session", "sess-loop")]
    capsys.readouterr()
    assert cli.main(["day", "loop", "end"]) == 1, "a live session keeps the day open"
    out = capsys.readouterr().out
    assert "careerbrew work session end sess-loop" in out, "end of day names the session's own end command"
    sessions.write_handoff("sess-loop", "Waiting on review; next: merge")
    sessions.end("sess-loop")
    assert watch.status()["running"] == [] and cli.main(["day", "loop", "end"]) == 0
    watch.add("session", "tool-only-session")  # a session the agent recorded by hand, not a project session
    capsys.readouterr()
    cli.main(["day", "loop", "end"])
    assert "careerbrew day loop stop --id tool-only-session" in capsys.readouterr().out


def test_a_session_from_an_earlier_day_is_stale_and_not_reused(monkeypatch):
    sessions.open_session("billing", "sess-old")
    _at(monkeypatch, "2026-10-29T09:00+00:00")
    assert sessions.listing()[0]["state"] == "stale"
    with pytest.raises(sessions.SessionError, match="no live session for this project yet"):
        sessions.open_session("billing")
    with pytest.raises(sessions.SessionError, match="opened on 2026-10-28"):
        sessions.live("sess-old")
    new = sessions.open_session("billing", "sess-new")
    assert new["reused"] is False and {r["state"] for r in sessions.listing()} == {"stale", "live"}


# 2. Sessions write through the store; the main session and the web app read it.


def test_a_sessions_steps_are_agent_events_under_its_project(capsys):
    sessions.open_session("billing", "sess-b")
    assert cli.main(["work", "agent", "event", "billing-bot", f"{CANARY} drafting the reply", "--session", "sess-b"]) == 0
    r = agents.runs()[0]
    assert (r["agent"], r["run"], r["project"]) == ("billing-bot", "sess-b", "billing"), "--session sets the run and the project"
    groups = server.agents_view()["groups"]
    assert groups[0]["project"] == "billing" and groups[0]["runs"][0]["run"] == "sess-b", "Work's Agent activity shows the session under its project"
    capsys.readouterr()
    assert cli.main(["work", "agent", "event", "billing-bot", "x", "--session", "no-such"]) == 2
    assert "no open session" in capsys.readouterr().out
    assert cli.main(["work", "agent", "event", "billing-bot", "x", "--session", "sess-b", "--project", "ledger"]) == 2
    assert "leave out --run and --project" in capsys.readouterr().out


def test_the_list_is_the_main_sessions_one_view(monkeypatch, capsys):
    sessions.open_session("billing", "sess-v")
    cli.main(["work", "agent", "event", "bot", "working", "--session", "sess-v"])
    sessions.take(drafts.uses_entry(TARGET), "sess-v")
    sessions.ask("sess-v", "Send the synthetic reply?", ["send", "hold"], "hold", "true")
    row = sessions.listing()[0]
    assert row["state"] == "live" and row["activity"]["status"] == "running" and row["activity"]["events"] == 1
    assert row["claims"][0]["resource"] == f"slack-draft:{TARGET}" and row["questions"] == 1 and row["has_handoff"] is False
    capsys.readouterr()
    assert cli.main(["work", "session", "list"]) == 0
    out = capsys.readouterr().out
    assert "sess-v" in out and "1 claim(s), 1 open question(s), handoff not yet" in out and "sessions: 1 of 3 live" in out


# 3. Claims on shared threads and drafts.


def test_a_claimed_draft_is_skipped_by_another_session(tmp_path, capsys):
    sessions.open_session("billing", "sess-1")
    sessions.open_session("ledger", "sess-2")
    f = tmp_path / "draft.txt"
    f.write_text("Looks good, approved.")
    assert cli.main(["draft", "replace", TARGET, "--file", str(f), "--session", "sess-1"]) == 0, "the writer claims it first"
    assert sessions.holder(drafts.uses_entry(TARGET))["session"] == "sess-1"
    capsys.readouterr()
    f.write_text("Please rename the flag.")
    assert cli.main(["draft", "replace", TARGET, "--file", str(f), "--session", "sess-2"]) == 1
    out = capsys.readouterr().out
    assert "claimed by session sess-1" in out and "skip it" in out
    assert drafts.LocalBackend().read(TARGET) == "Looks good, approved.", "the other session's write is skipped"
    assert cli.main(["draft", "replace", TARGET, "--file", str(f)]) == 1, "a writer with no session skips a claimed target too"
    assert cli.main(["work", "claim", "take", f"slack-draft:{TARGET}", "--session", "sess-2"]) == 1
    assert "claimed by session sess-1" in capsys.readouterr().out
    assert cli.main(["work", "claim", "take", f"slack-draft:{TARGET}", "--session", "sess-1"]) == 0, "the holder renews it"
    assert "renewed" in capsys.readouterr().out
    assert cli.main(["work", "claim", "release", f"slack-draft:{TARGET}", "--session", "sess-1"]) == 0
    assert cli.main(["draft", "replace", TARGET, "--file", str(f), "--session", "sess-2"]) == 0, "released: the next session may write"


def test_the_live_cycle_skips_a_claimed_target():
    sessions.open_session("billing", "sess-1")
    sessions.take(drafts.uses_entry(TARGET), "sess-1")
    with pytest.raises(drafts.DraftBusy):  # listen.py catches DraftError and moves on: the claimed thread is skipped
        drafts.replace(TARGET, "an ack", drafts.LocalBackend(), if_empty=True)
    assert drafts.LocalBackend().read(TARGET) is None


def test_a_claim_expires_and_ends_with_its_session(monkeypatch):
    config.set_value("work_claim_minutes", "30")
    sessions.open_session("billing", "sess-1")
    sessions.open_session("ledger", "sess-2")
    sessions.take("browser:sso", "sess-1")
    _at(monkeypatch, "2026-10-28T09:29+00:00")
    with pytest.raises(sessions.ClaimHeld):
        sessions.take("browser:sso", "sess-2")
    _at(monkeypatch, "2026-10-28T09:31+00:00")
    assert sessions.holder("browser:sso") is None, "a claim lapses work_claim_minutes after it was taken: a dead session never blocks forever"
    sessions.take("browser:sso", "sess-2")
    sessions.write_handoff("sess-2", "done for today")
    sessions.end("sess-2")
    assert sessions.holder("browser:sso") is None and sessions.claims() == [], "ending a session releases its claims"


def test_bad_claims_are_refused_with_the_fix():
    sessions.open_session("billing", "sess-1")
    with pytest.raises(sessions.SessionError, match="uses:"):
        sessions.take("thread with spaces", "sess-1")
    with pytest.raises(sessions.SessionError, match="no open session"):
        sessions.take("browser:sso", "sess-none")
    with pytest.raises(sessions.SessionError, match="1 to 480"):
        sessions.take("browser:sso", "sess-1", minutes=1000)


# 4. One question queue.


def test_session_questions_land_in_the_one_batched_ask(capsys):
    sessions.open_session("billing", "sess-q")
    sessions.open_session("ledger", "sess-r")
    assert cli.main(["work", "session", "ask", "sess-q", "Merge the synthetic PR?", "--option", "merge", "--option", "wait", "--pick", "wait", "--recheck", "true"]) == 0
    cli.main(["work", "session", "ask", "sess-r", "Reply in the synthetic thread?", "--recheck", "false"])
    questions.add("/careerbrew", "A main-session ask")
    got = questions.recheck()
    assert sorted(q["step"] for q in got["ask"]) == ["/careerbrew", "session:sess-q"], "every session's open asks are in the main session's one ask"
    assert [d["step"] for d in got["dropped"]] == ["session:sess-r"], "a session's closed ask is dropped like any other"
    assert questions.records(date(2026, 10, 28))[0]["source"] == "agent"


def test_a_session_ask_keeps_the_day_files_other_keys():
    config.set_value("work_checkouts", "")
    cli.main(["day", "update", "/careerbrew", "--status", "running", "--handled", "pr:example/app#1"])
    sessions.open_session("billing", "sess-k")
    sessions.ask("sess-k", "kept?")
    data = store.read_json(status.path_for(date(2026, 10, 28)))
    assert data["rows"] and data["handled"] == {"pr:example/app#1": "/careerbrew"}, "a session's question keeps rows and handled"


# 5. End of day: one handoff per project, then archive.


def test_end_needs_a_handoff_then_archives_and_releases_claims(capsys):
    sessions.open_session("billing", "sess-e")
    sessions.open_session("ledger", "sess-f")
    sessions.take("browser:sso", "sess-e")
    capsys.readouterr()
    assert cli.main(["work", "session", "end", "--all"]) == 1, "a session with no handoff stays open"
    out = capsys.readouterr().out
    assert "write its handoff first" in out and "0 ended, 2 still open" in out
    sessions.write_handoff("sess-e", f"{CANARY} review waits on the owner; next: merge", "https://github.example.com/org/shop/pull/7")
    sessions.write_handoff("ledger", "Reconciled; next: close the ticket")
    assert cli.main(["work", "session", "end", "--all"]) == 0
    assert sessions.listing() == [] and sessions.claims() == []
    archive = sessions.listing(include_ended=True)
    assert {r["id"]: r["state"] for r in archive} == {"sess-e": "ended", "sess-f": "ended"} and all(r["handoff"] for r in archive), "ended sessions are archived with their handoff"


def test_session_cli_explains_a_handoff_that_does_not_count(capsys, monkeypatch):
    _business(monkeypatch, False)
    sessions.open_session("billing", "sess-stale")
    sessions.write_handoff("sess-stale", CANARY)
    capsys.readouterr()

    assert cli.main(["work", "session", "list"]) == 0
    assert "handoff written" in capsys.readouterr().out
    assert sessions.request_wrap("synthetic-wrap") == ["sess-stale"]

    assert cli.main(["work", "session", "list"]) == 0
    listed = capsys.readouterr().out
    assert cli.main(["work", "session", "list", "--json"]) == 0
    row = json.loads(capsys.readouterr().out)["sessions"][0]
    assert row["has_handoff"] is False
    assert cli.main(["work", "session", "end", "--all"]) == 1
    refused = capsys.readouterr().out
    assert CANARY not in listed + refused
    assert [row["id"] for row in sessions.listing()] == ["sess-stale"]

    sessions.write_handoff("sess-stale", "Synthetic next step")
    assert cli.main(["work", "session", "end", "--all"]) == 0
    assert sessions.listing() == []

    explanations = (
        "handoff stale or unverified: write a fresh handoff" in listed,
        "write a fresh handoff" in refused and "without a current handoff" in refused,
    )
    assert explanations == (True, True)


def test_one_handoff_item_per_project():
    sessions.open_session("billing", "sess-h")
    sessions.write_handoff("sess-h", "first version")
    sessions.write_handoff("sess-h", "second version")
    items = [i for i in handoff.load() if i["section"] == "projects"]
    assert [i["text"] for i in items] == ["billing: second version"], "a new handoff replaces the project's last one"
    assert handoff.TITLES["projects"] == "Project sessions"


# 6. The agent contract.


def test_the_contract_says_main_runs_only_the_process_and_sessions_write_through_the_store():
    p = plan.build()
    text = p["sessions"]
    for needle in ("runs only the work process", "work session open", "--session", "work project update", "work session ask", "work claim take", "work session handoff", "work session end --all"):
        assert needle in text, needle
    skill = (ROOT / "skills/careerbrew-work-day/SKILL.md").read_text()
    command = (ROOT / "commands/work.md").read_text()
    for needle in ("runs only the work process", "work session open", "work claim take", "work session ask", "work session end --all"):
        assert needle in skill, f"skill: {needle}"
    assert "one session per project" in command and "work session open" in command


# 7. Privacy.


def test_projects_handoffs_and_claims_show_only_when_business_may_show(monkeypatch, capsys):
    sessions.open_session("billing", "sess-p")
    sessions.take(f"file:/synthetic/{CANARY}", "sess-p")
    sessions.write_handoff("sess-p", f"{CANARY} handoff")
    capsys.readouterr()
    for argv in (["work", "session", "list"], ["work", "session", "list", "--json"], ["work", "claim", "list"], ["work", "claim", "list", "--json"]):
        assert cli.main(argv) == 0
        out = capsys.readouterr().out
        assert CANARY not in out and "billing" not in out, argv
    _business(monkeypatch)
    cli.main(["work", "session", "list", "--json"])
    row = json.loads(capsys.readouterr().out)["sessions"][0]
    assert row["project"] == "billing" and CANARY in row["handoff"]
    raw = sessions.path().read_bytes()
    assert CANARY.encode() not in raw and b"billing" not in raw, "the store is encrypted at rest"


def test_the_work_day_page_lists_a_sessions_question():
    sessions.open_session("billing", "sess-pg")
    sessions.ask("sess-pg", "Send it?")
    page = runs.work_day(plan.build())
    assert "Question from session:sess-pg (0 min old)" in [i.label for i in page.next], "the main session's page lists each session's ask with its age"


# Review fixes (PR #303).


def test_claim_expiry_compares_times_not_strings_across_utc_offsets(monkeypatch):
    sessions.open_session("billing", "sess-1")
    sessions.open_session("ledger", "sess-2")
    st = sessions._load()
    # Valid until 10:30 UTC, written with a +02:00 offset ("12:30+02:00"); now is 10:00 UTC stamped "06:00-04:00":
    # as strings "12:30+02:00" > "06:00-04:00" holds either way, so test the case where strings disagree with time.
    st["claims"] = [{"resource": "browser:sso", "session": "sess-1", "at": "2026-10-28T05:00:00-04:00", "until": "2026-10-28T09:30:00-04:00"}]
    store.write_json(sessions.path(), st)
    _at(monkeypatch, "2026-10-28T14:00+01:00")  # 13:00 UTC: the claim (13:30 UTC) still holds, though "09:30" < "14:00" as text
    assert sessions.holder("browser:sso") is not None
    with pytest.raises(sessions.ClaimHeld):
        sessions.take("browser:sso", "sess-2")


def test_a_held_claim_error_hides_the_resource_unless_business_may_show(tmp_path, capsys, monkeypatch):
    sessions.open_session("billing", "sess-1")
    sessions.open_session("ledger", "sess-2")
    sessions.take(drafts.uses_entry(TARGET), "sess-1")
    capsys.readouterr()
    assert cli.main(["work", "claim", "take", f"slack-draft:{TARGET}", "--session", "sess-2"]) == 1
    out = capsys.readouterr().out
    assert "C0SYN" not in out and "slack-draft:(hidden)" in out
    f = tmp_path / "d.txt"
    f.write_text("x")
    assert cli.main(["draft", "replace", TARGET, "--file", str(f)]) == 1
    out = capsys.readouterr().out
    assert "C0SYN" not in out and "slack-draft:(hidden)" in out
    _business(monkeypatch)
    cli.main(["work", "claim", "take", f"slack-draft:{TARGET}", "--session", "sess-2"])
    assert "C0SYN" in capsys.readouterr().out


def test_zero_minutes_is_refused_not_the_default():
    sessions.open_session("billing", "sess-1")
    with pytest.raises(sessions.SessionError, match="1 to 480"):
        sessions.take("browser:sso", "sess-1", minutes=0)
