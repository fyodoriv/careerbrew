"""rust-10 Python oracle for sessions, claims, agents and reply drafts.

Every input and output is synthetic. The workload uses the production Python
modules as the oracle, invocation-owned homes, fixed clocks and typed JSON.
Random record identities are reported by shape and stability, never by value.
No production route, owner data root, chat tool or network request is used.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import sys
from datetime import date, datetime
from pathlib import Path
from typing import Any

from careerbrew import cli, config
from careerbrew.work import agents, drafts, prior_selection, sessions

CASES = (
    "session-lifecycle",
    "claims-and-drafts",
    "agent-activity",
    "reply-fit",
    "prior-selection",
    "cli-handlers",
)
TARGET = "C0SYN/1700000000.000100"


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
    if hasattr(value, "isoformat"):
        return value.isoformat()
    return {"type": type(value).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> None:
    try:
        value = fn()
    except Exception as exc:
        value = {"error": type(exc).__name__, "message": str(exc)}
    cases.append({"id": name, "result": _typed(value)})


def _error(fn: Any) -> str:
    try:
        fn()
    except Exception as exc:
        return type(exc).__name__
    return "accepted"


def _invoke(argv: list[str]) -> dict[str, Any]:
    out = io.StringIO()
    with contextlib.redirect_stdout(out), contextlib.redirect_stderr(out):
        try:
            code = cli.main(argv)
        except Exception as exc:
            code = 99
            print(type(exc).__name__)
    return {"exit": code, "stdout": out.getvalue()}


class Workspace:
    def __init__(self, base: Path, name: str, projects_: tuple[str, ...] = ()):
        self.base = base / name
        self.root = self.base / "data"
        self.root.mkdir(parents=True)
        (self.base / "home").mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.base / "home"),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "off",
                "CAREERBREW_NOW": "2026-11-02T09:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "TZ": "UTC",
            }
        )
        _invoke(["init"])
        for name_ in projects_:
            _invoke(["work", "project", "add", name_])


def _at(value: str) -> None:
    os.environ["CAREERBREW_NOW"] = value


def _session_lifecycle(base: Path) -> dict[str, Any]:
    Workspace(base, "session-lifecycle", ("billing", "ledger", "search", "resume"))
    config.set_value("work_session_cap", "2")
    first = sessions.open_session("billing", "sess-a1")
    second = sessions.open_session("ledger", "sess-b2")
    identity_shape = len(first["record_id"]) == 32 and all(c in "0123456789abcdef" for c in first["record_id"])
    reused = sessions.open_session("billing")
    duplicate = _error(lambda: sessions.open_session("billing", "sess-other"))
    duplicate_id = _error(lambda: sessions.open_session("search", "sess-a1"))
    unknown = _error(lambda: sessions.open_session("missing", "sess-x"))
    cap = _invoke(["work", "session", "open", "search", "--id", "sess-search"])
    contract = {
        needle: needle in sessions.CONTRACT
        for needle in (
            "runs only the work process",
            "full scope",
            "one at a time, in order",
            "shipped and verified",
            "foreseeable approvals",
            "each scope item marked done or not done",
            "work claim take",
            "work session end --all",
        )
    }
    missing = sessions.end(every=True)
    request = sessions.request_wrap("wrap-1")
    repeat_request = sessions.request_wrap("wrap-1")
    fresh_before = sessions._fresh_handoff(sessions._must("sess-a1", sessions._load()))
    sessions.write_handoff("sess-a1", "SYNTHCANARY fresh after wrap")
    fresh_after = sessions._fresh_handoff(sessions._must("sess-a1", sessions._load()))
    sessions.write_handoff("sess-b2", "ledger complete")
    ended = sessions.end(every=True)
    claims_after_end = sessions.claims()
    _at("2026-11-03T09:00:00+00:00")
    resumed = sessions.open_session("billing", "sess-next")
    older = sessions.open_session("ledger", "sess-ledger-next")
    _at("2026-11-04T09:00:00+00:00")
    no_older_brief = sessions.open_session("billing", "sess-later")
    return {
        "identity": {"shape": identity_shape, "distinct": first["record_id"] != second["record_id"]},
        "opened": [first["id"], second["id"]],
        "reused": {"id": reused["id"], "reused": reused["reused"]},
        "errors": {"duplicate": duplicate, "duplicate_id": duplicate_id, "unknown": unknown},
        "cap": cap,
        "contract": contract,
        "handoff": {"missing": len(missing["missing"]), "wrap": request, "repeat_wrap": repeat_request, "fresh_before": fresh_before, "fresh_after": fresh_after},
        "ended": {"count": len(ended["ended"]), "claims": claims_after_end, "states": [r["state"] for r in sessions.listing(include_ended=True)]},
        "resume": {"same_project": resumed["brief"] == "SYNTHCANARY fresh after wrap", "other_project": older["brief"] == "ledger complete", "older_day_cleared": no_older_brief["brief"] == ""},
    }


class KeepsOld(drafts.LocalBackend):
    def clear(self, target: str) -> None:
        pass


class Trims(drafts.LocalBackend):
    def save(self, target: str, text: str) -> None:
        super().save(target, text[:10])


def _interleaved_model() -> dict[str, int]:
    """One thousand deterministic claim transitions; native threads add the lock control."""
    held: dict[str, str] = {}
    cross_session = stale = conflicting = 0
    for n in range(1000):
        resource = f"slack-draft:C{n % 4}/1.0"
        session = f"sess-{n % 2}"
        owner = held.get(resource)
        if owner is not None and owner != session:
            cross_session += 1
            continue
        held[resource] = session
        if held.get(resource) != session:
            conflicting += 1
        if n % 3 == 0:
            del held[resource]
        if any(not owner for owner in held.values()):
            stale += 1
    return {"operations": 1000, "cross_session_writes": cross_session, "stale_claims": stale, "conflicting_target_saves": conflicting}


def _claims_and_drafts(base: Path) -> dict[str, Any]:
    Workspace(base, "claims-and-drafts", ("billing", "ledger"))
    sessions.open_session("billing", "sess-a")
    sessions.open_session("ledger", "sess-b")
    canonical = sessions.resource("pr:Example/Tools#17")
    taken = sessions.take(canonical, "sess-a")
    held = _error(lambda: sessions.take("pr:example/tools#17", "sess-b"))
    renewed = sessions.take("pr:EXAMPLE/TOOLS#17", "sess-a")["renewed"]
    released = sessions.release("pr:example/tools#17", "sess-a")
    invalid = [_error(lambda value=value: sessions.take(value, "sess-a")) for value in ("pr:tools#17", "pr:example/tools#0", "pr:example/tools#01", "pr:./tools#1", " pr:example/tools#17", "thread with spaces")]
    sessions.take("browser:sso", "sess-a", minutes=30)
    _at("2026-11-02T09:29:00+00:00")
    before_expiry = sessions.holder("browser:sso") is not None
    _at("2026-11-02T09:31:00+00:00")
    expired = sessions.holder("browser:sso") is None
    _at("2026-11-02T09:00:00+00:00")
    saved = drafts.replace(TARGET, "SYNTHCANARY approved\r\n", session="sess-a")
    other = _error(lambda: drafts.replace(TARGET, "SYNTHCANARY other", session="sess-b"))
    without_session = _error(lambda: drafts.replace(TARGET, "SYNTHCANARY other"))
    kept_text = drafts.LocalBackend().read(TARGET)
    sessions.release(drafts.uses_entry(TARGET), "sess-a")
    after_release = drafts.replace(TARGET, "SYNTHCANARY second", session="sess-b")["same"]
    kept = drafts.replace(TARGET, "ignored", if_empty=True, session="sess-b")
    old = drafts.LocalBackend()
    old.save("C1/3.0", "SYNTHCANARY old")
    not_cleared = _error(lambda: drafts.replace("C1/3.0", "SYNTHCANARY new", KeepsOld()))
    mismatch = _error(lambda: drafts.replace("C1/2.0", "This reply is longer than ten characters.", Trims()))
    empty = _error(lambda: drafts.replace("C1/7.0", " \n"))
    bad_targets = [_error(lambda value=value: drafts.check_target(value)) for value in ("", "../etc", "a b", "x" * 200)]
    os.environ["CAREERBREW_ENCRYPT"] = "on"
    drafts.replace("C9/secure.1", "SYNTHCANARY private reply")
    raw = drafts.LocalBackend().path("C9/secure.1").read_bytes()
    return {
        "claim": {"canonical": canonical, "taken": taken["session"], "held": held, "renewed": renewed, "released": released, "invalid": invalid},
        "expiry": {"before": before_expiry, "after": expired, "zero": _error(lambda: sessions.take("browser:sso", "sess-a", minutes=0))},
        "save": {"same": saved["same"], "other": other, "without_session": without_session, "original_kept": kept_text == "SYNTHCANARY approved\r\n", "after_release": after_release, "if_empty": kept["kept"]},
        "failures": {"not_cleared": not_cleared, "mismatch": mismatch, "empty": empty, "bad_targets": bad_targets},
        "privacy": {"sealed": raw.startswith(b"CBV1\n"), "plain_text_absent": b"SYNTHCANARY" not in raw, "digest_name": "C9" not in drafts.LocalBackend().path("C9/secure.1").name},
        "compare": [drafts.compare("a  \n", "a\n\n"), drafts.compare("a\nb", "a\nc")],
        "interleavings": _interleaved_model(),
    }


def _agent_activity(base: Path) -> dict[str, Any]:
    Workspace(base, "agent-activity")
    agents.record("review-bot", "SYNTHCANARY Reviewing\nPR 7", project="billing", url="https://github.example.com/org/shop/pull/7")
    _at("2026-11-02T09:05:00+00:00")
    agents.record("fix-bot", "Fixing the flaky test", project="ledger")
    _at("2026-11-02T09:10:00+00:00")
    agents.record("review-bot", "Left 3 comments", status="done", project="billing")
    agents.record("review-bot", "PR 8", run="pr-8", project="billing")
    view = agents.view()
    _at("2026-11-02T10:11:00+00:00")
    quiet = agents.runs()[0]["status"] == "quiet"
    agents.record("review-bot", "Done again", status="done", run="pr-8")
    agents.record("review-bot", "Started again", run="pr-8", project="billing")
    restarted = next(row for row in agents.runs() if row["run"] == "pr-8")
    invalid = [
        _error(lambda kwargs=kwargs: agents.record(**kwargs))
        for kwargs in (
            {"agent": "a b", "title": "x"},
            {"agent": "bot", "title": " "},
            {"agent": "bot", "title": "x" * 201},
            {"agent": "bot", "title": "x", "status": "paused"},
            {"agent": "bot", "title": "x", "url": "javascript:alert(1)"},
            {"agent": "bot", "title": "x", "project": "p" * 81},
        )
    ]
    _at("2026-11-10T09:00:00+00:00")
    expired = agents.runs()
    agents.record("new-bot", "now")
    retained = [row["agent"] for row in agents.load()]
    return {
        "counts": view["counts"],
        "groups": [group["project"] for group in view["groups"]],
        "review": next(row for row in view["groups"] if row["project"] == "billing")["runs"],
        "quiet": quiet,
        "restart": {"events": restarted["events"], "started_is_last": restarted["started_at"] == restarted["at"]},
        "invalid": invalid,
        "expired": expired,
        "retained": retained,
    }


def _thread(*texts: str, mine_last: bool = False) -> list[dict[str, Any]]:
    rows = [{"ts": f"1700000{n:03d}.000100", "text": text, "mine": False} for n, text in enumerate(texts)]
    if mine_last:
        rows[-1]["mine"] = True
    return drafts.messages({"messages": rows})


def _reply_fit(base: Path) -> dict[str, Any]:
    Workspace(base, "reply-fit")
    started = drafts.fit(_thread("Here is the PR.", "Deploy started, will update when done."), draft="Could you rename SYNTHCANARY before we ship?")
    approve = drafts.fit(_thread("Rolling out to production now."), draft="LGTM, thanks!")
    major = drafts.fit(_thread("Release kicked off."), draft="nit: rename it", major="the migration drops the users table")
    planned = [
        (text, drafts.fit(_thread(text), draft="Could you add a test first?")["kind"])
        for text in ("We will deploy tomorrow.", "Can we release this?", "The release will start after lunch.", "We are about to start the deploy.", "Did the deploy start?", "The fix is not deployed yet.", "Hasn't been released.")
    ]
    rollback = drafts.fit(_thread("Deploy started.", "Rolled back."), draft="Can you add a test?")["kind"]
    restarted = drafts.fit(_thread("Deploy started.", "Rolled back.", "Deploying again with the fix."), draft="Can you add a test?")["kind"]
    emoji = [(text, drafts.fit(_thread("Merged your fix.", text), draft="You're welcome")["emoji"]) for text in ("thanks", "Thanks!", "ty", "ok", "Done.", "awesome", "+1", ":tada:")]
    stale_thread = _thread("Can you look at the PR?", "Never mind, I merged it.", "Deploy started.")
    stale = drafts.fit(stale_thread, draft="Please split the PR in two.", seen=stale_thread[0]["ts"])
    plain = drafts.fit(_thread("Can you look at the PR?", "Also the doc."), draft="Looking now.", seen="1700000000.000100")
    ordered = drafts.fit(drafts.messages({"messages": [{"ts": "2.0", "text": "Deploy started."}, {"ts": "1.0", "text": "Can you add a test?"}]}))["kind"]
    mine = drafts.fit(_thread("Can you add a test?", "Added it.", mine_last=True), draft="ok")["kind"]
    return {
        "started": {key: started[key] for key in ("kind", "deploy_started", "text", "newer", "stale")},
        "approve": {key: approve[key] for key in ("kind", "keep_draft", "text")},
        "major": {"kind": major["kind"], "text": major["text"], "long_error": _error(lambda: drafts.fit(_thread("Release kicked off."), major="x" * 201))},
        "planned": planned,
        "rollback": rollback,
        "restarted": restarted,
        "emoji": emoji,
        "small_draft": drafts.fit(_thread("Here is the doc."), draft="thanks!")["kind"],
        "long_not_small": drafts.small_ack("thanks for the detailed review of the doc"),
        "stale": {key: stale[key] for key in ("kind", "newer", "stale", "newest_ts")},
        "plain_stale": {"kind": plain["kind"], "keep": plain["keep_draft"]},
        "ordered": ordered,
        "mine": mine,
        "empty_thread": _error(lambda: drafts.messages({"messages": []})),
    }


def _prior_selection(base: Path) -> dict[str, Any]:
    Workspace(base, "prior-selection")
    a, b, p, q = "a" * 32, "b" * 32, "1" * 32, "2" * 32

    def row(day: str, record: Any = a, project: Any = p, **extra: Any) -> dict[str, Any]:
        return {"day": day, "record_id": record, "project_incarnation": project, **extra}

    today = date(2026, 11, 7)
    inputs = [
        (today, [row("2026-11-06")], [p]),
        (today, [row("2026-11-07")], [p]),
        (today, [row("2026-11-05")], [p]),
        (today, [row("2026-11-06", ended="yes", handoff="SYNTHCANARY")], [p]),
        (today, [row("2026-11-06", status="done", pending=True)], [p]),
        (today, [{"day": "2026-11-06"}], [p]),
        (today, [row("2026-11-06", project=q)], [p]),
        (today, [row("2026-11-06"), row("2026-11-06", project=q)], [p]),
        (today, [row("2026-02-29")], [p]),
        (date(2024, 3, 1), [row("2024-02-29")], [p]),
        (date(2027, 1, 1), [row("2026-12-31")], [p]),
        (today, [row("2026-11-06")], [p, p]),
        (today, [row("2026-11-06", record="A" * 32)], [p]),
        (today, [row("2026-11-06", project="short")], [p]),
        (today, [None], [p]),
        (today, [row("2026-11-06"), row("2026-11-07")], [p]),
    ]
    results = [prior_selection.select(day, rows, projects_) for day, rows, projects_ in inputs]
    errors = [
        _error(lambda: prior_selection.select(datetime(2026, 11, 7), [], [])),
        _error(lambda: prior_selection.select(today, (), [])),  # type: ignore[reportArgumentType]
        _error(lambda: prior_selection.select(date.min, [], [])),
        _error(lambda: prior_selection.select(today, [], [p, "bad"])),
        _error(lambda: prior_selection.select(today, [None] * 10001, [])),
    ]
    first = [row("2026-11-06", status="waiting", extra={"note": ["SYNTHCANARY"]}), row("2026-11-07", record=b)]
    repeated = prior_selection.select(today, first, [p]) == prior_selection.select(today, first, [p])
    reversed_ = prior_selection.select(today, list(reversed(first)), [p])
    return {"results": results, "errors": errors, "repeat": repeated, "reversed": reversed_}


def _cli_handlers(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "cli-handlers", ("billing", "ledger"))
    sessions.open_session("billing", "sess-a")
    sessions.open_session("ledger", "sess-b")
    agent_empty = _invoke(["work", "agent", "show"])
    agent_event = _invoke(["work", "agent", "event", "review-bot", "SYNTHCANARY Reviewing PR 7", "--project", "billing", "--url", "https://github.example.com/org/shop/pull/7"])
    agent_show = _invoke(["work", "agent", "show"])
    agent_json = _invoke(["work", "agent", "show", "--json"])
    agent_bad = _invoke(["work", "agent", "event", "bot", "x", "--url", "javascript:alert"])
    session_event = _invoke(["work", "agent", "event", "billing-bot", "SYNTHCANARY session step", "--session", "sess-a"])
    session_conflict = _invoke(["work", "agent", "event", "billing-bot", "x", "--session", "sess-a", "--project", "ledger"])
    claim = _invoke(["work", "claim", "take", "pr:Example/Tools#17", "--session", "sess-a"])
    claim_held = _invoke(["work", "claim", "take", "pr:example/tools#17", "--session", "sess-b"])
    claim_list = _invoke(["work", "claim", "list"])
    claim_json = _invoke(["work", "claim", "list", "--json"])
    claim_release = _invoke(["work", "claim", "release", "pr:EXAMPLE/TOOLS#17", "--session", "sess-a"])
    claim_release_twice = _invoke(["work", "claim", "release", "pr:example/tools#17", "--session", "sess-a"])
    thread = ws.base / "thread.json"
    thread.write_text(json.dumps({"messages": [{"ts": "1", "text": "SYNTHCANARY asked for a review"}, {"ts": "2", "text": "Deploy started."}]}))
    change = ws.base / "change.txt"
    change.write_text("Could you change SYNTHCANARY first?")
    fit_json = _invoke(["draft", "fit", "--thread", str(thread), "--draft", str(change), "--seen", "1", "--json"])
    fit_text = _invoke(["draft", "fit", "--thread", str(thread), "--draft", str(change)])
    replaced = _invoke(["draft", "replace", TARGET, "--file", str(change)])
    replaced_json = _invoke(["draft", "replace", TARGET, "--file", str(change), "--json"])
    shown = _invoke(["draft", "show", TARGET])
    missing = _invoke(["draft", "show", "C9/missing.1"])
    sent = ws.base / "sent.txt"
    sent.write_text("Looks good.\n")
    back = ws.base / "back.txt"
    back.write_text("Looks good.  \n\n")
    check_same = _invoke(["draft", "check", "--sent", str(sent), "--read-back", str(back)])
    back.write_text("Please change SYNTHCANARY.")
    check_bad = _invoke(["draft", "check", "--sent", str(sent), "--read-back", str(back)])
    fit_missing = _invoke(["draft", "fit", "--thread", str(ws.base / "missing.json")])
    return {
        "agent": {"empty": agent_empty, "event": agent_event, "show": agent_show, "json": agent_json, "bad": agent_bad, "session_event": session_event, "session_conflict": session_conflict},
        "claim": {"take": claim, "held": claim_held, "list": claim_list, "json": claim_json, "release": claim_release, "release_twice": claim_release_twice},
        "draft": {"fit_json": fit_json, "fit_text": fit_text, "replace": replaced, "replace_json": replaced_json, "show": shown, "missing": missing, "check_same": check_same, "check_bad": check_bad, "fit_missing": fit_missing},
    }


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust10-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "session-lifecycle": _session_lifecycle,
        "claims-and-drafts": _claims_and_drafts,
        "agent-activity": _agent_activity,
        "reply-fit": _reply_fit,
        "prior-selection": _prior_selection,
        "cli-handlers": _cli_handlers,
    }
    cases: list[dict[str, Any]] = []
    for name in CASES:
        if selected is None or name == selected:
            _call(cases, name, lambda fn=functions[name]: fn(base))
    return {"text": json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":")) + "\n"}


def main() -> int:
    args = sys.argv[1:]
    if args == ["--all"]:
        selected = None
    elif len(args) == 2 and args[0] == "--case" and args[1] in CASES:
        selected = args[1]
    else:
        return 2
    sys.stdout.write(run_all(selected)["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
