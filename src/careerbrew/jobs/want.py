"""Want factor: a proposed 0–100 score per posting, explained per dimension.

careerbrew proposes weights; the user rates postings 1–5; `learn_weights`
refits the weights to the user's ratings (non-negative least squares by
projected gradient descent). `agreement` reports the Spearman correlation
between proposed scores and ratings, so we can see whether the model learns.
"""

from __future__ import annotations

import re
from typing import Any

from .. import calendar, events, paths, store
from .sources import Posting

DEFAULT_WEIGHTS = {"comp": 3.0, "tech_match": 2.0, "growth": 2.0, "learning": 1.0, "remote": 1.0, "company": 2.0}


def profile() -> dict[str, Any]:
    return store.read_yaml(paths.data_root() / "profile.yaml", {}) or {}


def weights() -> dict[str, float]:
    saved = store.read_yaml(paths.data_root() / "jobs" / "weights.yaml", {}) or {}
    return {**DEFAULT_WEIGHTS, **{k: float(v) for k, v in saved.items() if k in DEFAULT_WEIGHTS}}


def features(p: Posting, prof: dict[str, Any], crit: dict[str, Any]) -> dict[str, float]:
    skills = {k.lower() for k in (prof.get("skills") or {})}
    learn = {k.lower() for k in prof.get("want_to_learn") or []}
    target = crit.get("min_salary") or prof.get("target_salary")
    if p.salary_max and target:
        comp = max(0.0, min(1.0, 0.5 + (p.salary_max - float(target)) / (2 * float(target)) * 2))
    else:
        comp = 0.5
    ps = {s.lower() for s in p.skills}
    tech = len(ps & skills) / len(ps) if ps else 0.5
    lvl = int(prof.get("current_level", 3))
    growth = 1.0 if p.seniority > lvl else 0.6 if p.seniority == lvl else 0.2
    learning = min(1.0, len(ps & learn) / 2) if learn else 0.5
    pref = prof.get("remote_preference", "any")
    remote = 1.0 if (p.remote and pref != "office") or pref == "any" else 0.3
    company = float((crit.get("company_scores") or {}).get(p.company, 0.5))
    return {"comp": comp, "tech_match": tech, "growth": growth, "learning": learning, "remote": remote, "company": company}


def score(p: Posting, w: dict[str, float], prof: dict[str, Any], crit: dict[str, Any]) -> tuple[float, dict[str, float]]:
    f = features(p, prof, crit)
    total = sum(w.values()) or 1.0
    return round(100 * sum(w[k] * f[k] for k in w) / total * tag_factor(p), 1), f


TAG_TIMES, TAG_LOW, TAG_HIGH = 2, 2, 4  # a tag on this many ratings of TAG_LOW or less (TAG_HIGH or more) becomes a signal
TAG_DOWN, TAG_UP = 0.85, 1.1  # per matching tag: what the owner's rating tags do to a posting's Want (issue #185)


def tag_words() -> tuple[set[str], set[str]]:
    """Tags the owner put on low ratings, and on high ones, at least TAG_TIMES times: (disliked, liked)."""
    low: dict[str, int] = {}
    high: dict[str, int] = {}
    latest = {row["id"]: row for row in store.read_jsonl(ratings_path())}  # re-rating one posting counts once
    for row in latest.values():
        bucket = low if int(row["rating"]) <= TAG_LOW else high if int(row["rating"]) >= TAG_HIGH else None
        for t in row.get("tags") or []:
            if bucket is not None and (t := str(t).strip().lower()):
                bucket[t] = bucket.get(t, 0) + 1
    disliked = {t for t, n in low.items() if n >= TAG_TIMES}
    return disliked, {t for t, n in high.items() if n >= TAG_TIMES} - disliked


_TAGS_CACHE: dict[str, Any] = {}  # module scope, keyed by the ratings file's size and mtime: re-read when it changes


def tag_factor(p: Posting) -> float:
    """How the owner's rating tags move one posting: x0.85 per disliked tag in its title, company, location, or
    skills, x1.1 per liked one. A tag is a word or phrase the owner typed on `jobs rate --tags`."""
    rp = ratings_path()
    stamp = f"{rp}:{rp.stat().st_size}:{rp.stat().st_mtime_ns}" if rp.exists() else ""
    if _TAGS_CACHE.get("stamp") != stamp:
        _TAGS_CACHE.update(stamp=stamp, words=tag_words())
    disliked, liked = _TAGS_CACHE["words"]
    if not disliked and not liked:
        return 1.0
    text = " ".join([p.title, p.company, p.location, *p.skills]).lower()

    def has(t: str) -> bool:  # a whole word or phrase: the tag "go" never matches Google
        return re.search(rf"(?<![a-z0-9]){re.escape(t)}(?![a-z0-9])", text) is not None

    return round(TAG_DOWN ** sum(has(t) for t in disliked) * TAG_UP ** sum(has(t) for t in liked), 4)


