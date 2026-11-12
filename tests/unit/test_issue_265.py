"""Issue #265: the work day ends by clock: wrap-up at `work_end` minus a lead time, the PM draft is the end-of-day output, and the handoff is rechecked next morning."""

from __future__ import annotations

import json
from types import SimpleNamespace

import pytest

from careerbrew import cli, config, paths, report, store
from careerbrew.day import handoff
from careerbrew.day import log as daylog
from careerbrew.day import watch as day_watch
from careerbrew.workday import end, gate, questions, start, status, watch

FIXTURES = paths.REPO_ROOT / "tests" / "fixtures"
CANARY = "zq-endofday-canary-5"
DAY = "2026-10-29"
NEXT = "2026-10-30"


def at(hhmm: str, day: str = DAY) -> str:
    return f"{day}T{hhmm}+00:00"


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00"))
    cli.main(["init"])
    for key, value in {"work_end": "17:30", "work_wrap_minutes": "30", "run_pages": "false"}.items():
        config.set_value(key, value)


@pytest.fixture
def fake(tmp_path, monkeypatch):
    """The fake notifier (tests/fixtures/notifier/) as terminal-notifier: it logs each call and shows nothing."""
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    log = tmp_path / "notifier.log"
    monkeypatch.setenv("PATH", f"{bin_dir}:/usr/bin:/bin")
    monkeypatch.setenv("FAKE_NOTIFIER_LOG", str(log))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setattr(day_watch, "notify", lambda text: pytest.fail("the osascript fallback must not run here"))
    (bin_dir / "terminal-notifier").symlink_to(FIXTURES / "notifier" / "fake-notifier.sh")
    for key, value in {"notify_events": "true", "notify_backend": "terminal-notifier", "notify_quiet_hours": "none"}.items():
        config.set_value(key, value)

    def calls() -> list[list[str]]:
        out: list[list[str]] = []
        for line in log.read_text().splitlines() if log.exists() else []:
            if line.startswith("--- "):
                out.append([line[4:]])
            else:
                out[-1].append(line)
        return out

    return SimpleNamespace(calls=calls)


def _day_file(day: str = DAY) -> dict:
    from datetime import date

    return store.read_json(status.path_for(date.fromisoformat(day)), {}) or {}


# ---------- acceptance: wrap-up starts at the configured time ----------


