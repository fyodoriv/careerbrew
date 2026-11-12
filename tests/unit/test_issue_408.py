"""Required source health is separate from new intake (registered 408-01–14)."""

import json
import os
from datetime import datetime

import pytest

from careerbrew import cli
from careerbrew.work import live
from careerbrew.workday import watch


@pytest.fixture
def source(home, tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-05T12:00:00+00:00")
    cli.main(["init"])
    path = tmp_path / "inbox.json"
    path.write_text('{"tickets": []}')
    at = datetime.fromisoformat("2026-11-05T12:00:00+00:00").timestamp()
    os.utime(path, (at, at))
    live.start(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "required": True, "inbox": str(path)}]}))
    return path


@pytest.mark.parametrize("through_loop", [False, True], ids=["cycle", "running-loop"])
def test_408_02_fresh_unchanged_input_has_no_false_failure(source, through_loop):
    assert live.cycle()["status"] == "ok"
    before = live._state()
    if through_loop:
        assert watch.run_pass()["live"]["status"] == "ok"
    else:
        rec = live.cycle()
        assert rec["status"] == "ok"
        assert not rec["sources"]["tickets"]["pulled"]
        assert not rec["sources"]["tickets"]["stored"]
    after = live._state()
    assert after["seen"] == before["seen"]
    assert after["cursors"] == before["cursors"]
    assert after["followed"] == before["followed"]


def _clock(monkeypatch, text):
    monkeypatch.setenv("CAREERBREW_NOW", text)


def _write(path, text='{"tickets": []}', at="2026-11-05T12:00:00+00:00"):
    path.write_text(text)
    seconds = datetime.fromisoformat(at).timestamp()
    os.utime(path, (seconds, seconds))


def test_408_01_first_fresh_validation(source):
    rec = live.cycle()
    row = rec["sources"]["tickets"]
    assert rec["status"] == "ok" and row["health"] == "healthy"
    assert row["pulled"] and row["stored"] and not row["unchanged"]


def test_408_03_exact_deadline_is_inclusive_without_renewal(source, monkeypatch):
    live.cycle()
    _clock(monkeypatch, "2026-11-05T12:15:00+00:00")
    rec = live.cycle()
    assert rec["status"] == "ok"
    row = rec["sources"]["tickets"]
    assert row["age_seconds"] == 900 and row["unchanged"]
    assert row["acquired_at"] == "2026-11-05T12:00:00+00:00"


def test_408_04_expired_validation_cannot_be_renewed_by_poll(source, monkeypatch):
    live.cycle()
    _clock(monkeypatch, "2026-11-05T12:15:01+00:00")
    rec = live.cycle()
    assert rec["status"] == "failed" and rec["not_stored"] == ["tickets"]
    assert rec["sources"]["tickets"]["health"] == "stale"
    assert not rec["sources"]["tickets"]["stored"]
    _clock(monkeypatch, "2026-11-05T12:16:00+00:00")
    assert live.cycle()["sources"]["tickets"]["age_seconds"] == 960


@pytest.mark.parametrize("text", ['{"tickets": []}', "[]", '{"messages": []}', '{"events": []}'])
def test_408_05_supported_quiet_reply_stays_healthy(source, text):
    _write(source, text)
    assert live.cycle()["sources"]["tickets"]["items"] == 0
    row = live.cycle()["sources"]["tickets"]
    assert row["health"] == "healthy" and row["unchanged"] and not row["stored"]


def test_408_06_actual_duplicate_pull_renews_without_repeating_followups(source, monkeypatch):
    event = '[{"id":"safe1","at":"2026-11-05T10:00:00Z","title":"synthetic action","actor":"me"}]'
    _write(source, event)
    live.cycle()
    before = live._state()
    _clock(monkeypatch, "2026-11-05T12:14:00+00:00")
    _write(source, event, "2026-11-05T12:14:00+00:00")
    row = live.cycle()["sources"]["tickets"]
    assert row["pulled"] and row["stored"] and row["new"] == row["updated"] == 0
    assert row["health"] == "healthy" and row["age_seconds"] == 0
    assert live._state()["followed"] == before["followed"]
    _clock(monkeypatch, "2026-11-05T12:28:00+00:00")
    assert live.cycle()["status"] == "ok"


