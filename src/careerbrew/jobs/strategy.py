"""Season strategy: learn → practice → target → dream.

The constraint: each company gives you one real interview loop per season
(typical cooldown 6–12 months). So the order of applications matters more
than their count. The model:

1. Tier the filtered postings by Want (the user's rating wins over the
   proposed score). One company sits in exactly one tier: its best posting.
   - dream: top 20% by Want (at least 1), or rated 5
   - target: next 40%
   - practice: the rest — real loops at companies you would accept but do
     not dream of. Prefer practice companies whose skills overlap the dream
     set, so practice transfers.
2. Skill gaps: skills that dream and target postings ask for, weighted by
   Want, that the profile rates below 3 (of 5) or lacks. They become tutor
   items (tutor.engine.add_gap_items).
3. Gates (pre-registered, checked by `careerbrew review`):
   - start practice applications when readiness >= 0.5 on the core topics;
   - move to target companies after >= 3 practice loops with a pass rate
     >= 0.5 and readiness >= 0.7;
   - apply to dream companies only when readiness >= 0.8 and the target
     pass rate >= 0.5. Below that, keep practicing: a dream loop spent
     early is gone for the season.
4. Expected value per company = P(offer) x Want, with P(offer) from the
   tier base rate scaled by readiness. This ranks applications inside a tier.
"""

from __future__ import annotations

from collections import Counter, defaultdict
from collections.abc import Sequence
from datetime import datetime, timedelta
from typing import Any

from .. import calendar, events, paths, store
from ..tutor import engine as tutor
from . import criteria, sources, want

CORE_TOPICS = ["data structures", "system design", "behavioral"]
GATES = {"practice": 0.5, "target": 0.7, "dream": 0.8, "pass_rate": 0.5, "min_practice_loops": 3}
BASE_P_OFFER = {"practice": 0.35, "target": 0.25, "dream": 0.15}
STAGES = ["applied", "screen", "onsite", "offer", "rejected", "withdrawn"]


def applications_path():
    return paths.sub("jobs", "applications.jsonl")


def log_application(company: str, tier: str, stage: str) -> None:
    if stage not in STAGES:
        raise ValueError(f"stage must be one of {STAGES}")
    store.append_jsonl(applications_path(), [{"company": company, "tier": tier, "stage": stage, "at": calendar.now_utc().isoformat(timespec="seconds"), "origin": "user"}])
    events.emit("jobs.application", tier=tier, status=stage)


COOLDOWN_DAYS = 365  # reapply cooldowns are 6-12 months (rule one-loop-per-season); use the long end


def used_companies(at: datetime | None = None) -> set[str]:
    """Companies (casefolded) whose loop is spent: any stage past `applied` within the cooldown."""
    at = at or calendar.now_utc()
    since = (at - timedelta(days=COOLDOWN_DAYS)).isoformat()
    out: set[str] = set()
    for row in store.read_jsonl(applications_path()):
        when = str(row.get("at") or "")
        if row["stage"] != "applied" and when and when >= since[: len(when)]:  # no date: never block forever
            out.add(row["company"].casefold())
    return out


def pass_rates() -> dict[str, dict[str, float]]:
    """Per tier: loops (companies past 'applied') and pass rate (reached onsite or offer)."""
    latest: dict[str, dict[str, Any]] = {}
    reached: dict[str, set[str]] = defaultdict(set)
    for row in store.read_jsonl(applications_path()):
        latest[row["company"]] = row
        reached[row["company"]].add(row["stage"])
    out: dict[str, dict[str, float]] = {}
    for tier in BASE_P_OFFER:
        comps = [c for c, r in latest.items() if r["tier"] == tier and reached[c] - {"applied"}]
        passed = [c for c in comps if reached[c] & {"onsite", "offer"}]
        out[tier] = {"loops": len(comps), "pass_rate": round(len(passed) / len(comps), 3) if comps else 0.0}
    return out


TIERS = ("dream", "target", "practice")


def _chosen_path():
    return paths.sub("jobs", "chosen.json")


def chosen() -> dict[str, str]:
    """Postings the owner chose, with the tier they chose. Only the owner fills this (CLI or web)."""
    return store.read_json(_chosen_path(), {}) or {}


