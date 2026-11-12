"""Hard filters from `<data root>/jobs/criteria.yaml`. A posting must pass all of them."""

from __future__ import annotations

from collections import Counter
from typing import Any

from .. import paths, store
from .sources import REGIONS, Posting

DEFAULTS: dict[str, Any] = {
    "titles_any": ["engineer", "developer"],
    "titles_none": ["intern", "manager", "director", "recruiter"],
    "min_seniority": 0,
    "remote": "any",  # required | any
    "locations_any": [],
    "min_salary": None,
    "exclude_companies": [],
    "regions_ok": None,  # e.g. [americas, global]: drops remote roles limited to another country or continent
    "region_words": {},  # e.g. {examplia: [examplian]}: more words for a label of your own (its name always matches)
}


def load() -> dict[str, Any]:
    user = store.read_yaml(paths.data_root() / "jobs" / "criteria.yaml", {}) or {}
    return {**DEFAULTS, **user}


def _region_labels(c: dict[str, Any]) -> list[str]:
    """The labels of `regions_ok` in lower case. One label written without a list is a list of one."""
    labels = c.get("regions_ok") or []
    if isinstance(labels, str):
        labels = [labels]
    if not isinstance(labels, list) or not all(isinstance(x, str) and x.strip() for x in labels):
        raise ValueError("criteria regions_ok must be a list of labels in quotes or plain words, for example [global, examplia]")
    return [x.strip().lower() for x in labels]


def custom_regions(c: dict[str, Any]) -> list[tuple[str, tuple[str, ...]]]:
    """The user's own region labels with their words, in lower case, for `sources.region_of`.

    A label is custom when it is not built in and `regions_ok` or `region_words` names it.
    It matches its own name, plus the words that `region_words` lists for it.
    """
    built_in = {label for label, _words in REGIONS}
    extra = c.get("region_words") or {}
    if not isinstance(extra, dict):
        raise ValueError("criteria region_words must be a mapping of label to a list of words, for example {examplia: [examplian]}")
    words: dict[str, list[str]] = {}
    for name in _region_labels(c):
        if name not in built_in:
            words.setdefault(name, [name])
    for label, more in extra.items():
        name = label.strip().lower() if isinstance(label, str) else ""
        if not name:
            raise ValueError("criteria region_words: each label must be a non-empty text")
        if name in built_in:
            raise ValueError(f"criteria region_words: {name} is a built-in label; add words only for a label of your own")
        if not isinstance(more, list):
            raise ValueError(f"criteria region_words: the words for {name} must be a list, for example [examplian]")
        if not all(isinstance(w, str) and w.strip() for w in more):
            raise ValueError(f"criteria region_words: each word for {name} must be a non-empty text")
        words.setdefault(name, [name]).extend(" ".join(w.lower().split()) for w in more)
    return [(name, tuple(dict.fromkeys(ws))) for name, ws in words.items()]


def reject_reason(p: Posting, c: dict[str, Any]) -> str | None:
    title = p.title.lower()
    if c["titles_any"] and not any(t.lower() in title for t in c["titles_any"]):
        return "title"
    if any(t.lower() in title for t in c["titles_none"]):
        return "title-excluded"
    if p.seniority < int(c["min_seniority"] or 0):
        return "seniority"
    if p.company.lower() in {x.lower() for x in c["exclude_companies"]}:
        return "company-excluded"
    if c.get("regions_ok") and getattr(p, "region", "") and p.region.lower() not in _region_labels(c):
        return "region"
    if c["remote"] == "required" and not p.remote:
        return "not-remote"
    if c["locations_any"] and p.location and not p.remote and not any(loc.lower() in p.location.lower() for loc in c["locations_any"]):
        return "location"
    if c["min_salary"] and p.salary_max is not None and p.salary_max < int(c["min_salary"]):
        return "salary"
    return None


def apply(postings: list[Posting], c: dict[str, Any] | None = None) -> tuple[list[Posting], Counter[str]]:
    c = c or load()
    kept: list[Posting] = []
    rejected: Counter[str] = Counter()
    for p in postings:
        reason = reject_reason(p, c)
        if reason:
            rejected[reason] += 1
        else:
            kept.append(p)
    return kept, rejected
