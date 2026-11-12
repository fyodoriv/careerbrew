"""Job platforms beyond company ATS boards, for remote roles and roles near you.

Field names and terms: knowledge/research/api-notes.md (checked 2026-10-18).

| kind      | needs                      | cache  | terms we follow                                    |
|-----------|----------------------------|--------|----------------------------------------------------|
| adzuna    | app_id, app_key (free)     | 6 h    | personal research use; credit "The Adzuna API"    |
| remoteok  | none                       | 6 h    | keep the Remote OK job URL as the link; credit it  |
| remotive  | none                       | 6 h    | <= 4 fetches/day; link back; credit Remotive       |
| himalayas | none (optional key)        | 12 h   | link back; credit Himalayas; never resubmit jobs  |
| hn        | none                       | 24 h   | one thread fetch per month is enough               |
| wwr       | none                       | 1 h    | RSS ttl 60 min; link to the job                    |

LinkedIn, Indeed, Glassdoor, Wellfound, and Welcome to the Jungle have no
usable public API and forbid scraping; LinkedIn comes in through your own
data export and job-alert emails (jobs/linkedin.py).

Every posting keeps the platform's own URL (link-back) and `source_name`.
Postings are never republished anywhere: careerbrew is personal research.
"""

from __future__ import annotations

import html
import json
import re
import urllib.parse
import xml.etree.ElementTree as ET  # noqa: S405 (parsing a public RSS feed we fetched ourselves)
from typing import Any

from .. import connectors, secrets, store
from .sources import CustomRegions, Posting, extract_skills, parse_salary, region_of, seniority_of, skill_vocab, strip_html

KINDS = ("adzuna", "remoteok", "remotive", "himalayas", "hn", "wwr")
TTL = {"adzuna": 6 * 3600, "remoteok": 6 * 3600, "remotive": 6 * 3600, "himalayas": 12 * 3600, "hn": 24 * 3600, "wwr": 3600}
WWR_FEEDS = {
    "all": "https://weworkremotely.com/remote-jobs.rss",
    "programming": "https://weworkremotely.com/categories/remote-programming-jobs.rss",
    "full-stack": "https://weworkremotely.com/categories/remote-full-stack-programming-jobs.rss",
    "back-end": "https://weworkremotely.com/categories/remote-back-end-programming-jobs.rss",
    "front-end": "https://weworkremotely.com/categories/remote-front-end-programming-jobs.rss",
    "devops": "https://weworkremotely.com/categories/remote-devops-sysadmin-jobs.rss",
}
CREDIT = {"adzuna": "The Adzuna API", "remoteok": "Remote OK", "remotive": "Remotive", "himalayas": "Himalayas", "hn": "Hacker News", "wwr": "We Work Remotely"}


def _get_json(url: str) -> Any:
    return json.loads(connectors.http_get(url).decode("utf-8"))


def _cached(kind: str, url: str, fn, no_cache: bool) -> Any:
    if no_cache:
        return fn(url)
    value, _hit = store.cached(["platform", kind, url], lambda: fn(url), ttl_seconds=TTL[kind])
    return value


def _posting(kind: str, pid: str, company: str, title: str, location: str, url: str, desc: str, smin, smax, remote: bool, vocab, origin: str) -> Posting:
    if smin is None:
        smin, smax = parse_salary(desc)
    return Posting(
        id=f"{kind}:{pid}",
        company=company.strip() or "Unknown",
        title=title.strip(),
        location=location.strip(),
        remote=remote,
        url=url,
        description=desc[:6000],
        salary_min=int(smin) if smin else None,
        salary_max=int(smax) if smax else None,
        seniority=seniority_of(title),
        skills=extract_skills(f"{title} {desc}", vocab),
        origin=origin,
        region=region_of(location) or ("global" if remote else ""),
        source_name=CREDIT[kind],
    )


class SourceSettingError(ValueError):
    """A job source entry lacks a required setting; the message says which one and how to add it."""


