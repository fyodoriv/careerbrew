"""Flashcards: question and answer cards reviewed with FSRS.

How a review works, and why (sources in docs/practice.md):
- Recall before reveal: the question shows first; you answer from memory, then
  see the answer and grade yourself (retrieval practice).
- FSRS schedules the next review from your grade: again, hard, good, or easy
  (spaced repetition).
- New cards are interleaved by topic, and due reviews come first.
- At most `cards_new_per_day` new cards a day (settings.yaml, default 20), so
  the review load stays steady.
- A card you forget LEECH_LAPSES times after you had learned it (an "again"
  on a card in review, as Anki counts lapses) is a leech: it is suspended until
  you rewrite or unsuspend it. After an unsuspend, the next warning comes after
  half as many further lapses.
- "Today" is your local calendar day (the TZ of this machine).

Storage (all under the data root):
- tutor/decks/<deck>.yaml   cards, one file per deck (`mine`, `notion`, ...);
- tutor/card-state.json     FSRS state per card id;
- tutor/card-reviews.jsonl  one row per review;
- tutor/card-meta.json      suspended (leech) card ids.
State lives in these files only; nothing is kept in memory between calls.

A learn project keeps the same four files under its own folder
(`learn/<project>/srs/`): `deck_root(folder)` points every call inside it there.
The override is a call-scoped ContextVar (`_root`), set and reset by that context
manager only; a deck root never shows the public interview deck.
"""

from __future__ import annotations

import re
from collections.abc import Iterator
from contextlib import contextmanager
from contextvars import ContextVar
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from fsrs import Card, Rating, Scheduler, State

from .. import calendar, events, paths, store

RATINGS = {"again": Rating.Again, "hard": Rating.Hard, "good": Rating.Good, "easy": Rating.Easy}
LEECH_LAPSES = 8
DEFAULT_NEW_PER_DAY = 20
# Call-scoped: set only by `deck_root`, reset when its block ends; None means the interview deck (tutor/).
_root: ContextVar[Path | None] = ContextVar("cards_root", default=None)


@contextmanager
def deck_root(folder: Path) -> Iterator[None]:
    """Run card calls against the cards under `folder` (a learn project's srs/), not tutor/."""
    token = _root.set(folder)
    try:
        yield
    finally:
        _root.reset(token)


def _sub(*parts: str) -> Path:
    root = _root.get()
    if root is None:
        return paths.sub("tutor", *parts)
    path = root.joinpath(*parts)
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def now() -> datetime:
    return calendar.now_utc()


def _decks_dir() -> Path:
    return _sub("decks")


def _state_path() -> Path:
    return _sub("card-state.json")


def _reviews_path() -> Path:
    return _sub("card-reviews.jsonl")


def _meta_path() -> Path:
    return _sub("card-meta.json")


def _day(value: datetime | str):
    """The local calendar day of a time (reviews are stored in UTC)."""
    dt = datetime.fromisoformat(value) if isinstance(value, str) else value
    return (dt if dt.tzinfo else dt.replace(tzinfo=UTC)).astimezone().date()


def _slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.casefold()).strip("-") or "card"


def all_cards() -> dict[str, dict[str, Any]]:
    """Every deck in the data root, plus the public deck (`knowledge/cards/`) minus cards the owner already has."""
    from . import public

    out: dict[str, dict[str, Any]] = {}
    folder = _decks_dir()
    for f in sorted(folder.glob("*.yaml")) if folder.is_dir() else []:
        for c in (store.read_yaml(f, {}) or {}).get("cards", []):
            out[str(c["id"])] = {**c, "deck": f.stem}
    if _root.get() is not None:
        return out
    mine = {public.qhash(c["question"]) for c in out.values()}
    for c in public.load():
        if mine.isdisjoint({*c.get("match", []), public.qhash(c["question"])}):
            out[c["id"]] = {"id": c["id"], "question": c["question"], "answer": c["answer"], "topic": c["topic"], "tags": c.get("tags", []), "deck": "public"}
    return out


def with_explanation(card: dict[str, Any]) -> dict[str, Any]:
    if _root.get() is not None:
        return card
    from . import public

    pub = public.explanation_for(card["question"])
    return {**card, "explanation": pub["explanation"] if pub else "", "explanation_id": pub["id"] if pub else ""}


