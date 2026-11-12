"""`work project check <name> --since`: which sources changed since their own last check, stamping nothing (issue #109)."""

import os
import re
import urllib.parse
from datetime import date, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

import pytest

from careerbrew import store
from careerbrew.cli import main
from careerbrew.work import projects, research

FIX = Path(__file__).resolve().parents[1] / "fixtures" / "http"


@pytest.fixture
def conn(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIX))
    monkeypatch.setenv("CB_T", "synthetic")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-05T09:00+00:00")
    (home / "work").mkdir(parents=True, exist_ok=True)
    store.write_yaml(
        home / "work" / "connectors.yaml",
        {
            "connectors": [
                {"kind": "jira", "token": "env:CB_T", "base_url": "https://jira.example.com", "email": "me@example.com"},
                {"kind": "github", "token": "env:CB_T", "user": "me"},
                {"kind": "gdocs", "token": "env:CB_T"},
                {"kind": "slack", "token": "env:CB_T"},
            ]
        },
    )
    return home


def _file(tmp_path: Path, name: str, when: str) -> str:
    p = tmp_path / name
    p.write_text("# notes\n", encoding="utf-8")
    t = datetime.fromisoformat(when).timestamp()
    os.utime(p, (t, t))
    return str(p)


def _by_ref(rows):
    return {r["ref"]: r for r in rows}


def test_since_names_changed_unchanged_never_read_and_references_and_stamps_nothing(conn, tmp_path):
    old = _file(tmp_path, "kickoff.md", "2026-10-01T09:00+00:00")
    new = _file(tmp_path, "design.md", "2026-10-06T09:00+00:00")
    projects.add("billing", memory="work/billing-notes.md")
    for kind, ref in (("repo", "acme/billing"), ("doc", "doc-prd-1"), ("doc", old), ("doc", new), ("url", "https://outlook.office.com/mail/x")):
        projects.add_source("billing", kind, ref)
    for ref in ("acme/billing", "doc-prd-1", old, new):
        projects.check("billing", ref)  # stamped 2026-10-05 09:00 (pinned clock)
    before = {s["ref"]: s["checked_at"] for s in projects.show("billing")["sources"]}

    rows = _by_ref(research.changes("billing"))

    assert rows["acme/billing"]["state"] == "changed" and rows["acme/billing"]["count"] == 1  # PR #42 updated 2026-10-06
    assert rows["doc-prd-1"]["state"] == "changed" and rows["doc-prd-1"]["count"] == 2  # edited and one new comment on 2026-10-06
    assert rows[old]["state"] == "unchanged" and rows[old]["count"] == 0
    assert rows[new]["state"] == "changed"
    assert rows["work/billing-notes.md"]["state"] == "never read"
    assert rows["https://outlook.office.com/mail/x"]["state"] == "reference"
    assert {s["ref"]: s["checked_at"] for s in projects.show("billing")["sources"]} == before


def test_a_source_read_and_stamped_after_its_last_change_is_unchanged(conn, monkeypatch):
    projects.add("billing")
    projects.add_source("billing", "repo", "acme/billing")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-07T09:00+00:00")
    projects.check("billing", "acme/billing")
    (row,) = research.changes("billing", "acme/billing")
    assert row["state"] == "unchanged" and row["count"] == 0


def test_with_reading_off_only_local_files_are_compared(conn, tmp_path):
    old = _file(tmp_path, "kickoff.md", "2026-10-01T09:00+00:00")
    projects.add("billing")
    projects.add_source("billing", "repo", "acme/billing")
    projects.add_source("billing", "doc", old)
    projects.check("billing")
    rows = _by_ref(research.changes("billing", read_allowed=False))
    assert rows[old]["state"] == "unchanged"
    assert rows["acme/billing"]["state"] == "unknown" and "reading business data is off" in rows["acme/billing"]["reason"]


