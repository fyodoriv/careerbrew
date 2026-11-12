"""The public study deck: interview cards with full explanations, committed in `knowledge/cards/`.

Anyone can study it; nothing in it is personal. It grows from the owner's private
cards through a loop the `card-explainer` skill runs:

1. `explain_queue()` lists private cards no public card covers yet, public cards
   written with an older prompt (`prompts/card-explainer.md` changed), public cards
   that fail `lint`, and public cards learners keep failing. Behavioural answers
   (the owner's stories) never enter the queue, only the questions.
2. An agent writes cards with the prompt and saves them as YAML.
3. `publish()` lints each card, checks it for personal data (the owner's leak
   terms, name, and employer), and merges it into `knowledge/cards/<topic>.yaml`.
   A skipped question (personal) is remembered privately so it is not offered again.

A public card covers private cards through `match`: hashes of their questions,
never the text. The owner's matched cards show the public explanation.
"""

from __future__ import annotations

import os
import re
from functools import lru_cache
from pathlib import Path
from typing import Any

import yaml

from .. import paths, privacy, store

SECTIONS = ("## In plain words", "## How it works", "## Example", "## Common mistakes", "## In the interview")
PERSONAL_TOPICS = {"behavioural", "behavioral", "leadership", "tech lead", "business/product"}
MIN_WORDS, MAX_WORDS = 150, 700
LAPSES_TO_REWRITE = 3


class PublishError(ValueError):
    pass


REPO_CARDS = paths.REPO_ROOT / "knowledge" / "cards"


def cards_dir() -> Path:
    env = os.environ.get("CAREERBREW_PUBLIC_CARDS")  # tests point it at an empty folder
    return Path(env) if env else REPO_CARDS


def prompt_path() -> Path:
    return paths.REPO_ROOT / "prompts" / "card-explainer.md"


def prompt_version() -> str:
    return store.digest(prompt_path().read_text(encoding="utf-8"))[:8]


def normalize(question: str) -> str:
    return re.sub(r"[^a-z0-9]+", " ", question.casefold()).strip()


def qhash(question: str) -> str:
    return store.digest(normalize(question))[:12]


def _topic_file(topic: str) -> Path:
    return cards_dir() / f"{re.sub(r'[^a-z0-9]+', '-', topic.casefold()).strip('-') or 'general'}.yaml"


def load() -> list[dict[str, Any]]:
    files = tuple((str(f), f.stat().st_mtime_ns) for f in sorted(cards_dir().glob("*.yaml"))) if cards_dir().is_dir() else ()
    return [dict(c) for c in _load(files)]


@lru_cache(maxsize=2)
def _load(files: tuple[tuple[str, int], ...]) -> tuple[dict[str, Any], ...]:
    """Parsed once per set of file versions: Practice asks for explanations card by card."""
    out: list[dict[str, Any]] = []
    for f, _mtime in files:
        # the C parser when built: pure Python took 1.3-5.8 s for the 2 MB deck under load
        out += (yaml.load(Path(f).read_text(encoding="utf-8"), Loader=store._SAFE_LOADER) or {}).get("cards", [])  # noqa: S506 (a safe loader)
    return tuple(out)


def by_match() -> dict[str, dict[str, Any]]:
    """Private question hash -> the public card that explains it."""
    return {h: c for c in load() for h in c.get("match", [])}


def explanation_for(question: str) -> dict[str, Any] | None:
    h = qhash(question)
    mine = next((p for p in private_cards() if h in p.get("match", [])), None)
    if mine is not None:
        return mine
    c = by_match().get(h)
    if c is None:
        c = next((p for p in load() if qhash(p["question"]) == qhash(question)), None)
    return c


def lint(card: dict[str, Any]) -> list[str]:
    """Machine-checkable parts of the bar in the prompt. An empty list means the card passes."""
    problems = []
    for k in ("id", "question", "answer", "explanation", "topic"):
        if not str(card.get(k) or "").strip():
            problems.append(f"missing {k}")
    if problems:
        return problems
    q, a, e = str(card["question"]), str(card["answer"]), str(card["explanation"])
    if len(q) > 220:
        problems.append("question longer than 220 characters")
    if not 20 <= len(a) <= 600:
        problems.append("answer must be 20-600 characters")
    words = len(e.split())
    if not MIN_WORDS <= words <= MAX_WORDS:
        problems.append(f"explanation has {words} words (bar: {MIN_WORDS}-{MAX_WORDS})")
    positions = [e.find(s) for s in SECTIONS]
    if any(p < 0 for p in positions):
        problems.append("explanation lacks a section: " + ", ".join(s[3:] for s, p in zip(SECTIONS, positions, strict=True) if p < 0))
    elif positions != sorted(positions):
        problems.append("explanation sections are out of order")
    if e.count("```") % 2:
        problems.append("unclosed code fence")
    # http:// is fine for local hosts in code and for XML namespace names (identifiers that must be spelled exactly).
    if re.search(r"(?<!xmlns=\")(?<!xmlns:\w\w\w=\")(?<!xmlns:\w\w=\")(?<!xmlns:\w=\")http://(?!(localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\]|[\w.-]*example\.(com|org|net))\b)", e):
        problems.append("plain http link (use https; http is fine only for localhost or example.com in code)")
    return problems


