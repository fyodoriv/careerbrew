"""Field telemetry: anonymous usage counts from real runs, sent to PostHog EU (US-89).

This module is the only adapter between careerbrew's content-free event log
(`events.jsonl`, docs/observability.md) and the telemetry endpoint, so a move to a
self-hosted endpoint changes only this file.

What leaves the machine: per event, its name, a timestamp, and an allowlist of fields
(command, story, status, error kind, durations and counts as numbers), keyed by a random
install id. Never content, titles, names, paths, the host name, or the user name.

When it sends: only when a PostHog project key is set (`careerbrew secret set posthog_key`,
the macOS Keychain), the `telemetry` setting is on (the default), `DO_NOT_TRACK` is unset,
no plugin policy says `telemetry: false`, and the plugin host policy (`allowed_hosts`) allows the
endpoint's host. With no key, nothing is sent. The watcher sends
at most once a day, and only after `careerbrew` has shown its one-line notice. A send failure
never fails a command; the cursor moves only after a send succeeds, so nothing is sent twice.

Lifecycle: the install id, the log cursor, the last send day, and the notice flag live in
`<data root>/telemetry.json` (encrypted at rest; module scope, read and written per call).
Deleting the file makes a new install id and sends the whole log again.
"""

from __future__ import annotations

import ipaddress
import json
import math
import os
import re
import urllib.parse
import urllib.request
import uuid
from typing import Any

from . import __version__, calendar, config, connectors, events, paths, plugins, secrets, store

DEFAULT_HOST = "https://eu.i.posthog.com"
KEYCHAIN_SERVICE = "posthog_key"  # the Keychain entry name, not a value
BATCH_SIZE = 500
MAX_EVENTS_PER_SEND = 5000
STRING_KEYS = {"story", "command", "status", "error", "source_kind", "tier"}
_ID = re.compile(r"^[a-z0-9_.:-]{1,48}$")
_NUM_KEY = re.compile(r"^[a-z][a-z0-9_]{0,31}$")
NOTICE = "careerbrew sends anonymous usage counts (command, ok or error, duration, counts; never content, names, or paths) to PostHog EU. Turn off: careerbrew telemetry off (or DO_NOT_TRACK=1)."


def _state_path():
    return paths.sub("telemetry.json")


def _state() -> dict[str, Any]:
    return store.read_json(_state_path(), {}) or {}


def _save(state: dict[str, Any]) -> None:
    store.write_json_if_changed(_state_path(), state)


def install_id() -> str:
    state = _state()
    if not state.get("install_id"):
        state["install_id"] = uuid.uuid4().hex
        _save(state)
    return str(state["install_id"])


def endpoint() -> str:
    return os.environ.get("CAREERBREW_TELEMETRY_URL", DEFAULT_HOST).rstrip("/") + "/batch/"


def key() -> str:
    """The PostHog project key, or "". `CAREERBREW_POSTHOG_KEY`, when set (tests set it empty), wins over the Keychain."""
    if "CAREERBREW_POSTHOG_KEY" in os.environ:
        return os.environ["CAREERBREW_POSTHOG_KEY"]
    try:
        return secrets.resolve(f"keychain:{KEYCHAIN_SERVICE}")
    except secrets.SecretError:
        return ""


_LABEL = r"(?!-)[A-Za-z0-9-]{1,63}(?<!-)"
_DNS_NAME = re.compile(rf"{_LABEL}(\.{_LABEL})*\.?")  # no empty label


def _valid_host(host: str) -> bool:
    """A DNS name, or an IP literal (an IPv6 one may carry a scope id such as `2001:db8::1%en0`)."""
    if ":" in host or re.fullmatch(r"[0-9.]+", host):  # all digits and dots: an IPv4 literal, not a DNS name
        try:
            ipaddress.ip_address(host)
        except ValueError:
            return False
        return True
    return len(host.rstrip(".")) <= 253 and bool(_DNS_NAME.fullmatch(host))  # 253: the longest DNS name


def blocked() -> str:
    """Why telemetry is off by switch (not by a missing key), or ""."""
    if os.environ.get("DO_NOT_TRACK", "").strip() not in ("", "0"):
        return "DO_NOT_TRACK is set"
    if any((p["manifest"].get("policy") or {}).get("telemetry") is False for p in plugins.installed()):
        return "a plugin policy forbids telemetry on this machine"
    if not config.get("telemetry"):
        return "off (turn on: careerbrew telemetry on)"
    try:
        url = urllib.parse.urlsplit(endpoint())
        host, _port = url.hostname or "", url.port  # .port raises on a port that is not a number
    except ValueError:
        return "the telemetry endpoint is not a valid address"
    if url.scheme not in ("http", "https") or not _valid_host(host):
        return "the telemetry endpoint is not a valid address"
    if not connectors.host_allowed(host):
        return f"the plugin policy allowed_hosts does not list {host}"
    return ""


