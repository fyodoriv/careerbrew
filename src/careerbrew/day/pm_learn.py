"""PM update edits that teach the next draft (issue #185).

Three texts per day: the raw draft careerbrew builds, the shown draft (the raw one after the lessons below),
and the owner's final text. `remember()` keeps the raw and shown drafts each time the PM view is built
(`day/pm-shown-<day>.json`, written only on change); `record()` keeps all three in `day/pm-edits.jsonl`
(the last `KEEP` edited days), when the owner saves in the web app and again when careerbrew finds the
message they sent (issue #238). All are encrypted at rest.
The sent message is the final text, so it wins over a web save of the same day. `pm_sent_text(day)` is the hook
that finds it: a manual `careerbrew day pm-sent` record, an explicit app send with an exact receipt
(`day/pm-sent-<day>.md`), else the owner's own message (`actor: me`) in the
`pm_slack` channel or exact DM recipient that day in the work feed, the one most like the shown draft.
An app-send receipt identifies later edits to that exact message; a manual record remains authoritative.
`learn_sent(day)` records it; the watcher also revisits created days of retained own Slack originals.
Only what the owner changed from the shown draft is evidence, so a lesson never confirms itself:
- cut:    a label the owner deleted from the shown draft on at least `MIN_TIMES` days, more often than they
          kept it, is left out; adding a hidden line back counts double against the cut;
- rename: a label the owner wrote under another label (the same or a close text, any case, bold or not) on
          at least `MIN_TIMES` days is written their way; writing the old label again counts double against it;
- part:   a part (`next: …`, `overdue: …`) the owner cut on at least `MIN_TIMES` days, more often than they
          kept it, is left out of that label's line;
- order:  labels no project rank orders follow the owner's average position;
- added:  a line the owner wrote that matches no draft line, on at least `MIN_TIMES` days per source hint
          (`meeting`, `announcement`, `own post`, `other`; issue #238), so the draft builder knows what it misses.
`careerbrew signals reset <cut|rename|part> <label>` forgets one label's lessons (`reset`).
Lifecycle: files under the data root only; nothing is kept in process memory between calls.
"""

from __future__ import annotations

import contextlib
import difflib
import fcntl
import re
import secrets
from datetime import date, datetime
from typing import Any

from .. import calendar, events, paths, store

KEEP = 14  # edited days kept
MIN_TIMES = 2  # a change seen on this many edited days becomes a lesson
SAME_TEXT = 0.6  # two lines' texts this similar (difflib ratio) are one line, whatever their labels
SENT_LIKE = 0.3  # an own message this similar to the shown draft (difflib ratio) is the sent update
_PART = re.compile(r"(?:^|;\s*)(next|overdue):")
# What kind of item an added line is (issue #238), from its own words: the draft builder looks for that kind.
HINTS = (
    ("announcement", re.compile(r"\b(?:launch(?:ed|ing)?|announc\w*|now live|is live|went live|released|rolled out)\b", re.I)),
    ("meeting", re.compile(r"\b(?:led|hosted|ran|facilitated|meeting|sync|planning|review|1:1|workshop|kickoff)\b", re.I)),
    ("own post", re.compile(r"\b(?:checked in|check-in|posted|shared|wrote|thread|update[ds]? (?:in|on))\b", re.I)),
)
_BOLD = re.compile(r"[*_`]+")


def path():
    return paths.sub("day", "pm-edits.jsonl")


def shown_path(day: str):
    return paths.sub("day", f"pm-shown-{day}.json")


def sent_path(day: str):
    return paths.sub("day", f"pm-sent-{day}.md")


def receipt_path(day: str):
    return paths.sub("day", f"pm-sent-receipt-{day}.json")


def _receipt_id(day: str) -> str:
    path = receipt_path(day)
    if not path.exists():
        return ""
    try:
        receipt = store.read_json(path)
    except ValueError:
        receipt = None
    identity = receipt.get("id") if isinstance(receipt, dict) and set(receipt) == {"id"} else None
    if isinstance(identity, str) and re.fullmatch(r"slack:[CDG][A-Z0-9]+:\d+\.\d+", identity):
        return identity
    events.emit("day.source.error", source_kind="pm-receipt", error="InvalidReceipt", story="us-59")
    return ""


