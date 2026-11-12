"""Import a Notion career workspace (a bundle the `notion-import` skill writes) into careerbrew.

The agent reads Notion through its Notion connector and writes a bundle folder
(default `<data root>/imports/notion/`), one JSON file per kind, plus
`manifest.json` with the counts it found. The table and `STAGE_MAP` are an example schema:

| File | Each row | Lands in |
|---|---|---|
| pages.json | {id, title, path, section, updated, markdown, truncated?} | Library (`library/notion/`) |
| jobs.json | {id, title "Company: role", stage, want, salary, location, remote, url, reason_no, last_my_contact, last_their_contact, created, markdown} | Jobs board (`jobs/notion-board.json`); applied, rejected, and hired rows also go to the application log; rows with notes also go to Library, section `job` |
| flashcards.json | {id, question, answer, tags, confidence, priority} | the `notion` practice deck (`careerbrew cards`) |
| plan.json | {id, goal, status, markdown} | Library, section `plan` |
| skills.json | {id, name, years, highlight} | `profile.yaml` skills (added, never overwritten) |
| stories.json | {id, title, tags, markdown} | the interview story bank (`prep/stories.yaml`), plus Library, section `story` |
| past_job.json | {id, title, person, employer, updated, markdown} | Library, section `past-job`, plus a work-evidence note; person and employer become leak terms |

Re-running the import replaces what an earlier import wrote (every record carries
`origin: notion`) and keeps practice history. Nothing is ever written to Notion.
All content stays in the data root; logs carry counts only.
"""

from __future__ import annotations

import re
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from . import calendar, events, machine, paths, prep, store, vault
from .jobs import strategy
from .tutor import cards
from .work import evidence

KINDS = ("pages", "jobs", "flashcards", "plan", "skills", "stories", "past_job")
STAGE_MAP = {  # an example schema: the stage names of a sample Notion jobs board
    "Not started": None,
    "Resume stage": "applied",
    "No: Resume": "applied",
    "HR stage": "screen",
    "Quiz stage": "screen",
    "No: HR": "rejected",
    "No: Quiz": "rejected",
    "Onsite stage": "onsite",
    "No: Onsite": "rejected",
    "Negotiation stage": "offer",
    "No: Negotiation": "rejected",
    "Hired": "offer",
}
REQUIRED = {"pages": ("id", "title"), "jobs": ("id", "title"), "flashcards": ("id",), "plan": ("id", "goal"), "skills": ("id", "name"), "stories": ("id", "title"), "past_job": ("id", "title")}
NO_ANSWER = "No answer written in Notion yet. Say your answer out loud, then write it on the card."


class ImportError_(ValueError):  # noqa: N801  (avoid shadowing the builtin)
    pass


def bundle_dir() -> Path:
    return paths.sub("imports", "notion")


def _library_dir() -> Path:
    return paths.sub("library", "notion")


def _board_path() -> Path:
    return paths.sub("jobs", "notion-board.json")


def _slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.casefold()).strip("-")[:60] or "page"


def load(folder: Path) -> dict[str, list[dict[str, Any]]]:
    manifest = store.read_json(folder / "manifest.json", None)
    if not isinstance(manifest, dict):
        raise ImportError_(f"no manifest.json in {folder.name}/ (the notion-import skill writes it)")
    raw = manifest.get("counts")
    counts: dict[str, Any] = raw if isinstance(raw, dict) else {}
    missing = [k for k in KINDS if k not in counts]
    if missing:
        raise ImportError_(f"manifest counts must list every kind (missing: {', '.join(missing)}); write 0 for a kind with no rows")
    data: dict[str, list[dict[str, Any]]] = {}
    for kind in KINDS:
        if counts[kind] and not (folder / f"{kind}.json").exists():
            raise ImportError_(f"{kind}.json is missing but the manifest counts {counts[kind]} rows")
        rows = store.read_json(folder / f"{kind}.json", [])
        if not isinstance(rows, list):
            raise ImportError_(f"{kind}.json must be a list")
        for n, row in enumerate(rows, 1):
            lacking = [k for k in REQUIRED[kind] if not isinstance(row, dict) or k not in row]
            if lacking:
                raise ImportError_(f"{kind}.json row {n} lacks {', '.join(lacking)} (rerun the notion-import skill)")
        want = counts[kind]
        if want != len(rows):
            raise ImportError_(f"{kind}: manifest says {want}, file has {len(rows)} (the fetch is incomplete; rerun the notion-import skill)")
        data[kind] = rows
    return data


def _years_to_rating(years: Any) -> int:
    try:
        y = float(years or 0)
    except (TypeError, ValueError):
        return 1
    return 1 if y < 1 else 2 if y < 2 else 3 if y < 4 else 4 if y < 7 else 5


