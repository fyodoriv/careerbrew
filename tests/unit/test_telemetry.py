"""US-89 field telemetry: consent, allowlist, switches, cursor, failures. A fake local endpoint only; nothing real is sent."""

from __future__ import annotations

import json
import re
import subprocess
import threading
import urllib.request
from datetime import timedelta
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

from careerbrew import cli, config, events, telemetry

CANARY = "CANARY-EMPLOYER-3X"
FAKE_KEY = "phc_fake0test0ke"
ROOT = Path(__file__).resolve().parents[2]


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

    def events(self) -> list[dict]:
        return [e for b in self.bodies for e in b["json"]["batch"]]


def _switches_off(monkeypatch):
    for name in ("DO_NOT_TRACK", "CI", "CAREERBREW_TELEMETRY"):
        monkeypatch.delenv(name, raising=False)  # the caller's shell or the runner may set them; these tests need a send


@pytest.fixture
def endpoint(home, monkeypatch):
    """A fake endpoint and a key, but no answer yet."""
    f = _Fake()
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", f.url)
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", FAKE_KEY)
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    _switches_off(monkeypatch)
    yield f
    f.srv.shutdown()


@pytest.fixture
def fake(endpoint):
    """The same, after a yes."""
    telemetry.set_consent("yes")
    return endpoint


def _ts(seconds: int = 1) -> str:
    """A stamp just after now: after the yes, inside the backlog window."""
    return (events.utc_now() + timedelta(seconds=seconds)).strftime("%Y-%m-%dT%H:%M:%SZ")


def _log(*records):
    with events.log_path().open("a", encoding="utf-8") as fh:
        for i, r in enumerate(records):
            fh.write(json.dumps({"ts": _ts(i + 1), **r}) + "\n")


# --- consent -----------------------------------------------------------------------------------------------


def test_consent_unset_sends_nothing(endpoint, home):
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    assert telemetry.consent() == "unset"
    assert telemetry.send()["status"] == "off" and telemetry.maybe_send() is None
    assert endpoint.bodies == []


def test_no_sends_nothing_even_with_a_key(endpoint, home):
    telemetry.set_consent("no")
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    assert telemetry.send()["status"] == "off" and "you said no" in telemetry.send()["why"]
    assert endpoint.bodies == []


def test_a_yes_never_sends_an_event_from_before_it(endpoint, home):
    old = (events.utc_now() - timedelta(minutes=5)).strftime("%Y-%m-%dT%H:%M:%SZ")
    with events.log_path().open("a") as fh:
        fh.write(json.dumps({"ts": old, "event": "cli.run", "command": "start", "status": "ok"}) + "\n")
    telemetry.set_consent("yes")
    # A line written after the yes but stamped before it (a clock jump, an old copy) never leaves either.
    with events.log_path().open("a") as fh:
        fh.write(json.dumps({"ts": old, "event": "cli.run", "command": "day.log", "status": "ok"}) + "\n")
    _log({"event": "cli.run", "command": "review", "status": "ok"})
    assert telemetry.send()["status"] == "ok"
    sent = [e["properties"].get("command") for e in endpoint.events()]
    assert "start" not in sent and "day.log" not in sent and "review" in sent
    since = telemetry._state()["telemetry_consent_time"]
    assert all(e["timestamp"] >= since for e in endpoint.events())


def test_no_terminal_means_no_question_and_no_send(endpoint, home, capsys):
    asked: list[str] = []
    assert telemetry.ask_on_start(interactive=False, read=lambda: asked.append("read") or "y", write=asked.append) is None
    assert asked == [] and telemetry.consent() == "unset"
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    assert telemetry.maybe_send() is None and endpoint.bodies == []


def test_the_question_names_what_and_where_and_saves_the_answer(endpoint, home):
    shown: list[str] = []
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y\n", write=shown.append) == "yes"
    text = "".join(shown)
    for part in ("What is sent", "never sent", "PostHog EU", "telemetry send --dry-run"):
        assert part in text
    assert config.get("telemetry_consent") == "yes" and telemetry._state()["telemetry_consent_time"]
    assert telemetry.ask_on_start(interactive=True, read=lambda: "n\n", write=shown.append) is None, "an answer is never asked again"


def test_the_question_is_never_asked_in_ci_or_with_an_off_switch(endpoint, home, monkeypatch):
    monkeypatch.setenv("CI", "true")
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y", write=lambda t: None) is None
    monkeypatch.delenv("CI")
    monkeypatch.setenv("DO_NOT_TRACK", "1")
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y", write=lambda t: None) is None
    assert telemetry.consent() == "unset"


def test_no_answer_asks_again_at_most_once_a_day_and_three_times(endpoint, home, monkeypatch):
    n = []
    assert telemetry.ask_on_start(interactive=True, read=lambda: "", write=lambda t: n.append(t)) is None
    assert telemetry.ask_on_start(interactive=True, read=lambda: "", write=lambda t: n.append(t)) is None
    assert sum("Share anonymous" in t for t in n) == 1, "the second call on the same day shows nothing"
    state = telemetry._state()
    telemetry._patch(asked_day="2000-01-01", ask_count=telemetry.MAX_ASKS)
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y", write=lambda t: None) is None
    assert state["ask_count"] == 1


def test_a_corporate_machine_defaults_to_no(endpoint, home, monkeypatch):
    monkeypatch.setattr(telemetry, "_role", lambda: "corporate")
    assert telemetry.consent() == "no"
    _log({"event": "cli.run", "command": "start"})
    assert telemetry.send()["status"] == "off" and endpoint.bodies == []
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y", write=lambda t: None) is None


