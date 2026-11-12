"""Project sessions (issue #274, US-107): one long-lived agent session per pressing project, talking through the store.

careerbrew cannot start an agent session. It keeps the records that let the /work agent run one session per
project and keep its own context small:

- A session is `{id, project, day, started, ended, handoff}`: `open` records one for a project (the agent's own
  session id) or returns the live one, so a project gets one session, reused all day. At most `work_session_cap`
  sessions are live at once. `open` records the session as a `session` loop too, so `day loop end` checks it.
- Sessions write through the store, never through notes: steps as agent events (`work agent event --session <id>`,
  issue #272), project changes as project updates (`work project update`), and questions as question records
  (`ask`, issue #261), so the main session's one batched ask (`day question recheck`) holds every session's asks.
- A claim is a recorded, expiring hold on a shared thread or draft, named like a step's `uses:` entry
  (`slack-draft:<target>`, issue #258). A session takes it before it writes; another session's `take` and a
  draft save (`draft replace`, the live cycle) on a claimed target are refused, so they skip it. A claim ends
  when it is released, when its session ends, or `work_claim_minutes` after it was last taken.
- End of day: each session writes its project's handoff (`handoff`: one item per project in the day's handoff,
  "Project sessions"), then `end` archives the session (kept, with its handoff) and releases its claims.

Privacy: project names, handoff text, and claimed resources are business data. `public()` drops them, the CLI
shows them only where business data may show, and events carry counts and statuses only.

Lifecycle: `<data root>/work/sessions.json` (encrypted at rest through `store`). Every change reads, changes,
and writes it under `work/.sessions.json.lock` (an fcntl lock the OS frees on exit), like the draft lock (#269)
and the loop records (#264). Nothing is kept in process memory. Ended sessions older than `agent_keep_days` and
expired claims are dropped at the next write.
"""

from __future__ import annotations

import contextlib
import fcntl
import hashlib
import re
import uuid
from collections.abc import Iterator
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import config, events, paths, store

STORY = "US-107"
ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._:-]{0,63}$")  # the loop id rule (`day loop add --id`)
STATES = ("live", "stale", "ended")
HANDOFF_SECTION = "projects"
WRAP_NOTE = "Finish the current step, save work, update drafts, write the project handoff, then report. Do not start a new step. Keep the session running until the main session reads your report."
CONTRACT = (
    "Project sessions: the main /work session runs only the work process (stages, rows, the watch loop, the one batched ask). "
    "Read `careerbrew work project list --json` first. Skip quiet projects and their owner questions until the return date; report any PM promise as deferred. Resume from the saved handoff. "
    "Each pressing project gets one long-lived session, reused all day: `careerbrew work session open <project> --id <session id>` records it or returns the live one "
    "(at most `work_session_cap` live; record nothing else for it with `day loop add`). Give that session the project's work and this contract. "
    "Read each task's full scope in its ticket or issue, not only the brief. Deliver tasks one at a time, in order. "
    "Finish every scope item, shipped and verified, before the next task. A plan, dry run, or list of questions alone is not delivery. "
    "Pause an affected task when blocked by an operator-only decision, approval, or outside wait; continue independent work. "
    "Batch foreseeable approvals up front, record blockers with `careerbrew work session ask`, and continue work that does not depend on them. "
    "Report each task with each scope item marked done or not done and its verification or blocker. "
    "After each subtask, state the goal status in one line and start or name the next goal step. "
    "End an owner reply with ranked next steps they can approve or pick now. If no owner action is needed, say so and name the next autonomous step; do not invent an ask. "
    "Before ending a turn, watch each external wait with a real monitor, scheduled wakeup, or the existing work-day loop with a recheck that covers it. "
    "Record new loops with `careerbrew day loop add`; do not create a second work-day timer. "
    "If the host cannot register a watch, say the wait is unwatched and why. Stop watching when work is done, blocked on the owner, or the work day ends. "
    "At the start of each project-session pass, read `careerbrew work project rules <project>`; if business-data policy refuses, block that project's pass and continue independent work. "
    'A project session writes through careerbrew, never through notes: each step `careerbrew work agent event <agent> "<one line>" --session <id>`, '
    'each project change `careerbrew work project update <project> <kind> "<one line>"`, each question `careerbrew work session ask <id> "<the ask>" --option ... --pick ... --recheck ...` '
    "(it never asks the user). Before it writes a shared thread or draft it claims it: `careerbrew work claim take slack-draft:<target> --session <id>` "
    "(exit 1: another session holds it, so skip it) and saves the draft with `careerbrew draft replace <target> --file <file> --session <id>`. "
    "Read every session with `careerbrew work session list --json`; the batched ask (`careerbrew day question recheck`) holds every session's questions. "
    'End of day: each session writes its project\'s handoff (`careerbrew work session handoff <id> "<where it stands; the next step>"`), '
    "then `careerbrew work session end --all` archives them and releases their claims."
)


