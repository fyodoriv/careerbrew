"""rust-18 Python oracle for evidence, ladder grading, promotion and LinkedIn.

The native candidate mirrors this synthetic workload. Every data root, model call
and plugin is synthetic: models are off (`CAREERBREW_LLM=off`, the fallback text),
and no plugin is installed. The report holds
the real functions' results; the written reports land in each case's own data root,
where the differential runner compares them.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from careerbrew import privacy, store  # noqa: E402
from careerbrew.work import evidence, ladder_grade, linkedin, promo  # noqa: E402

CASES = (
    "evidence-tags",
    "promo-assess",
    "ladder-grade",
    "linkedin-block",
)
CANARY = "SYNTHCANARY-rust18-term"


def typed(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, datetime):
        return {"datetime": value.isoformat()}
    if isinstance(value, (list, tuple)):
        return [typed(item) for item in value]
    if isinstance(value, set):
        return [typed(item) for item in sorted(value)]
    if isinstance(value, dict):
        return {str(key): typed(item) for key, item in value.items()}
    raise TypeError(type(value))


def compact(value: Any) -> str:
    return json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"


def error(fn) -> str:
    try:
        fn()
    except (ValueError, privacy.PrivacyError) as exc:
        return f"{type(exc).__name__}: {exc}"
    return "accepted"


class Workspace:
    def __init__(self, base: Path, name: str, settings: dict[str, Any] | None = None):
        self.base = base / name
        self.root = self.base / "data"
        self.root.mkdir(parents=True)
        (self.base / "home").mkdir()
        os.environ.update(
            HOME=str(self.base / "home"),
            CAREERBREW_HOME=str(self.root),
            CAREERBREW_KEY_FILE=str(self.base / "keys/backup.key"),
            CAREERBREW_EVENTS="off",
            CAREERBREW_ENCRYPT="on",
            CAREERBREW_LLM="off",
            CAREERBREW_NOW="2026-11-03T11:00:00+00:00",
            CAREERBREW_RUST13_WRITE_TRACE=str(self.base / "writes.log"),
            TZ="UTC",
        )
        if settings:
            store.write_yaml(self.root / "settings.yaml", settings)

    def evidence(self, rows: list[dict[str, Any]]) -> None:
        store.write_jsonl(evidence.evidence_path(), rows)


# ---------- evidence ----------

TITLES = (
    "feat(api): ship the order export",
    "fix: flaky checkout test",
    "perf: cut p99 latency of search",
    "Refactor(core): split the parser",
    "docs: explain the migration",
    "ci: cache the build",
    "chore: bump deps",
    "Led the incident review for the outage",
    "Mentored two new hires on design",
    "RFC for the queue",
    "plain title with no prefix",
    "",
)


def evidence_case(base: Path) -> dict[str, Any]:
    Workspace(base, "evidence-tags")
    tags = [evidence.tags_for(title) for title in TITLES]
    first = evidence.note("Led the incident review", ["ownership", "custom"], impact="cut the time to fix by 40%", at="2026-11-02T15:00:00+00:00")
    second = evidence.note("Mentored a new hire on design", [], links=["https://example.com/doc/1"])
    again = evidence.note("Led the incident review", ["ownership", "custom"], impact="cut the time to fix by 40%", at="2026-11-02T15:00:00+00:00")
    loaded = evidence.load()
    gone = evidence.forget({first["id"], "note:missing"})
    after = [row["id"] for row in evidence.load()]
    return {"tags": tags, "first": first, "second": second, "again_id": again["id"], "loaded": len(loaded), "forgot": gone, "after": after}


# ---------- promo ----------

PROMO_EVIDENCE = [
    {"id": "pr:example/shop#12", "kind": "pr", "repo": "example/shop", "title": "feat: ship the order export", "url": "https://github.com/example/shop/pull/12", "at": "2026-10-20T10:00:00Z", "tags": ["delivery"]},
    {"id": "pr:example/shop#14", "kind": "pr", "repo": "example/shop", "title": "Launch the billing page", "url": "https://github.com/example/shop/pull/14", "at": "2026-10-22T10:00:00+00:00", "tags": []},
    {"id": "git:shop:abc123def456", "kind": "commit", "repo": "shop", "title": "fix: flaky checkout test", "at": "2026-09-01T10:00:00+00:00", "tags": ["quality"]},
    {"id": "git:shop:bbb123def456", "kind": "commit", "repo": "shop", "title": "perf: cut search latency 40%", "at": "2025-01-01T10:00:00+00:00", "tags": ["quality", "impact"]},
    {"id": "note:1", "kind": "note", "title": "Wrote the RFC for the job queue", "impact": "unblocked two teams", "at": "2026-10-01T10:00:00+00:00", "tags": ["design"]},
    {"id": "note:2", "kind": "note", "title": "Mentored a new hire", "at": "2026-08-01T10:00:00+00:00", "tags": ["leadership"]},
    {"id": "note:3", "kind": "note", "title": "Graded line about the design review", "at": "2026-10-30T18:00:00+00:00", "tags": ["dim:collaboration"]},
    {"id": "note:4", "kind": "note", "title": "Migrated the billing jobs", "at": "not a time", "tags": []},
    {"id": "note:5", "kind": "note", "title": "Shipping notes for mentoring", "tags": []},
]

BAD_LADDERS = (
    None,
    {"id": "x", "levels": ["a"]},
    {"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}}]},
    {"id": "x", "levels": ["a", "b"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {"a": 1}}]},
    {"id": "x", "levels": ["a"], "competencies": [{"id": "c", "name": "C", "weight": 1, "signals": {}, "expect": {"a": 1}, "examples": ["no"]}]},
)


def promo_case(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "promo-assess")
    ws.evidence(PROMO_EVIDENCE)
    store.write_yaml(ws.root / "goals.yaml", {"long_term": [{"text": "Grow into technical leadership and mentoring"}], "queue": []})
    when = datetime.fromisoformat("2026-11-03T11:00:00+00:00")
    assessed = promo.assess("generic-swe", "senior", at=when)
    staff = promo.assess("generic-swe", "staff", at=when)
    gaps = [row["id"] for row in promo.gaps(assessed)]
    keywords = [
        [promo._matches({"title": title, "tags": []}, {"id": "c", "signals": {"keywords": [keyword]}}) for title in ("Migrating jobs", "remigrate", "ship it", "shipped", "the feat-flag", "Fix: x")]
        for keyword in ("migrat*", "ship", "feat", "fix*")
    ]
    errors = [error(lambda d=data: promo.validate_ladder(d)) for data in BAD_LADDERS]
    errors.append(error(lambda: promo.assess("missing", "senior", at=when)))
    errors.append(error(lambda: promo.assess("generic-swe", "principal", at=when)))
    path = promo.write_doc(assessed)
    return {
        "assessed": {**assessed, "competencies": [{**row, "evidence": [item["id"] for item in row["evidence"]]} for row in assessed["competencies"]]},
        "staff_overall": staff["overall"],
        "staff_warnings": staff["warnings"],
        "gaps": gaps,
        "keywords": keywords,
        "errors": errors,
        "doc_path": str(path.relative_to(ws.root)),
        "doc": path.read_text(encoding="utf-8"),
    }


# ---------- ladder grade ----------

LINES = [
    "Shipped the order export to all stores",
    "Fixed the flaky checkout test and added a regression test",
    "Reviewed four pull requests",
    "",
    "   ",
    "Wrote the design doc for the queue",
    "Mentored a new hire",
    "Cut search latency",
]


def ladder_case(base: Path) -> dict[str, Any]:
    Workspace(base, "ladder-grade", {"ladder": "generic-swe", "target_level": "senior", "timezone": "America/New_York"})
    levels = ladder_grade.levels("generic-swe", "senior")
    dims = [ladder_grade.dims_of(line) for line in LINES]
    unknown = ladder_grade.dims_of("Shipped", "missing")
    graded = ladder_grade.grade(LINES, "generic-swe", "senior")
    added = ladder_grade.record(graded, "2026-11-02")
    again = ladder_grade.record(graded, "2026-11-02")
    errors = [error(lambda: ladder_grade.levels("missing", "senior")), error(lambda: ladder_grade.levels("generic-swe", "principal"))]
    stored = [{key: row.get(key) for key in ("id", "title", "at", "tags")} for row in evidence.load()]
    return {"target": list(ladder_grade.target()), "levels": levels, "dims": dims, "unknown": unknown, "graded": graded, "added": added, "again": again, "errors": errors, "stored": stored}


# ---------- LinkedIn ----------

# Tag ranks tie for some tags, and `linkedin.build` orders a set of them: these tags keep every
# rank among the first four distinct, so the focus line never depends on string hash order.
LINKEDIN_EVIDENCE = [
    {"id": "n1", "kind": "note", "title": "Led the billing migration", "impact": "cut cost by $40k a year", "at": "2026-10-01T10:00:00+00:00", "tags": ["impact"]},
    {"id": "n2", "kind": "pr", "repo": "example/shopfront", "title": "feat(api): add the order export to shopfront", "at": "2026-10-02T10:00:00+00:00", "tags": ["ownership"]},
    {"id": "n3", "kind": "commit", "repo": "shopfront", "title": "PROJ-12: fix flaky checkout test", "at": "2026-10-03T10:00:00+00:00", "tags": ["quality"]},
    {"id": "n4", "kind": "commit", "title": "perf: improve search latency 40% for " + CANARY, "at": "2026-10-04T10:00:00+00:00", "tags": ["quality", "impact"]},
    {"id": "n5", "kind": "slack", "title": "Thanks for the big launch", "at": "2026-10-05T10:00:00+00:00", "tags": ["design"]},
    {"id": "n6", "kind": "note", "title": "Designed the event schema", "at": "2026-10-06T10:00:00+00:00", "tags": ["design"]},
    {"id": "n7", "kind": "note", "title": "fix: x", "at": "2026-10-07T10:00:00+00:00", "tags": []},
    {"id": "n8", "kind": "note", "title": "Designed the event schema", "at": "2026-10-08T10:00:00+00:00", "tags": ["design"]},
    {"id": "n9", "kind": "commit", "title": "docs: see https://example.com/a/b/c/d/e/f/g/h/i/j/k/l/m/n/o/p", "at": "2026-10-09T10:00:00+00:00", "tags": ["quality"]},
    {"id": "n10", "kind": "commit", "title": "ci: automate the release in 3 days", "at": "2025-01-01T10:00:00+00:00", "tags": ["ownership"]},
]


def linkedin_case(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "linkedin-block")
    ws.evidence(LINKEDIN_EVIDENCE)
    store.write_text(ws.root / "sensitive-terms.txt", f"# synthetic terms\n{CANARY}\nab\n")
    terms = privacy.sensitive_terms()
    repos = {"shopfront", "api"}
    cleaned = [
        linkedin._clean(title, repos, [], metrics)
        for title in ("feat: add the shopfront export 2x faster", "PROJ-9: Migrate the api to v2 in 3 days", "fix: ", "refactor(core): reworked caching", "docs: ship notes", "cut $1.5m cost")
        for metrics in (True, False)
    ]
    ranks = [list(linkedin._rank(item)) for item in LINKEDIN_EVIDENCE]
    built = linkedin.build("Senior engineer", 4)
    since = linkedin.build("", 6, since="2026-10-05")
    text = linkedin.path().read_text(encoding="utf-8")
    blocked = error(lambda: linkedin.build("Engineer at " + "Proj-1 " + "PROJ-77", 2))
    view = lambda r: {**r, "path": str(Path(r["path"]).relative_to(ws.root))}  # noqa: E731
    return {"terms_count": len(terms), "cleaned": cleaned, "ranks": ranks, "built": view(built), "since": view(since), "file": text, "blocked": blocked}


RUNNERS = {
    "evidence-tags": evidence_case,
    "promo-assess": promo_case,
    "ladder-grade": ladder_case,
    "linkedin-block": linkedin_case,
}


def run(case: str, base: Path) -> Any:
    with contextlib.redirect_stdout(io.StringIO()):
        return RUNNERS[case](base)


def main() -> int:
    args = sys.argv[1:]
    if args not in (["--all"],) and not (len(args) == 2 and args[0] == "--case" and args[1] in CASES):
        print("rust18.py: --all | --case <id>", file=sys.stderr)
        return 2
    selected = CASES if args == ["--all"] else (args[1],)
    root = os.environ.get("CAREERBREW_HOME", "")
    if not root:
        print("rust18.py: CAREERBREW_HOME is required", file=sys.stderr)
        return 2
    base = Path(root) / "rust18-workload"
    base.mkdir(parents=True, exist_ok=True)
    original_write_text = store.write_text

    def traced(path: Path, text: str, **kwargs: Any) -> None:
        trace = os.environ.get("CAREERBREW_RUST13_WRITE_TRACE")
        if trace:
            with Path(trace).open("a") as fh:
                fh.write(str(path) + "\n")
        return original_write_text(path, text, **kwargs)

    store.write_text = traced
    report = {"cases": [{"id": case, "result": run(case, base)} for case in selected]}
    sys.stdout.write(compact(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
