"""Self-learning loop: measure outcomes, compare with pre-registered thresholds, pivot.

`run()` computes every metric in config/goals.yaml from real data under the
data root, labels it KEEP (>= success), WATCH (between), or PIVOT (< pivot),
appends a snapshot to `<data root>/metrics.jsonl`, and returns the pivot
actions. Missing data is reported as `no-data`, never as zero.
"""

from __future__ import annotations

import math
import statistics
from datetime import UTC, datetime, timedelta
from typing import Any

from . import calendar, charts, events, paths, store
from .jobs import criteria, sources, strategy, want
from .tutor import engine as tutor
from .work import evidence


def goals() -> dict[str, dict[str, Any]]:
    base = (store.read_yaml(paths.config_dir() / "goals.yaml", {}) or {}).get("metrics", {})
    mine = (store.read_yaml(paths.data_root() / "goals.yaml", {}) or {}).get("metrics", {})
    return {k: {**base.get(k, {}), **mine.get(k, {})} for k in {**base, **mine}}


def _last(metric: str) -> list[dict[str, Any]]:
    return [r for r in store.read_jsonl(paths.sub("metrics.jsonl")) if r.get("metric") == metric]


def measure(at: datetime | None = None) -> dict[str, float | None]:
    at = at or calendar.now()  # the pinned clock in tests and e2e runs, like every view (US-75)
    out: dict[str, float | None] = {}
    postings = sources.load()
    out["want_agreement"] = want.agreement(postings, want.weights(), want.profile(), criteria.load()) if postings else None
    out["readiness"] = tutor.readiness(strategy.CORE_TOPICS, at) if tutor.attempts() else None
    from .tutor import cards

    recent = [a for a in [*tutor.attempts(), *cards.reviews()] if datetime.fromisoformat(a["at"]) >= at - timedelta(days=30)]
    out["retention"] = round(sum(a["rating"] != "again" for a in recent) / len(recent), 3) if recent else None
    pr = strategy.pass_rates()["practice"]
    out["practice_pass_rate"] = pr["pass_rate"] if pr["loops"] else None
    promo = _last("promo_coverage")
    out["promo_coverage"] = promo[-1]["value"] if promo else None
    week_ago = [r for r in promo if datetime.fromisoformat(r["at"]) <= at - timedelta(days=6)]
    out["gap_closure"] = round(promo[-1]["value"] - week_ago[-1]["value"], 3) if promo and week_ago else None
    ratios = [v for r in _last("estimate_ratio") if isinstance(v := r.get("value"), int | float) and math.isfinite(v) and v > 0]
    out["estimate_ratio"] = round(statistics.median(ratios), 3) if ratios else None
    ev = evidence.load()
    if ev:
        latest = max(datetime.fromisoformat(e["at"].replace("Z", "+00:00")) for e in ev if e.get("at"))
        out["evidence_freshness_days"] = round((at - latest).total_seconds() / 86400, 1)
    else:
        out["evidence_freshness_days"] = None
    out.update(story_metrics(at))
    return out


def _iso(value: str) -> datetime | None:
    try:
        dt = datetime.fromisoformat(str(value).replace("Z", "+00:00"))
    except ValueError:
        try:
            from email.utils import parsedate_to_datetime

            dt = parsedate_to_datetime(str(value))
        except (TypeError, ValueError):
            return None
    return dt if dt.tzinfo else dt.replace(tzinfo=UTC)


