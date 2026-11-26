"""rust-15 Python oracle for work-day records, questions, stages and sessions.

The native candidate mirrors this synthetic workload. Every data root and every
external effect is synthetic. The report contains no question, handoff or agent
text; restricted projections are checked beside the raw records.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
from datetime import date
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from careerbrew import cli, config, store  # noqa: E402
from careerbrew import events as event_log  # noqa: E402
from careerbrew.work import projects, review_ask, sessions  # noqa: E402
from careerbrew.workday import gate, questions, stages, status, steps, watch  # noqa: E402

CASES = (
    "status-schema",
    "question-records",
    "step-validation",
    "stage-readiness",
    "work-session",
    "legacy-receipts",
)
CANARY = "SYNTHCANARY-rust15-private"


def typed(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": str(value)}
    if isinstance(value, list):
        return [typed(item) for item in value]
    if isinstance(value, dict):
        return {key: typed(item) for key, item in value.items()}
    raise TypeError(type(value))


def compact(value: Any) -> str:
    return json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"


def error(fn) -> str:
    try:
        fn()
    except (ValueError, sessions.SessionError) as exc:
        return type(exc).__name__
    return "accepted"


def invoke(args: list[str], *, json_output: bool = False, show: bool | None = None) -> dict[str, Any]:
    output = io.StringIO()
    original = cli.may_show_business
    if show is not None:
        cli.may_show_business = lambda: show
    try:
        with contextlib.redirect_stdout(output):
            code = cli.main(args)
    finally:
        cli.may_show_business = original
    result: dict[str, Any] = {"exit": code}
    if json_output:
        result["value"] = json.loads(output.getvalue())
    else:
        result["stdout"] = output.getvalue()
    return result


def trace_reset(base: Path) -> None:
    (base / "writes.log").write_text("")


def trace_count(base: Path) -> int:
    path = base / "writes.log"
    return len(path.read_text().splitlines()) if path.exists() else 0


class Workspace:
    def __init__(self, base: Path, name: str, project_names: tuple[str, ...] = ()):
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
            CAREERBREW_NOW="2026-11-02T09:00:00+00:00",
            CAREERBREW_TEST_OWNER="1",
            CAREERBREW_RUST13_WRITE_TRACE=str(self.base / "writes.log"),
            TZ="UTC",
        )
        os.environ.pop("CAREERBREW_TEST_BUSINESS", None)
        with contextlib.redirect_stdout(io.StringIO()):
            cli.main(["init"])
            for project in project_names:
                projects.add(project)
        trace_reset(self.base)


def at(stamp: str) -> None:
    os.environ["CAREERBREW_NOW"] = stamp


def status_case(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "status-schema")
    day = date(2026, 11, 2)
    blank = status.blank("/careerbrew")
    store.write_json(
        status.path_for(day),
        {
            "rows": [{"step": "/careerbrew", "status": "waiting"}],
            "handled": {"pr:example/tools#1": "/careerbrew"},
            "start": {"preflight": [{"check": "synthetic", "ok": True}]},
            "questions": [],
        },
    )
    legacy = status.stored(day)["/careerbrew"]
    names = ["/careerbrew", "/standup"]
    accepted_text = "  Approve\u2003" + "é" * 192 + "  "
    running = status.update("/careerbrew", names, "running", summary=accepted_text, needs_you="Need\u2003a decision", day=day)
    repeated = status.update("/careerbrew", names, "running", day=day)
    summary_only = status.update("/careerbrew", names, summary="Finished \u2003now", day=day)
    too_long = error(lambda: status.update("/careerbrew", names, summary="x" * 201, day=day))
    bad_link = error(lambda: status.update("/careerbrew", names, link="javascript:alert(1)", day=day))
    bad_class = error(lambda: status.update("/careerbrew", names, status="failed", error_class="other", day=day))
    missing_question = error(lambda: status.update("/careerbrew", names, status="blocked", needs_you="", day=day))
    claim = status.update("/standup", names, "done", handled_items=["checkout:~/synthetic"], day=day)
    conflict = error(lambda: status.update("/careerbrew", names, handled_items=["checkout:~/synthetic"], day=day))
    trace_reset(ws.base)
    stale = status.timed_out("/careerbrew", names, "not-the-current-start", day)
    noop_writes = trace_count(ws.base)
    blocked = status.update("/careerbrew", names, "blocked", needs_you="Owner picks", day=day)
    retry_running = status.update("/standup", names, "running", day=day)
    timed = status.timed_out("/standup", names, retry_running["started"], day)
    return {
        "blank": blank,
        "legacy": legacy,
        "running": running,
        "repeated_attempt": repeated["attempt"],
        "summary_only": summary_only,
        "errors": [too_long, bad_link, bad_class, missing_question, conflict],
        "claim": claim,
        "stale_timeout": stale,
        "noop_writes": noop_writes,
        "blocked": blocked,
        "timed": timed,
        "handled": status.handled(day),
        "counts": status.counts(status.rows(names, day)),
        "row_order": [row["step"] for row in store.read_json(status.path_for(day))["rows"]],
        "start_kept": status.start_record(day),
        "sealed": status.path_for(day).read_bytes().startswith(b"CBV1\n"),
        "plain_absent": CANARY.encode() not in status.path_for(day).read_bytes(),
    }


def question_records(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "question-records")
    at("2026-11-01T07:00:00+00:00")
    first = questions.add("/careerbrew", "  Approve\u2003" + CANARY + "?  ", ["merge", "merge", "wait", ""], "merge", "true")
    at("2026-11-02T09:00:00+00:00")
    second = questions.add("/careerbrew", "Approve " + CANARY + "?", ["merge", "wait"], "wait", "true")
    trace_reset(ws.base)
    third = questions.add("/careerbrew", "Approve " + CANARY + "?", ["merge", "wait"], "wait", "true")
    repeat_writes = trace_count(ws.base)
    invalid = [
        error(lambda: questions.record("/careerbrew", "x" * 201)),
        error(lambda: questions.record("/careerbrew", "x", ["x" * 81])),
        error(lambda: questions.record("/careerbrew", "x", [str(n) for n in range(7)])),
        error(lambda: questions.record("/careerbrew", "x", ["a"], "b")),
        error(lambda: questions.record("/careerbrew", "x", source="unknown")),
        error(lambda: questions.check_recheck("sh -c 'open")),
        error(lambda: questions.check_recheck("x" * 501)),
        error(lambda: questions.check_recheck("https://example.com/a b")),
    ]
    public = questions.public({**second, "age": "26 h", "state": "open", "why": "still open"})
    plugin_public = questions.public({**second, "source": "plugin:private-name", "recheck": "careerbrew:review-pr https://github.com/example/tools/pull/7"})
    stale_expected = {**second, "text": "stale writer", "day": "2026-11-01", "age": "26 h"}
    stale_close = questions.close(second["id"], "stale writer", date(2026, 11, 1), expected=stale_expected)
    changed = questions.records(date(2026, 11, 1))[0]
    expected = {**changed, "day": "2026-11-01", "age": "26 h"}
    actual_close = questions.close(second["id"], "answered", date(2026, 11, 1), expected=expected)
    at("2026-11-01T06:00:00+00:00")
    questions.add("/careerbrew", "open command", recheck="true")
    questions.add("/careerbrew", "closed command", recheck="false")
    at("2026-11-01T08:30:00+00:00")
    questions.add("/careerbrew", "agent link", recheck="https://example.com/pr/1")
    questions.add("/careerbrew", "manual close")
    at("2026-11-02T09:00:00+00:00")
    batch = questions.recheck(date(2026, 11, 2))
    signal_receipt = gate.run_verify("sh -c 'kill -TERM $$'")
    old_records = questions.records(date(2026, 11, 1))
    current_records = questions.records(date(2026, 11, 2))
    return {
        "first": questions.public(first),
        "second": questions.public(second),
        "third_same": third == second,
        "repeat_writes": repeat_writes,
        "invalid": invalid,
        "public": public,
        "plugin_public": plugin_public,
        "close": [stale_close, actual_close],
        "batch": batch,
        "signal_receipt": list(signal_receipt),
        "old_records": [questions.public(row) for row in old_records],
        "current_records": [questions.public(row) for row in current_records],
        "ages": [questions.age("bad"), questions.age("2026-11-01T08:45:00+00:00"), questions.age("2026-10-30T08:00:00+00:00")],
        "sealed": status.path_for(date(2026, 11, 1)).read_bytes().startswith(b"CBV1\n"),
        "plain_absent": CANARY.encode() not in status.path_for(date(2026, 11, 1)).read_bytes(),
    }


def step_validation(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "step-validation")
    rich = steps.entry(
        {
            "step": "standup",
            "scope": ["repo", "repo", "repo/web"],
            "after": "/refresh",
            "longRunning": False,
            "operatorTurn": True,
            "uses": ["browser:sso", "file:result"],
            "verify": ".venv/bin/python -m unittest",
            "cycle": "careerbrew work live cycle",
            "review_ask_cycle": "careerbrew work review-ask https://github.com/example/tools/pull/7",
            "watch": ["repo:tools", "feed:chat"],
            "about": "  Draft today's standup from synthetic work  ",
        },
        "settings",
    )
    defaults = steps.entry("/standup", "settings")
    verify_errors = [
        error(lambda value=value: steps.entry({"step": "/x", "verify": value}, "settings"))
        for value in (
            "python3 -c 'pass'",
            "env FOO=1 python3.12 -c 'pass'",
            "sh -c 'python3 -c pass'",
            "sh -c 'open",
            "",
            "x" * 501,
        )
    ]
    other_errors = [
        error(lambda: steps.normalize("/a:..:x")),
        error(lambda: steps.entry({"step": "/x", "use": "file:x"}, "settings")),
        error(lambda: steps.entry({"step": "/x", "uses": "printer:one"}, "settings")),
        error(lambda: steps.entry({"step": "/x", "scope": "x" * 201}, "settings")),
        error(lambda: steps.entry({"step": "/x", "operatorTurn": True, "longRunning": True}, "settings")),
        error(lambda: steps.entry({"step": "/x", "about": "one\ntwo"}, "settings")),
        error(lambda: steps.entry({"step": "/x", "about": " ".join(["word"] * 16)}, "settings")),
        error(lambda: steps.entry({"step": "/x", "watch": "url:https://example.com"}, "settings")),
        error(lambda: steps.validate({"step": "/x"}, "settings")),
        error(lambda: steps.validate(["/x"] * 31, "settings")),
    ]
    commands = ws.base / "commands"
    commands.mkdir()
    (commands / "standup.md").write_text('---\ndescription: "Draft the standup. It reads yesterday."\ndisable-model-invocation: true\n---\nbody\n')
    (commands / "plain.md").write_text("no front matter\n")
    resolved = steps.resolve("/standup", roots=[ws.base])
    canonical_raw = steps.resolve("/work")
    canonical = {"description": canonical_raw["description"], "user_only": canonical_raw["user_only"], "present": canonical_raw["path"] is not None, "source_error": canonical_raw.get("source_error", "")}
    plain_raw = steps.resolve("/plain", roots=[ws.base])
    plain = {"description": plain_raw["description"], "user_only": plain_raw["user_only"], "present": plain_raw["path"] is not None, "source_error": plain_raw.get("source_error", "")}
    return {
        "rich": rich,
        "defaults": defaults,
        "verify_errors": verify_errors,
        "other_errors": other_errors,
        "resolved": {"description": resolved["description"], "user_only": resolved["user_only"], "present": resolved["path"] is not None, "source_error": resolved.get("source_error", "")},
        "canonical": canonical,
        "plain": plain,
        "about": [steps.about({"about": "Own words"}, resolved), steps.about({"about": ""}, resolved), steps.missing_about([{"step": "/a", "about": ""}, {"step": "/b", "about": "ok"}])],
        "first_sentences": [steps.first_sentence(" 'One sentence. Next sentence.' "), steps.first_sentence("No punctuation"), steps.first_sentence("x" * 200)],
    }


def stage_readiness(base: Path) -> dict[str, Any]:
    Workspace(base, "stage-readiness")
    declared = [
        steps.entry("/careerbrew", "settings"),
        steps.entry({"step": "/z-operator", "operatorTurn": True, "uses": "browser:z"}, "settings"),
        steps.entry({"step": "/a-operator", "operatorTurn": True, "uses": "browser:a"}, "settings"),
        steps.entry({"step": "/dispatch", "operatorTurn": True, "uses": "browser:sso"}, "settings"),
        steps.entry({"step": "/feed", "longRunning": True, "uses": "browser:feed"}, "settings"),
        steps.entry({"step": "/check", "uses": "browser:feed"}, "settings"),
        steps.entry({"step": "/report", "scope": "report", "after": "/check"}, "settings"),
        steps.entry({"step": "/other", "scope": "other"}, "settings"),
    ]
    found, view = stages.build(declared, late={"/dispatch"})
    initial = stages.ready(found, [{"step": "/careerbrew", "status": "done"}])
    running = stages.ready(found, [{"step": "/careerbrew", "status": "done"}, {"step": "/check", "status": "running"}])
    checked = stages.ready(found, [{"step": "/careerbrew", "status": "done"}, {"step": "/check", "status": "done"}])
    failed = stages.ready(found, [{"step": "/careerbrew", "status": "done"}, {"step": "/check", "status": "failed"}])
    blocked = stages.ready(found, [{"step": "/careerbrew", "status": "done"}, {"step": "/check", "status": "blocked"}])
    errors = [
        error(lambda: stages.build([steps.entry({"step": "/a", "after": "/b"}, "settings"), steps.entry({"step": "/b", "after": "/a"}, "settings")])),
        error(lambda: stages.build([steps.entry({"step": "/feed", "longRunning": True}, "settings"), steps.entry({"step": "/a", "after": "/feed"}, "settings")])),
        error(lambda: stages.build([steps.entry({"step": "/f1", "longRunning": True, "uses": "browser:sso"}, "settings"), steps.entry({"step": "/f2", "longRunning": True, "uses": "browser:sso"}, "settings")])),
        error(lambda: stages.build([steps.entry({"step": "/check", "scope": "check"}, "settings"), steps.entry({"step": "/dispatch", "operatorTurn": True, "after": "/check"}, "settings")])),
    ]
    return {"found": found, "view": view, "ready": {"initial": initial, "running": running, "checked": checked, "failed": failed, "blocked": blocked}, "errors": errors}


def work_session(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "work-session", ("billing", "ledger", "search"))
    config.set_value("work_session_cap", "2")
    trace_reset(ws.base)
    opened = invoke(["work", "session", "open", "billing", "--id", "sess-a", "--json"], json_output=True)
    ledger = invoke(["work", "session", "open", "ledger", "--id", "sess-b", "--json"], json_output=True)
    cap = invoke(["work", "session", "open", "search", "--id", "sess-c"])
    trace_reset(ws.base)
    reused = invoke(["work", "session", "open", "billing", "--id", "sess-a", "--json"], json_output=True)
    reuse_writes = trace_count(ws.base)
    claim = sessions.take("slack-draft:C0SYN/1.0", "sess-a")
    asked = invoke(["work", "session", "ask", "sess-a", CANARY, "--option", "send", "--option", "hold", "--pick", "hold", "--recheck", "true"])
    activity = invoke(["work", "agent", "event", "billing-bot", CANARY, "--session", "sess-a"])
    listing = sessions.listing()
    public = sessions.public(listing[1] if listing[0]["id"] != "sess-a" else listing[0])
    listed = invoke(["work", "session", "list", "--json"], json_output=True)
    listed_text = invoke(["work", "session", "list"])
    missing = invoke(["work", "session", "end", "--all", "--json"], json_output=True)
    handoff = invoke(["work", "session", "handoff", "sess-a", CANARY])
    handoff_b = invoke(["work", "session", "handoff", "sess-b", "ledger complete"])
    ended = invoke(["work", "session", "end", "--all", "--json"], json_output=True)
    at("2026-11-03T09:00:00+00:00")
    reopened = invoke(["work", "session", "open", "billing", "--id", "sess-a", "--json"], json_output=True)
    os.environ["CAREERBREW_EVENTS"] = "on"
    original_write_row = event_log.write_row

    def traced_write_row(row):
        original_write_row(row)
        with (ws.base / "writes.log").open("a") as trace:
            trace.write(f"{ws.root / 'logs/events.jsonl'}\n")

    event_log.write_row = traced_write_row
    try:
        event_session = invoke(["work", "session", "open", "ledger", "--id", "sess-events", "--json"], json_output=True)
        error_event = invoke(["work", "session", "open", "search", "--id", "sess-cap"])
    finally:
        event_log.write_row = original_write_row
    event_names = [json.loads(line)["event"] for line in (ws.root / "logs/events.jsonl").read_text().splitlines()[-2:]]
    error_event_statuses = [json.loads(line)["status"] for line in (ws.root / "logs/events.jsonl").read_text().splitlines()[-2:]]
    os.environ["CAREERBREW_EVENTS"] = "off"
    raw = (ws.root / "work/sessions.json").read_bytes()
    return {
        "opened": opened,
        "ledger": ledger,
        "cap": cap,
        "reused": reused,
        "reuse_writes": reuse_writes,
        "raw_claim": claim,
        "asked": asked,
        "activity": activity,
        "public": public,
        "listed": listed,
        "listed_text": listed_text,
        "missing": missing,
        "handoff": handoff,
        "handoff_b": handoff_b,
        "ended": ended,
        "reopened": reopened,
        "event_session": event_session,
        "error_event": error_event,
        "event_names": event_names,
        "error_event_statuses": error_event_statuses,
        "claims_after_end": sessions.claims(),
        "sealed": raw.startswith(b"CBV1\n"),
        "plain_absent": CANARY.encode() not in raw,
    }


def legacy_receipts(base: Path) -> dict[str, Any]:
    Workspace(base, "legacy-receipts")
    day = date(2026, 11, 2)
    store.write_json(status.path_for(day), {"rows": "not-a-list", "questions": "not-a-list"})
    malformed_status = status.stored(day)
    malformed_questions = questions.records(day)
    store.write_json(
        status.path_for(day),
        {
            "rows": [],
            "questions": [
                {"id": "same", "step": "/careerbrew", "text": "old", "createdAt": "2026-11-01T08:00:00+00:00", "recheck": "", "closed": ""},
                {"id": "same", "step": "/careerbrew", "text": "new", "createdAt": "2026-11-02T08:00:00+00:00", "recheck": "", "closed": "answered"},
            ],
        },
    )
    open_rows = questions.open_records(day)
    at("2026-11-02T09:00:00+00:00")
    rec = questions.record("/careerbrew", "review?", recheck="careerbrew:review-pr https://github.com/example/tools/pull/7", ident="review")
    questions.save(rec, day)
    original_recheck = review_ask.recheck_pr

    def changed_review(_url: str, **_kwargs):
        questions._change(day, lambda rows: rows[-1].update(text="changed underneath") if rows and rows[-1]["id"] == "review" else None)
        return "closed", "review merged"

    review_ask.recheck_pr = changed_review
    try:
        review_batch = questions.recheck(day)
    finally:
        review_ask.recheck_pr = original_recheck
    review_open = [q for q in questions.records(day) if q["id"] == "review" and not q.get("closed")]
    projects.add("billing")
    sessions.open_session("billing", "sess-old")
    raw = store.read_json(sessions.path(), {})
    raw["claims"] = [{"resource": "browser:sso", "session": "sess-old", "until": "bad"}]
    store.write_json(sessions.path(), raw)
    before = sessions.path().read_bytes()
    invalid_claim = error(lambda: sessions.listing())
    unchanged = before == sessions.path().read_bytes()

    Workspace(base, "legacy-session-brief", ("billing",))
    at("2026-11-01T09:00:00+00:00")
    sessions.open_session("billing", "legacy")
    sessions.write_handoff("legacy", "prior private handoff")
    sessions.end("legacy")
    saved = store.read_json(sessions.path(), {})
    saved["sessions"][-1].pop("record_id")
    saved["sessions"][-1].pop("project_incarnation")
    store.write_json(sessions.path(), saved)
    at("2026-11-02T09:00:00+00:00")
    legacy_open = invoke(["work", "session", "open", "billing", "--id", "new", "--json"], json_output=True)
    sessions.write_handoff("new", "current private handoff")
    saved = store.read_json(sessions.path(), {})
    saved["sessions"][-1].pop("record_id")
    saved["sessions"][-1].pop("project_incarnation")
    store.write_json(sessions.path(), saved)
    legacy_end = invoke(["work", "session", "end", "new", "--json"], json_output=True)
    legacy_loops = [row["id"] for row in watch.status()["running"]]

    Workspace(base, "quiet-session-receipt", ("billing",))
    at("2026-10-30T09:00:00+00:00")
    sessions.open_session("billing", "old")
    sessions.write_handoff("old", "prior private handoff")
    sessions.end("old")
    generation = "a" * 32
    project_data = store.read_yaml(projects.path(), {})
    project_data["projects"]["billing"]["quiet_until"] = "2026-11-01"
    project_data["projects"]["billing"]["quiet_generation"] = generation
    store.write_yaml(projects.path(), project_data)
    at("2026-11-02T09:00:00+00:00")
    quiet_open = sessions.open_session("billing", "resumed")
    trace_reset(base / "quiet-session-receipt")
    quiet_reused = sessions.open_session("billing", "resumed")
    quiet_reuse_writes = trace_count(base / "quiet-session-receipt")
    quiet_receipt = generation in store.read_json(sessions.path(), {})["quiet_receipts"]
    return {
        "malformed_status": malformed_status,
        "malformed_questions": malformed_questions,
        "open_rows": open_rows,
        "review_batch": review_batch,
        "review_still_open": review_open,
        "invalid_claim": invalid_claim,
        "unchanged": unchanged,
        "legacy_open": legacy_open,
        "legacy_end": legacy_end,
        "legacy_loops": legacy_loops,
        "quiet_open": {"id": quiet_open["id"], "brief": quiet_open["brief"], "brief_status": quiet_open.get("brief_status", "")},
        "quiet_reused": quiet_reused["brief"],
        "quiet_receipt": quiet_receipt,
        "quiet_reuse_writes": quiet_reuse_writes,
    }


def run(case: str, base: Path) -> dict[str, Any]:
    return {
        "status-schema": status_case,
        "question-records": question_records,
        "step-validation": step_validation,
        "stage-readiness": stage_readiness,
        "work-session": work_session,
        "legacy-receipts": legacy_receipts,
    }[case](base)


def main() -> int:
    args = sys.argv[1:]
    if args not in (["--all"],) and not (len(args) == 2 and args[0] == "--case" and args[1] in CASES):
        print("rust15.py: --all | --case <id>", file=sys.stderr)
        return 2
    selected = CASES if args == ["--all"] else (args[1],)
    root = os.environ.get("CAREERBREW_HOME", "")
    if not root:
        print("rust15.py: CAREERBREW_HOME is required", file=sys.stderr)
        return 2
    base = Path(root) / "rust15-workload"
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
