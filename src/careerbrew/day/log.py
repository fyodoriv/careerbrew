"""The daily log: what you did today and what you plan for tomorrow, kept for every day.

`<data root>/day/log-YYYY-MM-DD.md` (encrypted at rest). Built from the results of the day
(everything the watcher saw from your work sources), tomorrow's plan (ranked tasks placed on
the next workday, plus project next steps), blockers (overdue tasks: stuck now), risks (tasks
due by the next workday: may go wrong later), housekeeping (stale project sources), open questions, and the focus advice. The watcher rebuilds it after every cycle that saw
something new. `pm_report()` is the short draft for your PM (issue #68): one line per project in
`project_priority` order, each naming its results (issue #214), plain words, only counts, dates, and
source titles (all checkable), and no task ids, ticket keys, or PR numbers. Its `why` notes (one
sentence per line: who benefits, what it should change, a checked result) are for the owner only and
never part of the sent text. It is shown in the web app with
Copy and Send; Send runs only on the owner's explicit click or `day send --yes`. A saved or sent text that names a
PR (`#12`) or a ticket key outside a link is refused, and the error names each such line (`unlinked_refs`, issue #227).
Each project line leads with a launch post and the meetings the owner led, then the work, small CI or config changes
last, then check-ins and reviews (issue #239).
"""

from __future__ import annotations

import re
from datetime import date, datetime, time, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, config, events, paths, store
from ..work import feed, priorities, project_priority, projects
from . import inbox, planner, results

GROUPS = (("Shipped", {"commit", "pr", "github", "jira"}), ("Meetings", {"meeting"}), ("Notes", {"note"}), ("Other work", {"gdocs", "slack", "zoom", "outlook", "pagerduty", "feed", "external"}))
# Only work kinds reach the log and the PM update; job-hunt rows (interview mail), unknown kinds, and
# personal rows (a private calendar entry, a feed event marked personal; issue #72) never do.
WORK_KINDS = set().union(*(kinds for _, kinds in GROUPS))


def normalize(title: str) -> str:
    """One change, one line: the merged PR title and its commit subject differ only by a trailing " (#N)", case, and spacing."""
    t = re.sub(r"\s*\(#\d+\)\s*$", "", title.strip())
    return " ".join(t.lower().split())


def path_for(day: date) -> Path:
    return paths.sub("day", f"log-{day.isoformat()}.md")


def next_workday(day: date) -> date:
    d = day + timedelta(days=1)
    while not planner.is_workday(d):
        d += timedelta(days=1)
    return d


