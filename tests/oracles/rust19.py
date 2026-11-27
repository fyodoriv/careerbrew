"""rust-19 Python oracle for job search, interview prep and public research.

Every case uses invocation-owned synthetic data, fixed clocks and the committed
HTTP and ATS fixtures. No live network, owner file or production route is used.
Reports hold postings, scores, plans and public knowledge text only.
"""

from __future__ import annotations

import json
import os
import sys
import time
from datetime import UTC, date, datetime
from pathlib import Path
from types import SimpleNamespace
from typing import Any

import fsrs

from careerbrew import interview, machine, paths, prep, research, secrets, store
from careerbrew.jobs import criteria, linkedin, platforms, sources, strategy, want
from careerbrew.tutor import engine

CASES = (
    "helpers",
    "ats-scan",
    "criteria-filter",
    "want-learn",
    "linkedin-alerts",
    "corporate-guard",
)
# Recorded vectors for later rust-19 slices (rust-followup). They run only with
# `--case <id>` and have no Rust candidate yet, so `--all` leaves them out.
DEFERRED = ("platforms", "strategy", "interview", "prep", "research")
ROOT = Path(__file__).resolve().parents[2]
FIX = ROOT / "tests" / "fixtures"
T0 = datetime(2026, 10, 19, 9, 0, tzinfo=UTC)
CANARIES = (b"RUST19CANARYPERSON", b"RUST19CANARYSECRET")

# Fixed vectors never use random interval fuzzing (process scope, oracle only).
engine.Scheduler = lambda: fsrs.Scheduler(enable_fuzzing=False)


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, set):
        return _typed(sorted(value))
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    if hasattr(value, "to_dict"):
        return _typed(value.to_dict())
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> list[str]:
    try:
        fn()
    except Exception as exc:
        return [type(exc).__name__, str(exc)]
    return ["accepted", ""]


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.root.mkdir(parents=True)
        self.root.chmod(0o700)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": "2026-10-19T13:00:00+00:00",
                "CAREERBREW_EXTRA_KNOWLEDGE": "0",
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
            }
        )
        os.environ.pop("CAREERBREW_HTTP_FIXTURES", None)
        time.tzset()


def _posting(pid: str, company: str, title: str, **extra: Any) -> sources.Posting:
    base: dict[str, Any] = {"location": "Remote", "remote": True, "url": f"https://example.com/{pid}", "description": ""}
    base.update(extra)
    return sources.Posting(id=pid, company=company, title=title, **base)


def _helpers(base: Path) -> dict[str, Any]:
    Workspace(base, "helpers")
    vocab = sources.skill_vocab()
    texts = [
        "&lt;p&gt;Go &amp; Kubernetes&lt;/p&gt;  <b>Kafka</b>\n\tnow",
        "<div>a&nbsp;b &#39;c&#39; &quot;d&quot; &#x41;</div>",
        "",
    ]
    salaries = ["$150k - $180k", "$210,000 - $260,000", "$90K to $1,200", "pay is $100k", "$1000k - $2000k", "$95k–$120k"]
    titles = ["Principal Engineer", "Staff SWE", "Team Lead", "Senior Dev", "Sr. Engineer", "Sr Engineer", "Junior Dev", "Intern", "Engineer", "SRE"]
    skill_texts = ["Python, golang and k8s on AWS", "Node.js plus TypeScript; ci/cd", "nothing here", "Rust-lang and C++ with ML", "Go-to market goal"]
    regions = [
        "Remote - US",
        "US only",
        "Anywhere in the World",
        "EMEA or US Pacific Time",
        "Asia Pacific time",
        "Remote, Central Time",
        "Australian Central Time",
        "Europe",
        "Examplia remote",
        "Examplian citizens",
        "Example City",
        "LATAM",
        "Worldwide",
        "Pacific timezone",
    ]
    custom = criteria.custom_regions({"regions_ok": ["global", "Examplia"], "region_words": {"examplia": ["Examplian", "examplia "]}})
    region_errors = [
        _error(lambda: criteria.custom_regions({"regions_ok": [""]})),
        _error(lambda: criteria.custom_regions({"regions_ok": 5})),
        _error(lambda: criteria.custom_regions({"region_words": []})),
        _error(lambda: criteria.custom_regions({"region_words": {"us": ["x"]}})),
        _error(lambda: criteria.custom_regions({"region_words": {"x": "y"}})),
        _error(lambda: criteria.custom_regions({"region_words": {"x": [""]}})),
        _error(lambda: criteria.custom_regions({"region_words": {"": ["y"]}})),
    ]
    return {
        "strip": [sources.strip_html(t) for t in texts],
        "salary": [list(sources.parse_salary(t)) for t in salaries],
        "seniority": [sources.seniority_of(t) for t in titles],
        "skills": [sources.extract_skills(t, vocab) for t in skill_texts],
        "regions": [sources.region_of(t) for t in regions],
        "custom": [[name, list(words)] for name, words in custom],
        "custom_regions": [sources.region_of(t, custom) for t in regions],
        "region_errors": region_errors,
        "single_label": criteria.custom_regions({"regions_ok": "Examplia"}),
        "vocab_size": len(vocab),
    }


