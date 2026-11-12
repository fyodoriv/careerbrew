"""Registered missing-secret completion and fail-closed source controls (#326)."""

import copy
import os
import re
import socket
import subprocess
import urllib.parse
import urllib.request
import webbrowser
from pathlib import Path

import pytest

from careerbrew import cli, connectors, machine, models, notify, plugins, store
from careerbrew.jobs import sources

from .conftest import FIXTURES

ID_SERVICE = "careerbrew.adzuna.id"
KEY_SERVICE = "careerbrew.adzuna.key"
FAKE_ID = "SYNTHETIC_ADZUNA_ID_326"
FAKE_KEY = "test-only-key"
FREE_REQUESTS = (
    ("himalayas.app", "/jobs/api/search"),
    ("remotive.com", "/api/remote-jobs"),
    ("remoteok.com", "/api"),
    ("weworkremotely.com", "/categories/remote-programming-jobs.rss"),
    ("hn.algolia.com", "/api/v1/search_by_date"),
    ("hn.algolia.com", "/api/v1/items/49922569"),
)
ADZUNA_REQUEST = ("api.adzuna.com", "/v1/api/jobs/gb/search/1")
FREE_IDS = frozenset(
    {
        "himalayas:senior-platform-engineer",
        "remotive:2000001",
        "remotive:2000002",
        "remoteok:901",
        "remoteok:902",
        "wwr:pineco-senior-full-stack-engineer",
        "hn:49922600",
    }
)


class ScanFixture:
    """One test invocation owns the synthetic backend and transport observations."""

    def __init__(self, home, fixture_root, monkeypatch):
        self.home = home
        self.fixture_root = fixture_root
        self.monkeypatch = monkeypatch
        self.values = {ID_SERVICE: FAKE_ID, KEY_SERVICE: FAKE_KEY}
        self.backend_error = False
        self.backend_calls = []
        self.http_calls = []
        self.forbidden_calls = []
        self.before = b""

    def configure(self, configured):
        store.write_yaml(self.home / "jobs/sources.yaml", {"sources": configured})

    def preset(self):
        return copy.deepcopy(store.read_yaml(self.home / "jobs/sources.yaml")["sources"])

    def only(self, kind):
        self.configure([source for source in self.preset() if source["kind"] == kind])

    def response(self, relative, text):
        path = self.fixture_root / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="ascii")
        self.monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(self.fixture_root))

    def unchanged(self):
        assert sources.postings_path().read_bytes() == self.before

    def persisted_ids(self):
        return {posting.id for posting in sources.load()}

    def safe_output(self, output):
        assert FAKE_ID not in output and FAKE_KEY not in output


@pytest.fixture
def scan_fixture(home, tmp_path, monkeypatch, capsys):
    state = ScanFixture(home, tmp_path / "http-326", monkeypatch)

    def deny(kind):
        def blocked(*args, **kwargs):
            state.forbidden_calls.append(kind)
            raise AssertionError(f"issue326 fixture forbids {kind}")

        return blocked

    def credential_backend(argv, **kwargs):
        assert kwargs == {"capture_output": True, "text": True, "check": False}
        assert isinstance(argv, list) and len(argv) == 7
        service = argv[3]
        assert service in (ID_SERVICE, KEY_SERVICE)
        assert argv == ["security", "find-generic-password", "-s", service, "-a", "synthetic-326", "-w"]
        state.backend_calls.append(service)
        if state.backend_error:
            raise OSError("synthetic-backend-unavailable-326")
        value = state.values[service]
        return subprocess.CompletedProcess(argv, 44 if value is None else 0, stdout="" if value is None else value + "\n", stderr="")

    real_http_get = connectors.http_get

    def fixture_http_get(url, headers=None):
        parsed = urllib.parse.urlsplit(url)
        endpoint = (parsed.hostname, parsed.path)
        assert parsed.scheme == "https" and parsed.netloc == parsed.hostname
        assert endpoint in (*FREE_REQUESTS, ADZUNA_REQUEST)
        selected = Path(os.environ["CAREERBREW_HTTP_FIXTURES"]).resolve()
        assert selected in {(FIXTURES / "http").resolve(), state.fixture_root.resolve()}
        state.http_calls.append(endpoint)
        return real_http_get(url, headers)

    monkeypatch.setenv("LOGNAME", "synthetic-326")
    monkeypatch.setenv("USER", "synthetic-326")
    monkeypatch.setenv("CAREERBREW_NOW", "2026-11-02T09:00:00+00:00")
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(FIXTURES / "http"))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setattr(subprocess, "run", credential_backend)
    monkeypatch.setattr(subprocess, "Popen", deny("host process"))
    monkeypatch.setattr(urllib.request, "urlopen", deny("URL opening"))
    monkeypatch.setattr(urllib.request.OpenerDirector, "open", deny("URL opener"))
    monkeypatch.setattr(socket, "create_connection", deny("socket connection"))
    monkeypatch.setattr(socket, "getaddrinfo", deny("DNS lookup"))
    monkeypatch.setattr(socket.socket, "connect", deny("socket connect"))
    monkeypatch.setattr(socket.socket, "connect_ex", deny("socket connect_ex"))
    monkeypatch.setattr(webbrowser, "open", deny("browser opening"))
    monkeypatch.setattr(webbrowser, "open_new", deny("new browser"))
    monkeypatch.setattr(webbrowser, "open_new_tab", deny("browser tab"))
    monkeypatch.setattr(models, "complete", deny("model call"))
    monkeypatch.setattr(notify, "deliver", deny("notification"))
    monkeypatch.setattr(notify, "remind", deny("reminder"))
    monkeypatch.setattr(connectors, "http_get", fixture_http_get)

    assert cli.main(["init"]) == 0
    machine.set_role("personal")
    assert machine.role() == "personal"
    assert cli.main(["jobs", "preset", "--force"]) == 0
    state.configure([{**source, "country": "gb"} if source["kind"] == "adzuna" else source for source in state.preset()])  # the preset picks no country
    old = sources.Posting(id="old:326", company="Synthetic326", title="Synthetic old role", location="Example City", remote=True, url="https://example.com/old-326", description="Synthetic old fixture", source_name="Synthetic old source")
    sources.save([old])
    state.before = sources.postings_path().read_bytes()
    capsys.readouterr()
    return state


