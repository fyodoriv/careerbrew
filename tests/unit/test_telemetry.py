"""US-89 field telemetry: allowlist, switches, cursor, failures. A fake local endpoint only; nothing real is sent."""

from __future__ import annotations

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

import pytest

from careerbrew import cli, config, events, telemetry

CANARY = "CANARY-EMPLOYER-3X"


class _Fake:
    def __init__(self, status: int = 200):
        self.bodies: list[dict] = []
        self.status = status
        fake = self

        class H(BaseHTTPRequestHandler):
            def do_POST(self):  # noqa: N802
                fake.bodies.append({"path": self.path, "json": json.loads(self.rfile.read(int(self.headers["Content-Length"])))})
                self.send_response(fake.status)
                self.end_headers()

            def log_message(self, *a):
                pass

        self.srv = ThreadingHTTPServer(("127.0.0.1", 0), H)
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.srv.server_address[1]}"


@pytest.fixture
def fake(home, monkeypatch):
    f = _Fake()
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", f.url)
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "phc_synthetic_test_key")
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    monkeypatch.delenv("DO_NOT_TRACK", raising=False)
    yield f
    f.srv.shutdown()


def _log(*records):
    with events.log_path().open("a", encoding="utf-8") as fh:
        for r in records:
            fh.write(json.dumps(r) + "\n")


def test_only_allowlisted_fields_leave_the_machine(fake, home):
    _log(
        {"ts": "2026-10-26T10:00:00Z", "event": "cli.run", "command": "day.log", "status": "ok", "ms": 120, "title": f"Offer from {CANARY}", "path": f"/Users/{CANARY}/x", "route": "/api/x", "added": 3},
        {"ts": "2026-10-26T10:00:01Z", "event": f"Bad {CANARY}"},
        {"ts": "2026-10-26T10:00:02Z", "event": "cli.error", "command": "x", "error": CANARY, "ok": False, "count": float("nan")},
    )
    assert telemetry.send() == {"sent": 2, "status": "ok"}
    sent = json.dumps([b["json"] for b in fake.bodies])
    for leak in (CANARY, "/Users/", "/api/x", "title", "path", "route"):
        assert leak not in sent
    batch = fake.bodies[0]["json"]["batch"]
    assert fake.bodies[0]["path"] == "/batch/"
    assert batch[0]["properties"]["command"] == "day.log" and batch[0]["properties"]["ms"] == 120 and batch[0]["properties"]["added"] == 3
    assert batch[0]["properties"]["$process_person_profile"] is False
    assert batch[0]["distinct_id"] == telemetry.install_id() and len(batch[0]["distinct_id"]) == 32
    assert "count" not in batch[1]["properties"] and batch[1]["properties"]["ok"] is False


def test_the_host_and_user_names_never_leave(fake, home):
    import getpass
    import socket

    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run", "command": "start", "status": "ok"})
    telemetry.send()
    sent = json.dumps(fake.bodies)
    assert socket.gethostname() not in sent and f'"{getpass.getuser()}"' not in sent


def test_nothing_is_sent_twice_and_a_partial_line_waits(fake, home, monkeypatch):
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run", "command": "a", "status": "ok"})
    with events.log_path().open("a") as fh:
        fh.write('{"event": "cli.run"')  # still being written
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")  # keep the send's own event out of this count
    assert telemetry.send()["sent"] == 1
    assert telemetry.send()["sent"] == 0
    with events.log_path().open("a") as fh:
        fh.write(', "command": "b"}\n')
    assert telemetry.send()["sent"] == 1
    assert [e["properties"]["command"] for b in fake.bodies for e in b["json"]["batch"]] == ["a", "b"]


def test_no_key_sends_nothing(fake, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "")
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    assert telemetry.send()["status"] == "off" and fake.bodies == []


@pytest.mark.parametrize("switch", ["dnt", "config"])
def test_off_switches_send_nothing(fake, home, monkeypatch, switch):
    if switch == "dnt":
        monkeypatch.setenv("DO_NOT_TRACK", "1")
    else:
        config.set_value("telemetry", "false")
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    assert telemetry.send()["status"] == "off"
    assert telemetry.maybe_send() is None and telemetry.notice_once() is None
    assert fake.bodies == []


def test_a_failed_send_never_raises_and_keeps_the_events(fake, home, capsys):
    fake.status = 500
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run", "command": "a"})
    assert telemetry.send()["status"] == "fail"
    assert cli.main(["telemetry", "send"]) == 1
    assert "stay queued" in capsys.readouterr().out
    fake.status = 200
    assert telemetry.send()["sent"] >= 1