def test_status_on_an_install_from_before_the_question_leaves_consent_unset_and_sends_nothing(endpoint, home, monkeypatch):
    """A state from before the question (the old notice, a send day) and a Keychain key: the answer stays unset, as in
    agentbrew. A read-only call (`telemetry status`, `blocked`) never writes an answer; the next terminal run asks."""
    monkeypatch.delenv("CAREERBREW_POSTHOG_KEY")
    monkeypatch.setattr(telemetry, "_keychain_key", lambda: FAKE_KEY)
    telemetry._patch(noticed=True, last_send_day="2026-11-01")
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    assert cli.main(["telemetry", "status"]) == 0
    assert telemetry.consent() == "unset" and telemetry.blocked().startswith("not answered")
    assert "telemetry_consent" not in config._file() and not telemetry._state().get("telemetry_consent_time")
    assert telemetry.maybe_send() is None and telemetry.send()["status"] == "off"
    assert endpoint.bodies == []


def test_an_old_telemetry_false_still_means_no(home, monkeypatch):
    monkeypatch.delenv("CAREERBREW_POSTHOG_KEY")
    monkeypatch.setattr(telemetry, "_keychain_key", lambda: FAKE_KEY)
    from careerbrew import paths, store

    store.write_yaml(paths.data_root() / "settings.yaml", {"telemetry": False})
    telemetry._patch(noticed=True)
    assert telemetry.consent() == "no"


def test_a_yes_saved_with_config_set_still_starts_at_its_own_time(endpoint, home):
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    config.set_value("telemetry_consent", "yes")
    assert telemetry.consent() == "yes"
    assert telemetry.pending()[0] == [], "nothing logged before the yes is pending"


# --- the key -------------------------------------------------------------------------------------------------


def test_the_key_lookup_order_and_shape(home, monkeypatch):
    monkeypatch.setattr(telemetry, "_keychain_key", lambda: "")
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "not-a-key")
    assert telemetry.resolve_key() == ("", "none"), "a set env var stops the lookup; a wrong shape is no key"
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", FAKE_KEY)
    assert telemetry.resolve_key() == (FAKE_KEY, "env")
    monkeypatch.delenv("CAREERBREW_POSTHOG_KEY")
    assert telemetry.resolve_key() == ("", "none"), "no key is committed: with no env value and no Keychain entry, there is none"
    monkeypatch.setattr(telemetry, "_keychain_key", lambda: FAKE_KEY)
    assert telemetry.resolve_key() == (FAKE_KEY, "keychain")


def test_no_key_file_is_committed_or_packaged():
    assert not list((ROOT / "src" / "careerbrew").glob("*key*.txt"))


def test_with_a_yes_and_no_key_events_stay_local_and_status_names_the_fix(endpoint, home, monkeypatch, capsys):
    monkeypatch.setattr(telemetry, "_keychain_key", lambda: "")
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "")
    cli.main(["telemetry", "on"])
    _log({"event": "cli.run", "command": "day.log", "status": "ok"})
    assert "careerbrew secret set posthog_key" in capsys.readouterr().out
    cli.main(["telemetry", "status"])
    out = capsys.readouterr().out
    assert "consent: yes" in out and "no key: set one with `careerbrew secret set posthog_key`" in out
    cli.main(["telemetry", "send"])
    assert endpoint.bodies == [] and telemetry.pending()[0], "nothing leaves, and the events wait locally"


def test_the_key_never_appears_in_output_logs_or_errors(fake, home, capsys):
    _log({"event": "cli.run", "command": "day.log", "status": "ok"})
    for argv in (["telemetry", "status"], ["telemetry", "send", "--dry-run"], ["telemetry", "on"], ["telemetry", "send"]):
        cli.main(argv)
    fake.status = 500
    _log({"event": "cli.run", "command": "day.log"})
    cli.main(["telemetry", "send"])
    out = capsys.readouterr()
    assert FAKE_KEY not in out.out + out.err
    assert FAKE_KEY not in events.log_path().read_text()
    from careerbrew import paths

    assert FAKE_KEY not in json.dumps(telemetry._state()) and FAKE_KEY not in (paths.data_root() / "settings.yaml").read_text()


# --- what leaves --------------------------------------------------------------------------------------------


def test_only_allowlisted_fields_leave_the_machine(fake, home):
    _log(
        {"event": "cli.run", "command": "day.log", "status": "ok", "ms": 120, "title": f"Offer from {CANARY}", "path": f"/Users/{CANARY}/x", "route": "/api/x", "added": 3},
        {"event": f"Bad {CANARY}"},
        {"event": "cli.error", "command": "x", "error": CANARY, "ok": False, "count": float("nan")},
        {"event": "cli.run", "command": CANARY.lower(), "status": CANARY.lower(), "story": CANARY.lower(), "source_kind": "acme-internal", "tier": "acme", "big": 10**15},
        {"event": "workday.update", "status": "done"},
    )
    assert telemetry.send() == {"sent": 4, "status": "ok"}
    sent = json.dumps([b["json"] for b in fake.bodies])
    for leak in (CANARY, CANARY.lower(), "/Users/", "/api/x", "title", "path", "route", "acme", '"big"'):
        assert leak not in sent
    batch = fake.bodies[0]["json"]["batch"]
    assert fake.bodies[0]["path"] == "/batch/"
    p = batch[0]["properties"]
    assert p["command"] == "day.log" and p["duration_ms"] == 120 and p["added"] == 3 and "ms" not in p
    assert p["$process_person_profile"] is False and p["$geoip_disable"] is True
    assert re.fullmatch(r"\d+\.\d+\.\d+", p["app_version"]) and p["os"] in ("darwin", "linux", "other") and p["arch"] in ("arm64", "x64", "other") and p["runtime"] == "python"
    assert batch[0]["distinct_id"] == telemetry.install_id() and len(batch[0]["distinct_id"]) == 32
    assert "count" not in batch[1]["properties"] and batch[1]["properties"]["ok"] is False and batch[1]["properties"]["error"] == "other"
    assert "command" not in batch[1]["properties"], "a command the parser does not know never leaves"


