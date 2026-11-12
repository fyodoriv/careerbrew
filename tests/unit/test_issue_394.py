"""Compatibility witnesses for the existing command recheck contract (#394)."""

import json
import os
import socket
import subprocess
import threading
from contextlib import ExitStack
from datetime import date
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path
from urllib.error import HTTPError

import pytest

from careerbrew import cli, connectors, secrets, store, vault
from careerbrew.workday import gate, questions, status

CLOCK = "2026-11-03T12:00:00-05:00"
DAY = date(2026, 11, 3)
CANARY = "synthetic-private-review-question-394"
COMMAND = "fixture-review 'two words' > not-a-shell"


@pytest.mark.parametrize(
    ("returncode", "closed"),
    [pytest.param(0, False, id="C01"), pytest.param(1, True, id="C02")],
)
def test_legacy_recheck_control(returncode, closed, home, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", CLOCK)
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "on")

    def forbidden(*_args, **_kwargs):
        pytest.fail("unapproved process or network call")

    monkeypatch.setattr(socket, "create_connection", forbidden)
    monkeypatch.setattr(socket, "getaddrinfo", forbidden)
    monkeypatch.setattr(socket.socket, "connect", forbidden)
    monkeypatch.setattr(subprocess, "Popen", forbidden)
    rec = questions.add("/careerbrew", CANARY, recheck=COMMAND, day=DAY)
    path = status.path_for(DAY)
    before = path.read_bytes()
    calls = []

    def completed(argv, **kwargs):
        calls.append(argv)
        assert argv == ["fixture-review", "two words", ">", "not-a-shell"]
        assert kwargs == {
            "stdin": subprocess.DEVNULL,
            "stdout": subprocess.DEVNULL,
            "stderr": subprocess.DEVNULL,
            "timeout": gate.VERIFY_SECONDS,
            "check": False,
        }
        return subprocess.CompletedProcess(argv, returncode)

    with monkeypatch.context() as seam:
        seam.setattr(subprocess, "run", completed)
        exit_code = cli.main(["day", "question", "recheck", "--json"])
    captured = capsys.readouterr()
    actual = json.loads(captured.out)
    records = questions.records(DAY)
    after = path.read_bytes()
    evidence = {
        "exit": exit_code,
        "checked": actual["checked"],
        "ask": len(actual["ask"]),
        "dropped": len(actual["dropped"]),
        "closed": bool(records[0]["closed"]),
        "sealed_before_after": vault.is_sealed(before) and vault.is_sealed(after),
        "bytes_preserved": before == after,
        "calls": len(calls),
    }
    (tmp_path / "issue394-legacy-observation.json").write_text(json.dumps(evidence, sort_keys=True), encoding="utf-8")
    assert CANARY not in captured.out and COMMAND not in captured.out and captured.err == ""
    assert len(records) == 1 and records[0]["id"] == rec["id"]
    assert records[0]["recheck"] == COMMAND
    assert evidence == {
        "exit": 0,
        "checked": 1,
        "ask": 0 if closed else 1,
        "dropped": 1 if closed else 0,
        "closed": closed,
        "sealed_before_after": True,
        "bytes_preserved": not closed,
        "calls": 1,
    }
    if closed:
        assert actual["dropped"][0]["why"] == "recheck: closed (exit 1)"
    else:
        assert actual["ask"][0]["state"] == "open" and actual["ask"][0]["recheck"] == "command"


_CASES_FILE = Path(__file__).resolve().parents[1] / "fixtures/review-pr-recheck/cases.json"
_FIXTURE = json.loads(_CASES_FILE.read_bytes())


