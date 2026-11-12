"""Read-only mail metadata: Outlook (Microsoft Graph), Fastmail (JMAP), Proton (Bridge IMAP).

careerbrew reads only message metadata (id, sender address, subject, date),
never bodies, and never sends. Config: `<data root>/mail.yaml`

  accounts:
    - {name: work, kind: graph, token: keychain:careerbrew.graph, machine: corporate}   # Mail.ReadBasic
    - {name: personal, kind: jmap, token: keychain:careerbrew.fastmail}                   # Fastmail API token, read-only mail scope
    - {name: proton, kind: imap, host: 127.0.0.1, port: 1143, starttls: true,             # Proton Mail Bridge
       user: env:PROTON_USER, password: keychain:careerbrew.proton-bridge}

`machine` (personal | corporate | any; default personal) keeps job-hunt mail
off corporate machines: on a corporate-role machine only corporate/any
accounts are read.

Uses:
- job hunt: LinkedIn job-alert emails become postings (jobs/linkedin.py), and
  recruiter or interview emails become suggested application updates;
- work: nothing is stored beyond counts (the day watcher uses calendar facts).
Sources: knowledge/research/api-notes.md sections 9-11.
"""

from __future__ import annotations

import email.utils
import imaplib
import re
import ssl
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime, timedelta
from email.header import decode_header, make_header
from typing import Any

from . import calendar, connectors, events, machine, msauth, paths, secrets, store


@dataclass(frozen=True)
class Message:
    id: str
    sender: str
    subject: str
    received: str
    account: str


def accounts() -> list[dict[str, Any]]:
    accs = list((store.read_yaml(paths.data_root() / "mail.yaml", {}) or {}).get("accounts", []))
    have = {a.get("token") for a in accs if a.get("kind") == "graph"}
    accs += [g for g in connectors.graph_sources("mail") if g["token"] not in have]  # an outlook connector (issue #63)
    if machine.role() == "corporate":
        accs = [a for a in accs if a.get("machine", "personal") in ("corporate", "any")]
    return accs


def _since(days: int) -> datetime:
    return calendar.now_utc() - timedelta(days=days)


def graph(acc: dict[str, Any], days: int) -> list[Message]:
    tok = msauth.bearer(acc)
    since = _since(days).strftime("%Y-%m-%dT%H:%M:%SZ")
    api = acc.get("api", "https://graph.microsoft.com/v1.0").rstrip("/")
    url = f"{api}/me/messages?$select=id,internetMessageId,from,subject,receivedDateTime&$top=100&$orderby=receivedDateTime%20desc&$filter=receivedDateTime%20ge%20{since}"
    data = connectors.http_json(url, {"Authorization": f"Bearer {tok}"})
    return [
        Message(m.get("internetMessageId") or m["id"], ((m.get("from") or {}).get("emailAddress") or {}).get("address", "").lower(), m.get("subject", ""), m.get("receivedDateTime", ""), acc.get("name", "graph"))
        for m in data.get("value", [])
    ]


def jmap(acc: dict[str, Any], days: int) -> list[Message]:
    tok = secrets.resolve(acc["token"])
    hdr = {"Authorization": f"Bearer {tok}"}
    session = connectors.http_json(acc.get("session_url", "https://api.fastmail.com/jmap/session"), hdr)
    account_id = session["primaryAccounts"]["urn:ietf:params:jmap:mail"]
    api_url = session["apiUrl"]
    req = {
        "using": ["urn:ietf:params:jmap:core", "urn:ietf:params:jmap:mail"],
        "methodCalls": [
            ["Email/query", {"accountId": account_id, "filter": {"after": _since(days).strftime("%Y-%m-%dT%H:%M:%SZ")}, "sort": [{"property": "receivedAt", "isAscending": False}], "limit": 100}, "0"],
            ["Email/get", {"accountId": account_id, "#ids": {"resultOf": "0", "name": "Email/query", "path": "/ids"}, "properties": ["id", "messageId", "from", "subject", "receivedAt"]}, "1"],
        ],
    }
    resp = connectors.http_json(api_url, hdr, body=req)
    got = next((r[1] for r in resp.get("methodResponses", []) if r[0] == "Email/get"), {"list": []})
    return [Message((m.get("messageId") or [m["id"]])[0], ((m.get("from") or [{}])[0].get("email") or "").lower(), m.get("subject", ""), m.get("receivedAt", ""), acc.get("name", "jmap")) for m in got.get("list", [])]


