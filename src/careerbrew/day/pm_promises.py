"""Sent PM commitments (#331), stored as promise projects by the projects writer.

Lifecycle: encrypted project records own the promises and their opaque typed
identity digest, independently of the capped display title. Each read builds a
fresh view; no process cache. Text and project labels are business data; public()
drops them. Repeated saves use the same source-day and full-bullet identity.
"""

from __future__ import annotations

import json
import re
from datetime import date, datetime, timedelta
from typing import Any
from urllib.parse import urlsplit

from .. import store
from ..work import project_priority, projects, promises

SOURCE = "pm-update"
WEIGHT = 100
BULLET = re.compile(r"^\s*(?:[-*•–]|\d+[.)])\s+(.+)$")
LINK = re.compile(r"https?://[^\s<>]+", re.I)
WEEKDAY = r"(?:mon(?:day)?|tue(?:sday)?|wed(?:nesday)?|thu(?:rsday)?|fri(?:day)?|sat(?:urday)?|sun(?:day)?)"
NAMED_TOMORROW = re.compile(rf"tomorrow[ \t]+(?:\({WEEKDAY}\)|-[ \t]+{WEEKDAY})")


def bullets(text: str) -> list[str]:
    found, active, active_plain = [], False, False
    for line in text.splitlines():
        heading = line.strip().strip("#*_ :").casefold()
        match = BULLET.match(line)
        if heading == "tomorrow":
            if active and not active_plain:
                break
            active, active_plain = True, True
            continue
        if not match and (heading == "next week" or NAMED_TOMORROW.fullmatch(heading)):
            if active:
                break
            active = True
            continue
        if not active or not line.strip():
            continue
        if not match:
            if line[:1].isspace() and found:
                found[-1] += " " + " ".join(line.split())
                continue
            break
        title = " ".join(match.group(1).split())
        if title not in found:
            found.append(title)
    return found


def _url_identity(ref: str) -> tuple[str, str, str, str, str, bool, bool]:
    """Keep the full target; normalize only scheme and host case."""
    parts = urlsplit(ref)
    userinfo, at, authority = parts.netloc.rpartition("@")
    if authority.startswith("["):
        host, close, port = authority.partition("]")
        address, percent, zone = host.partition("%")
        authority = address.lower() + percent + zone + close + port
    else:
        host, colon, port = authority.partition(":")
        authority = host.lower() + colon + port
    return parts.scheme.lower(), userinfo + at + authority, parts.path, parts.query, parts.fragment, "?" in ref.partition("#")[0], "#" in ref


def _repo_origin(link: str) -> tuple[str, str] | None:
    parsed = urlsplit(link)
    scheme, authority, *_ = _url_identity(link)
    if scheme not in {"http", "https"} or not parsed.hostname or parsed.username is not None:
        return None
    _ = parsed.port  # reject malformed ports before using configured authority
    return scheme, authority.removesuffix(":443" if scheme == "https" else ":80")


def _repo_origins() -> set[tuple[str, str]]:
    """Invocation scope: trust one configured provider origin, never an inferred host prefix."""
    from ..work import evidence

    configured = [source for source in evidence.sources_list() if source.get("kind") == "github"]
    if not configured:
        return {("https", "github.com"), ("http", "github.com")}
    origins = set()
    for source in configured:
        try:
            origin = _repo_origin(str(source.get("api") or "https://api.github.com"))
        except ValueError:
            return set()
        if origin is None:
            return set()
        scheme, authority = origin
        origins.add((scheme, "github.com" if authority == "api.github.com" else authority))
    return origins if len(origins) == 1 else set()


def _repo_ref(link: str, origins: set[tuple[str, str]]) -> str:
    try:
        if _repo_origin(link) not in origins:
            return ""
        path = urlsplit(link).path
    except ValueError:
        return ""
    parts = path.removeprefix("/").split("/")
    return "/".join(parts[:2]) if len(parts) >= 2 and all(parts[:2]) else ""


def _source_matches(kind: str, ref: str, link: str, source: dict[str, Any], repo: str) -> bool:
    expected = str(source.get("ref") or "")
    if source.get("kind") == "url":
        return _url_identity(link) == _url_identity(expected)
    if source.get("kind") == "repo":
        return bool(repo) and repo.casefold() == expected.casefold()
    if source.get("kind") != kind:
        return False
    return ref.casefold() == expected.casefold() if kind in {"repo", "tracker", "chat"} else ref == expected


