"""Issue #73: a re-collected item with the same id replaces its stored copy (newer kind or corrected title wins)."""

import subprocess
import sys
from datetime import date

from careerbrew import connectors
from careerbrew.day import log as daylog
from careerbrew.day import results, watch
from careerbrew.work import evidence, feed

DAY = date(2026, 10, 16)
REVIEWS = {"id": "github:reviews-2026-10-16", "at": "2026-10-16T12:00:00+00:00", "kind": "github", "title": "Reviewed 1411 pull requests", "source": "evidence", "origin": "user"}
PLUGIN_ROW = {"id": "acme:42", "at": "2026-10-16T11:00:00+00:00", "kind": "external", "title": "feat: invoice export", "source": "evidence", "origin": "plugin:acme"}


def _evidence(row: dict) -> dict:
    return {k: v for k, v in row.items() if k != "source"} | {"tags": []}


def test_day_result_with_a_corrected_title_replaces_the_stored_row(home):
    results.append([REVIEWS], DAY)
    fixed = {**REVIEWS, "title": "Reviewed 4 pull requests"}
    assert results.append([fixed], DAY) == 1, "a replaced row counts as a change, so the watcher rebuilds the log"
    assert [r["title"] for r in results.load(DAY)] == ["Reviewed 4 pull requests"]
    assert results.append([fixed], DAY) == 0, "an unchanged row is not written again"


def test_day_result_with_a_newer_kind_replaces_the_stored_row(home):
    results.append([PLUGIN_ROW], DAY)
    assert results.append([{**PLUGIN_ROW, "kind": "pr"}], DAY) == 1
    assert results.summary(DAY) == {"pr": 1}


def test_day_result_with_only_a_new_time_keeps_the_stored_row(home):
    results.append([REVIEWS], DAY)
    assert results.append([{**REVIEWS, "at": "2026-10-16T15:00:00+00:00"}], DAY) == 0, "the review count row gets a new time every cycle; that alone is no new version"
    assert [r["at"] for r in results.load(DAY)] == [REVIEWS["at"]]


def test_upsert_on_a_plaintext_file_appends_and_replaces(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "off")
    results.append([REVIEWS, PLUGIN_ROW], DAY)
    assert results.append([{**PLUGIN_ROW, "kind": "pr"}, {**REVIEWS, "id": "github:other"}], DAY) == 2
    assert [(r["id"], r["kind"]) for r in results.load(DAY)] == [(REVIEWS["id"], "github"), ("acme:42", "pr"), ("github:other", "github")]


WRITER = """
import sys
from datetime import date
from careerbrew.day import results
who = sys.argv[1]
for i in range(60):
    results.append([{"id": f"{who}:{i}", "kind": "commit", "title": "t"}, {"id": "shared", "kind": "commit", "title": f"{who} {i}"}], date(2026, 10, 16))
"""


def test_two_writers_lose_no_row(home):
    procs = [subprocess.Popen([sys.executable, "-c", WRITER, who], stderr=subprocess.PIPE, text=True) for who in ("a", "b")]
    errs = [p.communicate(timeout=120)[1] for p in procs]
    assert all(p.returncode == 0 for p in procs), errs
    ids = [r["id"] for r in results.load(DAY)]
    assert len(ids) == len(set(ids)) == 121, "60 rows from each writer plus one shared row, never twice"


def test_evidence_with_a_newer_kind_replaces_the_stored_row(home, monkeypatch):
    evidence.store.append_jsonl(evidence.evidence_path(), [_evidence(PLUGIN_ROW)])
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "command", "origin": "plugin:acme"}])
    monkeypatch.setattr(evidence, "collect_command", lambda src, origin: [{**_evidence(PLUGIN_ROW), "kind": "pr"}])
    out = evidence.collect()
    assert [r["kind"] for r in evidence.load() if r["id"] == "acme:42"] == ["pr"]
    assert out["new"] == 0 and out["total"] == 1


def test_feed_evidence_with_a_corrected_title_replaces_the_stored_row(home, monkeypatch):
    page = {"id": "pagerduty:P1", "kind": "pagerduty", "title": "Resolved page", "at": "2026-10-16T10:00:00+00:00", "url": "", "tags": [], "origin": "user"}
    evidence.store.append_jsonl(evidence.evidence_path(), [page])
    monkeypatch.setattr(evidence, "sources_list", lambda: [])
    monkeypatch.setattr(feed, "evidence_items", lambda: [{**page, "title": "Resolved page: checkout latency"}])
    evidence.collect()
    assert [r["title"] for r in evidence.load() if r["id"] == "pagerduty:P1"] == ["Resolved page: checkout latency"]


