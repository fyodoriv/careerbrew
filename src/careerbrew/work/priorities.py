"""Task prioritization and work plans.

Tasks live in `<data root>/work/tasks.yaml`:
  - {id, title, impact: 1-5, urgency: 1-5, estimate_h: hours, competencies: [ids], due: YYYY-MM-DD,
     why: text, links: [{label, url}]}
A task from before issue #143 has one `link` string; `load` reads it as `links` (migrate on read).

score = (impact + urgency + promo_bonus + project_bonus) / sqrt(estimate_h)
  promo_bonus = 3 x (the highest gap priority among the task's competencies,
  normalized to 0..1 by the top gap). This is WSJF-style cost of delay over
  job size, tilted toward work that closes promotion gaps.
  project_bonus = 3 x (n - rank + 1) / n when the task belongs to project
  `rank` of the n in the `project_priority` setting (project_priority.py), else 0.
Due dates within 2 days add +3 urgency. `done` records actual hours, which
feeds the plan-accuracy metric in `careerbrew review`.
"""

from __future__ import annotations

import math
from collections.abc import Iterable
from datetime import date, datetime
from typing import Any
from urllib.parse import urlsplit

from .. import events, paths, store

MAX_HOURS = 200.0
MAX_RATIO = 10.0  # one typo (0.05h planned, 200h done) must not inflate every later plan
MAX_LINKS = 8
URL_MAX, LABEL_MAX = 300, 80


def _clock() -> datetime:
    """The pinned clock (`CAREERBREW_NOW`) or local time, as day plans and suggestions use (US-75)."""
    from ..calendar import now  # local: calendar reaches connectors, which reach work

    return now()


def _num(value: Any, default: float) -> float:
    try:
        v = float(value)
    except (TypeError, ValueError):
        return default
    return v if math.isfinite(v) else default


def hours(value: Any) -> float:
    """Validate hours from a user: finite, at least 3 minutes, at most MAX_HOURS."""
    h = float(value)
    if not math.isfinite(h) or not 0.05 <= h <= MAX_HOURS:
        raise ValueError(f"hours must be a number from 0.05 to {MAX_HOURS:g}, not {value!r}")
    return h


def estimate(t: dict[str, Any]) -> float:
    """A task's estimate for planning; bad legacy values fall back to 1 hour."""
    try:
        h = float(t.get("estimate_h", 1))
    except (TypeError, ValueError):
        return 1.0
    return h if math.isfinite(h) and 0 < h <= MAX_HOURS else 1.0


def tasks_path():
    return paths.sub("work", "tasks.yaml")


def is_web(url: str) -> bool:
    """A full http(s) address with a host: the only kind the web app shows as a link (never javascript:, data:, file:)."""
    try:
        parts = urlsplit(url.strip())
    except ValueError:
        return False
    return parts.scheme.lower() in ("http", "https") and bool(parts.hostname)


def parse_link(value: str) -> dict[str, str]:
    """`URL` or `LABEL=URL` (the label when the part after the first `=` is an http(s) address) -> {label, url}.

    A value that is already a full http(s) URL stays whole, so a redirect query (`?next=https://...`) is never split.
    """
    value = " ".join(str(value).split())
    if is_web(value):
        return {"label": "", "url": value[:URL_MAX]}
    label, sep, rest = value.partition("=")
    if sep and label.strip() and is_web(rest):
        return {"label": label.strip()[:LABEL_MAX], "url": rest.strip()[:URL_MAX]}
    return {"label": "", "url": value[:URL_MAX]}


def _short(url: str) -> str:
    """A readable label for a link without one: host and last path part (`github.com/…/app.py`)."""
    if not is_web(url):
        return url
    p = urlsplit(url)
    parts = [x for x in p.path.split("/") if x]
    host = p.hostname or ""
    return host if not parts else f"{host}/{parts[0]}" if len(parts) == 1 else f"{host}/…/{parts[-1]}"


def links(t: dict[str, Any]) -> list[dict[str, str]]:
    """The task's links as {label, url}, each with a label (its own, or a short one made from the url)."""
    out = []
    for raw in t.get("links") or []:
        x = parse_link(raw) if isinstance(raw, str) else {"label": str(raw.get("label") or "")[:LABEL_MAX], "url": str(raw.get("url") or "")[:URL_MAX]}
        if x["url"].strip():
            out.append({"label": x["label"] or _short(x["url"]), "url": x["url"]})
    return out


def web_url(t: dict[str, Any]) -> str:
    """The task's first http(s) link, or "" (one source link for a row, as the day log shows)."""
    return next((x["url"] for x in links(t) if is_web(x["url"])), "")