def _project(text: str, every: dict[str, Any]) -> str:
    from ..work import research

    candidates = set()
    origins = _repo_origins()
    eligible = {n for n, p in every.items() if p.get("kind") != "promise" and not p.get("archived_at")}
    links = LINK.findall(text)
    for link in links:
        try:
            kind, ref = research.detect(link)
            for name in eligible:
                for source in every[name].get("sources", []):
                    try:
                        if _source_matches(kind, ref, link, source, _repo_ref(link, origins)):
                            candidates.add(name)
                    except ValueError:
                        continue
        except (projects.ProjectError, research.ReadError, ValueError):
            pass
    ranked = [entry for entry in project_priority.entries() if entry["name"] in eligible]
    keys = project_priority.keys(title=text, link=text, es=ranked)
    for link in links:
        keys.extend(project_priority.keys(repo=_repo_ref(link, origins), es=ranked))
    candidates.update(entry["name"] for entry in ranked if project_priority.match(keys, [entry]))
    if not candidates:
        prose = LINK.sub(" ", text)
        candidates = {name for name in eligible if re.search(rf"\b{re.escape(name)}\b", prose, re.I)}
    candidates = {n for n in candidates if n in every and every[n].get("kind") != "promise" and not every[n].get("archived_at")}
    return next(iter(candidates)) if len(candidates) == 1 else ""


def _bullet_identity(title: str) -> str:
    """Fold prose case while preserving each URL target's case-sensitive components."""
    parts: list[Any] = []
    last = 0
    for match in LINK.finditer(title):
        try:
            target = _url_identity(match.group())
        except ValueError:
            target = ("invalid-url", match.group())
        parts.extend((title[last : match.start()].casefold(), target))
        last = match.end()
    if not parts:
        return title.casefold()
    return "url-v2:" + json.dumps([*parts, title[last:].casefold()], separators=(",", ":"))


def _promise_name(day: str, title: str, key: str, identity_digest: str, every: dict[str, Any]) -> str:
    """Reuse only proven typed identities; keep new namespaces outside the legacy date slot."""
    has_url = bool(LINK.search(title))
    legacy = promises.name_for(f"{SOURCE}:{day}:{title.casefold()}")
    kind = "url" if has_url else "prose"
    typed = promises.name_for(f"{SOURCE}:{kind}-v3:{day}:{key}")
    previous_typed = promises.name_for(f"{SOURCE}:{kind}:{day}:{key}")
    url_v2 = promises.name_for(f"{SOURCE}:{day}:{key}")
    candidates = [typed, previous_typed, legacy, url_v2] if has_url else [typed, previous_typed, legacy]
    for name in candidates:
        record = every.get(name) or {}
        source = record.get("promise") or {}
        stored = str(record.get("title") or "")
        if source.get("source") != SOURCE or source.get("pm_day") != day:
            continue
        if source.get("identity_digest") == identity_digest:
            return name
        if source.get("identity_digest") is not None or bool(LINK.search(stored)) != has_url:
            continue
        exact = _bullet_identity(stored) == key
        # This old id hashes the full URL-aware key; a visible URL proves its category.
        truncated_url_v2 = has_url and name == url_v2 and len(title) > projects.TITLE_MAX and _bullet_identity(stored) == _bullet_identity(title[: projects.TITLE_MAX].strip())
        if exact or truncated_url_v2:
            return name
    name = typed if has_url or legacy in every else legacy
    if name in every:
        raise projects.ProjectError("a PM promise id belongs to another identity; preserve it and repair the promise identity")
    return name


def _rows(day: str, text: str) -> list[dict[str, Any]]:
    from .log import next_workday

    due = next_workday(date.fromisoformat(day)).isoformat()
    every = projects.records()
    rows = []
    seen: dict[str, tuple[bool, str]] = {}
    for title in bullets(text):
        key = _bullet_identity(title)
        identity = bool(LINK.search(title)), key
        identity_digest = store.digest(*identity)
        name = _promise_name(day, title, key, identity_digest, every)
        if name in seen:
            if seen[name] != identity:
                raise projects.ProjectError("distinct PM promises share an id; preserve both and repair the promise identity")
            continue
        seen[name] = identity
        rows.append(
            {
                "name": name,
                "title": title,
                "parent": _project(title, every),
                "link": next(iter(LINK.findall(title)), ""),
                "promise": {"source": SOURCE, "pm_day": day, "identity_digest": identity_digest, "event": f"{SOURCE}:{day}:{name}", "due": due, "weight": WEIGHT, "to": "", "made_at": day + "T00:00:00", "steps": {}},
            }
        )
    return rows