def hint(line: str) -> str:
    """The source hint of a line the owner added (issue #238)."""
    return next((name for name, pat in HINTS if pat.search(line)), "other")


@contextlib.contextmanager
def _sent_lock(day: str):
    """One sent-day file lock covers its source text and derived promises; released on exit."""
    canonical = date.fromisoformat(day).isoformat()
    lock = paths.sub("day", f".pm-sent-{canonical}.lock")
    lock.parent.mkdir(parents=True, exist_ok=True)
    with lock.open("a") as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def save_sent(day: str, text: str, *, receipt: dict[str, str] | None = None) -> None:
    """Keep one sent-day source and optional exact app-send receipt encrypted; a manual save clears the receipt."""
    with _sent_lock(day):
        _save_sent(day, text, receipt=receipt)


def _save_sent(day: str, text: str, *, receipt: dict[str, str] | None = None) -> None:
    from . import pm_promises

    valid = bool(text.strip() and receipt and re.fullmatch(r"[CDG][A-Z0-9]+", receipt.get("channel", "")) and re.fullmatch(r"\d+\.\d+", receipt.get("ts", "")))
    new_identity = f"slack:{receipt['channel']}:{receipt['ts']}" if valid and receipt else ""
    old_text = store.read_text(sent_path(day)) or ""
    if old_text.strip() != text.strip() or _receipt_id(day) != new_identity:
        _outcome_path(day).unlink(missing_ok=True)
    if not valid:
        pm_promises.record(day, text)
    if text.strip():
        store.write_text_if_changed(sent_path(day), text.strip() + "\n")
    elif sent_path(day).exists():
        sent_path(day).unlink()
    if valid and receipt:
        identity = new_identity
        store.write_json_if_changed(receipt_path(day), {"id": identity})
        revision = _record_outcome_revision(day, identity, text)
        pm_promises.record(day, text, revision=revision)
    else:
        receipt_path(day).unlink(missing_ok=True)
        _outcome_path(day).unlink(missing_ok=True)


def _outcome_path(day: str):
    return paths.sub("day", f"pm-sent-outcome-{day}.json")


def _record_outcome_revision(day: str, identity: str, text: str) -> str:
    digest = store.digest("pm-sent-binding-v2", day, identity, text.strip())
    try:
        old = store.read_json(_outcome_path(day))
    except ValueError:
        old = None
    revision = old.get("revision") if isinstance(old, dict) and old.get("digest") == digest else None
    if not isinstance(revision, str) or not re.fullmatch(r"[0-9a-f]{32}", revision):
        revision = secrets.token_hex(16)
    store.write_json_if_changed(_outcome_path(day), {"digest": digest, "revision": revision})
    return revision


def _outcome_revision(day: str, text: str | None = None) -> str:
    """Read a committed sent-text and receipt binding under the sent-day lock."""
    saved = store.read_text(sent_path(day))
    identity = _receipt_id(day) if saved else ""
    if not saved or not identity or (text is not None and text.strip() != saved.strip()):
        return ""
    digest = store.digest("pm-sent-binding-v2", day, identity, saved.strip())
    try:
        binding = store.read_json(_outcome_path(day))
    except ValueError:
        return ""
    if not isinstance(binding, dict) or set(binding) != {"digest", "revision"} or binding.get("digest") != digest:
        return ""
    revision = binding.get("revision")
    return revision if isinstance(revision, str) and re.fullmatch(r"[0-9a-f]{32}", revision) else ""


def retained_sent_days() -> set[str]:
    """Created days of retained own feed originals; retention is owned by the feed."""
    from ..work import feed

    originals = feed.originals()
    days = set()
    for row in feed.load():
        if row.get("id") not in originals or row.get("actor") != "me" or row.get("personal") or row.get("source") != "slack":
            continue
        with contextlib.suppress(ValueError):
            when = datetime.fromisoformat(str(row.get("at", "")).replace("Z", "+00:00"))
            days.add(when.astimezone(calendar.local_tz()).date().isoformat())
    return days


