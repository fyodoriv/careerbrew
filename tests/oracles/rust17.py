"""rust-17 Python oracle for day results, handoff and sent-PM learning.

The native candidate mirrors this synthetic workload. Every data root, feed row
and Slack identity is synthetic. Sent-Tomorrow promise writes stay behind the
native `PromiseRecorder` seam; these cases send no Tomorrow bullets, so the
Python promise writer only takes its project-file lock.
"""

from __future__ import annotations

import contextlib
import difflib
import io
import json
import os
import re
import sys
from datetime import date
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from careerbrew import cli, store  # noqa: E402
from careerbrew.day import handoff, pm_learn, results  # noqa: E402

CASES = (
    "difflib-vectors",
    "results-records",
    "handoff-records",
    "pm-sent-receipts",
    "pm-lessons",
)
CANARY = "SYNTHCANARY-rust17-private"


def typed(value: Any) -> Any:
    if value is None or isinstance(value, (str, bool)):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": str(value)}
    if isinstance(value, (list, tuple)):
        return [typed(item) for item in value]
    if isinstance(value, dict):
        return {key: typed(item) for key, item in value.items()}
    raise TypeError(type(value))


def compact(value: Any) -> str:
    return json.dumps(typed(value), ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n"


def error(fn) -> str:
    try:
        fn()
    except ValueError as exc:
        return type(exc).__name__
    return "accepted"


def trace_reset(base: Path) -> None:
    (base / "writes.log").write_text("")


def trace_count(base: Path) -> int:
    path = base / "writes.log"
    return len(path.read_text().splitlines()) if path.exists() else 0


def plant() -> str:
    return os.environ.get("CAREERBREW_RUST17_PLANT_WRITE", "")


class Workspace:
    def __init__(self, base: Path, name: str):
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
        trace_reset(self.base)


def set_setting(key: str, value: Any) -> None:
    from careerbrew import paths

    path = paths.data_root() / "settings.yaml"
    data = store.read_yaml(path, {}) or {}
    data[key] = value
    store.write_yaml(path, data)


def at(stamp: str) -> None:
    os.environ["CAREERBREW_NOW"] = stamp


def sealed(path: Path) -> bool:
    return path.read_bytes().startswith(b"CBV1\n")


def plain_absent(path: Path) -> bool:
    return CANARY.encode() not in path.read_bytes()


LONG_A = "the quick brown fox jumps over the lazy dog " * 6
LONG_B = "the quick brown cat jumps over the lazy dog " * 5 + "and then naps"


def difflib_vectors(_base: Path) -> dict[str, Any]:
    pairs = [
        ("", ""),
        ("abc", ""),
        ("Billing: shipped invoices", "Billing: shipped the invoices"),
        ("Search: tuned ranking", "Discovery: tuned ranking"),
        ("abcd", "bcda"),
        ("éa b", "a bé"),
        (LONG_A, LONG_B),
        (LONG_B, LONG_A),
        ("x" * 250, "x" * 120 + "y" + "x" * 129),
    ]
    ratios = [difflib.SequenceMatcher(None, a, b).ratio() for a, b in pairs]
    blocks = [[list(block) for block in difflib.SequenceMatcher(None, a, b).get_matching_blocks()] for a, b in pairs]
    shown = ["a", "b", "c", "d", "e"]
    saved = ["a", "x", "c", "e", "f", "g"]
    opcodes = [list(op) for op in difflib.SequenceMatcher(a=shown, b=saved, autojunk=False).get_opcodes()]
    lines = ["one", "two", "three", "two"]
    more = ["two", "one", "two", "four"]
    opcodes_b = [list(op) for op in difflib.SequenceMatcher(a=lines, b=more, autojunk=False).get_opcodes()]
    return {"ratios": ratios, "blocks": blocks, "opcodes": opcodes, "opcodes_b": opcodes_b}


def results_records(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "results-records")
    day = date(2026, 11, 2)
    first = [
        {"id": "pr:example/tools#1", "at": "2026-11-02T08:00:00+00:00", "kind": "pr", "title": "Fix login " + CANARY, "source": "github", "origin": "user"},
        {"id": "review:example/tools#2", "at": "2026-11-02T08:10:00+00:00", "kind": "review", "title": "Review search", "source": "github", "origin": "user"},
        {"id": "review:example/tools#2", "at": "2026-11-02T08:11:00+00:00", "kind": "review", "title": "Review search v2", "source": "github", "origin": "user"},
    ]
    added = results.append(first, day)
    trace_reset(ws.base)
    repeat = results.append(first, day)
    repeat_writes = trace_count(ws.base)
    moved_at = results.append([{**first[0], "at": "2026-11-02T08:30:00+00:00"}], day)
    corrected = results.append([{**first[0], "title": "Fix login flow " + CANARY}, {"id": "note:1", "at": "2026-11-02T08:40:00+00:00", "kind": "note", "title": "Wrote notes", "source": "manual", "origin": "user"}], day)
    draft = results.append([{**first[1], "title": "Review search v2", "draft": True}], day)
    loaded = [{key: row[key] for key in sorted(row) if key != "title"} for row in results.load(day)]
    items = [
        {"id": "e1", "at": "2026-11-02T10:00:00Z", "kind": "pr", "title": "Zulu", "draft": False},
        {"id": "e2", "at": "2026-11-02T23:30:00-05:00", "title": "Next day in UTC"},
        {"id": "e3", "at": "2026-11-02T07:00:00", "kind": "note", "origin": "agent", "state": "open", "merged_at": "", "feed_source": "slack"},
        {"id": "e4", "at": "2026-11-01T23:59:59+00:00", "kind": "pr"},
        {"id": "e5", "at": "not-a-date", "kind": "pr"},
        {"id": "e6", "at": "2026-11-02", "kind": "pr"},
        {"id": "e7", "kind": "pr"},
    ]
    evidence = results.evidence_rows(items, day)
    if plant() == "extra":
        (ws.root / "planted-extra.txt").write_text("synthetic")
    path = results.path_for(day)
    return {
        "added": added,
        "repeat": repeat,
        "repeat_writes": repeat_writes,
        "moved_at": moved_at,
        "corrected": corrected,
        "draft": draft,
        "loaded": loaded,
        "titles_plain": [row["title"].replace(CANARY, "<canary>") for row in results.load(day)],
        "summary": results.summary(day),
        "empty_summary": results.summary(date(2026, 11, 3)),
        "evidence": evidence,
        "sealed": sealed(path),
        "plain_absent": plain_absent(path),
    }


def handoff_records(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "handoff-records")
    day = date(2026, 11, 2)
    errors = [
        error(lambda: handoff.item("unknown", "text")),
        error(lambda: handoff.item("prs", "    ")),
        error(lambda: handoff.item("prs", "x" * 201)),
    ]
    edge = handoff.item("prs", "é" * 200)
    extra = handoff.item("questions", "  Ask   the team  ", "https://example.com/q", recheck="true", empty="", fixed=None)
    first = handoff.add("prs", "Review " + CANARY, "https://github.com/example/tools/pull/1", day)
    trace_reset(ws.base)
    again = handoff.add("prs", "Review " + CANARY, "https://github.com/example/tools/pull/1", day)
    repeat_writes = trace_count(ws.base)
    second = handoff.add("waiting", "Waiting on design", day=day)
    owned = [handoff.item("lessons", "Rule one"), handoff.item("gotchas", "Gotcha two")]
    handoff.replace_owned("wrap", owned, day)
    trace_reset(ws.base)
    handoff.replace_owned("wrap", owned, day)
    owned_repeat_writes = trace_count(ws.base)
    handoff.replace_owned("wrap", [handoff.item("lessons", "Rule one"), handoff.item("waiting", "Waiting on design")], day)
    after_replace = [{"id": row["id"], "section": row["section"], "owner": row.get("owner", "")} for row in handoff.load(day)]
    done_known = handoff.done(second["id"], day)
    done_missing = handoff.done("h-missing", day)
    if plant() == "operation-duplicate":
        store.write_json(handoff.path_for(day), store.read_json(handoff.path_for(day)))
    handoff.add("calendar", "Standup 09:30", day=date(2026, 10, 30))
    handoff.add("calendar", "Empty later", day=date(2026, 10, 31))
    handoff.done(handoff.item("calendar", "Empty later")["id"], date(2026, 10, 31))
    found_day, found = handoff.latest_before(date(2026, 11, 2))
    later_day, later = handoff.latest_before(date(2026, 11, 3))
    none_day, none = handoff.latest_before(date(2026, 10, 30))
    path = handoff.path_for(day)
    return {
        "errors": errors,
        "edge_len": len(edge["text"]),
        "extra": extra,
        "first_id": first["id"],
        "again_same": again == first,
        "repeat_writes": repeat_writes,
        "second": second,
        "owned_repeat_writes": owned_repeat_writes,
        "after_replace": after_replace,
        "done": [done_known, done_missing],
        "latest": [found_day.isoformat() if found_day else None, [row["id"] for row in found]],
        "latest_later": [later_day.isoformat() if later_day else None, len(later)],
        "latest_none": [none_day, none],
        "sealed": sealed(path),
        "plain_absent": plain_absent(path),
    }


def feed_rows(events: list[dict[str, Any]], originals: list[dict[str, Any]]) -> None:
    from careerbrew import paths

    store.write_jsonl(paths.sub("work", "feed", "events.jsonl"), events)
    store.write_jsonl(paths.sub("work", "feed", "originals.jsonl"), originals)


def history_view() -> list[list[str]]:
    return [[row["day"], row["from"], row["body"].replace(CANARY, "<canary>")] for row in store.read_jsonl(pm_learn.path())]


def files_present(days: list[str]) -> dict[str, list[bool]]:
    return {day: [pm_learn.sent_path(day).exists(), pm_learn.receipt_path(day).exists(), pm_learn._outcome_path(day).exists()] for day in days}


def pm_sent_receipts(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "pm-sent-receipts")
    manual_day, receipt_day, invalid_day, channel_day, dm_day, none_day = (
        "2026-10-26",
        "2026-10-27",
        "2026-10-28",
        "2026-10-29",
        "2026-10-30",
        "2026-10-31",
    )
    draft = "Billing: shipped invoices.\nSearch: tuned ranking."
    for day in (manual_day, receipt_day, invalid_day, channel_day, dm_day, none_day):
        pm_learn.remember(day, draft, draft)
    feed_rows(
        [
            {"id": "slack:C0PM:100.1", "source": "slack", "actor": "me", "channel": "pm-updates", "at": "2026-10-26T15:00:00+00:00"},
            {"id": "slack:C0SYN:200.5", "source": "slack", "actor": "me", "channel": "dm", "at": "2026-10-27T15:00:00+00:00"},
            {"id": "slack:C0PM:300.1", "source": "slack", "actor": "me", "channel": "PM-Updates", "at": "2026-10-29T14:00:00Z"},
            {"id": "slack:C0PM:300.2", "source": "slack", "actor": "me", "channel": "pm-updates", "at": "2026-10-29T16:00:00+00:00"},
            {"id": "slack:C0PM:300.3", "source": "slack", "actor": "other", "channel": "pm-updates", "at": "2026-10-29T16:30:00+00:00"},
            {"id": "slack:C0PM:300.4", "source": "slack", "actor": "me", "channel": "pm-updates", "personal": True, "at": "2026-10-29T16:40:00+00:00"},
            {"id": "slack:C0PM:300.5", "source": "slack", "actor": "me", "channel": "random", "at": "2026-10-29T16:50:00+00:00"},
            {"id": "slack:D0PM:400.1", "source": "slack", "actor": "me", "at": "2026-10-30T15:00:00+00:00"},
            {"id": "slack:D0XX:400.2", "source": "slack", "actor": "me", "at": "2026-10-30T15:10:00+00:00"},
            {"id": "slack:G0MP:400.3", "source": "slack", "actor": "me", "at": "2026-10-30T15:20:00+00:00"},
            {"id": "github:example/tools#9", "source": "github", "actor": "me", "at": "2026-10-30T15:30:00+00:00"},
        ],
        [
            {"id": "slack:C0PM:100.1", "text": "Billing: shipped invoices.\nSearch: from the channel."},
            {"id": "slack:C0SYN:200.5", "text": "Billing: shipped invoices (edited " + CANARY + ").\nSearch: tuned ranking."},
            {"id": "slack:C0PM:300.1", "text": "Lunch plans for the team?"},
            {"id": "slack:C0PM:300.2", "text": "Billing: shipped invoices today.\nSearch: tuned ranking more."},
            {"id": "slack:C0PM:300.3", "text": draft},
            {"id": "slack:C0PM:300.4", "text": draft},
            {"id": "slack:C0PM:300.5", "text": draft},
            {"id": "slack:D0PM:400.1", "text": "Billing: shipped invoices.\nSearch: DM to the right person.", "original": {"channel": {"is_im": True, "name": "pm-person"}}},
            {"id": "slack:D0XX:400.2", "text": draft, "original": {"channel": {"is_im": True, "name": "other-person"}}},
            {"id": "slack:G0MP:400.3", "text": draft, "original": {"channel": {"is_im": True, "is_mpim": True, "name": "pm-person"}}},
            {"id": "github:example/tools#9", "text": draft},
        ],
    )
    trace_reset(ws.base)
    no_target = [pm_learn.pm_sent_text(channel_day), pm_learn.pm_sent_text(dm_day)]
    no_target_writes = trace_count(ws.base)
    set_setting("pm_slack", "#pm-updates")
    pm_learn.save_sent(manual_day, "  Manual " + CANARY + " update\n\n")
    manual = pm_learn.pm_sent_text(manual_day)
    pm_learn.save_sent(receipt_day, "Billing: shipped invoices.", receipt={"channel": "C0SYN", "ts": "200.5"})
    receipt_id = store.read_json(pm_learn.receipt_path(receipt_day))
    receipt_text = pm_learn.pm_sent_text(receipt_day)
    learned_receipt = pm_learn.learn_sent(receipt_day)
    trace_reset(ws.base)
    learned_again = pm_learn.learn_sent(receipt_day)
    idle_writes = trace_count(ws.base)
    revision_ok = bool(re.fullmatch(r"[0-9a-f]{32}", store.read_json(pm_learn._outcome_path(receipt_day))["revision"]))
    pm_learn.save_sent(invalid_day, "Invalid receipt text", receipt={"channel": "c0lower", "ts": "1.2"})
    invalid = pm_learn.pm_sent_text(invalid_day)
    channel = pm_learn.pm_sent_text(channel_day)
    learned_channel = pm_learn.learn_sent(channel_day)
    set_setting("pm_slack", "pm-person")
    dm = pm_learn.pm_sent_text(dm_day)
    learned_dm = pm_learn.learn_sent(dm_day)
    none = pm_learn.pm_sent_text(none_day)
    learned_none = pm_learn.learn_sent(none_day)
    store.write_json(pm_learn.receipt_path(manual_day), {"id": "slack:bad"})
    malformed = pm_learn.pm_sent_text(manual_day)
    pm_learn.save_sent(receipt_day, "Billing: manual correction.")
    after_manual = pm_learn.pm_sent_text(receipt_day)
    pm_learn.save_sent(invalid_day, "   ")
    cleared = pm_learn.pm_sent_text(invalid_day)
    days = [manual_day, receipt_day, invalid_day, channel_day, dm_day, none_day]
    return {
        "no_target": no_target,
        "no_target_writes": no_target_writes,
        "manual": (manual or "").replace(CANARY, "<canary>"),
        "receipt_id": receipt_id,
        "receipt_text": (receipt_text or "").replace(CANARY, "<canary>"),
        "learned": [learned_receipt, learned_again, learned_channel, learned_dm, learned_none],
        "idle_writes": idle_writes,
        "revision_ok": revision_ok,
        "invalid": invalid,
        "channel": channel,
        "dm": dm,
        "none": none,
        "malformed": (malformed or "").replace(CANARY, "<canary>"),
        "after_manual": after_manual,
        "cleared": cleared,
        "files": files_present(days),
        "history": history_view(),
        "locks": sorted(p.name for p in (ws.root / "day").glob(".pm-sent-*.lock")),
        "sealed": sealed(pm_learn.sent_path(receipt_day)),
        "plain_absent": plain_absent(pm_learn.path()),
    }


RAW = "Billing: shipped invoices; next: tax export.\nSearch: tuned ranking.\nOps: status check: no action needed.\nLedger: fixed totals; overdue: audit."
BODY = "Discovery: tuned ranking.\nBilling: shipped invoices.\nLedger: fixed totals; overdue: audit.\nLed the planning meeting with design.\nPayments: In draft"


def pm_lessons(base: Path) -> dict[str, Any]:
    ws = Workspace(base, "pm-lessons")
    at("2026-10-20T18:00:00+00:00")
    pm_learn.remember("2026-10-20", RAW, RAW)
    pm_learn.record("2026-10-20", BODY + "\n" + CANARY)
    after_one = pm_learn.rules()
    at("2026-10-21T18:00:00+00:00")
    pm_learn.remember("2026-10-21", RAW, RAW)
    pm_learn.record("2026-10-21", BODY)
    after_two = pm_learn.rules()
    pm_learn.remember("2026-10-22", RAW, RAW)
    trace_reset(ws.base)
    pm_learn.record("2026-10-22", RAW)
    unchanged_writes = trace_count(ws.base)
    pm_learn.record("2026-10-22", "")
    pm_learn.remember("2026-10-23", "", "")
    pm_learn.record("2026-10-23", "anything")
    pm_learn.remember("2026-10-24", RAW, RAW)
    pm_learn.record("2026-10-24", BODY, source="sent")
    trace_reset(ws.base)
    pm_learn.record("2026-10-24", "web save after the sent text")
    web_after_sent_writes = trace_count(ws.base)
    learned = pm_learn.rules()
    lines = RAW.splitlines()
    applied = pm_learn.apply(lines, {"Ledger"}, learned)
    applied_none = pm_learn.apply(lines, set(), {"cut": [], "rename": {}, "parts": [], "order": {}, "added": {}, "days": 0})
    all_cut = pm_learn.apply(["Ops: status check: no action needed."], set(), learned)
    left = pm_learn.left_out(lines, learned)
    rows = store.read_jsonl(pm_learn.path())
    projection = pm_learn.cut_projection(lines, "Billing: shipped.\nLedger: fixed.", learned, rows)
    projection_present = pm_learn.cut_projection(lines, "- Ops: still here.", learned, rows)
    projection_unknown = pm_learn.cut_projection(lines, "| table | row |", learned, rows)
    hints = [pm_learn.hint(text) for text in ("We launched billing", "Led the sync", "Posted an update in #team", "Fixed a bug")]
    at("2026-10-25T09:00:00+00:00")
    reset_had = pm_learn.reset("**ops**")
    reset_missing = error(lambda: pm_learn.reset("Nope"))
    after_reset = pm_learn.rules()
    if plant() == "duplicate":
        store.write_jsonl(pm_learn.path(), store.read_jsonl(pm_learn.path()))
    for n in range(15):
        day = date(2026, 9, 1 + n).isoformat()
        pm_learn.remember(day, "A: one.", "A: one.")
        pm_learn.record(day, "A: two.")
    kept = [row["day"] for row in store.read_jsonl(pm_learn.path())]
    if plant() == "missing":
        pm_learn.resets_path().unlink()
    strip = lambda value: json.loads(json.dumps(value).replace(CANARY, "<canary>"))  # noqa: E731
    return {
        "after_one": strip(after_one),
        "after_two": strip(after_two),
        "unchanged_writes": unchanged_writes,
        "web_after_sent_writes": web_after_sent_writes,
        "learned": strip(learned),
        "applied": applied,
        "applied_none": applied_none,
        "all_cut": all_cut,
        "left_out": left,
        "projection": list(projection),
        "projection_present": list(projection_present),
        "projection_unknown": list(projection_unknown),
        "hints": hints,
        "reset_had": reset_had,
        "reset_missing": reset_missing,
        "after_reset_cut": after_reset["cut"],
        "kept_days": kept,
        "sealed": sealed(pm_learn.path()),
        "plain_absent": plain_absent(pm_learn.path()),
    }


def run(case: str, base: Path) -> dict[str, Any]:
    return {
        "difflib-vectors": difflib_vectors,
        "results-records": results_records,
        "handoff-records": handoff_records,
        "pm-sent-receipts": pm_sent_receipts,
        "pm-lessons": pm_lessons,
    }[case](base)


def main() -> int:
    args = sys.argv[1:]
    if args not in (["--all"],) and not (len(args) == 2 and args[0] == "--case" and args[1] in CASES):
        print("rust17.py: --all | --case <id>", file=sys.stderr)
        return 2
    selected = CASES if args == ["--all"] else (args[1],)
    root = os.environ.get("CAREERBREW_HOME", "")
    if not root:
        print("rust17.py: CAREERBREW_HOME is required", file=sys.stderr)
        return 2
    base = Path(root) / "rust17-workload"
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