def context(tasks: list[dict[str, Any]] | None = None) -> dict[str, dict[str, Any]]:
    """Task id -> {links, task_why}: what a task row in the web app shows beside the computed score (issue #143)."""
    return {t["id"]: {"links": links(t), "task_why": str(t.get("why") or "")} for t in (load() if tasks is None else tasks)}


def _migrate(t: dict[str, Any]) -> dict[str, Any]:
    """A task from before issue #143 kept one `link` string: read it as `links`, so a later save stores the new shape."""
    if "link" in t:
        old = " ".join(str(t.pop("link") or "").split())
        have = [x.get("url") if isinstance(x, dict) else x for x in t.get("links") or []]
        if old and old not in have:
            t["links"] = [{"label": "", "url": old[:URL_MAX]}, *(t.get("links") or [])]
    return t


def load() -> list[dict[str, Any]]:
    return [_migrate(t) for t in (store.read_yaml(tasks_path(), {}) or {}).get("tasks", [])]


def save(tasks: list[dict[str, Any]]) -> None:
    store.write_yaml(tasks_path(), {"tasks": tasks})


SOURCES = ("user", "agent", "proposal")


def is_open(t: dict[str, Any]) -> bool:
    """Open and accepted: a task an agent proposed waits in the inbox until the owner accepts it (issue #46)."""
    return t.get("status", "open") == "open"


def add(title: str, impact: int, urgency: int, estimate_h: float, competencies: list[str], due: str | None = None, source: str = "user", why: str = "", links: Iterable[str] = ()) -> dict[str, Any]:
    if due:
        try:
            date.fromisoformat(str(due))
        except ValueError:
            raise ValueError(f"due must be a date like 2026-10-21, not {due!r}") from None
    estimate_h = hours(estimate_h)
    tasks = load()
    tid = f"t{len(tasks) + 1:03d}"
    if source != "user" and not why.strip():
        raise ValueError("an agent-added task needs --why (what made you add it)")
    t = {
        "id": tid,
        "title": title,
        "impact": impact,
        "urgency": urgency,
        "estimate_h": estimate_h,
        "competencies": competencies,
        "due": due,
        "status": "open" if source in ("user", "proposal") else "proposed",
        "source": source,
        "added_at": _clock().isoformat(timespec="seconds"),
        "why": why.strip()[:300],
        "links": [x for x in (parse_link(v) for v in links) if x["url"]][:MAX_LINKS],
    }
    tasks.append(t)
    save(tasks)
    # Counts only (issue #143): how many links a task got, and how many the web app can show as links.
    events.emit("work.task", status=t["status"], source_kind=source, links=len(t["links"]), web_links=sum(is_web(x["url"]) for x in t["links"]), story="us-70")
    return t


def rank(gap_priorities: dict[str, float], today: date | None = None) -> list[dict[str, Any]]:
    from . import project_priority, projects  # local: projects imports this module

    today = today or _clock().date()  # the pinned clock, as the Dashboard and planner use (US-75)
    top = max(gap_priorities.values(), default=0.0) or 1.0
    ranked_projects = project_priority.entries()
    linked = projects.task_projects() if ranked_projects else {}
    from . import ladder_grade  # local: ladder_grade imports promo, which reads evidence

    lid = ladder_grade.target()[0]
    out = []
    for t in load():
        if not is_open(t):
            continue
        urgency = _num(t.get("urgency", 3), 3.0)
        due_days = None
        if t.get("due"):
            try:
                days = (date.fromisoformat(str(t["due"])) - today).days
            except ValueError:
                days = None  # a bad legacy due date must never break planning
            due_days = days
            if days is not None and days <= 2:
                urgency += 3
        dims = t.get("competencies") or ladder_grade.dims_of(str(t.get("title", "")), lid)  # untagged tasks: the ladder reads the title (issue #216)
        bonus = 3 * max((gap_priorities.get(c, 0.0) for c in dims), default=0.0) / top
        impact, size = _num(t.get("impact", 3), 3.0), max(0.25, estimate(t))
        pp = project_priority.of_task(t, linked, ranked_projects)
        pbonus = project_priority.bonus(pp["rank"], len(ranked_projects)) if pp else 0.0
        s = (impact + urgency + bonus + pbonus) / math.sqrt(size)
        # The exact numbers behind the score, so a dashboard can explain it truthfully.
        parts = {"impact": impact, "urgency": urgency, "due_days": due_days, "bonus": round(bonus, 2), "size_h": size, "ladder_dims": dims}
        parts |= {"project": pp["name"] if pp else None, "project_rank": pp["rank"] if pp else None, "projects": len(ranked_projects), "project_bonus": pbonus}
        out.append({**t, "score": round(s, 2), "promo_bonus": round(bonus, 2), "parts": parts})
    return sorted(out, key=lambda t: (-t["score"], t["id"]))