def story_metrics(at: datetime) -> dict[str, float | None]:
    """Outcome metrics for stories US-17..US-25 (see docs/user-stories.md). None means no data yet."""
    from . import interview, plugins
    from .day import watch
    from .work import priorities

    out: dict[str, float | None] = {}
    # US-17: share of dream/target companies in the strategy with a sourced interview loop
    plan = store.read_json(paths.sub("jobs", "strategy.json"), {}) or {}
    tops = [r["company"] for t in ("dream", "target") for r in (plan.get("tiers") or {}).get(t, [])]
    out["loops_sourced"] = round(sum(1 for c in tops if interview.find_company(c)) / len(tops), 3) if tops else None
    # US-18: mean latest prep score over companies checked in the last 14 days
    scores = []
    for f in paths.sub("prep", ".keep").parent.glob("*.json"):
        runs = (store.read_json(f, {}) or {}).get("runs") or []
        if runs and (d := _iso(runs[-1]["at"])) and at - d <= timedelta(days=14):
            scores.append(runs[-1]["score"])
    out["prep_score"] = round(sum(scores) / len(scores), 3) if scores else None
    # US-19: eligible postings (pass criteria) currently known
    postings = sources.load()
    out["eligible_postings"] = float(len(criteria.apply(postings)[0])) if postings else None
    # US-21: share of interview emails followed by a logged application stage within 2 days
    inbox = store.read_jsonl(paths.sub("jobs", "inbox.jsonl"))
    apps = [d for r in store.read_jsonl(strategy.applications_path()) if (d := _iso(r["at"]))]
    if inbox:
        followed = sum(1 for m in inbox if (d := _iso(m.get("received", ""))) and any(d <= a <= d + timedelta(days=2) for a in apps))
        out["interview_followup"] = round(followed / len(inbox), 3)
    else:
        out["interview_followup"] = None
    # US-22: minutes from plugin install to its first collected evidence
    ev = evidence.load()
    mins = []
    for p in plugins.installed():
        # #54: a refresh resets installed_at; measure from the first install, and skip older evidence.
        inst = _iso(p.get("first_installed_at") or p.get("installed_at", ""))
        firsts = [d for e in ev if e.get("origin") == p["origin"] and (d := _iso(e.get("collected_at", ""))) and (not inst or d >= inst)]
        if inst and firsts:
            mins.append((min(firsts) - inst).total_seconds() / 60)
    out["plugin_setup_minutes"] = round(min(mins), 1) if mins else None
    # US-23: p95 CPU ms per watcher cycle (last 50 cycles)
    hist = sorted(watch.read_state().get("cpu_history") or [])
    out["watcher_cpu_ms"] = float(hist[min(len(hist) - 1, int(0.95 * len(hist)))]) if hist else None
    # US-24: share of suggested tasks finished the same day
    from .calendar import local_tz

    tz = local_tz()

    def local_day(value: str) -> str:
        d = _iso(value)
        return d.astimezone(tz).date().isoformat() if d else ""

    sugg = [s for s in store.read_jsonl(paths.sub("day", "suggestions.jsonl")) if "plugin" not in s]  # plugin rows share the file (#186)
    # Compare local calendar days: suggestions are stamped in local time, task completion in UTC.
    done = {t["id"]: local_day(str(t.get("done_at", ""))) for t in priorities.load() if t.get("status") == "done"}
    if sugg:
        out["suggestion_follow_rate"] = round(sum(1 for s in sugg if done.get(s["task"]) == local_day(s["at"])) / len(sugg), 3)
    else:
        out["suggestion_follow_rate"] = None
    # US-25: share of tasks in the most recent past plan that are done
    plans = sorted(p for p in paths.sub("day", ".keep").parent.glob("plan-*.json") if p.stem[5:] < at.date().isoformat())
    if plans:
        sched = (store.read_json(plans[-1], {}) or {}).get("schedule") or []
        out["plan_done_rate"] = round(sum(1 for i in sched if i["task"] in done) / len(sched), 3) if sched else None
    else:
        out["plan_done_rate"] = None
    return out


def status(value: float | None, goal: dict[str, Any]) -> str:
    if value is None:
        return "no-data"
    hib = goal.get("higher_is_better", True)
    ok = value >= goal["success"] if hib else value <= goal["success"]
    bad = value < goal["pivot"] if hib else value > goal["pivot"]
    return "KEEP" if ok else "PIVOT" if bad else "WATCH"


def run(at: datetime | None = None) -> list[dict[str, Any]]:
    at = at or calendar.now()
    gs = goals()
    values = measure(at)
    rows = []
    for name, goal in gs.items():
        v = values.get(name)
        st = status(v, goal)
        rows.append(
            {
                "metric": name,
                "value": v,
                "status": st,
                "success": goal.get("success"),
                "pivot": goal.get("pivot"),
                "higher_is_better": goal.get("higher_is_better", True),
                "question": goal.get("question", ""),
                "action": goal.get("pivot_action", "") if st == "PIVOT" else "",
            }
        )
    store.append_jsonl(paths.sub("review.jsonl"), [{"at": at.isoformat(timespec="seconds"), "values": values, "status": {r["metric"]: r["status"] for r in rows}}])
    events.emit("review.run", status="ok", pivots=sum(r["status"] == "PIVOT" for r in rows))
    return rows


def dashboard(rows: list[dict[str, Any]]) -> str:
    rep = charts.Report("careerbrew dashboard", f"Generated {calendar.now_utc().isoformat(timespec='minutes')} from your private data. Stays on this machine.")
    rep.kpis([(r["metric"].replace("_", " "), "—" if r["value"] is None else f"{r['value']}") for r in rows if r["value"] is not None][:6])
    rep.heading("Review")
    lines = []
    for r in rows:
        keep, piv = ("≥", "<") if r.get("higher_is_better", True) else ("≤", ">")
        lines.append(f"{r['status']}: {r['metric']} = {r['value']} (keep {keep} {r['success']}, pivot {piv} {r['pivot']}). {r['action']}".strip())
    rep.text(lines)
    rep.bars("Topic mastery (retrievability × last rubric score)", sorted(tutor.mastery().items(), key=lambda kv: -kv[1]))
    history = store.read_jsonl(paths.sub("tutor", "readiness.jsonl"))
    rep.line("Readiness over time (all topics)", [(h["at"][:16].replace("T", " "), h["readiness"]) for h in history])
    rep.line("Rubric score per attempt", [(a["at"][:16].replace("T", " "), a["score"]) for a in tutor.attempts()])
    promo = _last("promo_coverage")
    rep.line("Promotion coverage over time", [(p["at"][:10], p["value"]) for p in promo])
    return rep.html()
