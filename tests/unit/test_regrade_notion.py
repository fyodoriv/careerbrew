"""Second grader pass on the Notion import and flashcards (2026-10-19)."""

import json
import shutil
from datetime import UTC, datetime, timedelta

import pytest

from careerbrew import notion_import, store
from careerbrew.jobs import strategy
from careerbrew.tutor import cards

from .conftest import FIXTURES

BUNDLE = FIXTURES / "notion-bundle"
T0 = datetime(2026, 10, 19, 13, 0, tzinfo=UTC)


def _bundle(tmp_path, edit=None, manifest=None):
    b = tmp_path / "b"
    shutil.copytree(BUNDLE, b)
    if edit:
        kind, fn = edit
        rows = json.loads((b / f"{kind}.json").read_text())
        fn(rows)
        (b / f"{kind}.json").write_text(json.dumps(rows))
    if manifest is not None:
        (b / "manifest.json").write_text(json.dumps(manifest))
    return b


def test_d1_manifest_must_count_every_kind(home, tmp_path):
    b = _bundle(tmp_path, manifest={"counts": {}})
    (b / "stories.json").unlink()
    with pytest.raises(notion_import.ImportError_, match="counts"):
        notion_import.run(b)


def test_d2_unknown_stage_is_reported(home, tmp_path):
    b = _bundle(tmp_path, ("jobs", lambda r: r[0].update(stage="Ghosted")))
    r = notion_import.run(b)
    assert r["unmapped_stages"] == ["Ghosted"]


def test_d3_job_without_a_valid_date_does_not_block_forever(home, tmp_path):
    b = _bundle(tmp_path, ("jobs", lambda r: r[2].update(created="garbage")))
    notion_import.run(b)
    assert "zzgamma" not in strategy.used_companies(), "no date: do not block the company"


def test_d4_missing_title_names_the_file_and_row(home, tmp_path):
    b = _bundle(tmp_path, ("jobs", lambda r: r[1].pop("title")))
    with pytest.raises(notion_import.ImportError_, match=r"jobs\.json row 2.*title"):
        notion_import.run(b)


def _one(home):
    cards.write_deck("t", [{"id": "c1", "question": "q", "answer": "a", "topic": "x"}])


def test_d5_failing_while_learning_is_not_a_lapse(home):
    _one(home)
    at = T0
    for _ in range(cards.LEECH_LAPSES + 2):
        row = cards.review("c1", "again", at=at)
        at += timedelta(minutes=1)
    assert row["leech"] is False and row["lapses"] == 0


def _graduate_and_lapse(n, start):
    at = start
    row = {}
    for _ in range(n):
        cards.review("c1", "good", at=at)
        at += timedelta(days=30)
        cards.review("c1", "good", at=at)
        at += timedelta(days=60)
        row = cards.review("c1", "again", at=at)
        at += timedelta(days=1)
    return row, at


def test_d5_d6_d7_leech_counts_real_lapses_and_unsuspend_gives_room(home):
    _one(home)
    row, at = _graduate_and_lapse(cards.LEECH_LAPSES, T0)
    assert row["leech"] is True and row["lapses"] == cards.LEECH_LAPSES
    with pytest.raises(ValueError, match="suspended"):
        cards.review("c1", "good", at=at)
    cards.unsuspend("c1")
    row, at = _graduate_and_lapse(1, at)
    assert row["leech"] is False, "after unsuspend, the next leech warning waits half the threshold"


def test_unsuspend_unknown_card_is_refused(home):
    _one(home)
    with pytest.raises(KeyError):
        cards.unsuspend("nope")


def test_d8_today_is_the_local_day(home, monkeypatch):
    import time

    monkeypatch.setenv("TZ", "Etc/GMT+4")
    time.tzset()
    try:
        cards.write_deck("t", [{"id": f"c{i}", "question": "q", "answer": "a", "topic": "x"} for i in range(5)])
        evening = datetime(2026, 10, 19, 23, 30, tzinfo=UTC)  # 19:30 at UTC-4
        for c in cards.queue(at=evening, new_per_day=3):
            cards.review(c["id"], "good", at=evening)
        later = evening + timedelta(hours=1)  # 20:30 at UTC-4, next UTC day
        assert cards.queue(at=later, new_per_day=3) == [] or all(c["mode"] == "review" for c in cards.queue(at=later, new_per_day=3))
        s = cards.stats(at=later)
        assert s["reviewed_today"] == 3 and s["streak_days"] == 1
    finally:
        monkeypatch.delenv("TZ")
        time.tzset()


def test_d9_cli_review_can_quit(home, monkeypatch):
    from careerbrew import cli

    _one(home)
    answers = iter(["", "q"])
    monkeypatch.setattr("builtins.input", lambda *_: next(answers))
    assert cli.main(["cards", "review"]) == 0

    def eof(*_):
        raise EOFError

    monkeypatch.setattr("builtins.input", eof)
    assert cli.main(["cards", "review"]) == 0


def test_streak_counts_yesterday_until_you_review_today(home):
    _one(home)
    cards.review("c1", "good", at=T0)
    assert cards.stats(at=T0 + timedelta(days=1))["streak_days"] == 1
    _ = store


def test_empty_careerbrew_home_is_an_error_not_the_real_data(monkeypatch):
    from careerbrew import paths

    monkeypatch.setenv("CAREERBREW_HOME", "")
    with pytest.raises(RuntimeError, match="CAREERBREW_HOME is set but empty"):
        paths.data_root()
