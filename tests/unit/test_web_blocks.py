"""Issue #142: blocks that load on their own, a warm server cache, and a per-tab key for the page's cache."""

import base64
import json
import threading
import time
import urllib.error
import urllib.request

import pytest

from careerbrew import config, server
from careerbrew.day import inbox
from careerbrew.work import evidence, focus, priorities

from .conftest import FIXTURES


@pytest.fixture
def srv(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    (home / "day").mkdir(parents=True, exist_ok=True)
    (home / "day" / "calendars.yaml").write_text(f"calendars:\n  - {{kind: ics, path: '{FIXTURES}/calendar/work.ics'}}\n")
    priorities.add("write design doc", 3, 3, 1, ["design"])
    priorities.add("migrate billing", 5, 2, 16, ["impact"], due="2026-10-30")
    evidence.note("feat: shipped search", ["delivery"])
    s, _t = server.serve_in_thread(0, "tok")
    yield s
    s.shutdown()
    s.server_close()


def get(s, path):
    port = s.server_address[1]
    req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}"})
    try:
        with urllib.request.urlopen(req, timeout=60) as r:
            return r.status, json.loads(r.read()), r.headers.get("X-Careerbrew-Cache"), r.headers.get("Cache-Control")
    except urllib.error.HTTPError as e:
        return e.code, json.loads(e.read() or b"null"), e.headers.get("X-Careerbrew-Cache"), e.headers.get("Cache-Control")


def cache_of(s) -> server.ResponseCache:
    return s.RequestHandlerClass.cache


def test_the_server_and_focus_list_the_same_blocks():
    assert server.FOCUS_BLOCKS == focus.BLOCKS
    assert list(focus.BLOCKS)[:7] == list(focus.HORIZONS)


def test_each_dashboard_block_answers_on_its_own_with_what_the_whole_view_shows(srv):
    whole = get(srv, "/api/focus")[1]
    by = {h["id"]: h for h in whole["horizons"]}
    for block in focus.BLOCKS:
        status, data, cache, _ = get(srv, f"/api/focus/{block}")
        assert status == 200 and cache == "miss", block
        assert data["id"] == block and data["at"] == whole["at"]
        if block in focus.HORIZONS:
            assert data["horizon"] == by[block], f"the {block} block shows what /api/focus shows"
        else:
            assert data[block] == whole[block]
        assert get(srv, f"/api/focus/{block}")[2] == "hit", "each block is its own cached view"
    assert get(srv, "/api/focus/company")[1]["ladder"] == whole["ladder"]
    assert get(srv, "/api/focus/now")[1].get("ladder") is None, "only the ladder blocks name the ladder"


def test_an_unknown_block_answers_404(srv):
    status, data, _, _ = get(srv, "/api/focus/forever")
    assert status == 404 and "Dashboard block" in data["error"]


def test_blocks_asked_for_at_once_share_one_build(srv, monkeypatch):
    made, assessed = [], []
    real_init, real_assess = focus.Build.__init__, focus.promo.assess

    def counting_init(self, now):
        made.append(now)
        real_init(self, now)

    def counting_assess(*a, **k):
        assessed.append(1)
        time.sleep(0.2)  # a slow ladder: the other blocks arrive while it runs
        return real_assess(*a, **k)

    monkeypatch.setattr(focus.Build, "__init__", counting_init)
    monkeypatch.setattr(focus.promo, "assess", counting_assess)
    threads = [threading.Thread(target=get, args=(srv, f"/api/focus/{b}")) for b in ("quarter", "year", "company")]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert len(made) == 1, "one build per minute and data version"
    assert len([1 for _ in assessed]) <= 2, "the three ladder blocks share one assessment (next_step may assess once)"


def test_a_build_computes_a_shared_part_once_when_two_threads_ask_at_once():
    b = focus.Build(focus.calendar.now())
    calls = []

    def slow():
        calls.append(1)
        time.sleep(0.1)
        return 42

    out = []
    threads = [threading.Thread(target=lambda: out.append(b._once("part", slow))) for _ in range(4)]  # noqa: SLF001
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert calls == [1] and out == [42] * 4


