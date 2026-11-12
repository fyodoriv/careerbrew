"""Issue #141: the web token stays across restarts, so open tabs and bookmarks keep working.

The token lives in `day/web-token.json` (encrypted at rest) and is reused by every start until
`careerbrew web rotate-token` or `web_token_max_days`. A tab that has it gets an HttpOnly,
SameSite=Strict cookie for 127.0.0.1, so a reload, a new tab, or a bookmark signs in again; every
API route still needs the token header. The token never reaches the logs, an agent's stdout, or any
URL but the one-time open link.

The restart test starts real servers on a free port from the OS (never 8765) and stops every one
it started in teardown; the rest run a server in this process.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
import urllib.error
import urllib.request
from types import SimpleNamespace

import pytest

from careerbrew import actions, cli, config, server, store, vault

from .test_issue_118 import _cmdline, _free_port, _listeners, _status

needs_lsof = pytest.mark.skipif(not shutil.which("lsof") or not shutil.which("ps"), reason="needs lsof and ps to find the server on a port")


def _token_of(url: str) -> str:
    return url.rsplit("token=", 1)[1]


def _wait(cond, seconds: float = 10) -> None:
    deadline = time.monotonic() + seconds
    while not cond():
        assert time.monotonic() < deadline, "timed out"
        time.sleep(0.05)


@pytest.fixture
def served(home):
    """A free port set as `serve_port`; teardown stops every careerbrew server left on it or in the web state."""
    p = _free_port()
    config.set_value("serve_port", str(p))
    started: set[int] = set()
    yield p, started
    st = store.read_json(server.web_state_path(), {}) or {}
    for pid in started | _listeners(p) | ({int(st["pid"])} if st.get("pid") else set()):
        if "careerbrew serve" in _cmdline(pid):  # never a process that is not a careerbrew server
            os.kill(pid, signal.SIGKILL)


@pytest.fixture
def web(home, monkeypatch):
    """A server in this process that reads the stable token from the store (no pinned token)."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T09:00")
    monkeypatch.delenv("CAREERBREW_SERVE_TOKEN", raising=False)
    srv, _t = server.serve_in_thread(0)
    port = srv.server_address[1]

    def call(path, token=None, cookie=None, site=None, host=None, body=None, method=None):
        headers = {"Host": host or f"127.0.0.1:{port}", "Content-Type": "application/json"}
        if token is not None:
            headers["X-Careerbrew-Token"] = token
        if cookie is not None:
            headers["Cookie"] = cookie
        if site is not None:
            headers["Sec-Fetch-Site"] = site
        data = json.dumps(body if body is not None else {}).encode() if method == "POST" else None
        req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", data=data, headers=headers, method=method)
        try:
            with urllib.request.urlopen(req, timeout=10) as r:  # noqa: S310 (local test server)
                return r.status, r.headers, json.loads(r.read() or b"null")
        except urllib.error.HTTPError as e:
            return e.code, e.headers, json.loads(e.read() or b"null")

    yield SimpleNamespace(call=call, port=port, jar=lambda tok: f"careerbrew-{port}={tok}")
    srv.shutdown()
    srv.server_close()


@needs_lsof
def test_a_restart_keeps_the_token_so_an_open_tab_keeps_working(served, monkeypatch, home):
    """The issue's own test: start the web app, end it (an update or a crash), start it again: the old tab works."""
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    p, started = served
    first = server.start_background(p)
    pid = int(store.read_json(server.web_state_path(), {})["pid"])
    started.add(pid)
    assert _status(first) == 200
    os.kill(pid, signal.SIGKILL)  # an update, a crash, or a reboot ends the server
    _wait(lambda: not _listeners(p))
    second = server.start_background(p)
    started.add(int(store.read_json(server.web_state_path(), {})["pid"]))
    tok = _token_of(first)
    assert _token_of(second) == tok, "the same token after a restart"
    assert _status(first) == 200, "the old tab's token works on the new server: no 'missing or wrong token'"
    raw = server.web_token_path().read_bytes()
    assert vault.is_sealed(raw) and tok.encode() not in raw, "the token is encrypted at rest"
    assert "token" not in (store.read_json(server.web_state_path(), {}) or {}), "web.json keeps the pid and port only"
    for name in ("events.jsonl", "web.log"):
        log = home / "logs" / name
        assert not log.exists() or tok not in log.read_text(), f"{name} never holds the token"


