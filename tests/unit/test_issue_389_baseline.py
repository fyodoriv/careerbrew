"""Check the previously measured sent-update outcome boundary (#389)."""

import json
from pathlib import Path

import pytest

from careerbrew import cli
from careerbrew.day import pm_learn
from careerbrew.work import promises

HISTORIES = Path(__file__).resolve().parents[1] / "fixtures/pm-promise-outcomes/histories.json"


def test_moved_item_keeps_one_id_on_replay(monkeypatch):
    data = json.loads(HISTORIES.read_text(encoding="utf-8"))
    monkeypatch.setenv("CAREERBREW_NOW", data["clock"])
    pm_learn.save_sent(data["first_day"], data["first"], receipt=data["first_receipt"])
    original = promises.open_list()
    assert len(original) == 1
    results = []
    for _ in range(data["O02"]["replay"]):
        pm_learn.save_sent(data["next_day"], data["O02"]["sent"], receipt=data["next_receipt"])
        results.append(sorted(row["name"] for row in promises.open_list()))
    assert results[0] == [original[0]["name"]]
    assert results[0] == results[1]


@pytest.mark.parametrize("argv", [["outcomes", "--json"], ["stats", "--days", "7", "--json"]])
def test_outcome_actions_are_available(argv, capsys):
    assert cli.main(["work", "promise", *argv]) == 0
    assert isinstance(json.loads(capsys.readouterr().out), dict)
