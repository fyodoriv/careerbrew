"""Issue #199: Outlook signs in with a device code, refreshes its token before each read, and asks to reconnect when Microsoft ends the sign-in."""

import json
from datetime import date

import pytest

from careerbrew import calendar, connectors, msauth, store
from careerbrew.cli import main

from .conftest import FIXTURES

SIGNED_IN = {"kind": "outlook", "auth": "device", "client_id": "synthetic-client", "tenant": "organizations", "token": "keychain:careerbrew.outlook.refresh"}


@pytest.fixture
def http(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:00+00:00")
    monkeypatch.setenv("CAREERBREW_TEST_OWNER", "1")  # stands in for your own terminal; the sign-in code prints nowhere else
    msauth._ACCESS.clear()
    keychain: dict[str, str] = {"careerbrew.outlook.refresh": "synthetic-refresh-token-1"}
    monkeypatch.setattr(msauth.secrets, "store_keychain", lambda svc, v: keychain.__setitem__(svc, v) or f"keychain:{svc}")
    real = msauth.secrets.resolve
    monkeypatch.setattr(msauth.secrets, "resolve", lambda ref: keychain[ref[9:]] if ref.startswith("keychain:") and ref[9:] in keychain else real(ref))
    return keychain


def _connect(home, conns):
    (home / "work").mkdir(parents=True, exist_ok=True)
    (home / "work" / "connectors.yaml").write_text(json.dumps({"connectors": conns}))


def test_connect_signs_in_with_a_device_code_and_keeps_only_the_refresh_token(http, home, capsys, monkeypatch):
    monkeypatch.setattr(msauth.time, "sleep", lambda _s: None)
    rc = main(["connect", "outlook", "--client-id", "synthetic-client"])
    out = capsys.readouterr().out
    assert rc == 0, out
    assert "Open https://microsoft.com/devicelogin and enter the code SYNTH123" in out
    assert "synthetic-access-token" not in out and "synthetic-refresh-token" not in out
    (c,) = store.read_yaml(home / "work" / "connectors.yaml")["connectors"]
    assert c == {"kind": "outlook", "auth": "device", "client_id": "synthetic-client", "tenant": "organizations", "token": "keychain:careerbrew.outlook.refresh"}
    assert http["careerbrew.outlook.refresh"] == "synthetic-refresh-token-2"
    assert not any(b"synthetic-refresh" in p.read_bytes() or b"synthetic-access" in p.read_bytes() for p in home.rglob("*") if p.is_file())


def test_connect_without_a_client_id_names_the_setting_and_the_manual_token(http, capsys):
    rc = main(["connect", "outlook"])
    out = capsys.readouterr().out
    assert rc == 2 and "careerbrew config set outlook_client_id" in out and "expires after about an hour" in out and "--paste" in out


def test_a_plugins_connector_defaults_give_the_client_id_and_tenant(http, monkeypatch):
    monkeypatch.setattr(connectors, "plugin_defaults", lambda kind: {"client_id": "plugin-client", "tenant": "plugin-tenant"} if kind == "outlook" else {})
    assert msauth.settings() == ("plugin-client", "plugin-tenant")
    assert msauth.settings("flag-client") == ("flag-client", "plugin-tenant")
    main(["config", "set", "outlook_client_id", "my-client"])
    assert msauth.settings() == ("my-client", "plugin-tenant")


def test_a_declined_sign_in_saves_nothing(http, home, capsys):
    rc = main(["connect", "outlook", "--client-id", "synthetic-client", "--tenant", "synthetic-declined-tenant"])
    assert rc == 1 and "you declined the sign-in" in capsys.readouterr().out
    assert not (home / "work" / "connectors.yaml").exists()


def test_poll_waits_while_pending_and_slows_down_when_asked(monkeypatch):
    answers = iter([{"error": "authorization_pending"}, {"error": "slow_down"}, {"access_token": "a", "refresh_token": "r"}])
    monkeypatch.setattr(msauth, "http_form", lambda url, fields: next(answers))
    slept: list[float] = []
    ans = msauth.poll("c", "organizations", {"device_code": "d", "interval": 5, "expires_in": 900}, sleep=slept.append, clock=lambda: 0.0)
    assert ans["refresh_token"] == "r" and slept == [5.0, 10.0]


def test_poll_stops_when_the_code_expires(monkeypatch):
    monkeypatch.setattr(msauth, "http_form", lambda url, fields: {"error": "authorization_pending"})
    now = iter([0.0, 0.0, 600.0, 901.0])
    with pytest.raises(connectors.ConnectorError, match="the code expired"):
        msauth.poll("c", "organizations", {"device_code": "d", "interval": 5, "expires_in": 900}, sleep=lambda _s: None, clock=lambda: next(now))


def test_each_read_refreshes_the_token_once_and_keeps_the_rotated_one(http, home, monkeypatch):
    calls: list[str] = []
    real = msauth.http_form
    monkeypatch.setattr(msauth, "http_form", lambda url, fields: calls.append(fields["grant_type"]) or real(url, fields))
    _connect(home, [SIGNED_IN])
    evs, errors = calendar.events_for(date(2026, 10, 19))
    assert errors == [] and [e.title for e in evs] == ["Focus time", "Design review"]
    assert connectors.run(SIGNED_IN, "user")  # a second read in the same process reuses the access token
    assert calls == ["refresh_token"]
    assert http["careerbrew.outlook.refresh"] == "synthetic-refresh-token-2"


def test_an_expired_sign_in_shows_one_reconnect_step_not_empty_data(http, home):
    expired = {**SIGNED_IN, "tenant": "synthetic-expired-tenant"}
    _connect(home, [expired])
    evs, errors = calendar.events_for(date(2026, 10, 19))
    assert evs == [] and errors == [msauth.RECONNECT]
    st = connectors.status({**expired, "origin": "user"})
    assert st["ready"] is False and st["steps"] == [msauth.RECONNECT + "."]
    with pytest.raises(msauth.ReconnectNeeded):
        connectors.feed(expired)


def test_a_new_sign_in_clears_the_reconnect_step(http, home, monkeypatch):
    _connect(home, [{**SIGNED_IN, "tenant": "synthetic-expired-tenant"}])
    calendar.events_for(date(2026, 10, 19))
    assert msauth.needs_reconnect()
    monkeypatch.setattr(msauth.time, "sleep", lambda _s: None)
    assert main(["connect", "outlook", "--client-id", "synthetic-client"]) == 0
    assert not msauth.needs_reconnect()
    assert connectors.status({**SIGNED_IN, "origin": "user"})["ready"] is True


def test_a_manual_token_still_works_with_paste_or_ref(http, home, monkeypatch, capsys):
    monkeypatch.setenv("CB_TEST_TOKEN", "test-token-not-real")
    assert main(["connect", "outlook", "--ref", "env:CB_TEST_TOKEN"]) == 0
    out = capsys.readouterr().out
    assert "expires after about an hour" in out
    (c,) = store.read_yaml(home / "work" / "connectors.yaml")["connectors"]
    assert "auth" not in c and c["token"] == "env:CB_TEST_TOKEN"


def test_the_web_app_refuses_the_device_sign_in(http, capsys):
    from careerbrew import actions

    tok = actions.OWNER_UI.set(True)
    try:
        assert main(["connect", "outlook", "--client-id", "synthetic-client"]) == 3
    finally:
        actions.OWNER_UI.reset(tok)
    assert "in your terminal" in capsys.readouterr().out


@pytest.mark.parametrize("code", ["unauthorized_client", "invalid_client", "invalid_scope"])
def test_a_refusal_reconnecting_cannot_fix_names_its_code_not_reconnect(http, monkeypatch, code):
    monkeypatch.setattr(msauth, "http_form", lambda url, fields: {"error": code})
    with pytest.raises(connectors.ConnectorError, match=code) as e:
        msauth.bearer(SIGNED_IN)
    assert not isinstance(e.value, msauth.ReconnectNeeded) and not msauth.needs_reconnect()


def test_a_non_json_answer_from_a_proxy_is_a_plain_connector_error(monkeypatch):
    import io
    import urllib.error

    monkeypatch.delenv("CAREERBREW_HTTP_FIXTURES", raising=False)

    def proxy(req, timeout):
        raise urllib.error.HTTPError(req.full_url, 401, "Unauthorized", {}, io.BytesIO(b"<html>sign in to the proxy</html>"))  # type: ignore[arg-type]

    monkeypatch.setattr(msauth, "open_url", proxy)
    with pytest.raises(connectors.ConnectorError, match="not JSON"):
        msauth.http_form(msauth._url("organizations", "token"), {})


def test_one_network_blip_while_waiting_for_the_code_keeps_waiting(monkeypatch):
    answers = iter([OSError("network down"), {"error": "authorization_pending"}, {"access_token": "a", "refresh_token": "r"}])

    def form(url, fields):
        a = next(answers)
        if isinstance(a, Exception):
            raise a
        return a

    monkeypatch.setattr(msauth, "http_form", form)
    slept: list[float] = []
    ans = msauth.poll("c", "organizations", {"device_code": "d", "interval": 5, "expires_in": 900}, sleep=slept.append, clock=lambda: 0.0)
    assert ans["refresh_token"] == "r" and slept == [5.0, 5.0]


def test_a_locked_keychain_keeps_the_fresh_access_token(http, monkeypatch):
    def locked(svc, v):
        raise msauth.secrets.SecretError("locked")

    monkeypatch.setattr(msauth.secrets, "store_keychain", locked)
    monkeypatch.setattr(msauth.secrets, "backend_of", lambda ref: "keychain")
    assert msauth.bearer(SIGNED_IN)


def test_a_network_failure_while_refreshing_is_a_plain_connector_error(http, monkeypatch):
    import urllib.error

    def down(url, fields):
        raise urllib.error.URLError("synthetic: no route to host")

    monkeypatch.setattr(msauth, "http_form", down)
    with pytest.raises(connectors.ConnectorError, match="network") as e:
        msauth.bearer(SIGNED_IN)
    assert not isinstance(e.value, msauth.ReconnectNeeded) and not msauth.needs_reconnect()


def test_the_sign_in_code_never_prints_to_a_pipe_or_a_log(http, home, capsys, monkeypatch):
    """docs/privacy.md: the device code prints to your terminal only. Captured output is not a terminal."""
    monkeypatch.delenv("CAREERBREW_TEST_OWNER")
    monkeypatch.setattr(msauth, "start", lambda *_a: pytest.fail("the sign-in must not start without a terminal"))
    rc = main(["connect", "outlook", "--client-id", "synthetic-client"])
    out = capsys.readouterr().out
    assert rc == 3 and "in your own terminal" in out and "SYNTH123" not in out
    assert not (home / "work" / "connectors.yaml").exists()
