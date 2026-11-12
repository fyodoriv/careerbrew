"""Feedback signals: the owner's decisions, turned into weights that later decisions read (issue #185).

careerbrew stores many decisions (accept, dismiss, answer, skip). This module reads them back, so every
input changes something later: a rank, a question budget, what a plugin learns. It reads the stores on
each call and keeps nothing in process memory; the one thing it writes is the owner's resets
(`day/signal-resets.json`, `reset()`), encrypted like every data file.

Inbox (`day/inbox.json`): each decision of the last `WINDOW_DAYS` days counts for its key, (kind, source).
The source is a plugin (`plugin:<name>`), a project (a question or a source proposal about it), what a
question asks about (an impact question's tool, `github` (a `pr` too) or `jira`; a project question's
`tracker` or `repo`; a competency question's task origin, `user`, `agent`, or `proposal`), or `careerbrew`.
One Accept all or Dismiss all counts once, however many rows it decided.
- `weight_of(counts)` is 1.0 with no decisions and falls as the owner dismisses that key's items:
  `(accepted + 1) / (accepted + dismissed + 2)`, doubled, kept between `WEIGHT_MIN` and 1.0. Open
  proposals sort by it ("Best first"), and new questions fill `questions_per_day` highest weight first.
- `is_muted(counts)`: the owner skipped the key's last `MUTE_AFTER` questions in a row, the last one under
  `MUTE_DAYS` days ago, so no new question of that kind is asked about that source. After `MUTE_DAYS` one
  question comes through again: answering it ends the run; skipping it mutes the key again.
- `careerbrew signals reset <kind> <source>` forgets a key's decisions up to now: its weight is 1.0 again.
"""

from __future__ import annotations

import fnmatch
import re
from datetime import UTC, datetime, timedelta
from typing import Any

WEIGHT_MIN = 0.1
MUTE_AFTER = 3  # questions of one kind about one source skipped in a row before careerbrew stops asking
MUTE_DAYS = 14  # how long a mute lasts after the last skip; then one question comes through again
WINDOW_DAYS = 90  # decisions older than this no longer count, so an old run of dismissals fades
DECIDED = ("accepted", "dismissed", "answered")
SAME_TOOL = {"pr": "github"}  # one tool under two evidence kinds


def source(item: dict[str, Any]) -> str:
    """Where an inbox item comes from: a plugin, the project it is about, the tool or kind of thing it asks about
    (an impact question's `github`, a project question's `repo`), who added its task, or careerbrew itself."""
    origin = str(item.get("origin") or "")
    if origin.startswith("plugin:"):
        return origin
    data = item.get("data") or {}
    if data.get("project"):
        return f"project:{data['project']}"
    if item.get("kind") == "project" and data.get("kind"):  # which project a tracker key or a repo is part of
        return str(data["kind"])
    src = str(data.get("source") or "careerbrew")
    return SAME_TOOL.get(src, src)


def key(item: dict[str, Any]) -> tuple[str, str]:
    return (str(item.get("kind") or ""), source(item))


def _when(value: Any) -> datetime:
    """A stored time as an aware datetime (a naive one is UTC); the earliest time when it is missing or bad."""
    try:
        dt = datetime.fromisoformat(str(value))
    except ValueError:
        return datetime.min.replace(tzinfo=UTC)
    return dt if dt.tzinfo else dt.replace(tzinfo=UTC)


def _decided_at(i: dict[str, Any]) -> datetime:
    return _when(i.get("decided_at") or i.get("created_at"))


def resets_path():
    from . import paths

    return paths.sub("day", "signal-resets.json")


def _resets() -> dict[str, str]:
    from . import store

    return (store.read_json(resets_path(), {}) or {}).get("resets", {})


PM_KINDS = ("cut", "rename", "part")


def reset(kind: str, src: str) -> int:
    """Forget a signal's decisions up to now (`careerbrew signals reset`): an inbox key, or a PM label's lessons
    (`cut`, `rename`, `part` with the label). Returns how many decisions (or edited days) it had."""
    from . import calendar, events, store

    if kind in PM_KINDS:
        from .day import pm_learn

        n = pm_learn.reset(src)
        events.emit("signals.reset", status="ok", count=n)
        return n

    had = inbox_counts().get((kind, src))
    if had is None:
        raise ValueError(f"no signal for {kind} from {src} (see: careerbrew signals)")
    resets = {**_resets(), f"{kind}|{src}": calendar.now_utc().isoformat(timespec="seconds")}
    store.write_json(resets_path(), {"resets": resets})
    events.emit("signals.reset", status="ok", count=had["accepted"] + had["dismissed"])
    return had["accepted"] + had["dismissed"]


