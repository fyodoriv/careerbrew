"""Promotion case: match evidence to a ladder, write the doc, rank the gaps.

coverage(competency) = min(1, matched evidence / expected count at the target
level), where evidence older than the ladder's window counts half.
gap priority = weight x (1 - coverage) / effort: the most promotion value per
unit of effort comes first.

Keywords match whole words; `migrat*` matches any word that starts with
`migrat`. A competency that matches more than BROAD_SHARE of all evidence gets
a warning: its signals are probably too wide (for example a signal tag that
equals an auto-tag from `evidence.KIND_TAGS`).
"""

from __future__ import annotations

import json
import re
from datetime import datetime, timedelta
from pathlib import Path
from typing import Any

from .. import events, models, paths, plugins, privacy, store
from . import evidence

BROAD_SHARE = 0.3


class LadderError(ValueError):
    pass


def validate_ladder(data: Any) -> dict[str, Any]:
    if not isinstance(data, dict) or not data.get("id") or not data.get("levels") or not data.get("competencies"):
        raise LadderError("a ladder needs id, levels, and competencies")
    for c in data["competencies"]:
        for key in ("id", "name", "weight", "signals", "expect"):
            if key not in c:
                raise LadderError(f"competency {c.get('id', '?')} lacks `{key}`")
        missing = [lvl for lvl in data["levels"] if lvl not in c["expect"]]
        if missing:
            raise LadderError(f"competency {c['id']} has no expectation for levels {missing}")
        for key in ("descriptions", "examples"):
            if key in c and not isinstance(c[key], dict):
                raise LadderError(f"competency {c['id']}: `{key}` must map a level to its text")
    return data


def ladders() -> dict[str, dict[str, Any]]:
    out: dict[str, dict[str, Any]] = {}
    found = list(plugins.resources("ladders"))
    user_dir = paths.data_root() / "ladders"
    if user_dir.is_dir():
        found.extend(("user", p) for p in sorted(user_dir.glob("*.yaml")))
    for origin, path in found:
        data = validate_ladder(store.read_yaml(path, {}))
        out[data["id"]] = {**data, "origin": origin}
    return out


def import_ladder(src: Path) -> str:
    data = validate_ladder(store.read_yaml(src, {}))
    store.write_yaml(paths.sub("ladders", f"{data['id']}.yaml"), data)
    return data["id"]


def _matches(item: dict[str, Any], comp: dict[str, Any]) -> bool:
    sig = comp["signals"]
    if f"dim:{comp.get('id')}" in item.get("tags", []):  # a PM line graded against this dimension (issue #216)
        return True
    if set(item.get("tags", [])) & set(sig.get("tags", [])):
        return True
    title = item.get("title", "").lower()
    return any(re.search(_keyword_pattern(k), title) for k in sig.get("keywords", []))


def _keyword_pattern(keyword: str) -> str:
    k = keyword.lower()
    if k.endswith("*"):
        return rf"(?<!\w){re.escape(k[:-1])}"
    return rf"(?<!\w){re.escape(k)}(?!\w)"


def assess(ladder_id: str, level: str, at: datetime | None = None) -> dict[str, Any]:
    lads = ladders()
    if ladder_id not in lads:
        raise LadderError(f"unknown ladder {ladder_id}; have {sorted(lads)}")
    lad = lads[ladder_id]
    if level not in lad["levels"]:
        raise LadderError(f"level {level} not in {lad['levels']}")
    from ..calendar import now as clock_now  # local, as in evidence.note

    at = at or clock_now()  # the pinned clock, as the Dashboard uses (US-75)
    window = timedelta(days=30 * int(lad.get("window_months", 12)))
    items = evidence.load()
    rows = []
    warnings = []
    goal_words = _long_term_words()
    for comp in lad["competencies"]:
        matched = [it for it in items if _matches(it, comp)]
        credit = 0.0
        for it in matched:
            try:
                when = datetime.fromisoformat(it["at"].replace("Z", "+00:00"))
            except (KeyError, ValueError):
                when = at
            credit += 1.0 if at - when <= window else 0.5
        need = float(comp["expect"][level])
        coverage = 1.0 if need == 0 else min(1.0, credit / need)
        effort = float(comp.get("effort", 2))
        if items and len(matched) / len(items) > BROAD_SHARE:
            warnings.append(f"competency {comp['id']} matches {len(matched)} of {len(items)} evidence items (over {BROAD_SHARE:.0%}); narrow its signals")
        rows.append(
            {
                "id": comp["id"],
                "name": comp["name"],
                "weight": comp["weight"],
                "need": need,
                "credit": round(credit, 1),
                "coverage": round(coverage, 3),
                "priority": round(comp["weight"] * (1 - coverage) / effort * (GOAL_BOOST if _named_by_goal(comp, goal_words) else 1), 3),
                "goal": _named_by_goal(comp, goal_words),
                "how_to_close": comp.get("how_to_close", ""),
                "description": (comp.get("descriptions") or {}).get(level, ""),
                "examples": (comp.get("examples") or {}).get(level) or {},
                "evidence": sorted(matched, key=lambda it: it.get("at", ""), reverse=True)[:8],
            }
        )
    total_w = sum(r["weight"] for r in rows) or 1
    overall = round(sum(r["weight"] * r["coverage"] for r in rows) / total_w, 3)
    return {"ladder": ladder_id, "ladder_origin": lad["origin"], "level": level, "overall": overall, "competencies": rows, "warnings": warnings, "at": at.isoformat(timespec="seconds")}