def _ats_scan(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "ats")
    ats = FIX / "ats"
    store.write_yaml(
        ws.root / "jobs" / "sources.yaml",
        {
            "sources": [
                {"kind": "greenhouse", "board": "northwind", "company": "Northwind", "url": f"file://{ats / 'greenhouse-northwind.json'}", "tier": "dream"},
                {"kind": "lever", "board": "contoso", "company": "Contoso", "url": f"file://{ats / 'lever-contoso.json'}"},
                {"kind": "ashby", "board": "fabrikam", "url": f"file://{ats / 'ashby-fabrikam.json'}"},
            ]
        },
    )
    store.write_yaml(ws.root / "jobs" / "criteria.yaml", {"regions_ok": ["global", "us"], "region_words": {"sampleland": ["sample town"]}})
    scanned = sources.scan(no_cache=True)
    loaded = sources.load()
    bad_kind = _error(lambda: sources._normalize("workday", "X", {}, "user", None))
    return {
        "scanned": scanned,
        "loaded_ids": [p.id for p in loaded],
        "configured": [{k: v for k, v in s.items() if k != "url"} for s in sources.configured_sources()],
        "bad_kind": bad_kind,
    }


def _platform_sources() -> list[dict[str, Any]]:
    return [
        {"kind": "adzuna", "country": "GB", "what": "software engineer", "app_id": "env:RUST19_ADZUNA_ID", "app_key": "env:RUST19_ADZUNA_KEY"},
        {"kind": "remoteok", "tags": ["Python", "frontend"]},
        {"kind": "remoteok"},
        {"kind": "remotive", "search": "engineer"},
        {"kind": "himalayas", "country": "US"},
        {"kind": "hn", "must_mention": ["Python"]},
        {"kind": "hn"},
        {"kind": "wwr", "feed": "unknown-feed"},
    ]


def _platforms(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "platforms")
    os.environ["CAREERBREW_HTTP_FIXTURES"] = str(FIX / "http")
    os.environ["RUST19_ADZUNA_ID"] = "synthetic-id"
    os.environ["RUST19_ADZUNA_KEY"] = "RUST19CANARYSECRET"
    regions = criteria.custom_regions({"regions_ok": ["exampleland"], "region_words": {"exampleland": ["example city"]}})
    fetched = []
    for src in _platform_sources():
        fetched.append({"kind": src["kind"], "postings": platforms.fetch(dict(src), no_cache=True, regions=regions if src["kind"] == "adzuna" else ())})
    os.environ.pop("RUST19_ADZUNA_KEY")
    missing_secret = {"kind": "adzuna", "country": "gb", "app_id": "env:RUST19_ADZUNA_ID", "app_key": "env:RUST19_ADZUNA_KEY"}
    no_country = {"kind": "adzuna", "app_id": "env:RUST19_ADZUNA_ID", "app_key": "env:RUST19_ADZUNA_KEY"}
    default_keys = {"kind": "adzuna", "app_id": "keychain:careerbrew.adzuna.id", "app_key": "keychain:careerbrew.adzuna.key"}
    reasons = []
    for src in (missing_secret, no_country):
        try:
            platforms.fetch(dict(src), no_cache=True)
            reasons.append(["accepted"])
        except Exception as exc:
            reasons.append([type(exc).__name__, *platforms.skip_reason(src, exc)])
    reasons.append(list(platforms.skip_reason(default_keys, secrets.MissingSecretError("x"))))
    store.write_yaml(ws.root / "jobs" / "sources.yaml", {"sources": [missing_secret, {"kind": "wwr"}]})
    skipped: list[str] = []
    scanned = sources.scan(no_cache=True, on_skip=skipped.append)
    store.write_yaml(ws.root / "jobs" / "sources.yaml", {"sources": [missing_secret, no_country]})
    all_missing = _error(lambda: sources.scan(no_cache=True))
    return {
        "fetched": fetched,
        "reasons": reasons,
        "scan_skipped": skipped,
        "scan_ids": [p.id for p in scanned],
        "all_missing": all_missing,
        "kept_ids": [p.id for p in sources.load()],
        "preset": platforms.REMOTE_PRESET,
        "preset_criteria": platforms.REMOTE_CRITERIA,
    }


