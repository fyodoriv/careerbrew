"""Issue #229: an agent adds talking points (--talk) and actions careerbrew does not collect (--extra) to the standup.

Synthetic rows and actions only. The clock is pinned with CAREERBREW_NOW. 2026-10-19 is a Monday; the default
standup is 10:00 for 15 minutes, Monday to Friday, so a Monday 09:00 run counts from Friday's standup.
"""

from __future__ import annotations

import json

import pytest

from careerbrew import events, store
from careerbrew.cli import main
from careerbrew.work import standup

NOW = "2026-10-19T09:00"
ROWS = [{"title": "Order export", "owner": "me"}, {"title": "Billing ledger", "owner": "me"}, {"title": "Search ranking", "owner": "Ana"}]
TALK = "Shipped the export API and walked support through it. Next: the CSV format. Blocked on nothing."


def _row(report: dict, title: str) -> dict:
    return next(r for r in report["rows"] if r["title"] == title)


@pytest.fixture
def rows(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    standup.save_rows(json.dumps(ROWS))


def test_a_talking_point_from_an_agent_replaces_the_generated_line(rows):
    standup.save_notes(talk=json.dumps({"Order export": TALK}))
    r = standup.build()
    mine = _row(r, "Order export")
    assert mine["talking_point"] == TALK and mine["agent_written"] is True
    assert "Billing ledger" not in [x["title"] for x in r["rows"]], "issue #271: a row with no update is hidden"
    assert r["counts"]["talked"] == 1
    assert "written by an agent" in standup.render(r)


def test_an_extra_action_turns_a_no_new_work_row_into_a_row_with_work(rows):
    assert "Billing ledger" not in [x["title"] for x in standup.build()["rows"]], "no update yet: hidden (issue #271)"
    standup.save_notes(extra=json.dumps([{"title": "Agreed the ledger schema with finance", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Billing ledger"}]))
    r = standup.build()
    after = _row(r, "Billing ledger")
    assert [a["title"] for a in after["actions"]] == ["Agreed the ledger schema with finance"]
    assert after["actions"][0]["nouns"] == ["meeting"] and after["actions"][0]["via"] == "an agent" and after["actions"][0]["extra"] is True
    assert after["talking_point"] == "Agreed the ledger schema with finance (1 meeting)."
    assert r["counts"]["actions"] == 1 and r["counts"]["on_rows"] == 1 and r["counts"]["extra"] == 1


def test_an_extra_action_shows_a_row_that_is_not_yours(rows):
    assert "Search ranking" not in [x["title"] for x in standup.build()["rows"]]
    standup.save_notes(extra=json.dumps([{"title": "Reviewed the ranking thread", "at": "2026-10-16T15:00:00", "kind": "slack", "url": "https://slack.example.com/archives/C1/p1", "row": "Search ranking"}]))
    r = standup.build()
    ana = _row(r, "Search ranking")
    assert ana["actions"][0]["url"] == "https://slack.example.com/archives/C1/p1" and ana["actions"][0]["nouns"] == ["thread"]
    assert r["hidden"] == 2, "only your two rows with no update are hidden"


def test_an_extra_action_with_no_row_is_matched_like_other_work(rows):
    standup.save_notes(extra=json.dumps([{"title": "Order export demo for support", "at": "2026-10-16T16:00:00", "kind": "mail"}]))
    r = standup.build()
    assert r["rows"] == []  # the row has no alias: the action is Other work, and no row has an update
    assert [a["title"] for g in r["groups"] for a in g["actions"]] == ["Order export demo for support"]


def test_a_new_standup_window_clears_both(rows, monkeypatch):
    standup.save_notes(talk=json.dumps({"Order export": TALK}), extra=json.dumps([{"title": "Ledger sync", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Billing ledger"}]))
    assert standup.build()["counts"]["talked"] == 1
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-20T11:00")  # Tuesday after the standup: the window starts Monday
    r = standup.build()
    assert r["rows"] == [] and r["counts"]["extra"] == 0 and r["counts"]["talked"] == 0
    standup.run()
    assert not standup.notes_path().exists(), "a run in a new window deletes the old notes"


def test_passing_one_input_keeps_the_other_in_the_same_window(rows):
    standup.save_notes(talk=json.dumps({"Order export": TALK}))
    standup.save_notes(extra=json.dumps([{"title": "Ledger sync", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Billing ledger"}]))
    r = standup.build()
    assert r["counts"]["talked"] == 1 and r["counts"]["extra"] == 1


@pytest.mark.parametrize(
    ("talk", "extra", "error"),
    [
        ({"Order export": "x" * 201}, None, "at most 200 characters"),
        ({"Nope": "said"}, None, "Nope"),
        ({"Order export": ""}, None, "empty"),
        (["not a mapping"], None, "a mapping of row title to talking point"),
        (None, [{"at": "2026-10-16T14:00:00", "kind": "meeting"}], "extra 1: missing title"),
        (None, [{"title": "x", "at": "soon", "kind": "meeting"}], "extra 1: at"),
        (None, [{"title": "x", "at": "2026-10-10T14:00:00", "kind": "meeting"}], "outside the standup window"),
        (None, [{"title": "x", "at": "2026-10-16T14:00:00"}], "extra 1: missing kind"),
        (None, [{"title": "x", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Nope"}], "Nope"),
        (None, {"title": "x"}, "a list of"),
    ],
)
def test_a_bad_input_is_refused_names_the_fix_and_writes_nothing(rows, talk, extra, error):
    with pytest.raises(standup.StandupError, match=error):
        standup.save_notes(talk=None if talk is None else json.dumps(talk), extra=None if extra is None else json.dumps(extra))
    assert not standup.notes_path().exists()


def test_cli_keeps_both_encrypted_and_prints_counts_only(monkeypatch, capsys, tmp_path):
    monkeypatch.setenv("CAREERBREW_NOW", NOW)
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    (tmp_path / "rows.json").write_text(json.dumps(ROWS))
    (tmp_path / "talk.json").write_text(json.dumps({"Order export": "Said PROJECTZEBRA on the call."}))
    (tmp_path / "extra.json").write_text(json.dumps([{"title": "PROJECTOKAPI sync", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Billing ledger"}]))
    assert main(["work", "standup", "--rows", str(tmp_path / "rows.json"), "--talk", str(tmp_path / "talk.json"), "--extra", str(tmp_path / "extra.json")]) == 0
    out = capsys.readouterr().out
    assert "agent notes: 1 talking point, 1 extra action (kept until the next standup)" in out
    assert "PROJECTZEBRA" not in out and "PROJECTOKAPI" not in out
    raw = standup.notes_path().read_bytes()
    assert b"PROJECTZEBRA" not in raw and b"PROJECTOKAPI" not in raw, "kept encrypted"
    assert "PROJECTZEBRA" in (store.read_text(standup.notes_path()) or "")
    log = events.log_path().read_text(encoding="utf-8")
    assert "work.standup.notes" in log and "PROJECTZEBRA" not in log and "PROJECTOKAPI" not in log
    assert main(["work", "standup", "--talk", str(tmp_path / "talk.json"), "--extra", "-", "--rows", "-"]) == 2, "stdin feeds one input only"


def test_web_api_shows_both(rows):
    from careerbrew import server

    standup.save_notes(talk=json.dumps({"Order export": TALK}), extra=json.dumps([{"title": "Ledger sync", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Billing ledger"}]))
    v = server.standup_view()
    assert _row(v, "Order export")["agent_written"] and _row(v, "Billing ledger")["actions"][0]["title"] == "Ledger sync"