def _personal_terms() -> list[str]:
    """The owner's leak terms, name, and employer: none may appear in a public card."""
    terms = list(privacy.sensitive_terms())
    prof = store.read_yaml(paths.data_root() / "profile.yaml", {}) or {}
    for k in ("name", "employer"):
        v = str(prof.get(k) or "").strip()
        if len(v) >= 3 and v not in ("Your Name", "Your Employer"):
            terms.append(v)
    return terms


def personal_hits(card: dict[str, Any]) -> list[str]:
    """The same strict match as the repo leak scan (inside words too), so a card that publishes never fails the gate."""
    text = " ".join(str(card.get(k) or "") for k in ("question", "answer", "explanation", "tags"))
    return privacy.find_leaks(text, _personal_terms())


def _private_path() -> Path:
    """Explanations for personal cards: same bar, never public, encrypted at rest with the rest of the data root."""
    return paths.sub("tutor", "private-explanations.yaml")


def private_cards() -> list[dict[str, Any]]:
    return (store.read_yaml(_private_path(), {}) or {}).get("cards", [])


def _flags_path() -> Path:
    return paths.sub("tutor", "explanation-flags.json")


def flags() -> dict[str, dict[str, str]]:
    return store.read_json(_flags_path(), {}) or {}


def flag(card_id: str, note: str) -> dict[str, str]:
    """A reader reports a problem with an explanation; the next explain-queue sends the card back for a rewrite."""
    from ..calendar import now_utc

    known = {c["id"] for c in load()} | {c["id"] for c in private_cards()}
    if card_id not in known:
        raise KeyError(f"unknown explanation {card_id}")
    note = " ".join(str(note).split())[:500] or "unclear or wrong"
    entry = {"note": note, "at": now_utc().isoformat(timespec="seconds")}
    store.write_json(_flags_path(), {**flags(), card_id: entry})
    return entry


def _skipped_path() -> Path:
    return paths.sub("tutor", "public-skipped.json")


def skipped() -> dict[str, str]:
    return store.read_json(_skipped_path(), {}) or {}


def _lapses() -> dict[str, int]:
    from . import cards

    out: dict[str, int] = {}
    for r in cards.reviews():
        if r.get("rating") == "again":
            out[r["card"]] = out.get(r["card"], 0) + 1
    return out


def explain_queue(limit: int | None = None) -> list[dict[str, Any]]:
    """What the next generation round should write or rewrite, most useful first."""
    from . import cards

    public = load()
    covered = by_match() | {qhash(c["question"]): c for c in public}
    skip = skipped()
    version = prompt_version()
    lapses = _lapses()
    items: list[dict[str, Any]] = []
    seen: set[str] = set()  # two private cards with the same question share one public card
    private_done = {h for p in private_cards() for h in p.get("match", [])}
    for c in cards.all_cards().values():
        if c.get("deck") == "public":
            continue
        h = qhash(c["question"])
        if h in seen:
            continue
        seen.add(h)
        if h in skip:
            if h not in private_done:
                items.append({"source_id": h, "question": c["question"], "topic": c.get("topic", "general"), "hint": "", "reason": "personal: explain privately", "private": True})
            continue
        pub = covered.get(h)
        if pub is None:
            hint = "" if str(c.get("topic", "")).casefold() in PERSONAL_TOPICS else c.get("answer", "")
            items.append({"source_id": h, "question": c["question"], "topic": c.get("topic", "general"), "hint": hint[:1500], "reason": "new"})
        elif lapses.get(c["id"], 0) >= LAPSES_TO_REWRITE:
            items.append({"source_id": h, "question": pub["question"], "topic": pub["topic"], "hint": "", "reason": "learners keep failing it: make it clearer", "rewrite": pub["id"]})
    flagged = flags()
    for p in [*public, *private_cards()]:
        if p["id"] in flagged and not any(i.get("rewrite") == p["id"] for i in items):
            private = p["id"].startswith("me-")
            items.append(
                {
                    "source_id": (p.get("match") or [qhash(p["question"])])[0],
                    "question": p["question"],
                    "topic": p["topic"],
                    "hint": "",
                    "reason": f"a reader reported: {flagged[p['id']]['note']}",
                    "rewrite": p["id"],
                    **({"private": True} if private else {}),
                }
            )
    for p in public:
        why = "" if p.get("prompt") == version else "written with an older prompt"
        why = why or ("; ".join(lint(p)) and "fails lint: " + "; ".join(lint(p)))
        if why and not any(i.get("rewrite") == p["id"] for i in items):
            items.append({"source_id": (p.get("match") or [qhash(p["question"])])[0], "question": p["question"], "topic": p["topic"], "hint": "", "reason": why, "rewrite": p["id"]})
    order = {"new": 2}  # reader reports and rewrites first, then new cards
    items.sort(key=lambda i: (order.get(i["reason"], 0), i["topic"], i["question"]))
    return items[:limit] if limit else items


