"""Settings with sane defaults: one table, one file, one command for people and agents.

Every setting careerbrew reads has a default here. Overrides live in `<data root>/settings.yaml`
(encrypted at rest). `careerbrew config` shows each value with its source (default or file);
`careerbrew config set <key> <value>` validates the value and writes the file, so an agent can
change a setting the same way a person does. `config unset <key>` returns to the default.
"""

from __future__ import annotations

import json
import re
from typing import Any

from . import events, paths, store


def _hhmm(v: str) -> str:
    if not re.fullmatch(r"([01]\d|2[0-3]):[0-5]\d", v):
        raise ValueError("use HH:MM, for example 09:00")
    return v


def _bool(v: str) -> bool:
    if v.lower() in ("true", "yes", "on", "1"):
        return True
    if v.lower() in ("false", "no", "off", "0"):
        return False
    raise ValueError("use true or false")


def _int(lo: int, hi: int):
    def parse(v: str) -> int:
        n = int(v)
        if not lo <= n <= hi:
            raise ValueError(f"use a whole number from {lo} to {hi}")
        return n

    return parse


def _days(v: str) -> list[int]:
    days = sorted({int(x) for x in v.replace(" ", "").split(",") if x != ""})
    if not days or any(not 0 <= d <= 6 for d in days):
        raise ValueError("use weekday numbers 0-6 (Monday=0), comma-separated, for example 0,1,2,3,4")
    return days


def _list(v: str) -> list[str]:
    return [x.strip() for x in v.split(",") if x.strip()]


def _email(v: str) -> str:
    if v and not re.fullmatch(r"[^@\s]+@[^@\s]+\.[^@\s]+", v):
        raise ValueError("use an email address, or empty")
    return v


def _text(v: str) -> str:
    if not v or len(v) > 120 or "\n" in v:
        raise ValueError("use one line of at most 120 characters")
    return v


def _priority(v: str) -> list[str]:
    """`name=glob|glob, name, ...` in rank order (rank 1 first); a bare name is its own pattern; empty clears it (issue #68)."""
    out: list[str] = []
    for raw in (x.strip() for x in v.split(",")):
        if not raw:
            continue
        name, eq, pats = (s.strip() for s in raw.partition("="))
        globs = [g.strip() for g in pats.split("|") if g.strip()]
        if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,39}", name):
            raise ValueError(f"project {name!r}: use lowercase letters, digits, and dashes (max 40), for example tools=*/repo-a|*/repo-b")
        if any(x.split("=", 1)[0] == name for x in out):
            raise ValueError(f"project {name} is listed twice; each project has one rank")
        if eq and not globs:
            raise ValueError(f"project {name}: no pattern after '=' (fix: {name}=PROJ-* or just {name})")
        if any(re.search(r"\s|=", g) or len(g) > 80 for g in globs):
            raise ValueError(f"project {name}: a pattern has no spaces or '=' and at most 80 characters")
        out.append(name + (f"={'|'.join(globs)}" if globs else ""))
    if len(out) > 20:
        raise ValueError("at most 20 projects")
    return out


def _stt(v: str) -> str:
    if v and "{audio}" not in v:
        raise ValueError("the command needs an {audio} placeholder, for example: whisper-cli -nt -np -m ~/models/ggml-base.en.bin -f {audio}")
    return v


def _quiet(v: str) -> str:
    """`HH:MM-HH:MM` local time; it may wrap past midnight (19:00-08:00). Empty, none, or off: no quiet hours (issue #74)."""
    v = v.strip()
    if v.lower() in ("", "none", "off"):
        return ""
    hhmm = r"([01]\d|2[0-3]):[0-5]\d"
    m = re.fullmatch(rf"({hhmm})-({hhmm})", v)
    if not m:
        raise ValueError("use HH:MM-HH:MM, for example 19:00-08:00 (or none for no quiet hours)")
    if m.group(1) == m.group(3):
        raise ValueError("the start and the end are the same time; use two different times, or none for no quiet hours")
    return v


def _tz(v: str) -> str:
    """An IANA time zone name, or empty for the `timezone` setting (issue #110)."""
    v = v.strip()
    if v:
        from zoneinfo import ZoneInfo, ZoneInfoNotFoundError

        try:
            ZoneInfo(v)
        except (ZoneInfoNotFoundError, ValueError):
            raise ValueError("use an IANA time zone such as Europe/Berlin, or empty for the timezone setting") from None
    return v


