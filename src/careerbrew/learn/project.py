"""Learn a project: staged sources, cited concepts, and what to practise next.

The learn-project skill (skills/careerbrew-learn-project) teaches; this module keeps its state
so learning lasts past one chat. Everything lives under `learn/<project>/` in the
data root, encrypted at rest, never in the repo being learned:

- manifest.json   sources: repos by path and commit, other inputs by content hash;
- inputs/         text the agent fetched (docs, pages, tickets), stored encrypted;
- notes/          the study notes the agent writes (`learn note`);
- concepts.yaml   concepts with section, prerequisites, citations, and questions;
- log.jsonl       one row per answer (no answer text);
- model.json      the retest schedule of mastered concepts;
- srs/            an FSRS deck, one card per concept (tutor.cards.deck_root).

How the next step is picked, and why (sources in docs/learn.md):
1. retests of mastered concepts at +1 and +7 days (only a delayed test measures learning);
2. due spaced reviews (FSRS);
3. a pretest per untouched section (questions before reading help, even when missed);
4. the weakest section, prerequisites first, at one Bloom level above the best so far.

Mastered means two correct answers that were not low-confidence guesses, one at
Bloom `apply` or higher. A simulation tried Bayesian Knowledge Tracing instead and
it agreed with the true state less often (the numbers are pinned in tests/unit/test_learn.py).
"""

from __future__ import annotations

import hashlib
import re
import subprocess
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, events, paths, store
from ..tutor import cards

KINDS = ("repo", "doc", "web", "context", "file")
BLOOM = ("remember", "understand", "apply", "analyze", "evaluate", "create")
RESULTS = ("correct", "partial", "wrong", "dontknow")
CONFIDENCE = {"low": 0.33, "medium": 0.66, "high": 0.9}
MODES = ("pretest", "teach", "quiz", "recall", "review", "retest", "task")
RETESTS = {"+1d": timedelta(days=1), "+7d": timedelta(days=7)}
RECALL_CAP = 0.3  # at most this share of questions may be pure recall (`remember`)
REPO_DOCS = ("README*", "AGENTS.md", "ARCHITECTURE*", "CONTRIBUTING*", "docs/**/*.md")


def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.casefold()).strip("-") or "project"


def folder(project: str) -> Path:
    return paths.sub("learn", slug(project), "x").parent


def _manifest(project: str) -> dict[str, Any]:
    return store.read_json(folder(project) / "manifest.json", {}) or {}


def projects() -> list[str]:
    root = paths.sub("learn", "x").parent
    return sorted(p.name for p in root.iterdir() if (p / "manifest.json").exists()) if root.is_dir() else []


def _git(repo: Path, *args: str) -> str:
    out = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, text=True, check=False)
    return out.stdout.strip() if out.returncode == 0 else ""


def _repo_docs(repo: Path) -> list[Path]:
    found = {p for pattern in REPO_DOCS for p in repo.glob(pattern) if p.is_file() and ".git" not in p.parts}
    return sorted(found)


def _source(kind: str, spec: str) -> dict[str, Any]:
    if kind == "repo":
        repo = Path(spec).expanduser().resolve()
        if not repo.is_dir():
            raise FileNotFoundError(f"no folder {repo}: pass a local checkout with --repo")
        docs = _repo_docs(repo)
        h = hashlib.sha256()
        for d in docs:
            h.update(d.relative_to(repo).as_posix().encode() + b"\0" + d.read_bytes())
        return {"key": f"repo:{repo.name}", "kind": "repo", "label": repo.name, "path": str(repo), "sha": _git(repo, "rev-parse", "HEAD"), "hash": h.hexdigest(), "docs": len(docs)}
    label, sep, path = spec.partition(":") if kind != "web" else spec.rpartition(":")
    if not sep or not label or not path:
        raise ValueError(f"--{kind} takes LABEL:PATH (the file you fetched), not {spec!r}")
    src = Path(path).expanduser()
    if not src.is_file():
        raise FileNotFoundError(f"no file {src} for --{kind} {label}")
    data = src.read_bytes()
    return {"key": f"{kind}:{slug(label)}", "kind": kind, "label": label, "file": f"inputs/{kind}/{slug(label)}{src.suffix or '.md'}", "hash": hashlib.sha256(data).hexdigest(), "_src": src, "_data": data}


