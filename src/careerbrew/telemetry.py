"""Field telemetry: anonymous usage counts from real runs, sent to PostHog EU (US-89).

This module is the only adapter between careerbrew's content-free event log
(`events.jsonl`, docs/observability.md) and the telemetry endpoint, so a move to a
self-hosted endpoint changes only this file. It mirrors agentbrew's field telemetry.

Consent: careerbrew asks its own one-time question (the `telemetry_consent` setting:
`unset`, `yes`, `no`; default `unset`). Nothing is sent unless the answer is `yes`. The
question shows only when `careerbrew` starts on a real terminal (never with no terminal:
an agent, a script, CI, the watcher). A yes covers only what happens after it: the log
cursor moves to the end, and no event stamped before the yes is ever sent. A corporate
machine defaults to `no`. An owner install from before the question (a Keychain key and
`telemetry` not false) counts as `yes`.

What leaves the machine: per event, its name (from a fixed list), a timestamp, a random
install id, the machine fields (app_version, os, arch, runtime), and allowlisted fields:
strings only from fixed lists in this file (command names from the CLI parser, statuses,
error classes, story ids, source kinds, tiers), numbers only when finite and within
bounds. Never content, titles, names, paths, the host name, or the user name.

Off switches, each stops every send: `DO_NOT_TRACK` (set, not 0), `CAREERBREW_TELEMETRY=0`,
`CI` (any value), `careerbrew telemetry off`, a plugin policy `telemetry: false`, and the
plugin host policy (`allowed_hosts`). The key: `CAREERBREW_POSTHOG_KEY` when set (an empty
value means no key), then the Keychain (`careerbrew secret set posthog_key`). No key is committed:
with none, a yes still records locally and nothing is sent. A value that is not one `phc_` key is no key.

Sends: batches of BATCH_SIZE, at most BACKLOG_EVENTS events from the last BACKLOG_DAYS days;
the cursor moves only after a 2xx answer; a lock keeps two sends from overlapping; the
watcher tries at most once a day with a short timeout. A failed send never fails a command.

Lifecycle: the install id, the log cursor, the consent time, the last send and attempt days,
the ask count, the saved install numbers, and the last recorded week live in
`<data root>/telemetry.json` (encrypted at rest; module scope, read and written per call).
Deleting the file makes a new install id; the cursor then starts at the end of the log.
"""

from __future__ import annotations

import builtins
import contextlib
import fcntl
import functools
import ipaddress
import json
import math
import os
import platform
import re
import sys
import time
import urllib.parse
import urllib.request
import uuid
from datetime import date, datetime, timedelta
from typing import Any

from . import __version__, calendar, config, connectors, events, paths, plugins, secrets, store

DEFAULT_HOST = "https://eu.i.posthog.com"
KEYCHAIN_SERVICE = "posthog_key"  # the Keychain entry name, not a value
NO_KEY = "no key: set one with `careerbrew secret set posthog_key`"  # the fix, never a value
KEY = re.compile(r"^phc_[A-Za-z0-9]+$")
BATCH_SIZE = 500
BACKLOG_EVENTS = 2000
BACKLOG_DAYS = 30
AUTO_TIMEOUT_S = 3
MANUAL_TIMEOUT_S = 10
MAX_ASKS = 3
MAX_NUMBER = 1e12  # a count or duration past this is not a real one
_VERSION = re.compile(r"^\d+\.\d+\.\d+$")
_TS = re.compile(r"^\d{4}-\d\d-\d\dT\d\d:\d\d:\d\dZ$")
_NUM_KEY = re.compile(r"^[a-z][a-z0-9_]{0,31}$")
_STORY = re.compile(r"^us-\d{1,3}$")  # a closed set of story ids: never text
CONSENT = ("unset", "yes", "no")