def test_408_07_changed_old_input_ingests_backlog_without_certifying_health(source):
    live.cycle()
    _write(source, '[{"id":"old","at":"2026-11-04T10:00:00Z","title":"old action"}]', "2026-11-05T11:00:00+00:00")
    row = live.cycle()["sources"]["tickets"]
    assert row["new"] == 1 and row["stored"] and row["health"] == "stale"
    assert live.status()["last"]["not_stored"] == ["tickets"]


def test_408_08_first_old_input_does_not_certify_freshness(source):
    _write(source, at="2026-11-05T11:00:00+00:00")
    assert live.cycle()["sources"]["tickets"]["health"] == "stale"


@pytest.mark.parametrize("failure", ["missing", "unreadable"])
def test_408_09_current_input_failure_overrides_good_receipt(source, monkeypatch, failure):
    live.cycle()
    if failure == "missing":
        source.unlink()
    else:
        original = type(source).read_bytes

        def denied(path):
            if path == source:
                raise PermissionError("synthetic denied")
            return original(path)

        monkeypatch.setattr(type(source), "read_bytes", denied)
    rec = live.cycle()
    assert rec["status"] == "failed" and rec["not_stored"] == ["tickets"]
    assert not rec["sources"]["tickets"]["stored"]


@pytest.mark.parametrize("text", ["not JSON", "", "{}", '"scalar"', '{"tickets":[{"bad":true}]}', '[{"id":"partial"}]', '{"tickets":[]}\nbad'])
def test_408_10_unusable_current_input_overrides_good_receipt(source, text):
    live.cycle()
    stamp = source.stat().st_mtime_ns
    source.write_text(text)
    os.utime(source, ns=(stamp, stamp))
    row = live.cycle()["sources"]["tickets"]
    assert row["health"] == "error" and not row["stored"]


@pytest.mark.parametrize("clock", ["2026-11-05T11:59:59+00:00", "2026-11-05T08:00:00-04:00"])
def test_408_11_clock_metadata_and_utc_equivalence(source, monkeypatch, clock):
    live.cycle()
    _clock(monkeypatch, clock)
    row = live.cycle()["sources"]["tickets"]
    assert row["health"] == ("healthy" if "08:00" in clock else "unknown")
    assert not row["stored"]


def test_408_11_backward_check_highwater_cannot_extend_lifetime(source, monkeypatch):
    live.cycle()
    _clock(monkeypatch, "2026-11-05T12:16:00+00:00")
    assert live.cycle()["status"] == "failed"
    _clock(monkeypatch, "2026-11-05T12:14:00+00:00")
    assert live.cycle()["sources"]["tickets"]["health"] == "unknown"


def test_408_11_future_input_is_unknown(source):
    _write(source, at="2026-11-05T12:01:00+00:00")
    assert live.cycle()["sources"]["tickets"]["health"] == "unknown"


@pytest.mark.parametrize("change", [{"kind": "code"}, {"pull": {"channels": ["synthetic"]}}, {"max_age_minutes": 1}])
def test_408_12_config_identity_requires_current_validation(source, change):
    live.cycle()
    config = {"name": "tickets", "kind": "tickets", "required": True, "inbox": str(source), **change}
    live.save_sources(json.dumps({"sources": [config]}))
    row = live.cycle()["sources"]["tickets"]
    assert row["pulled"] and row["stored"] and not row["unchanged"]


