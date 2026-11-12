import json
import urllib.error
import urllib.request

import pytest

from careerbrew import machine, server
from careerbrew.work import evidence, priorities

from .conftest import FIXTURES


@pytest.fixture
def api(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    home.mkdir(parents=True, exist_ok=True)
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text(f"calendars:\n  - {{kind: ics, path: '{FIXTURES}/calendar/work.ics'}}\n")
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]

    def call(path, token="tok", host=None, body=None):
        req = urllib.request.Request(
            f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body is not None else None, headers={"X-Careerbrew-Token": token, "Host": host or f"127.0.0.1:{port}", "Content-Type": "application/json"}
        )
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"null")

    yield call
    srv.shutdown()
    srv.server_close()


def test_token_and_host_are_required(api):
    assert api("/api/overview")[0] == 200
    assert api("/api/overview", token="wrong")[0] == 401
    assert api("/api/overview", host="evil.example.com")[0] == 403


def test_every_view_endpoint_returns_data(api):
    priorities.add("write design doc", 3, 3, 1, ["design"])
    evidence.note("feat: shipped search", ["delivery"])
    for path in ("/api/overview", "/api/today", "/api/week?days=3", "/api/jobs", "/api/interviews", "/api/interviews/Stripe", "/api/work", "/api/focus", "/api/review", "/api/setup"):
        status, data = api(path)
        assert status == 200, (path, data)
    status, wk = api("/api/week?days=3")
    assert len(wk["days"]) == 3 and wk["days"][0]["meetings"], "plan ahead uses calendar facts"
    assert api("/api/interviews")[1]["patterns"]["companies"] == 5


def test_week_view_does_not_write_future_plans(api, home):
    api("/api/week?days=5")
    assert len(list((home / "day").glob("plan-*.json"))) == 0


def test_post_task_done_and_watch(api, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    status, data = api("/api/tasks", body={"title": "plan the migration", "hours": 2, "competencies": ["design"]})
    assert status == 200 and data["task"]["id"] == "t001"
    assert api("/api/tasks/t001/done", body={"hours": 3})[1]["task"]["status"] == "done"
    assert api("/api/watch/nope", body={})[0] == 404
    assert api("/api/tasks", body={"hours": 1})[0] == 400, "missing title is a clean 400"


def test_corporate_machine_blocks_job_views(api):
    machine.set_role("corporate")
    assert api("/api/jobs")[0] == 403
    assert api("/api/interviews")[0] == 403
    assert api("/api/work")[0] == 200


def test_cards_library_and_board_endpoints(api, home):
    from careerbrew import notion_import

    from .conftest import FIXTURES

    notion_import.run(FIXTURES / "notion-bundle")
    status, d = api("/api/cards")
    assert status == 200 and d["stats"]["total"] == 4 and d["queue"]
    cid = d["queue"][0]["id"]
    assert api(f"/api/cards/{cid}/review", body={"rating": "good", "seconds": 7})[0] == 200
    assert api(f"/api/cards/{cid}/review", body={"rating": "meh"})[0] == 400
    assert api("/api/cards/nope/review", body={"rating": "good"})[0] == 404
    status, lib = api("/api/library")
    assert status == 200 and len(lib["pages"]) == 11, "pages, plan, past job, skills, 1 story, 2 job notes"
    status, page = api(f"/api/library/{lib['pages'][0]['id']}")
    assert status == 200 and page["markdown"] and not page["markdown"].startswith("CBV1"), "pages are decrypted for the app"
    assert api("/api/library/missing")[0] == 404
    assert len(api("/api/jobs")[1]["board"]) == 4


def test_cards_and_library_are_personal_only(api, home):
    machine.set_role("corporate")
    assert api("/api/cards")[0] == 403
    assert api("/api/library")[0] == 403


def test_responses_are_never_cached_or_framed(api, home):
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        for path in ("/api/overview", "/"):
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}"})
            try:
                r = urllib.request.urlopen(req, timeout=10)
            except urllib.error.HTTPError as e:
                r = e
            assert r.headers["Cache-Control"] == "no-store", path
            assert "frame-ancestors 'none'" in r.headers["Content-Security-Policy"], path
    finally:
        srv.shutdown()
        srv.server_close()


def test_reporting_a_problem_with_an_explanation(api, monkeypatch, tmp_path):
    from careerbrew.tutor import public

    from .test_public_cards import card

    monkeypatch.setattr(public, "cards_dir", lambda: tmp_path / "cards")
    public.publish([card()])
    [pub] = public.load()
    status, d = api(f"/api/explanations/{pub['id']}/flag", body={"note": "unclear"})
    assert status == 200 and d["flag"]["note"] == "unclear"
    assert api("/api/explanations/kb-nope/flag", body={"note": "x"})[0] == 404