def enabled() -> tuple[bool, str]:
    why = blocked()
    if why:
        return False, why
    if not key():
        return False, "no key, so nothing is sent (set one: careerbrew secret set posthog_key)"
    return True, "on"


def to_event(record: dict[str, Any], distinct_id: str) -> dict[str, Any] | None:
    """One log line as a PostHog event: allowlisted fields only, or None when the line is not usable."""
    name = record.get("event")
    if not isinstance(name, str) or not _ID.match(name):
        return None
    props: dict[str, Any] = {"$process_person_profile": False, "$geoip_disable": True, "app_version": __version__}
    for k, v in record.items():
        if k in ("event", "ts"):
            continue
        if k in STRING_KEYS and isinstance(v, str) and _ID.match(v):
            props[k] = v
        elif _NUM_KEY.match(k) and (isinstance(v, bool) or isinstance(v, int | float) and math.isfinite(v)):
            props[k] = v
        # Anything else (text, lists, other string keys) never leaves the machine.
    out: dict[str, Any] = {"event": name, "distinct_id": distinct_id, "properties": props}
    ts = record.get("ts")
    if isinstance(ts, str) and re.fullmatch(r"\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ", ts):
        out["timestamp"] = ts
    return out


def pending() -> tuple[list[dict[str, Any]], int]:
    """Events not sent yet (at most MAX_EVENTS_PER_SEND) and the log offset after them."""
    log = events.log_path()
    state = _state()
    offset = int(state.get("offset") or 0)
    if not log.exists():
        return [], 0
    data = log.read_bytes()
    if offset > len(data):  # the log was replaced or cut: start again
        offset = 0
    distinct = install_id()
    out: list[dict[str, Any]] = []
    pos = offset
    while pos < len(data) and len(out) < MAX_EVENTS_PER_SEND:
        end = data.find(b"\n", pos)
        if end == -1:
            break  # a line still being written
        try:
            record = json.loads(data[pos:end])
        except ValueError:
            record = None
        pos = end + 1
        ev = to_event(record, distinct) if isinstance(record, dict) else None
        if ev:
            out.append(ev)
    return out, pos


def body(batch: list[dict[str, Any]], api_key: str) -> dict[str, Any]:
    return {"api_key": api_key, "batch": batch}


def _post(payload: dict[str, Any]) -> None:
    connectors.check_host(endpoint())  # the same host policy as the connector calls
    req = urllib.request.Request(endpoint(), data=json.dumps(payload).encode(), headers={"Content-Type": "application/json", "User-Agent": f"careerbrew/{__version__}"})
    with connectors.open_url(req, timeout=10) as resp:
        if not 200 <= resp.status < 300:
            raise OSError(f"status {resp.status}")


def dry_run() -> dict[str, Any]:
    """The exact request the next send makes, with the key shown as a placeholder."""
    batch, _ = pending()
    return {"url": endpoint(), "body": body(batch[:BATCH_SIZE], "<posthog_key>"), "events_pending": len(batch)}


def send() -> dict[str, Any]:
    """Send every pending event now. Returns {sent, status}; never raises."""
    ok, why = enabled()
    if not ok:
        return {"sent": 0, "status": "off", "why": why}
    try:
        api_key = key()
        batch, end = pending()
        sent = 0
        for i in range(0, len(batch), BATCH_SIZE):
            _post(body(batch[i : i + BATCH_SIZE], api_key))
            sent += len(batch[i : i + BATCH_SIZE])
        state = _state()
        state.update(offset=end, last_send_day=calendar.now().date().isoformat())
        _save(state)
        events.emit("telemetry.send", status="ok", count=sent, story="us-89")
        return {"sent": sent, "status": "ok"}
    except Exception as exc:  # noqa: BLE001 (a send failure must never fail a command)
        events.emit("telemetry.send", status="fail", error=type(exc).__name__.lower(), story="us-89")
        return {"sent": 0, "status": "fail", "why": type(exc).__name__}


def maybe_send() -> dict[str, Any] | None:
    """For the watcher: at most once a day, only after the notice was shown, only when on."""
    try:
        state = _state()
        if not state.get("noticed") or state.get("last_send_day") == calendar.now().date().isoformat() or blocked():
            return None
        return send()
    except Exception:  # noqa: BLE001
        return None


def notice_once() -> str | None:
    """The one-line notice, the first time telemetry is on; None after that or when it is off."""
    try:
        state = _state()
        if state.get("noticed") or not enabled()[0]:
            return None
        state["noticed"] = True
        _save(state)
        return NOTICE
    except Exception:  # noqa: BLE001
        return None


def status() -> dict[str, Any]:
    ok, why = enabled()
    state = _state()
    batch, _ = pending()
    return {"on": ok, "why": why, "endpoint": endpoint(), "install_id": install_id(), "pending": len(batch), "last_send_day": state.get("last_send_day"), "noticed": bool(state.get("noticed"))}