def _artifact_url(v: str) -> str:
    if v and not re.fullmatch(r"https://claude\.ai/(?:code/)?artifact/[A-Za-z0-9-]{8,64}", v):
        raise ValueError("use a claude.ai artifact link, for example https://claude.ai/artifact/AbC123xyz, or empty")
    return v


def _tool_sources(v: str) -> list[str]:
    """`glob=source, ...`: which MCP tool's replies in an agent transcript go to which feed source (issue #234)."""
    out: list[str] = []
    for raw in (x.strip() for x in v.split(",")):
        if not raw:
            continue
        glob, eq, src = (s.strip() for s in raw.partition("="))
        if not eq or not glob or not src:
            raise ValueError(f"{raw[:40]!r}: use glob=source, for example mcp__chat__*=chat")
        if not re.fullmatch(r"[a-z0-9][a-z0-9._-]{0,39}", src):
            raise ValueError(f"source {src[:40]!r} must be a short lowercase name such as chat or tickets")
        out.append(f"{glob}={src}")
    return out


def _work_steps(v: str) -> list[str | dict[str, Any]]:
    """Owner step names, or a JSON list retaining validated scheduling fields."""
    from .workday import steps

    try:
        raw = json.loads(v) if v.lstrip().startswith("[") else _list(v)
        checked = steps.validate(raw, "settings")
        return [dict(item) if isinstance(item, dict) else row["step"] for item, row in zip(raw, checked, strict=True)]
    except (steps.StepError, json.JSONDecodeError, RecursionError) as e:
        raise ValueError(str(e)) from None


def _work_checkouts(v: str) -> list[str]:
    """`careerbrew, ~/apps/notes`: the checkouts `careerbrew day start` fast-forwards besides plugins and step sources (issue #260)."""
    from .workday import start

    return start.check_checkouts(_list(v))


def _signin_probe(v: str) -> str:
    """One command whose exit code says the shared browser is signed in (issue #260); empty: no probe."""
    from .workday import start

    return start.check_probe(v)


def _choice(*options: str):
    def parse(v: str) -> str:
        if v not in options:
            raise ValueError(f"use one of {', '.join(options)}")
        return v

    return parse