# Every event name careerbrew emits (test_telemetry.py checks this against each `events.emit` in the code).
EVENT_NAMES = frozenset(
    """approvals.decide approvals.request backup.create backup.restore calendar.error cards.add cards.publish cards.review
    changes.pivot checkup cli.error cli.run config.set connector.feed connector.reconnect connector.run data.encrypt data.set
    data.show day.cycle day.inbox day.inbox.accept day.inbox.answer day.inbox.expire day.inbox.group day.inbox.reopen
    day.inbox.suggest day.log day.plan day.pm.draft day.pm.learned day.pm.send day.source.error day.watch.reload
    day.watch.start day.watch.stop draft.fit draft.replace exercise.submit exercise.sync goals.update jobs.application
    jobs.choose jobs.learn jobs.linkedin.import jobs.rate jobs.source jobs.strategy learn.answer learn.concepts learn.replay
    learn.start learn.tour llm.call llm.unavailable machine.split_evidence mail.error mail.fetch metrics.weekly
    metrics.weekly.skip notify.act notify.error notify.remind notion.import plugin.add plugin.remove prep.check report.public
    report.write research.page review.run secret.missing signals.reset sync.export sync.import teach.feedback teach.step
    telemetry.send tutor.attempt tutor.voice web.api web.session web.timing web.token web.untracked web.warm work.agent.event
    work.claim work.collect work.collect.error work.explain work.feed.cycle work.feed.error work.feed.ingest work.feed.mark
    work.feed.pull work.feed.replies work.feed.research work.feed.retriage work.feed.team_search work.feed.transcript
    work.focus work.ladder_grade work.linkedin work.live.cycle work.live.error work.live.listen work.live.start
    work.live.stop work.memory.forget work.memory.in work.memory.out work.note work.plan work.prd work.prep work.priority
    work.project work.promise work.promo work.review_ask work.review_ask.check work.session work.standup
    work.standup.notes work.standup.rows work.task work.team workday.end workday.loop workday.morning workday.question
    workday.recheck workday.start workday.timeout workday.update workday.verify notify.check careerbrew.install careerbrew.week careerbrew.upgrade web.view
    web.act""".split()
)
STATUSES = frozenset(
    """ok error fail failed refused new done pass open kept held gap ended added written wrap-up waiting upcoming text
    stopped snoozed skipped sent reused renewed reloaded released pending on off not_cleared mismatch long_term keys hidden
    flagged expired dry_run claimed changed moved running blocked again hard good easy dream target practice
    missing_secret busy audio typed none emoji ack stop reply applied screen onsite offer rejected withdrawn""".split()
)
SOURCE_KINDS = frozenset(
    """agent plugin notify slack email calendar mail github gitlab jira linear notion confluence gmail outlook google
    pm-sent pm-receipt review readiness feed chat tickets docs hn greenhouse lever ashby linkedin rss""".split()
)
TIERS = frozenset({"personal", "corporate", "dream", "target", "practice"})
# The web app's views, by route name (`server._route`): a path parameter (a company, a page) is never part of the name.
VIEW_ROUTES = frozenset(
    """today focus feed.asks week daylog daylog.day work priority agents jobs interviews interviews.company overview inbox
    checklist standup actions review setup cards library library.page teach drill goals mistakes exercises reports
    reports.page promises""".split()
)
# A data-changing web control, by route name (`server._act_route`): the id in its path (an ask, a promise) never leaves.
WEB_ACTS = frozenset(
    """inbox.accept inbox.dismiss inbox.answer inbox.reopen inbox-group.accept inbox-group.dismiss notifications.read
    notifications.accept notifications.dismiss notifications.snooze promises.done promises.dropped actions.run pm.draft
    pm.send config drill.voice teach.complete teach.draft teach.feedback goals jobs.choose jobs.unchoose approvals.decide
    tasks tasks.done cards.review explanations.flag watch.on watch.off watch.pause watch.resume watch.reload""".split()
)
# What an event did (`action`): the fixed verbs careerbrew's own code emits; any other value never leaves.
ACTIONS = frozenset(
    """absorb accept add answer approve archive ask check close control dismiss done draft dropped end expire export
    extend gate handoff impact import list measure not-ask open plugin-roster pm-update quiz read reference refresh
    regrade remind remove reopen research rules set show since snooze source start status suggest track unarchive
    update""".split()
)
# Why a work-day step failed (`workday.gate.ERROR_CLASSES`), and where a command ran: a terminal, the web app, or an agent.
STEP_ERRORS = frozenset({"env", "auth", "network", "conflict", "tool"})
VIA = frozenset({"cli", "ui", "agent"})
# A work-day step is careerbrew's own (`workday.steps.BUILTIN_STEPS`) or one a plugin or repo declares; its name never leaves.
STEP_KINDS = frozenset({"builtin", "workflow"})
# Error classes: Python's built-in exceptions and careerbrew's own; any other class is sent as "other".
ERRORS = frozenset(
    {n.lower() for n, v in vars(builtins).items() if isinstance(v, type) and issubclass(v, BaseException)}
    | set("connectorerror secreterror missingsecreterror roleerror syncerror privacyerror pluginerror laddererror vaulterror shareerror gateerror invalidreceipt timeout other".split())
)
# The commands a person runs. Activity tiles count an install as active only on one of these (docs/telemetry-tiles.md);
# background work (the watcher, telemetry itself, the web app's reads) never counts.
USER_COMMANDS = frozenset(
    """start open review checklist day.log day.plan day.start day.end day.send day.check day.next cards.review cards.grade
    drill.session drill.answer exercises.submit teach teach.next jobs.scan jobs.log jobs.choose work.task work.collect
    work.standup work.note work.promo learn.start prep.check""".split()
)
WHAT = "the event name, the command, ok or error, the error class, durations and counts, the careerbrew version, your OS and CPU type, and a random install id"
NEVER = "content, titles, names, paths, arguments, the host name, or the user name; no IP-based location, no person profile"
WHERE = "PostHog EU (eu.i.posthog.com); the maintainer reads only totals and medians"
QUESTION_TEXT = f"""
careerbrew: share anonymous usage counts?
  What is sent: {WHAT}.
  What is never sent: {NEVER}.
  Where it goes: {WHERE}.
  See the exact request first: careerbrew telemetry send --dry-run
  Change it any time: careerbrew telemetry on | off (or DO_NOT_TRACK=1)
"""
QUESTION = "Share anonymous usage counts? [y/n] "


