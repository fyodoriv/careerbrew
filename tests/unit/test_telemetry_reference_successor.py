"""The telemetry successor changes only the telemetry actions of the frozen rules-promise-v3 reference."""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "xtask/tests/fixtures/capture"
PINS = {
    "actions-reference-rules-promise-v3.json": "e5069cb03701d43d14e8ccfe9a66cc42a6d710d51ae86cc76b5ee292a1dae6e9",
    "http-reference-rules-promise-v3.json": "bddc10911112bb79118ff109e0466426f912121996c4d7027276ee97ccdf8e64",
}
CHANGED = {"telemetry.status", "telemetry.on", "telemetry.off", "telemetry.send"}
ADDED = "telemetry.record-install"


def test_the_older_references_stay_byte_identical():
    for name, expected in PINS.items():
        assert hashlib.sha256((FIXTURES / name).read_bytes()).hexdigest() == expected


def test_telemetry_successor_changes_only_the_telemetry_actions():
    old = json.loads((FIXTURES / "actions-reference-rules-promise-v3.json").read_text())
    new = json.loads((FIXTURES / "actions-reference-telemetry-v4.json").read_text())
    names = [action["name"] for action in new]
    assert names.count(ADDED) == 1 and names[names.index(ADDED) - 1] == "telemetry.send"
    kept = [action for action in new if action["name"] != ADDED]
    assert [a["name"] for a in kept] == [a["name"] for a in old]
    for before, after in zip(old, kept):
        if before["name"] in CHANGED:
            assert before != after
        else:
            assert before == after, before["name"]


def test_http_successor_changes_only_the_two_catalog_answers():
    new_actions = json.loads((FIXTURES / "actions-reference-telemetry-v4.json").read_text())
    old_http = json.loads((FIXTURES / "http-reference-rules-promise-v3.json").read_text())
    new_http = json.loads((FIXTURES / "http-reference-telemetry-v4.json").read_text())
    assert len(old_http) == len(new_http)
    for index, (before, after) in enumerate(zip(old_http, new_http)):
        if index in (0, 4):
            expected = json.loads(json.dumps(before))
            expected["body"]["actions"] = new_actions
            expected["headers"]["content-length"] = str(len(json.dumps(expected["body"]).encode()))
            assert after == expected
        else:
            assert after == before