def test_a_url_is_unknown_without_a_fetch_and_a_read_error_names_its_reason(conn):
    projects.add("billing")
    projects.add_source("billing", "url", "https://no-fixture.example.com/page")
    projects.add_source("billing", "tracker", "PROJ-404")
    projects.check("billing")
    rows = _by_ref(research.changes("billing"))
    assert rows["https://no-fixture.example.com/page"]["state"] == "unknown"
    assert "no change date" in rows["https://no-fixture.example.com/page"]["reason"]
    assert rows["PROJ-404"]["state"] == "unknown" and "tracker could not be read" in rows["PROJ-404"]["reason"]


def _day_searches(monkeypatch, tz: str, messages=(), parent_updated: str = "", children=()):
    """Slack search and Jira JQL as the services treat a bare date: as a whole day in the searcher's own time
    zone, with Slack's `after:` leaving out the named day. The HTTP fixtures ignore the query, so they cannot."""
    zone = ZoneInfo(tz)

    def local_day(iso: str) -> date:
        return datetime.fromisoformat(iso).astimezone(zone).date()

    def http_json(url, headers, body=None):
        u = urllib.parse.urlsplit(url)
        q = {k: v[0] for k, v in urllib.parse.parse_qs(u.query).items()}
        if u.path.endswith("search.messages"):
            m = re.search(r"after:(\S+)", q["query"])
            hits = [t for t in messages if not m or local_day(t) > date.fromisoformat(m.group(1))]
            return {"ok": True, "messages": {"matches": [{"ts": f"{datetime.fromisoformat(t).timestamp():.6f}", "permalink": f"https://chat.example.com/p{i}"} for i, t in enumerate(hits)]}}
        if u.path.endswith("search/jql"):
            m = re.search(r"updated >= '([\d-]+)'", q["jql"])
            hits = [t for t in children if not m or local_day(t) >= date.fromisoformat(m.group(1))]
            return {"issues": [{"key": f"PROJ-{70 + i}", "fields": {"status": {"name": "In Progress"}, "updated": t}} for i, t in enumerate(hits)]}
        return {"key": "PROJ-7", "fields": {"summary": "Billing migration epic", "status": {"name": "In Progress"}, "updated": parent_updated}}

    monkeypatch.setattr("careerbrew.connectors.http_json", http_json)


@pytest.mark.parametrize(
    ("tz", "stamp", "reply"),
    [
        ("UTC", "2026-10-05T22:30:00+00:00", "2026-10-05T23:10:00+00:00"),  # later on the same day as the check
        ("America/Los_Angeles", "2026-10-06T02:00:00+00:00", "2026-10-06T03:00:00+00:00"),  # 10-05 evening there
        ("Etc/GMT+12", "2026-10-05T01:00:00+00:00", "2026-10-05T02:00:00+00:00"),  # UTC-12: still 10-04 there
    ],
)
def test_a_chat_reply_after_the_check_is_a_change_whatever_the_day_or_time_zone(conn, monkeypatch, tz, stamp, reply):
    earlier = (datetime.fromisoformat(stamp) - timedelta(minutes=30)).isoformat()
    _day_searches(monkeypatch, tz, messages=[earlier, reply])
    monkeypatch.setenv("CAREERBREW_NOW", stamp)
    projects.add("billing")
    projects.add_source("billing", "chat", "#billing-eng")
    projects.check("billing")
    (row,) = research.changes("billing")
    assert row["state"] == "changed" and row["count"] == 1  # the reply; the message before the check is not a change


