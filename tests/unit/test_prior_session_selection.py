"""Synthetic, read-only prior-session selection controls (#432)."""

import copy
from datetime import date, datetime

import pytest

from careerbrew.work import prior_selection

A, B, P, Q = "a" * 32, "b" * 32, "1" * 32, "2" * 32
TODAY = date(2026, 11, 7)


def row(day, record=A, project=P, **extra):
    return {"day": day, "record_id": record, "project_incarnation": project, **extra}


def result(index, outcome, reason):
    return {"index": index, "outcome": outcome, "reason": reason}


@pytest.mark.parametrize(
    ("today", "rows", "projects", "expected"),
    [
        (TODAY, [row("2026-11-06")], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [row("2026-11-07")], [P], [result(0, "out_of_day", "outside_day")]),
        (TODAY, [row("2026-11-05")], [P], [result(0, "out_of_day", "outside_day")]),
        (TODAY, [row("2026-11-06", ended="yes", handoff="SYNTHCANARY")], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [row("2026-11-06", status="idle")], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [row("2026-11-06", status="blocked")], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [row("2026-11-06", status="done", pending=True)], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [{"day": "2026-11-06"}], [P], [result(0, "attention", "invalid_record_id")]),
        (TODAY, [row("2026-11-06", project=Q)], [P], [result(0, "attention", "unknown_project_id")]),
        (TODAY, [row("2026-11-06"), row("2026-11-06", project=Q)], [P], [result(0, "attention", "duplicate_record_id"), result(1, "attention", "duplicate_record_id")]),
        (TODAY, [row("2026-02-29")], [P], [result(0, "attention", "invalid_day")]),
        (TODAY, [row("2026-1-06")], [P], [result(0, "attention", "invalid_day")]),
        (date(2024, 3, 1), [row("2024-02-29")], [P], [result(0, "candidate", "prior_day")]),
        (date(2027, 1, 1), [row("2026-12-31")], [P], [result(0, "candidate", "prior_day")]),
        (TODAY, [row("2026-11-08")], [P], [result(0, "out_of_day", "outside_day")]),
        (TODAY, [row("2026-11-06"), row("2026-11-07")], [P], [result(0, "attention", "duplicate_record_id"), result(1, "attention", "duplicate_record_id")]),
        (TODAY, [row("2026-11-06")], [P, P], [result(0, "attention", "unknown_project_id")]),
        (TODAY, [row("2026-11-06", record="A" * 32)], [P], [result(0, "attention", "invalid_record_id")]),
        (TODAY, [row("2026-11-06", project="short")], [P], [result(0, "attention", "invalid_project_id")]),
        (TODAY, [None], [P], [result(0, "attention", "invalid_row")]),
        (TODAY, [row("2026-02-29"), row("2026-11-06")], [P], [result(0, "attention", "invalid_day"), result(1, "attention", "duplicate_record_id")]),
        (TODAY, [row("2026-11-07", record="bad")], [P], [result(0, "attention", "invalid_record_id")]),
    ],
)
def test_fixed_selection_cases(today, rows, projects, expected):
    before = copy.deepcopy((rows, projects))
    observed = prior_selection.select(today, rows, projects)
    assert observed == expected
    assert (rows, projects) == before
    assert "SYNTHCANARY" not in repr(observed)


@pytest.mark.parametrize(
    ("today", "rows", "projects"),
    [(datetime(2026, 11, 7), [], []), (TODAY, (), []), (TODAY, [], ()), (date.min, [], []), (TODAY, [], [P, None]), (TODAY, [], [P, "bad"])],
)
def test_malformed_input_has_fixed_content_free_error(today, rows, projects):
    with pytest.raises(ValueError, match="^invalid selection input$"):
        prior_selection.select(today, rows, projects)


def test_waiting_permutation_repeat_and_nested_values_do_not_mutate(monkeypatch, capsys):
    r0 = row("2026-11-06", status="waiting", extra={"note": ["SYNTHCANARY"]})
    r1 = row("2026-11-07", record=B)
    inputs = [r0, r1]
    reversed_inputs = [r1, r0]
    projects = [P]
    before = copy.deepcopy((inputs, reversed_inputs, projects))
    monkeypatch.setattr("builtins.open", lambda *args, **kwargs: pytest.fail("selector performed I/O"))
    first = [result(0, "candidate", "prior_day"), result(1, "out_of_day", "outside_day")]
    assert prior_selection.select(TODAY, inputs, projects) == first
    assert prior_selection.select(TODAY, inputs, projects) == first
    assert prior_selection.select(TODAY, reversed_inputs, projects) == [result(0, "out_of_day", "outside_day"), result(1, "candidate", "prior_day")]
    assert (inputs, reversed_inputs, projects) == before
    assert "SYNTHCANARY" not in repr((first, capsys.readouterr()))


def test_input_size_limits_are_fixed_and_content_free():
    with pytest.raises(ValueError, match="^invalid selection input$"):
        prior_selection.select(TODAY, [None] * 10_001, [])
    with pytest.raises(ValueError, match="^invalid selection input$"):
        prior_selection.select(TODAY, [], [P] * 10_001)
