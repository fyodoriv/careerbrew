"""Task watch-idle-no-change (US-64): an idle watcher cycle does not move the web app's change version.

The old code rewrote `day/cursors.json`, `day/watch.json`, and `day/notifications.json` on every cycle
(and `changes/snapshot.json` every hour), so `/api/changes` moved every 300 s with no new data: every
open live view refetched and every cached view rebuilt.
"""

import json
import re
import sys
import urllib.request
from pathlib import Path

import pytest

from careerbrew import config, notify, server, store
from careerbrew.day import watch
from careerbrew.work import evidence, feed

WEB = Path(__file__).resolve().parents[2] / "web" / "src"


@pytest.fixture
def seeded(home, monkeypatch):
    """A data root after one cycle that saw something new: a note for today and an ask that notifies."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setattr(watch, "notify", lambda text: None)
    config.set_value("notify_events", "true")
    feed.add_lines(json.dumps({"id": "acme:1", "source": "acme", "kind": "dm", "at": "2026-10-19T08:30:00", "title": "Ask", "to_me": True}))
    evidence.note("feat: shipped search", ["delivery"])
    assert watch.run_cycle()["added"] == 1
    return home


def versions() -> tuple[str, str]:
    """The live views' signal (`/api/changes`) and the response cache's key version (the whole data root)."""
    return server.changes_view()["version"], server.files_version()


def test_an_idle_cycle_does_not_move_the_change_version(seeded):
    before = versions()
    assert watch.run_cycle()["added"] == 0
    after = versions()
    assert after[0] == before[0], "an idle cycle moved /api/changes: every live view refetches for nothing"
    assert after[1] == before[1], "an idle cycle moved the cache version: every cached view rebuilds for nothing"


def test_an_idle_cycle_with_every_source_due_does_not_move_it(seeded):
    store.write_json(watch.cursors_path(), {})  # as after 15 or 60 minutes: the throttled sources (feed, mail, changes) run too
    before = versions()
    assert watch.run_cycle()["added"] == 0
    assert versions() == before


def test_a_cycle_that_adds_a_result_still_moves_it(seeded):
    evidence.note("fix: flaky checkout test", ["quality"])  # new evidence for today: the next cycle adds a result row
    before = versions()
    assert watch.run_cycle()["added"] == 1
    after = versions()
    assert after[0] != before[0] and after[1] != before[1], "the live views must see the new result on their next poll"


def test_a_cycle_that_only_adds_a_notification_still_moves_it(seeded):
    feed.add_lines(json.dumps({"id": "acme:2", "source": "acme", "kind": "dm", "at": "2026-10-19T08:45:00", "title": "Another ask", "to_me": True}))
    before = versions()
    assert watch.run_cycle()["added"] == 0  # a feed ask is no result row, but the Notifications view (live) shows it
    after = versions()
    assert after[0] != before[0] and after[1] != before[1]


def live_paths() -> set[str]:
    """API paths of the web app's live views: `useData(api.<name>, ..., { live: true })` in web/src.

    A path with an argument in it reads as its prefix: `/api/focus/` for `/api/focus/<block>` (issue #142).
    Hooks count too: Today's day-log reads live in `useDayLog.ts` (issue #146).
    """
    names: set[str] = set()
    for f in [*WEB.glob("*.tsx"), *WEB.glob("*.ts")]:
        names |= set(re.findall(r"useData(?:<\w+>)?\((?:\(\) => )?api\.(\w+)\b[^;\n]*\{ live: true[ ,}]", f.read_text(encoding="utf-8")))
    api = (WEB / "api.ts").read_text(encoding="utf-8")
    out = set()
    for name in names:
        m = re.search(rf"^\s+{name}: (?:\([^)]*\) => call|view)<.*?>\([\"`](/api/[\w/-]+)", api, re.M)
        assert m, f"api.{name} in web/src/api.ts has no /api/ path this test can read"
        out.add(m.group(1))
    return out


def test_the_live_view_list_is_read_from_the_web_app():
    assert {"/api/today", "/api/week", "/api/focus/", "/api/feed/asks", "/api/notifications", "/api/daylog", "/api/inbox", "/api/standup"} <= live_paths()


def test_no_cached_or_live_view_reads_a_file_the_versions_skip(seeded, monkeypatch):
    """A file the versions skip must be no input of a view that relies on them (cached or live)."""
    read: list[str] = []
    real = store.read_text

    def tracing(path, default=None):
        if sys._getframe(1).f_code.co_name != "write_text_if_changed":  # a writer comparing its own file is no view input
            read.append(Path(path).name)
        return real(path, default)

    monkeypatch.setattr(store, "read_text", tracing)
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        live = live_paths()
        for prefix in (p for p in live if p.endswith("/")):  # a view per argument: each one is a cached view
            assert any(c.startswith(prefix) for c in server.CACHED_VIEWS), prefix
        for path in sorted({p for p in set(server.CACHED_VIEWS) | live if not p.endswith("/")}):
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}"})
            with urllib.request.urlopen(req, timeout=60) as r:  # noqa: S310 (local test server)
                assert r.status == 200, path
    finally:
        srv.shutdown()
        srv.server_close()
    assert read, "the views read their data through store.read_text"
    assert not [n for n in read if n.startswith(server.CHANGE_SKIP_PREFIXES)], "a cached or live view reads a file the change version skips: it would show stale data"
    assert not set(read) & server.CHANGE_SKIP, "a cached or live view reads a file the change version skips: it would show stale data"


def test_idle_cycle_does_not_rewrite_a_legacy_notification(seeded):
    state = notify.load()
    for row in state["items"]:
        for field in ("held_refs", "pending_refs", "snoozed_until_by_ref"):
            row.pop(field, None)
    store.write_json(notify.path(), state)
    before = versions()
    assert watch.run_cycle()["added"] == 0
    assert versions() == before