def _decided(items: list[dict[str, Any]] | None) -> list[dict[str, Any]]:
    from . import calendar

    if items is None:
        from .day import inbox

        items = inbox.load()
    cut = calendar.now_utc() - timedelta(days=WINDOW_DAYS)
    resets = _resets()
    rows = []
    for i in items:
        if i.get("status") not in DECIDED or _decided_at(i) < cut:
            continue
        k = key(i)
        if (since := resets.get(f"{k[0]}|{k[1]}")) and _decided_at(i) <= _when(since):
            continue
        rows.append(i)
    return sorted(rows, key=_decided_at)


def inbox_counts(items: list[dict[str, Any]] | None = None) -> dict[tuple[str, str], dict[str, Any]]:
    """Per (kind, source): accepted (accepts and answers), dismissed, the dismissals in a row at the end, when the
    last one was, and the type. The rows of one Accept all or Dismiss all (`decided_group`) count once."""
    out: dict[tuple[str, str], dict[str, Any]] = {}
    seen_groups: set[str] = set()
    for i in _decided(items):
        if (g := i.get("decided_group")) and g in seen_groups:
            continue
        if g:
            seen_groups.add(g)
        c = out.setdefault(key(i), {"accepted": 0, "dismissed": 0, "run": 0, "last": "", "type": i.get("type", "")})
        if i["status"] == "dismissed":
            c["dismissed"] += 1
            c["run"] += 1
        else:
            c["accepted"] += 1
            c["run"] = 0
        c["last"] = _decided_at(i).isoformat()
    return out


def weight_of(counts: dict[str, Any] | None) -> float:
    if not counts:
        return 1.0
    a, d = counts["accepted"], counts["dismissed"]
    return round(max(WEIGHT_MIN, min(1.0, 2 * (a + 1) / (a + d + 2))), 3)


def weights(items: list[dict[str, Any]] | None = None) -> dict[tuple[str, str], float]:
    return {k: weight_of(c) for k, c in inbox_counts(items).items()}


def is_muted(counts: dict[str, Any] | None) -> bool:
    from . import calendar

    if not counts or counts["type"] != "question" or counts["run"] < MUTE_AFTER:
        return False
    return calendar.now_utc() - _when(counts["last"]) < timedelta(days=MUTE_DAYS)


def plugin_weight(counts: dict[tuple[str, str], dict[str, Any]], plugin: str) -> float:
    return weight_of(counts.get(("plugin", f"plugin:{plugin}")))


def plugin_rates(plugin: str, items: list[dict[str, Any]] | None = None) -> dict[str, Any]:
    """What one plugin's suggestions taught careerbrew: content-free counts and the weight its next ones get."""
    c = inbox_counts(items).get(("plugin", f"plugin:{plugin}"))
    return {"accepted": c["accepted"] if c else 0, "dismissed": c["dismissed"] if c else 0, "weight": weight_of(c)}


def summary(items: list[dict[str, Any]] | None = None) -> list[dict[str, Any]]:
    """Each signal the owner's decisions made, and what it changes now: for `careerbrew signals` and agents."""
    rows = []
    for (kind, src), c in sorted(inbox_counts(items).items()):
        w = weight_of(c)
        undo = f" (undo: careerbrew signals reset {kind} {src})"
        if is_muted(c):
            effect = f"no new {kind} questions about {src} for {MUTE_DAYS} days after you skipped the last {c['run']}" + undo
        elif w < 1.0:
            effect = f"{kind} items from {src} rank lower (weight {w})" + undo
        else:
            effect = "no change yet"
        rows.append({"signal": "inbox", "kind": kind, "source": src, "accepted": c["accepted"], "dismissed": c["dismissed"], "weight": w, "muted": is_muted(c), "effect": effect})
    rows += pm_summary()
    return rows


