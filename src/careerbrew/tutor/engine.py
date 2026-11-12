"""Learner model and drill engine.

- Items come from knowledge/topics.yaml, plugin `knowledge` files with items,
  and generated skill-gap items (`add_gap_items`).
- Each item has an FSRS card (py-fsrs) in `<data root>/tutor/cards.json`.
- Each attempt records which binary rubric criteria were met. The share met
  maps to an FSRS rating: < 0.5 Again, < 0.7 Hard, < 0.9 Good, else Easy.
- Mastery per topic = mean over the topic's items of
  (current retrievability x last rubric score); unseen items count as 0.
- Readiness for a set of topics = mean mastery.

Lifecycle: all state is in files under the data root; nothing is kept in memory.
"""

from __future__ import annotations

from datetime import datetime
from pathlib import Path
from typing import Any

from fsrs import Card, Rating, Scheduler

from .. import calendar, events, paths, plugins, store

RATING_NAMES = {Rating.Again: "again", Rating.Hard: "hard", Rating.Good: "good", Rating.Easy: "easy"}


def now() -> datetime:
    return calendar.now_utc()


def rubrics() -> dict[str, dict[str, Any]]:
    out = {}
    for _origin, path in plugins.resources("rubrics"):
        if path.suffix in (".yaml", ".yml"):
            data = store.read_yaml(path, {}) or {}
            out[data.get("id", path.stem)] = data
    return out


def _gap_items_path() -> Path:
    return paths.sub("tutor", "gap-items.yaml")


def items() -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    base = store.read_yaml(paths.REPO_ROOT / "knowledge" / "topics.yaml", {}) or {}
    for it in base.get("items", []):
        out[it["id"]] = {**it, "origin": "base"}
    for origin, path in plugins.resources("knowledge"):
        if path.suffix in (".yaml", ".yml"):
            for it in (store.read_yaml(path, {}) or {}).get("items", []):
                out[it["id"]] = {**it, "origin": origin}
    for it in (store.read_yaml(_gap_items_path(), {}) or {}).get("items", []):
        out[it["id"]] = {**it, "origin": "generated"}
    return out


def add_gap_items(skills: list[str]) -> list[str]:
    data = store.read_yaml(_gap_items_path(), {"items": []}) or {"items": []}
    have = {it["id"] for it in data["items"]}
    added = []
    for skill in skills:
        slug = "".join(ch if ch.isalnum() else "-" for ch in skill.lower()).strip("-")
        iid = f"gap-{slug}-1"
        if iid in have:
            continue
        data["items"].append(
            {
                "id": iid,
                "topic": skill,
                "round": "skill",
                "rubric": "skill",
                "teach": f"Study {skill}: core concept, when to use it, one real example, one pitfall. Use the official docs first.",
                "prompt": f"Explain {skill} as you would to an interviewer: the core idea, when you would and would not use it, a concrete example, and a common pitfall.",
            }
        )
        added.append(iid)
    store.write_yaml(_gap_items_path(), data)
    return added


def _cards_path() -> Path:
    return paths.sub("tutor", "cards.json")


def _attempts_path() -> Path:
    return paths.sub("tutor", "attempts.jsonl")


def cards() -> dict[str, Card]:
    raw = store.read_json(_cards_path(), {}) or {}
    return {k: Card.from_dict(v) for k, v in raw.items()}


def _save_cards(cs: dict[str, Card]) -> None:
    store.write_json(_cards_path(), {k: v.to_dict() for k, v in cs.items()})


def attempts() -> list[dict[str, Any]]:
    return store.read_jsonl(_attempts_path())


def score_to_rating(score: float) -> Rating:
    if score < 0.5:
        return Rating.Again
    if score < 0.7:
        return Rating.Hard
    if score < 0.9:
        return Rating.Good
    return Rating.Easy


def next_item(focus_topics: list[str] | None = None, at: datetime | None = None) -> dict[str, Any] | None:
    """Due reviews first (retrieval practice), then new items in the weakest focus topic."""
    at = at or now()
    its = items()
    cs = cards()
    due = sorted((c.due, iid) for iid, c in cs.items() if iid in its and c.due <= at)
    if due:
        return {**its[due[0][1]], "mode": "review"}
    new = [it for iid, it in its.items() if iid not in cs]
    if focus_topics:
        focus = [it for it in new if it["topic"] in focus_topics]
        new = focus or new
    if not new:
        return None
    m = mastery(at)
    new.sort(key=lambda it: (m.get(it["topic"], 0.0), it["id"]))
    return {**new[0], "mode": "learn"}


def record(item_id: str, met: list[str], seconds: int | None = None, at: datetime | None = None, graded_by: str = "self") -> dict[str, Any]:
    at = at or now()
    its = items()
    if item_id not in its:
        raise KeyError(f"unknown item {item_id} (see `careerbrew drill list`)")
    item = its[item_id]
    rubric = rubrics().get(item["rubric"])
    if rubric is None:
        raise KeyError(f"rubric {item['rubric']} missing")
    ids = [c["id"] for c in rubric["criteria"]]
    bad = set(met) - set(ids)
    if bad:
        raise ValueError(f"unknown criteria {sorted(bad)}; this rubric has {ids}")
    score = round(len(set(met)) / len(ids), 3)
    rating = score_to_rating(score)
    cs = cards()
    card = cs.get(item_id) or Card()
    card, _log = Scheduler().review_card(card, rating, review_datetime=at)
    cs[item_id] = card
    _save_cards(cs)
    row = {
        "item": item_id,
        "topic": item["topic"],
        "round": item["round"],
        "met": sorted(set(met)),
        "missed": sorted(set(ids) - set(met)),
        "score": score,
        "rating": RATING_NAMES[rating],
        "seconds": seconds,
        "graded_by": graded_by,
        "at": at.isoformat(timespec="seconds"),
        "due": card.due.isoformat(timespec="seconds"),
        "origin": item.get("origin", "base"),
    }
    store.append_jsonl(_attempts_path(), [row])
    store.append_jsonl(paths.sub("tutor", "readiness.jsonl"), [{"at": row["at"], "readiness": readiness(None, at)}])
    events.emit("tutor.attempt", status=RATING_NAMES[rating], score=score)
    return row


def mastery(at: datetime | None = None) -> dict[str, float]:
    at = at or now()
    its = items()
    cs = cards()
    last_score: dict[str, float] = {}
    for a in attempts():
        last_score[a["item"]] = a["score"]
    sched = Scheduler()
    per_topic: dict[str, list[float]] = {}
    for iid, it in its.items():
        if iid in cs and iid in last_score:
            val = sched.get_card_retrievability(cs[iid], current_datetime=at) * last_score[iid]
        else:
            val = 0.0
        per_topic.setdefault(it["topic"], []).append(val)
    return {t: round(sum(v) / len(v), 3) for t, v in per_topic.items()}


def readiness(topics: list[str] | None, at: datetime | None = None) -> float:
    m = mastery(at)
    keys = [t for t in (topics or list(m)) if t in m]
    return round(sum(m[t] for t in keys) / len(keys), 3) if keys else 0.0


def feedback(row: dict[str, Any]) -> list[str]:
    item = items()[row["item"]]
    crit = {c["id"]: c["text"] for c in rubrics()[item["rubric"]]["criteria"]}
    lines = [f"Score {row['score']:.0%} -> {row['rating']}; next review {row['due'][:10]}."]
    for cid in row["missed"]:
        lines.append(f"Practice: {crit[cid]}")
    return lines
