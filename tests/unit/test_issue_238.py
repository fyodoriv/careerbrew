"""Issue #238: the PM update learns from the message the owner sent, not only from a web save.
The sent text comes from the `pm_sent_text(day)` hook: a text recorded with `day pm-sent` (a plugin skill or an
agent that sent it), else the owner's own message in the `pm_slack` channel that day, from the work feed."""

from __future__ import annotations

import pytest

from careerbrew import config, signals, store
from careerbrew.cli import main
from careerbrew.day import pm_learn
from careerbrew.work import feed

DAY = "2026-10-21"
DRAFT = "billing: export (1 PR); next: retries.\nledger: close the books (1 ticket).\ntooling: ci gate fix (1 PR)."
SENT = "billing: export (1 PR); next: retries.\nledger: close the books (1 ticket).\nLed the planning meeting with design; aligned on next steps.\nLaunched the export: now live for all teams."


@pytest.fixture(autouse=True)
def now(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-21T18:00")


def _own(eid: str, text: str, channel: str = "team-updates", at: str = "2026-10-21T21:30:00Z", actor: str = "me") -> dict:
    return {"id": eid, "source": "slack", "kind": "message", "at": at, "title": f"You wrote in #{channel}", "actor": actor, "channel": channel, "thread": eid, "text": text}


def test_the_hook_reads_the_owners_own_message_in_the_pm_channel():
    config.set_value("pm_slack", "#team-updates")
    pm_learn.remember(DAY, DRAFT, DRAFT)
    feed.ingest(
        [
            _own("slack:C1:1.1", "lunch anyone?"),
            _own("slack:C1:1.2", SENT),
            _own("slack:C2:1.3", SENT, channel="random"),
            _own("slack:C1:1.4", SENT + "\nsomeone else", actor="other"),
            _own("slack:C1:1.5", DRAFT, at="2026-10-20T21:00:00Z"),
        ]
    )
    assert pm_learn.pm_sent_text(DAY) == SENT, "the own message in the PM channel that day most like the draft"


def test_no_sent_message_is_none():
    config.set_value("pm_slack", "#team-updates")
    pm_learn.remember(DAY, DRAFT, DRAFT)
    feed.ingest([_own("slack:C1:1.1", "lunch anyone?")])
    assert pm_learn.pm_sent_text(DAY) is None, "a message that is not like the draft is not the update"


def test_a_recorded_sent_text_wins_and_is_learned_like_a_save():
    pm_learn.remember(DAY, DRAFT, DRAFT)
    pm_learn.save_sent(DAY, SENT)
    assert pm_learn.pm_sent_text(DAY) == SENT
    assert pm_learn.learn_sent(DAY) is True
    (row,) = store.read_jsonl(pm_learn.path())
    assert row["body"] == SENT and row["from"] == "sent"
    assert pm_learn.learn_sent(DAY) is False, "the same sent text is not recorded twice"


def test_lines_the_owner_added_become_added_lessons_with_a_source_hint():
    for day in ("2026-10-20", "2026-10-21"):
        pm_learn.remember(day, DRAFT, DRAFT)
        pm_learn.save_sent(day, SENT)
        pm_learn.learn_sent(day)
    learned = pm_learn.rules()
    assert learned["added"] == {"announcement": 2, "meeting": 2}, learned["added"]
    assert learned["cut"] == ["tooling"], "the small CI line the owner cut twice"


def test_one_added_line_is_not_a_lesson():
    pm_learn.remember(DAY, DRAFT, DRAFT)
    pm_learn.save_sent(DAY, SENT)
    pm_learn.learn_sent(DAY)
    assert pm_learn.rules()["added"] == {}


def test_signals_name_the_added_kinds(monkeypatch):
    monkeypatch.setattr("careerbrew.cli.may_show_business", lambda: True)
    for day in ("2026-10-20", "2026-10-21"):
        pm_learn.remember(day, DRAFT, DRAFT)
        pm_learn.save_sent(day, SENT)
        pm_learn.learn_sent(day)
    effects = [r["effect"] for r in signals.pm_summary() if r["kind"] == "added"]
    assert effects == ["you add announcement lines the draft misses (2 days)", "you add meeting lines the draft misses (2 days)"], effects


def test_day_pm_sent_records_the_text_and_never_prints_it(tmp_path, capsys):
    pm_learn.remember(DAY, DRAFT, DRAFT)
    f = tmp_path / "sent.txt"
    f.write_text(SENT + "\nSECRET-CANARY line")
    assert main(["day", "pm-sent", str(f), "--day", DAY]) == 0
    out = capsys.readouterr().out
    assert "SECRET-CANARY" not in out and "learned from the sent update" in out, out
    assert store.read_jsonl(pm_learn.path())[0]["from"] == "sent"


def test_day_pm_sent_with_no_shown_draft_says_so(tmp_path, capsys):
    f = tmp_path / "sent.txt"
    f.write_text(SENT)
    assert main(["day", "pm-sent", str(f), "--day", DAY]) == 0
    assert "no draft was shown for 2026-10-21" in capsys.readouterr().out


def test_the_watcher_learns_the_sent_update_once(monkeypatch):
    from datetime import date

    from careerbrew.day import watch

    config.set_value("pm_slack", "#team-updates")
    pm_learn.remember(DAY, DRAFT, DRAFT)
    feed.ingest([_own("slack:C1:1.2", SENT)])
    watch._learn_sent(date(2026, 10, 21))
    before = pm_learn.path().stat().st_mtime_ns
    watch._learn_sent(date(2026, 10, 21))
    assert pm_learn.path().stat().st_mtime_ns == before, "a second cycle writes nothing"
    (row,) = store.read_jsonl(pm_learn.path())
    assert row["from"] == "sent"


def test_a_web_save_never_replaces_the_sent_text():
    pm_learn.remember(DAY, DRAFT, DRAFT)
    pm_learn.save_sent(DAY, SENT)
    pm_learn.learn_sent(DAY)
    pm_learn.record(DAY, DRAFT + "\nlater web edit")
    (row,) = store.read_jsonl(pm_learn.path())
    assert row["body"] == SENT


def test_an_unlabeled_added_line_is_added_even_when_the_draft_has_an_unlabeled_line():
    for day in ("2026-10-20", "2026-10-21"):
        pm_learn.remember(day, "Nothing recorded yet.", "Nothing recorded yet.")
        pm_learn.save_sent(day, "Led the planning meeting with design.\nLaunched the export: now live.")
        pm_learn.learn_sent(day)
    assert pm_learn.rules()["added"] == {"announcement": 2, "meeting": 2}


def test_an_update_careerbrew_sends_is_learned_as_sent(home, monkeypatch):
    from pathlib import Path

    from careerbrew.day import log as daylog

    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(Path(__file__).resolve().parents[1] / "fixtures" / "http"))
    monkeypatch.setenv("CB_SLACK", "synthetic")
    assert main(["connect", "slack", "--ref", "env:CB_SLACK", "--no-verify"]) == 0
    config.set_value("pm_slack", "#team-updates")
    pm_learn.remember(DAY, DRAFT, DRAFT)
    daylog.send_pm(SENT)
    (row,) = store.read_jsonl(pm_learn.path())
    assert row["from"] == "sent" and row["body"] == SENT