def _state_path():
    return paths.sub("telemetry.json")


def _state() -> dict[str, Any]:
    state = store.read_json(_state_path(), {}) or {}
    return state if isinstance(state, dict) else {}


def _save(state: dict[str, Any]) -> None:
    store.write_json_if_changed(_state_path(), state)


def _patch(**fields: Any) -> dict[str, Any]:
    """Read the state again, change some fields, save it, so a field another call wrote in the meantime stays."""
    state = _state()
    state.update(fields)
    _save(state)
    return state


def install_id() -> str:
    state = _state()
    if not re.fullmatch(r"[0-9a-f]{32}", str(state.get("install_id") or "")):
        state = _patch(install_id=uuid.uuid4().hex)
    return str(state["install_id"])


def endpoint() -> str:
    return os.environ.get("CAREERBREW_TELEMETRY_URL", DEFAULT_HOST).rstrip("/") + "/batch/"


def _keychain_key() -> str:
    try:
        value = secrets.resolve(f"keychain:{KEYCHAIN_SERVICE}")
    except secrets.SecretError:
        return ""
    return value if KEY.match(value or "") else ""


def resolve_key() -> tuple[str, str]:
    """(key, source): `CAREERBREW_POSTHOG_KEY` when set (an empty one means no key: tests use it), then the
    Keychain. Each source must hold one `phc_` key; anything else is no key."""
    if "CAREERBREW_POSTHOG_KEY" in os.environ:
        value = os.environ["CAREERBREW_POSTHOG_KEY"]
        return (value, "env") if KEY.match(value) else ("", "none")
    if value := _keychain_key():
        return value, "keychain"
    return "", "none"


