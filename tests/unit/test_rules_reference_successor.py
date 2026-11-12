"""The rules successor is an independent expected-side delta over frozen v1."""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "xtask/tests/fixtures/capture"
PINS = {
    "actions-v1.json": "5b450b0ba5e325027fc94b6e59ad9b88b50986bbc6a547d51a52f300d8feead7",
    "actions-reference-v1.json": "99cc30068c2264989d166acc38f114494b6252d0a4860a32506af25a53ccbba0",
    "http-v1.json": "f48799c203a561edb696cf3fd177f44052934e292c62cd3c743e2f857f2faf88",
    "http-reference-v1.json": "e58db0db976b3205bc206437141e3877a440a3a8a46f9da713e4a215fdb9eaf3",
    "actions-reference-rules-v2.json": "173921bfce50fb42c6f667e3b4c9e51cc9a86b963f8cafc9ced1f67012a065a2",
    "http-reference-rules-v2.json": "cea13248b3e4fd0a72aead0d580b11829473c704568aad26d0567fbe385079a6",
}


def test_rules_successor_is_only_the_preregistered_delta():
    for name, expected in PINS.items():
        assert hashlib.sha256((FIXTURES / name).read_bytes()).hexdigest() == expected
    delta_path = ROOT / "docs/plans/2026-11-08-rules-action-delta.json"
    assert hashlib.sha256(delta_path.read_bytes()).hexdigest() == "e776df411b7f03ec602d8bbf57289d17280e24aeab1f118b3f352bd2e83f65db"
    delta = json.loads(delta_path.read_text())
    old = json.loads((FIXTURES / "actions-reference-v1.json").read_text())
    new = json.loads((FIXTURES / "actions-reference-rules-v2.json").read_text())
    names = [item["name"] for item in old]
    assert names.count("work.project.memory") == names.count("work.project.sync") == 1
    anchor = names.index("work.project.memory") + 1
    assert names[anchor] == "work.project.sync"
    assert not any(item["name"] in names for item in delta)
    assert new == old[:anchor] + delta + old[anchor:]
    original_http = json.loads((FIXTURES / "http-reference-v1.json").read_text())
    successor_http = json.loads((FIXTURES / "http-reference-rules-v2.json").read_text())
    assert len(original_http) == len(successor_http)
    for index, (before, after) in enumerate(zip(original_http, successor_http)):
        if index in (0, 4):
            assert (before["method"], before["path"], before["status"]) == ("GET", "/api/actions", 200)
            assert before["body"] == {"actions": old}
            assert before["headers"]["content-length"] == "142809"
            expected = json.loads(json.dumps(before))
            expected["body"]["actions"] = new
            expected["headers"]["content-length"] = "144198"
            assert len(json.dumps(expected["body"]).encode()) == 144198
            assert after == expected
        else:
            assert after == before


def test_combined_successor_adds_only_rules_to_the_promise_reference():
    delta = json.loads((ROOT / "docs/plans/2026-11-08-rules-action-delta.json").read_text())
    old = json.loads((FIXTURES / "actions-reference-promise-v2.json").read_text())
    new = json.loads((FIXTURES / "actions-reference-rules-promise-v3.json").read_text())
    anchor = next(i for i, action in enumerate(old) if action["name"] == "work.project.memory") + 1
    assert new == old[:anchor] + delta + old[anchor:]
    old_http = json.loads((FIXTURES / "http-reference-promise-v2.json").read_text())
    new_http = json.loads((FIXTURES / "http-reference-rules-promise-v3.json").read_text())
    assert len(old_http) == len(new_http)
    for index, (before, after) in enumerate(zip(old_http, new_http)):
        if index in (0, 4):
            expected = json.loads(json.dumps(before))
            assert expected["body"]["actions"] == old
            expected["body"]["actions"] = new
            expected["headers"]["content-length"] = "145591"
            assert len(json.dumps(expected["body"]).encode()) == 145591
            assert after == expected
        else:
            assert after == before
