"""A paste-ready LinkedIn Experience block from your work results.

The block is PUBLIC text, so it gets the strictest scrub in careerbrew:
- picks results with impact first (notes with `impact`, then impact, design,
  leadership, and ownership evidence), one bullet each, best first;
- removes ticket keys (PROJ-123), URLs, conventional-commit prefixes, and
  repo names; replaces every sensitive term with a neutral phrase;
- keeps numbers ("cut p99 latency 40%") unless a plugin policy sets
  `public_metrics: false`, which turns them into qualitative words;
- fits LinkedIn's Experience description limit (2,000 characters);
- a final leak check refuses the output if anything sensitive survives.

careerbrew never posts. You paste the block into LinkedIn yourself.
"""

from __future__ import annotations

import re
from pathlib import Path
from typing import Any

from .. import calendar, events, models, paths, plugins, privacy, store
from . import evidence

LIMIT = 2000
TAG_RANK = {"impact": 5, "design": 4, "leadership": 4, "ownership": 3, "delivery": 2, "quality": 2, "collaboration": 1, "communication": 1, "tooling": 1}
VERBS = {"feat": "Shipped", "perf": "Improved", "fix": "Fixed", "refactor": "Reworked", "docs": "Documented", "test": "Hardened", "ci": "Automated"}
PAST = {
    "add": "Added",
    "build": "Built",
    "create": "Created",
    "cut": "Cut",
    "design": "Designed",
    "fix": "Fixed",
    "implement": "Implemented",
    "improve": "Improved",
    "launch": "Launched",
    "lead": "Led",
    "migrate": "Migrated",
    "move": "Moved",
    "reduce": "Reduced",
    "refactor": "Refactored",
    "remove": "Removed",
    "ship": "Shipped",
    "speed": "Sped",
    "support": "Supported",
    "write": "Wrote",
    "automate": "Automated",
    "replace": "Replaced",
}
_TICKET = re.compile(r"\b[A-Z][A-Z0-9]{1,9}-\d+\b:?\s*")
_URL = re.compile(r"https?://\S+")
_PREFIX = re.compile(r"^(feat|fix|perf|refactor|docs|test|ci|chore|style)(\([^)]*\))?!?:\s*", re.IGNORECASE)
_NUM = re.compile(r"(\$\s?\d[\d,.]*\s?[kmb]?(?!\w)|\d[\d,.]*\s?(%|x|ms|s|hours?|minutes?|days?|weeks?|k|m)(?!\w))", re.IGNORECASE)


def _clean(title: str, repos: set[str], terms: list[str], public_metrics: bool) -> str:
    text = _TICKET.sub("", title)
    head = _PREFIX.match(text)
    verb = VERBS.get(head.group(1).lower(), "") if head else ""
    text = _PREFIX.sub("", text)
    for repo in sorted(repos, key=len, reverse=True):
        if len(repo) >= 3:
            text = re.sub(rf"(?:\b(?:the|a|an)\s+)?\b{re.escape(repo)}\b", "an internal service", text, flags=re.IGNORECASE)
    text = privacy.scrub_public(text, terms)
    if not public_metrics:
        text = _NUM.sub("significantly", text)
    text = re.sub(r"\s+", " ", text).strip(" .-:")
    if not text:
        return ""
    first, _, rest = text.partition(" ")
    low = first.lower()
    if low in PAST:
        text = f"{PAST[low]} {rest}".strip()
    elif verb and not low.endswith("ed") and low not in {p.lower() for p in PAST.values()}:
        text = f"{verb} {text[0].lower()}{text[1:]}"
    text = text[0].upper() + text[1:]
    return text + "."


def _rank(item: dict[str, Any]) -> tuple[int, int, str]:
    score = max((TAG_RANK.get(t, 0) for t in item.get("tags", [])), default=0)
    has_number = 1 if _NUM.search(item.get("impact", "") + " " + item.get("title", "")) else 0
    return (-(score + 3 * bool(item.get("impact")) + 2 * has_number), 0, item.get("at", ""))


def build(role: str = "", max_bullets: int = 6, since: str | None = None) -> dict[str, Any]:
    terms = privacy.sensitive_terms()
    public_metrics = plugins.effective_policy().get("public_metrics", True) is not False
    items = [e for e in evidence.load() if not since or (e.get("at") or "") >= since]
    repos = {e.get("repo", "").split("/")[-1] for e in items if e.get("repo")}
    picked: list[str] = []
    seen: set[str] = set()
    for it in sorted(items, key=_rank):
        if it.get("kind") in ("slack", "zoom"):
            continue  # signals, not results
        base = it.get("title", "")
        if it.get("impact"):
            base = f"{base}: {it['impact']}"
        line = _clean(base, repos, terms, public_metrics)
        key = line.lower()[:60]
        if len(line) < 12 or key in seen:
            continue
        # A bullet the scrub gutted (mostly links, hosts, ids) reads badly: skip it.
        if len(line) < 0.5 * len(_PREFIX.sub("", _TICKET.sub("", base))):
            continue
        seen.add(key)
        picked.append(line)
        if len(picked) >= max_bullets:
            break
    tags = sorted({t for e in items for t in e.get("tags", [])}, key=lambda t: -TAG_RANK.get(t, 0))
    focus = ", ".join(t for t in tags[:3]) or "delivery"
    intro = f"{role.strip() + '. ' if role.strip() else ''}Focused on {focus}."

    def fallback() -> str:
        return intro

    polished, route = models.complete(
        "Rewrite as one plain sentence for a LinkedIn Experience section. No names, no codenames, no numbers that are not given: " + intro,
        privacy.BUSINESS,
        fallback,
    )
    intro = _clean(polished, repos, terms, public_metrics) or intro
    text = intro + "\n\n" + "\n".join(f"• {b}" for b in picked)
    while len(text) > LIMIT and picked:
        picked.pop()
        text = intro + "\n\n" + "\n".join(f"• {b}" for b in picked)
    public_leaks = [p.pattern[:12] for p in privacy.PUBLIC_PATTERNS[:4] if p.search(text)]
    leaks = privacy.find_leaks(text, terms) + (["ticket-key"] if _TICKET.search(text) else []) + public_leaks
    if leaks:
        raise privacy.PrivacyError(f"LinkedIn block blocked: {len(leaks)} item(s) would be published ({', '.join(leaks)})")
    out = paths.sub("out", "linkedin.md")
    out.write_text(text + "\n", encoding="utf-8")
    events.emit("work.linkedin", status="ok", bullets=len(picked), route=route)
    store.append_jsonl(paths.sub("metrics.jsonl"), [{"metric": "linkedin_bullets", "value": len(picked), "at": calendar.now_utc().isoformat(timespec="seconds")}])
    return {"path": out, "text": text, "bullets": len(picked), "chars": len(text), "public_metrics": public_metrics, "suspects": privacy.public_suspects(text)}


def path() -> Path:
    return paths.sub("out", "linkedin.md")