def test_teach_goals_and_choice_endpoints(api, home):
    status, d = api("/api/teach")
    assert status == 200 and d["step"]["kind"] in ("goal", "lesson", "exercise", "card", "done") and d["rules_version"]
    status, g = api("/api/goals", body={"upcoming": {"kind": "learn", "title": "Hashing", "topics": ["arrays and hashing"], "minutes_per_day": 45}})
    assert status == 200 and g["plan"]["schedule"][0]["task"].startswith("goal:"), "a goal change re-plans the day in the same response"
    assert api("/api/goals", body={"upcoming": {"kind": "nope", "title": "x"}})[0] == 400
    status, r = api("/api/teach/complete", body={"step": d["step"], "result": {"confirmed": True}} if d["step"]["kind"] == "lesson" else {"step": {"kind": "mistake", "mistake": {"at": "x", "tip": "t"}}, "result": {}})
    assert status == 200 and "next" in r
    assert api("/api/teach/complete", body={"step": "x"})[0] == 400
    assert api("/api/teach/feedback", body={"text": "too slow", "rating": 3})[0] == 200
    assert api("/api/teach/improvements")[0] == 200 and api("/api/mistakes")[0] == 200
    assert api("/api/exercises")[1]["exercises"][0]["id"] == "contains-duplicate"
    assert api("/api/jobs/choose", body={"posting_id": "gh:Nope:1", "tier": "dream"})[0] == 404


def test_serve_prints_its_url_unbuffered(home, tmp_path):
    import os
    import subprocess
    import sys

    env = {**os.environ, "CAREERBREW_SERVE_TOKEN": "synthetic", "PYTHONUNBUFFERED": ""}
    env.pop("PYTHONUNBUFFERED")
    p = subprocess.Popen([sys.executable, "-c", "from careerbrew import server; server.run(0)"], stdout=subprocess.PIPE, text=True, env=env)
    import threading

    got: list[str] = []
    reader = threading.Thread(target=lambda: got.append(p.stdout.readline()), daemon=True)
    try:
        reader.start()
        reader.join(10)  # a buffered line never arrives while the server runs
        assert got and "127.0.0.1" in got[0]
    finally:
        p.kill()


def test_change_version_moves_when_evidence_inbox_or_log_is_written(api, home):
    # Issue #55: views refetch when this version changes, so every data write must move it.
    from careerbrew.day import inbox

    v0 = api("/api/changes")[1]["version"]
    assert api("/api/changes")[1]["version"] == v0, "no write, no change"
    evidence.note("feat: shipped search", ["delivery"])
    v1 = api("/api/changes")[1]["version"]
    assert v1 != v0
    from careerbrew import store

    store.write_json(inbox.path(), [{"id": "q1", "type": "question", "kind": "q", "text": "synthetic?", "status": "open"}])
    v2 = api("/api/changes")[1]["version"]
    assert v2 != v1
    (home / "day" / "log-2026-10-19.md").write_text("# synthetic\n")
    assert api("/api/changes")[1]["version"] != v2
    assert api("/api/changes", token="wrong")[0] == 401


def test_open_launches_the_browser_without_printing_the_token(home, monkeypatch, capsys):
    from careerbrew import cli

    opened: list[str] = []
    monkeypatch.setattr(server, "start_background", lambda port: f"http://127.0.0.1:{port}/#token=synthetic-secret")
    monkeypatch.setattr("webbrowser.open", lambda url, *a, **k: opened.append(url) or True)
    assert cli.main(["open", "--view", "today"]) == 0
    out = capsys.readouterr().out
    assert opened == ["http://127.0.0.1:8765/#/today&token=synthetic-secret"]
    assert "synthetic-secret" not in out and "127.0.0.1" in out


def test_teach_draft_is_saved_through_the_api(api):
    from careerbrew import teacher

    status, _ = api("/api/teach/draft", body={"id": "contains-duplicate", "code": "x = 1\n"})
    assert status == 200 and teacher.drafts() == {"contains-duplicate": "x = 1\n"}
    assert api("/api/teach/draft", body={"id": "", "code": ""})[0] == 400


# ---------- Today's open asks (task feed-web-view, US-63) ----------

ASKS = FIXTURES / "feed" / "web-asks.jsonl"
BUDGET, DANA = "outlook:AAMk-budget", "slack:D0DANA:1792410900.000400"


