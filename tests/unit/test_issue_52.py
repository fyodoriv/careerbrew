"""Issue #52: stale review row, review count by submitted_at, saved PM draft, inbox "more" count."""

from datetime import date

from careerbrew import connectors
from careerbrew.cli import _say_inbox
from careerbrew.day import inbox
from careerbrew.day import log as daylog
from careerbrew.work import evidence


def _fake_github(monkeypatch, submitted: list[str]):
    def fake(url, headers, body=None):
        if "reviewed-by" in url:
            return {"total_count": 2, "items": [{"number": 7, "repository_url": "https://api.github.com/repos/o/r"}, {"number": 8, "repository_url": "https://api.github.com/repos/o/r"}]}
        if url.endswith("/pulls/7/reviews?per_page=100"):
            return [{"user": {"login": "me"}, "submitted_at": s} for s in submitted]
        if url.endswith("/pulls/8/reviews?per_page=100"):
            return [{"user": {"login": "someone"}, "submitted_at": "2026-10-16T09:00:00Z"}, {"user": {"login": "me"}, "submitted_at": "2026-09-01T09:00:00Z"}]
        return {"items": []}

    monkeypatch.setattr(connectors, "http_json", fake)


def test_review_count_uses_reviews_submitted_today(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00+00:00")
    monkeypatch.setenv("CB_T", "x")
    _fake_github(monkeypatch, ["2026-10-16T08:00:00Z", "2026-10-16T12:00:00Z", "2026-10-10T12:00:00Z"])
    (rev,) = [i for i in connectors.github({"token": "env:CB_T", "user": "me"}, "user") if i["id"].startswith("github:reviews-")]
    assert rev["id"] == "github:reviews-2026-10-16" and rev["title"] == "Reviewed 1 pull requests", "PR 7 has two reviews today; PR 8 has none by me today"


def test_repeated_id_with_a_new_title_replaces_the_stored_row(home, monkeypatch):
    evidence.store.append_jsonl(evidence.evidence_path(), [{"id": "github:reviews-2026-10-16", "kind": "github", "title": "Reviewed 1411 pull requests", "at": "2026-10-16T08:00:00+00:00", "tags": [], "origin": "user"}])
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "github", "origin": "user"}])
    monkeypatch.setattr(connectors, "run", lambda src, origin: [{"id": "github:reviews-2026-10-16", "kind": "github", "title": "Reviewed 4 pull requests", "at": "2026-10-16T16:00:00+00:00", "tags": [], "origin": "user"}])
    evidence.collect()
    rows = [r for r in evidence.load() if r["id"] == "github:reviews-2026-10-16"]
    assert [r["title"] for r in rows] == ["Reviewed 4 pull requests"]


def test_edited_pm_draft_is_saved_per_day(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    d = daylog.build(date(2026, 10, 16))
    assert daylog.pm_report(d)["edited"] is False
    daylog.save_pm_draft("2026-10-16", "My own words")
    pm = daylog.pm_report(d)
    assert pm["body"] == "My own words" and pm["edited"] is True and pm["draft"] != pm["body"]
    daylog.save_pm_draft("2026-10-16", "")
    assert daylog.pm_report(d)["edited"] is False, "an empty save resets to the draft"


def test_pm_draft_groups_done_work_by_ticket_prefix(home, monkeypatch):
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["PROJ-1 export", "PROJ-2 import", "fix: flaky test"]}, "planned": [], "blockers": []}
    lines = daylog.pm_report(d)["body"].splitlines()
    assert lines == ["PROJ: export and import (2 changes).", "Other: flaky test (1 change)."], "one line per ticket prefix (issue #68 keeps the grouping, drops the ids)"


def test_inbox_more_count_is_items_not_shown(home, monkeypatch, capsys):
    monkeypatch.setattr(inbox, "open_items", lambda: [{"type": "question", "id": f"q{i}", "text": "t", "kind": "impact", "status": "open"} for i in range(60)])
    _say_inbox(limit=5)
    out = capsys.readouterr().out
    assert "... 55 more of 60 open" in out, "the total is named, so the count cannot be misread"