def key() -> str:
    return resolve_key()[0]


def env_off() -> str:
    """The environment off switch that is set, or ""."""
    if os.environ.get("DO_NOT_TRACK", "").strip() not in ("", "0"):
        return "DO_NOT_TRACK is set"
    if os.environ.get("CAREERBREW_TELEMETRY", "").strip() == "0":
        return "CAREERBREW_TELEMETRY=0 is set"
    if "CI" in os.environ:
        return "CI is set"
    return ""


def _now_ts() -> str:
    return events.stamp()


def _role() -> str:
    try:
        from . import machine

        return machine.role()
    except Exception:  # noqa: BLE001 (an unreadable machine file reads as personal)
        return "personal"


def consent() -> str:
    """The answer that counts: the saved setting; else an old `telemetry: false` is no; else a corporate machine is no;
    else unset. Any state from before the question is unset too (as in agentbrew): the next terminal run asks.
    A read (`status`, `blocked`) never writes an answer."""
    saved = config._file()
    answer = saved.get("telemetry_consent")
    if answer in ("yes", "no"):
        if answer == "yes" and not _state().get("telemetry_consent_time"):
            _accept()  # a yes saved with `config set`: it still covers only what happens after it
        return str(answer)
    if saved.get("telemetry") is False:
        return "no"
    if _role() == "corporate":
        return "no"  # until a company plugin sets its own sink (task telemetry-plugin-sink)
    return "unset"


def _log_end() -> int:
    """The byte offset just after the last whole line of the event log."""
    log = events.log_path()
    if not log.exists():
        return 0
    data = log.read_bytes()
    return data.rfind(b"\n") + 1


def _accept() -> None:
    """Record the time of a yes and move the cursor to the end of the log: nothing from before the yes is sent."""
    _patch(telemetry_consent_time=_now_ts(), offset=_log_end())


def set_consent(answer: str) -> None:
    """Save the answer (`telemetry on`, `telemetry off`, the question). A new yes covers only what happens after it,
    and then emits the saved install numbers once (task telemetry-outcome-events)."""
    if answer not in ("yes", "no"):
        raise ValueError("use yes or no")
    was = config._file().get("telemetry_consent")
    config.set_value("telemetry_consent", answer)
    if answer == "yes":
        if was != "yes" or not _state().get("telemetry_consent_time"):
            _accept()
        _emit_install()


def _events_on() -> bool:
    return os.environ.get("CAREERBREW_EVENTS", "on") != "off"


def record_install(started_ms: int, ok: bool, now_ms: int | None = None) -> dict[str, Any] | None:
    """`telemetry record-install`: save setup's numbers locally (never an event: consent comes after setup).
    The first `ok: true` wins; a failure after a success changes nothing. Nothing in CI or tests."""
    if env_off() or not _events_on():
        return None
    now_ms = now_ms if now_ms is not None else int(events.utc_now().timestamp() * 1000)
    duration = now_ms - int(started_ms)
    if not 0 <= duration <= 86_400_000:
        raise ValueError("--started must be a time in the last day, in epoch milliseconds")
    state = _state()
    if state.get("install_sent") or (state.get("install") or {}).get("ok") is True:
        return state.get("install")
    install = {"duration_ms": duration, "ok": bool(ok)}
    _patch(install=install)
    if config._file().get("telemetry_consent") == "yes":
        _emit_install()
    return install