def choose(posting_id: str, tier: str) -> dict[str, str]:
    if tier not in TIERS:
        raise ValueError(f"tier must be one of {', '.join(TIERS)}")
    if posting_id not in {p.id for p in sources.load()}:
        raise KeyError(f"unknown posting {posting_id} (see `careerbrew jobs list`)")
    data = {**chosen(), posting_id: tier}
    store.write_json(_chosen_path(), data)
    events.emit("jobs.choose", tier=tier, status="ok")
    return data


def unchoose(posting_id: str) -> dict[str, str]:
    data = {k: v for k, v in chosen().items() if k != posting_id}
    store.write_json(_chosen_path(), data)
    return data


def build() -> dict[str, Any]:
    prof = want.profile()
    crit = criteria.load()
    kept, rejected = criteria.apply(sources.load(), crit)
    w = want.weights()
    rated = want.ratings()
    used = used_companies()
    best: dict[str, tuple[float, sources.Posting, dict[str, float]]] = {}
    used_names: set[str] = set()
    for p in kept:
        key = p.company.casefold()
        if key in used:
            used_names.add(p.company)
            continue
        s, f = want.score(p, w, prof, crit)
        eff = rated[p.id] * 20.0 if p.id in rated else s
        if key not in best or eff > best[key][0]:
            best[key] = (eff, p, f)
    ranked = sorted(best.values(), key=lambda t: -t[0])
    n = len(ranked)
    n_dream = max(1, round(n * 0.2)) if n else 0
    n_target = round(n * 0.4)
    # careerbrew only suggests; a posting enters a tier when the owner chooses it.
    picks = chosen()
    suggested: dict[str, list[dict[str, Any]]] = {t: [] for t in TIERS}
    tiers: dict[str, list[dict[str, Any]]] = {t: [] for t in TIERS}
    for i, (eff, p, _f) in enumerate(ranked):
        if p.tier_hint in TIERS:
            tier = p.tier_hint
        elif i < n_dream:
            tier = "dream"
        elif i < n_dream + n_target:
            tier = "target"
        else:
            tier = "practice"
        row = {"company": p.company, "posting_id": p.id, "title": p.title, "want": round(eff, 1), "skills": p.skills, "url": p.url}
        suggested[tier].append(row)
    by_id = {p.id: (eff, p) for eff, p, _f in ranked} | {p.id: (want.score(p, w, prof, crit)[0], p) for p in kept if p.id in picks}
    for pid, tier in picks.items():
        if pid in by_id and by_id[pid][1].company.casefold() not in used:
            eff, p = by_id[pid]
            tiers[tier].append({"company": p.company, "posting_id": p.id, "title": p.title, "want": round(rated[p.id] * 20.0 if p.id in rated else eff, 1), "skills": p.skills, "url": p.url, "chosen": True})
    basis = tiers if any(tiers.values()) else suggested  # learn for the chosen jobs, or for the suggestions until the owner chooses

    for group in (tiers, suggested):
        dream_skills = Counter(s for row in group["dream"] for s in row["skills"])
        for row in group["practice"]:
            row["transfer"] = round(sum(dream_skills[s] for s in row["skills"]) / (sum(dream_skills.values()) or 1), 3)
        group["practice"].sort(key=lambda r: (-r["transfer"], -r["want"]))

    have = {k.lower(): int(v) for k, v in (prof.get("skills") or {}).items()}
    demand: Counter[str] = Counter()
    for tier in ("dream", "target"):
        for row in basis[tier]:
            for s in row["skills"]:
                demand[s] += row["want"]
    gaps = [(s, round(v, 1)) for s, v in demand.most_common() if have.get(s.lower(), 0) < 3][:8]
    gap_skills = [s for s, _ in gaps]
    tutor.add_gap_items(gap_skills)

    ready = tutor.readiness(CORE_TOPICS + gap_skills)
    rates = pass_rates()
    for group in (tiers, suggested):
        for tier, rows in group.items():
            p_base = BASE_P_OFFER[tier]
            for row in rows:
                row["p_offer"] = round(p_base * (0.5 + ready), 3)
                row["ev"] = round(row["p_offer"] * row["want"], 1)
    if rates["target"]["loops"] and rates["target"]["pass_rate"] >= GATES["pass_rate"] and ready >= GATES["dream"]:
        phase = "dream"
    elif rates["practice"]["loops"] >= GATES["min_practice_loops"] and rates["practice"]["pass_rate"] >= GATES["pass_rate"] and ready >= GATES["target"]:
        phase = "target"
    elif ready >= GATES["practice"]:
        phase = "practice"
    else:
        phase = "learn"
    plan = {
        "generated_at": calendar.now_utc().isoformat(timespec="seconds"),
        "phase": phase,
        "readiness": ready,
        "gates": GATES,
        "pass_rates": rates,
        "filtered_out": dict(rejected),
        "tiers": tiers,
        "suggested": suggested,
        "chosen": len(picks),
        "skill_gaps": gaps,
        "used_this_season": sorted(used_names),
        "steps": _steps(phase, ready, gaps, tiers),
    }
    store.write_json(paths.sub("jobs", "strategy.json"), plan)
    paths.sub("out", "strategy.md").write_text(render(plan), encoding="utf-8")
    events.emit("jobs.strategy", tier=phase, status="ok")
    return plan