def write_deck(name: str, deck: list[dict[str, Any]]) -> int:
    """Replace one deck's cards. Review history and FSRS state are kept by card id."""
    clean = []
    for c in deck:
        q, a = str(c.get("question", "")).strip(), str(c.get("answer", "")).strip()
        if not q or not a:
            continue
        tags = [str(t) for t in c.get("tags", [])]
        topic = str(c.get("topic") or (tags[0] if tags else "general")).casefold()
        clean.append({"id": str(c["id"]), "question": q, "answer": a, "topic": topic, "tags": tags, **({"source": c["source"]} if c.get("source") else {})})
    store.write_yaml(_decks_dir() / f"{_slug(name)}.yaml", {"cards": clean})
    return len(clean)


def add(question: str, answer: str, tags: list[str], deck: str = "mine") -> dict[str, Any]:
    if not question.strip():
        raise ValueError("a card needs a question")
    if not answer.strip():
        raise ValueError("a card needs an answer")
    path = _decks_dir() / f"{_slug(deck)}.yaml"
    data = store.read_yaml(path, {"cards": []}) or {"cards": []}
    cid = f"{_slug(deck)}-{store.digest(question)[:10]}"
    card = {"id": cid, "question": question.strip(), "answer": answer.strip(), "topic": (tags[0] if tags else "general").casefold(), "tags": tags}
    data["cards"] = [c for c in data.get("cards", []) if c["id"] != cid] + [card]
    store.write_yaml(path, data)
    events.emit("cards.add", status="ok")
    return {**card, "deck": _slug(deck)}


def _states() -> dict[str, Card]:
    return {k: Card.from_dict(v) for k, v in (store.read_json(_state_path(), {}) or {}).items()}


def scheduled() -> set[str]:
    """Ids of cards that have been reviewed at least once, so FSRS has a next review for them."""
    return set(_states())


def _suspended() -> set[str]:
    return set((store.read_json(_meta_path(), {}) or {}).get("suspended", []))


def reviews() -> list[dict[str, Any]]:
    return store.read_jsonl(_reviews_path())


def _new_per_day() -> int:
    s = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    try:
        return max(0, int(s.get("cards_new_per_day", DEFAULT_NEW_PER_DAY)))
    except (TypeError, ValueError):
        return DEFAULT_NEW_PER_DAY