def build(day: date | None = None) -> dict[str, Any]:
    day = day or results.today()
    personal = feed.personal_ids()
    rows = sorted((r for r in results.load(day) if not r.get("personal") and r.get("id") not in personal and r.get("ref") not in personal), key=lambda r: str(r.get("at", "")))
    done: dict[str, list[str]] = {name: [] for name, _ in GROUPS}
    done_details: dict[str, list[dict[str, Any]]] = {name: [] for name, _ in GROUPS}
    links = _evidence_links()
    ranked = project_priority.entries()
    seen: dict[str, str] = {}  # normalized title -> group it was listed in (issue #43)
    for r in sorted(rows, key=lambda r: r.get("kind") not in ("pr", "github")):  # PR rows first: they carry the link
        title = str(r.get("title") or "").strip()
        if r.get("kind") not in WORK_KINDS or not title:
            continue
        key = normalize(title)
        if key in seen:
            continue
        group = next(name for name, kinds in GROUPS if r.get("kind") in kinds)
        seen[key] = group
        done[group].append(title[:160])
        done_details[group].append(_done_detail(r, title[:160], links, ranked))
    tomorrow = next_workday(day)
    # Tomorrow's plan is that day of the week plan from now: a task today's plan still holds is not planned again,
    # and what no longer fits today moves to tomorrow. A log built for another day sees that day as over.
    now = calendar.now()
    if now.date() != day:
        now = datetime.combine(day, time.max, calendar.local_tz())
    days = planner.plan_week(day, (tomorrow - day).days + 1, now)
    plan = days[-1]
    # Work only: study blocks the teacher adds for your goals ("goal:..." ids) and prep gates ("gate:...", issue #276) are
    # the owner's own prep, never in the PM update.
    held = [s for x in days[:-1] for s in x["schedule"] if not str(s["task"]).startswith(("goal:", "gate:"))]  # what the rest of today holds
    by_id = {t["id"]: t for t in priorities.load()}

    def label(task_id: str, title: str) -> str:
        t = by_id.get(task_id, {})
        who = t.get("source", "user")
        return f"{task_id} {title}" + (f" (added by {who}: {t.get('why')})" if who not in ("user",) and t.get("why") else "")

    planned = [label(s["task"], s["title"]) for s in plan["schedule"] if not str(s["task"]).startswith(("goal:", "gate:"))]
    linked = {t for p in projects._load().values() for t in p.get("tasks", [])}
    open_tasks = {t["id"]: t for t in priorities.load() if priorities.is_open(t)}
    of = projects.task_projects()
    planned_details = [
        _plan_detail(by_id.get(s["task"], {"id": s["task"], "title": s["title"]}), of, "Placed on the plan by score: impact, urgency, due date, promotion gaps, and project priority.", ranked)
        for s in plan["schedule"]
        if not str(s["task"]).startswith(("goal:", "gate:"))
    ]
    held_ids = {s["task"] for s in held}
    extra = [t for t in sorted(linked) if t in open_tasks and t not in held_ids and not any(x.startswith(t + " ") for x in planned)]
    planned += [f"{t} {open_tasks[t]['title']} (project next step)" for t in extra]
    planned_details += [_plan_detail(open_tasks[t], of, f"The next step of project {of.get(t, '')}.", ranked) for t in extra]
    # The PM update's "next" starts with the work later today still holds: it is not done yet (pm_lines).
    later_details = [_plan_detail(by_id.get(s["task"], {"id": s["task"], "title": s["title"]}), of, "Planned for later today by score.", ranked) for s in held]
    overdue = [t for t in open_tasks.values() if t.get("due") and str(t["due"]) < day.isoformat()]
    blockers = [f"{t['id']} {t['title']} is overdue (due {t['due']})" for t in overdue]
    blocker_details = [{"task": t["id"], "title": t["title"], "due": str(t["due"]), "url": priorities.web_url(t), "project": of.get(t["id"], ""), "priority": _priority_of(t, of, ranked)} for t in overdue]
    risks = [f"{t['id']} {t['title']} is due {t['due']}" for t in open_tasks.values() if t.get("due") and day.isoformat() <= str(t["due"]) <= tomorrow.isoformat()]
    housekeeping = [f"{s['project']}: {s['kind']} {s['ref']} not read for {'ever' if s['age_days'] is None else str(s['age_days']) + ' days'}" for s in projects.stale_sources()[:5]]
    questions = [i["text"] for i in inbox.open_items() if i["type"] == "question"]
    try:
        from ..work import focus

        horizons = {h["id"]: h for h in focus.build()["horizons"]}
        advice = [f"{h['label']}: {h['items'][0]['title']}" for k in ("week", "quarter") if (h := horizons.get(k)) and h["items"]]
    except Exception:  # noqa: BLE001 (advice is optional; the log must still build)
        advice = []
    return {
        "day": day.isoformat(),
        "tomorrow": tomorrow.isoformat(),
        "done": done,
        "planned": planned,
        "today_holds": len(held),
        "blockers": blockers,
        "risks": risks,
        "housekeeping": housekeeping,
        "questions": questions,
        "advice": advice,
        "details": {"done": done_details, "planned": planned_details, "blockers": blocker_details, "later_today": later_details, "signals": _signals(day, ranked)},
    }


def _evidence_links() -> dict[str, str]:
    """Evidence id -> its source link (PRs carry one); results rows keep only the id."""
    from ..work import evidence

    try:
        return {str(e["id"]): str(e["url"]) for e in evidence.load() if e.get("url")}
    except Exception:  # noqa: BLE001 (links are optional; the log must still build)
        return {}


def _refs(*texts: str) -> list[str]:
    """Ticket keys (PROJ-123) and PR numbers (#12) named in the texts, in order, once each."""
    out: list[str] = []
    for t in texts:
        for m in re.findall(r"\b[A-Z][A-Z0-9]+-\d+\b|#\d+\b", t or ""):
            if m not in out:
                out.append(m)
    return out


def _done_detail(r: dict[str, Any], title: str, links: dict[str, str], ranked: list[dict[str, Any]]) -> dict[str, Any]:
    """Why a Done row is there: which source saw it, its link, project, related tickets and PRs, and when it happened (issues #61, #68)."""
    rid = str(r.get("id", ""))
    url = links.get(rid, "")
    repo = project_priority.repo_of(rid, url)
    hit = project_priority.match(project_priority.keys(title, repo, url, es=ranked), ranked)
    seen = f"Found by the {r.get('source', 'watcher')} source as a {r.get('kind', 'item')}" + ("" if r.get("origin", "user") == "user" else f" (from {r['origin']})")
    return {
        "title": title,
        "kind": r.get("kind", ""),
        "url": url,
        "repo": repo,
        "project": hit["name"] if hit else "",
        "why": seen + ".",
        "related": _refs(title, rid),
        "history": [{"at": str(r.get("at", "")), "event": f"{r.get('kind', 'item')} recorded"}],
        **({"draft": True} if r.get("draft") is True or r.get("isDraft") is True else {}),
        **{key: r[key] for key in ("state", "merged_at") if key in r},
        **({"led": True} if r.get("led") else {}),  # issue #239: a meeting the owner organized
        **({"recurring": True} if r.get("recurring") else {}),
    }


