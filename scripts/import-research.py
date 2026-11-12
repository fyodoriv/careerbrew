#!/usr/bin/env python3
"""Turn knowledge/research/interview-research*.yaml into the committed knowledge files.

The base files (interview-research.yaml, ai-policy-research.yaml) hold the shared steps, the claims and the
sample companies. Optional batch files (interview-research-*.yaml, ai-policy-research-*.yaml) add your own companies;
the base output never depends on them.

Writes:
- knowledge/sources.yaml            one id per distinct URL the base files cite (plus EXTRA_SOURCES);
- sources-extra.yaml (same folder)  sources only the batch files cite (removed when there are none);
- knowledge/interview/steps.yaml    shared steps; each tip and trap cites its step's sources;
- knowledge/companies/<id>.yaml     base company loops; each loop step and tip cites the company's sources;
- knowledge/companies-extra/<id>.yaml  company loops from the batch files;
- knowledge/strategy.yaml           strategy rules mapped to research claims.
Rerun after updating the research file; review the diff before committing.
"""

from __future__ import annotations

import re
from datetime import date
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
EXTRA_SOURCES = {
    "rohrer-taylor-2007-interleaving": {"title": "Rohrer & Taylor (2007), The shuffling of mathematics problems improves learning, Instructional Science", "url": "https://doi.org/10.1007/s11251-007-9015-8", "kind": "research"},
    "anki-manual-leeches": {"title": "Anki Manual: Leeches", "url": "https://docs.ankiweb.net/leeches.html", "kind": "guide"},
    "anki-manual-daily-limits": {"title": "Anki Manual: Deck Options, Daily Limits", "url": "https://docs.ankiweb.net/deck-options.html", "kind": "guide"},
    "wozniak-twenty-rules": {"title": "Wozniak (1999), Effective learning: Twenty rules of formulating knowledge, SuperMemo", "url": "https://www.supermemo.com/en/blog/twenty-rules-of-formulating-knowledge", "kind": "research"},
    "spearman-1904": {"title": "Spearman (1904), The proof and measurement of association between two things, American Journal of Psychology", "url": "https://doi.org/10.2307/1412159", "kind": "research"},
    "apple-energy-efficiency": {
        "title": "Apple: Energy Efficiency Guide for Mac Apps",
        "url": "https://developer.apple.com/library/archive/documentation/Performance/Conceptual/power_efficiency_guidelines_osx/index.html",
        "kind": "official",
    },
    "sep-expected-utility": {"title": "Stanford Encyclopedia of Philosophy: Normative Theories of Rational Choice: Expected Utility", "url": "https://plato.stanford.edu/entries/rationality-normative-utility/", "kind": "research"},
}
RULES = [
    ("want-learning", "Want weights are refit to your ratings and kept only if rank agreement (Spearman) does not drop.", ["spearman-1904"]),
    ("season-tiers", "Apply to practice companies before target and dream companies; practice loops raise later pass rates.", ["practice-interviews-before-target-companies"]),
    ("one-loop-per-season", "Each company gives about one loop per season; reapply cooldowns are 6-12 months or more, so a used company leaves the plan.", ["company-cooldown-periods"]),
    ("readiness-gates", "Move from practice to target to dream only when drill readiness and practice pass rate clear their gates.", ["practice-interviews-before-target-companies", "retrieval-practice"]),
    ("expected-value", "Inside a tier, rank applications by expected value: P(offer) x Want.", ["sep-expected-utility"]),
    ("drill-retrieval", "Drills ask before they show: retrieval practice beats re-reading.", ["retrieval-practice"]),
    ("drill-spacing", "Reviews are spaced over days, not massed.", ["spacing-effect"]),
    ("drill-fsrs", "FSRS schedules each review from your rating.", ["fsrs-scheduler"]),
    ("binary-rubrics", "Answers are graded on structured, binary criteria, like structured interviews.", ["structured-interviews-predict-performance"]),
    ("star-stories", "Behavioral answers follow STAR, and the story bank needs coverage across competencies.", ["star-method"]),
    ("task-ranking", "Tasks rank by cost of delay over job size (WSJF), tilted toward promotion gaps.", ["wsjf-prioritization"]),
    ("calibrated-plans", "Plans inflate estimates by your measured actual/estimate ratio and leave a 20% buffer.", ["planning-fallacy"]),
    ("evidence-capture", "Record wins as they happen in a brag document; memory fades by review time.", ["brag-document"]),
    ("outcome-metrics", "Every metric answers a question tied to a goal (Goal-Question-Metric).", ["goal-question-metric"]),
    ("salary-range", "Know the market range and your walk-away number before the offer call.", ["salary-negotiation"]),
]


