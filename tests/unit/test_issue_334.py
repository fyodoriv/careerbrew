"""Issue #334: day results use the configured zone and chronological order."""

from datetime import date

import pytest

from careerbrew import cli, store
from careerbrew.day import results


@pytest.mark.parametrize(
    ("zone", "at", "expected"),
    [
        ("Etc/GMT+4", "2026-10-30T14:11:00Z", "10:11"),
        ("Etc/GMT+3", "2026-12-01T14:11:00+00:00", "11:11"),
        ("Asia/Kolkata", "2026-10-30T14:11:00Z", "19:41"),
        ("Etc/GMT+4", "2026-10-30T10:11:00", "10:11"),
        ("Etc/GMT+4", "2026-10-30T10:11:00-04:00", "10:11"),
    ],
)
def test_results_show_times_in_the_configured_zone(home, monkeypatch, capsys, zone, at, expected):
    store.write_yaml(home / "settings.yaml", {"timezone": zone})
    day = date.fromisoformat(at[:10])
    results.append([{"id": "pr-1", "kind": "pr", "at": at, "title": "Synthetic change"}], day)
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "results", "--date", day.isoformat(), "--show"]) == 0
    assert f"- {expected} [pr] Synthetic change" in capsys.readouterr().out


def test_results_sort_mixed_offsets_by_instant_not_stored_text(home, monkeypatch, capsys):
    store.write_yaml(home / "settings.yaml", {"timezone": "Etc/GMT+4"})
    day = date(2026, 10, 30)
    results.append(
        [
            {"id": "note-1", "kind": "note", "at": "2026-10-30T10:12:00", "title": "Later note"},
            {"id": "pr-1", "kind": "pr", "at": "2026-10-30T14:11:00Z", "title": "Earlier change"},
        ],
        day,
    )
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "results", "--date", day.isoformat(), "--show"]) == 0
    out = capsys.readouterr().out
    assert out.index("Earlier change") < out.index("Later note"), out


def test_results_keep_undated_rows_and_privacy_gate(home, monkeypatch, capsys):
    day = date(2026, 10, 30)
    results.append(
        [
            {"id": "missing", "kind": "note", "title": "Undated result"},
            {"id": "bad", "kind": "note", "at": "not-a-date", "title": "Malformed date result"},
        ],
        day,
    )
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["day", "results", "--date", day.isoformat(), "--show"]) == 0
    out = capsys.readouterr().out
    assert "titles hidden" in out
    assert "Undated result" not in out and "Malformed date result" not in out
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "results", "--date", day.isoformat(), "--show"]) == 0
    out = capsys.readouterr().out
    assert "Undated result" in out and "Malformed date result" in out