def publish(rows: list[dict[str, Any]], dry_run: bool = False) -> dict[str, Any]:
    """Lint, privacy-check, and merge generated cards. Raises before writing anything if one card fails."""
    version = prompt_version()
    public = {c["id"]: c for c in load()}
    by_q = {qhash(c["question"]): c["id"] for c in public.values()}
    skip = skipped()
    new: dict[str, dict[str, Any]] = {}
    mine = {c["id"]: c for c in private_cards()}
    new_private: dict[str, dict[str, Any]] = {}
    problems: list[str] = []
    n_skip = 0
    for n, r in enumerate(rows, 1):
        src = str(r.get("source_id") or "")
        if r.get("skip"):
            if src:
                skip[src] = str(r.get("reason") or "personal")
                n_skip += 1
            continue
        cid = str(r.get("rewrite") or by_q.get(qhash(str(r.get("question") or ""))) or f"kb-{qhash(str(r.get('question') or ''))}")
        card = {
            "id": cid,
            "question": str(r.get("question") or "").strip(),
            "answer": str(r.get("answer") or "").strip(),
            "topic": str(r.get("topic") or "general").casefold(),
            "tags": [str(t).casefold() for t in (r.get("tags") or [])][:5],
            "explanation": str(r.get("explanation") or "").strip() + "\n",
            "match": sorted({*public.get(cid, {}).get("match", []), *new.get(cid, {}).get("match", []), *([src] if src else [])}),
            "prompt": version,
        }
        if r.get("private"):
            card["id"] = str(r.get("rewrite") or f"me-{src or qhash(card['question'])}")
            card["match"] = [src] if src else []
            bad = lint(card)
            if bad:
                problems.append(f"card {n}: {'; '.join(bad)}")
            else:
                new_private[card["id"]] = card
            continue
        bad = lint(card)
        hits = personal_hits(card)
        if hits:
            bad.append(f"personal data ({', '.join(hits)})")
        if bad:
            problems.append(f"card {n}: {'; '.join(bad)}")
            continue
        new[cid] = card
    if problems:
        raise PublishError(f"{len(problems)} card(s) not published; fix them and publish again:\n" + "\n".join(problems))
    merged = public | new
    if dry_run:
        return {"published": len(new), "skipped": n_skip, "total": len(merged), "private": len(new_private)}
    done = {*new, *new_private}
    if done & set(flags()):
        store.write_json(_flags_path(), {k: v for k, v in flags().items() if k not in done})
    if new_private:
        store.write_yaml(_private_path(), {"cards": sorted((mine | new_private).values(), key=lambda c: c["question"].casefold())})
    files: dict[Path, list[dict[str, Any]]] = {}
    for c in merged.values():
        files.setdefault(_topic_file(c["topic"]), []).append(c)
    cards_dir().mkdir(parents=True, exist_ok=True)
    for f in cards_dir().glob("*.yaml"):
        if f not in files:
            f.unlink()
    for f, cs in files.items():
        body = yaml.safe_dump({"cards": sorted(cs, key=lambda c: c["question"].casefold())}, sort_keys=False, allow_unicode=True, width=100)
        f.write_text("# Public study cards. Generated by the card-explainer skill; edit through `careerbrew cards publish`.\n" + body, encoding="utf-8")
    store.write_json(_skipped_path(), skip)
    return {"published": len(new), "skipped": n_skip, "total": len(merged), "private": len(new_private)}


def check() -> list[str]:
    """Gate: every committed card passes lint and ids are unique."""
    problems, seen = [], set()
    for c in load():
        if c.get("id") in seen:
            problems.append(f"{c.get('id')}: duplicate id")
        seen.add(c.get("id"))
        problems += [f"{c.get('id')}: {p}" for p in lint(c)]
    return problems


def _page(c: dict[str, Any], section: str) -> dict[str, Any]:
    md = f"**Short answer.** {c['answer']}\n\n{c['explanation']}"
    return {"id": c["id"], "title": c["question"], "section": section, "markdown": md}


def library_entries() -> list[dict[str, Any]]:
    """One Library page per card: public cards grouped by topic, the owner's private explanations apart."""
    pages = [_page(c, f"knowledge:{c['topic']}") for c in sorted(load(), key=lambda c: (c["topic"], c["question"].casefold()))]
    return pages + [_page(c, "my-explanations") for c in private_cards()]
