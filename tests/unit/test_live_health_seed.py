"""Source-health browser fixture uses a stable local day and the real clock."""

import importlib.util
from datetime import UTC, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

import pytest


@pytest.mark.parametrize("hour", range(24))
def test_fixture_timezone_keeps_real_instant_away_from_midnight(hour):
    path = Path(__file__).resolve().parents[1] / "e2e" / "live_health_seed.py"
    spec = importlib.util.spec_from_file_location("live_health_seed", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    instant = datetime(2026, 11, 8, hour, 59, 59, tzinfo=UTC)
    local = instant.astimezone(ZoneInfo(module.fixture_timezone(instant)))
    assert local.hour == 12
    assert local.astimezone(UTC) == instant
    assert (local + timedelta(minutes=2)).date() == local.date()


def test_fixture_keeps_incident_on_today_across_host_midnight(monkeypatch):
    """Exercise the fixture setup at the retained failure's wall-clock boundary."""
    import json
    import os
    from types import SimpleNamespace

    from careerbrew import calendar, cli, config, server
    from careerbrew.work import standup

    path = Path(__file__).resolve().parents[1] / "e2e" / "live_health_seed.py"
    spec = importlib.util.spec_from_file_location("live_health_seed_boundary", path)
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    instant = datetime(2026, 11, 8, 4, 54, 54, tzinfo=UTC)
    settings = {"timezone": "Etc/GMT+4"}
    observed = {}
    monkeypatch.setattr(os, "environ", dict(os.environ))
    monkeypatch.setattr(calendar, "now_utc", lambda: instant)
    monkeypatch.setattr(cli, "main", lambda *_: None)
    monkeypatch.setattr(config, "set_value", lambda key, value: settings.__setitem__(key, value))
    monkeypatch.setattr(standup, "save_rows", lambda *_: None)
    monkeypatch.setattr(module.signal, "signal", lambda *_: None)
    monkeypatch.setattr(module.threading, "Event", lambda: SimpleNamespace(wait=lambda: None, set=lambda: None))

    def serve(*_args):
        root = Path(os.environ["HOME"]).parent
        event = json.loads((root / "update.jsonl").read_text())
        tz = ZoneInfo(settings["timezone"])
        observed["event_day"] = datetime.fromisoformat(event["at"]).astimezone(tz).date()
        observed["today"] = (instant + timedelta(minutes=7)).astimezone(tz).date()
        app = SimpleNamespace(server_address=("127.0.0.1", 1), shutdown=lambda: None, server_close=lambda: None)
        return app, SimpleNamespace(join=lambda timeout: None)

    monkeypatch.setattr(server, "serve_in_thread", serve)
    module.main()
    assert observed["event_day"] == observed["today"]
