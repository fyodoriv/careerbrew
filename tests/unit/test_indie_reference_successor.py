"""The indie successor only adds the `indie` and `peer` actions to the frozen telemetry-v4 reference."""

import hashlib
import json
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
FIXTURES = ROOT / "xtask/tests/fixtures/capture"
PINS = {
    "actions-reference-telemetry-v4.json": "4fc9c3b278eb98d7afbe843d84ac82332eb427a8846b054586a0ebd1dc49c38e",
    "http-reference-telemetry-v4.json": "52dc5c7914da8fd01a76e6694c26797aac358e6e7cc5a2ff625a28141cfd1419",
}
ADDED = ["indie.goal", "indie.income", "indie.scan", "indie.bets", "indie.bet", "indie.decide", "indie.measure", "indie.review", "indie.run", "indie.end", "peer.status", "peer.experiment"]


def test_the_older_references_stay_byte_identical():
    for name, expected in PINS.items():
        assert hashlib.sha256((FIXTURES / name).read_bytes()).hexdigest() == expected


def test_indie_successor_only_adds_the_indie_and_peer_actions():
    old = json.loads((FIXTURES / "actions-reference-telemetry-v4.json").read_text())
    new = json.loads((FIXTURES / "actions-reference-indie-v5.json").read_text())
    assert [a["name"] for a in new if a["name"] in ADDED] == ADDED
    assert [a for a in new if a["name"] not in ADDED] == old


def test_http_successor_changes_only_the_two_catalog_answers():
    new_actions = json.loads((FIXTURES / "actions-reference-indie-v5.json").read_text())
    old_http = json.loads((FIXTURES / "http-reference-telemetry-v4.json").read_text())
    new_http = json.loads((FIXTURES / "http-reference-indie-v5.json").read_text())
    assert len(old_http) == len(new_http)
    for index, (before, after) in enumerate(zip(old_http, new_http)):
        if index in (0, 4):
            expected = json.loads(json.dumps(before))
            expected["body"]["actions"] = new_actions
            expected["headers"]["content-length"] = str(len(json.dumps(expected["body"]).encode()))
            assert after == expected
        else:
            assert after == before


def test_the_live_catalog_equals_the_indie_successor(tmp_path, monkeypatch):
    from careerbrew import actions

    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path))
    live = json.loads(json.dumps(actions.catalog()))
    assert live == json.loads((FIXTURES / "actions-reference-indie-v5.json").read_text())