GOAL_BOOST = 1.5  # a competency the owner's long-term goal names: its gap counts this much more (issue #185)
_WORD = re.compile(r"[a-z0-9]{4,}")


def _long_term_words() -> set[str]:
    from .. import goals

    return {w for g in goals.load()["long_term"] for w in _WORD.findall(str(g.get("text", "")).lower())}


GENERIC = {"technical", "code", "work", "with", "skills", "quality", "impact", "team", "teams", "ownership"}  # in many names


def _named_by_goal(comp: dict[str, Any], words: set[str]) -> bool:
    """A long-term goal names the competency: a word of its id or name (not a generic one) shares its first five
    letters with a goal word, so "mentor" finds "mentoring" and "lead" finds "leadership"."""
    own = set(_WORD.findall(f"{comp['id']} {comp['name']}".lower().replace("-", " "))) - GENERIC
    stems = {w[:5] for w in own}
    return any(w[:5] in stems or any(s.startswith(w) for s in own) for w in words - GENERIC)


def gaps(assessment: dict[str, Any]) -> list[dict[str, Any]]:
    return sorted((r for r in assessment["competencies"] if r["coverage"] < 1.0), key=lambda r: (-r["priority"], r["id"]))


def write_doc(assessment: dict[str, Any]) -> Path:
    rows = assessment["competencies"]
    strongest = sorted(rows, key=lambda r: -r["coverage"])[:3]

    def fallback() -> str:
        names = ", ".join(r["name"].lower() for r in strongest)
        return f"Case for {assessment['level']}: overall ladder coverage {assessment['overall']:.0%}. Strongest areas: {names}."

    prompt = "Write a 3-sentence promotion summary from this JSON. Use only facts in it.\n" + json.dumps(
        {"level": assessment["level"], "overall": assessment["overall"], "strongest": [{"name": r["name"], "coverage": r["coverage"], "examples": [e["title"] for e in r["evidence"][:3]]} for r in strongest]}
    )
    summary, route = models.complete(prompt, privacy.BUSINESS, fallback)
    lines = [
        f"# Promotion case: {assessment['level']}",
        "",
        f"_Ladder `{assessment['ladder']}` ({assessment['ladder_origin']}) · generated {assessment['at']} · summary by {route}_",
        "",
        "## Summary",
        "",
        summary.strip(),
        "",
        "## Evidence by competency",
    ]
    for r in rows:
        lines += ["", f"### {r['name']} — {r['coverage']:.0%} of the bar ({r['credit']}/{r['need']:g})", ""]
        if r.get("description"):
            lines += [f"**Bar at {assessment['level']}:** {r['description']}", ""]
        for key, label in (("looks_like", "Looks like"), ("doesnt_look_like", "Does not look like")):
            shown = r.get("examples", {}).get(key) or []
            if shown:
                lines += [f"**{label}:**", *[f"- {x}" for x in ([shown] if isinstance(shown, str) else shown)], ""]
        if r["evidence"]:
            for e in r["evidence"]:
                link = f" ({e['url']})" if e.get("url") else ""
                impact = f" — {e['impact']}" if e.get("impact") else ""
                lines.append(f"- {e.get('at', '')[:10]} {e['title']}{impact}{link}")
        else:
            lines.append("- No evidence yet.")
    lines += ["", "## What is missing (highest priority first)", ""]
    for i, g in enumerate(gaps(assessment), 1):
        lines.append(f"{i}. **{g['name']}** — {g['coverage']:.0%} covered, priority {g['priority']}. {g['how_to_close']}")
    if not gaps(assessment):
        lines.append("Nothing: every competency meets the bar. Ask your manager to start the promotion process.")
    out = paths.sub("out", f"promo-{assessment['ladder']}-{assessment['level']}.md")
    out.write_text("\n".join(lines) + "\n", encoding="utf-8")
    store.append_jsonl(paths.sub("metrics.jsonl"), [{"metric": "promo_coverage", "value": assessment["overall"], "at": assessment["at"], "ladder": assessment["ladder"], "level": assessment["level"]}])
    events.emit("work.promo", status="ok", coverage=assessment["overall"])
    return out