def test_wrap_up_starts_at_work_end_minus_the_lead_time(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", at("16:59"))
    assert end.tick()["phase"] == "work", "a minute before work_end minus 30 min: still the work phase"
    assert "wrapup" not in _day_file()
    monkeypatch.setenv("CAREERBREW_NOW", at("17:00"))
    got = end.tick()
    assert got["phase"] == "wrap-up" and got["started"], "work_end 17:30 minus 30 min: wrap-up starts at 17:00"
    assert got["wrap_at"] == at("17:00")
    assert _day_file()["wrapup"]["at"] == at("17:00")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:10"))
    again = end.tick()
    assert again["phase"] == "wrap-up" and not again["started"], "wrap-up starts once a day"


def test_the_lead_time_is_a_setting(monkeypatch):
    config.set_value("work_wrap_minutes", "45")
    monkeypatch.setenv("CAREERBREW_NOW", at("16:45"))
    assert end.tick()["phase"] == "wrap-up"
    with pytest.raises(ValueError):
        config.set_value("work_wrap_minutes", "-5")


def test_wrap_up_reminds_once_through_the_notifier(fake, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", at("17:05"))
    first = end.tick()
    end.tick()
    monkeypatch.setenv("CAREERBREW_NOW", at("17:20"))
    end.tick()
    calls = fake.calls()
    assert len(calls) == 1, f"one reminder a day, not {len(calls)}"
    assert first["reminded"] == "sent"
    assert calls[0][0] == "terminal-notifier"
    assert "careerbrew day end" in " ".join(calls[0]), "the reminder names the one next step"


def test_no_pop_up_when_notifications_are_off(fake, monkeypatch):
    config.set_value("notify_events", "false")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "on")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:05"))
    assert end.tick()["reminded"] == "off"
    assert fake.calls() == [], "notifications are opt-in: wrap-up still starts, with no pop-up"


def test_wrap_up_keeps_the_rows_and_start_record_of_the_day_file(monkeypatch):
    status.save_start({"at": at("09:00"), "refresh": []})
    status.update("/careerbrew", ["/careerbrew"], status="done", summary="did it")
    questions.add("/careerbrew", f"Merge the {CANARY} draft?")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:05"))
    end.tick()
    data = _day_file()
    assert data["start"]["at"] == at("09:00"), "every writer of the day file keeps the keys it does not own"
    assert data["rows"][0]["status"] == "done"
    assert len(data["questions"]) == 1


def test_a_pass_in_wrap_up_keeps_the_cycles_running(tmp_path, monkeypatch):
    marker = tmp_path / "ran"
    config.set_value("work_steps", "/repo-watch")
    monkeypatch.setattr(watch, "_live_cycle", lambda: {"status": "off"})
    monkeypatch.setenv("CAREERBREW_NOW", at("17:05"))
    got = watch.run_pass(runner=lambda cmd, s: (marker.write_text("x") or True, ""))
    assert got["status"] == "ok"
    assert got["phase"] == "wrap-up", "the pass names the phase"
    assert not watch.status()["ended"], "wrap-up keeps the loops running"


def test_a_pass_at_work_end_ends_the_day_and_the_run_loop_stops(monkeypatch):
    monkeypatch.setattr(watch, "_live_cycle", lambda: {"status": "off"})
    monkeypatch.setenv("CAREERBREW_NOW", at("17:30"))
    passes = watch.run(sleep=lambda s: None, clock=iter(range(10**6)).__next__, max_passes=5)
    assert passes == 1, "the pass at work_end ends the day, so the run loop stops after it"
    assert watch.status()["ended"]
    assert _day_file()["end"]["by"] == "clock"
    assert watch.run_pass()["phase"] == "ended", "an explicit pass after end of day runs, and the day stays ended"


# ---------- acceptance: the end-of-day output is the PM draft, not a new report ----------


def _files() -> set[str]:
    root = paths.data_root()
    return {str(p.relative_to(root)) for p in root.rglob("*") if p.is_file() and "logs" not in p.parts}


def _unexpected_end_files(new: set[str]) -> set[str]:
    allowed = {f"day/log-{DAY}.md", f"day/plan-{DAY}.json", f"day/pm-shown-{DAY}.json", f"day/work-{DAY}.json", f"day/handoff-{DAY}.json", "day/loops.json", "work/.sessions.json.lock"}
    lock = paths.data_root() / "work/feed/.lock"
    if lock.is_file() and not lock.is_symlink() and lock.stat().st_size == 0:
        allowed.add("work/feed/.lock")
    return new - allowed


def test_end_of_day_allowlist_rejects_nonempty_lock_and_extra_report():
    lock = paths.data_root() / "work/feed/.lock"
    lock.parent.mkdir(parents=True, exist_ok=True)
    lock.write_text("synthetic content")
    assert _unexpected_end_files({"work/feed/.lock", "work/extra-report.md"}) == {"work/feed/.lock", "work/extra-report.md"}
    lock.write_text("")
    assert _unexpected_end_files({"work/feed/.lock", "work/extra-report.md"}) == {"work/extra-report.md"}
    lock.unlink()
    target = lock.parent / "synthetic-target"
    target.touch()
    lock.symlink_to(target)
    assert _unexpected_end_files({"work/feed/.lock"}) == {"work/feed/.lock"}


def test_end_of_day_output_is_the_pm_draft_not_a_new_report(monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    before = _files()
    got = end.finish()
    pm = daylog.pm_report(daylog.build())
    assert got["pm"]["body"] == pm["body"], "end of day reads careerbrew's own PM draft"
    assert got["pm"]["subject"] == pm["subject"]
    new = {f for f in _files() - before if not f.startswith("day/.")}
    unexpected = _unexpected_end_files(new)
    assert not unexpected, f"end of day writes no second report: {sorted(unexpected)}"
    assert not [p for p in report.folder().glob("*.html")] if report.folder().exists() else True, "run_pages off: no page"
    assert got["dashboard"] == "careerbrew open --view dashboard", "it links the dashboard, where the PM draft is edited and sent"
    assert _day_file()["end"]["pm"]["edited"] is False


def test_the_pm_draft_the_owner_edited_is_the_one_shown(monkeypatch):
    daylog.save_pm_draft(DAY, f"Daily update: {CANARY} shipped.")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    got = end.finish()
    assert got["pm"]["body"] == f"Daily update: {CANARY} shipped."
    assert got["pm"]["edited"]


def test_day_end_cli_prints_counts_not_the_draft_for_an_agent(monkeypatch, capsys):
    daylog.save_pm_draft(DAY, f"Daily update: {CANARY} shipped.")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    capsys.readouterr()
    rc = cli.main(["day", "end"])
    out = capsys.readouterr().out
    assert rc == 0
    assert CANARY not in out, "the PM draft is business text: never on stdout for an agent"
    assert "PM draft" in out and "careerbrew open --view dashboard" in out
    assert "end of day:" in out
    assert cli.main(["day", "end", "--json"]) == 0
    data = json.loads(capsys.readouterr().out)
    assert CANARY not in json.dumps(data)
    assert data["pm"]["edited"] is True and "body" not in data["pm"]


def test_day_end_prints_the_required_loop_stop_id_flag(monkeypatch, capsys):
    watch.add("monitor", "monitor-395")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    capsys.readouterr()
    assert cli.main(["day", "end"]) == 1
    assert "careerbrew day loop stop --id monitor-395" in capsys.readouterr().out


def test_end_of_day_stops_the_loops(monkeypatch):
    watch.add("cron", "cron-1")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    got = end.finish()
    assert [lp["id"] for lp in got["loops"]["running"]] == ["cron-1"], "a loop only the agent can stop is named"
    assert watch.status()["ended"]


# ---------- acceptance: the handoff, and the next morning's recheck ----------


def _seed_day(tmp_path):
    """A day with an open PR (a row's link), a handled PR, a promise-free wait, and two open questions."""
    status.update("/review-prs", ["/review-prs"], status="done", link="https://github.com/example/shop/pull/12", handled_items=["pr:example/tools#7"])
    keep = tmp_path / "still-open"
    keep.write_text("x")
    gone = tmp_path / "will-close"
    gone.write_text("x")
    q_open = questions.add("/review-prs", f"Merge the {CANARY} draft?", recheck=f"test -e {keep}")
    q_gone = questions.add("/review-prs", "Rename the flag?", recheck=f"test -e {gone}")
    q_link = questions.add("/review-prs", "Is the deploy done?", recheck="https://example.com/deploys/1")
    handoff.add("waiting", "Waiting on the design review")
    return SimpleNamespace(keep=keep, gone=gone, q_open=q_open, q_gone=q_gone, q_link=q_link)


def test_end_of_day_writes_open_prs_and_open_questions_into_the_handoff(tmp_path, monkeypatch):
    s = _seed_day(tmp_path)
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    got = end.finish()
    items = {i["id"]: i for i in handoff.load()}
    prs = [i for i in items.values() if i["section"] == "prs"]
    assert {i.get("link", "") for i in prs} == {"https://github.com/example/shop/pull/12", ""}, "a handled pr:<repo>#<n> names no host, so it has no link"
    assert {i["text"].split(" from ")[0] for i in prs} == {"PR example/shop#12", "PR example/tools#7"}
    assert all("gh pr view" in i["recheck"] and "MERGED|CLOSED" in i["recheck"] for i in prs), "an open PR's recheck fails once it merges or closes"
    assert "--repo" in next(i["recheck"] for i in prs if "tools" in i["text"])
    asks = [i for i in items.values() if i["section"] == "questions"]
    assert {i["id"] for i in asks} == {s.q_open["id"], s.q_gone["id"], s.q_link["id"]}, "every open question record goes into the handoff"
    assert items[s.q_open["id"]]["recheck"] == s.q_open["recheck"]
    assert any(i["section"] == "waiting" for i in items.values()), "items the agent added stay"
    assert got["handoff"]["prs"] == 2 and got["handoff"]["questions"] == 3


def test_ending_twice_keeps_one_item_each_and_drops_a_question_closed_since(tmp_path, monkeypatch):
    s = _seed_day(tmp_path)
    monkeypatch.setenv("CAREERBREW_NOW", at("17:05"))
    end.tick()
    questions.close(s.q_gone["id"])
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    end.finish()
    end.finish()
    ids = [i["id"] for i in handoff.load()]
    assert len(ids) == len(set(ids))
    assert s.q_gone["id"] not in ids, "a question closed after wrap-up started leaves the handoff"


def test_next_morning_shows_each_handoff_item_with_its_recheck_result(tmp_path, monkeypatch):
    s = _seed_day(tmp_path)
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    end.finish()
    s.gone.unlink()  # that ask got answered overnight: its recheck now exits non-zero
    real = gate.run_verify

    def verify(command: str):
        if command.startswith("sh -c"):  # a PR's recheck: #12 is still open, #7 merged
            return (True, "") if "shop" in command else (False, "exit 1")
        return real(command)

    monkeypatch.setattr(gate, "run_verify", verify)
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00", NEXT))
    got = end.morning()
    assert got["from"] == DAY
    states = {r["id"]: r["state"] for r in got["items"]}
    pr12 = next(i["id"] for i in handoff.load(__import__("datetime").date.fromisoformat(DAY)) if i.get("link", "").endswith("/shop/pull/12"))
    assert states[pr12] == "open"
    assert states[s.q_open["id"]] == "open"
    assert states[s.q_gone["id"]] == "closed"
    assert states[s.q_link["id"]] == "agent", "a link recheck is for the agent to check"
    assert "closed" in states.values() and sum(1 for v in states.values() if v == "closed") == 2, "the merged PR and the answered ask are closed"
    assert s.q_gone["id"] not in {q["id"] for q in questions.open_records()}, "a closed ask is closed in the question store too"
    left = {i["id"] for i in handoff.latest_before()[1]}
    assert s.q_gone["id"] not in left and pr12 in left, "closed items leave the handoff; open ones stay"
    assert CANARY not in json.dumps(got), "the morning record holds ids, states, and reasons only"


def test_day_start_records_and_prints_the_handoff_recheck(tmp_path, monkeypatch, capsys):
    s = _seed_day(tmp_path)
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    end.finish()
    s.gone.unlink()
    monkeypatch.setattr(gate, "run_verify", lambda c: (False, "exit 1") if c.startswith("sh -c") or str(s.gone) in c else (True, ""))
    monkeypatch.setattr(start, "checkouts", lambda declared, *, lookup=None: [])
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00", NEXT))
    capsys.readouterr()
    assert cli.main(["day", "start"]) == 0
    out = capsys.readouterr().out
    assert f"handoff from {DAY}:" in out
    assert "3 closed" in out, "2 PRs merged and 1 ask answered"
    assert CANARY not in out
    rec = status.start_record()
    assert rec["handoff"]["from"] == DAY and len(rec["handoff"]["items"]) >= 4


def test_morning_with_no_handoff_is_empty():
    assert end.morning() == {"from": "", "items": []}


def test_plan_names_the_wrap_up_time_and_phase(monkeypatch):
    from careerbrew.workday import plan

    p = plan.build()
    assert p["wrap_up"] == {"at": at("17:00"), "phase": "work", "command": "careerbrew day end", "hold": False, "end_at": at("17:30")}
    assert any("careerbrew day end" in line for line in p["end_of_day"])
    assert not any("Today / Tomorrow" in line for line in p["end_of_day"]), "no second report"


@pytest.mark.parametrize(("answer", "exit_code", "state"), [("OPEN", 0, "open"), ("MERGED", 0, "closed"), ("CLOSED", 0, "closed"), ("", 4, "open")])
def test_a_pr_recheck_closes_only_on_a_merged_or_closed_answer(tmp_path, monkeypatch, answer, exit_code, state):
    """A gh that is missing, signed out, or offline must never drop an open PR from the handoff."""
    stub = tmp_path / "stub-bin"
    stub.mkdir()
    (stub / "gh").write_text(f"#!/bin/sh\n[ -n '{answer}' ] && echo {answer}\nexit {exit_code}\n")
    (stub / "gh").chmod(0o755)
    monkeypatch.setenv("PATH", f"{stub}:/usr/bin:/bin")
    status.update("/review-prs", ["/review-prs"], status="done", link="https://github.com/example/shop/pull/12", handled_items=["pr:example/tools#7"])
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    end.finish()
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00", NEXT))
    got = end.morning()
    assert [r["state"] for r in got["items"]] == [state, state], got


def test_a_second_day_start_does_not_fall_back_to_an_older_handoff(tmp_path, monkeypatch):
    handoff.add("waiting", "An old wait", day=__import__("datetime").date.fromisoformat("2026-10-27"))
    status.update("/review-prs", ["/review-prs"], status="done", link="https://github.com/example/shop/pull/12")
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    end.finish()
    monkeypatch.setattr(gate, "run_verify", lambda c: (False, "exit 1"))  # the PR merged overnight
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00", NEXT))
    assert end.morning()["from"] == DAY
    again = end.morning()
    assert again["from"] == DAY and again["items"] == [], "the last handoff is now empty: not the one from two days before"


# ---------- review fixes: once a day, one writer at a time ----------


def test_a_second_end_of_day_changes_nothing_and_returns_the_first(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    stops = []
    real = watch.end
    monkeypatch.setattr(watch, "end", lambda: stops.append(1) or real())
    first = end.finish()
    monkeypatch.setenv("CAREERBREW_NOW", at("17:50"))
    again = end.finish(by="clock")
    assert stops == [1], "the loops are stopped once: a second end of day claims nothing"
    assert again["already"] is True and again["at"] == first["at"] and again["by"] == "owner"
    assert _day_file()["end"]["at"] == first["at"]


def test_the_clock_check_runs_inside_the_pass_lock(monkeypatch):
    import fcntl

    held = []

    def tick(*_a, **_k):
        with watch.lock_path().open("a+") as fh:
            try:
                fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
                held.append(False)
                fcntl.flock(fh, fcntl.LOCK_UN)
            except BlockingIOError:
                held.append(True)
        return {"phase": "work"}

    monkeypatch.setattr(end, "tick", tick)
    watch.run_pass(runner=lambda *_: (True, ""))
    assert held == [True], "two passes at work_end must not both end the day"


HANDOFF_WRITER = """
import sys, time
from careerbrew.day import handoff
load = handoff.load
def slow(day=None):
    items = load(day)
    time.sleep(0.5)
    return items
handoff.load = slow
if sys.argv[1] == "add":
    handoff.add("waiting", "Waiting on the design review")
else:
    handoff.replace_owned("end-of-day", [handoff.item("prs", "PR example/shop#12 from /review-prs")])
"""


def test_two_writers_of_one_handoff_keep_both_items(monkeypatch):
    import subprocess
    import sys

    procs = [subprocess.Popen([sys.executable, "-c", HANDOFF_WRITER, w], stderr=subprocess.PIPE, text=True) for w in ("add", "owned")]
    errs = [p.communicate(timeout=60)[1] for p in procs]
    assert all(p.returncode == 0 for p in procs), errs
    assert sorted(i["section"] for i in handoff.load()) == ["prs", "waiting"]


def test_a_handoff_recheck_with_no_source_runs_as_an_agents(monkeypatch):
    from datetime import date

    handoff._save([handoff.item("prs", "PR example/shop#12", recheck="true")], date.fromisoformat(DAY))
    seen = []
    monkeypatch.setattr(questions, "check_one", lambda q: seen.append(q["source"]) or ("open", ""))
    monkeypatch.setenv("CAREERBREW_NOW", at("09:00", NEXT))
    end.morning()
    assert seen == ["agent"]


def test_day_end_json_hides_a_loops_step_name_from_an_agent(monkeypatch, capsys):
    watch.add("cron", "cron-1", step=f"/{CANARY}")
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    monkeypatch.setenv("CAREERBREW_NOW", at("17:40"))
    capsys.readouterr()
    cli.main(["day", "end", "--json"])
    out = capsys.readouterr().out
    assert CANARY not in out
    assert json.loads(out)["loops"]["running"][0]["step"] == "other step 1"


def test_day_end_asks_to_confirm_since_it_stops_loops():
    from careerbrew import actions

    assert "day.end" in actions.CONFIRM


def test_day_check_shows_held_clock_end(capsys):
    assert cli.main(["day", "wrap", "--hold"]) == 0
    capsys.readouterr()
    assert cli.main(["day", "check"]) == 0
    assert "automatic day end is paused" in capsys.readouterr().out
    assert cli.main(["day", "check", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["wrap_up"]["phase"] == "held"