def _filter_postings() -> list[sources.Posting]:
    return [
        _posting("p1", "Acme", "Senior Software Engineer", seniority=3, salary_max=200000, region="global"),
        _posting("p2", "Acme", "Engineering Manager", seniority=2),
        _posting("p3", "Globex", "Software Engineering Intern", seniority=0),
        _posting("p4", "Initech", "Staff Developer", seniority=4, region="europe"),
        _posting("p5", "Umbrella", "Senior Engineer", seniority=3, remote=False, location="Example City"),
        _posting("p6", "Hooli", "Senior Engineer", seniority=3, remote=False, location="Sample Town", region=""),
        _posting("p7", "Vandelay", "Senior Engineer", seniority=3, salary_max=90000),
        _posting("p8", "Pied", "Product Designer", seniority=3),
        _posting("p9", "Massive", "Junior Developer", seniority=1),
        _posting("p10", "EXCLUDED CO", "Senior Engineer", seniority=3),
    ]


def _criteria_filter(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "criteria")
    defaults = criteria.load()
    store.write_yaml(
        ws.root / "jobs" / "criteria.yaml",
        {"min_seniority": 2, "exclude_companies": ["excluded co"], "regions_ok": ["global", "americas"], "locations_any": ["example city"], "min_salary": 150000},
    )
    loaded = criteria.load()
    postings = _filter_postings()
    reasons = {p.id: criteria.reject_reason(p, loaded) for p in postings}
    kept, rejected = criteria.apply(postings)
    remote_required = {**loaded, "remote": "required", "regions_ok": None, "min_salary": None}
    no_titles = {**defaults, "titles_any": [], "titles_none": []}
    return {
        "defaults": defaults,
        "loaded": loaded,
        "reasons": reasons,
        "kept": [p.id for p in kept],
        "rejected": dict(rejected),
        "remote_required": {p.id: criteria.reject_reason(p, remote_required) for p in postings},
        "no_titles": {p.id: criteria.reject_reason(p, no_titles) for p in postings},
        "bad_regions": _error(lambda: criteria.reject_reason(postings[0], {**loaded, "regions_ok": [3]})),
    }


def _want_postings() -> list[sources.Posting]:
    return [
        _posting("w1", "Alpha", "Senior Python Engineer", seniority=3, salary_max=220000, skills=["python", "aws", "kafka"]),
        _posting("w2", "Beta", "Staff Go Engineer", seniority=4, salary_max=180000, skills=["go", "kubernetes"]),
        _posting("w3", "Gamma", "Engineer", seniority=2, skills=[], location="Remote Golang shop"),
        _posting("w4", "Delta", "Senior Frontend Engineer", seniority=3, salary_max=150000, skills=["react", "typescript"], remote=False),
        _posting("w5", "Epsilon", "Principal Rust Engineer", seniority=5, salary_max=260000, skills=["rust", "distributed systems"]),
        _posting("w6", "Zeta", "Senior Data Engineer", seniority=3, skills=["sql", "python", "kafka"]),
        _posting("w7", "Eta", "Senior Go Engineer at Google", seniority=3, salary_max=200000, skills=["go"]),
    ]


