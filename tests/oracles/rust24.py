"""rust-24 Python oracle for content-free report, metric and export builders.

The native candidate mirrors this synthetic workload. Every case reads the same
synthetic inputs from `tests/fixtures/rust24/cases.json`, and every data root
is synthetic. The report holds counts, ids, allowlisted words and careerbrew's
own chart markup only; free text planted in the inputs must never appear.

Typed seams (rust-24 component, owner decision 2026-11-27): the work-tool,
watcher, task, goal and inbox readers that `checklist` asks are owned by
rust-17, rust-18, rust-19 and rust-23, so this oracle supplies their facts
from the fixture; `field.machine_label` is pinned because its salt is random.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from careerbrew import charts, checklist, events, field, goals, paths, repo_health, telemetry, time_saved  # noqa: E402
from careerbrew.day import inbox, watch  # noqa: E402
from careerbrew.work import evidence, priorities  # noqa: E402

CASES = (
    "event-clean",
    "telemetry-allowlist",
    "repo-health",
    "time-saved",
    "field-totals",
    "charts",
    "checklist",
)
FIXTURE = ROOT / "tests" / "fixtures" / "rust24" / "cases.json"
MACHINE = "m-synthetic0"


def typed(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": repr(value)}
    if isinstance(value, (list, tuple)):
        return [typed(item) for item in value]
    if isinstance(value, dict):
        return {key: typed(item) for key, item in value.items()}
    raise TypeError(type(value))


def compact(value: Any) -> str:
    return json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"


def fixture() -> dict[str, Any]:
    return json.loads(FIXTURE.read_text(encoding="utf-8"))


def settings(text: str) -> None:
    (paths.data_root() / "settings.yaml").write_text(text, encoding="utf-8")


def scratch(name: str) -> Path:
    base = Path(os.environ["TMPDIR"]) / name
    base.mkdir(parents=True, exist_ok=True)
    return base


def event_clean(data: dict[str, Any]) -> dict[str, Any]:
    return {"cleaned": [events.clean(fields) for fields in data["events"]]}


def telemetry_allowlist(data: dict[str, Any]) -> dict[str, Any]:
    t = data["telemetry"]
    return {
        "events": [telemetry.to_event(record, t["distinct_id"], t["since"]) for record in t["records"]],
        "command_names": sorted(telemetry.command_names()),
        "errors": sorted(telemetry.ERRORS),
        "sets": {
            "event_names": sorted(telemetry.EVENT_NAMES),
            "statuses": sorted(telemetry.STATUSES),
            "source_kinds": sorted(telemetry.SOURCE_KINDS),
            "tiers": sorted(telemetry.TIERS),
            "view_routes": sorted(telemetry.VIEW_ROUTES),
            "web_acts": sorted(telemetry.WEB_ACTS),
            "actions": sorted(telemetry.ACTIONS),
            "user_commands": sorted(telemetry.USER_COMMANDS),
        },
    }


def _apply(payload: Any, path: list[Any], op: str, value: Any) -> Any:
    copy = json.loads(json.dumps(payload))
    target = copy
    for part in path[:-1]:
        target = target[part]
    if op == "del":
        del target[path[-1]]
    else:
        target[path[-1]] = value
    return copy


def _error(fn) -> str:
    try:
        fn()
    except repo_health.PublicError as exc:
        return str(exc)
    return "accepted"


def repo_health_case(data: dict[str, Any]) -> dict[str, Any]:
    r = data["repo_health"]
    settings(data["settings"])
    (paths.data_root() / "sensitive-terms.txt").write_text(r["terms"], encoding="utf-8")
    repo = scratch("rust24-repo")
    (repo / "scripts").mkdir(exist_ok=True)
    (repo / ".metrics").mkdir(exist_ok=True)
    (repo / "scripts" / "check.sh").write_text(r["check_sh"], encoding="utf-8")
    (repo / ".metrics" / "events.jsonl").write_text("\n".join(r["metrics_events"]) + "\n", encoding="utf-8")
    (repo / ".metrics" / "e2e-results.json").write_text(r["e2e_results"], encoding="utf-8")
    reports = paths.data_root() / "out" / "reports"
    reports.mkdir(parents=True, exist_ok=True)
    for name, body in r["summaries"]:
        (reports / name).write_text(body, encoding="utf-8")
    os.environ["CAREERBREW_REPO_DIR"] = str(repo)
    payload = repo_health.collect(r["days"])
    out = scratch("rust24-public")
    written = [p.name for p in repo_health.write_rows(payload, out)]
    return {
        "gate_steps": sorted(repo_health.gate_steps()),
        "payload": payload,
        "valid": _error(lambda: repo_health.validate(payload)),
        "bad_days": [_error(lambda d=d: repo_health.collect(d)) for d in r["bad_days"]],
        "mutations": [_error(lambda m=m: repo_health.validate(_apply(payload, *m))) for m in r["mutations"]],
        "written": written,
        "inside": _error(lambda: repo_health.write_rows(payload, paths.data_root() / "public")),
    }


def _log(data: dict[str, Any]) -> Path:
    path = paths.data_root() / "rust24" / "events.jsonl"
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(data["time_saved"]["log"]) + "\n", encoding="utf-8")
    return path


def time_saved_case(data: dict[str, Any]) -> dict[str, Any]:
    settings(data["settings"])
    log = _log(data)
    captured: list[Any] = []
    original = events.emit
    events.emit = lambda event, **fields: captured.append(events.clean({"event": event, **fields}))
    try:
        web = []
        for saves, step, seconds in data["time_saved"]["web"]:
            try:
                time_saved.record_web(saves, step, seconds)
                web.append({"accepted": captured[-1]})
            except ValueError as exc:
                web.append({"refused": str(exc)})
    finally:
        events.emit = original
    return {"computed": time_saved.compute(log), "web": web, "command_action": time_saved.COMMAND_ACTION}


def field_totals(data: dict[str, Any]) -> dict[str, Any]:
    settings(data["settings"])
    log = _log(data)
    original = field.machine_label
    field.machine_label = lambda: MACHINE
    try:
        return {"field": field.compute(log), "empty": field.compute(log.parent / "missing.jsonl")}
    finally:
        field.machine_label = original


def charts_case(data: dict[str, Any]) -> dict[str, Any]:
    c = data["charts"]
    rep = charts.Report(c["title"], c["subtitle"])
    rep.kpis([tuple(item) for item in c["kpis"]])
    rep.text(c["text"])
    for bar in c["bars"]:
        rep.bars(bar["title"], [tuple(row) for row in bar["rows"]], max_value=bar["max_value"], fmt=bar["fmt"])
    for line in c["lines"]:
        rep.line(line["title"], [tuple(point) for point in line["points"]], y_max=line["y_max"], fmt=line["fmt"])
    empty = charts.Report("Empty")
    return {"html": rep.html(), "empty": empty.html()}


def checklist_case(data: dict[str, Any]) -> dict[str, Any]:
    saved = (evidence.sources_list, watch.read_state, priorities.load, goals.load, inbox.load)
    results = []
    try:
        for facts in data["checklist"]:
            settings(facts["settings"])
            evidence.sources_list = lambda f=facts: f["sources"]
            watch.read_state = lambda f=facts: f["watch"]
            priorities.load = lambda f=facts: f["tasks"]
            goals.load = lambda f=facts: f["goals"]
            inbox.load = lambda f=facts: f["inbox"]
            results.append(checklist.build())
    finally:
        evidence.sources_list, watch.read_state, priorities.load, goals.load, inbox.load = saved
    return {"results": results}


def run(case: str) -> dict[str, Any]:
    data = fixture()
    return {
        "event-clean": event_clean,
        "telemetry-allowlist": telemetry_allowlist,
        "repo-health": repo_health_case,
        "time-saved": time_saved_case,
        "field-totals": field_totals,
        "charts": charts_case,
        "checklist": checklist_case,
    }[case](data)


def main() -> int:
    args = sys.argv[1:]
    if args not in (["--all"],) and not (len(args) == 2 and args[0] == "--case" and args[1] in CASES):
        print("rust24.py: --all | --case <id>", file=sys.stderr)
        return 2
    if not os.environ.get("CAREERBREW_HOME") or not os.environ.get("TMPDIR"):
        print("rust24.py: CAREERBREW_HOME and TMPDIR are required", file=sys.stderr)
        return 2
    os.environ["CAREERBREW_EVENTS"] = "off"
    selected = CASES if args == ["--all"] else (args[1],)
    with contextlib.redirect_stderr(io.StringIO()):
        report = {"cases": [{"id": case, "result": run(case)} for case in selected]}
    sys.stdout.write(compact(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
