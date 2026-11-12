"""Issue #118: a careerbrew server on the port that `day/web.json` does not track.

Each test starts real servers on a free port from the OS (never the default 8765)
over the synthetic test home, and stops every one it started in teardown.
"""

from __future__ import annotations

import json
import os
import shutil
import signal
import socket
import subprocess
import sys
import threading
import urllib.error
import urllib.request

import pytest

from careerbrew import cli, config, server, store

pytestmark = pytest.mark.skipif(not shutil.which("lsof") or not shutil.which("ps"), reason="needs lsof and ps to find the server on a port")


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _cmdline(pid: int) -> str:
    return subprocess.run(["ps", "-o", "command=", "-p", str(pid)], capture_output=True, text=True, check=False).stdout.strip()


def _listeners(port: int) -> set[int]:
    out = subprocess.run(["lsof", "-nP", "-t", f"-iTCP:{port}", "-sTCP:LISTEN"], capture_output=True, text=True, check=False).stdout
    return {int(p) for p in out.split()}


def _status(url: str, token: str | None = None) -> int:
    """GET /api/overview the way the browser tab does: the token from the link, or another one."""
    base, _, frag = url.partition("#")
    port = base.rstrip("/").rsplit(":", 1)[1]
    tok = token if token is not None else frag.rsplit("token=", 1)[1]
    req = urllib.request.Request(f"{base.rstrip('/')}/api/overview", headers={"X-Careerbrew-Token": tok, "Host": f"127.0.0.1:{port}"})
    try:
        with urllib.request.urlopen(req, timeout=10) as r:  # noqa: S310 (local test server)
            return int(r.status)
    except urllib.error.HTTPError as e:
        return int(e.code)


@pytest.fixture
def port(home):
    """A free port set as `serve_port`; teardown stops every careerbrew server left on it or in the web state."""
    p = _free_port()
    config.set_value("serve_port", str(p))
    started: set[int] = set()
    yield p, started
    st = store.read_json(server.web_state_path(), {}) or {}
    for pid in started | _listeners(p) | ({int(st["pid"])} if st.get("pid") else set()):
        if "careerbrew serve" in _cmdline(pid):  # never a process that is not a careerbrew server
            os.kill(pid, signal.SIGKILL)


def _opened(monkeypatch) -> list[str]:
    opened: list[str] = []
    monkeypatch.setattr("webbrowser.open", lambda url, *a, **k: opened.append(url) or True)
    return opened


def test_open_stops_an_untracked_careerbrew_server_and_opens_a_working_link(port, monkeypatch, capsys):
    """The issue's own test: start a server, clear the web state, run `open`, and get a working page."""
    p, started = port
    first = server.start_background(p)
    old_pid = int(store.read_json(server.web_state_path(), {})["pid"])
    started.add(old_pid)
    assert _status(first) == 200
    store.write_json(server.web_state_path(), {})  # the server keeps the port, but nothing tracks it
    opened = _opened(monkeypatch)
    assert cli.main(["open"]) == 0, capsys.readouterr()
    st = store.read_json(server.web_state_path(), {})
    started.add(int(st["pid"]))
    assert len(opened) == 1 and _status(opened[0]) == 200, "the opened link works: no 'missing or wrong token'"
    assert st["pid"] != old_pid and _listeners(p) == {int(st["pid"])}, "the untracked server was stopped, the new one is tracked"
    assert _status(first) == 200, "the old tab keeps working on the new server: the token is stable (issue #141)"
    assert "token=" not in capsys.readouterr().out


def test_a_tracked_server_on_the_port_with_another_token_is_replaced(port, monkeypatch):
    """The state names a live pid, but the port answers to another token (another data folder's server): never hand out a link that cannot work."""
    p, started = port
    env = {**os.environ, "CAREERBREW_SERVE_TOKEN": "a-token-nobody-has"}
    other = subprocess.Popen([sys.executable, "-m", "careerbrew", "serve", "--port", str(p)], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, env=env)  # noqa: S603
    started.add(other.pid)
    assert other.stdout is not None and any("127.0.0.1" in other.stdout.readline() for _ in range(2)), "it listens (a first line may say the app is not built)"
    store.write_json(server.web_state_path(), {"pid": os.getpid(), "port": p})  # a live pid that is not the server
    opened = _opened(monkeypatch)
    assert cli.main(["open"]) == 0
    assert _status(opened[0]) == 200 and _status(opened[0], token="a-token-nobody-has") == 401
    assert other.wait(10) is not None, "the server with the other token was stopped"
    st = store.read_json(server.web_state_path(), {})
    started.add(int(st["pid"]))
    assert os.getpid() != st["pid"]


def test_two_starts_at_once_leave_one_tracked_server(port):
    """Two callers (say `careerbrew open` and a notification click) race: one server, and the state tracks it."""
    p, started = port
    urls: list[str] = []
    errors: list[BaseException] = []

    def go() -> None:
        try:
            urls.append(server.start_background(p))
        except BaseException as exc:  # noqa: BLE001 (collected for the assert)
            errors.append(exc)

    threads = [threading.Thread(target=go) for _ in range(2)]
    for t in threads:
        t.start()
    for t in threads:
        t.join(60)
    st = store.read_json(server.web_state_path(), {}) or {}
    if st.get("pid"):
        started.add(int(st["pid"]))
    started |= _listeners(p)
    assert errors == [] and len(urls) == 2 and urls[0] == urls[1], "the second caller gets the first one's link"
    assert _listeners(p) == {int(st["pid"])} and _status(urls[0]) == 200


def test_a_program_that_is_not_careerbrew_is_never_stopped(port):
    p, _started = port
    other = subprocess.Popen(  # noqa: S603 (a synthetic listener)
        [sys.executable, "-c", "import socket,sys,time; s=socket.socket(); s.bind(('127.0.0.1', int(sys.argv[1]))); s.listen(); print('up', flush=True); time.sleep(120)", str(p)],
        stdout=subprocess.PIPE,
        text=True,
    )
    try:
        assert other.stdout is not None and other.stdout.readline().strip() == "up"
        with pytest.raises(RuntimeError, match="serve_port") as exc:
            server.start_background(p)
        assert other.poll() is None, "the other program still runs"
        assert str(other.pid) in str(exc.value), "the error names the process that holds the port"
        assert store.read_json(server.web_state_path(), {}) in ({}, None)
    finally:
        other.kill()
        other.wait()


def test_the_stop_is_logged_without_content(port, monkeypatch, home):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    p, started = port
    server.start_background(p)
    started.add(int(store.read_json(server.web_state_path(), {})["pid"]))
    store.write_json(server.web_state_path(), {})
    url = server.start_background(p)
    started.add(int(store.read_json(server.web_state_path(), {})["pid"]))
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    stops = [r for r in rows if r["event"] == "web.untracked"]
    assert stops and stops[-1]["status"] == "stopped" and stops[-1]["count"] == 1
    tok = url.rsplit("token=", 1)[1]
    assert tok not in json.dumps(rows), "the log holds no token"