def test_the_background_server_reads_the_token_from_the_store_not_its_environment(monkeypatch):
    seen: dict = {}

    class Proc:
        pid = 1

        def poll(self):
            return None

        def kill(self):
            pass

    monkeypatch.setenv("CAREERBREW_SERVE_TOKEN", "pinned-by-a-test-run")
    monkeypatch.setattr(subprocess, "Popen", lambda cmd, **kw: seen.update(kw) or Proc())
    monkeypatch.setattr(server, "listening_pids", lambda port: [])  # never look at, or stop, a real server
    monkeypatch.setattr(server, "_ping", lambda port, token, timeout=2: True)
    url = server.start_background(8799)
    tok = server.web_token()
    assert _token_of(url) == tok
    assert "CAREERBREW_SERVE_TOKEN" not in seen["env"], "a test's pinned token never leaks into the real server"
    assert tok not in json.dumps(seen["env"]), "the token is not in the server's environment (ps shows it)"


def test_a_tab_with_the_token_gets_an_httponly_strict_cookie_for_127_0_0_1(web):
    tok = server.web_token()
    status, headers, body = web.call("/api/session", method="POST", token=tok)
    assert status == 200 and tok not in json.dumps(body)
    attrs = [a.strip() for a in headers["Set-Cookie"].split(";")]
    assert attrs[0] == web.jar(tok), "one cookie per port: two careerbrew apps on 127.0.0.1 never share one"
    assert {"HttpOnly", "SameSite=Strict", "Path=/"} <= set(attrs[1:])
    assert not any(a.lower().startswith(("domain=", "expires=")) for a in attrs), "host-only (127.0.0.1); its age is Max-Age"
    max_age = int(next(a for a in attrs if a.startswith("Max-Age=")).split("=", 1)[1])
    assert 29 * 86400 < max_age <= 30 * 86400, "it lasts as long as the token (web_token_max_days, default 30)"
    status, headers, _ = web.call("/api/session", method="POST", token="wrong")
    assert status == 401 and "Set-Cookie" not in headers
    status, headers, _ = web.call("/api/session", method="POST", token=tok, host=f"localhost:{web.port}")
    assert status == 200 and "Set-Cookie" not in headers, "the cookie is bound to 127.0.0.1, the host every link uses"
    status, headers, _ = web.call("/api/session", method="POST", token=tok, site="cross-site")
    assert status == 403 and "Set-Cookie" not in headers


def test_the_cookie_restores_the_token_for_this_apps_own_pages_only(web):
    tok = server.web_token()
    jar = web.jar(tok)
    status, headers, body = web.call("/api/session", cookie=jar, site="same-origin")
    assert (status, body) == (200, {"token": tok}), "a reload, a new tab, or a bookmark signs in from the cookie"
    assert headers["Cross-Origin-Resource-Policy"] == "same-origin" and headers["Cache-Control"] == "no-store"
    assert web.call("/api/session", cookie=jar)[0] == 200, "no Sec-Fetch-Site (an older browser): the same-origin policy still hides the answer"
    for site in ("cross-site", "same-site", "none"):
        assert web.call("/api/session", cookie=jar, site=site)[0] == 403, site
    assert web.call("/api/session", site="cross-site")[0] == 403, "a cross-site request without the cookie is refused"
    assert web.call("/api/session", site="same-origin")[0] == 401, "no cookie: not signed in"
    assert web.call("/api/session", cookie=web.jar("wrong"), site="same-origin")[0] == 401
    assert web.call("/api/session", cookie=f"careerbrew-1={tok}", site="same-origin")[0] == 401, "another port's cookie does not count"
    assert web.call("/api/session", cookie=jar, site="same-origin", host="evil.example.com")[0] == 403, "DNS rebinding"
    other = "theme=dark; " + jar + "; x=1"
    assert web.call("/api/session", cookie=other, site="same-origin")[0] == 200, "other cookies on 127.0.0.1 do not matter"