def pm_sent_text(day: str) -> str | None:
    """The PM update the owner sent that day, or None (issue #238). Reads only; never sends.
    A manual record wins. An app receipt follows only its exact message; otherwise match the configured destination."""
    with _sent_lock(day):
        return _pm_sent_text(day)


def _pm_sent_text(day: str) -> str | None:
    """Read the source/receipt pair while the caller holds its canonical sent-day lock."""
    saved = store.read_text(sent_path(day))
    identity = _receipt_id(day) if saved else ""
    if saved and not identity:
        return saved.strip() or None
    from .. import config
    from ..work import feed

    target = str(config.get("pm_slack") or "").strip()
    if not target and not identity:
        return None
    seen = store.read_json(shown_path(day), {}) or {}
    shown = str(seen.get("shown") or seen.get("raw") or "")
    if not shown and not identity:
        return None
    tz = calendar.local_tz()
    ids = set()
    originals = feed.originals()
    for e in feed.load():
        if e.get("source") != "slack" or e.get("actor") != "me" or e.get("personal"):
            continue
        if identity:
            if e.get("id") != identity:
                continue
        elif target.startswith("#"):
            if str(e.get("channel") or "").casefold() != target[1:].casefold():
                continue
        else:
            raw = originals.get(str(e.get("id") or ""), {}).get("original") or {}
            channel = raw.get("channel") if isinstance(raw, dict) else None
            if not isinstance(channel, dict) or channel.get("is_im") is not True or channel.get("is_mpim", False) is not False or channel.get("name") != target:
                continue
        with contextlib.suppress(ValueError):
            when = datetime.fromisoformat(str(e.get("at", "")).replace("Z", "+00:00"))
            if when.astimezone(tz).date().isoformat() == day:
                ids.add(e["id"])
    texts = [str(o.get("text") or "").strip() for i, o in originals.items() if i in ids]
    if identity:
        return next((text for text in texts if text), (saved or "").strip() or None)
    scored = [(difflib.SequenceMatcher(None, t, shown).ratio(), t) for t in texts if t]
    best = max(scored, default=(0.0, ""))
    return best[1] if best[0] >= SENT_LIKE else None


def learn_sent(day: str) -> bool:
    """Record the sent message as the day's final text (issue #238); False when there is none or it is kept already."""
    with _sent_lock(day):
        return _learn_sent(day)


def _learn_sent(day: str) -> bool:
    text = _pm_sent_text(day)
    if not text:
        return False
    identity = _receipt_id(day)
    if identity and sent_path(day).exists():
        if (store.read_text(sent_path(day)) or "").strip() != text.strip():
            _outcome_path(day).unlink(missing_ok=True)
        store.write_text_if_changed(sent_path(day), text.strip() + "\n")
        revision = _record_outcome_revision(day, identity, text)
    else:
        revision = ""
    from . import pm_promises

    pm_promises.record(day, text, revision=revision)
    if any(r.get("day") == day and r.get("from") == "sent" and r.get("body") == text for r in store.read_jsonl(path())):
        return False
    record(day, text, source="sent")
    return True


def resets_path():
    return paths.sub("day", "pm-resets.json")


def _norm(label: str) -> str:
    return _BOLD.sub("", label).strip().casefold()


def _split(line: str) -> tuple[str, str]:
    line = line.strip().lstrip("-•◦ ").strip()
    label, sep, text = line.partition(":")
    return (_BOLD.sub("", label).strip(), text.strip()) if sep else ("", line.strip())


def _lines(text: str) -> list[tuple[str, str]]:
    return [_split(raw) for raw in text.splitlines() if raw.strip()]


def _parts(body: str) -> set[str]:
    return set(_PART.findall(body))


