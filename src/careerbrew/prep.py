"""Interactive preparation check for one company's interview loop.

For each step of the loop (interview.loop_for) it builds a checklist:
- `auto` items measured from your data: tutor readiness on the step's topic
  (>= 0.7), story bank size for behavioral steps (>= 6 STAR stories covering
  >= 4 competencies);
- `ask` items you confirm: logistics (date, time zone, link, environment
  test), company research (values, product, recent news), questions to ask,
  salary range researched (offer step), and each step's own `prep` list.

`careerbrew prep check <company>` asks every `ask` item on a terminal (y/n,
Enter keeps the last answer), or reads `--yes id,id` / `--answers file.json`
for agents. Score = share of weighted items done; gaps come back sorted by
weight with the action that closes each one. History goes to
`<data root>/prep/<company>.json` so the score trend is visible.
Story bank: `careerbrew prep story add` → `<data root>/prep/stories.yaml`.
"""

from __future__ import annotations

import json
import sys
from collections.abc import Callable
from pathlib import Path
from typing import Any

from . import calendar, events, interview, paths, store
from .tutor import engine as tutor

READY = 0.7
MIN_STORIES = 6
MIN_COMPETENCIES = 4

GLOBAL_ASK = [
    {"id": "logistics", "text": "Date, time zone, interviewer names, and the call or onsite link are confirmed", "weight": 2, "action": "Reply to the recruiter to confirm the schedule and ask who you will meet."},
    {"id": "environment", "text": "Camera, mic, IDE or coding pad, and internet are tested in the same setup", "weight": 1, "action": "Do a 10-minute test call and open the coding tool they named."},
    {
        "id": "company-research",
        "text": "You can explain the product, the team's mission, and two recent company events",
        "weight": 2,
        "action": "Read the careers page, the engineering blog, and the latest company news; write 5 bullet notes.",
    },
    {"id": "questions", "text": "You have at least 3 questions to ask each interviewer type", "weight": 1, "action": "Write 3 questions per interviewer: about the team, the work, and how success is measured."},
    {"id": "why-company", "text": "You have a 60-second, specific answer to 'why this company'", "weight": 2, "action": "Draft it from your Want score reasons and the company's mission; say it out loud twice."},
]


def stories_path() -> Path:
    return paths.sub("prep", "stories.yaml")


def stories() -> list[dict[str, Any]]:
    return (store.read_yaml(stories_path(), {}) or {}).get("stories", [])


def add_story(title: str, competencies: list[str], situation: str = "", action: str = "", result: str = "") -> dict[str, Any]:
    data = {"stories": stories()}
    s = {"id": f"s{len(data['stories']) + 1:02d}", "title": title, "competencies": competencies, "situation": situation, "action": action, "result": result, "added": calendar.now_utc().date().isoformat()}
    data["stories"].append(s)
    store.write_yaml(stories_path(), data)
    return s


def _auto_items(step: dict[str, Any]) -> list[dict[str, Any]]:
    items = []
    topic = interview.STEP_TOPICS.get(step["id"])
    if topic:
        r = tutor.readiness([topic])
        items.append(
            {
                "id": f"{step['id']}:readiness",
                "text": f"Drill readiness on {topic} is at least {READY:.0%} (now {r:.0%})",
                "weight": 3,
                "done": r >= READY,
                "auto": True,
                "action": f"Run `careerbrew drill session` focused on {topic} until readiness reaches {READY:.0%}.",
            }
        )
    if step["id"] in ("behavioral", "bar-raiser", "hiring-manager"):
        st = stories()
        comps = {c for s in st for c in s.get("competencies", [])}
        ok = len(st) >= MIN_STORIES and len(comps) >= MIN_COMPETENCIES
        items.append(
            {
                "id": f"{step['id']}:stories",
                "text": f"Story bank has >= {MIN_STORIES} STAR stories over >= {MIN_COMPETENCIES} competencies (now {len(st)} stories, {len(comps)} competencies)",
                "weight": 3,
                "done": ok,
                "auto": True,
                "action": "Add stories with `careerbrew prep story add` (leadership, conflict, failure, impact, ambiguity, ownership).",
            }
        )
    return items


def _stale_path() -> Path:
    return paths.sub("prep", "stale.json")


def mark_stale(company: str, reason: str) -> None:
    """Called by `careerbrew changes` when a company's loop or sources change."""
    marks = store.read_json(_stale_path(), {}) or {}
    marks[company.casefold()] = {"company": company, "reason": reason, "at": calendar.now_utc().date().isoformat()}
    store.write_json(_stale_path(), marks)