def adzuna(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    country = str(src.get("country") or "").strip().lower()
    if not country:  # no silent default: a search in the wrong country looks like "no jobs"
        raise SourceSettingError("adzuna source needs `country` (a two-letter code Adzuna supports, for example gb)")
    app_id, app_key = secrets.resolve(src["app_id"]), secrets.resolve(src["app_key"])
    q = urllib.parse.urlencode({k: v for k, v in {"app_id": app_id, "app_key": app_key, "what": src.get("what", "software engineer"), "where": src.get("where", ""), "results_per_page": 50, "content-type": "application/json"}.items() if v})
    url = f"https://api.adzuna.com/v1/api/jobs/{country}/search/1?{q}"
    # The cache key must not contain the key itself: hash the URL without credentials.
    raw = _cached("adzuna", url.replace(app_key, "KEY").replace(app_id, "ID"), lambda _u: _get_json(url), no_cache)
    out = []
    for j in raw.get("results", []):
        desc = strip_html(j.get("description", ""))
        loc = (j.get("location") or {}).get("display_name", "")
        remote = "remote" in f"{j.get('title', '')} {desc}".lower()
        out.append(_posting("adzuna", str(j["id"]), (j.get("company") or {}).get("display_name", ""), j.get("title", ""), loc, j.get("redirect_url", ""), desc, j.get("salary_min"), j.get("salary_max"), remote, vocab, origin))
    return out


def remoteok(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    raw = _cached("remoteok", "https://remoteok.com/api", _get_json, no_cache)
    tags = {t.lower() for t in src.get("tags", [])}
    out = []
    for j in raw[1:] if isinstance(raw, list) else []:  # item 0 is the legal notice
        if tags and not tags & {t.lower() for t in j.get("tags", [])}:
            continue
        out.append(
            _posting(
                "remoteok",
                str(j["id"]),
                j.get("company", ""),
                j.get("position", ""),
                j.get("location", "") or "Worldwide",
                j.get("url", ""),
                strip_html(j.get("description", "")),
                j.get("salary_min") or None,
                j.get("salary_max") or None,
                True,
                vocab,
                origin,
            )
        )
    return out


def remotive(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    q = urllib.parse.urlencode({k: v for k, v in {"category": src.get("category", "software-development"), "search": src.get("search"), "limit": src.get("limit", 100)}.items() if v})
    raw = _cached("remotive", f"https://remotive.com/api/remote-jobs?{q}", _get_json, no_cache)
    out = []
    for j in raw.get("jobs", []):
        desc = strip_html(j.get("description", ""))
        smin, smax = parse_salary(j.get("salary", "") or "")
        out.append(_posting("remotive", str(j["id"]), j.get("company_name", ""), j.get("title", ""), j.get("candidate_required_location", "") or "Worldwide", j.get("url", ""), desc, smin, smax, True, vocab, origin))
    return out


def himalayas(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    q = urllib.parse.urlencode({k: v for k, v in {"q": src.get("q", "software engineer"), "country": src.get("country", ""), "sort": "recent", "page": 1}.items() if v})
    raw = _cached("himalayas", f"https://himalayas.app/jobs/api/search?{q}", _get_json, no_cache)
    out = []
    for j in raw.get("jobs", []):
        loc = ", ".join(j.get("locationRestrictions") or []) or "Worldwide"
        out.append(
            _posting(
                "himalayas",
                str(j.get("guid", "")).rstrip("/").rsplit("/", 1)[-1],
                j.get("companyName", ""),
                j.get("title", ""),
                loc,
                j.get("guid", ""),
                strip_html(j.get("description", "") or j.get("excerpt", "")),
                j.get("minSalary"),
                j.get("maxSalary"),
                True,
                vocab,
                origin,
            )
        )
    return out


_HN_SPLIT = re.compile(r"\s*\|\s*")


def hn(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    """Top-level comments of the latest 'Ask HN: Who is hiring?' thread; the first line is Company | Role | Location | ..."""
    found = _cached("hn", "https://hn.algolia.com/api/v1/search_by_date?tags=story,author_whoishiring&hitsPerPage=5", _get_json, no_cache)
    thread = next((h for h in found.get("hits", []) if str(h.get("title", "")).startswith("Ask HN: Who is hiring?")), None)
    if not thread:
        return []
    tree = _cached("hn", f"https://hn.algolia.com/api/v1/items/{thread['objectID']}", _get_json, no_cache)
    want = [w.lower() for w in src.get("must_mention", ["remote"])]
    out = []
    for c in tree.get("children", []):
        text = c.get("text") or ""
        first = html.unescape(re.split(r"<p>|\n", text, maxsplit=1)[0])
        parts = [p for p in _HN_SPLIT.split(strip_html(first)) if p]
        if len(parts) < 2:
            continue
        body = strip_html(text)
        if want and not any(w in body.lower() for w in want):
            continue
        company, title = parts[0], parts[1]
        loc = next((p for p in parts[2:] if re.search(r"remote|onsite|hybrid|[A-Z][a-z]+,", p, re.IGNORECASE)), "")
        remote = "remote" in first.lower()
        out.append(_posting("hn", str(c["id"]), company, title, loc, f"https://news.ycombinator.com/item?id={c['id']}", body, None, None, remote, vocab, origin))
    return out


def wwr(src: dict[str, Any], no_cache: bool, vocab, origin: str) -> list[Posting]:
    url = WWR_FEEDS.get(src.get("feed", "programming"), WWR_FEEDS["programming"])
    xml_text = _cached("wwr", url, lambda u: connectors.http_get(u).decode("utf-8", "replace"), no_cache)
    root = ET.fromstring(xml_text)  # noqa: S314
    out = []
    for item in root.iter("item"):
        title_full = item.findtext("title", "")
        company, _, title = title_full.partition(": ")
        loc = " ".join(x for x in (item.findtext("region", ""), item.findtext("country", ""), item.findtext("state", "")) if x)
        guid = item.findtext("guid", "") or item.findtext("link", "")
        out.append(
            _posting("wwr", guid.rstrip("/").rsplit("/", 1)[-1], company, title or title_full, loc or "Anywhere in the World", item.findtext("link", guid), strip_html(item.findtext("description", "")), None, None, True, vocab, origin)
        )
    return out


FETCHERS = {"adzuna": adzuna, "remoteok": remoteok, "remotive": remotive, "himalayas": himalayas, "hn": hn, "wwr": wwr}


def fetch(src: dict[str, Any], no_cache: bool = False, regions: CustomRegions = ()) -> list[Posting]:
    postings = FETCHERS[src["kind"]](src, no_cache, skill_vocab(), src.get("origin", "user"))
    if regions:  # the user's own labels win over the built-in label each fetcher gave
        for p in postings:
            p.region = region_of(p.location, regions) or p.region
    return postings


def skip_reason(src: dict[str, Any], exc: Exception) -> tuple[str, str, str]:
    """Why a source was skipped, the fix, and the event status."""
    if isinstance(exc, SourceSettingError):
        return str(exc), "careerbrew data edit jobs/sources.yaml", "missing_setting"
    default_keys = src["kind"] == "adzuna" and src.get("app_id") == "keychain:careerbrew.adzuna.id" and src.get("app_key") == "keychain:careerbrew.adzuna.key"
    fix = "careerbrew secret set careerbrew.adzuna.id && careerbrew secret set careerbrew.adzuna.key" if default_keys else "check the configured secret references with careerbrew data edit jobs/sources.yaml"
    return "missing optional credential", fix, "missing_secret"


REMOTE_PRESET = {  # remote roles everywhere; the adzuna source has no country: add `country` (and `where` for roles near you)
    "sources": [
        {"kind": "himalayas", "q": "software engineer"},
        {"kind": "remotive", "category": "software-development"},
        {"kind": "remoteok", "tags": ["dev", "engineer", "backend", "frontend", "python", "typescript"]},
        {"kind": "wwr", "feed": "programming"},
        {"kind": "hn", "must_mention": ["remote"]},
        {"kind": "adzuna", "what": "software engineer", "app_id": "keychain:careerbrew.adzuna.id", "app_key": "keychain:careerbrew.adzuna.key"},
    ]
}
REMOTE_CRITERIA = {"regions_ok": ["global"]}
