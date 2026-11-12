"""Issue #214, second half (task pm-why-it-matters): a "why it matters" note per PM update line, for the owner only,
and every result named, grouped by project, with no line cap."""

from __future__ import annotations

from datetime import date

import pytest

from careerbrew import config, connectors
from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.day import pm_learn, results
from careerbrew.work import evidence, priorities

DAY = date(2026, 10, 21)
CANARY = "CANARY-WHY-7f3a"  # a synthetic word only the owner-only note holds


@pytest.fixture(autouse=True)
def now(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T10:00")
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")


def _row(rid: str, kind: str, title: str) -> dict:
    return {"id": rid, "at": "2026-10-21T09:00", "kind": kind, "title": title, "source": "evidence", "origin": "user"}


def _task_with_impact(title: str, who: str, expected: str, metric: str = "") -> str:
    tid = priorities.add(title, 4, 4, 1.0, [])["id"]
    priorities.record_impact(tid, expected=expected, who=who, metric=metric)
    return tid


def test_every_project_gets_its_own_line_with_no_cap():
    repos = ["alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf"]
    results.append([_row(f"pr:acme/{r}#{n}", "pr", f"Build the {r} widget (#{n})") for n, r in enumerate(repos, 1)], DAY)
    lines = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert [x.split(":", 1)[0] for x in lines] == sorted(repos), "one line per project, no shared Other line"
    assert len(lines) == 7


def test_a_project_line_names_each_result_not_only_a_count():
    titles = ["Build the error card", "Fix the retry loop", "Add the export button", "Speed up the search page"]
    results.append([_row(f"pr:acme/shop#{n}", "pr", f"{t} (#{n})") for n, t in enumerate(titles, 1)], DAY)
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    for t in titles:
        assert t[0].lower() + t[1:] in line, line
    assert "(4 PRs)" in line, "the count stays, so the numbers can still be checked"


def test_a_very_busy_project_names_the_first_items_then_how_many_more():
    results.append([_row(f"pr:acme/shop#{n}", "pr", f"Ship shop change {chr(96 + n)} (#{n})") for n in range(1, 13)], DAY)
    (line,) = daylog.pm_lines(daylog.build(DAY), learn=False)
    assert line.count("ship shop change") == daylog.MAX_NAMED and f"and {12 - daylog.MAX_NAMED} more (12 PRs)" in line, line


def test_a_line_gets_a_why_note_from_the_tasks_expected_and_measured_impact():
    tid = _task_with_impact("Fix the failed exports", "the support team", "fewer failed exports", "failed exports per day")
    priorities.record_impact(tid, result="failed exports 40 to 3 per day", evidence_links=["https://example.com/pr/1"])
    pm = daylog.pm_report(daylog.build(DAY))
    (why,) = pm["why"]
    assert why["label"] == pm["draft"].split(":", 1)[0]
    note = why["note"]
    assert "support team" in note and "fewer failed exports" in note, note
    assert "checked: failed exports 40 to 3 per day (1 evidence link)" in note, note
    assert note.endswith(".") and note.count(".") == 1, "one plain sentence"


def test_an_unchecked_result_is_not_called_checked_and_the_metric_is_named():
    tid = _task_with_impact("Fix the failed exports", "support", "fewer failed exports", "failed exports per day")
    priorities.record_impact(tid, result="support seems happier", unsure=True)
    (why,) = daylog.pm_report(daylog.build(DAY))["why"]
    assert "checked" not in why["note"] and "measured by failed exports per day" in why["note"], why


def test_a_ranked_project_line_says_its_rank():
    config.set_value("project_priority", "billing=acme/billing, shop=acme/shop")
    results.append([_row("pr:acme/shop#1", "pr", "Build the error card (#1)"), _row("pr:acme/billing#2", "pr", "Close the books (#2)")], DAY)
    why = {w["label"]: w["note"] for w in daylog.pm_report(daylog.build(DAY))["why"]}
    assert why == {"billing": "Not sure: project 1 of 2 in your priority.", "shop": "Not sure: project 2 of 2 in your priority."}, "no result checks either line (issue #226)"


def test_no_invented_text_when_nothing_is_known():
    results.append([_row("pr:acme/shop#1", "pr", "Build the error card (#1)")], DAY)
    priorities.add("Plan the next sprint", 3, 3, 1.0, [])
    pm = daylog.pm_report(daylog.build(DAY))
    assert pm["draft"] and {w["note"] for w in pm["why"]} == {f"Not sure: {daylog.NO_IMPACT}."}, "issue #226: a note per line, and it invents nothing"


def test_a_renamed_label_keeps_its_note_and_a_cut_label_has_none(monkeypatch):
    _task_with_impact("Fix the failed exports", "support", "fewer failed exports")
    results.append([_row("pr:acme/shop#1", "pr", "Build the error card (#1)")], DAY)  # a second line: a cut never empties the draft
    learned = {"cut": [], "rename": {"Other": "Exports"}, "parts": [], "order": {}, "days": 2}
    monkeypatch.setattr(pm_learn, "rules", lambda rows=None: learned)
    pm = daylog.pm_report(daylog.build(DAY))
    assert "Exports: next: fix the failed exports." in pm["draft"].splitlines() and [w["label"] for w in pm["why"]] == ["shop", "Exports"], pm
    learned["rename"], learned["cut"] = {}, ["Other"]
    pm = daylog.pm_report(daylog.build(DAY))
    assert [w["label"] for w in pm["why"]] == ["shop"], "a line the lessons leave out has no note either"


def test_the_why_note_never_reaches_the_sent_or_copied_text(monkeypatch, capsys):
    """Canary: the note is in the report for the owner, and never in the body, the draft, Slack, or the mail draft."""
    _task_with_impact("Fix the failed exports", CANARY, "fewer failed exports")
    pm = daylog.pm_report(daylog.build(DAY))
    assert CANARY in pm["why"][0]["note"], "the canary is in the owner-only note"
    assert CANARY not in pm["body"] and CANARY not in pm["draft"] and CANARY not in pm["subject"]
    config.set_value("pm_slack", "#team-updates")
    monkeypatch.setattr(evidence, "sources_list", lambda: [{"kind": "slack", "token": "literal"}])
    monkeypatch.setattr("careerbrew.secrets.resolve", lambda ref: "xoxp-synthetic")
    sent: list[dict] = []
    monkeypatch.setattr(connectors, "http_json", lambda url, headers, payload: sent.append(payload) or {"ok": True, "ts": "1"})
    assert main(["day", "send", "--yes"]) == 0
    assert sent and CANARY not in sent[0]["text"] and "fewer failed exports" not in sent[0]["text"], sent
    capsys.readouterr()
    assert main(["day", "send", "--via", "email"]) == 0
    assert CANARY not in capsys.readouterr().out, "the mail draft link holds the body only"


def test_the_send_preview_shows_the_notes_apart_from_the_text(monkeypatch, capsys):
    _task_with_impact("Fix the failed exports", CANARY, "fewer failed exports")
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)  # the owner's own terminal
    assert main(["day", "send"]) == 2
    out = capsys.readouterr().out
    text, sep, notes = out.partition("Why it matters (for you only, never sent):")
    assert sep and CANARY not in text and f"Other: Not sure: helps {CANARY}" in notes, out
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: False)  # an agent sees a count, never the note
    assert main(["day", "send"]) == 2
    out = capsys.readouterr().out
    assert CANARY not in out and "why it matters: 1 note" in out, out