def _glob_escape(value: str) -> str:
    """A channel or sender as an exact feed-rule glob: `[`, `*`, and `?` in it match only themselves."""
    return re.sub(r"([\[\]*?])", r"[\1]", value)


RULE_AFTER = 3  # not-ask (or personal) marks from one channel or sender before careerbrew proposes a feed rule


def _group(row: dict[str, Any]) -> tuple[str, str, str]:
    """Where a feed event came from, for a rule: its channel when it has one, else its sender."""
    if row.get("channel"):
        return (str(row.get("source") or ""), "channel", str(row["channel"]))
    return (str(row.get("source") or ""), "from", str(row.get("from") or ""))


def feed_rule_proposals() -> list[dict[str, Any]]:
    """Feed rules the owner's marks suggest (issue #185): `RULE_AFTER` not-ask marks (or dismissed ask notifications)
    from one channel or sender propose `triage: fyi` there; that many personal marks propose `personal: true`.
    A rule the owner already has for that place is not proposed again."""
    from . import notify
    from .work import feed

    rows = feed.load()
    by_id = {r["id"]: r for r in rows}
    not_ask: dict[tuple[str, str, str], int] = {}
    personal: dict[tuple[str, str, str], int] = {}
    for r in rows:
        g = _group(r)
        if not g[2]:
            continue
        if r.get("ask_mark") == "not-ask":
            not_ask[g] = not_ask.get(g, 0) + 1
        if r.get("marked") is True:
            personal[g] = personal.get(g, 0) + 1
    for n in notify.load()["items"]:  # dismissing an ask's notification says it was not worth asking
        if n.get("status") != "dismissed" or not str(n.get("key", "")).startswith("feed:"):
            continue
        refs = [by_id[x] for x in (n.get("refs") or [n.get("ref")]) if x in by_id]
        r = next((x for x in refs if x.get("ask_mark") != "not-ask"), None)  # an event already marked counted above
        if r is not None and (g := _group(r))[2]:
            not_ask[g] = not_ask.get(g, 0) + 1
    try:
        owner_rules = [r for r in feed.rules() if not r.get("builtin")]
    except feed.FeedError:  # a typo in work/feed-rules.yaml: `work feed rules` names it; the inbox still works
        return []
    out = []
    for counts, kind, sets in ((not_ask, "not-ask", {"triage": "fyi"}), (personal, "personal", {"personal": True})):
        key = next(iter(sets))
        for (source, field, value), n in sorted(counts.items()):
            pattern = _glob_escape(value)
            covered = any(r.get(key) == sets[key] and _matches(value.lower(), r["when"].get(field), lower=True) and _matches(source, r["when"].get("source") or "*") for r in owner_rules if field in r["when"])
            if n >= RULE_AFTER and not covered:
                rule = {"name": f"{kind}-{field}-{re.sub(r'[^a-z0-9-]+', '-', value.lower())}"[:60], "when": {"source": source, field: pattern}, **sets}
                out.append({"kind": kind, "source": source, "field": field, "value": value, "count": n, "rule": rule})
    return out


def _matches(value: str, patterns: Any, lower: bool = False) -> bool:
    """A rule's `when` value is one glob or a list of them."""
    return any(fnmatch.fnmatchcase(value, str(p).lower() if lower else str(p)) for p in (patterns if isinstance(patterns, list) else [patterns]))


def pm_summary() -> list[dict[str, Any]]:
    """What the owner's PM update edits teach the next draft (`day/pm_learn.py`)."""
    from .day import pm_learn

    learned = pm_learn.rules()
    out = [{"signal": "pm", "kind": "cut", "source": label, "effect": f"the PM update leaves out the {label} line: you cut it"} for label in learned["cut"]]
    out += [{"signal": "pm", "kind": "rename", "source": old, "effect": f"the PM update writes {old} as {new}: you renamed it"} for old, new in learned["rename"].items()]
    out += [{"signal": "pm", "kind": "part", "source": label, "effect": f"the PM update leaves {part} out of the {label} line: you cut it"} for label, part in learned["parts"]]
    out += [{"signal": "pm", "kind": "added", "source": h, "effect": f"you add {h} lines the draft misses ({n} days)"} for h, n in learned.get("added", {}).items()]
    return out