def _find(label: str, text: str, lines: list[tuple[str, str]]) -> tuple[str, str] | None:
    """The line in `lines` that is this one: the same label (any case or bold), else the closest text."""
    for lab, body in lines:
        if _norm(lab) == _norm(label):
            return lab, body
    best = max(lines, key=lambda lt: difflib.SequenceMatcher(None, lt[1], text).ratio(), default=None)
    if best and difflib.SequenceMatcher(None, best[1], text).ratio() >= SAME_TEXT:
        return best
    return None


def _added(label: str, text: str, lines: list[tuple[str, str]]) -> bool:
    """A line the owner wrote that is no draft line: no draft line has its label, and none has text like it.
    An unlabeled line never matches another unlabeled line by its empty label alone."""
    if label and any(_norm(lab) == _norm(label) for lab, _ in lines):
        return False
    return not any(difflib.SequenceMatcher(None, f"{lab} {body}", f"{label} {text}").ratio() >= SAME_TEXT for lab, body in lines)


def remember(day: str, raw: str, shown: str) -> None:
    """Keep the day's raw and shown drafts, so a later save is compared with what the owner saw (no rebuild)."""
    store.write_json_if_changed(shown_path(day), {"raw": raw, "shown": shown})


def record(day: str, body: str, source: str = "web") -> None:
    """Keep one day's three texts when the owner saves (`web`) or sends (`sent`, issue #238); an empty body
    (a reset) or no change drops the day. A web save never replaces a sent text: the sent one is the final."""
    seen = store.read_json(shown_path(day), {}) or {}
    raw, shown = str(seen.get("raw") or ""), str(seen.get("shown") or seen.get("raw") or "")
    history = path()
    history.parent.mkdir(parents=True, exist_ok=True)
    # Sent learners take their day lock before this shared history lock.
    with history.with_name(f".{history.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        old = store.read_jsonl(history)
        if source == "web" and any(r.get("day") == day and r.get("from") == "sent" for r in old):
            return
        rows = [r for r in old if r.get("day") != day]
        if raw and body.strip() and body.strip() != shown.strip():
            row = {"day": day, "draft": raw, "shown": shown, "body": body, "saved": calendar.now_utc().isoformat(timespec="seconds"), "from": source}
            rows.append({**row, "edit_rules": edit_rules([row])})
        rows = sorted(rows, key=lambda r: r["day"])[-KEEP:]
        if rows != old:
            store.write_jsonl(history, rows)
    learned = rules()
    events.emit("day.pm.learned", status="ok", source_kind=source, cut=len(learned["cut"]), rename=len(learned["rename"]), parts=len(learned["parts"]), added=len(learned["added"]), count=learned["days"])


def reset(label: str) -> int:
    """Forget one label's lessons up to now (`careerbrew signals reset cut <label>`). Returns the edited days it had."""
    had = sum(1 for r in store.read_jsonl(path()) if any(_norm(lab) == _norm(label) for lab, _ in _lines(str(r.get("draft") or ""))))
    if not had:
        raise ValueError(f"no PM lesson for {label} (see: careerbrew signals)")
    data = store.read_json(resets_path(), {}) or {}
    data[_norm(label)] = calendar.now_utc().isoformat(timespec="seconds")
    store.write_json(resets_path(), data)
    return had


def _before(row: dict[str, Any], since: str) -> bool:
    """A row saved at or before a reset. A reset is a UTC stamp (an older one, a local date); a row has its
    save stamp (an older one, only its local day), so a same-day edit after a reset still counts."""
    if len(since) == 10 or not row.get("saved"):
        return str(row.get("day", "")) <= since[:10]
    return str(row["saved"]) <= since


def edit_rules(rows: list[dict[str, Any]]) -> list[dict[str, str]]:
    """Candidates kept with each encrypted edit row; never an unguarded text exit.

    Each changed shown line is evidence. Candidates need the same source facts
    before reuse; stable cut/rename/part lessons still use MIN_TIMES distinct days.
    """
    from .log import _no_action

    out = []
    for row in rows:
        shown = [line.strip() for line in str(row.get("shown") or row.get("draft") or "").splitlines() if line.strip()]
        saved = [line.strip() for line in str(row.get("body") or "").splitlines() if line.strip()]
        for change, a, b, c, d in difflib.SequenceMatcher(a=shown, b=saved, autojunk=False).get_opcodes():
            if change == "equal":
                continue
            before, after = shown[a:b], saved[c:d]
            for i in range(max(len(before), len(after))):
                old = before[i] if i < len(before) else ""
                new = after[i] if i < len(after) else ""
                instruction = "keep the owner's replacement only when the same source facts recur"
                if re.search(r"\bIn draft\b", new, re.I) and not re.search(r"\bIn draft\b", old, re.I):
                    instruction = "mark unfinished work In draft; do not claim it shipped"
                elif _no_action(old) and (not new or old.casefold() not in new.casefold()):
                    instruction = "drop a status check with no action or outcome"
                elif not old:
                    instruction = "include this missing work when fresh sources support it"
                elif not new:
                    instruction = "leave out this removed line when its context recurs"
                out.append({"day": str(row.get("day") or ""), "change": "replace" if old and new else "remove" if old else "add", "before": old, "after": new, "rule": instruction})
    return out


def rules(rows: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    """What the owner's edits teach: labels to cut, labels to rename, parts to cut per label, and an order."""
    rows = store.read_jsonl(path()) if rows is None else rows
    resets = store.read_json(resets_path(), {}) or {}
    cut: dict[str, list[int]] = {}  # label -> [cut, kept]
    renames: dict[tuple[str, str], int] = {}
    parts: dict[tuple[str, str], list[int]] = {}
    position: dict[str, list[int]] = {}
    shown_label: dict[str, str] = {}
    added: dict[str, int] = {}  # source hint -> edited days with such an added line
    for r in rows:
        raw, shown, body = _lines(str(r.get("draft") or "")), _lines(str(r.get("shown") or r.get("draft") or "")), _lines(str(r.get("body") or ""))
        for h in {hint(f"{lab} {text}") for lab, text in body if _added(lab, text, raw + shown)}:
            added[h] = added.get(h, 0) + 1
        for n, (label, _) in enumerate(body):
            position.setdefault(label, []).append(n)
        for label, text in raw:
            key = _norm(label)
            if (since := resets.get(key)) and _before(r, since):
                continue
            seen, got = _find(label, text, shown), _find(label, text, body)
            c = cut.setdefault(label, [0, 0])
            if seen and not got:
                c[0] += 1
            elif got:
                c[1] += 1 if seen else 2  # adding a hidden line back counts double
            if got and seen:
                shown_label[key] = seen[0]
                if _norm(got[0]) != _norm(seen[0]):  # the owner wrote it under another label than the one shown
                    if _norm(got[0]) == key:  # back to careerbrew's own label: against the rename shown
                        renames[(label, seen[0])] = renames.get((label, seen[0]), 0) - 2
                    else:
                        renames[(label, got[0])] = renames.get((label, got[0]), 0) + 1
                for part in _parts(text):
                    p = parts.setdefault((label, part), [0, 0])
                    if part in _parts(seen[1]) and part not in _parts(got[1]):
                        p[0] += 1
                    elif part in _parts(got[1]):
                        p[1] += 1 if part in _parts(seen[1]) else 2
    rename: dict[str, str] = {}
    for (old, new), n in sorted(renames.items(), key=lambda kv: kv[1]):
        if n >= MIN_TIMES:
            rename[old] = new
    return {
        "cut": sorted(k for k, (c, kept) in cut.items() if c >= MIN_TIMES and c > kept and k not in rename),
        "rename": rename,
        "parts": sorted([label, part] for (label, part), (c, kept) in parts.items() if c >= MIN_TIMES and c > kept),
        "order": {label: sum(ns) / len(ns) for label, ns in position.items()},
        "added": {h: n for h, n in sorted(added.items()) if n >= MIN_TIMES},
        "days": len(rows),
        "edit_rules": edit_rules(rows),
    }


def apply(lines: list[str], ranked: set[str], learned: dict[str, Any] | None = None) -> list[str]:
    """The draft's lines after the owner's lessons: cut labels left out, renames applied, cut parts dropped,
    unranked labels in the owner's order (ranked projects keep their rank). Never empties the draft."""
    learned = rules() if learned is None else learned
    if not learned["days"]:
        return lines
    drop_parts = {(label, part) for label, part in learned["parts"]}
    out: list[tuple[str, str, str]] = []  # (the draft's label, the label to write, the text)
    for line in lines:
        label, body = _split(line)
        if label in learned["cut"]:
            continue
        kept = [p.strip() for p in re.split(r";\s+(?=(?:next|overdue):)", body.rstrip(".")) if p.strip() and (label, p.strip().split(":", 1)[0]) not in drop_parts]
        if not kept:
            continue
        out.append((label, learned["rename"].get(label, label), f"{'; '.join(kept)}."))
    order = learned["order"]
    ranked_rows = [r for r in out if r[0] in ranked]
    rest = sorted((r for r in out if r[0] not in ranked), key=lambda r: order.get(r[1], order.get(r[0], len(order) + 1)))
    result = [f"{label}: {body}" if label else body for _, label, body in [*ranked_rows, *rest]]
    return result or lines


def left_out(raw: list[str], learned: dict[str, Any] | None = None) -> list[str]:
    """The labels of the raw draft that the lessons leave out of the shown one: the PM view names them."""
    learned = rules() if learned is None else learned
    return [label for label, _ in (_split(line) for line in raw) if label in learned["cut"]]


def _cut_date(label: str, rows: list[dict[str, Any]], resets: dict[str, Any]) -> dict[str, Any]:
    key = _norm(label)
    dated: list[tuple[datetime, str, str | None]] = []
    days: list[str] = []
    for row in rows:
        if (since := resets.get(key)) and _before(row, since):
            continue
        raw = _lines(str(row.get("draft") or ""))
        shown = _lines(str(row.get("shown") or row.get("draft") or ""))
        body = _lines(str(row.get("body") or ""))
        if not any(_norm(lab) == key and _find(lab, text, shown) and not _find(lab, text, body) for lab, text in raw):
            continue
        day = str(row.get("day") or "")
        try:
            if date.fromisoformat(day).isoformat() == day:
                days.append(day)
        except ValueError:
            pass
        saved = str(row.get("saved") or "")
        try:
            stamp = datetime.fromisoformat(saved)
            if stamp.utcoffset() is not None:
                dated.append((stamp, saved, day if day in days else None))
        except (ValueError, OverflowError):
            pass
    if dated:
        _, saved, day = max(dated, key=lambda value: value[0])
        return {"recorded_at": saved, "report_day": day, "date_basis": "saved"}
    return {"recorded_at": None, "report_day": max(days, default=None), "date_basis": "report-day" if days else "unavailable"}


def cut_projection(raw: list[str], body: str, learned: dict[str, Any], rows: list[dict[str, Any]]) -> tuple[list[str], dict[str, Any]]:
    """Project learned cut labels against current text without changing lessons."""
    candidates = []
    seen = set()
    resets = store.read_json(resets_path(), {}) or {}
    for label in left_out(raw, learned):
        key = _norm(label)
        if key in seen:
            continue
        seen.add(key)
        spellings = dict.fromkeys([label, learned["rename"].get(label, label)])
        candidates.append({"label": label, "aliases": [{"spelling": value, "norm": _norm(value)} for value in spellings], **_cut_date(label, rows, resets)})

    present = set()
    unknown = False
    spellings = {alias["spelling"] for candidate in candidates for alias in candidate["aliases"]}
    for line in body.splitlines():
        clean = line.strip().lstrip("-•◦ ").strip()
        if not clean:
            continue
        label, _ = _split(line)
        if not label or re.match(r"(?:[#>|<]|\[|`{3}|~{3}|\d+[.)]\s)", clean) or ("|" in label):
            unknown = True
        elif label.isascii() or label in spellings:
            present.add(_norm(label))
        else:
            unknown = True
    absent = [candidate["label"] for candidate in candidates if not any(alias["norm"] in present for alias in candidate["aliases"])]
    return ([] if unknown else absent), {"schema": 1, "candidates": candidates}