def _emit_install() -> None:
    """The one install event, stamped now (after the yes), with the saved numbers. Never twice."""
    state = _state()
    install = state.get("install")
    if state.get("install_sent") or not isinstance(install, dict) or env_off() or not _events_on():
        return
    duration = install.get("duration_ms")
    events.emit("careerbrew.install", duration_ms=duration if isinstance(duration, int) else 0, ok=install.get("ok") is True, tier=_role(), story="us-89")
    _patch(install_sent=True)


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
    """Why nothing may be sent (a switch, the answer, or the host policy), or ""."""
    if why := env_off():
        return why
    if any((p["manifest"].get("policy") or {}).get("telemetry") is False for p in plugins.installed()):
        return "a plugin policy forbids telemetry on this machine"
    answer = consent()
    if answer == "no":
        return "you said no; turn on: careerbrew telemetry on"
    if answer != "yes":
        return "not answered yet; turn on: careerbrew telemetry on"
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
        return False, NO_KEY
    return True, "on"


@functools.lru_cache(maxsize=1)
def command_names() -> frozenset[str]:
    """Every command name the CLI parser knows (`day.log`, `start`): the only command values that may leave."""
    import argparse

    from . import cli

    names: set[str] = set()
    for action in cli.build_parser()._actions:
        if isinstance(action, argparse._SubParsersAction):
            for name, sub in action.choices.items():
                names.add(name)
                for a in sub._actions:
                    if isinstance(a, argparse._SubParsersAction):
                        names.update(f"{name}.{s}" for s in a.choices)
    return frozenset(names)


def _string_ok(k: str, v: str) -> str | None:
    if k == "command":
        return v if v in command_names() else None
    if k == "status":
        return v if v in STATUSES else None
    if k == "story":
        return v if _STORY.match(v) else None
    if k == "error":
        return v.lower() if v.lower() in ERRORS else "other"
    if k == "source_kind":
        return v if v in SOURCE_KINDS else None
    if k == "tier":
        return v if v in TIERS else None
    if k == "route":
        return v if v in VIEW_ROUTES or v in WEB_ACTS else None
    if k == "action":
        return v if v in ACTIONS else None
    if k == "error_class":
        return v if v in STEP_ERRORS else None
    if k == "via":
        return v if v in VIA else None
    if k == "step_kind":
        return v if v in STEP_KINDS else None
    if k == "from_version":
        return v if _VERSION.match(v) else None
    return None


def machine_fields() -> dict[str, Any]:
    arch = platform.machine().lower()
    return {
        "app_version": __version__ if _VERSION.match(__version__) else "0.0.0",
        "os": sys.platform if sys.platform in ("darwin", "linux") else "other",
        "arch": {"arm64": "arm64", "aarch64": "arm64", "x86_64": "x64", "amd64": "x64"}.get(arch, "other"),
        "runtime": "python",
    }


def to_event(record: dict[str, Any], distinct_id: str, since: str = "") -> dict[str, Any] | None:
    """One log line as a PostHog event: allowlisted fields only, or None when the line may not leave.
    `since` is the earliest stamp that may leave (the time of the yes, or the backlog cap)."""
    name = record.get("event")
    ts = record.get("ts")
    if not isinstance(name, str) or name not in EVENT_NAMES:
        return None
    if not isinstance(ts, str) or not _TS.match(ts) or ts < since:
        return None  # no stamp, or one from before the yes: it never leaves
    props: dict[str, Any] = {}
    for k, v in record.items():
        if k in ("event", "ts"):
            continue
        if isinstance(v, str):
            if (value := _string_ok(k, v)) is not None:
                props[k] = value
        elif _NUM_KEY.match(k) and (isinstance(v, bool) or isinstance(v, int | float) and math.isfinite(v) and abs(v) <= MAX_NUMBER):
            props["duration_ms" if k == "ms" else k] = v
        # Anything else (text, lists, other keys, numbers out of bounds) never leaves the machine.
    props.update(machine_fields(), **{"$process_person_profile": False, "$geoip_disable": True})
    return {"event": name, "distinct_id": distinct_id, "timestamp": ts, "properties": props}


def _since() -> str:
    """The earliest stamp that may leave: the time of the yes, and no older than BACKLOG_DAYS."""
    cap = (datetime.fromisoformat(_now_ts().replace("Z", "+00:00")) - timedelta(days=BACKLOG_DAYS)).strftime("%Y-%m-%dT%H:%M:%SZ")
    return max(str(_state().get("telemetry_consent_time") or "9999"), cap)