ImapFactory = Callable[[str, int], Any]


def _decode(value: str) -> str:
    try:
        return str(make_header(decode_header(value)))
    except (ValueError, LookupError):
        return value


def imap(acc: dict[str, Any], days: int, factory: ImapFactory | None = None) -> list[Message]:
    """Read-only (EXAMINE) envelope fetch; works with Proton Mail Bridge and any IMAP server."""
    host, port = acc.get("host", "127.0.0.1"), int(acc.get("port", 1143))
    connectors.check_host(host)
    user, password = secrets.resolve(acc["user"]), secrets.resolve(acc["password"])
    conn = (factory or imaplib.IMAP4)(host, port)
    try:
        if acc.get("starttls", True):
            ctx = ssl.create_default_context()
            if host in ("127.0.0.1", "localhost"):
                ctx.check_hostname = False  # Bridge uses a self-signed cert on localhost only
                ctx.verify_mode = ssl.CERT_NONE
            conn.starttls(ssl_context=ctx)
        conn.login(user, password)
        conn.select(acc.get("mailbox", "INBOX"), readonly=True)
        since = _since(days).strftime("%d-%b-%Y")
        _typ, data = conn.search(None, f"(SINCE {since})")
        ids = (data[0] or b"").split()[-200:]
        out = []
        for raw_num in ids:
            num = raw_num.decode()
            _t, parts = conn.fetch(num, "(BODY.PEEK[HEADER.FIELDS (MESSAGE-ID FROM SUBJECT DATE)])")
            raw = b"".join(p[1] for p in parts if isinstance(p, tuple)).decode("utf-8", "replace")
            hdr = email.message_from_string(raw)
            sender = email.utils.parseaddr(hdr.get("From", ""))[1].lower()
            out.append(Message(hdr.get("Message-ID", num), sender, _decode(hdr.get("Subject", "")), hdr.get("Date", ""), acc.get("name", "imap")))
        return out
    finally:
        try:
            conn.logout()
        except (OSError, imaplib.IMAP4.error):
            pass


READERS = {"graph": graph, "jmap": jmap, "imap": imap}

INTERVIEW = re.compile(r"\b(interview|phone screen|onsite|on-site|technical screen|recruiter|next steps|availability|offer|coding challenge|take[- ]home|assessment)\b", re.IGNORECASE)


def fetch(days: int = 7) -> tuple[list[Message], list[str]]:
    out: list[Message] = []
    errors: list[str] = []
    for acc in accounts():
        try:
            out.extend(READERS[acc["kind"]](acc, days))
        except (OSError, KeyError, ValueError, RuntimeError, imaplib.IMAP4.error) as exc:
            errors.append(f"{acc.get('name', acc.get('kind'))}: {type(exc).__name__}")
            events.emit("mail.error", source_kind=str(acc.get("kind")), error=type(exc).__name__)
    events.emit("mail.fetch", status="ok", messages=len(out))
    return out, errors


def classify(msgs: list[Message]) -> dict[str, list[Message]]:
    from .jobs import linkedin

    out: dict[str, list[Message]] = {"linkedin_alert": [], "linkedin_application": [], "interview": [], "other": []}
    for m in msgs:
        if m.sender in linkedin.ALERT_SENDERS:
            out["linkedin_alert"].append(m)
        elif m.sender in linkedin.APPLICATION_SENDERS:
            out["linkedin_application"].append(m)
        elif INTERVIEW.search(m.subject):
            out["interview"].append(m)
        else:
            out["other"].append(m)
    return out