@pytest.mark.parametrize("case", _FIXTURE["cases"], ids=[case["id"] for case in _FIXTURE["cases"]])
def test_review_pr_recheck(case, home, tmp_path, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", _FIXTURE["clock"])
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "on")

    def forbidden(*_args, **_kwargs):
        pytest.fail("unapproved process or network call")

    monkeypatch.setattr(socket, "create_connection", forbidden)
    monkeypatch.setattr(socket, "getaddrinfo", forbidden)
    monkeypatch.setattr(socket.socket, "connect", forbidden)
    monkeypatch.setattr(socket.socket, "connect_ex", forbidden)
    monkeypatch.setattr(socket.socket, "sendto", forbidden)
    monkeypatch.setattr(subprocess, "Popen", forbidden)
    store.write_yaml(home / "work/connectors.yaml", {"connectors": [{"kind": "github", "token": "env:FIXTURE_REVIEW_AUTH"}]})
    rec = questions.add("/careerbrew", CANARY, recheck=f"careerbrew:review-pr {_FIXTURE['pr_url']}", day=DAY)
    path = status.path_for(DAY)
    before = path.read_bytes()
    transport_calls = []
    secret_calls = []
    replacement = []

    def secret(ref):
        assert ref == "env:FIXTURE_REVIEW_AUTH"
        secret_calls.append(True)
        return _FIXTURE["synthetic_token"]

    def transport(url, headers, body=None, *, allow_redirects=True):
        assert allow_redirects is False
        expected = case["calls"][len(transport_calls)]
        transport_calls.append(url)
        assert expected["method"] == "GET" and url == expected["url"] and body is None
        assert headers == {"Authorization": f"Bearer {_FIXTURE['synthetic_token']}"}
        if changed := case.get("before_return_replace_recheck"):
            saved = questions.add("/careerbrew", CANARY, recheck=f"careerbrew:review-pr {changed}", day=DAY)
            assert saved["id"] == rec["id"]
            replacement.append(path.read_bytes())
        if error := expected.get("error"):
            assert error["kind"] == "HTTPError"
            raise HTTPError(url, error["status"], "synthetic transport refusal", {}, None)
        return expected["body"]

    monkeypatch.setattr(secrets, "resolve", secret)
    monkeypatch.setattr(connectors, "http_json", transport)
    exit_code = cli.main(["day", "question", "recheck", "--json"])
    output = capsys.readouterr()
    result = json.loads(output.out)
    (record,) = questions.records(DAY)
    after = path.read_bytes()
    closed = case["outcome"] == "closed"
    actual_outcome = "closed" if result["dropped"] else result["ask"][0]["state"]
    evidence = {
        "exit": exit_code,
        "outcome": actual_outcome,
        "checked": result["checked"],
        "stored_closed": bool(record["closed"]),
        "stored_sealed": vault.is_sealed(after),
        "same_id": record["id"] == rec["id"],
        "attempts": len(transport_calls),
        "secret_calls": len(secret_calls),
        "unchanged_since_observation": after == (replacement[0] if replacement else before),
        "replacement_count": len(replacement),
    }
    (tmp_path / "issue394-typed-observation.json").write_text(json.dumps(evidence, sort_keys=True), encoding="utf-8")
    assert evidence == {
        "exit": 0,
        "outcome": case["outcome"],
        "checked": 1,
        "stored_closed": closed,
        "stored_sealed": True,
        "same_id": True,
        "attempts": len(case["calls"]),
        "secret_calls": 1,
        "unchanged_since_observation": not closed,
        "replacement_count": int("before_return_replace_recheck" in case),
    }
    selected = result["dropped"] if closed else result["ask"]
    assert len(selected) == 1 and selected[0]["id"] == rec["id"]
    if not closed:
        assert selected[0]["recheck"] == "review-pr"
    for private in (CANARY, "github.com", "fixture-reviewer", _FIXTURE["synthetic_token"]):
        assert private not in output.out
    assert output.err == ""


AUTH = "Bearer SYNTHETIC-REDIRECT-REVIEW-394"


@pytest.fixture
def review_loopback(monkeypatch):
    requests = []
    violations = []
    stop = threading.Event()

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):  # noqa: N802
            origin = "primary" if self.server is primary else "secondary"
            requests.append({"origin": origin, "path": self.path, "auth_match": self.headers.get("Authorization") == AUTH})
            code, location = {
                "/cross": (302, f"http://127.0.0.1:{secondary.server_port}/destination"),
                "/same": (307, f"http://127.0.0.1:{primary.server_port}/destination"),
                "/ok": (200, None),
                "/refused": (403, None),
                "/destination": (200, None),
            }.get(self.path, (404, None))
            body = json.dumps({"result": "synthetic-ok"}).encode()
            self.send_response(code)
            if location is not None:
                self.send_header("Location", location)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, _format, *_args):
            pass

    def serve(server):
        while not stop.is_set():
            server.handle_request()

    with ExitStack() as stack:
        primary = stack.enter_context(HTTPServer(("127.0.0.1", 0), Handler))
        secondary = stack.enter_context(HTTPServer(("127.0.0.1", 0), Handler))
        allowed = {primary.server_address, secondary.server_address}
        real_connect = socket.socket.connect
        real_connect_ex = socket.socket.connect_ex
        real_resolve = socket.getaddrinfo

        def forbid(*_args, **_kwargs):
            violations.append("forbidden")
            pytest.fail("review transport tried an unapproved operation")

        def connect(sock, address):
            if address not in allowed:
                forbid()
            return real_connect(sock, address)

        def connect_ex(sock, address):
            if address not in allowed:
                forbid()
            return real_connect_ex(sock, address)

        def resolve(host, port, *args, **kwargs):
            if (host, port) not in allowed:
                forbid()
            return real_resolve(host, port, *args, **kwargs)

        for name in tuple(os.environ):
            if name.lower().endswith("_proxy"):
                monkeypatch.delenv(name)
        monkeypatch.setenv("no_proxy", "127.0.0.1")
        monkeypatch.delenv("CAREERBREW_HTTP_FIXTURES", raising=False)
        monkeypatch.setattr(socket.socket, "connect", connect)
        monkeypatch.setattr(socket.socket, "connect_ex", connect_ex)
        monkeypatch.setattr(socket.socket, "sendto", forbid)
        monkeypatch.setattr(socket, "getaddrinfo", resolve)
        monkeypatch.setattr(subprocess, "Popen", forbid)

        workers = []
        try:
            for server in (primary, secondary):
                server.timeout = 0.1
                worker = threading.Thread(target=serve, args=(server,))
                worker.start()
                workers.append(worker)
            yield f"http://127.0.0.1:{primary.server_port}", requests, violations
        finally:
            stop.set()
            for worker in workers:
                worker.join(timeout=2)
                assert not worker.is_alive(), "owned HTTP fixture did not stop"
    assert not violations