def _want_learn(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "want")
    if os.environ.get("CAREERBREW_RUST19_EVENT_CONTROL") == "1":
        os.environ["CAREERBREW_EVENTS"] = "on"
    prof = {"skills": {"Python": 4, "AWS": 3, "Go": 2}, "want_to_learn": ["Rust", "Kubernetes"], "current_level": 3, "remote_preference": "remote", "target_salary": 180000}
    store.write_yaml(ws.root / "profile.yaml", prof)
    store.write_yaml(ws.root / "jobs" / "weights.yaml", {"comp": 4, "unknown": 9})
    crit = {**criteria.load(), "company_scores": {"Alpha": 0.9, "Delta": 0.2}}
    postings = _want_postings()
    w = want.weights()
    before = {p.id: want.score(p, w, want.profile(), crit) for p in postings}
    want.rate("w1", 5, ["python", "kafka"])
    want.rate("w2", 2, ["go", "Golang"])
    want.rate("w3", 1, ["go", "remote"])
    want.rate("w4", 2, ["remote"])
    want.rate("w5", 4, ["kafka"])
    want.rate("w2", 1, ["go"])
    bad_rating = _error(lambda: want.rate("w1", 6))
    tags = want.tag_words()
    factors = {p.id: want.tag_factor(p) for p in postings}
    after = {p.id: want.score(p, w, want.profile(), crit) for p in postings}
    agree = want.agreement(postings, w, want.profile(), crit)
    few = want.learn_weights(postings[:3], want.profile(), crit)
    learned = want.learn_weights(postings, want.profile(), crit, steps=400)
    want.rate("w6", 4, [])
    want.rate("w7", 3, ["google"])
    relearned = want.learn_weights(postings, want.profile(), crit, steps=300, lr=0.2)
    return {
        "weights": w,
        "before": before,
        "after": after,
        "tags": [sorted(tags[0]), sorted(tags[1])],
        "factors": factors,
        "ratings": want.ratings(),
        "agreement": agree,
        "few": few,
        "learned": learned,
        "relearned": relearned,
        "weights_after": want.weights(),
        "bad_rating": bad_rating,
        "spearman": [want.spearman([1.0, 2.0], [2.0, 1.0]), want.spearman([1.0, 1.0, 1.0], [1.0, 2.0, 3.0]), want.spearman([3.0, 1.0, 2.0, 2.0], [1.0, 2.0, 3.0, 3.0])],
        "rank": want._rank([3.0, 1.0, 3.0, 2.0, 1.0]),
    }


def _seed_readiness() -> None:
    criteria_ids = [c["id"] for c in engine.rubrics()["system-design"]["criteria"]]
    engine.record("sd-url-shortener-1", criteria_ids[:-1], at=T0)


def _strategy(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "strategy")
    store.write_yaml(ws.root / "profile.yaml", {"skills": {"python": 4, "aws": 3, "go": 1}, "current_level": 3, "target_salary": 180000, "remote_preference": "any"})
    store.write_yaml(ws.root / "jobs" / "criteria.yaml", {"min_seniority": 2})
    postings = _want_postings() + [
        _posting("w8", "alpha", "Senior Platform Engineer", seniority=3, salary_max=230000, skills=["python", "terraform"]),
        _posting("w9", "Theta", "Senior Backend Engineer", seniority=3, salary_max=190000, skills=["python", "redis"], tier_hint="practice"),
        _posting("w10", "Iota", "Senior SRE Engineer", seniority=3, salary_max=170000, skills=["kubernetes", "terraform", "go"]),
        _posting("w11", "Used", "Senior Engineer", seniority=3, salary_max=300000, skills=["llm"]),
        _posting("w12", "Kappa", "Intern Engineer", seniority=0),
    ]
    sources.save(postings)
    want.rate("w5", 5)
    want.rate("w3", 1)
    strategy.log_application("Used", "dream", "screen")
    strategy.log_application("Old", "practice", "onsite")
    store.append_jsonl(strategy.applications_path(), [{"company": "Ancient", "tier": "practice", "stage": "onsite", "at": "2024-01-01T00:00:00+00:00", "origin": "user"}])
    store.append_jsonl(strategy.applications_path(), [{"company": "Undated", "tier": "target", "stage": "screen", "at": "", "origin": "user"}])
    strategy.log_application("Applied", "target", "applied")
    bad_stage = _error(lambda: strategy.log_application("X", "dream", "ghosted"))
    used = strategy.used_companies()
    rates = strategy.pass_rates()
    first = strategy.build()
    first_md = paths.sub("out", "strategy.md").read_text(encoding="utf-8")
    bad_tier = _error(lambda: strategy.choose("w1", "stretch"))
    unknown = _error(lambda: strategy.choose("nope", "dream"))
    strategy.choose("w2", "dream")
    strategy.choose("w6", "target")
    strategy.choose("w10", "practice")
    strategy.choose("w7", "practice")
    strategy.choose("w11", "practice")
    strategy.choose("w1", "target")
    after_unchoose = strategy.unchoose("w1")
    _seed_readiness()
    for company, stage in (("P1", "screen"), ("P1", "onsite"), ("P2", "screen"), ("P3", "offer")):
        strategy.log_application(company, "practice", stage)
    second = strategy.build()
    second_md = paths.sub("out", "strategy.md").read_text(encoding="utf-8")
    gap_items = store.read_yaml(paths.sub("tutor", "gap-items.yaml"), {})
    return {
        "bad_stage": bad_stage,
        "used": sorted(used),
        "rates": rates,
        "first": first,
        "first_md": first_md,
        "bad_tier": bad_tier,
        "unknown": unknown,
        "after_unchoose": after_unchoose,
        "chosen": strategy.chosen(),
        "second": second,
        "second_md": second_md,
        "gap_items": [it["id"] for it in gap_items.get("items", [])],
        "steps_empty": strategy._steps("dream", 0.9, [], {"dream": [], "target": [], "practice": []}),
    }


