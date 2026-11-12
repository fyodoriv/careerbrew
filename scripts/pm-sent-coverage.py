#!/usr/bin/env python3
"""Report aggregate coverage of sent PM edits from local records."""

import argparse
import difflib
import fcntl
import json
import os
import re
import sys
from contextlib import ExitStack
from datetime import date, datetime, timedelta
from pathlib import Path
from zoneinfo import ZoneInfo

from yaml import YAMLError

from careerbrew import calendar, paths, plugins, store
from careerbrew.day.pm_learn import SENT_LIKE

RECEIPT = re.compile(r"slack:[CDG][A-Z0-9]+:[0-9]+\.[0-9]+")


def _signature(files):
    result = []
    for path in files:
        try:
            info = path.stat()
        except FileNotFoundError:
            result.append(None)
        else:
            result.append((info.st_ino, info.st_size, info.st_mtime_ns))
    return result


def _sent_body(folder, day, shown, target, events, originals, zone):
    saved = (store.read_text(folder / f"pm-sent-{day}.md", create_key=False) or "").strip()
    receipt = store.read_json(folder / f"pm-sent-receipt-{day}.json", create_key=False)
    if receipt is not None:
        identity = receipt.get("id") if isinstance(receipt, dict) and set(receipt) == {"id"} else None
        if not saved or not isinstance(identity, str) or not RECEIPT.fullmatch(identity):
            return ""
    elif saved or not target:
        return ""
    else:
        identity = None

    matches = set()
    for event in events:
        ident = event.get("id")
        if not isinstance(ident, str) or not RECEIPT.fullmatch(ident):
            continue
        if event.get("source") != "slack" or event.get("actor") != "me" or event.get("personal"):
            continue
        try:
            at = datetime.fromisoformat(event.get("at", ""))
            if at.utcoffset() is None or at.astimezone(zone).date().isoformat() != day:
                continue
        except (TypeError, ValueError):
            continue
        original = originals.get(ident, {})
        if identity:
            if ident != identity:
                continue
        elif target.startswith("#"):
            if str(event.get("channel") or "").casefold() != target[1:].casefold():
                continue
        else:
            raw = original.get("original")
            channel = raw.get("channel") if isinstance(raw, dict) else None
            if not isinstance(channel, dict) or channel.get("is_im") is not True or channel.get("is_mpim", False) is not False or channel.get("name") != target:
                continue
        text = original.get("text")
        if isinstance(text, str) and text.strip() and (identity or difflib.SequenceMatcher(None, text.strip(), shown).ratio() >= SENT_LIKE):
            matches.add((ident, text.strip()))
    if len(matches) == 1:
        return next(iter(matches))[1]
    return saved if identity and not matches else ""


def audit(root: Path, days: list[str]):
    folder = root / "day"
    feed = root / "work" / "feed"
    settings = root / "settings.yaml"
    history = folder / "pm-edits.jsonl"
    events_path, originals_path = feed / "events.jsonl", feed / "originals.jsonl"
    locks = [feed / ".lock", folder / ".pm-edits.jsonl.lock", *(folder / f".pm-sent-{day}.lock" for day in days)]
    inputs = [settings, history, events_path, originals_path, *locks]
    inputs.extend(
        folder / f"{prefix}{day}{suffix}"
        for day in days
        for prefix, suffix in (
            ("pm-shown-", ".json"),
            ("pm-sent-", ".md"),
            ("pm-sent-receipt-", ".json"),
        )
    )
    with ExitStack() as stack:
        before = _signature(inputs)
        for path in locks:
            try:
                handle = stack.enter_context(path.open("rb"))
            except FileNotFoundError:
                continue
            fcntl.flock(handle, fcntl.LOCK_SH | fcntl.LOCK_NB)
        cfg = store.read_yaml(settings, {}, create_key=False)
        zone = ZoneInfo(cfg.get("timezone") or calendar.system_tz())
        target = str(cfg.get("pm_slack") or "").strip()
        events = store.read_jsonl(events_path, create_key=False)
        originals = {row["id"]: row for row in store.read_jsonl(originals_path, create_key=False)}
        by_day = {}
        for row in store.read_jsonl(history, create_key=False):
            if row.get("from") == "sent":
                by_day.setdefault(row["day"], []).append(row.get("body"))

        excluded = {"no_shown_draft": 0, "unidentified_source": 0, "unchanged": 0}
        defects = {"missing_history": 0, "stale_history": 0, "wrong_day_history": 0, "duplicate_days": 0}
        covered = eligible = 0
        for day in days:
            draft = store.read_json(folder / f"pm-shown-{day}.json", {}, create_key=False)
            shown = (draft.get("shown") or "").strip()
            if not shown:
                excluded["no_shown_draft"] += 1
                continue
            sent = _sent_body(folder, day, shown, target, events, originals, zone)
            if not sent:
                excluded["unidentified_source"] += 1
            elif sent == shown:
                excluded["unchanged"] += 1
            else:
                eligible += 1
                rows = by_day.get(day, [])
                defects["duplicate_days"] += len(rows) > 1
                recorded = rows[-1] if rows else None
                if isinstance(recorded, str) and recorded.strip() == sent:
                    covered += 1
                elif rows:
                    defects["stale_history"] += 1
                else:
                    elsewhere = any(isinstance(body, str) and body.strip() == sent for other_day, bodies in by_day.items() if other_day != day for body in bodies)
                    defects["wrong_day_history" if elsewhere else "missing_history"] += 1
        if _signature(inputs) != before:
            raise RuntimeError("audit inputs changed")
    return {
        "status": "ok" if eligible else "insufficient_data",
        "covered": covered,
        "eligible": eligible,
        "rate": covered / eligible if eligible else None,
        "excluded": excluded,
        "defects": defects,
    }


def _owner_terminal():
    if not (sys.stdin.isatty() and sys.stdout.isatty()):
        return False
    try:
        with open("/dev/tty", "rb") as terminal:
            return os.tcgetpgrp(terminal.fileno()) == os.getpgrp()
    except OSError:
        return False


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--from", dest="start", required=True, type=date.fromisoformat)
    parser.add_argument("--to", dest="end", required=True, type=date.fromisoformat)
    parser.add_argument("--json", action="store_true", required=True)
    args = parser.parse_args(argv)
    span = (args.end - args.start).days + 1
    if not 1 <= span <= 366:
        parser.error("use an inclusive date window of 1 to 366 days")
    try:
        root = paths.data_root(create=False)
        if not (_owner_terminal() or plugins.effective_policy(read_root=root)["agent_may_read_business"] is True):
            print("PM coverage audit requires an owner terminal or business-read policy", file=sys.stderr)
            return 2
        days = [(args.start + timedelta(days=offset)).isoformat() for offset in range(span)]
        print(json.dumps(audit(root, days)))
    except (OSError, ValueError, KeyError, RuntimeError, TypeError, AttributeError, YAMLError):
        print(json.dumps({"status": "incomplete"}))
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
