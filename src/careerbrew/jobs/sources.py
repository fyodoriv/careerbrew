"""Job sources: public ATS job-board APIs, normalized to one Posting shape.

Supported boards (all public, no login, no scraping):
- Greenhouse: https://boards-api.greenhouse.io/v1/boards/<board>/jobs?content=true
- Lever:      https://api.lever.co/v0/postings/<board>?mode=json
- Ashby:      https://api.ashbyhq.com/posting-api/job-board/<board>?includeCompensation=true

A source may set `url:` to override the endpoint (tests use file:// fixtures).
Responses are cached by URL for six hours.
"""

from __future__ import annotations

import html
import json
import re
import urllib.request
from collections.abc import Callable, Sequence
from dataclasses import asdict, dataclass, field
from pathlib import Path
from typing import Any

from .. import events, paths, plugins, secrets, store

ENDPOINTS = {
    "greenhouse": "https://boards-api.greenhouse.io/v1/boards/{board}/jobs?content=true",
    "lever": "https://api.lever.co/v0/postings/{board}?mode=json",
    "ashby": "https://api.ashbyhq.com/posting-api/job-board/{board}?includeCompensation=true",
}
CACHE_TTL = 6 * 3600
_TAG = re.compile(r"<[^>]+>")
_SALARY = re.compile(r"\$\s?(\d{2,3})(?:,(\d{3})|k)\s*(?:-|–|to)\s*\$?\s?(\d{2,3})(?:,(\d{3})|k)", re.IGNORECASE)
SENIORITY = [
    ("principal", 5),
    ("staff", 4),
    ("lead", 4),
    ("senior", 3),
    ("sr.", 3),
    ("sr ", 3),
    ("junior", 1),
    ("intern", 0),
]


@dataclass
class Posting:
    id: str
    company: str
    title: str
    location: str
    remote: bool
    url: str
    description: str
    salary_min: int | None = None
    salary_max: int | None = None
    seniority: int = 2
    skills: list[str] = field(default_factory=list)
    origin: str = "base"
    tier_hint: str | None = None
    region: str = ""
    source_name: str = ""

    def to_dict(self) -> dict[str, Any]:
        return asdict(self)


def strip_html(text: str) -> str:
    return re.sub(r"\s+", " ", html.unescape(_TAG.sub(" ", html.unescape(text or "")))).strip()


def parse_salary(text: str) -> tuple[int | None, int | None]:
    m = _SALARY.search(text)
    if not m:
        return None, None

    def num(a: str, b: str | None) -> int:
        return int(a) * 1000 + (int(b) if b else 0) if b else int(a) * 1000

    return num(m.group(1), m.group(2)), num(m.group(3), m.group(4))


def seniority_of(title: str) -> int:
    low = title.lower() + " "
    for word, level in SENIORITY:
        if word in low:
            return level
    return 2


def skill_vocab() -> dict[str, list[str]]:
    data = store.read_yaml(paths.REPO_ROOT / "knowledge" / "skills.yaml", {}) or {}
    return {k: [a.lower() for a in (v or [])] + [k.lower()] for k, v in data.get("skills", {}).items()}


def extract_skills(text: str, vocab: dict[str, list[str]] | None = None) -> list[str]:
    vocab = vocab or skill_vocab()
    low = f" {text.lower()} "
    found = []
    for skill, aliases in vocab.items():
        if any(re.search(rf"(?<![a-z0-9]){re.escape(a)}(?![a-z0-9])", low) for a in aliases):
            found.append(skill)
    return sorted(found)


# A user's own labels, as (label, words) pairs in lower case; `criteria.custom_regions` builds them.
CustomRegions = Sequence[tuple[str, Sequence[str]]]


def fetch_json(url: str) -> Any:
    if url.startswith("file://"):
        return json.loads(Path(url[7:]).read_text(encoding="utf-8"))
    req = urllib.request.Request(url, headers={"User-Agent": "careerbrew/0.1 (personal job search)"})
    with urllib.request.urlopen(req, timeout=30) as resp:  # noqa: S310 (fixed public endpoints)
        return json.loads(resp.read())


def _normalize(kind: str, company: str, raw: Any, origin: str, tier_hint: str | None, regions: CustomRegions = ()) -> list[Posting]:
    vocab = skill_vocab()
    out = []
    if kind == "greenhouse":
        items = raw.get("jobs", [])
        for j in items:
            desc = strip_html(j.get("content", ""))
            loc = (j.get("location") or {}).get("name", "")
            out.append(_mk(f"gh:{company}:{j['id']}", company, j.get("title", ""), loc, j.get("absolute_url", ""), desc, None, None, vocab, origin, tier_hint, regions=regions))
    elif kind == "lever":
        for j in raw:
            desc = strip_html((j.get("descriptionPlain") or j.get("description") or "") + " " + " ".join(strip_html(x.get("content", "")) for x in j.get("lists", [])))
            cats = j.get("categories") or {}
            rng = j.get("salaryRange") or {}
            out.append(
                _mk(
                    f"lever:{company}:{j['id']}",
                    company,
                    j.get("text", ""),
                    cats.get("location", ""),
                    j.get("hostedUrl", ""),
                    desc,
                    rng.get("min"),
                    rng.get("max"),
                    vocab,
                    origin,
                    tier_hint,
                    workplace=j.get("workplaceType", ""),
                    regions=regions,
                )
            )
    elif kind == "ashby":
        for j in raw.get("jobs", []):
            desc = strip_html(j.get("descriptionHtml") or j.get("descriptionPlain") or "")
            comp = (j.get("compensation") or {}).get("summaryComponents") or []
            smin = smax = None
            for c in comp:
                if c.get("compensationType") == "Salary":
                    smin, smax = c.get("minValue"), c.get("maxValue")
            out.append(_mk(f"ashby:{company}:{j['id']}", company, j.get("title", ""), j.get("location", ""), j.get("jobUrl", ""), desc, smin, smax, vocab, origin, tier_hint, workplace="remote" if j.get("isRemote") else "", regions=regions))
    else:
        raise ValueError(f"unknown source kind {kind!r}; use one of {sorted(ENDPOINTS)}")
    return out