def start(project: str, specs: list[tuple[str, str]], consume: bool = False) -> dict[str, Any]:
    """Stage sources. Repos are read in place (path + commit); fetched text is stored encrypted.

    `consume` deletes each fetched file after it is stored, so no plaintext copy stays behind.
    Sources whose hash is unchanged are skipped; `freshness.changed` names the ones to re-teach.
    """
    if not specs and not _manifest(project):
        raise ValueError("name at least one source: --repo PATH, --doc LABEL:FILE, --web URL:FILE, --context LABEL:FILE, or --file LABEL:FILE")
    base = folder(project)
    old = {s["key"]: s for s in _manifest(project).get("sources", [])}
    new = dict(old)
    changed, unchanged = [], []
    for kind, spec in specs:
        if kind not in KINDS:
            raise ValueError(f"unknown source kind {kind!r}")
        s = _source(kind, spec)
        same = old.get(s["key"], {}).get("hash") == s["hash"]
        (unchanged if same else changed).append(s["key"])
        if "_data" in s:
            src, data = s.pop("_src"), s.pop("_data")
            if not same:
                text = data.decode("utf-8", errors="replace")
                store.write_text(base / s["file"], text)
            if consume:
                src.unlink()
        s["staged_at"] = calendar.now_utc().isoformat(timespec="seconds")
        new[s["key"]] = s if not same else {**old[s["key"]], "staged_at": s["staged_at"]}
    stale = sorted(set(_manifest(project).get("stale", [])) | {k for k in changed if k in old})
    manifest = {"project": project, "sources": sorted(new.values(), key=lambda r: r["key"]), "freshness": {"changed": changed, "unchanged": unchanged}, "stale": stale}
    store.write_json(base / "manifest.json", manifest)
    events.emit("learn.start", status="ok", sources=len(new), changed=len(changed))
    return manifest


def note(project: str, rel: str, text: str | None = None) -> str:
    if not _manifest(project):
        raise KeyError(f"no learn project {project!r}; start with `careerbrew learn start {project} --repo PATH`")
    rel = rel.strip("/")
    if ".." in Path(rel).parts or not rel.endswith(".md"):
        raise ValueError("a note path is relative and ends in .md, e.g. 01-overview/data-flow.md")
    path = folder(project) / "notes" / rel
    if text is not None:
        store.write_text(path, text)
        return text
    out = store.read_text(path)
    if out is None:
        raise FileNotFoundError(f"no note {rel} in {project}")
    return out


def _notes(project: str) -> set[str]:
    root = folder(project) / "notes"
    return {p.relative_to(root).as_posix() for p in root.rglob("*.md")} if root.is_dir() else set()


def _cite_ok(project: str, cite: str, manifest: dict[str, Any], notes: set[str]) -> bool:
    """A citation names a note, a staged input, or a file inside a staged repo (`repo:<name>/<path>`)."""
    if cite.removeprefix("note:") in notes:
        return True
    for s in manifest.get("sources", []):
        if cite == s["key"] or cite == s.get("file"):
            return True
        if s["kind"] == "repo" and cite.startswith(s["key"] + "/"):
            root = Path(s["path"]).resolve()
            target = (root / cite[len(s["key"]) + 1 :].split("#")[0].split(":")[0]).resolve()
            return target.is_relative_to(root) and target.exists()
    return False


