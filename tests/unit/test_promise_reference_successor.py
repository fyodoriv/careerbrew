"""The promise successor is an independent expected-side delta over frozen v1."""

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
    "actions-reference-promise-v2.json": "f8f8787727c3cb588692b8737fb2f618461eca7b63917694c32a2c7468468921",
    "http-reference-promise-v2.json": "c37759bd5a280e3abf21923117b554228ede773823c90e9e584cec302b8331b0",
}


def test_promise_successor_is_only_the_preregistered_delta():
    for name, expected in PINS.items():
        assert hashlib.sha256((FIXTURES / name).read_bytes()).hexdigest() == expected
    delta_path = ROOT / "docs/plans/2026-11-08-promise-action-delta.json"
    assert hashlib.sha256(delta_path.read_bytes()).hexdigest() == "320f56dc37908b55b21a01d119cb676844018c140652e1d93c8811f908e6671f"
    delta = json.loads(delta_path.read_text())
    old = json.loads((FIXTURES / "actions-reference-v1.json").read_text())
    new = json.loads((FIXTURES / "actions-reference-promise-v2.json").read_text())
    names = [item["name"] for item in old]
    assert names.count("work.promise.add") == names.count("work.promise.done") == 1
    anchor = names.index("work.promise.add") + 1
    assert names[anchor] == "work.promise.done"
    assert not any(item["name"] in names for item in delta)
    assert new == old[:anchor] + delta + old[anchor:]
    original_http = json.loads((FIXTURES / "http-reference-v1.json").read_text())
    successor_http = json.loads((FIXTURES / "http-reference-promise-v2.json").read_text())
    assert len(original_http) == len(successor_http)
    for index, (before, after) in enumerate(zip(original_http, successor_http)):
        if index in (0, 4):
            assert (before["method"], before["path"], before["status"]) == ("GET", "/api/actions", 200)
            assert before["body"] == {"actions": old}
            assert before["headers"]["content-length"] == "142809"
            expected = json.loads(json.dumps(before))
            expected["body"]["actions"] = new
            expected["headers"]["content-length"] = "144202"
            assert len(json.dumps(expected["body"]).encode()) == 144202
            assert after == expected
        else:
            assert after == before