def pending() -> tuple[list[tuple[dict[str, Any], int]], int]:
    """Events not sent yet, each with the log offset after its line (the newest BACKLOG_EVENTS), and the offset
    after the last whole line read. Nothing is pending before a yes."""
    log = events.log_path()
    state = _state()
    offset = int(state.get("offset") or 0)
    if not log.exists() or not state.get("telemetry_consent_time"):
        return [], offset
    data = log.read_bytes()
    if offset > len(data):  # the log was replaced or cut: start again (the stamps still keep pre-yes lines out)
        offset = 0
    distinct = install_id()
    since = _since()
    out: list[tuple[dict[str, Any], int]] = []
    pos = offset
    while pos < len(data):
        end = data.find(b"\n", pos)
        if end == -1:
            break  # a line still being written
        try:
            record = json.loads(data[pos:end])
        except ValueError:
            record = None
        pos = end + 1
        ev = to_event(record, distinct, since) if isinstance(record, dict) else None
        if ev:
            out.append((ev, pos))
    return out[-BACKLOG_EVENTS:], pos


def body(batch: list[dict[str, Any]], api_key: str) -> dict[str, Any]:
    return {"api_key": api_key, "batch": batch}


def _post(payload: dict[str, Any], timeout: float = MANUAL_TIMEOUT_S) -> None:
    connectors.check_host(endpoint())  # the same host policy as the connector calls
    req = urllib.request.Request(endpoint(), data=json.dumps(payload).encode(), headers={"Content-Type": "application/json", "User-Agent": f"careerbrew/{__version__}"})
    with connectors.open_url(req, timeout=timeout) as resp:
        if not 200 <= resp.status < 300:
            raise OSError(f"status {resp.status}")


def dry_run() -> dict[str, Any]:
    """The exact request the next send makes, with the key shown as a placeholder. Sends nothing."""
    items, _ = pending()
    batch = [ev for ev, _ in items]
    ok, why = enabled()
    return {"url": endpoint(), "body": body(batch[:BATCH_SIZE], "<posthog_key>"), "events_pending": len(batch), "would_send": ok, "why": why}


@contextlib.contextmanager
def _lock():
    """One send at a time on this data folder (an advisory file lock; the OS releases it if the process dies)."""
    path = paths.sub("telemetry.lock")
    with path.open("a") as fh:
        try:
            fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def _clock() -> float:
    return time.monotonic()


def send(timeout: float = MANUAL_TIMEOUT_S, total: float | None = None) -> dict[str, Any]:
    """Send every pending event now; with `total`, stop once that many seconds have passed in all (the rest waits).
    Returns {sent, status}; never raises."""
    try:
        ok, why = enabled()
        if not ok:
            return {"sent": 0, "status": "off", "why": why}
        with _lock() as held:
            if not held:
                return {"sent": 0, "status": "off", "why": "another send is running"}
            return _send_locked(timeout, None if total is None else _clock() + total)
    except Exception as exc:  # noqa: BLE001 (a send failure must never fail a command)
        with contextlib.suppress(Exception):
            events.emit("telemetry.send", status="fail", error=type(exc).__name__.lower(), story="us-89")
        return {"sent": 0, "status": "fail", "why": type(exc).__name__}


def _send_locked(timeout: float, deadline: float | None = None) -> dict[str, Any]:
    api_key = key()
    items, end = pending()  # read under the lock: a send that just ended has moved the cursor
    sent = 0
    for i in range(0, len(items), BATCH_SIZE):
        chunk = items[i : i + BATCH_SIZE]
        wait = timeout if deadline is None else min(timeout, deadline - _clock())
        if wait <= 0:
            break  # out of time: the cursor stays after the last batch sent, so the rest goes next time
        _post(body([ev for ev, _ in chunk], api_key), wait)
        sent += len(chunk)
        last = i + BATCH_SIZE >= len(items)
        _patch(offset=end if last else chunk[-1][1])  # the cursor moves only after a 2xx answer
    if not items:
        _patch(offset=end)
    _patch(last_send_day=calendar.now().date().isoformat())
    events.emit("telemetry.send", status="ok", count=sent, story="us-89")
    return {"sent": sent, "status": "ok"}