def check_concepts(project: str, concepts: list[dict[str, Any]]) -> list[str]:
    """Every problem that blocks an import: ids, prerequisites (no cycles), citations, questions."""
    manifest, notes, problems = _manifest(project), _notes(project), []
    ids = [str(c.get("id", "")) for c in concepts]
    for i in sorted({i for i in ids if ids.count(i) > 1 or not i}):
        problems.append(f"concept id {i or '(empty)'} is missing or repeated")
    known = set(ids)
    questions = 0
    recall = 0
    for c in concepts:
        cid = c.get("id", "?")
        for field in ("section", "title"):
            if not str(c.get(field, "")).strip():
                problems.append(f"{cid}: needs a {field}")
        for p in c.get("prereqs", []):
            if p not in known:
                problems.append(f"{cid}: prerequisite {p} is not a concept")
        cites = c.get("cites", [])
        if not cites:
            problems.append(f"{cid}: cite a note or a staged source (cite or refuse)")
        for cite in cites:
            if not _cite_ok(project, str(cite), manifest, notes):
                problems.append(f"{cid}: citation {cite} is not a note or a staged source")
        qs = c.get("questions", [])
        if not qs:
            problems.append(f"{cid}: needs at least one question")
        for q in qs:
            questions += 1
            level = q.get("bloom")
            recall += level == "remember"
            if level not in BLOOM:
                problems.append(f"{cid}: question bloom must be one of {', '.join(BLOOM)}")
            if not str(q.get("prompt", "")).strip() or not str(q.get("answer", "")).strip():
                problems.append(f"{cid}: each question needs a prompt and an answer")
            if q.get("options") and len(q.get("misconceptions", [])) != len(q["options"]) - 1:
                problems.append(f"{cid}: tag each wrong option with the misconception it targets")
    if questions and recall / questions > RECALL_CAP:
        problems.append(f"{recall} of {questions} questions are pure recall (remember); keep it at most {int(RECALL_CAP * 100)}%")
    graph = {str(c.get("id")): list(c.get("prereqs", [])) for c in concepts}
    if _order(graph) is None:
        problems.append("prerequisites form a cycle; the study order needs a line, not a loop")
    return problems


def _order(graph: dict[str, list[str]]) -> list[str] | None:
    """Prerequisites first (a topological order that keeps the given order where it can), or None on a cycle."""
    done: list[str] = []
    seen: set[str] = set()
    while len(done) < len(graph):
        ready = [c for c in graph if c not in seen and all(p in seen or p not in graph for p in graph[c])]
        if not ready:
            return None
        seen.add(ready[0])
        done.append(ready[0])
    return done


def concepts(project: str) -> list[dict[str, Any]]:
    return (store.read_yaml(folder(project) / "concepts.yaml", {}) or {}).get("concepts", [])