def test_a_canary_path_host_or_user_name_never_leaves(fake, home):
    import getpass
    import socket

    _log({"event": "cli.run", "command": "start", "status": "ok", "error": socket.gethostname(), "source_kind": getpass.getuser(), "story": str(home)})
    cli.main(["work", "task", f"Prepare {CANARY} offer", "--hours", "1"])
    telemetry.send()
    sent = json.dumps(fake.bodies)
    assert fake.bodies and CANARY not in sent and str(home) not in sent
    assert socket.gethostname() not in sent and f'"{getpass.getuser()}"' not in sent


def test_every_emitted_event_name_is_on_the_fixed_list():
    names = set()
    for f in (ROOT / "src" / "careerbrew").rglob("*.py"):
        names |= set(re.findall(r'events\.emit\(\s*"([^"]+)"', f.read_text()))
    assert names and not names - telemetry.EVENT_NAMES, f"add to telemetry.EVENT_NAMES: {sorted(names - telemetry.EVENT_NAMES)}"


def test_command_values_come_from_the_cli_parser():
    names = telemetry.command_names()
    assert {"day.log", "telemetry.send", "review"} <= names and "nope.nope" not in names
    assert telemetry.USER_COMMANDS <= names | {"start"}


# --- sending ------------------------------------------------------------------------------------------------


def test_nothing_is_sent_twice_and_a_partial_line_waits(fake, home, monkeypatch):
    _log({"event": "cli.run", "command": "start", "status": "ok"})
    with events.log_path().open("a") as fh:
        fh.write(json.dumps({"ts": _ts(5), "event": "cli.run"})[:-1])  # still being written
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")  # keep the send's own event out of this count
    assert telemetry.send()["sent"] == 1
    assert telemetry.send()["sent"] == 0
    with events.log_path().open("a") as fh:
        fh.write(', "command": "review"}\n')
    assert telemetry.send()["sent"] == 1
    assert [e["properties"]["command"] for e in fake.events()] == ["start", "review"]