def _mk(pid, company, title, loc, url, desc, smin, smax, vocab, origin, tier_hint, workplace="", regions: CustomRegions = ()) -> Posting:
    if smin is None:
        smin, smax = parse_salary(desc)
    remote = "remote" in f"{loc} {workplace} {title}".lower()
    return Posting(
        id=pid,
        company=company,
        title=title,
        location=loc,
        remote=remote,
        url=url,
        description=desc[:6000],
        salary_min=int(smin) if smin else None,
        salary_max=int(smax) if smax else None,
        seniority=seniority_of(title),
        skills=extract_skills(f"{title} {desc}", vocab),
        origin=origin,
        tier_hint=tier_hint,
        region=region_of(f"{loc} {workplace}", regions),
    )


REGIONS = [
    # No city or country label is built in: put your own city in `locations_any` and your own
    # country label in `regions_ok` (jobs/criteria.yaml).
    ("us", ("us only", "usa only", "united states only", "us-only", "remote - us", "remote, us", "remote (us", "(usa)", "u.s. only")),
    ("americas", ("americas", "north america", "latam", "na timezones", "est", "eastern time")),
    ("europe", ("europe", "emea", "uk only", "eu only", "cet")),
    ("asia", ("apac", "asia", "india")),
    ("global", ("worldwide", "anywhere", "global", "remote")),
]
# The other US zones by their long names. Checked after every label above `global`, so "Asia Pacific time" stays
# asia and "EMEA or US Pacific Time" stays europe. Short names (CST, PST) also name zones outside the Americas.
_US_ZONES = re.compile(r"(?<!\w)(?<!australian )(?:central|mountain|pacific) time(?!\w)")


def region_of(text: str, custom: CustomRegions = ()) -> str:
    """Where a role can be done from, as one label. Order matters: the most specific match wins.

    `custom` holds the user's own labels. They are checked first, so a country wins over a broad built-in label.
    """
    low = " ".join(text.lower().split())
    for label, words in (*custom, *REGIONS):
        if words is REGIONS[-1][1] and _US_ZONES.search(low):
            return "americas"
        # A whole word: no letter of any alphabet, digit or underscore on either side.
        if any(re.search(rf"(?<!\w){re.escape(w)}(?!\w)", low) for w in words):
            return label
    return ""


def configured_sources() -> list[dict[str, Any]]:
    out = []
    mine = store.read_yaml(paths.data_root() / "jobs" / "sources.yaml", {}) or {}
    out.extend({**s, "origin": "user"} for s in mine.get("sources", []))
    for origin, path in plugins.resources("job_sources"):
        data = store.read_yaml(path, {}) or {}
        out.extend({**s, "origin": origin} for s in data.get("sources", []))
    return out


def scan(no_cache: bool = False, *, on_skip: Callable[[str], None] | None = None) -> list[Posting]:
    postings: list[Posting] = []
    from . import criteria, platforms

    regions = criteria.custom_regions(criteria.load())  # read once per scan, never per posting

    keep = [p for p in load() if p.source_name.startswith("LinkedIn")]
    postings.extend(keep)
    completed, skipped = 0, 0
    for src in configured_sources():
        if src["kind"] in platforms.KINDS:
            try:
                fetched = platforms.fetch(src, no_cache=no_cache, regions=regions)
            except (secrets.MissingSecretError, platforms.SourceSettingError) as exc:
                skipped += 1
                # One skip line per source: what is missing and the command that fixes it. A missing
                # secret and a missing required setting (platforms.SourceSettingError) both skip the source.
                why, fix, status = platforms.skip_reason(src, exc)
                if on_skip is not None:
                    on_skip(f"skipped {src['kind']}: {why}; fix: {fix}")
                events.emit("jobs.source", source_kind=src["kind"], status=status)
                continue
            postings.extend(fetched)
            completed += 1
            events.emit("jobs.source", source_kind=src["kind"], status="ok")
            continue
        kind, board = src["kind"], src["board"]
        url = src.get("url") or ENDPOINTS[kind].format(board=board)
        company = src.get("company", board)
        if no_cache:
            raw = fetch_json(url)
        else:
            raw, _hit = store.cached(["ats", url], lambda u=url: fetch_json(u), ttl_seconds=CACHE_TTL)
        for p in _normalize(kind, company, raw, src.get("origin", "user"), src.get("tier"), regions):
            p.source_name = kind.title()
            postings.append(p)
        events.emit("jobs.source", source_kind=kind, status="ok")
        completed += 1
    if skipped and not completed:
        raise secrets.MissingSecretError("no job source completed: optional credentials are missing; previous postings were kept")
    save(postings)
    return postings


def postings_path() -> Path:
    return paths.sub("jobs", "postings.json")


def save(postings: list[Posting]) -> None:
    store.write_json(postings_path(), {p.id: p.to_dict() for p in postings})


def load() -> list[Posting]:
    data = store.read_json(postings_path(), {}) or {}
    return [Posting(**v) for v in data.values()]