def test_today_lists_open_asks_newest_first_with_link_and_wait(api):
    from careerbrew.work import feed

    status, empty = api("/api/feed/asks")
    assert status == 200 and empty == {"asks": [], "total": 0, "limit": server.ASKS_LIMIT, "events": 0, "overdue_hours": 8, "closed_today": 0}, "no feed yet: the view says how to start"
    feed.add_lines(ASKS.read_text())
    status, data = api("/api/feed/asks")
    assert status == 200
    assert [a["id"] for a in data["asks"]] == [DANA, BUDGET], "newest first (issue #275); the answered mention and the personal DM are left out"
    dana, budget = data["asks"]
    assert budget["url"] == "https://outlook.example.com/mail/AAMk-budget" and budget["source"] == "outlook"
    assert (budget["waiting_minutes"], budget["work_minutes"], budget["overdue"]) == (5310, 570, True), "Thursday 16:30 to Monday 09:00: 9h 30m of work time, over 8 work hours"
    assert (dana["waiting_minutes"], dana["work_minutes"], dana["overdue"]) == (45, 0, False), "08:15 is before the workday starts"
    assert data["total"] == 2 and data["events"] == 6
    assert "text" not in budget and "Synthetic" not in json.dumps(data), "the view reads the analysis only, never the original text"


def test_with_60_open_asks_the_10_newest_are_listed_and_the_rest_page_in(api):
    """Issue #275: the list kept the 50 oldest of 82 open asks, so the 21 newest were hidden."""
    from datetime import datetime, timedelta

    from careerbrew.work import feed

    start = datetime.fromisoformat("2026-10-15T09:00:00-04:00")
    lines = [json.dumps({"id": f"acme:{i}", "source": "acme", "kind": "dm", "at": (start + timedelta(minutes=10 * i)).isoformat(), "title": f"Ask {i}", "to_me": True}) for i in range(60)]
    feed.add_lines("\n".join(lines))
    data = api("/api/feed/asks")[1]
    listed = [a["id"] for a in data["asks"]]
    assert (data["total"], len(listed)) == (60, server.ASKS_MAX)
    assert {f"acme:{i}" for i in range(50, 60)} <= set(listed), "the 10 newest asks are listed"
    assert listed[:3] == ["acme:59", "acme:58", "acme:57"], "newest first"
    every = api("/api/feed/asks?limit=60")[1]
    assert [a["id"] for a in every["asks"]] == [f"acme:{i}" for i in reversed(range(60))] and every["total"] == 60, "a larger limit pages in the rest"
    assert len(api("/api/feed/asks?limit=0")[1]["asks"]) == 1 and len(api("/api/feed/asks?limit=nope")[1]["asks"]) == server.ASKS_MAX, "a bad limit is clamped or ignored"
    assert data["limit"] == server.ASKS_LIMIT, "the answer names the cap, so the web app never keeps its own copy"


def test_past_the_cap_the_terminal_hint_lists_the_oldest_asks_too(api, monkeypatch, capsys):
    """Issue #275 review: past ASKS_LIMIT, Today names `work feed show --show --open --top <total>`; it must print the oldest ask."""
    from datetime import datetime, timedelta

    from careerbrew import cli
    from careerbrew.work import feed

    monkeypatch.setattr(server, "ASKS_LIMIT", 5)  # a small cap stands in for 1,000
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)  # the owner's own terminal
    start = datetime.fromisoformat("2026-10-15T09:00:00-04:00")
    feed.add_lines("\n".join(json.dumps({"id": f"acme:{i}", "source": "acme", "kind": "dm", "at": (start + timedelta(minutes=10 * i)).isoformat(), "title": f"Ask {i}", "to_me": True}) for i in range(8)))
    data = api("/api/feed/asks?limit=1000")[1]
    assert (len(data["asks"]), data["total"], data["limit"]) == (5, 8, 5), "the answer stops at the cap and says so"
    assert {a["id"] for a in data["asks"]}.isdisjoint({"acme:0", "acme:1", "acme:2"}), "the 3 oldest are not on the page"
    assert cli.main(["work", "feed", "show", "--show", "--open", "--top", str(data["total"])]) == 0
    out = capsys.readouterr().out
    assert all(f"(acme:{i})" in out for i in range(3)), "the terminal command the page names lists the oldest asks"


def test_an_ask_link_that_is_not_a_web_link_is_dropped(api):
    from careerbrew.work import feed

    feed.add_lines(json.dumps({"id": "x1", "source": "acme", "kind": "dm", "at": "2026-10-19T08:00:00-04:00", "title": "Ask", "to_me": True, "url": "javascript:alert(1)"}))
    assert feed.load()[0]["triage"] == "act"
    assert api("/api/feed/asks")[1]["asks"][0]["url"] == ""