def test_watcher_cycle_fixes_stale_rows_in_the_day_log(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00+00:00")
    # The state the issue describes: both stores hold the first copy of each id.
    evidence.store.append_jsonl(evidence.evidence_path(), [_evidence(REVIEWS), _evidence(PLUGIN_ROW)])
    results.append([REVIEWS, PLUGIN_ROW], DAY)
    before = daylog.build(DAY)["done"]
    assert before["Other work"] == ["feat: invoice export"] and "Reviewed 1411 pull requests" in before["Shipped"]
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "github", "origin": "user"}, {"kind": "command", "origin": "plugin:acme"}])
    monkeypatch.setattr(connectors, "run", lambda src, origin: [{**_evidence(REVIEWS), "title": "Reviewed 4 pull requests", "at": "2026-10-16T15:59:00+00:00"}])
    monkeypatch.setattr(evidence, "collect_command", lambda src, origin: [{**_evidence(PLUGIN_ROW), "kind": "pr"}])
    assert watch.run_cycle()["added"] == 2, "two replaced rows rebuild the log"
    done = daylog.build(DAY)["done"]
    assert done["Other work"] == [], "the plugin row moved with its new kind"
    assert sorted(done["Shipped"]) == ["Reviewed 4 pull requests", "feat: invoice export"]
    assert watch.run_cycle()["added"] == 0, "a second cycle with the same items changes nothing"


# Review gap: an id already stored twice. On main, two collects at once could store one id twice (the stored ids were
# read outside the lock). The upsert must collapse such a file to one row per id, or the stale copy stays forever.
def test_upsert_collapses_an_id_stored_twice_and_replaces_it(home):
    path = evidence.evidence_path()
    evidence.store.append_jsonl(path, [_evidence(REVIEWS), _evidence(REVIEWS)])
    fixed = {**_evidence(REVIEWS), "title": "Reviewed 4 pull requests"}
    assert evidence.store.upsert_jsonl(path, [fixed], results.VERSION_FIELDS) == (0, 1)
    assert [r["title"] for r in evidence.load()] == ["Reviewed 4 pull requests"]


def test_upsert_keeps_the_last_stored_copy_of_an_id_stored_twice(home):
    path = results.path_for(DAY)
    evidence.store.append_jsonl(path, [REVIEWS, PLUGIN_ROW, {**REVIEWS, "title": "Reviewed 4 pull requests"}])
    assert evidence.store.upsert_jsonl(path, [], results.VERSION_FIELDS) == (0, 1), "collapsing a duplicate changes what readers see"
    assert [(r["id"], r["title"]) for r in results.load(DAY)] == [(REVIEWS["id"], "Reviewed 4 pull requests"), (PLUGIN_ROW["id"], PLUGIN_ROW["title"])]
    assert evidence.store.upsert_jsonl(path, [], results.VERSION_FIELDS) == (0, 0), "a clean file is not written again"


def test_a_batch_with_a_stale_and_a_fresh_copy_changes_nothing_when_the_fresh_copy_is_stored(home):
    fixed = {**REVIEWS, "title": "Reviewed 4 pull requests"}
    results.append([fixed], DAY)
    assert [results.append([REVIEWS, fixed], DAY) for _ in range(3)] == [0, 0, 0], "the last copy in a batch wins; it equals the stored row"
    assert [r["title"] for r in results.load(DAY)] == ["Reviewed 4 pull requests"]


def test_watcher_settles_when_evidence_stored_an_id_twice(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00+00:00")
    evidence.store.append_jsonl(evidence.evidence_path(), [_evidence(REVIEWS), _evidence(REVIEWS)])
    results.append([REVIEWS], DAY)
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "github", "origin": "user"}])
    monkeypatch.setattr(connectors, "run", lambda src, origin: [{**_evidence(REVIEWS), "title": "Reviewed 4 pull requests"}])
    assert [watch.run_cycle()["added"] for _ in range(3)] == [1, 0, 0], "one rebuild for the fix, then the watcher settles"
    assert [r["title"] for r in evidence.load()] == ["Reviewed 4 pull requests"]
    assert [r["title"] for r in results.load(DAY)] == ["Reviewed 4 pull requests"]
