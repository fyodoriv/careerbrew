"""Interview knowledge: shared step patterns, company loops, tips and traps, all sourced.

Data (public, committed, every fact cites knowledge/sources.yaml; checked by
scripts/check-sources.py):
- knowledge/interview/steps.yaml   shared steps (recruiter screen → offer);
- knowledge/companies/<id>.yaml    company loops built from those steps (your own batch: knowledge/companies-extra/);
- plugin `knowledge` files may add company files for a private employer.

For a company without a file, `loop_for` builds the default loop from the
shared patterns and the posting's level, and says so.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import paths, store

DEFAULT_LOOP = ["recruiter-screen", "technical-phone-screen", "coding-onsite", "system-design", "behavioral", "offer-negotiation"]
JUNIOR_LOOP = ["recruiter-screen", "technical-phone-screen", "coding-onsite", "behavioral", "offer-negotiation"]
STEP_TOPICS = {
    "technical-phone-screen": "data structures",
    "coding-onsite": "data structures",
    "online-assessment": "data structures",
    "pair-programming": "data structures",
    "system-design": "system design",
    "behavioral": "behavioral",
    "bar-raiser": "behavioral",
    "hiring-manager": "behavioral",
}


def sources() -> dict[str, dict[str, str]]:
    return {k: v for f in (("sources-extra.yaml", "sources.yaml") if paths.extra_knowledge() else ("sources.yaml",)) for k, v in ((store.read_yaml(paths.REPO_ROOT / "knowledge" / f, {}) or {}).get("sources") or {}).items()}


def steps() -> dict[str, dict[str, Any]]:
    return (store.read_yaml(paths.REPO_ROOT / "knowledge" / "interview" / "steps.yaml", {}) or {}).get("steps", {})


def _company_files() -> list[Path]:
    files = sorted(p for d in (("companies-extra", "companies") if paths.extra_knowledge() else ("companies",)) for p in (paths.REPO_ROOT / "knowledge" / d).glob("*.yaml"))
    from . import plugins

    files.extend(p for _o, p in plugins.resources("knowledge") if p.parent.name == "companies" or p.name.startswith("company-"))
    return files


def companies() -> dict[str, dict[str, Any]]:
    out = {}
    for f in _company_files():
        data = store.read_yaml(f, {}) or {}
        if data.get("loop"):
            out[str(data.get("id", f.stem)).casefold()] = data
    return out


def find_company(name: str) -> dict[str, Any] | None:
    key = name.casefold().strip()
    cs = companies()
    if key in cs:
        return cs[key]
    return next((c for c in cs.values() if c.get("name", "").casefold() == key), None)


def patterns(min_share: float = 0.4) -> dict[str, Any]:
    """What interview loops share across every sourced company: step frequency, typical order, transitions, AI policy."""
    cs = list(companies().values())
    n = len(cs) or 1
    freq: dict[str, int] = {}
    positions: dict[str, list[float]] = {}
    pairs: dict[tuple[str, str], int] = {}
    ai: dict[str, int] = {}
    for c in cs:
        loop = [s["step"] for s in c["loop"]]
        for i, st in enumerate(dict.fromkeys(loop)):
            freq[st] = freq.get(st, 0) + 1
            positions.setdefault(st, []).append(i / max(1, len(loop) - 1))
        for a, b in zip(loop, loop[1:], strict=False):
            pairs[(a, b)] = pairs.get((a, b), 0) + 1
        allowed = str((c.get("ai_policy") or {}).get("allowed", "unknown"))
        ai[allowed] = ai.get(allowed, 0) + 1
    steps_ranked = sorted(freq, key=lambda s: -freq[s])
    median = {s: sorted(v)[len(v) // 2] for s, v in positions.items()}
    common = [s for s in steps_ranked if freq[s] / n >= min_share]
    typical = sorted(common, key=lambda s: median[s])
    strategy = store.read_yaml(paths.REPO_ROOT / "knowledge" / "strategy.yaml", {}) or {}
    return {
        "companies": len(cs),
        "step_share": {s: round(freq[s] / n, 2) for s in steps_ranked},
        "typical_order": typical,
        "transitions": [(a, b, k) for (a, b), k in sorted(pairs.items(), key=lambda kv: -kv[1])[:8]],
        "ai_policy": ai,
        "ai_shared_patterns": strategy.get("ai_shared_patterns", []),
    }


def default_loop(seniority: int = 3) -> list[str]:
    """The shared default: steps most sourced companies use, in their typical order."""
    order = patterns()["typical_order"] or DEFAULT_LOOP
    loop = [s for s in order if s != "offer-negotiation" and (seniority >= 3 or s != "system-design")]
    if seniority >= 3 and "system-design" not in loop:
        loop.insert(max(0, len(loop) - 1), "system-design")
    return loop + ["offer-negotiation"]


def loop_for(company: str, seniority: int = 3) -> dict[str, Any]:
    company = company.strip()
    if not company:
        raise ValueError("name a company, e.g. careerbrew interview Stripe")
    c = find_company(company)
    all_steps = steps()
    if c:
        loop = [{**all_steps.get(s["step"], {}), "id": s["step"], "company_notes": s.get("notes", ""), "company_sources": s.get("sources", [])} for s in c["loop"]]
        return {
            "company": c.get("name", company),
            "known": True,
            "loop": loop,
            "company_tips": c.get("company_tips", []),
            "values": c.get("values_or_principles", []),
            "sources": c.get("sources", []),
            "remote_policy": c.get("remote_policy"),
            "local_presence": c.get("local_presence"),
            "ai_policy": c.get("ai_policy"),
            "checked": c.get("checked"),
        }
    ids = default_loop(seniority)
    return {
        "company": company,
        "known": False,
        "loop": [{**all_steps.get(i, {}), "id": i, "company_notes": "", "company_sources": []} for i in ids],
        "company_tips": [],
        "values": [],
        "sources": [],
        "ai_shared": patterns()["ai_shared_patterns"],
    }


def render(info: dict[str, Any], with_sources: bool = True) -> str:
    """Markdown guide; citations are numbered [n] with a Sources list at the end."""
    src = sources()
    order: list[str] = []

    def cite(ids: list[str]) -> str:
        if not with_sources:
            return ""
        nums = []
        for i in ids:
            if i in src:
                if i not in order:
                    order.append(i)
                nums.append(str(order.index(i) + 1))
        return f" [{','.join(nums)}]" if nums else ""

    lines = [f"# Interview loop: {info['company']}", ""]
    if not info["known"]:
        lines += ["No sourced loop for this company yet: this is the shared default for the level, built from the steps most sourced companies use (see `careerbrew interview-patterns`). Run the company-research skill to add one.", ""]
    if info.get("remote_policy"):
        lines.append(f"Remote policy: {info['remote_policy'].get('text', '')}{cite(info['remote_policy'].get('sources', []))}")
    if info.get("local_presence"):
        tp = info["local_presence"]
        lines.append(f"Local presence: {tp.get('value')}. {tp.get('note', '')}{cite(tp.get('sources', []))}")
    if info.get("ai_policy"):
        ap = info["ai_policy"]
        lines.append(f"AI tools in this process: {ap.get('allowed', 'unknown')}. {ap.get('summary', '')} (checked {ap.get('checked', '?')}){cite(ap.get('sources', []))}")
    if info.get("ai_shared"):
        lines += ["", "AI tools: this company has no sourced AI rule yet. Ask the recruiter. What sourced companies share:"]
        lines += [f"- {p['text']}{cite(p.get('sources', []))}" for p in info["ai_shared"]]
    if info.get("values"):
        lines.append("Published values or principles: " + "; ".join(info["values"]))
    for n, st in enumerate(info["loop"], 1):
        lines += ["", f"## {n}. {st.get('name', st['id'])} (~{st.get('typical_minutes', '?')} min)", st.get("description", "") + cite(st.get("sources", []))]
        if st.get("company_notes"):
            lines.append(f"At {info['company']}: {st['company_notes']}{cite(st.get('company_sources', []))}")
        if st.get("evaluated"):
            lines.append("They evaluate: " + " ".join(e.rstrip(".") + "." for e in st["evaluated"]))
        lines += [f"- Tip: {t['text']}" for t in st.get("tips", [])]
        lines += [f"- Avoid: {t['text']}" for t in st.get("traps", [])]
    if info.get("company_tips"):
        lines += ["", "## Company tips"]
        lines += [f"- {t['text']}{cite(t.get('sources', []))}" for t in info["company_tips"]]
    if with_sources and order:
        lines += ["", "## Sources", *[f"{k}. {src[i]['title']}: {src[i]['url']}" for k, i in enumerate(order, 1)]]
    return "\n".join(lines) + "\n"