def _interleave(rows: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """Round-robin across topics so neighbours differ (interleaved practice)."""
    by_topic: dict[str, list[dict[str, Any]]] = {}
    for r in rows:
        by_topic.setdefault(r["topic"], []).append(r)
    out: list[dict[str, Any]] = []
    while any(by_topic.values()):
        for t in sorted(by_topic):
            if by_topic[t]:
                out.append(by_topic[t].pop(0))
    return out


def queue(at: datetime | None = None, new_per_day: int | None = None, limit: int = 50, topics: list[str] | None = None) -> list[dict[str, Any]]:
    """Due reviews first (oldest due first), then today's share of new cards, interleaved by topic. New cards on the
    goal's `topics` come first (issue #185: the owner's goal steers practice); due reviews keep their order."""
    at = at or now()
    cap = _new_per_day() if new_per_day is None else new_per_day
    cs, st, sus = all_cards(), _states(), _suspended()
    due = sorted((st[i].due, i) for i in cs if i in st and i not in sus and st[i].due <= at)
    due_rows = _interleave([{**cs[i], "mode": "review"} for _, i in due])
    today = _day(at)
    introduced = {r["card"] for r in reviews() if r.get("first") and _day(r["at"]) == today}
    slots = max(0, cap - len(introduced))
    fresh = [{**cs[i], "mode": "new"} for i in sorted(cs) if i not in st and i not in sus]
    wanted = {t.casefold() for t in topics or []}
    on_goal = [c for c in fresh if str(c["topic"]).casefold() in wanted or wanted & {str(t).casefold() for t in c.get("tags") or []}]
    goal_ids = {c["id"] for c in on_goal}
    new_rows = (_interleave(on_goal) + _interleave([c for c in fresh if c["id"] not in goal_ids]))[:slots]
    return [with_explanation(c) for c in (due_rows + new_rows)[:limit]]


def review(card_id: str, rating: str, at: datetime | None = None, seconds: int | None = None) -> dict[str, Any]:
    at = at or now()
    if rating not in RATINGS:
        raise ValueError(f"rating must be one of {list(RATINGS)}, not {rating!r}")
    cs = all_cards()
    if card_id not in cs:
        raise KeyError(f"unknown card {card_id}")
    if card_id in _suspended():
        raise ValueError(f"card {card_id} is suspended as a leech; rewrite it, then `careerbrew cards unsuspend {card_id}`")
    st = _states()
    first = card_id not in st
    lapse = rating == "again" and not first and st[card_id].state == State.Review
    card, _log = Scheduler().review_card(st.get(card_id) or Card(), RATINGS[rating], review_datetime=at)
    st[card_id] = card
    store.write_json(_state_path(), {k: v.to_dict() for k, v in st.items()})
    lapses = sum(1 for r in reviews() if r["card"] == card_id and r.get("lapse")) + lapse
    meta = store.read_json(_meta_path(), {}) or {}
    base = (meta.get("lapse_base") or {}).get(card_id)
    threshold = LEECH_LAPSES if base is None else base + LEECH_LAPSES // 2
    leech = lapse and lapses >= threshold
    if leech:
        meta["suspended"] = sorted(set(meta.get("suspended", [])) | {card_id})
        store.write_json(_meta_path(), meta)
    row = {
        "card": card_id,
        "topic": cs[card_id]["topic"],
        "rating": rating,
        "first": first,
        "at": at.isoformat(timespec="seconds"),
        "due": card.due.isoformat(timespec="seconds"),
        "seconds": seconds,
        "lapse": lapse,
        "lapses": lapses,
        "leech": leech,
    }
    store.append_jsonl(_reviews_path(), [row])
    events.emit("cards.review", status=rating)
    return row


def suspend(card_id: str) -> None:
    """Take a card out of review: the owner dropped it (issue #185). `careerbrew cards unsuspend <id>` brings it back."""
    if card_id not in all_cards():
        raise KeyError(f"unknown card {card_id}")
    meta = store.read_json(_meta_path(), {}) or {}
    meta["suspended"] = sorted(set(meta.get("suspended", [])) | {card_id})
    store.write_json(_meta_path(), meta)


def unsuspend(card_id: str) -> None:
    if card_id not in all_cards():
        raise KeyError(f"unknown card {card_id}")
    meta = store.read_json(_meta_path(), {}) or {}
    meta["suspended"] = sorted(set(meta.get("suspended", [])) - {card_id})
    meta.setdefault("lapse_base", {})[card_id] = sum(1 for r in reviews() if r["card"] == card_id and r.get("lapse"))
    store.write_json(_meta_path(), meta)


def stats(at: datetime | None = None) -> dict[str, Any]:
    at = at or now()
    cs, st, sus, rv = all_cards(), _states(), _suspended(), reviews()
    since = (at - timedelta(days=30)).isoformat()
    recent = [r for r in rv if r["at"] >= since and r["card"] in cs]
    week = [r for r in recent if r["at"] >= (at - timedelta(days=7)).isoformat() and not r.get("first")]
    today = _day(at)
    local = at.astimezone()
    end_of_day = datetime.combine(today, datetime.max.time(), tzinfo=local.tzinfo)

    def retention(rows: list[dict[str, Any]]) -> float | None:
        return round(sum(r["rating"] != "again" for r in rows) / len(rows), 3) if rows else None

    by_topic: dict[str, dict[str, Any]] = {}
    for i, c in cs.items():
        t = by_topic.setdefault(c["topic"], {"total": 0, "due": 0, "new": 0})
        t["total"] += 1
        t["new"] += i not in st
        t["due"] += i in st and i not in sus and st[i].due <= at
    for t, row in by_topic.items():
        row["retention"] = retention([r for r in recent if r["topic"] == t])
    days = {_day(r["at"]) for r in rv}
    streak, day = 0, today if today in days else today - timedelta(days=1)  # today still counts until it ends
    while day in days:
        streak, day = streak + 1, day - timedelta(days=1)
    return {
        "total": len(cs),
        "new": sum(i not in st for i in cs),
        "due_now": sum(1 for i in cs if i in st and i not in sus and st[i].due <= at),
        "due_today": sum(1 for i in cs if i in st and i not in sus and st[i].due <= end_of_day),
        "reviewed_today": sum(_day(r["at"]) == today for r in rv),
        "retention_30d": retention(recent),
        "retention_7d": retention(week),
        "suspended": sorted(sus & set(cs)),
        "streak_days": streak,
        "new_per_day": _new_per_day(),
        "by_topic": by_topic,
    }