def _identity_digest(title: str) -> str:
    return store.digest(bool(LINK.search(title)), _bullet_identity(title))


def _safe_day(value: Any) -> str:
    if not isinstance(value, str):
        return ""
    try:
        parsed = date.fromisoformat(value)
    except ValueError:
        return ""
    return value if parsed.isoformat() == value else ""


def _today_bullets(text: str) -> list[str]:
    lines: list[str] = []
    active = False
    for line in text.splitlines():
        if line.strip().strip("#*_ :").casefold() == "today":
            active = True
            continue
        if not active or not line.strip():
            continue
        if not BULLET.match(line) and not (line[:1].isspace() and lines):
            break
        lines.append(line)
    return bullets("Tomorrow\n" + "\n".join(lines))


def _observations(day: str, text: str, revision: str, every: dict[str, Any]) -> tuple[dict[str, dict[str, str]], set[str]]:
    from .log import next_workday

    today = {_identity_digest(title) for title in _today_bullets(text)}
    tomorrow = {_identity_digest(title) for title in bullets(text)}
    observed_day = date.fromisoformat(day)
    prior = previous_workday(observed_day).isoformat()
    eligible: dict[str, list[str]] = {}
    same_day: set[str] = set()
    for name, row in every.items():
        promise = row.get("promise") or {}
        identity = promise.get("identity_digest")
        if not identity or promise.get("source") != SOURCE or row.get("archived_at"):
            continue
        if promise.get("pm_day") == day:
            same_day.add(identity)
        elif promise.get("pm_day", "") < day and (promise.get("pm_day") == prior or promise.get("due") == day or identity in today or identity in tomorrow):
            eligible.setdefault(identity, []).append(name)
    observations: dict[str, dict[str, str]] = {}
    suppressed: set[str] = set()
    for identity, names in eligible.items():
        in_today, in_tomorrow = identity in today, identity in tomorrow
        if (in_today and in_tomorrow) or (len(names) != 1 and (in_today or in_tomorrow)) or (identity in same_day and (in_today or in_tomorrow)):
            state = "ambiguous"
        elif in_today:
            state = "done"
        elif in_tomorrow:
            state = "moved"
            suppressed.add(identity)
        else:
            state = "not-mentioned"
        due = next_workday(observed_day).isoformat() if state == "moved" else ""
        for name in names:
            observations[name] = {"day": day, "revision": revision, "state": state, "due": due}
    return observations, suppressed


def _review_identities(day: str, revision: str, every: dict[str, Any]) -> dict[str, list[str]]:
    pending: dict[str, list[str]] = {}
    for name, row in every.items():
        promise = row.get("promise") or {}
        decision = promise.get("pm_decision") or {}
        identity = promise.get("identity_digest")
        if promise.get("source") == SOURCE and identity and decision.get("day") == day and (not revision or decision.get("revision") != revision):
            pending.setdefault(identity, []).append(name)
    return pending


def _resolution_context(name: str, day: str, text: str, revision: str, every: dict[str, Any]) -> dict[str, Any] | None:
    promise = (every.get(name) or {}).get("promise") or {}
    identity = promise.get("identity_digest")
    if not identity or not revision:
        return None
    matches = projects.pm_identity_records(every, identity)
    pending = _review_identities(day, revision, every).get(identity, [])
    candidates = [row for row in _rows(day, text) if row["promise"]["identity_digest"] == identity]
    same_day = [key for key, row in matches.items() if (row.get("promise") or {}).get("pm_day") == day]
    prior = [key for key in matches if key not in same_day]
    if pending != [name] or prior != [name] or len(candidates) > 1 or len(same_day) > 1:
        return None
    candidate = candidates[0] if candidates else None
    if same_day and (not candidate or same_day != [candidate["name"]] or every[same_day[0]].get("archived_at")):
        return None
    if candidate and candidate["name"] in every and candidate["name"] not in matches:
        return None
    if candidate and not projects.pm_candidate_valid(every, candidate):
        return None
    return {"candidate": candidate, "matches": matches}