def plan(gap_priorities: dict[str, float], capacity_h: float = 6.0) -> dict[str, Any]:
    ranked = rank(gap_priorities)
    chosen, used = [], 0.0
    for t in ranked:
        if used + estimate(t) <= capacity_h:
            chosen.append(t)
            used += estimate(t)
    events.emit("work.plan", status="ok", tasks=len(chosen))
    return {"capacity_h": capacity_h, "planned_h": used, "today": chosen, "backlog": [t for t in ranked if t not in chosen]}


def regrade(task_id: str, impact: int | None = None, urgency: int | None = None, estimate_h: float | None = None) -> dict[str, Any]:
    """The owner's own grade for a task (issue #185): an accepted task starts at 3/3/1h; this changes it."""
    for name, v in (("impact", impact), ("urgency", urgency)):
        if v is not None and not 1 <= int(v) <= 5:
            raise ValueError(f"{name} is 1 to 5")
    tasks = load()
    for t in tasks:
        if t["id"] == task_id:
            if impact is not None:
                t["impact"] = int(impact)
            if urgency is not None:
                t["urgency"] = int(urgency)
            if estimate_h is not None:
                t["estimate_h"] = hours(estimate_h)
            save(tasks)
            events.emit("work.task", status="ok", action="regrade")
            return t
    raise KeyError(f"unknown task {task_id} (see `careerbrew work plan`)")


def record_impact(
    task_id: str,
    expected: str = "",
    who: str = "",
    metric: str = "",
    result: str = "",
    evidence_links: Iterable[str] = (),
    unsure: bool = False,
) -> dict[str, Any]:
    """Impact as work happens (issue #214): the expected impact at the start, the measured result at delivery.
    A result also becomes a promotion evidence note, so the promo doc and the impact questions read it."""
    links_in = [str(x).strip() for x in evidence_links if str(x).strip()]
    if not (expected or who or metric or result or links_in):
        raise ValueError("nothing to record: pass --expect, --who, --metric, --result, or --evidence")
    tasks = load()
    for t in tasks:
        if t["id"] != task_id:
            continue
        if expected or who or metric:
            plan = dict(t.get("expected_impact") or {})
            for key, value in (("hypothesis", expected), ("who", who), ("metric", metric)):
                if value:
                    plan[key] = " ".join(value.split())[:300]
            t["expected_impact"] = plan
        if result or links_in:
            got = dict(t.get("measured_impact") or {})
            if result:
                got["result"] = ("Not sure: " if unsure and not result.startswith("Not sure:") else "") + " ".join(result.split())[:300]
            got["evidence"] = list(dict.fromkeys([*got.get("evidence", []), *links_in]))[:MAX_LINKS]
            got["at"] = _clock().isoformat(timespec="seconds")
            t["measured_impact"] = got
        save(tasks)
        if result:
            from . import evidence

            evidence.note(t["title"], list(t.get("competencies") or []), impact=t["measured_impact"]["result"], links=t["measured_impact"]["evidence"])
        events.emit("work.task", status="ok", action="impact")
        return t
    raise KeyError(f"unknown task {task_id} (see `careerbrew work plan`)")


def measured_on(day: date) -> list[dict[str, Any]]:
    """Tasks whose measured impact was recorded on `day` (local date), for the daily update."""
    return [t for t in load() if str((t.get("measured_impact") or {}).get("at", ""))[:10] == day.isoformat() and (t.get("measured_impact") or {}).get("result")]


def done(task_id: str, actual_h: float) -> dict[str, Any]:
    actual_h = hours(actual_h)
    tasks = load()
    for t in tasks:
        if t["id"] == task_id:
            if t.get("status") == "done":
                raise ValueError(f"task {task_id} is already done")
            t["status"] = "done"
            t["actual_h"] = actual_h
            t["done_at"] = _clock().isoformat(timespec="seconds")
            save(tasks)
            ratio = min(MAX_RATIO, actual_h / max(0.25, estimate(t)))
            # task and source let calibration learn per project and per source (issue #185)
            store.append_jsonl(paths.sub("metrics.jsonl"), [{"metric": "estimate_ratio", "value": round(ratio, 3), "at": t["done_at"], "task": task_id, "source": str(t.get("source") or "user")}])
            return t
    raise KeyError(f"unknown task {task_id} (see `careerbrew work plan`)")