def _skip_warnings(output):
    return [line for line in output.splitlines() if "adzuna" in line.lower() and "skip" in line.lower()]


@pytest.mark.parametrize("missing", [ID_SERVICE, KEY_SERVICE], ids=["missing-id", "missing-key"])
def test_missing_secret_scope_completes_and_persists_free_sources(scan_fixture, capsys, missing):
    state = scan_fixture
    state.values[missing] = None
    result = cli.main(["jobs", "scan", "--no-cache"])
    output = capsys.readouterr().out

    assert state.http_calls == list(FREE_REQUESTS), "all five free groups complete, with no Adzuna request"
    assert state.backend_calls == ([ID_SERVICE] if missing == ID_SERVICE else [ID_SERVICE, KEY_SERVICE])
    assert state.forbidden_calls == []
    state.safe_output(output)

    persisted = state.persisted_ids()
    unchanged = sources.postings_path().read_bytes() == state.before
    assert result == 0, {"exit": result, "persisted_ids": sorted(persisted), "old_postings_unchanged": unchanged}
    assert persisted == FREE_IDS
    assert not unchanged
    warnings = _skip_warnings(output)
    assert len(warnings) == 1
    assert "careerbrew secret set careerbrew.adzuna.id" in warnings[0]
    assert "careerbrew secret set careerbrew.adzuna.key" in warnings[0]
    summary = next(line for line in output.splitlines() if line.startswith("scanned "))
    assert "scanned 7 posting(s)" in summary
    assert re.search(r"\b5 (?:successful |completed )?source", summary)
    assert re.search(r"\b1 (?:source\(s\) )?skipp", summary)


@pytest.mark.parametrize("case", ["valid-six", "all-missing-preserves", "valid-empty-replaces", "policy-denial", "invalid-reference", "backend-oserror", "malformed-source"])
def test_missing_secret_guard_preserves_other_source_boundaries(scan_fixture, monkeypatch, capsys, case):
    state = scan_fixture
    if case == "all-missing-preserves":
        state.only("adzuna")
        state.values[ID_SERVICE] = None
    elif case == "valid-empty-replaces":
        state.only("remoteok")
        state.response("remoteok.com/api.json", "[]")
    elif case == "policy-denial":
        monkeypatch.setattr(plugins, "effective_policy", lambda: {"secret_backends": ["env"]})
    elif case == "invalid-reference":
        configured = state.preset()
        configured[-1]["app_id"] = "not-a-reference-326"
        state.configure(configured)
    elif case == "backend-oserror":
        state.only("adzuna")
        state.backend_error = True
    elif case == "malformed-source":
        state.only("himalayas")
        state.response("himalayas.app/jobs_api_search.json", "not-json-326")

    if case == "backend-oserror":
        with pytest.raises(OSError, match="synthetic-backend-unavailable-326"):
            sources.scan(no_cache=True)
        assert state.backend_calls == [ID_SERVICE]
        assert state.http_calls == []
        assert state.forbidden_calls == []
        state.unchanged()
        return

    result = cli.main(["jobs", "scan", "--no-cache"])
    output = capsys.readouterr().out
    state.safe_output(output)
    assert state.forbidden_calls == []

    if case == "valid-six":
        assert state.http_calls == [*FREE_REQUESTS, ADZUNA_REQUEST]
        assert state.backend_calls == [ID_SERVICE, KEY_SERVICE]
        assert result == 0
        assert state.persisted_ids() == FREE_IDS | {"adzuna:4321"}
        assert "scanned 8 posting(s)" in output
        assert _skip_warnings(output) == []
        for path in (state.home / "cache").rglob("*"):
            if path.is_file():
                state.safe_output(store.read_text(path))
    elif case == "valid-empty-replaces":
        assert state.http_calls == [("remoteok.com", "/api")]
        assert state.backend_calls == []
        assert result == 0
        assert state.persisted_ids() == set()
        assert sources.postings_path().read_bytes() != state.before
        assert _skip_warnings(output) == []
    else:
        if case in ("policy-denial", "invalid-reference"):
            assert state.http_calls == list(FREE_REQUESTS)
            assert state.backend_calls == []
            assert _skip_warnings(output) == []
        elif case == "all-missing-preserves":
            assert state.http_calls == []
            assert state.backend_calls == [ID_SERVICE]
        else:
            assert case == "malformed-source"
            assert state.http_calls == [("himalayas.app", "/jobs/api/search")]
            assert state.backend_calls == []
            assert _skip_warnings(output) == []
        assert result != 0
        state.unchanged()