# key: (default, parser, help)
DEFAULTS: dict[str, tuple[Any, Any, str]] = {
    "ladder": ("generic-swe", _text, "promotion ladder id (a company plugin sets its own)"),
    "target_level": ("senior", _text, "the level you are working toward, a level of the ladder"),
    "work_start": ("09:00", _hhmm, "work day start, local time"),
    "work_end": ("17:30", _hhmm, "work day end, local time"),
    "work_wrap_minutes": (
        30,
        _int(0, 240),
        "the /work day switches to wrap-up this many minutes before work_end: the loops keep running, tomorrow's handoff is written, and one reminder pops up (with notify_events on); at work_end the day ends by itself",
    ),
    "work_days": ([0, 1, 2, 3, 4], _days, "workdays, Monday=0"),
    "timezone": ("", lambda v: v, "IANA time zone; empty means the system one"),
    "backup_targets": ([], _list, "backup folders, comma-separated (empty: ~/Backups/careerbrew)"),
    "backup_icloud": (False, _bool, "also write each encrypted backup to iCloud Drive/careerbrew (opt-in; refused on a corporate machine)"),
    "serve_port": (8765, _int(1024, 65535), "port of the local web app"),
    "web_cache_seconds": (300, _int(0, 3600), "web views answer a repeat from memory until a data file, a setting, or the clock changes, at most this long (calendar feeds); 0: off"),
    "web_token_max_days": (30, _int(1, 365), "the web app's sign-in token lasts this many days across restarts, then the next `careerbrew open` makes a new one (`careerbrew web rotate-token` does it now)"),
    "web_tab_cache": (True, _bool, "each browser tab keeps the last data of every view it showed, encrypted with a key only the running app holds, so a reload shows it at once (off: memory only, a reload loads again)"),
    "start_watch": (True, _bool, "`careerbrew` with no arguments turns the day watcher on"),
    "start_web": (True, _bool, "`careerbrew` with no arguments starts the web app in the background"),
    "questions_per_day": (5, _int(0, 50), "most open questions careerbrew asks at once"),
    "inbox_group_min": (3, _int(2, 100), "this many or more alike suggestions (same kind, project, and source) show as one group with Accept all, Review each, and Dismiss all"),
    "inbox_expire_done": (True, _bool, "hide a suggestion when data careerbrew already reads shows it done elsewhere (a PR merged, a review you gave, CI green); an unsure match only says maybe done; Bring back undoes it"),
    "first_run_checklist": (True, _bool, "show the Getting started checklist on Dashboard until every step is done (false: hide it)"),
    "run_pages": (
        True,
        _bool,
        "long runs (scripts/checkup.sh, `careerbrew`, review, work collect, day plan, day render, checkup, machine ready, machine new-user, plugin new) end with an HTML page (done, blockers, next) and print only its path and one summary line (off: their step text)",
    ),
    "pm_report_title": ("Daily update", _text, "subject and first line of the PM report"),
    "metrics_share": (False, _bool, "write a weekly numbers-only metrics file for agentbrew's metrics share (opt-in)"),
    "telemetry": (True, _bool, "send anonymous usage counts (no content) to PostHog EU once a day; sends only when a key is set (careerbrew secret set posthog_key); DO_NOT_TRACK=1 also stops it"),
    "checkup_artifact_url": ("", _artifact_url, "this machine's repo health Dashboard on claude.ai: /careerbrew-checkup refreshes it from `careerbrew report public` (empty: the next checkup creates one)"),
    "pm_send_via": ("slack", lambda v: v if v in ("slack", "email") else (_ for _ in ()).throw(ValueError("use slack or email")), "how Send delivers the PM update: slack (posts it) or email (opens a draft)"),
    "pm_slack": ("", _text, "Slack channel or person for the PM update: a channel id, #channel, or a member id (U...)"),
    "pm_email": ("", _email, "your PM's address when pm_send_via is email: Send opens a draft to it"),
    "project_priority": ([], _priority, "projects by priority, rank 1 first: name=glob|glob, ... (globs match repos, ticket keys, titles); the PM update, plan, and Next step follow it"),
    "feed_keep_days": (30, _int(1, 365), "days the work feed keeps each original event (encrypted); its analysis (triage, personal flag) stays"),
    "feed_tool_sources": ([], _tool_sources, "MCP tools whose replies `work feed transcript` stores from an agent session, glob=source, comma-separated (for example mcp__chat__*=chat, mcp__tickets__*=tickets)"),
    "live_source_max_age_minutes": (15, _int(1, 10080), "maximum acquisition age in minutes for a validated live-source reply; a source max_age_minutes overrides it"),
    "live_open_links": (False, _bool, "the live cycle opens the links in kept events to summarize them (off: each link waits for your agent); never private or local addresses, at most 1 MB per page"),
    "feed_ask_overdue_hours": (8, _int(1, 168), "work hours (work_start to work_end on work_days) an open ask waits before Today and `work feed show` mark it overdue"),
    "project_memory_sync": (True, _bool, "a project's memory file edit updates the project (links, trackers, status), and matching work-tool items go into it as dated notes"),
    "stt_command": ("", _stt, "local speech-to-text command for voice practice; {audio} is the file, stdout is the transcript (empty: whisper-cli with stt_model)"),
    "stt_model": ("", lambda v: v, "path to a whisper.cpp model file (ggml-*.bin) used when stt_command is empty"),
    "notify_events": (False, _bool, "notify me about important new events: pages, direct messages, review requests, mentions, high-priority suggestions (opt-in)"),
    "notify_min_priority": (4, _int(1, 5), "notify only at or above this priority: 5 a page on you, 4 a DM or review request, 3 a mention, important mail, or suggestion (+1 for your rank-1 project)"),
    "notify_quiet_hours": ("19:00-08:00", _quiet, "no pop-ups between these local times, HH:MM-HH:MM (none: no quiet hours); held ones arrive together after"),
    "notify_daily_cap": (6, _int(0, 50), "most pop-ups a day; past it, new ones wait in the web app's Notifications view"),
    "notify_batch_minutes": (10, _int(0, 240), "new events within this many minutes of the last pop-up arrive together in the next one"),
    "notify_snooze_minutes": (60, _int(5, 1440), "Snooze hides a notification for this many minutes, then it comes back"),
    "notify_backend": ("auto", _choice("auto", "alerter", "terminal-notifier", "osascript"), "how pop-ups show: auto picks alerter (buttons), then terminal-notifier (click opens), then osascript (plain)"),
    "standup_days": ([0, 1, 2, 3, 4], _days, "standup weekdays, Monday=0; a Monday report counts from the last standup day of the week before"),
    "standup_time": ("10:00", _hhmm, "standup start, HH:MM in standup_tz"),
    "standup_tz": ("", _tz, "IANA time zone of the standup and of evidence times with no offset (empty: the timezone setting)"),
    "standup_window": ("previous-day", _choice("previous-day", "since-last"), "where the standup report starts: previous-day (the previous standup day's standup, all day long) or since-last (the last standup that has ended, today's too)"),
    "standup_minutes": (15, _int(1, 240), "standup length; a report run during the standup still counts from the one before"),
    "standup_proof_minutes": (60, _int(1, 1440), "how old a live proof passed with `work standup --proof` may be at each build; an older one fails its row (issue #271)"),
    "agent_quiet_minutes": (60, _int(1, 1440), "a background agent's run whose last `work agent event` is running and older than this shows as quiet in Agent activity (issue #272)"),
    "work_session_cap": (3, _int(1, 10), "most project sessions live at once on /work (`work session open`); one per project, reused all day (issue #274)"),
    "work_claim_minutes": (30, _int(1, 480), "minutes a project session's claim on a shared thread or draft lasts after it was last taken (`work claim take`, issue #274)"),
    "agent_keep_days": (7, _int(1, 90), "days Agent activity keeps each background agent's events (`work agent event`, issue #272)"),
    "outlook_client_id": ("", _text, "the app registration (client ID) `careerbrew connect outlook` signs in with: your company's approved app; empty: a company plugin's connector_defaults.outlook.client_id"),
    "outlook_tenant": ("", _text, "the Microsoft tenant for Outlook sign-in (a tenant ID or domain; empty: the plugin's, else organizations)"),
    "work_steps": ([], _work_steps, "your own `/work` steps before plugin steps: comma-separated names or a JSON list of names and mappings (docs/plugins.md)"),
    "work_step_timeout": (30, _int(1, 1440), "minutes a `/work` step may run; `careerbrew day check` then retries it once, and marks it failed after the second timeout (a plugin step's `timeout:` overrides it)"),
    "work_slow_percent": (200, _int(110, 1000), "a finished `/work` step that took more than this percent of its median (its done runs on earlier days) is flagged slow on the day's page (issue #263)"),
    "work_checkouts": (
        ["careerbrew"],
        _work_checkouts,
        "checkouts `careerbrew day start` fast-forwards before the first `/work` step, besides each plugin's folder and each step's source: careerbrew (its own checkout) and full paths, comma-separated",
    ),
    "work_signin_probe": (
        "",
        _signin_probe,
        "a command `careerbrew day start` runs to check the shared browser is signed in (exit 0: signed in); when it fails, the day gets one sign-in question and the sign-in steps run last (empty: no probe)",
    ),
    "standup_me": ([], _list, "names, handles, or emails that mean you in a standup row's owner (owner `me` always does); your rows show when they have an update since the last standup"),
}
# Manual minutes per action that you timed by hand (`careerbrew time baseline <action> <minutes>`); 0: use the
# labeled estimate in time_saved.ACTIONS. The ids match time_saved.ACTIONS (test_time_saved.py).
for _action in ("pm_update", "standup", "day_log", "day_plan", "evidence", "promo_doc", "job_scan", "inbox"):
    DEFAULTS[f"time_baseline_{_action}"] = (0, _int(0, 600), f"minutes {_action} takes you by hand, timed once (0: use the labeled estimate)")