@pytest.mark.parametrize(
    ("tz", "stamp", "parent", "kids", "count"),
    [
        ("UTC", "2026-10-05T22:30:00+00:00", "2026-10-05T08:00:00.000+0000", ["2026-10-05T10:00:00.000+0000"], 0),  # read and stamped after both: quiet
        ("America/Los_Angeles", "2026-10-06T02:00:00+00:00", "2026-10-05T20:00:00.000+0000", ["2026-10-06T03:00:00.000+0000"], 1),  # the child's update is 10-05 there
        ("Etc/GMT+12", "2026-10-05T01:00:00+00:00", "2026-10-05T03:00:00.000+0000", ["2026-10-05T02:00:00.000+0000", "2026-10-04T23:00:00.000+0000"], 2),
    ],
)
def test_a_tracker_counts_its_own_update_and_each_child_updated_after_the_check(conn, monkeypatch, tz, stamp, parent, kids, count):
    _day_searches(monkeypatch, tz, parent_updated=parent, children=kids)
    monkeypatch.setenv("CAREERBREW_NOW", stamp)
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-7")
    projects.check("billing")
    (row,) = research.changes("billing")
    assert row["count"] == count and row["state"] == ("changed" if count else "unchanged")


@pytest.mark.parametrize(
    ("stamp", "since", "after"),
    [
        ("2026-10-05T06:00:00.000-0400", "2026-10-05T09:00:00+00:00", True),  # 10:00 UTC: a string compare says "before"
        ("2026-10-05T04:00:00.000-0400", "2026-10-05T09:00:00+00:00", False),
        ("2026-10-06T09:00:00Z", "2026-10-05T09:00:00+00:00", True),
        ("2026-10-05T09:00:00", "2026-10-05T09:00:00+00:00", False),  # no offset: UTC
        ("", "2026-10-05T09:00:00+00:00", True),  # no date: counted as new, so a change is never missed
        ("2026-10-01T00:00:00Z", None, True),
    ],
)
def test_after_compares_times_not_strings(stamp, since, after):
    assert research._after(stamp, since) is after


def test_cli_prints_each_state_and_what_to_read(conn, tmp_path, capsys, monkeypatch):
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)
    old = _file(tmp_path, "kickoff.md", "2026-10-01T09:00+00:00")
    projects.add("billing", memory="work/billing-notes.md")
    projects.add_source("billing", "repo", "acme/billing")
    projects.add_source("billing", "doc", old)
    projects.check("billing", "acme/billing")
    projects.check("billing", old)
    assert main(["work", "project", "check", "billing", "--since"]) == 0
    out = capsys.readouterr().out
    assert "nothing stamped" in out
    assert "changed    repo     acme/billing  1 change(s) since 2026-10-05 09:00 UTC" in out
    assert f"unchanged  doc      {old}" in out
    assert "never read memory   work/billing-notes.md" in out
    assert "read these 2 of 3" in out and "careerbrew work project check billing <ref>" in out


def test_cli_refuses_since_with_status_and_an_unknown_ref(conn, capsys):
    projects.add("billing")
    projects.add_source("billing", "repo", "acme/billing")
    assert main(["work", "project", "check", "billing", "--since", "--status", "x"]) == 2
    assert "stamps nothing" in capsys.readouterr().out
    assert main(["work", "project", "check", "billing", "nope", "--since"]) == 2
    assert "has no source nope" in capsys.readouterr().out
    assert main(["work", "project", "check", "nobody", "--since"]) == 2
    assert "careerbrew work project add nobody" in capsys.readouterr().out
    assert projects.show("billing")["sources"][0]["checked_at"] is None


def test_agents_see_the_since_flag_and_the_stamp_rule_in_the_actions_list():
    from careerbrew import actions

    (check,) = [x for x in actions.catalog() if x["name"] == "work.project.check"]
    assert "--since" in {a["flag"] for a in check["args"]}
    assert "stamp only the sources you read" in check["agent_note"]


def test_the_since_report_logs_a_content_free_event(conn, monkeypatch):
    seen = []
    monkeypatch.setattr("careerbrew.work.research.events.emit", lambda name, **f: seen.append((name, f)))
    projects.add("billing")
    projects.add_source("billing", "repo", "acme/billing")
    projects.check("billing")
    research.changes("billing")
    assert ("work.project", {"status": "ok", "action": "since", "count": 1}) in seen  # counts only: no ref, title, or text