def test_an_unreachable_endpoint_never_fails_the_watcher(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "phc_synthetic_test_key")
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    telemetry.notice_once()
    assert telemetry.maybe_send()["status"] == "fail"  # 127.0.0.1:9 refuses


def test_the_watcher_sends_once_a_day_and_only_after_the_notice(fake, home):
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    assert telemetry.maybe_send() is None, "no notice shown yet"
    assert telemetry.notice_once() == telemetry.NOTICE
    assert telemetry.notice_once() is None, "the notice shows once"
    assert telemetry.maybe_send()["sent"] >= 1
    _log({"ts": "2026-10-26T11:00:00Z", "event": "cli.run"})
    assert telemetry.maybe_send() is None, "once a day"


def test_dry_run_prints_the_exact_body_without_the_key_and_sends_nothing(fake, home, capsys):
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run", "command": "day.log", "status": "ok", "ms": 5})
    assert cli.main(["telemetry", "send", "--dry-run"]) == 0
    out = capsys.readouterr().out
    assert "phc_synthetic_test_key" not in out and '"<posthog_key>"' in out and '"day.log"' in out
    assert fake.bodies == []


def test_status_on_off(fake, home, capsys):
    assert cli.main(["telemetry", "off"]) == 0
    cli.main(["telemetry", "status"])
    assert "telemetry: off" in capsys.readouterr().out
    cli.main(["telemetry", "on"])
    cli.main(["telemetry", "status"])
    assert "telemetry: on (on)" in capsys.readouterr().out


def test_a_plugin_policy_forbids_telemetry(fake, home, monkeypatch):
    monkeypatch.setattr(telemetry.plugins, "installed", lambda: [{"manifest": {"policy": {"telemetry": False}}}])
    assert telemetry.enabled() == (False, "a plugin policy forbids telemetry on this machine")


@pytest.mark.parametrize("address", ["http://[2001:db8::1%25en0]:8000", "http://[::1]:8000", "http://127.0.0.1:8000", "https://telemetry.example.net."])
def test_a_valid_ip_or_dns_telemetry_address_is_not_called_malformed(fake, home, monkeypatch, address):
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", address)
    monkeypatch.setattr(telemetry.connectors, "host_allowed", lambda host: True)
    assert "not a valid address" not in telemetry.blocked()


@pytest.mark.parametrize(
    "address",
    [
        "https://[",
        "https://telemetry.example.net:bad",
        "https://",
        "ftp://telemetry.example.net",
        "telemetry.example.net",
        "https://bad host.example",
        "https://a..example.net",
        "https://-a.example.net",
        "https://[::1::2]",
        "https://[2001:db8::1%]",
        "https://" + ".".join(["a" * 63] * 5),
        "https://127.0.0.999",
        "https://999.999.999.999",
        "https://1.2.3",
    ],
)
def test_a_malformed_telemetry_address_turns_telemetry_off_and_never_raises(fake, home, monkeypatch, address):
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", address)
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    ok, why = telemetry.enabled()
    assert not ok and "not a valid address" in why
    assert telemetry.send()["status"] == "off" and telemetry.maybe_send() is None


def test_a_host_policy_without_the_telemetry_host_sends_nothing(fake, home, monkeypatch):
    """The telemetry post obeys the plugin `allowed_hosts` policy, like the connector calls."""
    calls: list[str] = []
    monkeypatch.setattr(telemetry.connectors, "open_url", lambda req, **kw: calls.append(req.full_url))
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", "https://telemetry.example.net")
    monkeypatch.setattr(telemetry.connectors.plugins, "effective_policy", lambda: {"allowed_hosts": ["jira.example.com"]})
    _log({"ts": "2026-10-26T10:00:00Z", "event": "cli.run"})
    ok, why = telemetry.enabled()
    assert not ok and "allowed_hosts" in why
    assert telemetry.send()["status"] == "off"
    assert telemetry.maybe_send() is None and telemetry.notice_once() is None
    with pytest.raises(telemetry.connectors.ConnectorError, match="allowed_hosts"):
        telemetry._post({"api_key": "phc_synthetic_test_key", "batch": []})
    assert calls == []
    monkeypatch.setattr(telemetry.connectors.plugins, "effective_policy", lambda: {"allowed_hosts": ["example.net"]})
    assert telemetry.enabled() == (True, "on")


def test_cli_runs_log_their_duration(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    cli.main(["telemetry", "status"])
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    assert isinstance([r for r in rows if r["event"] == "cli.run"][-1]["ms"], int)