def test_every_other_api_route_still_needs_the_token_header(web):
    tok = server.web_token()
    jar = web.jar(tok)
    for path in ("/api/overview", "/api/today", "/api/changes", "/api/ping", "/api/config"):
        status, _, body = web.call(path, cookie=jar, site="same-origin")
        assert status == 401 and "careerbrew open" in body["error"], path
    assert web.call("/api/tasks", method="POST", cookie=jar, site="same-origin", body={"title": "x"})[0] == 401
    assert web.call("/api/overview", token=tok)[0] == 200


def test_rotate_token_signs_out_the_old_token_at_once(web, capsys):
    old = server.web_token()
    assert web.call("/api/overview", token=old)[0] == 200
    assert cli.main(["web", "rotate-token"]) == 1, "it signs out every open tab: it needs --yes"
    assert server.web_token() == old and "--yes" in capsys.readouterr().out
    assert cli.main(["web", "rotate-token", "--yes"]) == 0
    out = capsys.readouterr().out
    new = server.web_token()
    assert new != old and old not in out and new not in out and "careerbrew open" in out
    assert web.call("/api/overview", token=old)[0] == 401, "the running server reads the new token: no restart"
    assert web.call("/api/session", cookie=web.jar(old), site="same-origin")[0] == 401, "an old cookie is signed out too"
    assert web.call("/api/overview", token=new)[0] == 200


def test_rotate_token_runs_from_the_web_app_only_when_confirmed(web):
    assert actions.CONFIRM["web.rotate-token"]
    old = server.web_token()
    status, _, body = web.call("/api/actions/web.rotate-token", method="POST", token=old, body={"values": {}})
    assert status == 400 and "confirm" in body["error"]
    status, _, body = web.call("/api/actions/web.rotate-token", method="POST", token=old, body={"values": {}, "confirm": True})
    assert status == 200 and body["exit"] == 0 and old not in body["output"]
    assert server.web_token() != old and web.call("/api/overview", token=old)[0] == 401


