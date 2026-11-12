"""Read-only calendar facts for daily planning: Outlook (Microsoft Graph) and ICS feeds.

Config: `<data root>/day/calendars.yaml`, plus any `outlook` connector (work/connectors.yaml)
  calendars:
    - {kind: ics, url: keychain:careerbrew.ics.personal}      # private ICS link, kept as a secret
    - {kind: ics, path: ~/Downloads/work.ics}                 # or a local file
    - {kind: graph, token: keychain:careerbrew.graph}         # Calendars.Read, delegated

Only the start, end, and a busy flag matter for planning. Titles are kept in
memory for "prepare for <meeting>" suggestions and never logged.
Recurring events are expanded with `recurring-ical-events`.
"""

from __future__ import annotations

import os
import urllib.request
from collections.abc import Iterable
from dataclasses import dataclass
from datetime import UTC, date, datetime, time, timedelta
from pathlib import Path
from typing import Any
from zoneinfo import ZoneInfo

import icalendar
import recurring_ical_events

from . import connectors, events, msauth, paths, secrets, store


@dataclass(frozen=True)
class Event:
    start: datetime
    end: datetime
    title: str
    busy: bool = True
    personal: bool = False  # a private entry (Outlook sensitivity, ICS CLASS:PRIVATE): busy time, never in the day log or PM update
    uid: str = ""  # the work feed's id for the same meeting (`outlook:<id>`), so the owner's personal mark applies
    led: bool = False  # the owner organized it (Graph isOrganizer; ICS ORGANIZER is one of the calendar's `me` addresses; issue #239)
    recurring: bool = False  # one occurrence of a series (Graph type; ICS RRULE or RECURRENCE-ID): a led series is no news


def local_tz() -> ZoneInfo:
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    return ZoneInfo(settings.get("timezone") or system_tz())


def now() -> datetime:
    """Local time; `CAREERBREW_NOW=2026-10-19T10:00` pins it for tests and e2e runs."""
    pinned = os.environ.get("CAREERBREW_NOW")
    if pinned:
        dt = datetime.fromisoformat(pinned)
        return dt if dt.tzinfo else dt.replace(tzinfo=local_tz())
    return datetime.now(local_tz())


def now_utc() -> datetime:
    """`now()` in UTC, for stamps stored in UTC: the same pinned clock, the same stored format."""
    return now().astimezone(UTC)


def _day_bounds(day: date, tz: ZoneInfo) -> tuple[datetime, datetime]:
    start = datetime.combine(day, time(0, 0), tz)
    return start, start + timedelta(days=1)


def _aware(value: Any, tz: ZoneInfo) -> datetime:
    if isinstance(value, datetime):
        return value if value.tzinfo else value.replace(tzinfo=tz)
    return datetime.combine(value, time(0, 0), tz)  # all-day event


def parse_ics(text: str, day: date, tz: ZoneInfo, me: Iterable[str] = ()) -> list[Event]:
    cal = icalendar.Calendar.from_ical(text)
    mine = {str(m).strip().lower() for m in me if str(m).strip()}
    # The expander drops RRULE and stamps RECURRENCE-ID on every occurrence, so a series is known from the source.
    series = {str(c.get("UID")) for c in cal.walk("VEVENT") if c.get("RRULE") or c.get("RECURRENCE-ID")}
    start, end = _day_bounds(day, tz)
    out = []
    for comp in recurring_ical_events.of(cal).between(start, end):
        s, e = comp.get("DTSTART").dt, comp.get("DTEND").dt if comp.get("DTEND") else None
        all_day = not isinstance(s, datetime)
        s_dt = _aware(s, tz)
        e_dt = _aware(e, tz) if e is not None else s_dt + timedelta(hours=1)
        transp = str(comp.get("TRANSP", "OPAQUE")).upper()
        private = str(comp.get("CLASS", "PUBLIC")).upper() == "PRIVATE"
        organizer = str(comp.get("ORGANIZER", "")).lower().removeprefix("mailto:")
        recurring = str(comp.get("UID")) in series
        out.append(Event(s_dt.astimezone(tz), e_dt.astimezone(tz), str(comp.get("SUMMARY", "")), busy=not all_day and transp != "TRANSPARENT", personal=private, led=bool(mine) and organizer in mine, recurring=recurring))
    return out