def record(day: str, text: str, *, revision: str = "") -> None:
    from .. import events

    with projects.locked():
        every = projects.records()
        observations, suppressed = _observations(day, text, revision, every) if revision else ({}, set())
        pending = _review_identities(day, revision, every)
        rows = [row for row in _rows(day, text) if (row["promise"]["identity_digest"] not in suppressed and row["promise"]["identity_digest"] not in pending) or (row["promise"]["identity_digest"] in pending and row["name"] in every)]
        changed = projects.sync_pm_promises(day, rows, observations)
    if changed:
        events.emit("work.promise", status="ok", action="pm-update", count=len(rows))


def outcomes() -> list[dict[str, Any]]:
    """Show observations bound to the current committed sent receipt."""
    from ..work import feed
    from . import pm_learn

    result = []
    revisions: dict[str, str] = {}
    texts: dict[str, str] = {}

    def current(day: str) -> str:
        if day not in revisions:
            with pm_learn._sent_lock(day):
                with feed._locked():
                    text = pm_learn._pm_sent_text(day)
                    texts[day] = text or ""
                    revisions[day] = pm_learn._outcome_revision(day, text) if text is not None else ""
        return revisions[day]

    every = projects.records()
    for name, row in every.items():
        promise = row.get("promise") or {}
        observed = promise.get("pm_observation") or {}
        decision = promise.get("pm_decision") or {}
        decision_day = _safe_day(decision.get("day"))
        revision = current(decision_day) if decision_day else ""
        if decision and (not decision_day or revision != decision.get("revision")):
            prior = decision.get("state") if decision.get("state") in ("done", "moved") else "unknown"
            closed = promise.get("closed")
            valid_closed = closed in (None, "", "done", "dropped")
            current_state = closed if closed in ("done", "dropped") else "archived" if row.get("archived_at") else "open"
            consistent = valid_closed and bool(closed) == bool(row.get("archived_at"))
            current_due = _safe_day(promise.get("due"))
            original_due = _safe_day(promise.get("original_due", promise.get("due")))
            prior_due = _safe_day(decision.get("due")) if prior == "moved" else ""
            try:
                datetime.fromisoformat(decision.get("accepted_at", ""))
                valid_decision = bool(_safe_day(decision.get("day")))
            except (TypeError, ValueError):
                valid_decision = False
            valid_dates = bool(current_due and original_due and valid_decision and (prior != "moved" or prior_due))
            context = _resolution_context(name, decision_day, texts.get(decision_day, ""), revision, every) if decision_day else None
            candidate = context.get("candidate") if context else None
            resolutions = (
                [
                    {
                        "choice": choice,
                        "id": projects.pm_resolution_id(name, row, revision, choice, context),
                        "due": original_due if choice == "reopen" else prior_due if choice == "keep" else "",
                        "separate_due": candidate["promise"]["due"] if candidate else "",
                        "separate_existing": bool(candidate and candidate["name"] in every),
                    }
                    for choice in ("keep", "reopen", "drop")
                ]
                if context is not None and prior != "unknown" and consistent and valid_dates
                else []
            )
            result.append({"id": "", "promise": name, "state": "decision-needs-review", "due": "", "prior_state": prior, "current_state": current_state, "prior_due": prior_due, "current_due": current_due, "resolutions": resolutions})
            continue
        day = _safe_day(observed.get("day"))
        if not day or row.get("archived_at"):
            continue
        revision = current(day)
        if not revision or revision != observed.get("revision") or (promise.get("pm_decision") or {}).get("revision") == revision:
            continue
        state = observed["state"]
        ident = projects.pm_proposal_id(name, promise) if state in {"done", "moved"} else ""
        result.append({"id": ident, "promise": name, "state": state, "due": observed["due"]})
    return sorted(result, key=lambda row: row["promise"])


def accept(proposal_id: str) -> None:
    from ..work import feed
    from . import pm_learn

    for name, row in projects.records().items():
        promise = row.get("promise") or {}
        observed = promise.get("pm_observation") or {}
        decision = promise.get("pm_decision") or {}
        normal = bool(observed) and projects.pm_proposal_id(name, promise) == proposal_id
        resolving = proposal_id.startswith("pm-resolution-") and bool(decision.get("day"))
        if not normal and not resolving:
            continue
        day = _safe_day(decision.get("day") if resolving else observed.get("day"))
        if not day:
            continue
        with pm_learn._sent_lock(day):
            with feed._locked():
                text = pm_learn._pm_sent_text(day)
                revision = pm_learn._outcome_revision(day, text) if text is not None else ""
                if revision:
                    if normal:
                        projects.accept_pm_outcome(name, proposal_id, revision)
                        return
                    every = projects.records()
                    current_row = every.get(name)
                    context = _resolution_context(name, day, text or "", revision, every)
                    for choice in ("keep", "reopen", "drop"):
                        if current_row and context is not None and projects.pm_resolution_id(name, current_row, revision, choice, context) == proposal_id:
                            projects.resolve_pm_outcome(name, proposal_id, revision, choice, context)
                            return
        if normal:
            break
    raise projects.ProjectError("PM promise proposal is no longer current")