def stale_mark(company: str) -> dict[str, Any] | None:
    return (store.read_json(_stale_path(), {}) or {}).get(company.casefold())


def checklist(company: str, seniority: int = 3) -> dict[str, Any]:
    info = interview.loop_for(company, seniority)
    items: list[dict[str, Any]] = [{**g, "step": "all", "auto": False} for g in GLOBAL_ASK]
    mark = stale_mark(info["company"])
    if mark:
        items.insert(
            0,
            {
                "id": "company:changed",
                "text": f"{info['company']}'s interview process or sources changed on {mark['at']} ({mark['reason']}); you re-read the loop with `careerbrew interview {info['company']} --show`",
                "weight": 4,
                "step": "all",
                "auto": False,
                "action": f'Re-read the updated loop: careerbrew interview "{info["company"]}" --show',
            },
        )
    for st in info["loop"]:
        items.extend({**a, "step": st["id"]} for a in _auto_items(st))
        for n, p in enumerate(st.get("prep", []) or [], 1):
            items.append({"id": f"{st['id']}:prep{n}", "text": p if isinstance(p, str) else p.get("text", ""), "weight": 1, "step": st["id"], "auto": False, "action": p if isinstance(p, str) else p.get("text", "")})
        if st["id"] == "offer-negotiation":
            items.append(
                {
                    "id": "offer:range",
                    "text": "You know the market salary range for this level and city from at least two sources",
                    "weight": 2,
                    "step": st["id"],
                    "auto": False,
                    "action": "Check levels.fyi and your country's public wage data for the role in your city; write your walk-away number.",
                }
            )
    return {"company": info["company"], "known_loop": info["known"], "items": items}


def history_path(company: str) -> Path:
    slug = "".join(ch if ch.isalnum() else "-" for ch in company.casefold()).strip("-") or "company"
    return paths.sub("prep", f"{slug}.json")


def run(company: str, answers: dict[str, bool] | None = None, ask: Callable[[str], str] | None = None, seniority: int = 3) -> dict[str, Any]:
    cl = checklist(company, seniority)
    hist = store.read_json(history_path(company), {"runs": []}) or {"runs": []}
    last = hist["runs"][-1]["answers"] if hist["runs"] else {}
    answers = dict(answers or {})
    for it in cl["items"]:
        if it["auto"]:
            continue
        if it["id"] in answers:
            it["done"] = bool(answers[it["id"]])
        elif ask is not None:
            prev = last.get(it["id"])
            default = "y" if prev else "n"
            reply = ask(f"[{it['step']}] {it['text']}? (y/n, Enter={default}) ").strip().lower() or default
            it["done"] = reply.startswith("y")
        else:
            it["done"] = bool(last.get(it["id"], False))
        answers[it["id"]] = it["done"]
    total = sum(it["weight"] for it in cl["items"]) or 1
    score = round(sum(it["weight"] for it in cl["items"] if it["done"]) / total, 3)
    gaps = sorted((it for it in cl["items"] if not it["done"]), key=lambda it: (-it["weight"], it["id"]))
    if answers.get("company:changed"):
        marks = store.read_json(_stale_path(), {}) or {}
        if marks.pop(cl["company"].casefold(), None) is not None:
            store.write_json(_stale_path(), marks)
        answers.pop("company:changed", None)  # a new change must be confirmed again
    run_row = {"at": calendar.now_utc().isoformat(timespec="seconds"), "score": score, "answers": {k: v for k, v in answers.items()}}
    hist["runs"].append(run_row)
    store.write_json(history_path(company), hist)
    trend = [r["score"] for r in hist["runs"][-5:]]
    events.emit("prep.check", status="ok", score=score, gaps=len(gaps))
    return {"company": cl["company"], "known_loop": cl["known_loop"], "score": score, "trend": trend, "items": cl["items"], "gaps": gaps}


def load_answers(path: str | None, yes: str | None) -> dict[str, bool]:
    out: dict[str, bool] = {}
    if path:
        out.update({k: bool(v) for k, v in json.loads(Path(path).read_text(encoding="utf-8")).items()})
    for i in (yes or "").split(","):
        if i.strip():
            out[i.strip()] = True
    return out


def interactive() -> bool:
    return sys.stdin.isatty()