class SessionError(ValueError):
    """A refused session or claim change: the message names the fix."""


class ClaimHeld(SessionError):
    """Another live session holds the claim: skip the resource."""


def path() -> Path:
    return paths.sub("work", "sessions.json")


def _now() -> datetime:
    from ..calendar import now

    return now()


def _stamp(at: datetime | None = None) -> str:
    return (at or _now()).isoformat(timespec="seconds")


def _today() -> str:
    return _now().date().isoformat()


def _load() -> dict[str, Any]:
    st = store.read_json(path(), {}) if path().exists() else {}
    if not isinstance(st, dict) or not isinstance(st.get("sessions", []), list) or not isinstance(st.get("claims", []), list):
        raise SessionError("saved session records are invalid; repair the session store before continuing")
    if any(not isinstance(s, dict) or not s.get("id") for s in st.get("sessions", [])):
        raise SessionError("saved session records are invalid; repair the session store before continuing")
    for session in st.get("sessions", []):
        if any(not isinstance(session.get(k), str) for k in ("id", "project", "day", "started", "ended")) or not session["id"] or not session["project"]:
            raise SessionError("saved session records are invalid; repair the session store before continuing")
        try:
            datetime.fromisoformat(session["started"])
            if date.fromisoformat(session["day"]).isoformat() != session["day"]:
                raise ValueError
            if session["ended"]:
                datetime.fromisoformat(session["ended"])
        except ValueError:
            raise SessionError("saved session records are invalid; repair the session store before continuing") from None
    if any(("record_id" in s) != ("project_incarnation" in s) for s in st.get("sessions", [])):
        raise SessionError("saved session identity is incomplete; repair the session store before continuing")
    identities = [s["record_id"] for s in st.get("sessions", []) if "record_id" in s]
    if any(not isinstance(value, str) or not re.fullmatch(r"[0-9a-f]{32}", value) for value in identities) or len(identities) != len(set(identities)):
        raise SessionError("saved session identities are invalid or duplicated; repair the session store before continuing")
    for claim in st.get("claims", []):
        if not isinstance(claim, dict) or any(not isinstance(claim.get(k), str) or not claim[k] for k in ("resource", "session", "until")):
            raise SessionError("saved claims are invalid; repair the session store before continuing")
        try:
            datetime.fromisoformat(claim["until"])
        except ValueError:
            raise SessionError("saved claims are invalid; repair the session store before continuing") from None
    st.setdefault("sessions", [])
    st.setdefault("claims", [])
    return st


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    p = path()
    p.parent.mkdir(parents=True, exist_ok=True)
    with p.with_name(f".{p.name}.lock").open("a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


@contextlib.contextmanager
def _changing() -> Iterator[dict[str, Any]]:
    """Read, change, and write the store under its lock, so two sessions never lose each other's claim."""
    with _locked():
        st = _load()
        before = repr(st)
        yield st
        _prune(st)
        if repr(st) != before:
            store.write_json(path(), st)


def state(s: dict[str, Any], today: str | None = None) -> str:
    if s.get("ended"):
        return "ended"
    return "live" if s.get("day") == (today or _today()) else "stale"


def _valid(c: dict[str, Any], now: datetime) -> bool:
    """A claim still holds: its `until` is later than now, compared as times (stamps may carry different UTC offsets)."""
    try:
        until = datetime.fromisoformat(str(c.get("until", "")))
    except ValueError:
        return False
    if until.tzinfo is None:
        until = until.replace(tzinfo=now.tzinfo)
    return until > now


def shown(res: str) -> str:
    """A claimed resource as the CLI may print it: `<kind>:(hidden)` unless business data may show."""
    from ..cli import may_show_business

    return res if may_show_business() else res.split(":", 1)[0] + ":(hidden)"


def _prune(st: dict[str, Any]) -> None:
    now = _now()
    open_ids = {s["id"] for s in st["sessions"] if not s.get("ended")}
    st["claims"] = [c for c in st["claims"] if _valid(c, now) and c.get("session") in open_ids]
    keep = (_now() - timedelta(days=int(config.get("agent_keep_days")))).date().isoformat()
    st["sessions"] = [s for s in st["sessions"] if not s.get("ended") or s.get("day", "") >= keep]


def check_id(ident: str) -> str:
    ident = str(ident or "").strip()
    if not ID.match(ident):
        raise SessionError(f"session id {ident[:40]!r}: use the agent tool's own session id, 1 to 64 letters, digits, . _ : - (no spaces)")
    return ident


def _project(name: str) -> str:
    from . import projects

    name = str(name or "").strip()
    if name not in projects.records():
        raise SessionError("unknown project: add it first with `careerbrew work project add <name>` (careerbrew work project list shows them)")
    return name


def find(target: str, st: dict[str, Any] | None = None, open_only: bool = True) -> dict[str, Any] | None:
    """The newest session with this id, else the newest open session of this project."""
    st = st if st is not None else _load()
    rows = [s for s in reversed(st["sessions"]) if not (open_only and s.get("ended"))]
    return next((s for s in rows if s["id"] == target), None) or next((s for s in rows if s["project"] == target), None)


def _must(target: str, st: dict[str, Any]) -> dict[str, Any]:
    s = find(target, st)
    if s is None:
        raise SessionError(f"no open session {str(target)[:64]!r}: open one with `careerbrew work session open <project> --id <session id>` (careerbrew work session list shows them)")
    return s


def live(target: str) -> dict[str, Any]:
    """The live session by id or project; refuses an ended, stale, or unknown one."""
    from . import projects

    s = _must(target, _load())
    if state(s) != "live":
        raise SessionError(f"session {s['id']} was opened on {s['day']}: end it (careerbrew work session end {s['id']}) and open today's")
    if projects.quiet(s["project"]):
        raise SessionError("this project is quiet until its saved date; clear --quiet-until to resume it")
    return s


def open_session(project: str, ident: str = "") -> dict[str, Any]:
    """Record the project's session, or return its live one (`reused`). Refuses a second session for one project and a session past the cap."""
    from ..workday import watch
    from . import projects

    project = _project(project)
    ident = check_id(ident) if ident else ""
    cap = int(config.get("work_session_cap"))
    # One lock order covers admission and all three file replacements. The
    # session store is authoritative if a later replacement is interrupted.
    with projects.locked(), _locked(), watch._locked():
        st = _load()
        loop_state = watch._load()
        before, loop_before = repr(st), repr(loop_state)
        today = _today()
        record, all_projects, migrated = projects.session_record(project)
        incarnation = record["incarnation_id"]
        if sum(row.get("incarnation_id") == incarnation for row in all_projects.values()) != 1:
            raise SessionError("saved project identity is ambiguous; repair the project record before opening a session")
        if projects.quiet(project, records=all_projects):
            raise SessionError("this project is quiet until its saved date; clear --quiet-until to resume it")
        ambiguous = next((s for s in st["sessions"] if s["project"] == project and state(s, today) == "live" and s.get("project_incarnation") != incarnation), None)
        if ambiguous:
            raise SessionError(f"an older or unverified session is still live; record its handoff and end it with careerbrew work session end {ambiguous['id']} before opening another")
        mine = next((s for s in reversed(st["sessions"]) if s["project"] == project and state(s, today) == "live"), None)
        if mine:
            if ident and ident != mine["id"]:
                events.emit("work.session", action="open", status="refused", error="one_per_project")
                raise SessionError(
                    f"this project already has a live session today, {mine['id']}: send its work there (one session per project, reused all day). "
                    f'If that session cannot respond, record its status and next step with careerbrew work session handoff {mine["id"]} "<status and next step>", '
                    f"then run careerbrew work session end {mine['id']} and open the new session"
                )
            got = {**mine, "reused": True}
        else:
            if not ident:
                raise SessionError("no live session for this project yet: pass --id <session id> (the agent tool's own id for the new session)")
            if any(s["id"] == ident and not s.get("ended") for s in st["sessions"]):
                raise SessionError(f"session id {ident} already belongs to another open session: each project gets its own session")
            n = sum(1 for s in st["sessions"] if state(s, today) == "live")
            if n >= cap:
                events.emit("work.session", action="open", status="refused", error="cap", live=n)
                raise SessionError(
                    f'{n} of {cap} sessions are live (work_session_cap): end one first (careerbrew work session handoff <id> "<text>", then careerbrew work session end <id>), or raise the cap: careerbrew config set work_session_cap <n>'
                )
            row = {"id": ident, "record_id": uuid.uuid4().hex, "project": project, "project_incarnation": incarnation, "day": today, "started": _stamp(), "ended": "", "handoff": "", "handoff_link": "", "handoff_id": ""}
            if not re.fullmatch(r"[0-9a-f]{32}", row["record_id"]) or any(s.get("record_id") == row["record_id"] for s in st["sessions"]):
                raise SessionError("new session identity is invalid or duplicated; try again")
            st["sessions"].append(row)
            got = {**row, "reused": False}
        current = mine if mine is not None else row
        latest = next((s for s in reversed(st["sessions"]) if s["project"] == project and s.get("project_incarnation") == incarnation and s is not current), None)
        legacy_history = any(s["project"] == project and not s.get("project_incarnation") and s is not current for s in st["sessions"])
        generation = record.get("quiet_generation")
        if generation is not None and (not isinstance(generation, str) or not re.fullmatch(r"[0-9a-f]{32}", generation)):
            raise SessionError("saved quiet identity is invalid; set the quiet period again")
        generations = [p.get("quiet_generation") for p in all_projects.values() if p.get("quiet_generation")]
        if len(generations) != len(set(generations)):
            raise SessionError("saved quiet identities are ambiguous; set the affected quiet periods again")
        receipts = st.setdefault("quiet_receipts", {})
        if not isinstance(receipts, dict):
            raise SessionError("saved quiet receipts are invalid; repair the session record")
        st["quiet_receipts"] = receipts = {key: value for key, value in receipts.items() if key in generations}
        if "opening_brief" not in current:
            yesterday = (datetime.fromisoformat(today).date() - timedelta(days=1)).isoformat()
            quiet_first = bool(generation and generation not in receipts)
            if quiet_first:
                brief = latest.get("handoff", "") if latest and latest["id"] != current["id"] else ""
                receipts[generation] = current["record_id"]
            else:
                brief = latest.get("handoff", "") if latest and latest.get("day") == yesterday else ""
            current["opening_brief"] = brief
            if (legacy_history and latest is None) or (not generation and (record.get("quiet_until") or record.get("quiet_resumed"))):
                current["opening_brief"] = ""
                current["brief_status"] = "legacy-unverified"
        elif generation and generation not in receipts:
            receipts[generation] = current["record_id"]
            current["opening_brief"] = latest.get("handoff", "") if latest and latest["id"] != current["id"] else ""
        got["brief"] = current["opening_brief"]
        if current.get("brief_status"):
            got["brief_status"] = current["brief_status"]
        watch._ensure_session(loop_state, got["id"], current["record_id"])
        _prune(st)
        if migrated:
            projects._save(all_projects)
        if repr(st) != before:
            store.write_json(path(), st)
        loop_added = repr(loop_state) != loop_before
        if loop_added:
            watch._save(loop_state)
    if loop_added:
        events.emit("workday.loop", status="added", kind_owned=False)
    events.emit("work.session", action="open", status="reused" if got["reused"] else "ok")
    return got


def resource(value: str) -> str:
    """A claim's resource: a `uses:` entry (`slack-draft:<target>`, `browser:sso`, `file:<path>`, `checkout:<path>`)."""
    from ..workday.steps import USE, USE_KINDS

    raw = str(value or "")
    value = raw.strip()
    if value.startswith("pr:"):
        match = re.fullmatch(r"pr:([A-Za-z0-9_.-]+)/([A-Za-z0-9_.-]+)#([1-9][0-9]*)", value)
        if raw != value or len(value) > 200 or not match or any(part in {".", ".."} for part in match.groups()[:2]):
            raise SessionError("a PR claim is pr:owner/repo#number, with no whitespace or leading zeroes")
        return f"pr:{match[1].lower()}/{match[2].lower()}#{match[3]}"
    if not USE.match(value) or len(value) > 200:
        raise SessionError(f"claim {value[:40]!r}: name it like a step's `uses:` entry, <kind>:<name> with kind {', '.join(USE_KINDS)} (a thread's reply draft is slack-draft:<channel>/<ts>)")
    return value


def _held(st: dict[str, Any], res: str) -> dict[str, Any] | None:
    now = _now()
    open_ids = {s["id"] for s in st["sessions"] if not s.get("ended")}
    return next((c for c in st["claims"] if c["resource"] == res and _valid(c, now) and c.get("session") in open_ids), None)


def holder(res: str) -> dict[str, Any] | None:
    """The claim that holds this resource now, or None."""
    return _held(_load(), resource(res))


def take(res: str, session: str, minutes: int | None = None) -> dict[str, Any]:
    """Claim a resource for a live session (again: renew it). Raises `ClaimHeld` when another session holds it."""
    res = resource(res)
    minutes = int(minutes if minutes is not None else config.get("work_claim_minutes"))
    if not 1 <= minutes <= 480:
        raise SessionError("--minutes: a claim lasts 1 to 480 minutes")
    s = live(session)
    with _changing() as st:
        c = _held(st, res)
        if c and c["session"] != s["id"]:
            events.emit("work.claim", status="held")
            raise ClaimHeld(f"{shown(res)} is claimed by session {c['session']} until {c['until'][11:16]}: skip it (the claim ends when that session releases it, ends, or at that time)")
        st["claims"] = [x for x in st["claims"] if x["resource"] != res]
        row = {"resource": res, "session": s["id"], "at": _stamp(), "until": _stamp(_now() + timedelta(minutes=minutes))}
        st["claims"].append(row)
    events.emit("work.claim", status="renewed" if c else "taken", minutes=minutes)
    return {**row, "renewed": bool(c)}


def release(res: str, session: str) -> bool:
    """Drop this session's claim on the resource. False when it held none."""
    res, ident = resource(res), check_id(session)
    with _changing() as st:
        n = len(st["claims"])
        st["claims"] = [c for c in st["claims"] if not (c["resource"] == res and c["session"] == ident)]
        gone = len(st["claims"]) < n
    events.emit("work.claim", status="released" if gone else "none")
    return gone


def claims(session: str = "") -> list[dict[str, Any]]:
    st = _load()
    _prune(st)
    return [c for c in st["claims"] if not session or c["session"] == session]


def step_of(ident: str) -> str:
    """The question records' `step` for a session's asks."""
    return f"session:{ident}"


def quiet_question(step: str, day: str, today: str) -> bool:
    """Defer only a question with one identifiable quiet project session."""
    from . import projects

    if not step.startswith("session:"):
        return False
    ident = step.removeprefix("session:")
    matches = [s for s in _load()["sessions"] if s["id"] == ident and s.get("day") == day]
    if len(matches) != 1 or matches[0]["project"] not in projects.records():
        return False
    return projects.quiet(matches[0]["project"], datetime.fromisoformat(today).date())


def ask(session: str, text: str, options: list[str] | None = None, pick: str = "", recheck: str = "") -> dict[str, Any]:
    """Save a session's question as a record in the work day's one question store (issue #261)."""
    from ..workday import questions
    from . import projects

    with projects.locked():
        s = live(session)
        q = questions.add(step_of(s["id"]), text, options, pick, recheck, "agent")
    events.emit("work.session", action="ask", status="ok")
    return q


def write_handoff(target: str, text: str, link: str = "") -> dict[str, Any]:
    """The project's end-of-day handoff: one item per project in the day's handoff ("Project sessions"); a new one replaces the last."""
    from ..day import handoff

    text = " ".join(str(text or "").split())
    if not text:
        raise SessionError("the handoff is empty: say in one line where the project stands and the next step")
    if link and not link.startswith(("https://", "http://")):
        raise SessionError("--link must be a full http(s) URL")
    with _changing() as st:
        s = _must(target, st)
        item = handoff.add(HANDOFF_SECTION, f"{s['project']}: {text}", link)
        if s.get("handoff_id") and s["handoff_id"] != item["id"]:
            handoff.done(s["handoff_id"])
        s.update(handoff=text, handoff_link=link, handoff_id=item["id"])
        stamp = _now()
        if s.get("wrap_requested_at"):
            requested = datetime.fromisoformat(s["wrap_requested_at"])
            if requested.tzinfo is None:
                requested = requested.replace(tzinfo=stamp.tzinfo)
            stamp = max(stamp, requested + timedelta(microseconds=1))
        s["handoff_at"] = stamp.isoformat(timespec="microseconds")
        got = dict(s)
    events.emit("work.session", action="handoff", status="ok")
    return got


def request_wrap(request_id: str = "") -> list[str]:
    """Record each graceful wrap request once; return only the newly requested ids."""
    if not path().exists():
        return []
    requested = []
    with _changing() as st:
        for s in st["sessions"]:
            if s.get("ended") or (s.get("wrap_requested_at") and (not request_id or s.get("wrap_request_id") == request_id)):
                continue
            stamp = _now()
            try:
                prior = datetime.fromisoformat(s.get("handoff_at", ""))
                if prior.tzinfo is not None:
                    stamp = max(stamp, prior + timedelta(microseconds=1))
            except ValueError:
                pass
            s["wrap_requested_at"] = stamp.isoformat(timespec="microseconds")
            s["wrap_request_id"] = request_id
            requested.append(s["id"])
    return requested


def _end(st: dict[str, Any], s: dict[str, Any]) -> None:
    s["ended"] = _stamp()
    st["claims"] = [c for c in st["claims"] if c["session"] != s["id"]]


def _fresh_handoff(s: dict[str, Any]) -> bool:
    if not s.get("handoff"):
        return False
    if not s.get("wrap_requested_at"):
        return True
    try:
        written = datetime.fromisoformat(s.get("handoff_at", ""))
        requested = datetime.fromisoformat(s["wrap_requested_at"])
        if written.tzinfo is None or requested.tzinfo is None:
            return False
        return written > requested
    except (TypeError, ValueError):
        return False


def end(target: str = "", every: bool = False) -> dict[str, Any]:
    """Archive sessions that wrote their handoff and release their claims. A session with no handoff stays open, listed in `missing`."""
    from ..workday import watch

    ended, missing = [], []
    with _changing() as st:
        rows = [s for s in st["sessions"] if not s.get("ended")] if every else [_must(target, st)]
        for s in rows:
            if not _fresh_handoff(s):
                missing.append(dict(s))
                continue
            if s.get("record_id"):
                try:
                    watch.stop(s["id"], session_record=s["record_id"], missing_ok=True)
                except watch.WatchError:
                    raise SessionError("session loop records cannot be verified; repair the loop store before ending the session") from None
            _end(st, s)
            ended.append(dict(s))
    events.emit("work.session", action="end", status="ok" if not missing else "missing_handoff", ended=len(ended), missing=len(missing))
    return {"ended": ended, "missing": missing}


def questions_of(ident: str, open_rows: list[dict[str, Any]]) -> int:
    return sum(1 for q in open_rows if q.get("step") == step_of(ident))


def listing(include_ended: bool = False) -> list[dict[str, Any]]:
    """Every open session (and with `include_ended` the archive), each with its state, its last agent event,
    its claims, and its open questions: the main session's one view of every project session."""
    from ..workday import questions
    from . import agents

    st = _load()
    _prune(st)
    today = _today()
    runs = {r["run"]: r for r in agents.runs()}
    open_q = questions.open_records()
    out = []
    for s in reversed(st["sessions"]):
        if s.get("ended") and not include_ended:
            continue
        r = runs.get(s["id"])
        mine = [c for c in st["claims"] if c["session"] == s["id"]]
        out.append(
            {
                **{k: s.get(k, "") for k in ("id", "project", "day", "started", "ended", "handoff", "handoff_link", "wrap_requested_at", "handoff_at")},
                "state": state(s, today),
                "activity": {"status": r["status"], "at": r["at"], "events": r["events"], "title": r["title"]} if r else {"status": "none", "at": "", "events": 0, "title": ""},
                "claims": [{"resource": c["resource"], "until": c["until"]} for c in mine],
                "questions": questions_of(s["id"], open_q),
                "has_handoff": _fresh_handoff(s),
            }
        )
    return out


def public(row: dict[str, Any]) -> dict[str, Any]:
    """A session row without business data: no project, handoff, claimed resource, or event title."""
    return {
        "id": row["id"],
        "project": hashlib.sha256(row["project"].encode()).hexdigest()[:8],
        **{k: row[k] for k in ("day", "started", "ended", "state", "questions", "has_handoff", "wrap_requested_at", "handoff_at")},
        "activity": {k: v for k, v in row["activity"].items() if k != "title"},
        "claims": len(row["claims"]),
    }