def test_mark_personal_from_today_runs_the_cli_action(api):
    from careerbrew.work import feed

    feed.add_lines(ASKS.read_text())
    status, out = api("/api/actions/work.feed.mark", body={"values": {"id": DANA, "value": "personal"}})
    assert status == 200 and out["exit"] == 0 and out["argv"] == ["careerbrew", "work", "feed", "mark", "--", DANA, "personal"]
    assert [a["id"] for a in api("/api/feed/asks")[1]["asks"]] == [BUDGET], "a personal ask leaves Today"
    assert next(r for r in feed.load() if r["id"] == DANA)["personal_by"] == "owner"
    assert api("/api/actions/work.feed.mark", body={"values": {"id": DANA, "value": "work"}})[1]["exit"] == 0
    assert len(api("/api/feed/asks")[1]["asks"]) == 2, "Undo marks it work again"
    status, out = api("/api/actions/work.feed.mark", body={"values": {"id": "slack:nope", "value": "personal"}})
    assert out["exit"] == 2 and "no feed event slack:nope" in out["output"]


def test_feed_show_counts_asks_waiting_over_the_overdue_hours(api, capsys, monkeypatch):
    from careerbrew import cli, config
    from careerbrew.work import feed

    feed.add_lines(ASKS.read_text())
    assert feed.summary()["open_asks_overdue"] == 1
    assert cli.main(["work", "feed", "show"]) == 0
    assert "open asks=2 (1 waiting over 8 work hours)" in capsys.readouterr().out
    config.set_value("feed_ask_overdue_hours", "24")
    assert feed.summary()["open_asks_overdue"] == 0
    assert api("/api/feed/asks")[1]["overdue_hours"] == 24


# The overdue mark counts work hours (work_start to work_end on work_days, local time), as the task
# pre-registered: nights and weekends do not make an ask overdue. Etc/GMT+4, Monday 2026-10-19.
@pytest.mark.parametrize(
    ("asked", "checked", "work_minutes", "overdue"),
    [
        ("2026-10-18T16:30:00-04:00", "2026-10-19T09:00", 0, False),  # a Sunday mail on Monday at the start of the workday
        ("2026-10-16T17:00:00-04:00", "2026-10-17T01:00", 30, False),  # Friday 17:00, seen Saturday 01:00: only 17:00-17:30 counts
        ("2026-10-16T17:00:00-04:00", "2026-10-19T17:15", 525, True),  # Friday 17:00 to Monday 17:15: 30m + 8h 15m
        ("2026-10-19T08:00:00-04:00", "2026-10-19T17:00", 480, False),  # exactly 8 work hours is not over 8
        ("2026-10-19T12:00:00Z", "2026-10-19T09:30", 30, False),  # a UTC time is read in the local zone (08:00 there)
        ("2026-10-30T17:00:00-04:00", "2026-11-02T10:00", 90, False),  # across a weekend and a month end
    ],
)
def test_an_ask_is_overdue_after_8_work_hours_not_8_clock_hours(api, monkeypatch, asked, checked, work_minutes, overdue):
    from careerbrew.work import feed

    feed.add_lines(json.dumps({"id": "acme:1", "source": "acme", "kind": "dm", "at": asked, "title": "Ask", "to_me": True}))
    monkeypatch.setenv("CAREERBREW_NOW", checked)
    ask = api("/api/feed/asks")[1]["asks"][0]
    assert (ask["overdue"], feed.summary()["open_asks_overdue"]) == (overdue, int(overdue))
    assert ask["work_minutes"] == work_minutes


def test_the_overdue_mark_follows_the_work_hours_settings(api, monkeypatch):
    from careerbrew import config
    from careerbrew.work import feed

    feed.add_lines(json.dumps({"id": "acme:1", "source": "acme", "kind": "dm", "at": "2026-10-18T08:00:00-04:00", "title": "Ask", "to_me": True}))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    assert api("/api/feed/asks")[1]["asks"][0]["work_minutes"] == 0, "Sunday is not a workday by default"
    config.set_value("work_days", "0,1,2,3,4,6")
    config.set_value("work_end", "18:00")
    ask = api("/api/feed/asks")[1]["asks"][0]
    assert (ask["work_minutes"], ask["overdue"]) == (540, True), "with Sunday as a workday: 09:00-18:00 counts"


def test_a_company_page_lists_its_sources_in_one_order_every_start():
    """US-75 visual review: the Sources list came from a set, so each server start (a new hash seed) showed another order."""
    import os
    import subprocess
    import sys

    from careerbrew import interview

    code = "import json, sys; from careerbrew import server; print(json.dumps(list(server.interview_detail(sys.argv[1])['source_list'])))"
    orders = {subprocess.run([sys.executable, "-c", code, "Datadog"], capture_output=True, text=True, check=True, env={**os.environ, "PYTHONHASHSEED": seed}).stdout for seed in ("1", "2", "3")}
    assert len(orders) == 1, "the same sources in the same order, whatever the hash seed"
    titles = [interview.sources()[k]["title"] for k in server.interview_detail("Datadog")["source_list"]]
    assert titles == sorted(titles, key=str.casefold), "sorted by title, so sources of one site sit together"