@pytest.mark.parametrize("failure", ["missing", "stale", "malformed"])
def test_408_13_optional_problem_does_not_fail_required_coverage(source, tmp_path, failure):
    optional = tmp_path / "optional.json"
    if failure != "missing":
        _write(optional, "bad" if failure == "malformed" else "[]", "2026-11-05T11:00:00+00:00")
    live.save_sources(
        json.dumps(
            {
                "sources": [
                    {"name": "tickets", "kind": "tickets", "required": True, "inbox": str(source)},
                    {"name": "optional", "kind": "chat", "inbox": str(optional)},
                ]
            }
        )
    )
    rec = live.cycle()
    assert rec["status"] == "ok" and not rec["not_stored"]
    assert rec["degraded"] == ["optional"]


@pytest.mark.parametrize("limit", [True, False, 1.5, "15", 0, -1, 10081, float("nan"), float("inf")])
def test_408_14_invalid_yaml_limits_save_nothing(source, limit):
    before = live.sources()
    with pytest.raises(live.LiveError, match="max_age_minutes"):
        live.save_sources(json.dumps({"sources": [{"name": "bad", "kind": "chat", "inbox": "synthetic", "max_age_minutes": limit}]}))
    assert live.sources() == before


@pytest.mark.parametrize("limit", [1, 10080])
def test_408_14_valid_limits_and_default_setting(source, limit):
    from careerbrew import config

    assert config.get("live_source_max_age_minutes") == 15
    assert config.set_value("live_source_max_age_minutes", str(limit)) == limit
    assert live.cycle()["sources"]["tickets"]["max_age_minutes"] == limit
    live.save_sources(json.dumps({"sources": [{"name": "tickets", "kind": "tickets", "inbox": str(source), "max_age_minutes": 15}]}))
    assert live.cycle()["sources"]["tickets"]["max_age_minutes"] == 15


@pytest.mark.parametrize("text", ['[{"type":"text","text":"broken"}]', '{"ok":false,"messages":[]}', '{"isError":true,"tickets":[]}'])
def test_408_10_error_or_malformed_wrapper_cannot_be_quiet(source, text):
    live.cycle()
    _write(source, text)
    row = live.cycle()["sources"]["tickets"]
    assert row["health"] == "error" and not row["stored"]


def test_408_02_cli_explains_healthy_unchanged_and_required_expiry(source, capsys, monkeypatch):
    live.cycle()
    capsys.readouterr()
    assert cli.main(["work", "live", "cycle"]) == 0
    out = capsys.readouterr().out
    assert "healthy; unchanged" in out
    _clock(monkeypatch, "2026-11-05T12:16:00+00:00")
    assert cli.main(["work", "live", "cycle"]) == 1
    out = capsys.readouterr().out
    assert "stale; unchanged" in out and "required source is not healthy" in out


def test_408_12_effective_default_policy_change_invalidates_previous_identity(source):
    from careerbrew import config

    live.cycle()
    config.set_value("live_source_max_age_minutes", "30")
    row = live.cycle()["sources"]["tickets"]
    assert row["pulled"] and not row["unchanged"]


def test_408_14_explicit_null_is_not_an_absent_limit(source):
    before = live.sources()
    with pytest.raises(live.LiveError, match="max_age_minutes"):
        live.save_sources(json.dumps({"sources": [{"name": "bad", "kind": "chat", "inbox": "synthetic", "max_age_minutes": None}]}))
    assert live.sources() == before


def test_408_11_subsecond_highwater_is_not_truncated(source, monkeypatch):
    _clock(monkeypatch, "2026-11-05T12:00:00.500000+00:00")
    live.cycle()
    _clock(monkeypatch, "2026-11-05T12:00:00.250000+00:00")
    assert live.cycle()["sources"]["tickets"]["health"] == "unknown"


def test_408_11_invalid_persisted_clock_is_unknown(source):
    from careerbrew import store

    live.cycle()
    st = live._state()
    st["checked_at"] = "invalid"
    store.write_json(live.state_path(), st)
    assert live.cycle()["sources"]["tickets"]["health"] == "unknown"


def test_408_11_unknown_future_reply_is_not_reingested_on_poll(source):
    _write(source, at="2026-11-05T12:01:00+00:00")
    assert live.cycle()["sources"]["tickets"]["health"] == "unknown"
    row = live.cycle()["sources"]["tickets"]
    assert row["unchanged"] and not row["stored"] and not row["pulled"]
    assert row["health"] == "unknown"


