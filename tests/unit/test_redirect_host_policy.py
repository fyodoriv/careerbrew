"""A redirect must obey the same host policy as the first request (outside review of the public release)."""

from __future__ import annotations

import re
import threading
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

import pytest

from careerbrew import connectors, telemetry
from careerbrew.work import listen

SRC = Path(__file__).resolve().parents[2] / "src" / "careerbrew"
# Modules whose calls the docs say are host-checked. Each must open URLs through connectors.open_url.
CHECKED = ("connectors.py", "telemetry.py", "calendar.py", "msauth.py", "work/listen.py")


@pytest.fixture
def redirector(monkeypatch):
    """A local server: /out redirects to another host, /in to a local page, /page answers."""
    seen: list[str] = []

    class Handler(BaseHTTPRequestHandler):
        def _answer(self):
            seen.append(self.path)
            if self.path.startswith("/out"):
                self.send_response(302)
                self.send_header("Location", "http://other.example.net/landing")
                self.end_headers()
            elif self.path == "/alias":  # the same server under another host name: another origin
                self.send_response(302)
                self.send_header("Location", f"http://localhost:{self.server.server_address[1]}/page")
                self.end_headers()
            elif self.path == "/in":
                self.send_response(302)
                self.send_header("Location", "/page")
                self.end_headers()
            else:
                self.send_response(200)
                self.end_headers()
                self.wfile.write(b"{}")

        do_GET = do_POST = _answer

        def log_message(self, *args):
            pass

    server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
    threading.Thread(target=server.serve_forever, daemon=True).start()
    monkeypatch.delenv("CAREERBREW_HTTP_FIXTURES", raising=False)
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {"allowed_hosts": ["jira.example.com"]})
    yield f"http://127.0.0.1:{server.server_address[1]}", seen
    server.shutdown()
    server.server_close()


def test_a_redirect_to_a_host_outside_the_policy_is_refused(redirector):
    base, _ = redirector
    with pytest.raises(connectors.ConnectorError, match="allowed_hosts"):
        connectors.http_get(f"{base}/out")
    with pytest.raises(connectors.ConnectorError, match="allowed_hosts"):
        connectors.http_json(f"{base}/out", {})


def test_a_redirect_to_an_allowed_host_is_followed(redirector):
    base, seen = redirector
    assert connectors.http_get(f"{base}/in") == b"{}"
    assert seen == ["/in", "/page"]


def test_a_request_with_a_token_never_follows_a_redirect_to_another_origin(redirector):
    """urllib would send the token header to the new address; the allowed-hosts list alone does not stop that."""
    base, seen = redirector
    for header in ({"Authorization": "Bearer made-up"}, {"PRIVATE-TOKEN": "made-up"}, {"Cookie": "s=made-up"}):
        with pytest.raises(connectors.ConnectorError, match="carries a token"):
            connectors.http_get(f"{base}/alias", headers=header)
        with pytest.raises(connectors.ConnectorError, match="carries a token"):
            connectors.http_json(f"{base}/alias", header)
    assert "/page" not in seen
    assert connectors.http_get(f"{base}/in", headers={"Authorization": "Bearer made-up"}) == b"{}", "the same origin keeps the token"
    assert connectors.http_get(f"{base}/alias") == b"{}", "a request with no token may move to another allowed origin"


def test_a_redirect_from_https_to_plain_http_is_refused(monkeypatch):
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {})
    handler = connectors._PolicyRedirect()
    for headers in ({}, {"Authorization": "Bearer made-up"}):
        req = urllib.request.Request("https://jira.example.com/a", headers=headers)
        with pytest.raises(connectors.ConnectorError, match="plain HTTP"):
            handler.redirect_request(req, None, 302, "Found", {}, "http://jira.example.com/a")
    kept = handler.redirect_request(urllib.request.Request("https://jira.example.com/a", headers={"Authorization": "Bearer made-up"}), None, 302, "Found", {}, "https://jira.example.com:443/b")
    assert kept.full_url == "https://jira.example.com:443/b", "the default port is the same origin"
    with pytest.raises(connectors.ConnectorError, match="carries a token"):
        handler.redirect_request(urllib.request.Request("https://jira.example.com/a", headers={"Authorization": "Bearer made-up"}), None, 302, "Found", {}, "https://jira.example.com:0/b")


def test_a_telemetry_redirect_to_a_host_outside_the_policy_sends_nothing_there(redirector, monkeypatch):
    base, _ = redirector
    monkeypatch.setenv("CAREERBREW_TELEMETRY_URL", f"{base}/out")
    with pytest.raises(connectors.ConnectorError, match="allowed_hosts"):
        telemetry._post({"api_key": "k", "batch": []})


def test_a_link_read_refuses_a_redirect_to_a_local_address(redirector, monkeypatch):
    """The first address of a link read is checked for a private target; a redirect gets the same check."""
    base, seen = redirector
    monkeypatch.setattr(connectors.plugins, "effective_policy", lambda: {})
    with pytest.raises(connectors.ConnectorError, match="private or local"):
        listen._fetch(f"{base}/in")
    assert seen == ["/in"]


def test_host_checked_modules_open_urls_only_through_the_checked_opener():
    bare = re.compile(r"urllib\.request\.urlopen\(|(?<![.\w])urlopen\(")
    assert {name: len(bare.findall((SRC / name).read_text(encoding="utf-8"))) for name in CHECKED} == dict.fromkeys(CHECKED, 0)