def _signals(day: date, ranked: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The owner's own launch posts and initiative check-ins of the day, from the work feed (issue #239): the signal
    word, the channel, the link, and the project. A reply in a thread that asked the owner is no check-in.
    Never the post's text; personal events never."""
    tz = calendar.local_tz()
    out = []
    rows = feed.load()
    asks = {e["id"] for e in rows if e.get("to_me") or e.get("triage") == "act"}
    for e in rows:
        if not e.get("signal") or e.get("personal") or e.get("actor") != "me":
            continue
        if e["signal"] == "check-in" and e.get("thread") in asks:
            continue  # a reply to an ask answers it: that is not an initiative check-in
        try:
            when = datetime.fromisoformat(str(e.get("at", "")).replace("Z", "+00:00"))
        except ValueError:
            continue
        if (when if when.tzinfo else when.replace(tzinfo=tz)).astimezone(tz).date() != day:
            continue
        channel, url = str(e.get("channel") or ""), str(e.get("url") or "")
        hit = project_priority.match(project_priority.keys(channel, link=url, project=str(e.get("project") or ""), es=ranked), ranked)
        out.append({"signal": e["signal"], "channel": channel, "url": url, "project": hit["name"] if hit else str(e.get("project") or ""), "at": str(e.get("at", ""))})
    return out


def _priority_of(t: dict[str, Any], of: dict[str, str], ranked: list[dict[str, Any]]) -> str:
    hit = project_priority.of_task(t, of, ranked)
    return hit["name"] if hit else ""


def _plan_detail(t: dict[str, Any], of: dict[str, str], reason: str, ranked: list[dict[str, Any]]) -> dict[str, Any]:
    """Why a Plan row is there: project, who added it and why, its link, related tickets and PRs, and its history (issue #61)."""
    who = t.get("source", "user")
    why = reason + (f" Added by {who}: {t['why']}" if who != "user" and t.get("why") else "")
    history = [{"at": str(t["added_at"]), "event": f"added by {who}"}] if t.get("added_at") else []
    if t.get("due"):
        history.append({"at": str(t["due"]), "event": "due"})
    if t.get("done_at"):
        history.append({"at": str(t["done_at"]), "event": "done"})
    return {
        "task": t.get("id", ""),
        "title": t.get("title", ""),
        "url": priorities.web_url(t),
        "project": of.get(t.get("id", ""), ""),
        "priority": _priority_of(t, of, ranked),
        "why": why,
        "related": _refs(t.get("title", ""), *(x["url"] for x in priorities.links(t)), t.get("why", "")),
        "history": history,
    }


def render(d: dict[str, Any]) -> str:
    lines = [f"# Work log {d['day']}", "", "## Done today", ""]
    any_done = False
    for group, items in d["done"].items():
        if items:
            any_done = True
            lines += [f"**{group}**", *[f"- {x}" for x in items], ""]
    if not any_done:
        lines += ['- Nothing recorded yet. Add what tools miss: `careerbrew work note "..."`.', ""]
    empty = f"- Nothing left for {d['tomorrow']}: today's plan holds every open task." if d.get("today_holds") else "- No task fits yet: add one with `careerbrew work task`."
    lines += [f"## Plan for {d['tomorrow']}", "", *([f"- {x}" for x in d["planned"]] or [empty]), ""]
    if d["blockers"]:
        lines += ["## Blockers", "", *[f"- {x}" for x in d["blockers"]], ""]
    if d.get("risks"):
        lines += ["## Risks", "", *[f"- {x}" for x in d["risks"]], ""]
    if d["questions"]:
        lines += ["## Open questions for me", "", *[f"- {x}" for x in d["questions"]], ""]
    if d["advice"]:
        lines += ["## Focus", "", *[f"- {x}" for x in d["advice"]], ""]
    if d.get("housekeeping"):
        lines += ["## Housekeeping", "", *[f"- {x}" for x in d["housekeeping"]], ""]
    return "\n".join(lines)


def _slack_source() -> dict[str, Any] | None:
    from ..work import evidence

    return next((source for source in evidence.sources_list() if source.get("kind") == "slack"), None)


def pm_report(d: dict[str, Any]) -> dict[str, Any]:
    """The short update for the PM: done today, next, and overdue work, one line per project. No questions or advice.

    `updated_at` says when the shown text last changed (issue #145): the owner's last saved edit, or else the
    last change of the kept day log that the draft is built from ("" when no log is kept for that day).
    """
    from . import pm_learn

    title = f"{config.get('pm_report_title')} {d['day']}"
    via = str(config.get("pm_send_via"))
    raw, notes = _pm_build(d)
    learned = pm_learn.rules()
    draft = "\n".join(_learned_pm_lines(raw, {e["name"] for e in project_priority.entries()}, learned))
    pm_learn.remember(d["day"], "\n".join(raw), draft)  # a later save is compared with what was shown (issue #185)
    saved = store.read_text(pm_draft_path(d["day"]))  # the owner's edits for that day win (issue #52)
    updated = _written_at(pm_draft_path(d["day"]) if saved else path_for(date.fromisoformat(d["day"])))
    return {
        "subject": title,
        "body": saved or draft,
        "draft": draft,
        "edited": bool(saved),
        "updated_at": updated,
        "via": via,
        "slack_configured": via == "slack" and _slack_source() is not None,
        "to": str(config.get("pm_slack") if via == "slack" else config.get("pm_email")) or "",
        "left_out": pm_learn.left_out([line for line in raw if not line.startswith("Yesterday's promise:")], learned),
        "ladder": _ladder(saved or draft),  # owner-only grading against the next level; never part of the sent text (issue #216)  # lines your past edits leave out: the view names them
        "why": _why_shown(draft, notes, learned, saved=saved),  # owner-only notes for the displayed body (issue #372)
        "checks": content_checks(saved or draft, d),  # owner-only content rules a machine can check (issue #226)
    }


# Content rules a machine can check (issue #226); the rest are in skills/careerbrew-work-companion/SKILL.md for the agent that drafts.
_ESTIMATE = re.compile(
    r"\bETA\b|\bestimat(?:e[sd]?|ion)\b|\b(?:will|should|would|might|may) take\b|~\s*\d+(?:\.\d+)?\s*(?:h|d|w)\b"
    r"|\b\d+(?:\.\d+)?\s*(?:h|hrs?|hours?|d|days?|w|wks?|weeks?|sprints?)\s+(?:left|to go|remaining|more)\b",
    re.I,
)
_CLAIM = re.compile(r"\b\d+(?:\.\d+)?\s*(?:%|x\b|times (?:faster|fewer|less|more|smaller|bigger)\b)", re.I)
_URL = re.compile(r"https?://\S+")
_NO_ACTION = re.compile(
    r"(?:(?:status(?: check)?|checked (?:the )?(?:tickets|issues|queue|asks|tracker))\s*[:;,–-]\s*)?"
    r"(?:(?:all\s+)?(?:tickets|issues|asks)\s+(?:are\s+)?(?:closed|resolved|handled)[,;:–-]?\s*)?"
    r"(?:no action (?:is )?(?:needed|required)|nothing to do)(?: from (?:the )?(?:owner|user|customer|team)| from you)?[.!]?",
    re.I,
)
_DRAFT_PREFIX = re.compile(r"^\s*(?:\[draft\]\s*|draft(?: pr)?[ :–-]+)", re.I)
_DRAFT_END = re.compile(r"\bin draft[.!]?\s*$", re.I)


def _no_action(text: str, labeled: bool = True) -> bool:
    plain = _URL.sub("", text).strip()
    prefix, sep, tail = plain.partition(":")
    commit_prefix = re.fullmatch(r"(?:feat|fix|docs|chore|refactor|test|perf|build|ci|style|revert)(?:\([^)]*\))?!?", prefix, re.I)
    body = tail.strip() if labeled and sep and re.fullmatch(r"[\w.-]+", prefix) and not commit_prefix else plain
    return bool(_NO_ACTION.fullmatch(body))


def _is_draft(row: dict[str, Any]) -> bool:
    return row.get("draft") is True or row.get("isDraft") is True or bool(_DRAFT_PREFIX.search(str(row.get("title") or "")))


def _state_note(row: dict[str, Any]) -> str:
    if _is_draft(row):
        return "In draft"
    if row.get("kind") in ("pr", "github"):
        state = str(row.get("state") or "").casefold()
        if state == "open":
            return "Open PR"
        if state == "closed" and not row.get("merged_at"):
            return "Closed PR; merge not confirmed"
    return ""


CHECK_FIX = {
    "time-estimate": "no time estimates: say what is done and what is next",
    "number-without-evidence": "a big number needs its evidence: add a link, or say it plainly",
    "next-done": "next lists only work that is not done",
    "no-action": "drop a status check that found no action or outcome",
    "draft-state": "this source is unfinished: end its line with In draft",
}


def content_checks(text: str, d: dict[str, Any]) -> list[dict[str, str]]:
    """The draft's lines that break a content rule a machine can check (issue #226), each with its rule and fix:
    a time estimate; a percent or "3x" claim with no link and no source title that names it; a `next:` item that
    names work done today. For the owner only: a check never changes the text or blocks a save."""
    done = [str(t) for ts in (d.get("done") or {}).values() for t in ts]
    done_plain = {p.casefold() for t in done if (p := _plain(t, limit=200))}
    sources = " ".join(done).casefold()
    out: list[dict[str, str]] = []
    for raw in text.splitlines():
        line = raw.strip()
        if not line:
            continue
        rules = []
        if _no_action(line):
            rules.append("no-action")
        draft_titles = [
            (_plain(_DRAFT_PREFIX.sub("", str(row.get("title") or "")), limit=200).casefold(), str(row.get("project") or str(row.get("repo") or "").rsplit("/", 1)[-1]).casefold())
            for rows in ((d.get("details") or {}).get("done") or {}).values()
            for row in rows
            if _is_draft(row)
        ]
        label, labeled, _ = line.partition(":")
        label = label.strip(" -*_").casefold()
        if any(title and (not project or not labeled or project == label) and re.search(rf"(?<!\w){re.escape(title)}(?!\w)", line, re.I) for title, project in draft_titles) and not _DRAFT_END.search(line):
            rules.append("draft-state")
        if _ESTIMATE.search(_URL.sub(" ", line)):
            rules.append("time-estimate")
        if not _URL.search(line) and any(" ".join(m.group(0).casefold().split()) not in sources for m in _CLAIM.finditer(line)):
            rules.append("number-without-evidence")
        nxt = re.search(r"\bnext:\s*(.+?)(?:;\s*(?:overdue|impact):|$)", line)
        if nxt and any(item.strip(" .").casefold() in done_plain for item in re.split(r",\s*(?:and\s+)?|\s+and\s+", nxt.group(1))):
            rules.append("next-done")
        out += [{"rule": r, "line": line, "fix": CHECK_FIX[r]} for r in rules]
    return out


def _why_shown(draft: str, notes: dict[str, str], learned: dict[str, Any], *, saved: str | None = None) -> list[dict[str, str]]:
    """Keep generated evidence with its line and describe every deliverable in a saved edit."""
    from . import pm_learn

    raw_label = {new: old for old, new in learned["rename"].items()}
    generated = {}
    for line in draft.splitlines():
        label, _ = pm_learn._split(line)
        if note := notes.get(raw_label.get(label, label)):
            generated[line] = {"label": label, "note": note}
    if not saved:
        return [generated[line] for line in draft.splitlines() if line in generated]

    out = []
    for raw in saved.splitlines():
        line = raw.strip()
        title = line.lstrip("-•◦ ").strip()
        heading = title.strip("#*_ ").rstrip(":").strip("*_ ").casefold()
        if not title or heading in {"today", "tomorrow"}:
            continue
        bound = generated.get(line)
        out.append({"label": title, "note": bound["note"] if bound else f"Not sure: {NO_IMPACT}."})
    return out


def _ladder(text: str) -> dict[str, Any]:
    from ..work import ladder_grade, promo

    try:
        return ladder_grade.grade(text.splitlines(), *ladder_grade.target())
    except promo.LadderError:
        return {}


def _written_at(p: Path) -> str:
    """A file's last write in local time, to the minute; "" when it is missing. Reads only: a read must not look like new data (issue #117)."""
    try:
        return datetime.fromtimestamp(p.stat().st_mtime, calendar.local_tz()).isoformat(timespec="minutes")
    except FileNotFoundError:
        return ""


# The PM update is built from an allowlist of groups: job-hunt rows and personal notes never reach it.
PM_GROUPS = {"Shipped": "change", "Other work": "item", "Meetings": "meeting"}
MAX_NAMED = 8  # a project line names this many done items, then says how many more (issue #214 dropped the line cap)
# What a row counts as, in a manager's words. Titled nouns are named when a project has only one or two of them.
NOUN = {"pr": "PR", "commit": "commit", "jira": "ticket", "gdocs": "doc", "meeting": "meeting", "zoom": "hosted meeting", "outlook": "hosted meeting", "slack": "shout-out in Slack", "pagerduty": "incident"}
TITLED = ("PR", "commit", "ticket", "doc", "change", "incident", "item")
# Issue #239: what a manager values most leads the line: a launch, then a meeting the owner led, then the work.
STANDUP = re.compile(r"\b(?:stand-?ups?|daily (?:sync|scrum|huddle))\b", re.I)
_HOSTED = re.compile(r"^(?:Hosted|Organized):\s*", re.I)
# Low-signal work (issue #239): small CI, build, lint, or config changes are named last, so "and N more" drops them first.
_LOW = re.compile(r"^(?:ci|chore|build|style)(?:\([^)]*\))?!?:|\b(?:bump|lint|config|ci)\b", re.I)
COUNTED = ("meeting", "hosted meeting", "shout-out in Slack")
PLURAL = {"shout-out in Slack": "shout-outs in Slack"}
REVIEWS = re.compile(r"^Reviewed (\d+) pull requests?$")
OVERDUE = re.compile(r"^\S+ (.+) is overdue \(due (.+)\)$")
_CC = re.compile(r"^(?:feat|fix|docs|chore|refactor|tests?|perf|build|ci|style|revert)(?:\([^)]*\))?!?:\s*", re.I)
_TAIL = re.compile(r"\s+(?:for|in|on|of|to|from|re|via|and|with|closes|fixes)$", re.I)


def _plain(title: str, limit: int = 70, ticket: re.Pattern[str] | None = None) -> str:
    """A title in plain words: no commit prefix, task id, ticket key, or PR number; a lower-case start.
    Ticket keys are `project_priority.ticket_pattern()` keys, so "decode UTF-8 names" keeps UTF-8."""
    ref = rf"(?:#\d+|{(ticket or project_priority.ticket_pattern()).pattern})"
    t = re.sub(rf"\s*[(\[](?:[A-Za-z]+\s+)?{ref}(?:[,\s]+{ref})*[)\]]:?", " ", _CC.sub("", title.strip()))
    t = re.sub(rf"{ref}:?|\bt\d{{3,}}\b", " ", t)
    t = " ".join(t.split()).strip(" :-,;.")
    while _TAIL.search(t):
        t = _TAIL.sub("", t).strip(" :-,;.")
    if len(t) > limit:
        t = t[:limit].rsplit(" ", 1)[0].rstrip(" :-,;.") + "…"
    return t[:1].lower() + t[1:] if t[:1].isupper() and not t[1:2].isupper() else t


def _n(count: int, noun: str) -> str:
    return f"{count} {noun if count == 1 else PLURAL.get(noun, noun + 's')}"


def _few(items: list[str]) -> str:
    """Name two at most: "a and b", or "a, b, and 3 more"."""
    return " and ".join(items) if len(items) <= 2 else f"{items[0]}, {items[1]}, and {len(items) - 2} more"


def _label(x: dict[str, Any], title: str, ticket: re.Pattern[str]) -> str:
    """The line a row goes on: its priority project, its linked work project, its repo, or its ticket prefix."""
    if x.get("priority") or x.get("project"):
        return str(x.get("priority") or x.get("project"))
    if repo := str(x.get("repo") or "") or project_priority.repo_of(url=str(x.get("url") or "")):
        return repo.rsplit("/", 1)[-1]
    m = ticket.search(title)
    return m.group(1) if m else ""


def _all(items: list[str]) -> str:
    """Name each one: "a", "a and b", "a, b, and c"; past MAX_NAMED, "a, …, h, and 4 more"."""
    shown, more = items[:MAX_NAMED], len(items) - MAX_NAMED
    if more > 0:
        return f"{', '.join(shown)}, and {more} more"
    return " and ".join(shown) if len(shown) <= 2 else f"{', '.join(shown[:-1])}, and {shown[-1]}"


def _done_phrase(items: list[dict[str, Any]], label: str, ticket: re.Pattern[str]) -> str:
    """Done work named by its plain titles (issue #214), with the counts of rows (each has a source, so it can be checked)."""
    work = sorted((i for i in items if i["noun"] in TITLED), key=lambda i: bool(_LOW.search(str(i["title"]))))
    counts = " and ".join(_n(k, noun) for noun in TITLED if (k := sum(1 for i in work if i["noun"] == noun)))
    repos = sorted({r for i in work if (r := str(i["repo"]).rsplit("/", 1)[-1]) and r != label})
    where = "" if not repos else f" in {repos[0]}" if len(repos) == 1 else f" in {len(repos)} repos ({', '.join(repos[:3])}{', and more' if len(repos) > 3 else ''})"
    named = list(dict.fromkeys(p for i in work if (p := _plain(i["title"], ticket=ticket))))
    parts = [f"launched ({_link(f'announced in #{i["title"]}', i['url'])})" for i in items if i["noun"] == "launch"]
    parts += [f"led {p}" for i in items if i["noun"] == "led meeting" and (p := _plain(i["title"], ticket=ticket))]
    if work:
        parts.append(f"{_all(named)} ({counts}{where})" if named else counts + where)
    parts += [_n(k, noun) for noun in COUNTED if (k := sum(1 for i in items if i["noun"] == noun))]
    if checks := list(dict.fromkeys(_link(f"#{i['title']}", i["url"]) for i in items if i["noun"] == "check-in")):
        parts.append(f"checked in on {_few(checks)}")
    if reviewed := sum(i["reviews"] for i in items):
        parts.append(f"reviewed {_n(reviewed, 'PR')}")
    return ", ".join(parts)


def _link(label: str, url: str) -> str:
    """A markdown link when the source gave one (issue #227: a reader can click it); else the label."""
    return f"[{label}]({url})" if url.startswith(("https://", "http://")) else label


def pm_lines(d: dict[str, Any], learn: bool = True) -> list[str]:
    """One line per project: ranked projects in rank order, then the busiest, then Other (issue #68); no line cap (issue #214).
    With `learn`, the owner's past edits shape it (`pm_learn`, issue #185); `learn=False` is the raw draft they edit against."""
    lines, _ = _pm_build(d)
    if not learn:
        return lines
    return _learned_pm_lines(lines, {e["name"] for e in project_priority.entries()})


def _learned_pm_lines(lines: list[str], ranked: set[str], learned: dict[str, Any] | None = None) -> list[str]:
    from . import pm_learn

    protected = [line for line in lines if line.startswith("Yesterday's promise:")]
    normal = [line for line in lines if not line.startswith("Yesterday's promise:")]
    return [*pm_learn.apply(normal, ranked, learned), *protected]


def _pm_build(d: dict[str, Any]) -> tuple[list[str], dict[str, str]]:
    """The raw draft's lines, and the owner-only "why it matters" note of each line's label (issue #214)."""
    details = d.get("details") or {}
    groups: dict[str, dict[str, list[Any]]] = {}
    ranked = project_priority.entries()
    rows = [*(x for xs in (details.get("done") or {}).values() for x in xs), *(details.get("planned") or []), *(details.get("blockers") or [])]
    ticket = project_priority.ticket_pattern(ranked, [str(x.get("url") or "") for x in rows])  # one pattern for the whole update

    def group(label: str) -> dict[str, list[Any]]:
        return groups.setdefault(label, {"done": [], "next": [], "late": [], "impact": [], "tasks": []})

    tasks = {t["id"]: t for t in priorities.load()}

    for name, default in PM_GROUPS.items():
        titles = d["done"].get(name, [])
        for text, x in zip(titles, (details.get("done") or {}).get(name) or [{} for _ in titles], strict=False):
            if _no_action(text, labeled=False):
                continue
            kind = str(x.get("kind") or default)
            m = REVIEWS.match(text) if kind == "github" else None
            noun = "review" if m else NOUN.get(kind, "PR" if kind == "github" else default)
            if noun in ("meeting", "hosted meeting") and (x.get("led") or kind == "zoom") and not x.get("recurring") and not STANDUP.search(text):
                noun = "led meeting"  # issue #239: named, not counted
            group(_label(x, text, ticket))["done"].append({"title": _HOSTED.sub("", text), "noun": noun, "repo": x.get("repo", ""), "reviews": int(m.group(1)) if m else 0, "state_note": _state_note({**x, "title": text})})
    for s in details.get("signals") or []:  # issue #239: launches and initiative check-ins, each with its link
        group(_label({"project": s.get("project")}, "", ticket))["done"].append({"title": s["channel"], "noun": s["signal"], "url": s["url"], "repo": "", "reviews": 0})
    late = details.get("blockers") or [{"title": m.group(1), "due": m.group(2)} for x in d.get("blockers", []) if (m := OVERDUE.match(x))]
    for b in late:
        g = group(_label(b, b["title"], ticket))
        g["late"].append(f"{_plain(b['title'], ticket=ticket)} (due {b['due']})")
        g["tasks"].append(tasks.get(str(b.get("task") or "")))
    named_late = {b.get("task") or b["title"] for b in late}
    plan = [*(details.get("later_today") or []), *(details.get("planned") or [{"title": re.sub(r" \((?:project next step|added by .*)\)$", "", x.split(" ", 1)[-1])} for x in d.get("planned", [])])]
    for p in plan:
        if (p.get("task") or p["title"]) not in named_late and (t := _plain(p["title"], ticket=ticket)):
            g = group(_label(p, p["title"], ticket))
            g["next"].append(t)
            g["tasks"].append(tasks.get(str(p.get("task") or "")))
    for t in priorities.measured_on(date.fromisoformat(d["day"])) if d.get("day") else []:  # issue #214: measured results, with their evidence
        g = group(_label({"project": ""}, t["title"], ticket))
        g["impact"].append(t["measured_impact"]["result"])
        g["tasks"].append(t)
    rank = {e["name"]: e["rank"] for e in ranked}
    size = {k: sum(len(v) for v in g.values()) for k, g in groups.items()}
    order = [k for k in sorted(groups, key=lambda k: (0, rank[k], "") if k in rank else (2, 0, "") if not k else (1, -size[k], k)) if _line(k, groups[k], ticket)]
    lines = [_line(k, groups[k], ticket) for k in order] or ["Nothing recorded yet."]
    notes = {k or "Other": _why(k, groups[k]["tasks"], rank) for k in order}
    if d.get("day"):
        from . import pm_promises

        lines.extend(pm_promises.report_lines(date.fromisoformat(d["day"])))
    return lines, notes


NO_IMPACT = "no impact recorded yet (careerbrew work impact)"


def _why(label: str, tasks: list[dict[str, Any] | None], rank: dict[str, int]) -> str:
    """One plain sentence for the owner: who benefits, what it should change, a checked result (or the metric that
    will show it), and the project's rank. Only facts careerbrew holds (`work impact`, `project_priority`); with no
    checked result it starts "Not sure:", and with no fact it says no impact is recorded yet (issue #226)."""
    seen = list({t["id"]: t for t in tasks if t}.values())
    plans = [t.get("expected_impact") or {} for t in seen]
    who = list(dict.fromkeys(str(p["who"]) for p in plans if p.get("who")))
    hope = next((str(p["hypothesis"]) for p in plans if p.get("hypothesis")), "")
    metric = next((str(p["metric"]) for p in plans if p.get("metric")), "")
    checked = [m for t in seen if (m := t.get("measured_impact") or {}).get("result") and m.get("evidence") and not str(m["result"]).startswith("Not sure:")]
    parts = []
    if who:
        parts.append("helps " + " and ".join(who[:2]))
    if hope:
        parts.append("expected: " + hope)
    if checked:
        parts.append(f"checked: {checked[0]['result']} ({_n(len(checked[0]['evidence']), 'evidence link')})")
    elif metric:
        parts.append("measured by " + metric)
    if label in rank:
        parts.append(f"project {rank[label]} of {len(rank)} in your priority")
    text = "; ".join(p.rstrip(". ") for p in parts)
    if not checked:  # issue #226: every line gets a note; one no result checks says so
        return f"Not sure: {text or NO_IMPACT}."
    return f"{text[:1].upper()}{text[1:]}."


def _line(label: str, g: dict[str, list[Any]], ticket: re.Pattern[str]) -> str:
    """One project's line: done, next, overdue; empty when there is nothing to say."""
    nxt = sorted(g["next"], key=lambda t: bool(re.match(r"review\b", t, re.I)))  # issue #239: project work before "review PRs"
    parts = [p for p in (_done_phrase(g["done"], label, ticket), "next: " + _few(nxt) if nxt else "", "overdue: " + _few(g["late"]) if g["late"] else "", "impact: " + "; ".join(g["impact"]) if g.get("impact") else "") if p]
    states = list(dict.fromkeys(item["state_note"] for item in g["done"] if item.get("state_note")))
    states.sort(key=lambda note: note == "In draft")
    state = " " + ". ".join(states) if states else ""
    return f"{label or 'Other'}: {'; '.join(parts)}.{state}" if parts else ""


def pm_draft_path(day: str) -> Path:
    return path_for(date.fromisoformat(day)).with_name(f"pm-{day}.md")


# A link: a markdown link, a Slack link or mention (`<https://...|label>`, `<#C123|team>`), or a plain URL (issue #227).
_LINKED = re.compile(r"\[[^\]\n]*\]\([^)\s]*\)|<(?:https?://|mailto:|[#@!])[^>\n]*>|https?://\S+")
_PR_REF = re.compile(r"(?<![\w&/#])#\d+\b")


def unlinked_refs(text: str) -> list[str]:
    """The lines that name a PR (`#12`) or a ticket key outside a link: the reader has nothing to click (issue #227).
    Ticket keys are `project_priority.ticket_pattern()` keys, so UTF-8 and ISO-8601 are words."""
    ticket = project_priority.ticket_pattern(project_priority.entries(), re.findall(r"https?://\S+", text))
    out = []
    for line in text.splitlines():
        bare = _LINKED.sub(" ", line)
        if _PR_REF.search(bare) or ticket.search(bare):
            out.append(line.strip())
    return out


class UnlinkedRefError(ValueError):
    """A PM update with PR numbers or ticket keys that are not links; `lines` names each such line."""

    def __init__(self, lines: list[str]):
        self.lines = lines
        shown = "\n".join(f"  {x}" for x in lines)
        super().__init__(f"{len(lines)} line(s) name a PR or ticket with no link; add a link (for example [#12](https://...)) or remove the number:\n{shown}")


def check_refs(text: str) -> None:
    """Refuse a text with an unlinked PR or ticket reference (issue #227); the event holds only the count."""
    if bad := unlinked_refs(text):
        events.emit("day.pm.draft", status="refused", unlinked=len(bad))
        raise UnlinkedRefError(bad)


def save_pm_draft(day: str, text: str) -> None:
    """Keep the owner's edited PM update for one day (encrypted at rest); an empty text resets to the generated draft.
    A text with an unlinked PR or ticket reference is refused before anything is written (`UnlinkedRefError`)."""
    check_refs(text)
    p = pm_draft_path(day)
    if text.strip():
        store.write_text(p, text)
    elif p.exists():
        p.unlink()
    from . import pm_learn

    try:  # the lesson is extra: a failure here never fails the save (issue #185)
        pm_learn.record(day, text)
    except (OSError, ValueError, KeyError) as exc:
        events.emit("day.pm.learned", status="error", error=type(exc).__name__)
    events.emit("day.pm.draft", status="ok", edited=bool(text.strip()))


class SendError(RuntimeError):
    pass


def send_pm(text: str) -> dict[str, Any]:
    """Post the PM update to Slack (`pm_send_via: slack`). Runs only on the owner's explicit Send (web) or `day send --yes`.

    Uses the Slack connector's token (needs the chat:write scope) under the connector host policy.
    """
    from .. import connectors, secrets

    text = text.strip()
    if not text:
        raise SendError("the update is empty")
    check_refs(text)
    target = str(config.get("pm_slack") or "").strip()
    if not target:
        raise SendError("no Slack destination (fix: careerbrew config set pm_slack <#channel or member id>)")
    conn = _slack_source()
    if conn is None:
        raise SendError("Slack is not connected (fix: careerbrew connect slack, with a user token that has chat:write)")
    try:
        token = secrets.resolve(conn["token"])
        resp = connectors.http_json("https://slack.com/api/chat.postMessage", {"Authorization": f"Bearer {token}"}, {"channel": target, "text": text})
    except (connectors.ConnectorError, secrets.SecretError, OSError) as exc:
        raise SendError(f"Slack could not be reached: {exc}") from None
    if not resp.get("ok"):
        raise SendError(f"Slack refused the message: {resp.get('error', 'unknown')} (fix: check pm_slack and that the token has chat:write)")
    events.emit("day.pm.send", status="ok", source_kind="slack")
    from . import pm_learn

    day = calendar.now().date().isoformat()
    try:  # issue #238: the sent text is the day's final text; a learning error never fails a send that worked
        pm_learn.save_sent(day, text, receipt={"channel": str(resp.get("channel") or ""), "ts": str(resp.get("ts") or "")})
        pm_learn.learn_sent(day)
    except (OSError, ValueError) as exc:
        events.emit("day.source.error", source_kind="pm-sent", error=type(exc).__name__)
    return {"via": "slack", "to": target, "ts": resp.get("ts", "")}


def save(day: date | None = None) -> dict[str, Any]:
    d = build(day)
    store.write_text_if_changed(path_for(date.fromisoformat(d["day"])), render(d))  # a read must not look like new data (issue #117)
    events.emit("day.log", status="ok", items=sum(len(v) for v in d["done"].values()))
    return d


def history(limit: int = 60) -> list[str]:
    return sorted((p.stem.removeprefix("log-") for p in paths.sub("day", ".keep").parent.glob("log-*.md")), reverse=True)[:limit]


def read(day: date) -> str | None:
    return store.read_text(path_for(day))