def test_batches_of_500_and_the_backlog_cap(fake, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")
    _log(*[{"event": "cli.run", "command": "start", "n": i} for i in range(telemetry.BACKLOG_EVENTS + 100)])
    assert telemetry.send()["sent"] == telemetry.BACKLOG_EVENTS
    sizes = [len(b["json"]["batch"]) for b in fake.bodies]
    assert max(sizes) == telemetry.BATCH_SIZE and sum(sizes) == telemetry.BACKLOG_EVENTS
    assert fake.events()[0]["properties"]["n"] == 100, "the newest events are kept"


def test_a_second_send_waits_for_the_lock(fake, home):
    _log({"event": "cli.run", "command": "start"})
    with telemetry._lock() as held:
        assert held
        assert telemetry.send() == {"sent": 0, "status": "off", "why": "another send is running"}
    assert fake.bodies == []


def test_a_failed_batch_keeps_the_cursor(fake, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "off")
    _log({"event": "cli.run", "command": "start"})
    fake.status = 500
    assert telemetry.send()["status"] == "fail"
    fake.status = 200
    assert telemetry.send()["sent"] == 1


def test_no_key_sends_nothing(fake, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", "")
    _log({"event": "cli.run"})
    assert telemetry.send()["status"] == "off" and fake.bodies == []


@pytest.mark.parametrize("switch", ["dnt", "ci", "env0", "off"])
def test_off_switches_send_nothing(fake, home, monkeypatch, switch):
    if switch == "dnt":
        monkeypatch.setenv("DO_NOT_TRACK", "1")
    elif switch == "ci":
        monkeypatch.setenv("CI", "")  # any value, even an empty one
    elif switch == "env0":
        monkeypatch.setenv("CAREERBREW_TELEMETRY", "0")
    else:
        assert cli.main(["telemetry", "off"]) == 0
    _log({"event": "cli.run"})
    assert telemetry.send()["status"] == "off"
    assert telemetry.maybe_send() is None
    assert fake.bodies == []


def test_do_not_track_0_does_not_stop_it(fake, home, monkeypatch):
    monkeypatch.setenv("DO_NOT_TRACK", "0")
    assert telemetry.enabled() == (True, "on")


def test_a_failed_send_never_raises_and_keeps_the_events(fake, home, capsys):
    fake.status = 500
    _log({"event": "cli.run", "command": "start"})
    assert telemetry.send()["status"] == "fail"
    assert cli.main(["telemetry", "send"]) == 1
    assert "stay queued" in capsys.readouterr().out
    fake.status = 200
    assert telemetry.send()["sent"] >= 1


def test_an_unreachable_endpoint_never_fails_the_watcher(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_POSTHOG_KEY", FAKE_KEY)
    _switches_off(monkeypatch)
    telemetry.set_consent("yes")
    _log({"event": "cli.run"})
    assert telemetry.maybe_send()["status"] == "fail"  # 127.0.0.1:9 refuses
    assert telemetry.maybe_send() is None, "a dead network costs one try a day"


def test_the_watcher_sends_once_a_day_with_a_short_timeout(fake, home, monkeypatch):
    seen: list[float] = []
    real = telemetry._post
    monkeypatch.setattr(telemetry, "_post", lambda payload, timeout=10: seen.append(timeout) or real(payload, timeout))
    _log({"event": "cli.run"})
    assert telemetry.maybe_send()["sent"] >= 1
    assert seen and all(0 < t <= telemetry.AUTO_TIMEOUT_S for t in seen), "the auto send has 3 s in all"
    _log({"event": "cli.run"})
    assert telemetry.maybe_send() is None, "once a day"


def test_dry_run_prints_the_exact_body_without_the_key_and_sends_nothing(fake, home, capsys):
    _log({"event": "cli.run", "command": "day.log", "status": "ok", "ms": 5})
    assert cli.main(["telemetry", "send", "--dry-run"]) == 0
    out = capsys.readouterr().out
    assert FAKE_KEY not in out and '"<posthog_key>"' in out and '"day.log"' in out
    assert fake.bodies == []


def test_dry_run_json_prints_only_the_json(fake, home, capsys):
    _log({"event": "cli.run", "command": "day.log", "status": "ok"})
    assert cli.main(["telemetry", "send", "--dry-run", "--json"]) == 0
    got = json.loads(capsys.readouterr().out)
    assert got["body"]["api_key"] == "<posthog_key>" and got["body"]["batch"][0]["event"] == "cli.run"


def test_status_on_off(endpoint, home, capsys):
    assert cli.main(["telemetry", "off"]) == 0
    cli.main(["telemetry", "status"])
    out = capsys.readouterr().out
    assert "telemetry: off" in out and "consent: no" in out
    cli.main(["telemetry", "on"])
    cli.main(["telemetry", "status"])
    out = capsys.readouterr().out
    assert "telemetry: on (on)" in out and "consent: yes since" in out and "key: env" in out


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
    _log({"event": "cli.run"})
    ok, why = telemetry.enabled()
    assert not ok and "not a valid address" in why
    assert telemetry.send()["status"] == "off" and telemetry.maybe_send() is None


def test_a_host_policy_without_the_telemetry_host_sends_nothing(fake, home, monkeypatch):
    """The telemetry post obeys the plugin `allowed_hosts` policy, like the connector calls."""
    calls: list[str] = []
    monkeypatch.setattr(telemetry.connectors, "open_url", lambda req, **kw: calls.append(req.full_url))
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", "https://telemetry.example.net")
    monkeypatch.setattr(telemetry.connectors.plugins, "effective_policy", lambda: {"allowed_hosts": ["jira.example.com"]})
    _log({"event": "cli.run"})
    ok, why = telemetry.enabled()
    assert not ok and "allowed_hosts" in why
    assert telemetry.send()["status"] == "off"
    assert telemetry.maybe_send() is None
    with pytest.raises(telemetry.connectors.ConnectorError, match="allowed_hosts"):
        telemetry._post({"api_key": FAKE_KEY, "batch": []})
    assert calls == []
    monkeypatch.setattr(telemetry.connectors.plugins, "effective_policy", lambda: {"allowed_hosts": ["example.net"]})
    assert telemetry.enabled() == (True, "on")


def test_cli_runs_log_their_duration(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    cli.main(["telemetry", "status"])
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    assert isinstance([r for r in rows if r["event"] == "cli.run"][-1]["ms"], int)


# --- outcome events (task telemetry-outcome-events) ----------------------------------------------------------


def _install_events(fake) -> list[dict]:
    telemetry.send()
    return [e for e in fake.events() if e["event"] == "careerbrew.install"]


def _started(ms_ago: int = 4000) -> int:
    return int(events.utc_now().timestamp() * 1000) - ms_ago


def test_setup_alone_writes_no_event_line(endpoint, home):
    assert cli.main(["telemetry", "record-install", "--started", str(_started()), "--ok", "true"]) == 0
    text = events.log_path().read_text() if events.log_path().exists() else ""
    assert "careerbrew.install" not in text
    assert telemetry._state()["install"]["ok"] is True and 3000 <= telemetry._state()["install"]["duration_ms"] <= 60_000


def test_a_yes_after_setup_emits_the_install_event_once_stamped_after_the_yes(endpoint, home):
    telemetry.record_install(_started(), True)
    telemetry.set_consent("yes")
    telemetry.set_consent("yes")
    got = _install_events(endpoint)
    assert len(got) == 1
    p = got[0]["properties"]
    assert p["ok"] is True and 3000 <= p["duration_ms"] <= 60_000 and p["tier"] == "personal"
    assert got[0]["timestamp"] >= telemetry._state()["telemetry_consent_time"]


def test_a_second_setup_of_the_same_data_folder_emits_none(endpoint, home):
    telemetry.record_install(_started(), True)
    telemetry.set_consent("yes")
    telemetry.record_install(_started(100), True)
    telemetry.record_install(_started(100), False)
    assert len(_install_events(endpoint)) == 1


def test_a_failed_setup_then_a_yes_emits_ok_false(endpoint, home):
    telemetry.record_install(_started(), False)
    telemetry.set_consent("yes")
    got = _install_events(endpoint)
    assert len(got) == 1 and got[0]["properties"]["ok"] is False


def test_a_failure_after_a_success_changes_nothing(endpoint, home):
    telemetry.record_install(_started(), True)
    telemetry.record_install(_started(), False)
    assert telemetry._state()["install"]["ok"] is True


@pytest.mark.parametrize("switch", [("CI", "true"), ("DO_NOT_TRACK", "1"), ("CAREERBREW_EVENTS", "off")])
def test_record_install_does_nothing_in_ci_or_tests(endpoint, home, monkeypatch, switch):
    monkeypatch.setenv(*switch)
    assert telemetry.record_install(_started(), True) is None
    assert "install" not in telemetry._state()


def test_with_consent_unset_neither_outcome_event_leaves(endpoint, home):
    telemetry.record_install(_started(), True)
    assert telemetry.maybe_send() is None
    telemetry.send()
    assert endpoint.bodies == []
    assert telemetry.dry_run()["body"]["batch"] == []


def test_the_week_event_carries_only_numbers_and_no_planted_name(fake, home, monkeypatch):
    from careerbrew.work import evidence

    monkeypatch.setattr(telemetry, "_week_bounds", lambda today: (today - timedelta(days=3), today + timedelta(days=4), "2026-W46"))
    monkeypatch.setattr(evidence, "load", lambda: [{"at": events.stamp(), "title": f"Shipped {CANARY} launch", "project": "PROJECTZEBRA"}])
    _log({"event": "cli.run", "command": "day.log", "status": "ok"})
    assert telemetry.maybe_send()["status"] == "ok"
    week = [e for e in fake.events() if e["event"] == "careerbrew.week"]
    assert len(week) == 1
    props = {k: v for k, v in week[0]["properties"].items() if not k.startswith("$") and k not in ("app_version", "os", "arch", "runtime", "story")}
    assert props["evidence_items"] == 1 and props["days_active"] >= 1
    assert set(props) <= {"days_active", "cards_reviewed", "exercises_solved", "work_days_started", "evidence_items"}
    assert all(isinstance(v, int) and not isinstance(v, bool) for v in props.values())
    sent = json.dumps(fake.bodies)
    assert CANARY not in sent and "PROJECTZEBRA" not in sent and "Shipped" not in sent


def test_the_week_event_is_recorded_once_even_when_the_send_fails(fake, home, monkeypatch):
    monkeypatch.setattr(telemetry, "_week_bounds", lambda today: (today - timedelta(days=3), today + timedelta(days=4), "2026-W46"))
    fake.status = 500
    assert telemetry.maybe_send()["status"] == "fail"
    telemetry._patch(last_attempt_day="2000-01-01")
    fake.bodies.clear()  # the fake keeps the refused body too
    fake.status = 200
    assert telemetry.maybe_send()["status"] == "ok"
    assert sum(e["event"] == "careerbrew.week" for e in fake.events()) == 1


def test_a_week_that_ended_before_the_yes_is_never_recorded(fake, home):
    telemetry.maybe_send()  # the real last finished week ended before today's yes
    assert not any(e["event"] == "careerbrew.week" for e in fake.events())


def _trap_script(tmp_path: Path, path_dirs: str) -> subprocess.CompletedProcess:
    # Run outside the checkout: its own .venv/bin/careerbrew is the fallback command.
    tmp_path = tmp_path / "trap"
    tmp_path.mkdir(exist_ok=True)
    script = tmp_path / "s.sh"
    script.write_text(f'set -euo pipefail\nsource "{ROOT}/scripts/telemetry-install.sh"\ntelemetry_install_begin\nexit 7\n')
    return subprocess.run(["/bin/bash", str(script)], env={"PATH": path_dirs, "HOME": str(tmp_path)}, cwd=tmp_path, capture_output=True, text=True, check=False)


def test_the_setup_trap_with_no_careerbrew_writes_nothing_and_keeps_the_exit_code(tmp_path):
    got = _trap_script(tmp_path, "/usr/bin:/bin")
    assert got.returncode == 7 and got.stderr == "" and list((tmp_path / "trap").iterdir()) == [tmp_path / "trap" / "s.sh"]


def test_the_setup_trap_records_a_failure_with_careerbrew(tmp_path):
    bin_dir = tmp_path / "trap" / ".venv" / "bin"  # the checkout's own command, never a launcher on PATH
    bin_dir.mkdir(parents=True)
    (bin_dir / "careerbrew").write_text(f'#!/bin/sh\necho "$@" > "{tmp_path}/called"\nexit 3\n')
    (bin_dir / "careerbrew").chmod(0o755)
    got = _trap_script(tmp_path, "/usr/bin:/bin")
    assert got.returncode == 7, "a failing careerbrew never hides setup's own exit code"
    assert re.fullmatch(r"telemetry record-install --started \d{13} --ok false\n", (tmp_path / "called").read_text())


def test_the_setup_trap_never_uses_a_careerbrew_launcher_on_path(tmp_path):
    # A launcher on PATH can belong to another checkout and write that checkout's data.
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    (bin_dir / "careerbrew").write_text(f'#!/bin/sh\necho "$@" > "{tmp_path}/called"\n')
    (bin_dir / "careerbrew").chmod(0o755)
    got = _trap_script(tmp_path, f"{bin_dir}:/usr/bin:/bin")
    assert got.returncode == 7 and not (tmp_path / "called").exists()


# --- review fixes: funnel stages, the send after a command, the question lock, new events --------------------------


def test_every_job_funnel_stage_leaves_as_its_status(home):
    from careerbrew.jobs import strategy

    for stage in strategy.STAGES:
        ev = telemetry.to_event({"event": "jobs.application", "ts": _ts(), "tier": "target", "status": stage}, "id")
        assert ev and ev["properties"]["status"] == stage, stage


def test_an_ordinary_command_sends_once_a_day_with_the_short_timeout(fake, home, monkeypatch, capsys):
    seen: list[float] = []
    real = telemetry._post
    monkeypatch.setattr(telemetry, "_post", lambda payload, timeout=10: seen.append(timeout) or real(payload, timeout))
    _log({"event": "cli.run", "command": "review", "status": "ok"})
    assert cli.main(["config"]) == 0
    assert fake.events() and seen and max(seen) <= telemetry.AUTO_TIMEOUT_S
    n = len(fake.bodies)
    assert cli.main(["config"]) == 0
    assert len(fake.bodies) == n, "once a day"


def test_a_telemetry_command_never_sends_by_itself(fake, home, capsys):
    _log({"event": "cli.run", "command": "review", "status": "ok"})
    assert cli.main(["telemetry", "status"]) == 0
    assert fake.bodies == [] and not telemetry._state().get("last_attempt_day")


def test_the_whole_auto_send_ends_within_its_timeout(fake, home, monkeypatch):
    """Three batches, each post 2 s on a fake clock: the auto send stops at 3 s in all and keeps the rest for later."""
    clock = [100.0]
    seen: list[float] = []

    def slow(payload, timeout=10):
        seen.append(timeout)
        clock[0] += 2

    monkeypatch.setattr(telemetry, "_clock", lambda: clock[0])
    monkeypatch.setattr(telemetry, "_post", slow)
    _log(*({"event": "cli.run", "command": "review", "status": "ok"} for _ in range(1200)))
    got = telemetry.maybe_send()
    assert seen == [telemetry.AUTO_TIMEOUT_S, 1] and got["sent"] == 2 * telemetry.BATCH_SIZE
    assert len(telemetry.pending()[0]) >= 200, "what did not fit waits for the next send"


def test_two_terminals_never_both_ask(endpoint, home):
    import fcntl

    from careerbrew import paths

    shown: list[str] = []
    with paths.sub("telemetry-ask.lock").open("a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)  # another terminal is asking right now
        assert telemetry.ask_on_start(interactive=True, read=lambda: "y\n", write=shown.append) is None
        assert shown == [] and not telemetry._state().get("ask_count")
    assert telemetry.ask_on_start(interactive=True, read=lambda: "y\n", write=shown.append) == "yes"


def test_the_ask_lock_is_bookkeeping_the_web_app_ignores():
    from careerbrew import server

    assert "telemetry-ask.lock" in server.CHANGE_SKIP


def test_every_view_name_is_a_real_route_of_the_web_app():
    from careerbrew import server

    src = Path(server.__file__).read_text()
    known = {server._route(p) for p in re.findall(r'"(/api/[a-z/-]+)": ', src)} | {"library.page", "daylog.day", "interviews.company", "reports.page"}
    assert telemetry.VIEW_ROUTES <= known, telemetry.VIEW_ROUTES - known


def test_a_web_view_leaves_only_its_fixed_name_once_a_day(fake, home, monkeypatch):
    from careerbrew import server

    monkeypatch.setattr(telemetry, "_views_seen", set())
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        for path in ("/api/overview", "/api/overview", f"/api/interviews/{CANARY}", "/api/changes"):
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}"})
            with urllib.request.urlopen(req, timeout=10):
                pass
    finally:
        srv.shutdown()
        srv.server_close()
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    views = [r["route"] for r in rows if r["event"] == "web.view"]
    assert sorted(views) == ["interviews.company", "overview"], views
    telemetry.send()
    sent = [e for e in fake.events() if e["event"] == "web.view"]
    assert sorted(e["properties"]["route"] for e in sent) == ["interviews.company", "overview"]
    assert CANARY not in json.dumps(fake.bodies)


def test_a_route_off_the_view_list_never_leaves(home):
    ev = telemetry.to_event({"event": "web.view", "ts": _ts(), "route": "made.up"}, "id")
    assert ev and "route" not in ev["properties"]


def test_a_work_day_step_leaves_its_kind_and_seconds(fake, home, monkeypatch):
    from careerbrew.workday import status

    names = ["/careerbrew", "/standup"]
    for step in names:
        status.update(step, names, "running")
        status.update(step, names, "done")
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    done = [r for r in rows if r["event"] == "workday.update" and r["status"] == "done"]
    assert [r["step_kind"] for r in done] == ["builtin", "workflow"]
    assert all(isinstance(r["seconds"], int) for r in done)
    telemetry.send()
    sent = [e["properties"] for e in fake.events() if e["event"] == "workday.update" and e["properties"]["status"] == "done"]
    assert sorted(p["step_kind"] for p in sent) == ["builtin", "workflow"] and all("seconds" in p for p in sent)
    assert "/standup" not in json.dumps(fake.bodies), "a step name is content and never leaves"


def test_a_new_version_emits_one_upgrade_event(fake, home, monkeypatch):
    monkeypatch.setattr(telemetry, "__version__", "0.2.0")
    telemetry.maybe_send()  # the first version seen is the baseline: no event
    telemetry._patch(last_attempt_day=None)
    monkeypatch.setattr(telemetry, "__version__", "0.3.0")
    telemetry.maybe_send()
    telemetry._patch(last_attempt_day=None)
    telemetry.maybe_send()
    ups = [e for e in fake.events() if e["event"] == "careerbrew.upgrade"]
    assert len(ups) == 1 and ups[0]["properties"]["from_version"] == "0.2.0" and ups[0]["properties"]["app_version"] == "0.3.0"


def test_a_from_version_that_is_not_a_version_never_leaves(home):
    ev = telemetry.to_event({"event": "careerbrew.upgrade", "ts": _ts(), "from_version": "my-branch"}, "id")
    assert ev and "from_version" not in ev["properties"]


# --- command fields of cli.run and cli.error (bare `careerbrew`, error classes) ----------------------------------


def test_a_bare_careerbrew_run_counts_as_the_start_command(fake, home, monkeypatch):
    monkeypatch.setattr(cli, "cmd_start", lambda a: 0)
    cli.main([])
    telemetry.send()
    runs = [e["properties"].get("command") for e in fake.events() if e["event"] == "cli.run"]
    assert "start" in runs  # the default command is the one a person runs most: it must count as active


def test_a_failed_command_leaves_its_command_and_error_class(fake, home, monkeypatch):
    def boom(a):
        raise RuntimeError("private-canary-text")

    monkeypatch.setattr(cli, "cmd_start", boom)
    assert cli.main([]) == 2
    telemetry.send()
    errors = [e["properties"] for e in fake.events() if e["event"] == "cli.error"]
    assert errors and errors[-1].get("command") == "start" and errors[-1].get("error") == "runtimeerror"
    assert "private-canary-text" not in json.dumps(fake.bodies)


# --- /work and web UI coverage (the owner's ask, 2026-11-15) -------------------------------------------------


@pytest.mark.parametrize(("env", "via"), [({}, "cli"), ({"CLAUDECODE": "1"}, "agent"), ({"CODEX_SANDBOX": "seatbelt"}, "agent")])
def test_a_command_says_where_it_ran(fake, home, monkeypatch, env, via):
    for name in ("CLAUDECODE", "CODEX_SANDBOX"):
        monkeypatch.delenv(name, raising=False)  # this test may itself run under an agent
    for k, v in env.items():
        monkeypatch.setenv(k, v)
    monkeypatch.setattr(cli, "cmd_start", lambda a: 0)
    cli.main([])
    telemetry.send()
    runs = [e["properties"] for e in fake.events() if e["event"] == "cli.run" and e["properties"].get("command") == "start"]
    assert runs and runs[-1].get("via") == via


def test_an_action_run_from_the_web_app_says_ui(fake, home, monkeypatch):
    from careerbrew import actions

    monkeypatch.delenv("CLAUDECODE", raising=False)
    monkeypatch.delenv("CODEX_SANDBOX", raising=False)
    actions.run("telemetry.status", {})
    telemetry.send()
    runs = [e["properties"] for e in fake.events() if e["event"] == "cli.run" and e["properties"].get("command") == "telemetry.status"]
    assert runs and runs[-1].get("via") == "ui"


def test_a_work_day_step_failure_leaves_its_class_and_a_session_its_action(fake, home):
    _log(
        {"event": "workday.update", "status": "failed", "error_class": "auth", "step_kind": "workflow", "seconds": 12},
        {"event": "workday.update", "status": "failed", "error_class": CANARY.lower()[:40], "step_kind": "workflow"},
        {"event": "work.session", "action": "handoff", "status": "ok"},
        {"event": "work.session", "action": "made-up", "status": "ok"},
    )
    telemetry.send()
    ups = [e["properties"] for e in fake.events() if e["event"] == "workday.update"]
    assert [u.get("error_class") for u in ups] == ["auth", None]  # only the five fixed classes leave
    sess = [e["properties"] for e in fake.events() if e["event"] == "work.session"]
    assert [s.get("action") for s in sess] == ["handoff", None]
    assert CANARY.lower()[:40] not in json.dumps(fake.bodies)


def test_a_failed_step_keeps_its_class_in_the_local_event_log(home, monkeypatch):
    from careerbrew.workday import status

    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    status.update("lint", ["/lint"], status="failed", error_class="network")
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    assert [r.get("error_class") for r in rows if r["event"] == "workday.update"] == ["network"]


def test_the_end_of_a_work_day_counts_its_step_outcomes(fake, home):
    _log({"event": "workday.end", "status": "ended", "action": "end", "steps_done": 4, "steps_blocked": 1, "steps_failed": 1, "steps_waiting": 2, "needs_you": 1})
    telemetry.send()
    end = next(e["properties"] for e in fake.events() if e["event"] == "workday.end")
    assert (end["action"], end["steps_done"], end["steps_blocked"], end["steps_failed"], end["steps_waiting"], end["needs_you"]) == ("end", 4, 1, 1, 2, 1)


def test_step_outcomes_count_each_status_and_the_steps_that_need_the_owner(home, monkeypatch):
    from careerbrew.workday import end, status

    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    rows = [{**status.blank("a"), "status": "done"}, {**status.blank("b"), "status": "blocked", "needs_you": "sign in"}, {**status.blank("c"), "status": "failed"}]
    assert end.step_outcomes(rows) == {"steps_done": 1, "steps_blocked": 1, "steps_failed": 1, "steps_waiting": 0, "steps_running": 0, "needs_you": 1}


@pytest.mark.parametrize(
    ("path", "route"),
    [
        (f"/api/inbox/{CANARY}/accept", "inbox.accept"),
        (f"/api/inbox-group/{CANARY}/dismiss", "inbox-group.dismiss"),
        (f"/api/notifications/{CANARY}/snooze", "notifications.snooze"),
        (f"/api/promises/{CANARY}/done", "promises.done"),
        (f"/api/approvals/{CANARY}/decide", "approvals.decide"),
        (f"/api/tasks/{CANARY}/done", "tasks.done"),
        (f"/api/cards/{CANARY}/review", "cards.review"),
        (f"/api/explanations/{CANARY}/flag", "explanations.flag"),
        ("/api/actions/day.log", "actions.run"),
        ("/api/watch/pause", "watch.pause"),
        ("/api/pm/send", "pm.send"),
        ("/api/goals", "goals"),
        ("/api/jobs/choose", "jobs.choose"),
    ],
)
def test_a_web_control_is_named_by_a_fixed_route_never_by_its_id(path, route):
    from careerbrew import server

    assert server._act_route(path) == route
    assert route in telemetry.WEB_ACTS


def test_every_data_changing_web_route_is_on_the_act_list():
    """A new POST route in server.post must get a fixed act name, or its clicks are never counted."""
    from careerbrew import server

    src = (ROOT / "src" / "careerbrew" / "server.py").read_text()
    body = src.split("\ndef post(", 1)[1].split("\ndef ", 1)[0]
    fixed = set(re.findall(r'path == "(/api/[a-z/_-]+)"', body)) | {p for pair in re.findall(r'path in \(("[^)]+)\)', body) for p in re.findall(r'"(/api/[^"]+)"', pair)}
    for p in fixed - {"/api/timing"}:  # a timing is the page's own beacon, not a click
        assert server._act_route(p) in telemetry.WEB_ACTS, p


def test_a_web_click_leaves_one_act_with_its_route_and_status_never_its_id(fake, home, monkeypatch):
    from careerbrew import server

    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        for path in (f"/api/promises/{CANARY}/done", "/api/watch/nope"):
            req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=b"{}", headers={"X-Careerbrew-Token": "tok", "Host": f"127.0.0.1:{port}", "Content-Type": "application/json"})
            try:
                urllib.request.urlopen(req, timeout=10).close()
            except urllib.error.HTTPError:
                pass
    finally:
        srv.shutdown()
        srv.server_close()
    telemetry.send()
    acts = [e["properties"] for e in fake.events() if e["event"] == "web.act"]
    assert [(a.get("route"), a.get("status")) for a in acts] == [("promises.done", "refused")], "an unknown route logs nothing"
    assert all("duration_ms" in a for a in acts)
    assert CANARY not in json.dumps(fake.bodies)


def test_the_step_error_list_is_the_work_day_gates_list():
    from careerbrew.workday import gate

    assert telemetry.STEP_ERRORS == set(gate.ERROR_CLASSES)


def test_every_action_careerbrew_emits_may_leave():
    """A new `action="<verb>"` in an event must join telemetry.ACTIONS, or its dimension silently drops."""
    found = set()
    for f in (ROOT / "src" / "careerbrew").rglob("*.py"):
        for call in re.findall(r"events\.emit\((.*?)\)\n", f.read_text(), flags=re.S):
            found |= set(re.findall(r'\baction="([a-z_-]+)"', call))
    assert found and found <= telemetry.ACTIONS, sorted(found - telemetry.ACTIONS)


def test_an_error_class_in_camel_case_is_kept_lowercase():
    """23 emits pass `error=type(exc).__name__` (`ValueError`); the event log keeps it as a lowercase id."""
    assert events.clean({"error": "ValueError"}) == {"error": "valueerror"}
    assert events.clean({"error": "Not A Class"}) == {}


def test_a_request_the_guard_refuses_is_not_a_click(home, monkeypatch):
    """A foreign page can POST to the local app; with no token it is refused and must not count as a control click."""
    from careerbrew import server

    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]
    try:
        for token in ("", "wrong"):
            req = urllib.request.Request(f"http://127.0.0.1:{port}/api/promises/x/done", data=b"{}", headers={"X-Careerbrew-Token": token, "Host": f"127.0.0.1:{port}", "Content-Type": "application/json"})
            with pytest.raises(urllib.error.HTTPError):
                urllib.request.urlopen(req, timeout=10)
    finally:
        srv.shutdown()
        srv.server_close()
    log = events.log_path()
    rows = [json.loads(x) for x in log.read_text().splitlines()] if log.exists() else []
    assert not [r for r in rows if r["event"] == "web.act"]


def test_day_end_sends_one_count_per_planned_step_including_the_ones_never_started(home, monkeypatch):
    from careerbrew import config
    from careerbrew.workday import end, status

    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-29T17:40+00:00")
    cli.main(["init"])
    config.set_value("run_pages", "false")
    config.set_value("work_steps", "/alpha, /beta, /gamma")
    status.update("/alpha", ["/alpha", "/beta", "/gamma"], status="done")
    end.finish()
    rows = [json.loads(x) for x in events.log_path().read_text().splitlines()]
    got = next(r for r in rows if r["event"] == "workday.end" and r.get("action") == "end")
    from careerbrew.workday import plan

    planned = [x["step"] for x in plan.build()["steps"]]
    assert {"/alpha", "/beta", "/gamma"} <= set(planned)
    assert (got["steps_done"], got["steps_waiting"], got["steps_failed"], got["steps_blocked"]) == (1, len(planned) - 1, 0, 0), got