def _iso(value: Any) -> str:
    """Notion dates come as YYYY-MM-DD or ISO datetimes; every store compares aware UTC datetimes."""
    if not value:
        return ""
    text = str(value).replace("Z", "+00:00")
    try:
        dt = datetime.fromisoformat(text)
    except ValueError:
        return ""
    if dt.tzinfo is None:
        dt = dt.replace(tzinfo=UTC)
    return dt.astimezone(UTC).isoformat(timespec="seconds")


def plain(text: str) -> str:
    """Card questions are shown as plain text: drop Notion's bold, italics, code ticks, and escapes."""
    t = re.sub(r"(\*\*|__|`)", "", str(text))
    t = re.sub(r"\\([\\`*_{}\[\]()#+\-.!^~|>])", r"\1", t)
    return t.strip()


def _company(title: str) -> tuple[str, str]:
    company, _, role = title.partition(":")
    return company.strip(), role.strip()


def _write_library(entries: list[dict[str, Any]]) -> None:
    folder = _library_dir()
    folder.mkdir(parents=True, exist_ok=True)
    for old in folder.glob("*.md"):
        old.unlink()
    index = []
    for e in entries:
        name = f"{_slug(e['title'])}-{str(e['id'])[:8]}.md"
        store.write_text(folder / name, e.get("markdown") or "")
        index.append({k: e.get(k) for k in ("id", "title", "section", "path", "updated", "status", "truncated")} | {"file": name})
    store.write_json(folder / "index.json", index)