def _file() -> dict[str, Any]:
    return store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}


def get(key: str) -> Any:
    if key not in DEFAULTS:
        raise KeyError(f"unknown setting {key}; see `careerbrew config`")
    return _file().get(key, DEFAULTS[key][0])


def effective() -> list[dict[str, Any]]:
    data = _file()
    rows = [{"key": k, "value": data.get(k, d), "source": "settings.yaml" if k in data else "default", "default": d, "help": h} for k, (d, _p, h) in DEFAULTS.items()]
    rows += [{"key": k, "value": v, "source": "settings.yaml", "default": None, "help": "not a known setting (kept as is)"} for k, v in data.items() if k not in DEFAULTS]
    return rows


def set_value(key: str, raw: str) -> Any:
    if key not in DEFAULTS:
        raise KeyError(f"unknown setting {key}; known: {', '.join(DEFAULTS)}")
    try:
        value = DEFAULTS[key][1](raw)
    except ValueError as e:
        raise ValueError(f"{key}: {e}") from None
    data = _file()
    data[key] = value
    store.write_yaml(paths.data_root() / "settings.yaml", data)
    events.emit("config.set", status="ok")
    return value


def unset(key: str) -> None:
    data = _file()
    if data.pop(key, None) is not None:
        store.write_yaml(paths.data_root() / "settings.yaml", data)
