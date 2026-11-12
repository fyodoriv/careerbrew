"""Issue #117: views answer a repeat request from memory, and fresh data still shows within one poll."""

import json
import time
import urllib.error
import urllib.request

import pytest

from careerbrew import server
from careerbrew.day import log as daylog
from careerbrew.work import evidence, priorities

from .conftest import FIXTURES

# A cached repeat answers from memory in well under 100 ms on an idle machine. The gate also runs
# at load ~150, so the bound is generous; the cache-hit path itself is asserted by counting rebuilds.
CACHED_REPEAT_LIMIT_S = 1.0


def test_empty_feed_lock_is_not_a_view_input_but_nonempty_file_is(home):
    feed_dir = home / "work" / "feed"
    feed_dir.mkdir(parents=True, exist_ok=True)
    before = server.files_version()
    lock = feed_dir / ".lock"
    lock.touch()
    assert server.files_version() == before
    lock.write_text("synthetic content")
    assert server.files_version() != before


@pytest.fixture
def api(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text(f"calendars:\n  - {{kind: ics, path: '{FIXTURES}/calendar/work.ics'}}\n")
    priorities.add("write design doc", 3, 3, 1, ["design"])
    evidence.note("feat: shipped search", ["delivery"])
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]

    def call(path, body=None):
        req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=json.dumps(body).encode() if body is not None else None, headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}", "Content-Type": "application/json"})
        t0 = time.perf_counter()
        try:
            with urllib.request.urlopen(req, timeout=30) as r:
                return r.status, json.loads(r.read() or b"null"), r.headers.get("X-Careerbrew-Cache"), time.perf_counter() - t0
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"null"), e.headers.get("X-Careerbrew-Cache"), time.perf_counter() - t0

    yield call
    srv.shutdown()
    srv.server_close()


def version(api) -> str:
    return api("/api/changes")[1]["version"]


@pytest.fixture
def builds(monkeypatch):
    """Count day log rebuilds: a cache hit must not rebuild."""
    count = {"n": 0}
    real = daylog.build

    def counting(*a, **k):
        count["n"] += 1
        return real(*a, **k)

    monkeypatch.setattr(daylog, "build", counting)
    return count


def test_reading_a_view_does_not_move_the_change_version(api):
    # Old code wrote day/plan-<day>.json and day/log-<day>.md on every read, so the version moved after
    # each read and every live view refetched on every poll (5 s) even with no new data.
    for path in ("/api/today", "/api/daylog"):
        api(path)
    v = version(api)
    for path in ("/api/today", "/api/daylog", "/api/week?days=3", "/api/focus", "/api/work", "/api/feed/asks"):
        assert api(path)[0] == 200
        assert version(api) == v, f"reading {path} moved the change version"


def test_a_cached_repeat_of_the_day_log_answers_from_memory(api, builds):
    daylog.save()  # the watcher keeps today's log after each cycle that saw something new
    builds["n"] = 0
    status, first, cache, _ = api("/api/daylog")
    assert status == 200 and cache == "miss" and builds["n"] == 1
    status, second, cache, seconds = api("/api/daylog")
    assert status == 200 and cache == "hit"
    assert builds["n"] == 1, "a repeat with no input change must not rebuild the day log"
    assert second == first
    assert seconds < CACHED_REPEAT_LIMIT_S, f"a cached repeat of /api/daylog took {seconds:.3f} s (limit {CACHED_REPEAT_LIMIT_S} s)"


def test_new_data_shows_within_one_poll(api, builds):
    from careerbrew.day import results

    daylog.save()
    api("/api/daylog")
    assert api("/api/daylog")[2] == "hit"
    v = version(api)
    results.append([{"id": "git:abc123", "at": "2026-10-19T08:30:00-04:00", "kind": "commit", "title": "feat: shipped order export", "source": "git"}])  # the watcher saw a commit
    assert version(api) != v, "the live views see a new version on their next poll"
    status, data, cache, _ = api("/api/daylog")
    assert status == 200 and cache == "miss"
    assert "feat: shipped order export" in data["markdown"], "the refetch after that poll shows the new data"
    api("/api/daylog")  # that rebuild kept the new log, itself a write: one more rebuild, with nothing new to write
    assert api("/api/daylog")[2] == "hit", "then it is cached again"


def test_a_settings_change_is_an_input_change(api):
    from careerbrew import config

    api("/api/feed/asks")
    assert api("/api/feed/asks")[2] == "hit"
    config.set_value("feed_ask_overdue_hours", "24")  # settings.yaml is outside work/ and day/
    status, data, cache, _ = api("/api/feed/asks")
    assert cache == "miss" and data["overdue_hours"] == 24


def test_the_clock_is_an_input(api, monkeypatch):
    from careerbrew.work import feed

    feed.add_lines(json.dumps({"id": "acme:1", "source": "acme", "kind": "dm", "at": "2026-10-19T08:30:00-04:00", "title": "Ask", "to_me": True}))
    assert api("/api/feed/asks")[1]["asks"][0]["waiting_minutes"] == 30
    assert api("/api/feed/asks")[2] == "hit"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:01")  # views that count minutes key on the minute
    status, data, cache, _ = api("/api/feed/asks")
    assert cache == "miss" and data["asks"][0]["waiting_minutes"] == 31
    api("/api/work")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:02")
    assert api("/api/work")[2] == "hit", "a view that only needs the date keys on the day"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:02")
    assert api("/api/work")[2] == "miss", "a new day is a new key"


