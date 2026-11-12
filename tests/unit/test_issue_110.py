"""Issue #110: a standup report since the last standup, one section per row you report on.

Synthetic rows, evidence, and feed events only. The clock is pinned with CAREERBREW_NOW.
2026-10-19 is a Monday; the default standup is 10:00 for 15 minutes, Monday to Friday.
"""

from __future__ import annotations

import json

import pytest

from careerbrew import config, store
from careerbrew.cli import main
from careerbrew.work import evidence, feed, project_priority, standup


def _evidence(*rows: dict) -> None:
    store.write_jsonl(evidence.evidence_path(), [{"tags": [], "origin": "user", **r} for r in rows])


def _rows(data: object) -> None:
    standup.save_rows(json.dumps(data))


def _row(report: dict, title: str) -> dict:
    return next(r for r in report["rows"] if r["title"] == title)


def _group(report: dict, label: str) -> dict:
    return next(g for g in report["groups"] if g["label"] == label)


# ---------- the window ----------


def test_window_counts_from_the_last_standup_that_ended(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T09:00")
    w = standup.window()
    assert w["start"].isoformat() == "2026-10-19T10:00:00-04:00" and w["since"] == "Mon Oct 19, 10:00"


def test_monday_counts_from_the_last_standup_day_of_the_week_before(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    assert standup.window()["since"] == "Fri Oct 16, 10:00"
    config.set_value("standup_days", "0,2")  # Monday and Wednesday
    assert standup.window()["since"] == "Wed Oct 14, 10:00"


def test_a_run_during_the_standup_counts_from_the_one_before(monkeypatch):
    config.set_value("standup_window", "since-last")  # issue #200: previous-day never starts at today's standup
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:05")
    assert standup.window()["since"] == "Fri Oct 16, 10:00", "Monday's standup is still running"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:15")
    assert standup.window()["since"] == "Mon Oct 19, 10:00", "it ended at 10:15"
    config.set_value("standup_minutes", "30")
    assert standup.window()["since"] == "Fri Oct 16, 10:00", "a 30-minute standup ends at 10:30"


def test_the_window_uses_the_standup_zone(monkeypatch):
    config.set_value("standup_window", "since-last")
    config.set_value("standup_tz", "America/Los_Angeles")
    config.set_value("standup_time", "09:30")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T12:30-04:00")  # 09:30 in Los Angeles: Tuesday's standup just started
    w = standup.window()
    assert w["since"] == "Mon Oct 19, 09:30" and w["tz"] == "America/Los_Angeles"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T13:00-04:00")  # 10:00 there: it ended
    assert standup.window()["since"] == "Tue Oct 20, 09:30"


def test_standup_settings_are_in_the_config_table_and_validated():
    keys = {r["key"] for r in config.effective()}
    assert {"standup_days", "standup_time", "standup_tz", "standup_minutes", "standup_me"} <= keys
    for key, bad, why in (("standup_time", "25:00", "HH:MM"), ("standup_tz", "Mars/Base", "IANA"), ("standup_minutes", "0", "whole number"), ("standup_days", "7", "weekday")):
        with pytest.raises(ValueError, match=why):
            config.set_value(key, bad)
    assert config.set_value("standup_tz", "Europe/Berlin") == "Europe/Berlin"
    assert config.set_value("standup_me", "Jane Doe, jane@example.com") == ["Jane Doe", "jane@example.com"]


# ---------- the gotchas from the plugin that built this alone ----------


def test_evidence_times_without_an_offset_use_the_configured_zone_not_dropped(monkeypatch):
    config.set_value("standup_tz", "America/Los_Angeles")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T08:00-07:00")  # Tuesday 08:00 there: since Monday 10:00 there
    _evidence(
        {"id": "acme:1", "kind": "external", "title": "Moved the ledger job to the new queue", "at": "2026-10-19T10:30:00"},  # no offset: 10:30 in Los Angeles
        {"id": "acme:2", "kind": "external", "title": "Drafted the rollback plan", "at": "2026-10-19T09:30:00"},  # before the standup there
        {"id": "acme:3", "kind": "external", "title": "A row with a broken time", "at": "yesterday"},
    )
    titles = [a["title"] for g in standup.build()["groups"] for a in g["actions"]]
    # Read as UTC (03:30) or as the machine zone (07:30 there), the first row would fall before the window and vanish.
    assert titles == ["Moved the ledger job to the new queue"]


def test_ticket_keys_are_read_only_for_configured_projects():
    text = "fix: decode UTF-8 file names for PROJ-12 and ISO-8601 dates"
    assert [m.group(0) for m in project_priority.TICKET.finditer(text)] == ["UTF-8", "PROJ-12", "ISO-8601"], "the generic pattern is the trap"
    assert standup.ticket_keys(text, standup.key_pattern({"PROJ"})) == ["PROJ-12"]
    assert standup.ticket_keys(text, standup.key_pattern(set())) == [], "no configured project: no keys at all"


def test_a_utf_8_commit_is_other_work_with_no_ticket(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows({"rows": [{"title": "Order export", "owner": "me", "keys": ["PROJ-12"]}]})
    _evidence({"id": "git:shop:aaa111", "kind": "commit", "repo": "shop", "title": "fix: decode UTF-8 file names", "at": "2026-10-16T15:00:00-04:00"})
    r = standup.build()
    other = _group(r, "Other work")
    assert [a["keys"] for a in other["actions"]] == [[]]
    assert other["talking_point"] == "Decode UTF-8 file names (1 commit).", "the talking point keeps UTF-8 too"
    assert [x["title"] for x in r["rows"]] == [], "no work matched the row: it has no update (issue #271)"


def test_feed_project_titles_can_be_null(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    lines = [
        {"id": "jira:1", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T15:00:00Z", "title": "Moved a ticket to review", "project": None},
        {"id": "jira:2", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T16:00:00Z", "title": "Closed a ticket", "project": {"title": None}},
        {"id": "jira:3", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T17:00:00Z", "title": "Started a ticket", "project": {"title": "Billing"}},
    ]
    feed.add_lines("\n".join(json.dumps(x) for x in lines))
    assert {r["id"]: r["project"] for r in feed.load()} == {"jira:1": "", "jira:2": "", "jira:3": "Billing"}
    _rows([{"title": "No project", "owner": "Ana", "project": None}, {"title": "Billing", "owner": "Sam", "project": "billing"}])
    r = standup.build()
    assert [a["title"] for a in _row(r, "Billing")["actions"]] == ["Started a ticket"]
    assert [a["via"] for a in _row(r, "Billing")["actions"]] == ["project Billing"]
    assert "No project" not in [x["title"] for x in r["rows"]], "a null project matches nothing"
    assert sorted(a["title"] for a in _group(r, "Other work")["actions"]) == ["Closed a ticket", "Moved a ticket to review"]


# ---------- rows, matching, and groups ----------


def test_rows_show_only_when_your_work_matched_yours_too(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    config.set_value("standup_me", "Jane Doe")
    _rows(
        [
            {"title": "Billing ledger", "status": "To do", "owner": "me", "keys": ["PROJ-40"]},
            {"title": "Order export", "status": "In review", "owner": "jane doe", "keys": ["PROJ-21"]},
            {"title": "Checkout retries", "status": "In progress", "owner": "Sam Lee", "keys": ["PROJ-31"]},
            {"title": "Search ranking", "status": "In progress", "owner": "Ana", "keys": ["PROJ-50"]},
        ]
    )
    _evidence(
        {"id": "note:1", "kind": "note", "title": "Paired with Sam on PROJ-31 retries", "at": "2026-10-16T13:00:00-04:00"},
        {"id": "note:2", "kind": "note", "title": "Wrote the PROJ-21 export spec", "at": "2026-10-16T14:00:00-04:00"},
    )
    r = standup.build()
    assert [x["title"] for x in r["rows"]] == ["Order export", "Checkout retries"]
    assert r["hidden"] == 2, "issue #271: your Billing ledger had no update, so it is hidden too"
    assert _row(r, "Order export")["mine"], "an owner in standup_me is you"
    sam = _row(r, "Checkout retries")
    assert not sam["mine"] and [a["via"] for a in sam["actions"]] == ["ticket PROJ-31"]


def test_matching_order_is_project_then_ticket_then_repo_then_whole_word(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows(
        [
            {"title": "By word", "owner": "me", "aliases": ["export", "log"]},
            {"title": "By repo", "owner": "me", "repos": ["acme/shop"]},
            {"title": "By key", "owner": "me", "keys": ["PROJ-7"]},
            {"title": "By project", "owner": "me", "project": "payments"},
        ]
    )
    _evidence(
        {"id": "a", "kind": "external", "title": "Export totals for PROJ-7 in shop", "repo": "acme/shop", "project": "Payments", "at": "2026-10-16T12:00:00-04:00"},
        {"id": "b", "kind": "external", "title": "Export rows for PROJ-7 in shop", "repo": "acme/shop", "at": "2026-10-16T12:01:00-04:00"},
        {"id": "c", "kind": "external", "title": "Export fixes", "repo": "acme/shop", "at": "2026-10-16T12:02:00-04:00"},
        {"id": "d", "kind": "external", "title": "Export fixes, round two", "at": "2026-10-16T12:03:00-04:00"},
        {"id": "e", "kind": "external", "title": "Cleaned the catalog page", "at": "2026-10-16T12:04:00-04:00"},
    )
    r = standup.build()
    assert [a["via"] for a in _row(r, "By project")["actions"]] == ["project Payments"]
    assert [a["via"] for a in _row(r, "By key")["actions"]] == ["ticket PROJ-7"]
    assert [a["via"] for a in _row(r, "By repo")["actions"]] == ["repo acme/shop"]
    assert [a["via"] for a in _row(r, "By word")["actions"]] == ['word "export"']
    assert [a["title"] for a in _group(r, "Other work")["actions"]] == ["Cleaned the catalog page"], '"log" is not a whole word in "catalog"'


def test_a_group_that_never_matches_rows_takes_tooling_work_first(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows(
        {
            "rows": [{"title": "Order export", "owner": "me", "aliases": ["export"]}],
            "groups": [{"label": "Tooling", "repos": ["agentbrew", "dotfiles"], "never_rows": True}, {"label": "Reviews", "aliases": ["reviewed"]}],
        }
    )
    _evidence(
        {"id": "git:agentbrew:bbb222", "kind": "commit", "repo": "agentbrew", "title": "feat: export skill for agents", "at": "2026-10-17T11:00:00-04:00"},
        {"id": "git:shop:ccc333", "kind": "commit", "repo": "shop", "title": "feat: export as CSV", "at": "2026-10-17T12:00:00-04:00"},
        {"id": "note:2", "kind": "note", "title": "Reviewed the queue design", "at": "2026-10-17T13:00:00-04:00"},
        {"id": "note:3", "kind": "note", "title": "Interviewed a candidate", "at": "2026-10-17T14:00:00-04:00"},
    )
    r = standup.build()
    assert [a["title"] for a in _row(r, "Order export")["actions"]] == ["feat: export as CSV"]
    assert [g["label"] for g in r["groups"]] == ["Tooling", "Reviews", "Other work"], "groups in order, Other work last"
    assert [a["title"] for a in _group(r, "Tooling")["actions"]] == ["feat: export skill for agents"], "tooling never matches a row, even by word"
    assert [a["title"] for a in _group(r, "Reviews")["actions"]] == ["Reviewed the queue design"]
    assert [a["title"] for a in _group(r, "Other work")["actions"]] == ["Interviewed a candidate"]


def test_one_line_per_action_when_a_pr_shows_up_as_a_pr_a_commit_and_a_feed_event(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows([{"title": "Order export", "owner": "me", "keys": ["PROJ-21"]}])
    url = "https://github.example.com/acme/shop/pull/12"
    _evidence(
        {"id": "pr:acme/shop#12", "kind": "pr", "repo": "acme/shop", "title": "PROJ-21 add the export endpoint", "url": url, "at": "2026-10-16T13:00:00-04:00"},
        {"id": "git:shop:ddd444", "kind": "commit", "repo": "shop", "title": "PROJ-21 add the export endpoint (#12)", "at": "2026-10-16T14:00:00-04:00"},
    )
    feed.add_lines(json.dumps({"id": "gh:merge:12", "source": "github", "kind": "merge", "actor": "me", "at": "2026-10-16T18:30:00Z", "title": "Merged pull request #12", "url": url}))
    acts = _row(standup.build(), "Order export")["actions"]
    assert len(acts) == 1, acts
    a = acts[0]
    assert a["kinds"] == ["pr", "commit", "feed"] and a["title"] == "PROJ-21 add the export endpoint"
    assert a["repo"] == "acme/shop" and a["url"] == url
    assert a["at"] == "2026-10-16T14:30:00-04:00", "the latest copy's time"


def test_the_merged_line_keeps_repo_and_link_from_any_copy(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    url = "https://github.example.com/acme/shop/pull/15"
    _evidence({"id": "git:shop:eee555", "kind": "commit", "repo": "shop", "title": "fix: retry the export on timeout (#15)", "at": "2026-10-16T14:00:00-04:00"})
    feed.add_lines(json.dumps({"id": "gh:merge:15", "source": "github", "kind": "merge", "actor": "me", "at": "2026-10-16T18:30:00Z", "title": "Merged PR #15", "url": url}))
    acts = _group(standup.build(), "Other work")["actions"]
    assert len(acts) == 1 and acts[0]["repo"] == "acme/shop" and acts[0]["url"] == url and acts[0]["title"] == "fix: retry the export on timeout (#15)"


def test_two_prs_with_the_same_title_stay_two_lines(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _evidence(
        {"id": "pr:acme/shop#1", "kind": "pr", "repo": "acme/shop", "title": "fix: typo", "url": "https://github.example.com/acme/shop/pull/1", "at": "2026-10-16T11:00:00-04:00"},
        {"id": "git:shop:aaa000", "kind": "commit", "repo": "shop", "title": "fix: typo", "at": "2026-10-16T11:30:00-04:00"},  # a rebase merge: no number
        {"id": "pr:acme/shop#2", "kind": "pr", "repo": "acme/shop", "title": "fix: typo", "url": "https://github.example.com/acme/shop/pull/2", "at": "2026-10-16T12:00:00-04:00"},
    )
    acts = _group(standup.build(), "Other work")["actions"]
    assert sorted(a["url"].rsplit("/", 1)[-1] for a in acts) == ["1", "2"]
    assert sorted(a["kinds"] for a in acts) == [["pr"], ["pr", "commit"]]


def test_two_feed_events_with_one_title_on_two_tickets_stay_two_lines_on_their_rows(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows([{"title": "Row A", "owner": "me", "keys": ["PROJ-1"]}, {"title": "Row B", "owner": "me", "keys": ["PROJ-2"]}])
    lines = [
        {"id": "jira:t1", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T15:00:00Z", "title": "Moved a ticket to review", "url": "https://jira.example.com/browse/PROJ-1"},
        {"id": "jira:t2", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T16:00:00Z", "title": "Moved a ticket to review", "url": "https://jira.example.com/browse/PROJ-2"},
    ]
    feed.add_lines("\n".join(json.dumps(x) for x in lines))
    r = standup.build()
    assert r["counts"]["actions"] == 2
    assert [(a["ids"], a["keys"]) for a in _row(r, "Row A")["actions"]] == [(["jira:t1"], ["PROJ-1"])]
    assert [(a["ids"], a["keys"]) for a in _row(r, "Row B")["actions"]] == [(["jira:t2"], ["PROJ-2"])]
    assert not _row(r, "Row B")["talking_point"].startswith("No new work"), "row B keeps its work"


def test_two_commits_or_two_notes_with_one_title_are_two_actions(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _evidence(
        {"id": "git:shop:aaaa1111", "kind": "commit", "repo": "shop", "title": "fix: lint", "at": "2026-10-16T12:00:00-04:00"},
        {"id": "git:shop:bbbb2222", "kind": "commit", "repo": "shop", "title": "fix: lint", "at": "2026-10-18T12:00:00-04:00"},
        {"id": "note:a", "kind": "note", "title": "Reviewed a design doc", "at": "2026-10-16T13:00:00-04:00"},
        {"id": "note:b", "kind": "note", "title": "Reviewed a design doc", "at": "2026-10-18T13:00:00-04:00"},
    )
    acts = _group(standup.build(), "Other work")["actions"]
    assert sorted(a["ids"] for a in acts) == [["git:shop:aaaa1111"], ["git:shop:bbbb2222"], ["note:a"], ["note:b"]]


def test_one_title_from_two_sources_with_two_links_is_two_actions(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _evidence({"id": "jira:e1", "kind": "jira", "title": "Moved a ticket to review", "url": "https://jira.example.com/browse/PROJ-1", "at": "2026-10-16T13:00:00-04:00"})
    feed.add_lines(json.dumps({"id": "jira:f2", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T18:00:00Z", "title": "Moved a ticket to review", "url": "https://jira.example.com/browse/PROJ-2"}))
    acts = _group(standup.build(), "Other work")["actions"]
    assert sorted(a["ids"] for a in acts) == [["jira:e1"], ["jira:f2"]]


def test_a_note_and_a_feed_event_of_one_action_are_one_line(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _evidence({"id": "note:c", "kind": "note", "title": "Reviewed the queue design", "at": "2026-10-16T13:00:00-04:00"})
    feed.add_lines(json.dumps({"id": "gdocs:c1", "source": "gdocs", "kind": "comment", "actor": "me", "at": "2026-10-16T17:30:00Z", "title": "Reviewed the queue design", "url": "https://docs.example.com/d/1"}))
    acts = _group(standup.build(), "Other work")["actions"]
    assert [(a["ids"], a["kinds"]) for a in acts] == [(["note:c", "gdocs:c1"], ["note", "feed"])]


def test_personal_events_chat_and_work_outside_the_window_stay_out(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    lines = [
        {"id": "x:1", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T15:00:00Z", "title": "Dentist booking", "personal": True},
        {"id": "x:2", "source": "slack", "kind": "dm", "actor": "me", "at": "2026-10-16T15:00:00Z", "title": "DM to Sam: lunch?"},
        {"id": "x:3", "source": "jira", "kind": "transition", "actor": "other", "at": "2026-10-16T15:00:00Z", "title": "Sam moved a ticket"},
        {"id": "x:4", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-15T15:00:00Z", "title": "Before the window"},
        {"id": "x:5", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T15:00:00Z", "title": "Moved the export ticket"},
    ]
    feed.add_lines("\n".join(json.dumps(x) for x in lines))
    titles = [a["title"] for g in standup.build()["groups"] for a in g["actions"]]
    assert titles == ["Moved the export ticket"]


# ---------- input, page, CLI, web ----------


def test_bad_rows_name_the_fix_and_write_nothing():
    _rows([{"title": "Order export", "owner": "me"}])
    for bad, why in (
        ("", "no rows"),
        ("[{title: ok}, {status: x}]", "row 2: missing title"),
        ("[{title: ok, keys: ['proj 12']}]", "row 1: key"),
        ("{rows: [{title: ok}], groups: [{label: x}]}", "group 1"),
        ('{"rows": [{"title": "cut off"', "not JSON or YAML"),
    ):
        with pytest.raises(standup.StandupError, match=why):
            standup.save_rows(bad)
    assert [r["title"] for r in standup.load_rows()["rows"]] == ["Order export"], "the last good rows stay"
    assert "Order export" not in (standup.rows_path().read_bytes().decode("latin-1")), "kept encrypted at rest"


def test_the_page_puts_a_talking_point_first_then_status_then_evidence(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    _rows([{"title": "Order <export>", "status": "In review", "owner": "me", "keys": ["PROJ-21"]}])
    _evidence(
        {"id": "pr:acme/shop#12", "kind": "pr", "repo": "acme/shop", "title": "feat: add the export endpoint (PROJ-21)", "url": "https://github.example.com/acme/shop/pull/12", "at": "2026-10-16T13:00:00-04:00"},
        {"id": "git:shop:fff666", "kind": "commit", "repo": "shop", "title": "fix: retry the export on timeout (PROJ-21)", "at": "2026-10-16T15:00:00-04:00"},
        {"id": "git:shop:fff777", "kind": "commit", "repo": "shop", "title": "<script>alert(1)</script>", "at": "2026-10-16T16:00:00-04:00"},
    )
    r = standup.run()
    page = standup.page_path().read_text(encoding="utf-8")
    assert r["page"] == str(standup.page_path()) and "out" in standup.page_path().parts
    section = page.split('class="row mine"', 1)[1].split("</section>", 1)[0]
    talk, status, ev = section.index('class="talk"'), section.index('class="status"'), section.index('class="evidence"')
    assert talk < status < ev
    assert '<ul class="talk"><li>Retry the export on timeout (1 commit).</li><li>Add the export endpoint (1 PR).</li></ul>' in section, section
    assert "<b>Status:</b> In review" in section
    assert "<script>" not in page and "&lt;script&gt;" in page and "Order &lt;export&gt;" in page
    assert 'href="https://github.example.com/acme/shop/pull/12"' in page
    assert "<link" not in page and "src=" not in page, "the page loads nothing from the network"


def test_cli_prints_counts_and_the_page_path_to_an_agent_never_titles(monkeypatch, capsys, tmp_path):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    assert main(["work", "standup"]) == 0
    assert "no rows to report on yet" in capsys.readouterr().out
    rows = tmp_path / "rows.json"
    rows.write_text(json.dumps([{"title": "Order export", "owner": "me"}, {"title": "Search ranking", "owner": "Ana"}]))
    _evidence({"id": "note:9", "kind": "note", "title": "Wrote the PROJECTZEBRA export plan", "at": "2026-10-16T13:00:00-04:00"})
    assert main(["work", "standup", "--rows", str(rows), "--show"]) == 0
    out = capsys.readouterr().out
    assert "standup since Fri Oct 16, 10:00 (Etc/GMT+4)" in out
    assert "rows: 0 shown (0 yours, 0 matched by your work), 2 with no update hidden, updated 09:00" in out
    assert "your actions: 1 (0 on rows, 1 in Other work)" in out
    assert str(standup.page_path()) in out
    assert "PROJECTZEBRA" not in out and "Order export" not in out, "titles stay off a piped stdout"
    assert "titles hidden" in out
    assert main(["work", "standup", "--rows", str(tmp_path / "missing.json")]) == 2


def test_web_api_returns_the_same_report(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    from careerbrew import server

    _rows([{"title": "Order export", "status": "In review", "owner": "me", "aliases": ["export"]}])
    _evidence({"id": "note:1", "kind": "note", "title": "Wrote the export spec", "at": "2026-10-16T14:00:00-04:00"})
    v = server.standup_view()
    assert v["window"]["since"] == "Fri Oct 16, 10:00" and [r["title"] for r in v["rows"]] == ["Order export"]
    assert v["has_rows"] and v["page"].endswith("standup.html")
