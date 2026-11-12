#!/usr/bin/env python3
"""Gate step: every strategy rule, goal metric, interview tip, and company fact cites a source.

Checks (exit 1 on any failure, one line per problem with the fix):
- knowledge/sources.yaml: each source has a title and an https URL;
- knowledge/strategy.yaml: each rule has text and >= 1 known source id;
- config/goals.yaml: each metric lists >= 1 known source id;
- knowledge/interview/steps.yaml: each step has a description, >= 3 evaluated
  points, >= 3 tips and >= 3 traps, and every tip/trap and the step cite sources;
- knowledge/companies/*.yaml: every loop step names a known step id and cites
  sources; local_presence (or an older *_presence key) and remote_policy cite sources; every company tip cites sources.
"""

from __future__ import annotations

import os
import sys
from datetime import date
from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[1]
EXTRA_SOURCES = "/".join(("knowledge", "sources-extra.yaml"))  # optional: sources only your own batch research cites
STALE_DAYS = 180  # company processes change often; warn, then re-check with the browser-research skill
problems: list[str] = []


def load(rel: str):
    p = ROOT / rel
    if not p.exists():
        problems.append(f"{rel}: missing (fix: create it)")
        return {}
    return yaml.safe_load(p.read_text(encoding="utf-8")) or {}


def need_sources(where: str, ids, known: set[str]) -> None:
    ids = ids or []
    if not ids:
        problems.append(f"{where}: no sources (fix: add `sources: [<id from knowledge/sources.yaml>]`)")
    for i in ids:
        if i not in known:
            problems.append(f"{where}: unknown source id {i!r} (fix: add it to knowledge/sources.yaml)")


def main() -> int:
    extra = os.environ.get("CAREERBREW_EXTRA_KNOWLEDGE", "1") != "0"  # 0: check only the base set (tests)
    srcs = {**(load("knowledge/sources.yaml").get("sources") or {}), **((load(EXTRA_SOURCES) if extra and (ROOT / EXTRA_SOURCES).exists() else {}).get("sources") or {})}
    company_files = sorted(f for d in ("companies", *(("companies-extra",) if extra else ())) for f in (ROOT / "knowledge" / d).glob("*.yaml"))
    for sid, s in srcs.items():
        if not isinstance(s, dict) or not s.get("title") or not str(s.get("url", "")).startswith("https://"):
            problems.append(f"knowledge/sources.yaml: {sid} needs a title and an https url")
    known = set(srcs)
    today = date.today()
    stale = 0
    for sid, s in srcs.items():
        if not isinstance(s, dict) or not s.get("checked"):
            problems.append(f"knowledge/sources.yaml: {sid} has no `checked` date (fix: rerun scripts/import-research.py)")
            continue
        try:
            age = (today - date.fromisoformat(str(s["checked"]))).days
        except ValueError:
            problems.append(f"knowledge/sources.yaml: {sid} checked date is not YYYY-MM-DD")
            continue
        if age > STALE_DAYS:
            stale += 1
            print(f"WARN  {sid}: last checked {age} days ago (re-check: careerbrew research stale)")

    for rule in load("knowledge/strategy.yaml").get("rules") or []:
        if not rule.get("text"):
            problems.append(f"strategy rule {rule.get('id')}: no text")
        need_sources(f"strategy rule {rule.get('id')}", rule.get("sources"), known)

    for mid, goal in (load("config/goals.yaml").get("metrics") or {}).items():
        need_sources(f"goal metric {mid}", goal.get("sources"), known)

    steps = load("knowledge/interview/steps.yaml").get("steps") or {}
    for sid, st in steps.items():
        where = f"interview step {sid}"
        if not st.get("description"):
            problems.append(f"{where}: no description")
        if len(st.get("evaluated") or []) < 3:
            problems.append(f"{where}: needs >= 3 evaluated points")
        for kind in ("tips", "traps"):
            items = st.get(kind) or []
            if len(items) < 3:
                problems.append(f"{where}: needs >= 3 {kind}")
            for n, item in enumerate(items, 1):
                need_sources(f"{where} {kind[:-1]} {n}", (item or {}).get("sources"), known)
        need_sources(where, st.get("sources"), known)

    for f in company_files:
        c = yaml.safe_load(f.read_text(encoding="utf-8")) or {}
        where = f"company {f.stem}"
        need_sources(where, c.get("sources"), known)
        for key in sorted(k for k in c if k == "remote_policy" or k.endswith("_presence")):  # older files use another *_presence name
            need_sources(f"{where} {key}", (c[key] or {}).get("sources"), known)
        for n, step in enumerate(c.get("loop") or [], 1):
            if step.get("step") not in steps:
                problems.append(f"{where} loop {n}: unknown step {step.get('step')!r}")
            need_sources(f"{where} loop {n}", step.get("sources"), known)
        if c.get("ai_policy"):
            need_sources(f"{where} ai_policy", c["ai_policy"].get("sources"), known)
        for n, tip in enumerate(c.get("company_tips") or [], 1):
            need_sources(f"{where} tip {n}", (tip or {}).get("sources"), known)

    for p in problems:
        print(f"FAIL  {p}")
    n_companies = len(company_files)
    print(f"check-sources: {len(srcs)} sources ({stale} stale), {len(steps)} interview steps, {n_companies} companies, {len(problems)} problem(s)")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