def import_concepts(project: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    problems = check_concepts(project, rows)
    if problems:
        raise ValueError("concepts not imported:\n  " + "\n  ".join(problems))
    store.write_yaml(folder(project) / "concepts.yaml", {"concepts": rows})
    manifest = _manifest(project)
    if manifest.get("stale"):
        store.write_json(folder(project) / "manifest.json", {**manifest, "stale": []})
    model, ids = _model(project), {c["id"] for c in rows}
    if any(cid not in ids for cid in model.get("retests", {})):
        model["retests"] = {cid: rs for cid, rs in model["retests"].items() if cid in ids}
        store.write_json(folder(project) / "model.json", model)
    deck = []
    for c in rows:
        q = next((q for q in c["questions"] if q.get("kind", "open") == "open"), c["questions"][0])
        deck.append({"id": c["id"], "question": q["prompt"], "answer": q["answer"], "tags": [c["section"]], "source": ", ".join(c["cites"])})
    with cards.deck_root(folder(project) / "srs"):
        cards.write_deck("concepts", deck)
    events.emit("learn.concepts", status="ok", concepts=len(rows))
    return {"concepts": len(rows), "sections": len({c["section"] for c in rows})}


def _model(project: str) -> dict[str, Any]:
    return store.read_json(folder(project) / "model.json", {}) or {}


def _log(project: str) -> list[dict[str, Any]]:
    return store.read_jsonl(folder(project) / "log.jsonl")


def answer(project: str, concept: str, result: str, confidence: str, level: str, mode: str = "quiz", kind: str = "open", seconds: int | None = None, at: datetime | None = None) -> dict[str, Any]:
    """Record one answer: mastery, FSRS grade, retest schedule, and a content-free log row."""
    at = at or calendar.now_utc()
    if result not in RESULTS or confidence not in CONFIDENCE or level not in BLOOM or mode not in MODES:
        raise ValueError(f"result: {RESULTS}; confidence: {tuple(CONFIDENCE)}; level: {BLOOM}; mode: {MODES}")
    by_id = {c["id"]: c for c in concepts(project)}
    if concept not in by_id:
        raise KeyError(f"unknown concept {concept} in {project}; see `careerbrew learn status {project}`")
    model = _model(project)
    log = [r for r in _log(project) if r["concept"] == concept]
    row = {"concept": concept, "section": by_id[concept]["section"], "result": result, "confidence": confidence, "level": level, "mode": mode, "kind": kind, "at": at.isoformat(timespec="seconds"), "seconds": seconds}
    mastered = _mastered([*log, row])
    retests = model.setdefault("retests", {})
    if mode == "retest":
        pending = [r for r in retests.get(concept, []) if "result" not in r and r["due"] <= at.isoformat()]
        if pending:
            pending[0]["result"] = result
    if not mastered and concept in retests and mode in ("retest", "review") and result in ("wrong", "dontknow"):
        model.setdefault("retest_log", []).extend(r for r in retests.pop(concept) if "result" in r)
    if mastered and concept not in retests:
        retests[concept] = [{"kind": k, "due": (at + d).isoformat(timespec="seconds")} for k, d in RETESTS.items()]
    if mode != "pretest":
        rating = "again" if result in ("wrong", "dontknow") else "hard" if result == "partial" or confidence == "low" else "easy" if confidence == "high" and BLOOM.index(level) >= 2 else "good"
        with cards.deck_root(folder(project) / "srs"):
            if concept in cards._suspended():
                cards.unsuspend(concept)  # answering it again is the re-teach a leech asks for
            row["leech"] = cards.review(concept, rating, at=at, seconds=seconds)["leech"]
    store.write_json(folder(project) / "model.json", model)
    row["mastered"] = mastered
    store.append_jsonl(folder(project) / "log.jsonl", [row])
    events.emit("learn.answer", status=result, mode=mode)
    return row


def _counts(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Answers that count as evidence: not a pretest, not a low-confidence guess, and after the last
    missed retest or review (a delayed miss means teach again)."""
    misses = [i for i, r in enumerate(rows) if r["mode"] in ("retest", "review") and r["result"] in ("wrong", "dontknow")]
    rows = rows[misses[-1] + 1 :] if misses else rows
    return [r for r in rows if r["mode"] != "pretest" and r["result"] == "correct" and r["confidence"] != "low"]


def _mastered(rows: list[dict[str, Any]]) -> bool:
    good = _counts(rows)
    return len(good) >= 2 and any(BLOOM.index(r["level"]) >= BLOOM.index("apply") for r in good)


def _best(rows: list[dict[str, Any]]) -> str | None:
    good = _counts(rows)
    return max((r["level"] for r in good), key=BLOOM.index) if good else None


def _by_concept(log: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    out: dict[str, list[dict[str, Any]]] = {}
    for r in log:
        out.setdefault(r["concept"], []).append(r)
    return out


def _brier(rows: list[dict[str, Any]]) -> float | None:
    """Mean squared gap between stated confidence and the result (0 is perfect)."""
    scored = [r for r in rows if r["result"] != "partial"]
    if not scored:
        return None
    return round(sum((CONFIDENCE[r["confidence"]] - (r["result"] == "correct")) ** 2 for r in scored) / len(scored), 3)


def status(project: str, at: datetime | None = None) -> dict[str, Any]:
    at = at or calendar.now_utc()
    cs, model, log = concepts(project), _model(project), _log(project)
    if not _manifest(project):
        raise KeyError(f"no learn project {project!r}; start with `careerbrew learn start {project} --repo PATH`")
    rows = _by_concept(log)

    def is_mastered(cid: str) -> bool:
        return _mastered(rows.get(cid, []))

    sections: dict[str, dict[str, Any]] = {}
    for c in cs:
        s = sections.setdefault(c["section"], {"concepts": 0, "mastered": 0, "remaining": []})
        s["concepts"] += 1
        if is_mastered(c["id"]):
            s["mastered"] += 1
        else:
            s["remaining"].append(c["id"])
    for s in sections.values():
        s["known_pct"] = round(100 * s["mastered"] / s["concepts"])
    with cards.deck_root(folder(project) / "srs"):
        srs = cards.stats(at)
        states = cards.scheduled()
    taught = {r["concept"] for r in log if r["mode"] != "pretest"}
    ids = {c["id"] for c in cs}
    retests = [r for cid, rs in model.get("retests", {}).items() if cid in ids for r in rs]
    week = [r for r in [*retests, *model.get("retest_log", [])] if r["kind"] == "+7d" and "result" in r]
    sources = _manifest(project).get("sources", [])
    return {
        "project": project,
        "concepts": len(cs),
        "mastered": sum(is_mastered(c["id"]) for c in cs),
        "sections": sections,
        "taught": len(taught),
        "scheduled_share": round(sum(c in states for c in taught) / len(taught), 3) if taught else None,
        "due_reviews": srs["due_now"],
        "due_retests": sum(1 for r in retests if "result" not in r and r["due"] <= at.isoformat()),
        "retention_7d": srs["retention_7d"],
        "retest_retained_7d": round(sum(r["result"] == "correct" for r in week) / len(week), 3) if week else None,
        "brier": _brier([r for r in log if r["mode"] != "pretest"]),
        "answers": len(log),
        "sources": len(sources),
        "stale": _manifest(project).get("stale", []),
    }


def next_step(project: str, at: datetime | None = None) -> dict[str, Any]:
    """The one thing to practise next, with its mode and the Bloom level to ask at."""
    at = at or calendar.now_utc()
    cs, model, log = concepts(project), _model(project), _log(project)
    if not cs:
        return {"mode": "build", "why": "no concepts yet: write notes, then `careerbrew learn concepts` (skills/careerbrew-learn-project)"}
    by_id = {c["id"]: c for c in cs}
    for cid, rs in sorted(model.get("retests", {}).items()):
        if cid not in by_id:
            continue
        for r in rs:
            if "result" not in r and r["due"] <= at.isoformat():
                return _step(by_id[cid], "retest", f"{r['kind']} retest of a mastered concept, in new words", _by_concept(log))
    with cards.deck_root(folder(project) / "srs"):
        due = [c for c in cards.queue(at, new_per_day=0) if c["mode"] == "review"]
    if due:
        return _step(by_id[due[0]["id"]], "review", "a spaced review is due", _by_concept(log))
    touched = {r["section"] for r in log}
    order = _order({c["id"]: list(c.get("prereqs", [])) for c in cs}) or [c["id"] for c in cs]
    for cid in order:
        if by_id[cid]["section"] not in touched:
            return _step(by_id[cid], "pretest", "a new section starts with an unscored pretest: predict before you read", _by_concept(log))
    rows = _by_concept(log)

    def mastered(cid: str) -> bool:
        return _mastered(rows.get(cid, []))

    open_ids = [cid for cid in order if not mastered(cid)]
    if not open_ids:
        return {"mode": "done", "why": "every concept is mastered; reviews and retests come back on their own"}
    weak: dict[str, list[float]] = {}
    for c in cs:
        weak.setdefault(c["section"], []).append(float(mastered(c["id"])))
    for section in sorted(weak, key=lambda s: sum(weak[s]) / len(weak[s])):
        for cid in open_ids:
            c = by_id[cid]
            blocked = [p for p in c.get("prereqs", []) if not mastered(p)]
            if c["section"] == section and not blocked:
                seen = any(r["concept"] == cid and r["mode"] != "pretest" for r in log)
                return _step(c, "quiz" if seen else "teach", "weakest section first, prerequisites done", rows)
    first = by_id[open_ids[0]]
    return _step(first, "teach", "its prerequisites come first", rows)


def _step(c: dict[str, Any], mode: str, why: str, rows: dict[str, list[dict[str, Any]]]) -> dict[str, Any]:
    best = _best(rows.get(c["id"], []))
    target = "understand" if best is None else BLOOM[min(BLOOM.index(best) + 1, len(BLOOM) - 1)]
    if mode in ("retest", "review"):
        target = best or "understand"
    qs = sorted(c["questions"], key=lambda q: abs(BLOOM.index(q["bloom"]) - BLOOM.index(target)))
    return {"mode": mode, "why": why, "concept": c["id"], "section": c["section"], "title": c["title"], "level": target, "question": qs[0], "cites": c["cites"]}