def test_plans_that_start_from_the_free_time_left_key_on_the_minute(api, monkeypatch):
    """Tomorrow, Week, and the day log plan from now: a task that no longer fits today moves on (US-75 pictures)."""
    views = ("/api/focus/tomorrow", "/api/week?days=3", "/api/daylog")  # the day log writes its file: last
    assert api(views[0])[1]["tomorrow"]["items"] == [], "at 09:00 today's plan holds the 1h task"
    for _ in range(3):  # the first day log build writes its file, a new data version: settle first
        for view in views:
            api(view)
    assert [api(view)[2] for view in views] == ["hit"] * 3
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T17:15")
    assert [api(view)[2] for view in views] == ["miss"] * 3, "a new minute is a new key"
    tomorrow = api(views[0])[1]["tomorrow"]
    assert [it["title"] for it in tomorrow["items"]] == ["write design doc"], "15 minutes left today: the task is tomorrow's"


def test_each_query_is_its_own_entry(api):
    assert len(api("/api/week?days=3")[1]["days"]) == 3
    assert len(api("/api/week?days=5")[1]["days"]) == 5
    assert api("/api/week?days=3")[2] == "hit"


def test_a_post_clears_the_cache(api):
    api("/api/today")
    status, data, _, _ = api("/api/tasks", body={"title": "plan the migration", "hours": 2})
    assert status == 200
    status, today, cache, _ = api("/api/today")
    assert cache == "miss"
    assert any(s["task"] == data["task"]["id"] for s in today["plan"]["schedule"]), "the new task is on today's plan at once"


def test_a_read_that_started_before_a_write_never_answers_a_read_after_it(api, monkeypatch):
    # Task open-asks-mark-flake: a busy machine can finish an open asks build that read the feed before
    # Mark personal only after the mark's POST cleared the cache. That stale body goes back into the cache;
    # the read after the mark must still rebuild, or Today shows the marked ask again.
    import threading

    from careerbrew.work import feed

    feed.add_lines((FIXTURES / "feed" / "web-asks.jsonl").read_text())
    dana = "slack:D0DANA:1792410900.000400"
    real, read, release, early = server.feed_asks, threading.Event(), threading.Event(), []

    def slow_build():
        body = real()  # the feed as it was before the mark
        read.set()
        release.wait(30)  # the machine is busy: this build ends after the mark
        return body

    monkeypatch.setattr(server, "feed_asks", slow_build)
    reader = threading.Thread(target=lambda: early.append(api("/api/feed/asks")))
    reader.start()
    assert read.wait(30)
    monkeypatch.setattr(server, "feed_asks", real)
    status, out, _, _ = api("/api/actions/work.feed.mark", body={"values": {"id": dana, "value": "personal"}, "confirm": False})
    assert status == 200 and out["exit"] == 0
    release.set()
    reader.join(30)
    assert dana in [a["id"] for a in early[0][1]["asks"]], "the early read answers with what it read before the mark"
    status, data, cache, _ = api("/api/feed/asks")
    assert status == 200 and cache == "miss", "a body built before a write is never served after it"
    assert dana not in [a["id"] for a in data["asks"]], "the marked ask has left Today"


def test_the_ttl_bounds_inputs_no_file_shows(api, monkeypatch):
    from careerbrew import config

    config.set_value("web_cache_seconds", "0")
    api("/api/today")
    assert api("/api/today")[2] == "miss", "web_cache_seconds 0 turns the cache off"
    config.set_value("web_cache_seconds", "300")
    api("/api/today")
    assert api("/api/today")[2] == "hit"
    real = time.monotonic
    monkeypatch.setattr(server, "monotonic", lambda: real() + 301)  # a calendar feed may have changed
    assert api("/api/today")[2] == "miss"


def test_errors_and_uncached_views_are_never_cached(api):
    assert api("/api/daylog/not-a-date")[0] == 400
    assert api("/api/daylog/not-a-date")[2] is None
    assert api("/api/overview")[2] is None, "the watcher's process state is not a file input"
    assert api("/api/changes")[2] is None, "the change version is the signal, never cached"


def test_a_cached_job_view_is_still_blocked_on_a_corporate_machine(api):
    from careerbrew import machine

    assert api("/api/jobs")[0] == 200
    machine.set_role("corporate")
    assert api("/api/jobs")[0] == 403


def test_the_api_event_names_the_route_and_the_cache_without_content(api, home, monkeypatch):
    daylog.save()  # the watcher kept today's log
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    api("/api/daylog")
    api("/api/daylog")
    api("/api/interviews/Stripe")
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    web = [r for r in rows if r["event"] == "web.api"]
    assert [(r["route"], r["cached"]) for r in web] == [("daylog", False), ("daylog", True), ("interviews.company", False)]
    assert all(isinstance(r["ms"], int) and r["ms"] >= 0 for r in web)
    assert "Stripe" not in json.dumps(web), "a path parameter (a company name) never reaches the log"


def test_the_cache_stays_bounded():
    cache = server.ResponseCache()
    for days in range(1, 100):
        cache.put(f"/api/week?days={days}", "k", 300, b"{}")
    assert cache.get("/api/week?days=1", "k") is None, "the oldest entry went first"
    assert cache.get("/api/week?days=99", "k") == b"{}"
    assert len(cache._entries) == server.ResponseCache.MAX_ENTRIES  # noqa: SLF001 (the bound itself is the point)


def test_a_hand_edited_cache_setting_falls_back_to_the_default(api, home):
    from careerbrew import store

    store.write_yaml(home / "settings.yaml", {"web_cache_seconds": "soon"})  # as `careerbrew data edit` could leave it
    assert api("/api/today")[0] == 200  # the first read of the day writes today's plan: a real change
    assert api("/api/today")[0] == 200
    assert api("/api/today")[2] == "hit", "the default (300 s) applies"