def test_the_token_expires_after_web_token_max_days(web, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00")
    config.set_value("web_token_max_days", "7")
    old = server.web_token()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-07T09:00")
    assert web.call("/api/overview", token=old)[0] == 200 and server.web_token() == old, "day 6 of 7: still signed in"
    max_age = int(web.call("/api/session", method="POST", token=old)[1]["Set-Cookie"].split("Max-Age=", 1)[1].split(";")[0])
    assert max_age == 86400, "the cookie ends with the token"
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-08T09:00")
    status, _, body = web.call("/api/overview", token=old)
    assert status == 401 and "expired" in body["error"] and "careerbrew open" in body["error"]
    assert web.call("/api/session", cookie=web.jar(old), site="same-origin")[0] == 401
    new = server.web_token()
    assert new != old and web.call("/api/overview", token=new)[0] == 200, "the next open makes a new one"


def test_the_max_age_setting_is_validated():
    with pytest.raises(ValueError, match="1 to 365"):
        config.set_value("web_token_max_days", "0")
    with pytest.raises(ValueError, match="1 to 365"):
        config.set_value("web_token_max_days", "366")
    assert config.get("web_token_max_days") == 30


def test_the_sign_in_page_button_opens_a_signed_in_tab_without_returning_the_token(web, monkeypatch):
    opened: list[str] = []
    monkeypatch.setattr("webbrowser.open", lambda url, *a, **k: opened.append(url) or True)
    tok = server.web_token()
    status, _, body = web.call("/api/session/open", method="POST", site="same-origin", body={"view": "week"})
    assert status == 200 and tok not in json.dumps(body)
    assert opened == [f"http://127.0.0.1:{web.port}/#/week&token={tok}"], "the one-time open link, to the default browser only"
    for site in (None, "cross-site", "same-site", "none"):
        assert web.call("/api/session/open", method="POST", site=site)[0] == 403, site
    assert web.call("/api/session/open", method="POST", site="same-origin", host="evil.example.com")[0] == 403
    assert len(opened) == 1, "no other site can make it open tabs"
    web.call("/api/session/open", method="POST", site="same-origin", body={"view": "../../etc"})
    assert opened[-1].endswith(f"/#/today&token={tok}"), "an unknown view opens Today"
    monkeypatch.setattr("webbrowser.open", lambda *a, **k: False)
    status, _, body = web.call("/api/session/open", method="POST", site="same-origin")
    assert status == 409 and "careerbrew open" in body["error"]


def test_the_sign_in_page_button_makes_a_new_token_when_the_old_one_expired(web, monkeypatch):
    opened: list[str] = []
    monkeypatch.setattr("webbrowser.open", lambda url, *a, **k: opened.append(url) or True)
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-01T09:00")
    old = server.web_token()
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-01T09:00")
    assert web.call("/api/session/open", method="POST", site="same-origin")[0] == 200
    new = _token_of(opened[0])
    assert new != old and web.call("/api/overview", token=new)[0] == 200


def test_an_open_tab_from_before_the_upgrade_keeps_its_token(home):
    """`day/web.json` held the per-run token before #141: the first start adopts it, so that tab keeps working."""
    store.write_json(server.web_state_path(), {"pid": 1, "port": 8765, "token": "token-from-the-old-version"})
    assert server.web_token() == "token-from-the-old-version"
    assert store.read_json(server.web_state_path(), {}) == {"pid": 1, "port": 8765}, "web.json drops the token"
    assert server.web_token() == "token-from-the-old-version"


def test_serve_prints_the_token_only_to_a_terminal(home):
    env = {k: v for k, v in os.environ.items() if k != "CAREERBREW_SERVE_TOKEN"}
    p = subprocess.Popen([sys.executable, "-c", "from careerbrew import server; server.run(0)"], stdout=subprocess.PIPE, text=True, env=env)  # noqa: S603
    try:
        assert p.stdout is not None
        first = p.stdout.readline()
        tok = server.web_token()
        assert "127.0.0.1" in first and "token=" not in first and tok not in first, "an agent's stdout never gets the token"
        assert "careerbrew open" in first
    finally:
        p.kill()
        p.wait()


def test_web_token_events_are_content_free(web, monkeypatch, home):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    monkeypatch.setattr("webbrowser.open", lambda url, *a, **k: True)
    tokens = [server.web_token()]
    web.call("/api/session", method="POST", token=tokens[0])
    web.call("/api/session", cookie=web.jar(tokens[0]), site="same-origin")
    web.call("/api/session", site="cross-site")
    web.call("/api/session/open", method="POST", site="same-origin")
    assert cli.main(["web", "rotate-token", "--yes"]) == 0
    tokens.append(server.web_token())
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    kinds = {(r["event"], r.get("route"), r["status"]) for r in rows if r["event"].startswith("web.")}
    assert {("web.session", "set", "ok"), ("web.session", "restore", "ok"), ("web.session", "restore", "refused"), ("web.session", "open", "ok")} <= kinds
    assert {r["status"] for r in rows if r["event"] == "web.token"} >= {"first", "request"}
    text = json.dumps(rows)
    assert all(t not in text for t in tokens), "the log holds no token"