def _week_bounds(today: date) -> tuple[date, date, str]:
    """The last finished ISO week: (Monday, next Monday, `YYYY-Www`)."""
    start = today - timedelta(days=today.weekday() + 7)
    y, w, _ = start.isocalendar()
    return start, start + timedelta(days=7), f"{y}-W{w:02d}"


def week_counts(start: date, end: date) -> dict[str, int]:
    """Numbers only, for the ISO week [start, end): each from the same local records `careerbrew review` and the
    weekly metrics file read. A number that cannot be read is left out, never guessed."""
    from .metrics_share import _in_week

    out: dict[str, int] = {}

    def count(name: str, fn) -> None:
        with contextlib.suppress(Exception):
            out[name] = int(fn())

    def active_days() -> int:
        rows = store.read_jsonl(events.log_path())
        return len({r["ts"][:10] for r in rows if r.get("event") == "cli.run" and r.get("command") in USER_COMMANDS and _in_week(r.get("ts", ""), start, end)})

    def cards() -> int:
        from .tutor import cards as c

        return sum(1 for r in c.reviews() if _in_week(r.get("at", ""), start, end))

    def exercises() -> int:
        from .tutor import exercises as ex

        return len({r.get("exercise") for r in ex.attempts() if r.get("correct") and _in_week(r.get("at", ""), start, end)})

    def work_days() -> int:
        days = paths.sub("day", ".keep").parent
        return sum(1 for p in days.glob("work-????-??-??.json") if start.isoformat() <= p.stem[5:] < end.isoformat())

    def evidence_items() -> int:
        from .work import evidence

        return sum(1 for e in evidence.load() if _in_week(e.get("at", ""), start, end))

    count("days_active", active_days)
    count("cards_reviewed", cards)
    count("exercises_solved", exercises)
    count("work_days_started", work_days)
    count("evidence_items", evidence_items)
    return out


def _record_week() -> None:
    """Write `careerbrew.week` for the last finished ISO week, once per install, before the send (so a failed send
    neither loses the week nor records it twice). A week that ended before the yes is never recorded."""
    start, end, week = _week_bounds(calendar.now().date())
    state = _state()
    yes_day = str(state.get("telemetry_consent_time") or "9999")[:10]
    if state.get("week_recorded") == week or end.isoformat() <= yes_day or not _events_on():
        return
    counts = week_counts(start, end)  # a missing number is None, which emit drops
    events.emit(
        "careerbrew.week",
        story="us-89",
        days_active=counts.get("days_active"),
        cards_reviewed=counts.get("cards_reviewed"),
        exercises_solved=counts.get("exercises_solved"),
        work_days_started=counts.get("work_days_started"),
        evidence_items=counts.get("evidence_items"),
    )
    _patch(week_recorded=week)


def _record_upgrade() -> None:
    """`careerbrew.upgrade` once when the careerbrew version changes; the first version seen is only the baseline."""
    seen = _state().get("last_version")
    if seen == __version__ or not _VERSION.match(__version__):
        return
    if isinstance(seen, str) and _VERSION.match(seen) and _events_on():
        events.emit("careerbrew.upgrade", from_version=seen, story="us-89")
    _patch(last_version=__version__)