def _interview(base: Path) -> dict[str, Any]:
    Workspace(base, "interview")
    known = interview.loop_for("  stripe ")
    by_name = interview.loop_for("Google")
    unknown = interview.loop_for("Unknown Co", 2)
    senior_unknown = interview.loop_for("Unknown Co")
    return {
        "patterns": interview.patterns(),
        "patterns_wide": interview.patterns(min_share=0.9),
        "default_2": interview.default_loop(2),
        "default_4": interview.default_loop(4),
        "known": {k: known[k] for k in ("company", "known")},
        "known_steps": [st["id"] for st in known["loop"]],
        "by_name": [st["id"] for st in by_name["loop"]],
        "unknown_steps": [st["id"] for st in unknown["loop"]],
        "senior_unknown": [st["id"] for st in senior_unknown["loop"]],
        "empty": _error(lambda: interview.loop_for("  ")),
        "render_known": interview.render(known),
        "render_plain": interview.render(by_name, with_sources=False),
        "render_unknown": interview.render(senior_unknown),
        "companies": sorted(interview.companies()),
        "source_count": len(interview.sources()),
        "find_missing": interview.find_company("nobody"),
    }


def _prep(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "prep")
    first_story = prep.add_story("Led a migration", ["leadership", "ownership"], "s", "a", "r")
    for n, comps in enumerate((["conflict"], ["failure"], ["impact"], ["ambiguity"], ["leadership"]), 2):
        prep.add_story(f"Story {n}", comps)
    stories = prep.stories()
    unknown = prep.checklist("Nowhere Inc", 2)
    prep.mark_stale("Stripe", "loop changed")
    known = prep.checklist("stripe")
    answers_file = ws.base / "answers.json"
    answers_file.write_text(json.dumps({"logistics": 1, "questions": 0}), encoding="utf-8")
    answers = prep.load_answers(str(answers_file), "environment, why-company,,")
    first = prep.run("Stripe", answers)
    second = prep.run("Stripe")
    _seed_readiness()
    third = prep.run("Stripe", {"company:changed": True, "offer:range": True})
    asked: list[str] = []
    fourth = prep.run("Nowhere Inc", ask=lambda q: (asked.append(q), "y" if len(asked) % 2 else "")[1], seniority=2)
    hist = store.read_json(prep.history_path("Stripe"), {})
    return {
        "first_story": {k: v for k, v in first_story.items()},
        "story_ids": [s["id"] for s in stories],
        "unknown": unknown,
        "known": known,
        "answers": answers,
        "first": first,
        "second": {k: second[k] for k in ("score", "trend")},
        "third": {k: third[k] for k in ("score", "trend", "gaps")},
        "fourth": fourth,
        "asked": asked,
        "stale_after": prep.stale_mark("Stripe"),
        "history_runs": len(hist["runs"]),
        "history_path": str(prep.history_path("Ünïcode Co!").relative_to(ws.root)),
        "empty_slug": str(prep.history_path("!!!").relative_to(ws.root)),
    }