def _timing_sources(source, connector_first):
    file_source = {"name": "tickets", "kind": "tickets", "required": True, "inbox": str(source)}
    connector = {"name": "chat", "kind": "chat", "required": True, "adapter": "connector", "connector": "slack"}
    live.save_sources(json.dumps({"sources": [connector, file_source] if connector_first else [file_source, connector]}))


def test_408_timing_refresh_during_earlier_connector(source, monkeypatch):
    _timing_sources(source, connector_first=True)

    def pull(_src):
        _clock(monkeypatch, "2026-11-05T12:02:00+00:00")
        _write(source, at="2026-11-05T12:01:00+00:00")
        return []

    monkeypatch.setattr(live, "_read_connector", pull)
    rec = live.cycle()
    assert rec["status"] == "ok", rec["sources"]
    row = rec["sources"]["tickets"]
    assert row["health"] == "healthy" and row["age_seconds"] == 60
    assert row["validated_at"] == row["checked_at"] == "2026-11-05T12:02:00+00:00"
    # Do not modify the producer on the second poll.
    monkeypatch.setattr(live, "_read_connector", lambda _src: [])
    again = live.cycle()["sources"]["tickets"]
    assert again["health"] == "healthy" and again["unchanged"]
    assert not again["pulled"] and not again["stored"]
    assert again["acquired_at"] == "2026-11-05T12:01:00+00:00"


@pytest.mark.parametrize("delayed_stage", ["later-connector", "standup-postprocessing"])
def test_408_timing_expiry_before_completion(source, monkeypatch, delayed_stage):
    _timing_sources(source, connector_first=False)

    def finish_later(*_args):
        _clock(monkeypatch, "2026-11-05T12:16:00+00:00")
        return [] if delayed_stage == "later-connector" else {"status": "skipped", "reason": "no rows"}

    monkeypatch.setattr(live, "_read_connector", finish_later if delayed_stage == "later-connector" else lambda _src: [])
    if delayed_stage == "standup-postprocessing":
        monkeypatch.setattr(live, "_standup", finish_later)
    rec = live.cycle()
    assert rec["status"] == "failed" and "tickets" in rec["not_stored"]
    row = rec["sources"]["tickets"]
    assert row["health"] == "stale" and row["age_seconds"] == 960
    assert row["acquired_at"] == row["validated_at"] == "2026-11-05T12:00:00+00:00"
    assert row["checked_at"] == rec["at"] == "2026-11-05T12:16:00+00:00"


def test_408_timing_genuine_future_at_validation_cannot_be_accepted_at_completion(source, monkeypatch):
    _write(source, at="2026-11-05T12:01:00+00:00")

    def finish_later(*_args):
        _clock(monkeypatch, "2026-11-05T12:02:00+00:00")
        return {"status": "skipped", "reason": "no rows"}

    monkeypatch.setattr(live, "_standup", finish_later)
    rec = live.cycle()
    assert rec["status"] == "failed" and rec["sources"]["tickets"]["health"] == "unknown"
    row = live.cycle()["sources"]["tickets"]
    assert row["health"] == "unknown" and row["unchanged"] and not row["stored"]


def test_408_timing_reversal_retains_latest_observed_highwater(source, monkeypatch):
    _timing_sources(source, connector_first=False)

    def pull(_src):
        _clock(monkeypatch, "2026-11-05T12:02:00+00:00")
        return []

    def reverse(*_args):
        _clock(monkeypatch, "2026-11-05T12:01:00+00:00")
        return {"status": "skipped", "reason": "no rows"}

    monkeypatch.setattr(live, "_read_connector", pull)
    monkeypatch.setattr(live, "_standup", reverse)
    rec = live.cycle()
    assert rec["status"] == "failed"
    assert all(row["health"] == "unknown" for row in rec["sources"].values())
    assert live._state()["checked_at"] == "2026-11-05T12:02:00+00:00"