def reconcile(day: date) -> None:
    """Day start backfills an older sent source; plan reads never write state."""
    from . import pm_learn

    prior = previous_workday(day).isoformat()
    with pm_learn._sent_lock(prior):
        text = pm_learn._pm_sent_text(prior)
        if text is not None:
            record(prior, text, revision=pm_learn._outcome_revision(prior, text))


def previous_workday(day: date) -> date:
    from .planner import is_workday

    return next((day - timedelta(days=n) for n in range(1, 8) if is_workday(day - timedelta(days=n))), day - timedelta(days=1))


def view(day: date) -> dict[str, Any]:
    from ..work import feed, sessions
    from . import pm_learn

    prior = previous_workday(day).isoformat()
    with pm_learn._sent_lock(prior):
        with feed._locked():
            sent = pm_learn._pm_sent_text(prior)
            revision = pm_learn._outcome_revision(prior, sent) if sent is not None else ""
            every = projects.records()
    pending = _review_identities(prior, revision, every)
    live = {s["project"] for s in sessions.listing() if s["state"] == "live" and s["day"] == day.isoformat()}
    candidates = (
        _rows(prior, sent)
        if sent is not None
        else [{"name": n, **p} for n, p in every.items() if (p.get("promise") or {}).get("source") == SOURCE and (p.get("promise") or {}).get("pm_day") == prior and not (p.get("promise") or {}).get("pm_removed")]
    )
    rows = []
    for candidate in candidates:
        name = candidate["name"]
        identity = candidate["promise"].get("identity_digest")
        if identity in pending:
            rows.append(
                {
                    "promise": name,
                    "title": candidate["title"],
                    "project": "",
                    "session_project": "",
                    "covered": False,
                    "due": candidate["promise"]["due"],
                    "weight": candidate["promise"]["weight"],
                    "status": "decision-needs-review",
                    "recorded": False,
                    "review_ids": sorted(pending[identity]),
                }
            )
            continue
        matches = [
            n
            for n, p in every.items()
            if (pr := p.get("promise") or {}).get("identity_digest") == identity
            and (obs := pr.get("pm_observation") or {}).get("day") == prior
            and obs.get("revision") == revision
            and obs.get("state") == "moved"
            and not p.get("archived_at")
        ]
        if revision and len(matches) == 1 and name not in every:
            name = matches[0]
        stored = every.get(name)
        recorded = stored is not None and not (stored.get("promise") or {}).get("pm_removed")
        p = stored if recorded and stored is not None else candidate
        pr = p["promise"]
        project = p.get("parent") or ""
        state = str(pr.get("closed") or "still open") if recorded else "not recorded"
        rows.append(
            {"promise": name, "title": p["title"], "project": project, "session_project": project or name, "covered": recorded and (project or name) in live, "due": pr["due"], "weight": pr["weight"], "status": state, "recorded": recorded}
        )
    return {
        "missing": sent is None,
        "needs_recording": any(not r["recorded"] and r["status"] != "decision-needs-review" for r in rows),
        "from": prior,
        "items": rows,
        "rule": "If needs_recording is true, run day start then read the plan again. For decision-needs-review, ask the owner to review work promise outcomes; do not start a session for that item. Cover other open PM promises before ranked or stale projects and count these first against work_session_cap. For an unmatched promise, use its session_project as the project. If sent text is missing, read the sent update with an authorized chat tool and record it with day pm-sent first.",
    }


def public(value: dict[str, Any]) -> dict[str, Any]:
    return {**value, "items": [{k: v for k, v in row.items() if k not in ("title", "project", "session_project")} for row in value["items"]]}


def report_lines(day: date) -> list[str]:
    return [f"Yesterday's promise: {r['title']} — {r['status']}." for r in view(day)["items"]]
