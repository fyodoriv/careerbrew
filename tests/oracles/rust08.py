"""rust-08 Python oracle for HTTP sources, calendars, mail and ``connect``.

Every read uses the checked-in synthetic fixtures or an invocation-owned fake.
Reports contain typed metadata and fixed errors, never credential values. The
native candidate answers the same schema without starting Python or a live
account connection.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
import urllib.request
from datetime import date
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

from careerbrew import calendar, cli, config, connectors, machine, mail, msauth, plugins, secrets, store
from careerbrew.work import feed as work_feed

CASES = (
    "http-policy",
    "connector-evidence",
    "calendar-recurrence",
    "mail-readers",
    "source-isolation",
    "cli-connect",
)
FIX = Path(__file__).resolve().parents[1] / "fixtures"


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, set):
        return _typed(sorted(value))
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    if isinstance(value, (calendar.Event, mail.Message)):
        return _typed(value.__dict__)
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__}
    cases.append({"id": name, "result": _typed(value)})


@contextlib.contextmanager
def _patched(module: Any, **attrs: Any):
    old = {name: getattr(module, name) for name in attrs}
    for name, value in attrs.items():
        setattr(module, name, value)
    try:
        yield
    finally:
        for name, value in old.items():
            setattr(module, name, value)


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "off",
                "CAREERBREW_HTTP_FIXTURES": str(FIX / "http"),
                "CAREERBREW_NOW": "2026-10-19T10:00:00+00:00",
                "CB_TEST_TOKEN": "synthetic-not-reported",
                "CB_JIRA": "synthetic-jira-not-reported",
                "TZ": "UTC",
            }
        )
        os.environ.pop("CAREERBREW_TEST_OWNER", None)
        msauth._ACCESS.clear()


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


def _http_policy(base: Path) -> dict[str, Any]:
    Workspace(base, "http-policy")
    with _patched(plugins, effective_policy=lambda: {"allowed_hosts": ["example.com", "example.org"]}):
        hosts = ["example.com", "api.example.com", "badexample.com", "other.invalid", "localhost", "127.0.0.1"]
        host_rows = [(host, connectors.host_allowed(host)) for host in hosts]
        denied = _error(lambda: connectors.check_host("https://other.invalid/private"))

        def redirect(source: str, target: str, headers: dict[str, str]) -> str:
            req = urllib.request.Request(source, headers=headers)
            try:
                answer = connectors._PolicyRedirect().redirect_request(req, None, 302, "Found", {}, target)
            except Exception as exc:
                return type(exc).__name__
            return answer.full_url if answer is not None else "none"

        redirects = [
            redirect("https://api.example.com/a", "https://api.example.com/b", {"Authorization": "Bearer hidden"}),
            redirect("https://api.example.com/a", "https://api.example.org/b", {"Authorization": "Bearer hidden"}),
            redirect("https://api.example.com/a", "http://api.example.com/b", {}),
            redirect("https://api.example.com/a", "https://api.example.org/b", {"User-Agent": "careerbrew"}),
            redirect("https://api.example.com/a", "https://other.invalid/b", {}),
        ]
    user = connectors.http_json("https://api.github.com/user", {"Authorization": "Bearer synthetic-not-reported"})
    raw = connectors.http_get("https://weworkremotely.com/categories/remote-programming-jobs.rss").decode()
    missing = _error(lambda: connectors.http_json("https://absent.example.com/missing", {}))
    status = _error(lambda: connectors.http_json("https://bad.atlassian.example.com/rest/api/3/myself", {}))
    return {
        "hosts": host_rows,
        "denied": denied,
        "origins": [connectors._origin(url) for url in ["https://example.com/a", "http://example.com:80/a", "https://example.com:444/a"]],
        "redirects": redirects,
        "fixture_user": user.get("login"),
        "raw_xml": raw.startswith("<?xml") and "<rss" in raw,
        "missing": missing,
        "status": status,
        "limits": {"jira_pages": connectors.JIRA_MAX_PAGES, "pagerduty_pages": connectors.PD_MAX_PAGES, "reply_pages": connectors.REPLY_PAGES},
    }


def _items(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    return [{key: item.get(key) for key in ("id", "kind", "title", "at", "url", "tags", "origin", "draft", "state", "merged_at") if key in item} for item in items]


def _connector_evidence(base: Path) -> dict[str, Any]:
    Workspace(base, "connector-evidence")
    configs = {
        "github": {"kind": "github", "token": "env:CB_TEST_TOKEN", "user": "me"},
        "jira": {"kind": "jira", "token": "env:CB_TEST_TOKEN", "base_url": "https://jira.example.com", "email": "me@example.com"},
        "slack": {"kind": "slack", "token": "env:CB_TEST_TOKEN"},
        "gdocs": {"kind": "gdocs", "token": "env:CB_TEST_TOKEN"},
        "zoom": {"kind": "zoom", "token": "env:CB_TEST_TOKEN"},
        "outlook": {"kind": "outlook", "token": "env:CB_TEST_TOKEN"},
    }
    evidence = {kind: _items(connectors.run(cfg, "user")) for kind, cfg in configs.items()}
    with _patched(work_feed, personal_check=lambda _origin: lambda _item: True):
        personal_slack = connectors.run(configs["slack"], "user")
    seen: list[str] = []

    def paged(url: str, _headers: dict[str, str], body: dict[str, Any] | None = None) -> Any:
        assert body is None
        if url.endswith("/myself"):
            return {"accountId": "synthetic"}
        seen.append(url)
        if "nextPageToken=" not in url:
            return {"issues": [{"key": f"PROJ-{i}", "fields": {"summary": f"done {i}", "resolutiondate": "2026-10-01T00:00:00.000+0000", "issuetype": {"name": "Task"}}} for i in range(100)], "nextPageToken": "tok2", "isLast": False}
        return {"issues": [{"key": "PROJ-100", "fields": {"summary": "done last", "resolutiondate": "2026-10-01T00:00:00.000+0000", "issuetype": {"name": "Task"}}}], "isLast": True}

    with _patched(connectors, http_json=paged):
        pages = connectors.jira(configs["jira"], "user")
    rejected = _error(lambda: connectors.jira({**configs["jira"], "base_url": "https://bad.atlassian.example.com"}, "user"))
    pd_calls: list[str] = []
    pd_pages = iter(
        [
            {
                "log_entries": [
                    {
                        "id": "L1",
                        "type": "acknowledge_log_entry",
                        "created_at": "2026-10-18T10:00:00Z",
                        "agent": {"id": "U1"},
                        "incident": {"id": "I1", "summary": "Synthetic outage", "html_url": "https://pagerduty.example.com/incidents/I1"},
                    }
                ],
                "more": True,
            },
            {
                "log_entries": [
                    {"id": "L2", "type": "resolve_log_entry", "created_at": "2026-10-18T11:00:00Z", "agent": {"id": "U1"}, "incident": {"id": "I1", "summary": "Synthetic outage", "html_url": "https://pagerduty.example.com/incidents/I1"}}
                ],
                "more": False,
            },
        ]
    )

    def pd(url: str, _headers: dict[str, str], body: dict[str, Any] | None = None) -> Any:
        assert body is None
        pd_calls.append(url)
        return next(pd_pages)

    with _patched(connectors, http_json=pd):
        actions = connectors._pd_actions("https://api.pagerduty.com", {}, "U1", calendar.now_utc(), calendar.now_utc())
    return {
        "evidence": evidence,
        "personal_slack": personal_slack,
        "jira_pages": {"count": len(pages), "first": pages[0]["id"], "last": pages[-1]["id"], "calls": len(seen), "cursor": "nextPageToken=tok2" in seen[1]},
        "jira_rejected": rejected,
        "pagerduty": {"items": actions, "calls": len(pd_calls), "second_offset": "offset=1" in pd_calls[1]},
        "token_page": connectors.token_page("github", "https://github.example.com/api/v3"),
        "status_missing": connectors.status({"kind": "jira", "token": "env:CB_MISSING", "base_url": "https://jira.example.com"}),
        "status_ready": connectors.status({"kind": "github", "token": "env:CB_TEST_TOKEN", "user": "me"}),
    }


def _calendar_recurrence(base: Path) -> dict[str, Any]:
    Workspace(base, "calendar")
    tz = ZoneInfo("UTC")
    work = (FIX / "calendar" / "work.ics").read_text()
    work_events = calendar.parse_ics(work, date(2026, 10, 19), tz, ["me@example.com"])
    synthetic = """BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:series@synthetic.example\r\nDTSTART;TZID=America/New_York:20261019T090000\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEXDATE;TZID=America/New_York:20261020T090000\r\nSUMMARY:Private recurring review\r\nCLASS:PRIVATE\r\nORGANIZER:mailto:me@example.com\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n"""
    days = [calendar.parse_ics(synthetic, date(2026, 10, day), ZoneInfo("America/New_York"), ["me@example.com"]) for day in (19, 20, 21)]
    graph = calendar.graph_events({"kind": "graph", "token": "env:CB_TEST_TOKEN"}, date(2026, 10, 19), tz)
    return {"work": work_events, "series_lengths": [len(rows) for rows in days], "series": days, "graph": graph}


class FakeImap:
    calls: list[str]

    def __init__(self, host: str, port: int):
        self.calls = [f"connect:{host}:{port}"]

    def starttls(self, ssl_context: Any) -> None:
        self.calls.append(f"starttls:{ssl_context.check_hostname}:{ssl_context.verify_mode}")

    def login(self, _user: str, _password: str) -> None:
        self.calls.append("login")

    def select(self, mailbox: str, readonly: bool) -> None:
        self.calls.append(f"select:{mailbox}:{readonly}")

    def search(self, _charset: Any, query: str) -> tuple[str, list[bytes]]:
        self.calls.append(query)
        return "OK", [b"1"]

    def fetch(self, num: str, query: str) -> tuple[str, list[tuple[bytes, bytes]]]:
        self.calls.append(f"fetch:{num}:{query}")
        raw = b"Message-ID: <imap@synthetic.example>\r\nFrom: Recruiter <Recruiter@Example.com>\r\nSubject: =?utf-8?q?Technical_screen?=\r\nDate: Mon, 19 Oct 2026 09:00:00 +0000\r\n\r\n"
        return "OK", [(b"1", raw)]

    def logout(self) -> None:
        self.calls.append("logout")


def _messages(messages: list[mail.Message]) -> list[dict[str, Any]]:
    return [m.__dict__ for m in messages]


def _mail_readers(base: Path) -> dict[str, Any]:
    w = Workspace(base, "mail")
    graph = mail.graph({"name": "graph", "kind": "graph", "token": "env:CB_TEST_TOKEN"}, 7)
    jmap = mail.jmap({"name": "jmap", "kind": "jmap", "token": "env:CB_TEST_TOKEN"}, 7)
    holder: list[FakeImap] = []

    def factory(host: str, port: int) -> FakeImap:
        conn = FakeImap(host, port)
        holder.append(conn)
        return conn

    imap = mail.imap({"name": "imap", "kind": "imap", "host": "127.0.0.1", "port": 1143, "user": "env:CB_TEST_TOKEN", "password": "env:CB_TEST_TOKEN"}, 7, factory)
    classes = mail.classify(graph + jmap + imap)
    store.write_yaml(w.root / "mail.yaml", {"accounts": [{"name": "personal", "kind": "graph", "machine": "personal"}, {"name": "work", "kind": "graph", "machine": "corporate"}]})
    with _patched(machine, role=lambda: "corporate"), _patched(connectors, graph_sources=lambda _kind: []):
        corporate_accounts = [a["name"] for a in mail.accounts()]
    with _patched(plugins, effective_policy=lambda: {"allowed_hosts": ["other.example"]}):
        wrong_host = _error(lambda: mail.imap({"host": "mail.example.com", "user": "env:CB_TEST_TOKEN", "password": "env:CB_TEST_TOKEN"}, 7, factory))
    return {
        "graph": _messages(graph),
        "jmap": _messages(jmap),
        "imap": _messages(imap),
        "imap_calls": holder[0].calls,
        "classes": {name: [m.id for m in rows] for name, rows in classes.items()},
        "corporate_accounts": corporate_accounts,
        "wrong_host": wrong_host,
    }


def _source_isolation(base: Path) -> dict[str, Any]:
    w = Workspace(base, "isolation")
    store.write_yaml(w.root / "day" / "calendars.yaml", {"calendars": [{"kind": "graph", "token": "env:CB_TEST_TOKEN"}, {"kind": "ics", "path": str(w.base / "missing.ics")}]})
    with _patched(connectors, graph_sources=lambda _kind: []):
        events, calendar_errors = calendar.events_for(date(2026, 10, 19))
    store.write_yaml(w.root / "mail.yaml", {"accounts": [{"name": "good", "kind": "graph", "token": "env:CB_TEST_TOKEN"}, {"name": "bad", "kind": "jmap", "token": "env:CB_TEST_TOKEN", "session_url": "https://absent.example.com/session"}]})
    with _patched(machine, role=lambda: "personal"), _patched(connectors, graph_sources=lambda _kind: []):
        messages, mail_errors = mail.fetch()
    literal_ics = _error(lambda: calendar._ics_text({"url": "https://calendar.example.com/private.ics"}))
    literal_token = _error(lambda: connectors.run({"kind": "github", "token": "literal-not-reported", "user": "me"}, "user"))
    return {
        "calendar_titles": [e.title for e in events],
        "calendar_errors": calendar_errors,
        "mail_ids": [m.id for m in messages],
        "mail_errors": mail_errors,
        "literal_ics": literal_ics,
        "literal_token": literal_token,
    }


def _invoke(args: list[str]) -> dict[str, Any]:
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        rc = cli.main(args)
    return {"exit": rc, "stdout": output.getvalue()}


def _saved(root: Path) -> list[dict[str, Any]]:
    path = root / "work" / "connectors.yaml"
    if not path.exists():
        return []
    return store.read_yaml(path)["connectors"]


def _cli_connect(base: Path) -> dict[str, Any]:
    out: dict[str, Any] = {}
    scoped = Workspace(base, "cli-scoped")
    out["scoped"] = _invoke(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://scoped.atlassian.example.com", "--email", "me@example.com"])
    out["scoped"]["saved"] = _saved(scoped.root)
    bad = Workspace(base, "cli-bad")
    out["bad"] = _invoke(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://bad.atlassian.example.com", "--email", "me@example.com"])
    out["bad"]["saved"] = _saved(bad.root)
    missing = Workspace(base, "cli-missing")
    out["missing"] = _invoke(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://jira.example.com"])
    out["missing"]["saved"] = _saved(missing.root)
    interactive = Workspace(base, "cli-interactive")
    answers = iter(["https://jira.example.com", "me@example.com"])
    with _patched(sys.stdin, isatty=lambda: True), _patched(sys, stdout=sys.stdout), contextlib.ExitStack() as stack:
        import builtins
        import getpass

        stack.enter_context(_patched(builtins, input=lambda _prompt: next(answers)))
        stack.enter_context(_patched(getpass, getpass=lambda _prompt: "synthetic-interactive-not-reported"))
        stack.enter_context(_patched(secrets, store_keychain=lambda service, _value: f"keychain:{service}"))
        out["interactive"] = _invoke(["connect", "jira"])
    out["interactive"]["saved"] = _saved(interactive.root)
    denied = Workspace(base, "cli-denied")
    with _patched(plugins, effective_policy=lambda: {"allowed_connectors": ["github"], "allowed_hosts": None}):
        out["denied"] = _invoke(["connect", "jira", "--ref", "env:CB_JIRA", "--base-url", "https://jira.example.com", "--email", "me@example.com"])
    out["denied"]["saved"] = _saved(denied.root)

    keychain = {"careerbrew.outlook.refresh": "synthetic-refresh-not-reported-1"}

    def store_token(service: str, value: str) -> str:
        keychain[service] = value
        return f"keychain:{service}"

    def resolve(ref: str) -> str:
        if ref.startswith("keychain:"):
            return keychain[ref.removeprefix("keychain:")]
        return os.environ[ref.removeprefix("env:")]

    signed = Workspace(base, "cli-outlook")
    os.environ["CAREERBREW_TEST_OWNER"] = "1"
    with _patched(msauth.secrets, store_keychain=store_token, resolve=resolve), _patched(msauth.time, sleep=lambda _seconds: None):
        out["outlook"] = _invoke(["connect", "outlook", "--client-id", "synthetic-client"])
    out["outlook"]["saved"] = _saved(signed.root)
    out["outlook"]["rotated"] = keychain["careerbrew.outlook.refresh"] == "synthetic-refresh-token-2"
    declined = Workspace(base, "cli-declined")
    os.environ["CAREERBREW_TEST_OWNER"] = "1"
    with _patched(msauth.secrets, store_keychain=store_token, resolve=resolve), _patched(msauth.time, sleep=lambda _seconds: None):
        out["declined"] = _invoke(["connect", "outlook", "--client-id", "synthetic-client", "--tenant", "synthetic-declined-tenant"])
    out["declined"]["saved"] = _saved(declined.root)
    Workspace(base, "cli-no-client")
    with _patched(connectors, plugin_defaults=lambda _kind: {}), _patched(config, get=lambda _key: None):
        out["no_client"] = _invoke(["connect", "outlook"])
    manual = Workspace(base, "cli-manual")
    out["manual"] = _invoke(["connect", "outlook", "--ref", "env:CB_TEST_TOKEN"])
    out["manual"]["saved"] = _saved(manual.root)
    piped = Workspace(base, "cli-piped")
    out["piped"] = _invoke(["connect", "outlook", "--client-id", "synthetic-client"])
    out["piped"]["saved"] = _saved(piped.root)
    text = json.dumps(out, sort_keys=True, default=_typed)
    for marker in ("synthetic-not-reported", "synthetic-jira-not-reported", "synthetic-interactive-not-reported", "synthetic-refresh-not-reported", "synthetic-access-token", "synthetic-refresh-token"):
        if marker in text:
            raise RuntimeError("synthetic credential reached the CLI report")
    return out


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust08-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "http-policy": _http_policy,
        "connector-evidence": _connector_evidence,
        "calendar-recurrence": _calendar_recurrence,
        "mail-readers": _mail_readers,
        "source-isolation": _source_isolation,
        "cli-connect": _cli_connect,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    text = json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":"))
    for marker in ("synthetic-not-reported", "synthetic-jira-not-reported", "synthetic-interactive-not-reported", "synthetic-refresh-not-reported", "synthetic-access-token", "synthetic-refresh-token"):
        if marker in text:
            raise RuntimeError("synthetic credential reached the oracle report")
    return {"text": text + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