def test_the_inbox_answers_on_its_own(srv):
    inbox.offer_task(priorities.add("review the rollout checklist", 3, 3, 1, [], source="agent", why="the release is Thursday"))
    status, data, cache, _ = get(srv, "/api/inbox")
    assert status == 200 and cache is None
    assert [i["kind"] for i in data["inbox"]] == ["agent-task"]
    assert data["inbox"] == get(srv, "/api/daylog")[1]["inbox"], "the same items as the day log"


def test_the_tab_key_is_per_server_and_off_with_the_setting(srv, home):
    status, data, cache, store_header = get(srv, "/api/tab-key")
    assert status == 200 and cache is None and store_header == "no-store"
    assert len(base64.b64decode(data["key"])) == 32, "an AES-256 key"
    assert get(srv, "/api/tab-key")[1]["key"] == data["key"], "one key per server: a reload can read what the tab kept"
    other, _t = server.serve_in_thread(0, "tok")
    try:
        assert get(other, "/api/tab-key")[1]["key"] != data["key"], "a new server cannot read an old tab's copy"
    finally:
        other.shutdown()
        other.server_close()
    config.set_value("web_tab_cache", "false")
    assert get(srv, "/api/tab-key")[1]["key"] is None, "off: the page keeps data in memory only"


def test_the_tab_key_needs_the_token(srv):
    port = srv.server_address[1]
    req = urllib.request.Request(f"http://127.0.0.1:{port}/api/tab-key", headers={"Host": f"127.0.0.1:{port}"})
    with pytest.raises(urllib.error.HTTPError) as e:
        urllib.request.urlopen(req, timeout=10)
    assert e.value.code == 401


def test_warm_builds_every_warm_view_so_the_first_open_is_a_hit(srv):
    cache = cache_of(srv)
    version, rebuilt = server.warm(cache)
    assert rebuilt >= len(server.WARM_VIEWS) and version == server.files_version()
    for view in server.WARM_VIEWS:
        assert get(srv, view)[2] == "hit", f"{view} was warmed"
    evidence.note("feat: shipped order export", ["delivery"])  # new data from another process
    server.warm(cache)
    for view in server.WARM_VIEWS:
        status, data, state, _ = get(srv, view)
        assert state == "hit", f"{view} was warmed again after new data"
    assert "feat: shipped order export" in json.dumps(get(srv, "/api/work")[1])


def test_the_warmer_warms_at_start_and_after_new_data(srv):
    cache = cache_of(srv)
    assert get(srv, "/api/ping")[1] == {"ok": True, "warm": "off"}, "no warmer: serve_in_thread runs none"
    warmer = server.Warmer(cache, poll=0.05).start()
    try:
        wait_warm(srv)
        assert all(get(srv, v)[2] == "hit" for v in server.WARM_VIEWS), "warmed at start"
        priorities.add("plan the migration", 3, 3, 2, [])  # a watcher cycle or the CLI writes
        wait_warm(srv)
        status, today, state, _ = get(srv, "/api/today")
        assert state == "hit", "the first open after new data answers from memory"
        assert any(s["title"] == "plan the migration" for s in today["plan"]["schedule"])
    finally:
        warmer.stop()


def test_warm_does_nothing_with_the_cache_off(srv):
    config.set_value("web_cache_seconds", "0")
    assert server.warm(cache_of(srv))[1] == 0


def test_the_warm_event_is_content_free(srv, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    warmer = server.Warmer(cache_of(srv), poll=0.05).start()
    try:
        wait_warm(srv)
    finally:
        warmer.stop()
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    warm = [r for r in rows if r["event"] == "web.warm"]
    assert warm and set(warm[0]) <= {"ts", "event", "status", "views", "rebuilt", "failed", "ms"}


def wait_warm(s, timeout=60.0):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if get(s, "/api/ping")[1].get("warm") == "done":
            return
        time.sleep(0.05)
    raise AssertionError("the warmer did not finish")


def test_a_burst_of_block_requests_is_never_reset(srv):
    # The Dashboard asks for its 9 blocks at once, beside the page's own reads. The stdlib server's listen
    # backlog of 5 reset some of those connections (ERR_CONNECTION_RESET in the browser) on a busy machine.
    errors: list[str] = []

    def one():
        try:
            get(srv, "/api/ping")
        except OSError as e:
            errors.append(type(e).__name__)

    for _ in range(5):
        threads = [threading.Thread(target=one) for _ in range(40)]
        for t in threads:
            t.start()
        for t in threads:
            t.join()
    assert errors == [], f"{len(errors)} of 200 requests failed to connect"