def slug(text: str) -> str:
    return re.sub(r"[^a-z0-9]+", "-", text.lower()).strip("-")[:48]


def main() -> None:
    d = yaml.safe_load((ROOT / "knowledge" / "research" / "interview-research.yaml").read_text(encoding="utf-8"))
    # Extra company research files (interview-research-*.yaml) add companies; the base file owns steps and claims.
    base_checked = str((d.get("meta") or {}).get("researched_on", "2026-10-18"))
    checked_by_company = {cid: base_checked for cid in d["companies"]}
    base_companies = list(d["companies"])
    official = {"sec.gov", *base_companies}
    for extra in sorted((ROOT / "knowledge" / "research").glob("interview-research-*.yaml")):
        more = yaml.safe_load(extra.read_text(encoding="utf-8")) or {}
        official |= set((more.get("meta") or {}).get("official_hosts") or [])
        extra_checked = str((more.get("meta") or {}).get("researched_on") or date.fromtimestamp(extra.stat().st_mtime).isoformat())
        for cid, c in (more.get("companies") or {}).items():
            if not c.get("loop"):
                print(f"skipped {cid}: no verified interview steps in {extra.name}")
                continue
            checked_by_company[cid] = extra_checked
            if cid in d["companies"]:
                raise SystemExit(f"{extra.name}: company {cid} already researched elsewhere")
            unknown = [s["step"] for s in c.get("loop", []) if s["step"] not in d["steps"]]
            if unknown:
                raise SystemExit(f"{extra.name}: company {cid} uses unknown steps {unknown}")
            d["companies"][cid] = c
    by_url: dict[str, str] = {}
    sources: dict[str, dict[str, str]] = {}
    current_checked = {"date": base_checked}
    frozen: set[str] = set()  # base source ids: a batch file never changes them

    def sid(src: dict[str, str]) -> str:
        url = src["url"].strip()
        if url in by_url:
            key = by_url[url]
            if key not in frozen:
                sources[key]["checked"] = max(sources[key]["checked"], current_checked["date"])
            return key
        host = re.sub(r"^https?://(www\.)?", "", url).split("/")[0]
        base = slug(f"{host.split('.')[0]}-{src.get('title', '')}")[:40] or slug(host)
        key, n = base, 2
        while key in sources:
            key, n = f"{base}-{n}", n + 1
        sources[key] = {
            "checked": current_checked["date"],
            "title": src.get("title", host),
            "url": url,
            "kind": "official" if any(w in host for w in (official if frozen else {"sec.gov", *base_companies})) else "guide",
        }
        by_url[url] = key
        return key

    steps = {}
    for step_id, s in d["steps"].items():
        ids = [sid(x) for x in s.get("sources", [])]
        steps[step_id] = {
            "name": s["name"],
            "description": s["description"],
            "evaluated": s["evaluated"],
            "typical_minutes": s.get("typical_minutes", "varies"),
            "tips": [{"text": t, "sources": ids} for t in s["tips"]],
            "traps": [{"text": t, "sources": ids} for t in s["traps"]],
            "prep": [f"You practiced this: {t}" for t in s["tips"][:2]],
            "sources": ids,
        }
    out_dir = ROOT / "knowledge" / "interview"
    out_dir.mkdir(parents=True, exist_ok=True)
    header = "# Generated by scripts/import-research.py from knowledge/research/interview-research.yaml. Edit the research file, then rerun.\n"
    (out_dir / "steps.yaml").write_text(header + yaml.safe_dump({"steps": steps}, sort_keys=False, allow_unicode=True, width=200), encoding="utf-8")

    comp_dir = ROOT / "knowledge" / "companies"
    comp_dir.mkdir(parents=True, exist_ok=True)
    extra_dir = ROOT / "knowledge" / "companies-extra"  # batch companies: outside the base set
    ai_file = ROOT / "knowledge" / "research" / "ai-policy-research.yaml"
    ai = yaml.safe_load(ai_file.read_text(encoding="utf-8")) if ai_file.exists() else {}
    ai_policies = dict((ai or {}).get("ai_policy") or {})
    for extra in sorted((ROOT / "knowledge" / "research").glob("ai-policy-research-*.yaml")):
        for cid, pol in ((yaml.safe_load(extra.read_text(encoding="utf-8")) or {}).get("ai_policy") or {}).items():
            ai_policies.setdefault(cid, pol)

    def write_company(cid: str, c: dict) -> None:
        current_checked["date"] = checked_by_company.get(cid, base_checked)
        ids = [sid(x) for x in c.get("sources", [])]
        out = {"id": cid, "name": c["name"], "careers_url": c.get("careers_url", ""), "checked": current_checked["date"]}
        tp = c.get("local_presence") or next((v for k, v in c.items() if k.endswith("_presence")), None)  # batch research files name this key after a place
        if tp:
            out["local_presence"] = {"value": tp.get("value"), "note": tp.get("note", ""), "sources": [sid(x) for x in tp.get("sources", [])] or ids}
        if c.get("remote_policy"):
            rp = c["remote_policy"]
            out["remote_policy"] = {"text": rp.get("summary") or rp.get("text", ""), "sources": [sid(x) for x in rp.get("sources", [])] or ids}
        out["loop"] = [{"step": s["step"], "notes": s.get("notes", ""), "sources": [sid(x) for x in s.get("sources", [])] or ids} for s in c["loop"]]
        out["company_tips"] = [{"text": t if isinstance(t, str) else t.get("text", ""), "sources": ids} for t in c.get("company_tips", [])]
        out["values_or_principles"] = c.get("values_or_principles", [])
        pol = ai_policies.get(cid.replace("-", "").replace(" ", "")) or ai_policies.get(cid)
        if pol and pol.get("sources"):
            current_checked["date"] = str(pol.get("checked", current_checked["date"]))
            out["ai_policy"] = {"allowed": pol.get("allowed", "unknown"), "summary": pol.get("summary", ""), "rounds": pol.get("rounds", {}), "checked": current_checked["date"], "sources": [sid(x) for x in pol["sources"]]}
        out["sources"] = ids
        target = comp_dir if cid in base_companies else extra_dir
        target.mkdir(parents=True, exist_ok=True)
        (target / f"{cid}.yaml").write_text(header + yaml.safe_dump(out, sort_keys=False, allow_unicode=True, width=200), encoding="utf-8")

    for cid in base_companies:
        write_company(cid, d["companies"][cid])
    claim_ids = {}
    for cl in d["claims"]:
        claim_ids[cl["claim_id"]] = [sid(x) for x in cl["sources"]]
        # A claim id also works as a source alias in rules: map it to its first source.
    current_checked["date"] = str(((ai or {}).get("meta") or {}).get("checked", base_checked))
    ai_shared = [{"text": p["text"], "sources": [sid(x) for x in p.get("sources", [])]} for p in (ai or {}).get("shared_patterns", []) if p.get("sources")]
    for k, v in EXTRA_SOURCES.items():
        sources[k] = {"checked": base_checked, **v}
    frozen.update(sources)
    for cid, c in d["companies"].items():
        if cid not in base_companies:
            write_company(cid, c)
    rules = []
    for rid, text, refs in RULES:
        ids = []
        for r in refs:
            ids.extend(claim_ids.get(r, [r]))
        rules.append({"id": rid, "text": text, "sources": sorted(set(ids))})
    (ROOT / "knowledge" / "strategy.yaml").write_text(
        "# Strategy rules careerbrew applies, each with its sources. Generated by scripts/import-research.py.\n"
        + yaml.safe_dump({"rules": rules, "claims": {k: v for k, v in claim_ids.items()}, "ai_shared_patterns": ai_shared}, sort_keys=False, width=200),
        encoding="utf-8",
    )
    (ROOT / "knowledge" / "sources.yaml").write_text(
        "# Every public source careerbrew cites. Generated by scripts/import-research.py; checked by scripts/check-sources.py.\n"
        + yaml.safe_dump({"sources": {k: v for k, v in sorted(sources.items()) if k in frozen}}, sort_keys=False, allow_unicode=True, width=200),
        encoding="utf-8",
    )
    extra_file = ROOT / "knowledge" / "sources-extra.yaml"
    extra_sources = {k: v for k, v in sorted(sources.items()) if k not in frozen}
    if extra_sources:
        extra_file.write_text("# Sources only the batch research files cite. Generated by scripts/import-research.py.\n" + yaml.safe_dump({"sources": extra_sources}, sort_keys=False, allow_unicode=True, width=200), encoding="utf-8")
    else:
        extra_file.unlink(missing_ok=True)
    print(f"sources {len(sources)}, steps {len(steps)}, companies {len(d['companies'])}, rules {len(rules)}")


if __name__ == "__main__":
    main()