def _steps(phase: str, ready: float, gaps: Sequence[tuple[str, float]], tiers: dict[str, list[dict[str, Any]]]) -> list[str]:
    gap_names = ", ".join(s for s, _ in gaps[:4]) or "none"
    steps = [
        f"Learn: close the top skill gaps ({gap_names}) with `careerbrew drill` daily until readiness >= {GATES['practice']:.0%} (now {ready:.0%}).",
        f"Practice: apply to {min(5, len(tiers['practice']))} practice companies, best skill transfer first; log each stage with `careerbrew jobs log`.",
        f"Target: after >= {GATES['min_practice_loops']} practice loops with pass rate >= {GATES['pass_rate']:.0%} and readiness >= {GATES['target']:.0%}, apply to target companies by expected value.",
        f"Dream: only when readiness >= {GATES['dream']:.0%} and target pass rate >= {GATES['pass_rate']:.0%}. One loop per company per season; do not spend it early.",
    ]
    order = ["learn", "practice", "target", "dream"]
    out = [("NOW → " if order[i] == phase else "") + s for i, s in enumerate(steps)]
    if not any(tiers.values()):
        out.insert(0, "First: choose the jobs you want. careerbrew only suggests: `careerbrew jobs choose <posting id> dream|target|practice` (or Job hunt in the web app).")
    return out


def render(plan: dict[str, Any]) -> str:
    lines = [
        "# Job-search strategy",
        "",
        f"Phase now: **{plan['phase']}** · readiness {plan['readiness']:.0%} · generated {plan['generated_at']}",
        "",
        "## Steps",
        *[f"{i + 1}. {s}" for i, s in enumerate(plan["steps"])],
        "",
        "## Skill gaps (weighted by Want)",
        *([f"- {s} ({v})" for s, v in plan["skill_gaps"]] or ["- none"]),
    ]
    for tier in ("dream", "target", "practice"):
        lines += ["", f"## {tier.title()} companies (your choice)", "", "| Company | Role | Want | P(offer) | EV |", "|---|---|---|---|---|"]
        for r in sorted(plan["tiers"][tier], key=lambda r: -r["ev"]):
            lines.append(f"| {r['company']} | {r['title']} | {r['want']} | {r['p_offer']:.0%} | {r['ev']} |")
        if not plan["tiers"][tier]:
            lines.append("| none chosen | | | | |")
    lines += ["", "## Suggestions (not chosen: choose with `careerbrew jobs choose <posting id> <tier>`)", "", "| Suggested tier | Company | Role | Posting | Want |", "|---|---|---|---|---|"]
    for tier in ("dream", "target", "practice"):
        for r in plan.get("suggested", {}).get(tier, []):
            lines.append(f"| {tier} | {r['company']} | {r['title']} | `{r['posting_id']}` | {r['want']} |")
    return "\n".join(lines) + "\n"
