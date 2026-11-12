"""Opt-in weekly aggregate metrics in the shared provider format (issue #47, agentbrew issue 346)."""

import json

import pytest

from careerbrew import config, metrics_share
from careerbrew.cli import main

from .conftest import CANARIES

CANARY_PERSON, CANARY_EMPLOYER, _CODENAME = CANARIES


@pytest.fixture
def out(home, monkeypatch, tmp_path):
    d = tmp_path / "metrics"
    monkeypatch.setenv("CAREERBREW_METRICS_DIR", str(d))
    return d


def test_off_by_default_writes_nothing(out, capsys):
    assert main(["metrics", "weekly"]) == 0
    assert "nothing written" in capsys.readouterr().out and not out.exists()
    assert metrics_share.maybe_write() is None


def test_on_writes_a_numbers_only_file_without_private_text(out, home):
    from careerbrew.work import evidence, priorities

    evidence.note(f"Shipped for {CANARY_EMPLOYER} with {CANARY_PERSON}", ["delivery"])
    priorities.add(f"Call {CANARY_PERSON}", 3, 3, 1, [])
    config.set_value("metrics_share", "true")
    path = metrics_share.write()
    data = json.loads(path.read_text())
    assert set(data) == {"tool", "version", "week", "machine_id", "metrics"} and data["tool"] == "careerbrew"
    assert path.name == f"weekly-{data['week']}.json" and data["metrics"]["evidence_items"] >= 1
    assert all(isinstance(v, int | float) and not isinstance(v, bool) for v in data["metrics"].values())
    text = path.read_text()
    assert not any(c in text for c in CANARIES)
    assert metrics_share.maybe_write() is None, "once per week"


def test_a_sunday_evening_note_counts_in_the_local_week_not_the_utc_week(out, home, monkeypatch):
    from careerbrew.work import evidence

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T20:09:00-04:00")  # Sunday local, already Monday in UTC
    evidence.note("Shipped the importer", ["delivery"])
    payload = metrics_share.collect()
    assert payload["week"] == "2026-W43" and payload["metrics"]["evidence_items"] == 1


def test_sessions_count_local_days(home, monkeypatch):
    from careerbrew import paths, store

    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-25T20:09:00-04:00")
    # Saturday 22:00 and Sunday 10:00 local: two local days, one UTC day (Sunday)
    store.append_jsonl(paths.sub("logs", "events.jsonl"), [{"ts": "2026-10-25T02:00:00Z", "event": "x"}, {"ts": "2026-10-25T14:00:00Z", "event": "x"}])
    assert metrics_share.collect()["metrics"]["sessions"] == 2


def test_a_text_value_is_refused():
    with pytest.raises(metrics_share.ShareError, match="not a number"):
        metrics_share.validate({"tool": "careerbrew", "version": "0", "week": "2026-W43", "machine_id": "x", "metrics": {"leak": "a title"}})
    with pytest.raises(metrics_share.ShareError, match="keys"):
        metrics_share.validate({"tool": "careerbrew", "metrics": {}})


def test_a_plugin_policy_forbids_it(out, tmp_path):
    root = tmp_path / "p"
    root.mkdir()
    (root / "careerbrew-plugin.yaml").write_text("name: acme\nversion: 0.1.0\npolicy: {metrics_share: false}\n")
    assert main(["plugin", "add", str(root)]) == 0
    config.set_value("metrics_share", "true")
    with pytest.raises(metrics_share.ShareError, match="plugin policy"):
        metrics_share.write()