def maybe_send() -> dict[str, Any] | None:
    """After an ordinary command and in the watcher: at most one try a day (a dead network costs one try), only after
    a yes and with a key, AUTO_TIMEOUT_S in all. Never raises."""
    try:
        today = calendar.now().date().isoformat()
        if _state().get("last_attempt_day") == today or blocked():
            return None
        _patch(last_attempt_day=today)  # before the key: a missing key costs one Keychain look a day
        if not key():
            return None
        _record_upgrade()
        _record_week()
        return send(timeout=AUTO_TIMEOUT_S, total=AUTO_TIMEOUT_S)
    except Exception:  # noqa: BLE001
        return None


_views_seen: set[tuple[str, str]] = set()  # module scope (one per process): the views already noted today


def note_view(route: str) -> None:
    """`web.view` for a view the web app served, once per view per day in this process. The route is a fixed name."""
    try:
        today = calendar.now().date().isoformat()
        if route not in VIEW_ROUTES or (route, today) in _views_seen:
            return
        _views_seen.add((route, today))
        events.emit("web.view", route=route, story="us-89")
    except Exception:  # noqa: BLE001 (a log line must never fail a request)
        return


def _interactive() -> bool:
    return all(getattr(s, "isatty", lambda: False)() for s in (sys.stdin, sys.stdout, sys.stderr))


def ask_on_start(interactive: bool | None = None, read=None, write=None) -> str | None:
    """The one-time question, when `careerbrew` starts on a real terminal and the answer is unset. Never with no
    terminal, in CI, with an off switch, or with no key. At most once a day and MAX_ASKS times. Never raises."""
    try:
        interactive = _interactive() if interactive is None else interactive
        if not interactive or env_off() or consent() != "unset" or not key():
            return None
        with _ask_lock() as held:
            if not held:
                return None  # another terminal is asking now
            return _ask_locked(read, write)
    except Exception:  # noqa: BLE001 (the question must never fail a command)
        return None


@contextlib.contextmanager
def _ask_lock():
    """One question at a time on this data folder, so two terminals never both ask (the OS releases it if one dies)."""
    with paths.sub("telemetry-ask.lock").open("a") as fh:
        try:
            fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except OSError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def _ask_locked(read, write) -> str | None:
    """The question itself, under the ask lock: the answer and the day are read again here."""
    if consent() != "unset":
        return None
    state = _state()
    today = calendar.now().date().isoformat()
    if state.get("asked_day") == today or int(state.get("ask_count") or 0) >= MAX_ASKS:
        return None
    _patch(asked_day=today, ask_count=int(state.get("ask_count") or 0) + 1)
    write = write or (lambda text: print(text, end="", file=sys.stderr, flush=True))
    read = read or sys.stdin.readline
    write(QUESTION_TEXT)
    for _ in range(3):
        write(QUESTION)
        answer = (read() or "").strip().lower()
        if answer in ("y", "yes"):
            set_consent("yes")
            write("Thank you. Telemetry is on. Change it any time: careerbrew telemetry off\n")
            return "yes"
        if answer in ("n", "no"):
            set_consent("no")
            write("Telemetry is off. Nothing is sent. Change it any time: careerbrew telemetry on\n")
            return "no"
        if not answer:
            break
    write("No answer. Nothing is sent. Answer later: careerbrew telemetry on | off\n")
    return None


def brief() -> str:
    """One line for the checkup page, which an agent may publish: on or off, the answer, the events waiting, and the
    last send day. Never the install id or the endpoint, and it makes no install id."""
    ok, why = enabled()
    state = _state()
    waiting = len(pending()[0]) if state.get("install_id") else 0
    return f"telemetry: {'on' if ok else 'off'} ({why}); answer: {consent()}; pending: {waiting} event(s); last send: {state.get('last_send_day') or 'never'}"


def status() -> dict[str, Any]:
    ok, why = enabled()
    state = _state()
    items, _ = pending()
    return {
        "on": ok,
        "why": why,
        "consent": consent(),
        "consent_time": state.get("telemetry_consent_time"),
        "key_source": resolve_key()[1],
        "endpoint": endpoint(),
        "install_id": install_id(),
        "pending": len(items),
        "last_send_day": state.get("last_send_day"),
    }