def _research(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "research")
    os.environ["CAREERBREW_HTTP_FIXTURES"] = str(FIX / "http")
    today = date(2027, 6, 1)
    plain = research.stale(180, today=today)
    store.write_json(paths.sub("jobs", "strategy.json"), {"tiers": {"dream": [{"company": "GitLab"}], "target": [], "practice": [{"company": "stripe"}]}})
    piped = research.stale(200, today=today)
    url = "https://careers.example.com/interview-process"
    first = research.page(url)
    second = research.page(url)
    record_path = research._page_path(url)
    raw = store.read_json(record_path, {})
    raw["sha256"] = "0" * 64
    store.write_json(record_path, raw)
    third = research.page(url)
    return {
        "plain": plain,
        "piped": piped,
        "none_stale": research.stale(100000, today=today),
        "first": first,
        "second_changed": second["changed"],
        "third_changed": third["changed"],
        "last": research.last_page(url),
        "missing": research.last_page("https://careers.example.com/other"),
        "page_file": str(record_path.relative_to(ws.root)),
        "not_https": _error(lambda: research.page("http://careers.example.com/interview-process")),
        "no_fixture": _error(lambda: research.page("https://careers.example.com/absent")),
    }


def _linkedin_alerts(base: Path) -> dict[str, Any]:
    Workspace(base, "linkedin")
    sources.save([_posting("keep-1", "Keep", "Engineer")])
    msgs = [
        SimpleNamespace(id="m1", subject='"python engineer": Acme Corp - Senior Python Engineer and more'),
        SimpleNamespace(id="m2", subject="Globex – Remote Staff Engineer"),
        SimpleNamespace(id="m3", subject="no separator here"),
        SimpleNamespace(id="m4", subject="  Initech - Senior Golang Developer  "),
    ]
    built = linkedin.postings_from_alerts(msgs)
    added = linkedin.add_alert_postings(msgs)
    again = linkedin.add_alert_postings(msgs)
    return {"built": built, "added": added, "again": again, "ids": sorted(p.id for p in sources.load())}


def _corporate_guard(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "corporate")
    store.write_yaml(ws.root / "profile.yaml", {"name": "RUST19CANARYPERSON", "skills": {"python": 3}})
    sources.save([_posting("c1", "Corp", "Engineer")])
    prep.add_story("Corp story", ["impact"])
    groups = ("jobs", "interview", "interview-patterns", "prep", "drill", "cards", "research", "day")
    personal = {g: machine.guard_decision(g, machine.role()) for g in groups}
    refused = _error(lambda: machine.set_role("corporate"))
    kept = {"role": machine.role(), "postings": len(store.read_json(ws.root / "jobs" / "postings.json", {}) or {}), "stories": len(prep.stories())}
    clean = Workspace(base, "corporate-clean")
    store.write_yaml(clean.root / "profile.yaml", {"name": "Synthetic", "skills": {"python": 3}})
    machine.set_role("corporate")
    corporate = {g: _error(lambda g=g: machine.guard(g)) for g in groups}
    return {
        "personal": personal,
        "refused": refused,
        "kept": kept,
        "corporate": corporate,
        "role": machine.role(),
        "jobs_dir": (clean.root / "jobs").exists(),
    }


FUNCTIONS = {
    "helpers": _helpers,
    "ats-scan": _ats_scan,
    "platforms": _platforms,
    "criteria-filter": _criteria_filter,
    "want-learn": _want_learn,
    "strategy": _strategy,
    "interview": _interview,
    "prep": _prep,
    "research": _research,
    "linkedin-alerts": _linkedin_alerts,
    "corporate-guard": _corporate_guard,
}


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust19-workload"
    base.mkdir(parents=True, exist_ok=True)
    cases: list[dict[str, Any]] = []
    for name in CASES + DEFERRED:
        if (selected is None and name in CASES) or name == selected:
            _call(cases, name, lambda fn=FUNCTIONS[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":")) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES + DEFERRED:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