def observe_transport(review_loopback, route, tmp_path):
    base, requests, violations = review_loopback
    try:
        body = connectors.http_json(base + route, {"Authorization": AUTH}, allow_redirects=False)
    except HTTPError as exc:
        status, body = exc.code, None
        exc.close()
    else:
        status = 200
    evidence = {"status": status, "body": body, "requests": requests, "violations": len(violations)}
    (tmp_path / "review-transport-observation.json").write_text(json.dumps(evidence, sort_keys=True), encoding="utf-8")
    return evidence


@pytest.mark.parametrize(
    ("route", "status"),
    [pytest.param("/cross", 302, id="cross-302"), pytest.param("/same", 307, id="same-307")],
)
def test_review_transport_redirect(route, status, review_loopback, tmp_path):
    evidence = observe_transport(review_loopback, route, tmp_path)
    assert evidence["requests"][0] == {"origin": "primary", "path": route, "auth_match": True}
    assert sum(request["path"] == "/destination" for request in evidence["requests"]) == 0
    assert evidence == {
        "status": status,
        "body": None,
        "violations": 0,
        "requests": [{"origin": "primary", "path": route, "auth_match": True}],
    }


@pytest.mark.parametrize(
    ("route", "status", "body"),
    [pytest.param("/ok", 200, {"result": "synthetic-ok"}, id="direct-200"), pytest.param("/refused", 403, None, id="direct-403")],
)
def test_review_transport_direct(route, status, body, review_loopback, tmp_path):
    assert observe_transport(review_loopback, route, tmp_path) == {
        "status": status,
        "body": body,
        "violations": 0,
        "requests": [{"origin": "primary", "path": route, "auth_match": True}],
    }


def test_review_pr_recheck_redirect_stays_unknown(home, review_loopback, tmp_path, monkeypatch, capsys):
    base, requests, violations = review_loopback
    monkeypatch.setenv("CAREERBREW_NOW", CLOCK)
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "on")
    store.write_yaml(home / "work/connectors.yaml", {"connectors": [{"kind": "github", "token": "env:FIXTURE_REVIEW_AUTH"}]})
    record = questions.add("/careerbrew", CANARY, recheck=f"careerbrew:review-pr {_FIXTURE['pr_url']}", day=DAY)
    path = status.path_for(DAY)
    before = path.read_bytes()
    actual_http_json = connectors.http_json
    calls = []

    def secret(ref):
        assert ref == "env:FIXTURE_REVIEW_AUTH"
        return AUTH.removeprefix("Bearer ")

    def local_transport(url, headers, body=None, *, allow_redirects=True):
        assert url == "https://api.github.com/repos/example/review-demo/pulls/41" and body is None
        assert headers == {"Authorization": AUTH}
        calls.append(allow_redirects)
        return actual_http_json(base + "/cross", headers, allow_redirects=allow_redirects)

    monkeypatch.setattr(secrets, "resolve", secret)
    monkeypatch.setattr(connectors, "http_json", local_transport)
    exit_code = cli.main(["day", "question", "recheck", "--json"])
    output = capsys.readouterr()
    result = json.loads(output.out)
    actual = {
        "exit": exit_code,
        "checked": result["checked"],
        "dropped": len(result["dropped"]),
        "unknown": [(row["id"], row["state"], row["recheck"]) for row in result["ask"]],
        "strict": calls == [False],
        "requests": requests,
        "violations": violations,
        "bytes_preserved": path.read_bytes() == before,
    }
    (tmp_path / "issue394-typed-redirect-observation.json").write_text(json.dumps(actual), encoding="utf-8")
    assert actual == {
        "exit": 0,
        "checked": 1,
        "dropped": 0,
        "unknown": [(record["id"], "unknown", "review-pr")],
        "strict": True,
        "requests": [{"origin": "primary", "path": "/cross", "auth_match": True}],
        "violations": [],
        "bytes_preserved": True,
    }
    assert all(private not in output.out for private in (CANARY, "github.com", AUTH, base))
    assert not output.err