def _rewrite_jsonl(path: Path, keep: list[dict[str, Any]], add: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    store.write_jsonl(path, [*keep, *add])


def run(folder: Path | None = None) -> dict[str, Any]:
    machine.guard("jobs")  # job-hunt history and interview prep stay off corporate machines
    folder = folder or bundle_dir()
    data = load(folder)
    root = paths.data_root()

    library = [{**p, "section": p.get("section") or "career"} for p in data["pages"]]
    library += [{"id": p["id"], "title": p["goal"], "section": "plan", "status": p.get("status"), "markdown": p.get("markdown", "")} for p in data["plan"]]
    library += [{**p, "section": "past-job"} for p in data["past_job"]]
    if data["skills"]:
        lines = [f"- **{s['name']}**: {s.get('years') or 0} years. {s.get('highlight') or ''}".rstrip() for s in data["skills"]]
        library.append({"id": "skills", "title": "My skills", "section": "career", "markdown": "\n".join(lines)})
    library += [{"id": st["id"], "title": st["title"], "section": "story", "markdown": st.get("markdown", "")} for st in data["stories"]]
    library += [{"id": j["id"], "title": j["title"], "section": "job", "status": j.get("stage"), "markdown": j["markdown"]} for j in data["jobs"] if str(j.get("markdown") or "").strip()]
    _write_library(library)

    board = []
    for j in data["jobs"]:
        company, role = _company(str(j["title"]))
        board.append(
            {
                "id": j["id"],
                "company": company,
                "role": role,
                **{k: j.get(k) for k in ("stage", "want", "salary", "location", "remote", "url", "reason_no", "last_my_contact", "last_their_contact", "next_follow_up", "difficulty", "created", "markdown")},
            }
        )
    store.write_json(_board_path(), board)

    apps_path = strategy.applications_path()
    keep = [r for r in store.read_jsonl(apps_path) if r.get("origin") != "notion"]
    add = []
    unmapped = sorted({str(j.get("stage")) for j in board if str(j.get("stage")) not in STAGE_MAP})
    for j in board:
        stage = STAGE_MAP.get(str(j.get("stage")))
        if stage:
            at = j.get("last_their_contact") or j.get("last_my_contact") or j.get("created") or ""
            add.append({"company": j["company"], "tier": "history", "stage": stage, "at": _iso(at), "origin": "notion", "notion_id": j["id"]})
    _rewrite_jsonl(apps_path, keep, add)

    blank = [c for c in data["flashcards"] if not str(c.get("question") or "").strip()]
    deck = [{"id": f"notion-{c['id']}", "question": plain(c.get("question") or ""), "answer": c.get("answer") or c.get("markdown") or NO_ANSWER, "tags": c.get("tags") or [], "source": "notion"} for c in data["flashcards"]]
    n_cards = cards.write_deck("notion", deck)

    prof_path = root / "profile.yaml"
    if not prof_path.exists():  # start from the template, so `careerbrew init` never finds a skills-only profile
        example = paths.REPO_ROOT / "profile" / "examples" / "profile.yaml"
        store.write_text(prof_path, machine.work_profile(example.read_text(encoding="utf-8")) if machine.role() == "corporate" else example.read_text(encoding="utf-8"))
    prof = store.read_yaml(prof_path, {}) or {}
    skills = dict(prof.get("skills") or {})
    have = {str(k).casefold() for k in skills}
    added_skills = [s for s in data["skills"] if str(s["name"]).casefold() not in have]
    for s in added_skills:
        skills[str(s["name"]).casefold()] = _years_to_rating(s.get("years"))
    if added_skills:
        store.write_yaml(prof_path, {**prof, "skills": skills})

    story_data = {"stories": [s for s in prep.stories() if s.get("origin") != "notion"]}
    for s in data["stories"]:
        story_data["stories"].append(
            {
                "id": f"notion-{s['id']}",
                "title": s["title"],
                "competencies": s.get("tags") or ["behavioral"],
                "situation": s.get("markdown", ""),
                "action": "",
                "result": "",
                "added": calendar.now_utc().date().isoformat(),
                "origin": "notion",
            }
        )
    store.write_yaml(prep.stories_path(), story_data)

    ev_keep = [e for e in evidence.load() if e.get("origin") != "notion"]
    ev_add = [
        {
            "id": f"notion:{p['id']}",
            "kind": "note",
            "title": f"1:1 notes with {p.get('person') or 'a report'} ({p.get('employer') or 'past job'})",
            "impact": "",
            "at": _iso(p.get("updated")) or calendar.now_utc().isoformat(timespec="seconds"),
            "tags": ["leadership", "mentoring"],
            "origin": "notion",
        }
        for p in data["past_job"]
    ]
    _rewrite_jsonl(evidence.evidence_path(), ev_keep, ev_add)

    terms_path = root / "sensitive-terms.txt"
    existing = (store.read_text(terms_path) or "").splitlines()
    new_terms = sorted({str(v) for p in data["past_job"] for v in (p.get("person"), p.get("employer")) if v} - set(existing))
    if new_terms:
        store.write_text(terms_path, "\n".join([*existing, *new_terms]) + "\n")

    vault.seal_files(sorted(folder.glob("*")))  # the bundle came in as plaintext from the fetch; keep it encrypted from now on
    counts = {k: len(v) for k, v in data.items()}
    events.emit("notion.import", status="ok", items=sum(counts.values()))
    return {"counts": counts, "unmapped_stages": unmapped, "skipped_empty": len(blank), "cards": n_cards, "applications": len(add), "skills_added": len(added_skills), "leak_terms_added": len(new_terms)}


def library() -> list[dict[str, Any]]:
    return store.read_json(_library_dir() / "index.json", []) or []


def page(page_id: str) -> dict[str, Any] | None:
    for e in library():
        if e["id"] == page_id:
            f = _library_dir() / e["file"]
            return {**e, "markdown": store.read_text(f, "") or ""}
    return None


def jobs_board() -> list[dict[str, Any]]:
    return store.read_json(_board_path(), []) or []


def check(folder: Path | None = None) -> dict[str, Any]:
    """Compare the bundle's counts with what the stores hold now."""
    data = load(folder or bundle_dir())
    lib_ids = {e["id"] for e in library()}
    deck = {i for i, c in cards.all_cards().items() if c.get("deck") == "notion"}
    board = {j["id"] for j in jobs_board()}
    prof_skills = {str(k).casefold() for k in ((store.read_yaml(paths.data_root() / "profile.yaml", {}) or {}).get("skills") or {})}
    story_ids = {s["id"] for s in prep.stories() if s.get("origin") == "notion"}
    present = {
        "pages": sum(p["id"] in lib_ids for p in data["pages"]),
        "plan": sum(p["id"] in lib_ids for p in data["plan"]),
        "past_job": sum(p["id"] in lib_ids for p in data["past_job"]),
        "jobs": sum(j["id"] in board for j in data["jobs"]),
        "flashcards": sum(f"notion-{c['id']}" in deck for c in data["flashcards"] if str(c.get("question") or "").strip()),
        "skills": sum(str(s["name"]).casefold() in prof_skills for s in data["skills"]),
        "stories": sum(f"notion-{s['id']}" in story_ids for s in data["stories"]),
    }
    blank = sum(not str(c.get("question") or "").strip() for c in data["flashcards"])
    expected = {k: len(v) for k, v in data.items()}
    expected["flashcards"] -= blank  # blank "New page" rows in Notion have nothing to practise
    total_present, total_expected = sum(present.values()), sum(expected.values())
    return {"ok": total_present == total_expected, "imported": total_present, "expected": total_expected, "skipped_empty": blank, "by_kind": {k: f"{present[k]}/{expected[k]}" for k in KINDS}}