def ratings_path():
    return paths.sub("jobs", "ratings.jsonl")


AUTO_LEARN_EVERY = 5  # `jobs rate` refits the Want weights after every this many ratings (issue #185)


def rate(posting_id: str, rating: int, note_tags: list[str] | None = None) -> None:
    if not 1 <= rating <= 5:
        raise ValueError("rating must be 1..5")
    store.append_jsonl(ratings_path(), [{"id": posting_id, "rating": rating, "tags": note_tags or [], "at": calendar.now_utc().isoformat(timespec="seconds"), "origin": "user"}])
    events.emit("jobs.rate", status="ok")


def ratings() -> dict[str, int]:
    out: dict[str, int] = {}
    for row in store.read_jsonl(ratings_path()):
        out[row["id"]] = int(row["rating"])  # latest wins
    return out


def _rank(xs: list[float]) -> list[float]:
    order = sorted(range(len(xs)), key=lambda i: xs[i])
    ranks = [0.0] * len(xs)
    i = 0
    while i < len(order):
        j = i
        while j + 1 < len(order) and xs[order[j + 1]] == xs[order[i]]:
            j += 1
        for k in range(i, j + 1):
            ranks[order[k]] = (i + j) / 2
        i = j + 1
    return ranks


def spearman(a: list[float], b: list[float]) -> float | None:
    if len(a) < 3:
        return None
    ra, rb = _rank(a), _rank(b)
    ma, mb = sum(ra) / len(ra), sum(rb) / len(rb)
    num = sum((x - ma) * (y - mb) for x, y in zip(ra, rb, strict=True))
    den = (sum((x - ma) ** 2 for x in ra) * sum((y - mb) ** 2 for y in rb)) ** 0.5
    return round(num / den, 3) if den else None


def agreement(postings: list[Posting], w: dict[str, float], prof: dict[str, Any], crit: dict[str, Any]) -> float | None:
    r = ratings()
    rated = [p for p in postings if p.id in r]
    return spearman([score(p, w, prof, crit)[0] for p in rated], [float(r[p.id]) for p in rated])


def learn_weights(postings: list[Posting], prof: dict[str, Any], crit: dict[str, Any], steps: int = 2000, lr: float = 0.05) -> dict[str, Any]:
    r = ratings()
    rated = [p for p in postings if p.id in r]
    if len(rated) < 5:
        return {"status": "need-more-ratings", "rated": len(rated)}
    before = agreement(postings, weights(), prof, crit)
    keys = list(DEFAULT_WEIGHTS)
    w = [weights()[k] / sum(weights().values()) for k in keys]
    xs = [[features(p, prof, crit)[k] for k in keys] for p in rated]
    ys = [(r[p.id] - 1) / 4 for p in rated]
    for _ in range(steps):
        grad = [0.0] * len(keys)
        for x, y in zip(xs, ys, strict=True):
            err = sum(wi * xi for wi, xi in zip(w, x, strict=True)) - y
            for i in range(len(keys)):
                grad[i] += 2 * err * x[i] / len(xs)
        w = [max(0.0, wi - lr * g) for wi, g in zip(w, grad, strict=True)]
    total = sum(w) or 1.0
    new = {k: round(10 * wi / total, 3) for k, wi in zip(keys, w, strict=True)}
    after = agreement(postings, new, prof, crit)
    # Keep the new weights only if they agree with the user at least as well (pre-registered keep rule).
    kept = after is not None and (before is None or after >= before)
    if kept:
        store.write_yaml(paths.data_root() / "jobs" / "weights.yaml", new)
    store.append_jsonl(paths.sub("metrics.jsonl"), [{"metric": "want_agreement", "value": after if kept else before, "at": calendar.now_utc().isoformat(timespec="seconds")}])
    events.emit("jobs.learn", status="kept" if kept else "reverted")
    return {"status": "kept" if kept else "reverted", "before": before, "after": after, "weights": new if kept else weights()}
