"""LinkedIn without scraping: your own data export, and the job-alert emails you already get.

LinkedIn offers no job-search API to individuals and its User Agreement bans
scraping (knowledge/research/api-notes.md section 7). Two allowed inputs:

1. `careerbrew jobs linkedin-import <export.zip>` reads LinkedIn's "Download
   your data" archive by header name (column order is not published):
   - Positions.csv      → your job history (private/linkedin/positions.json)
   - Skills.csv         → skills missing from profile.yaml are suggested
   - Jobs/Saved Jobs.csv → postings (source "LinkedIn saved")
   - Jobs/Job Applications*.csv → application log entries (stage "applied"),
     which also mark companies whose season loop may be in use
2. Job-alert emails (from jobalerts-noreply@linkedin.com) found by
   `careerbrew mail scan` become postings (source "LinkedIn alert") from the
   subject line only: `"<search>": <Company> - <Title> and more`.
"""

from __future__ import annotations

import csv
import io
import re
import zipfile
from pathlib import Path
from typing import Any

from .. import events, paths, store
from . import sources, strategy

ALERT_SENDERS = {"jobalerts-noreply@linkedin.com"}
APPLICATION_SENDERS = {"jobs-noreply@linkedin.com"}


def _rows(zf: zipfile.ZipFile, name_pattern: str) -> list[dict[str, str]]:
    out: list[dict[str, str]] = []
    for name in zf.namelist():
        if re.search(rf"(?:^|/){name_pattern}$", name, re.IGNORECASE):
            text = zf.read(name).decode("utf-8-sig", "replace")
            lines = text.splitlines()
            # Connections.csv-style files start with "Notes:" lines before the header row.
            start = next((i for i, line in enumerate(lines) if "," in line and not line.lower().startswith("notes")), 0)
            reader = csv.DictReader(io.StringIO("\n".join(lines[start:])))
            out.extend({(k or "").strip().lower(): (v or "").strip() for k, v in r.items()} for r in reader)
    return out


def import_export(zip_path: Path) -> dict[str, int]:
    with zipfile.ZipFile(zip_path) as zf:
        positions = _rows(zf, r"Positions\.csv")
        skills = _rows(zf, r"Skills\.csv")
        saved = _rows(zf, r"Saved Jobs\.csv")
        applied = _rows(zf, r"Job Applications(?:_\d+)?\.csv")
    store.write_json(paths.sub("linkedin", "positions.json"), [{k: p.get(k, "") for k in ("company name", "title", "location", "started on", "finished on")} for p in positions])
    prof = store.read_yaml(paths.data_root() / "profile.yaml", {}) or {}
    have = {s.lower() for s in (prof.get("skills") or {})}
    suggest = sorted({s["name"] for s in skills if s.get("name") and s["name"].lower() not in have})
    store.write_json(paths.sub("linkedin", "skill-suggestions.json"), suggest)
    current = {p.id: p for p in sources.load()}
    vocab = sources.skill_vocab()
    for row in saved:
        url = row.get("job url", "")
        pid = "linkedin-saved:" + store.digest(url or row.get("job title", ""))[:12]
        current[pid] = sources.Posting(
            id=pid,
            company=row.get("company name", "") or "Unknown",
            title=row.get("job title", ""),
            location="",
            remote=False,
            url=url,
            description="",
            seniority=sources.seniority_of(row.get("job title", "")),
            skills=sources.extract_skills(row.get("job title", ""), vocab),
            origin="user",
            source_name="LinkedIn saved",
        )
    sources.save(list(current.values()))
    logged = 0
    seen = {(r["company"].casefold(), r.get("stage")) for r in store.read_jsonl(strategy.applications_path())}
    for row in applied:
        company = row.get("company name", "")
        if company and (company.casefold(), "applied") not in seen:
            strategy.log_application(company, "unknown", "applied")
            seen.add((company.casefold(), "applied"))
            logged += 1
    events.emit("jobs.linkedin.import", status="ok", positions=len(positions), saved=len(saved), applied=logged)
    return {"positions": len(positions), "skills_suggested": len(suggest), "saved_jobs": len(saved), "applications_logged": logged}


_ALERT = re.compile(r'^(?:"[^"]+":\s*)?(?P<company>[^-–]+?)\s+[-–]\s+(?P<title>.+?)(?:\s+and\s+more)?$', re.IGNORECASE)


def postings_from_alerts(msgs: list[Any]) -> list[sources.Posting]:
    vocab = sources.skill_vocab()
    out = []
    for m in msgs:
        match = _ALERT.match(m.subject.strip())
        if not match:
            continue
        title = match.group("title").strip()
        out.append(
            sources.Posting(
                id="linkedin-alert:" + store.digest(m.id)[:12],
                company=match.group("company").strip(),
                title=title,
                location="",
                remote="remote" in title.lower(),
                url="https://www.linkedin.com/jobs/",
                description="",
                seniority=sources.seniority_of(title),
                skills=sources.extract_skills(title, vocab),
                origin="user",
                source_name="LinkedIn alert",
            )
        )
    return out


def add_alert_postings(msgs: list[Any]) -> int:
    current = {p.id: p for p in sources.load()}
    new = [p for p in postings_from_alerts(msgs) if p.id not in current]
    for p in new:
        current[p.id] = p
    sources.save(list(current.values()))
    return len(new)