def _ics_text(cfg: dict[str, Any]) -> str:
    """A local `path`, or a `url` that is a secret reference (a private ICS link grants read access)."""
    if cfg.get("path"):
        return Path(cfg["path"]).expanduser().read_text(encoding="utf-8")
    url = secrets.resolve(cfg["url"])  # rejects a literal link: it must be keychain:/op:/env:
    if url.startswith("file://"):
        return Path(url[7:]).read_text(encoding="utf-8")
    url = url.replace("webcal://", "https://", 1)
    connectors.check_host(url)
    req = urllib.request.Request(url, headers={"User-Agent": "careerbrew"})
    with connectors.open_url(req, timeout=30) as resp:
        return resp.read().decode("utf-8", "replace")


def graph_events(cfg: dict[str, Any], day: date, tz: ZoneInfo) -> list[Event]:
    tok = msauth.bearer(cfg)
    start, end = _day_bounds(day, tz)
    api = cfg.get("api", "https://graph.microsoft.com/v1.0").rstrip("/")
    url = f"{api}/me/calendarView?startDateTime={start.astimezone(UTC).strftime('%Y-%m-%dT%H:%M:%SZ')}&endDateTime={end.astimezone(UTC).strftime('%Y-%m-%dT%H:%M:%SZ')}&$select=id,subject,start,end,showAs,isAllDay,sensitivity,isOrganizer,type&$top=100"
    data = connectors.http_json(url, {"Authorization": f"Bearer {tok}", "Prefer": 'outlook.timezone="UTC"'})
    out = []
    for ev in data.get("value", []):
        s = datetime.fromisoformat(ev["start"]["dateTime"].rstrip("Z")[:26]).replace(tzinfo=UTC).astimezone(tz)
        e = datetime.fromisoformat(ev["end"]["dateTime"].rstrip("Z")[:26]).replace(tzinfo=UTC).astimezone(tz)
        busy = ev.get("showAs", "busy") not in ("free", "workingElsewhere") and not ev.get("isAllDay")
        out.append(
            Event(
                s,
                e,
                ev.get("subject", ""),
                busy,
                personal=ev.get("sensitivity") in connectors.PERSONAL_SENSITIVITY,
                uid=f"outlook:{ev['id']}" if ev.get("id") else "",
                led=bool(ev.get("isOrganizer")),
                recurring=ev.get("type") in ("occurrence", "exception"),
            )
        )
    return out


def configured() -> list[dict[str, Any]]:
    """calendars.yaml, plus an `outlook` connector's calendar (issue #63) unless the same token is already listed."""
    cals = list((store.read_yaml(paths.data_root() / "day" / "calendars.yaml", {}) or {}).get("calendars", []))
    have = {c.get("token") for c in cals if c.get("kind") == "graph"}
    return cals + [g for g in connectors.graph_sources("calendar") if g["token"] not in have]


def events_for(day: date) -> tuple[list[Event], list[str]]:
    """All events for a day across calendars, plus content-free error notes."""
    tz = local_tz()
    out: list[Event] = []
    errors: list[str] = []
    for cfg in configured():
        try:
            if cfg["kind"] == "ics":
                out.extend(parse_ics(_ics_text(cfg), day, tz, cfg.get("me") or ()))
            elif cfg["kind"] == "graph":
                out.extend(graph_events(cfg, day, tz))
        except (OSError, ValueError, KeyError, RuntimeError) as exc:  # ConnectorError is a RuntimeError
            errors.append(msauth.RECONNECT if isinstance(exc, msauth.ReconnectNeeded) else f"{cfg.get('kind')}: {type(exc).__name__}")
            events.emit("calendar.error", source_kind=str(cfg.get("kind", "?")), error=type(exc).__name__)
    return sorted(out, key=lambda e: e.start), errors


_warned_no_zone = False  # module scope: one warning per process


def system_tz() -> str:
    """The machine's IANA zone name, used when the `timezone` setting is empty: `TZ`, else the /etc/localtime link, else UTC."""
    import sys

    global _warned_no_zone
    name = os.environ.get("TZ", "").lstrip(":")
    if not name:
        link = os.path.realpath("/etc/localtime")
        name = link.split("/zoneinfo/", 1)[1] if "/zoneinfo/" in link else ""
    try:
        ZoneInfo(name)
    except (KeyError, ValueError, OSError):
        if not _warned_no_zone:
            _warned_no_zone = True
            print("careerbrew: cannot read this machine's time zone, so times use UTC; fix: careerbrew config set timezone <IANA name, for example Etc/UTC>", file=sys.stderr)
        return "UTC"
    return name
