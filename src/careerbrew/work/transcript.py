"""Store live-feed tool replies straight from an agent session transcript (issue #234).

An agent that drives a live feed pulls events with MCP tools (chat, ticket, or mail search). The
replies stay in local Claude or Codex session JSONL. `store()` normalizes supported calls/results, keeps
the replies of the tools that `feed_tool_sources` maps to a feed source, and stores each event in
them through `feed.ingest`, so an agent never copies a reply by hand:
- a reply is JSON (an object, a list, or JSON lines); each object in it, or in a list under one of
  its keys, with an id, a time, and a title (or the common aliases in `_ALIASES`) is one event,
  kept as returned in its original;
- a large reply that Claude Code saved to a file ("Full output saved to: <file>") is read from that
  file, only when the file is inside the transcript's own folder;
- a raw search reply (an object with a `tickets` or `messages` list, issue #254) goes through
  `feed.raw_events`, as `work feed add` does; one with no items is a quiet source, stored and no gap;
- an event already stored is not stored again, so a rerun is safe;
- a mapped source with no event stored in the cycle (the whole transcript, or `since` on) is a gap;
- every source stored counts in the feed's cycle record (`feed.mark_cycle`).

Lifecycle: no state of its own; each call reads the transcript and writes through `feed.ingest`.
"""

from __future__ import annotations

import fnmatch
import json
import re
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import config, events
from . import feed

_POINTER = re.compile(r"saved to:?\s+([^\s<>\"'`]+)", re.I)
_SINCE = re.compile(r"^(\d+)([mhd])$")
# Field names a tool may use for an event's id, time, and title, in the order they are tried.
_ALIASES = {
    "id": ("id", "key", "uuid", "ts"),
    "at": ("at", "timestamp", "created_at", "createdAt", "created", "time", "date", "ts"),
    "title": ("title", "subject", "summary", "name", "text"),
}
_COPY = ("kind", "actor", "actor_id", "to_me", "url", "thread", "tags", "personal", "channel", "from", "status", "importance", "project")


def tool_map() -> list[tuple[str, str]]:
    return [tuple(x.split("=", 1)) for x in config.get("feed_tool_sources")]  # type: ignore[misc]


def _source(tool: str, pairs: list[tuple[str, str]]) -> str:
    return next((src for glob, src in pairs if fnmatch.fnmatchcase(tool, glob)), "")


def _since(value: str) -> str:
    """An ISO time, or a duration back from now (30m, 2h, 1d), as a UTC stamp."""
    from ..calendar import now

    m = _SINCE.match(value.strip())
    if m:
        unit = {"m": "minutes", "h": "hours", "d": "days"}[m.group(2)]
        return (now().astimezone(UTC) - timedelta(**{unit: int(m.group(1))})).strftime("%Y-%m-%dT%H:%M:%SZ")
    try:
        return feed._utc(value)
    except feed.FeedError:
        raise feed.FeedError(f"--since {value[:40]!r}: use an ISO time or a duration such as 30m, 2h, 1d") from None


def _text(content: Any) -> str:
    if isinstance(content, str):
        return content
    if isinstance(content, list):
        return "\n".join(str(b.get("text", "")) for b in content if isinstance(b, dict) and b.get("type") in {"text", "input_text"})
    return ""


def _parse(text: str) -> Any:
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        pass
    out = []
    for line in text.splitlines():
        try:
            out.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return out


def _unwrap(data: Any) -> Any:
    if isinstance(data, list) and all(isinstance(b, dict) and b.get("type") in {"text", "input_text"} for b in data) and data:
        return _parse(_text(data))  # a saved file holds the content blocks
    return data


def _candidates(data: Any) -> list[dict[str, Any]]:
    """Objects that may be events: the reply itself, its list items, or the items of a list under one of its keys."""
    data = _unwrap(data)
    if isinstance(data, list):
        return [d for d in data if isinstance(d, dict)]
    if isinstance(data, dict):
        lists = [v for v in data.values() if isinstance(v, list) and any(isinstance(d, dict) for d in v)]
        if lists:
            return [d for v in lists for d in v if isinstance(d, dict)]
        return [data]
    return []


def _pick(obj: dict[str, Any], key: str) -> Any:
    return next((obj[k] for k in _ALIASES[key] if obj.get(k) not in (None, "")), None)


def _at(value: Any) -> Any:
    try:
        return datetime.fromtimestamp(float(value), UTC).strftime("%Y-%m-%dT%H:%M:%SZ")  # an epoch time such as a chat ts
    except (TypeError, ValueError):
        return value


def _event(obj: dict[str, Any], source: str) -> dict[str, Any] | None:
    ident, at, title = _pick(obj, "id"), _pick(obj, "at"), _pick(obj, "title")
    if ident is None or at is None or title is None:
        return None
    item: dict[str, Any] = {k: obj[k] for k in _COPY if k in obj}
    if item.get("actor") not in feed.ACTORS:
        item.pop("actor", None)
    text = obj.get("text") if isinstance(obj.get("text"), str) else obj.get("body")
    item.update(id=f"{source}:{ident}", source=source, at=_at(at), title=str(title).splitlines()[0] if str(title) else "", text=text if isinstance(text, str) else "", original=obj)
    try:
        feed.clean(item)
    except feed.FeedError:
        return None
    return item


def _read(line: str) -> dict[str, Any] | None:
    try:
        row = json.loads(line)
    except json.JSONDecodeError:
        return None
    return row if isinstance(row, dict) else None


def _blocks(row: dict[str, Any], kind: str) -> list[dict[str, Any]]:
    content = (row.get("message") or {}).get("content") if isinstance(row.get("message"), dict) else None
    return [b for b in content if isinstance(b, dict) and b.get("type") == kind] if isinstance(content, list) else []


def _normalized(row: dict[str, Any]) -> dict[str, Any]:
    """Normalize one Claude or native call/result row; no session state is retained."""
    payload = row.get("payload")
    if row.get("type") == "event_msg" and isinstance(payload, dict) and payload.get("type") == "mcp_tool_call_end":
        invocation = payload.get("invocation") or {}
        result = payload.get("result") or {}
        value = result.get("Ok") or {}
        error = "Err" in result or bool(value.get("isError"))
        content = value.get("content", [])
        if not content and isinstance(value.get("structuredContent"), dict):
            content = json.dumps(value["structuredContent"])
        blocks = [
            {"type": "tool_use", "id": payload.get("call_id"), "name": f"mcp__{invocation.get('server', '')}__{invocation.get('tool', '')}"},
            {"type": "tool_result", "tool_use_id": payload.get("call_id"), "content": content, "is_error": error},
        ]
        return {"timestamp": row.get("timestamp"), "message": {"content": blocks}}
    if row.get("type") != "response_item" or not isinstance(payload, dict):
        return row
    kind = payload.get("type")
    if kind == "function_call":
        name = str(payload.get("name") or "")
        namespace = str(payload.get("namespace") or "").rstrip("_")
        if namespace.startswith("mcp__") and not name.startswith("mcp__"):
            name = namespace + "__" + name
        block = {"type": "tool_use", "id": payload.get("call_id"), "name": name}
    elif kind == "function_call_output":
        output = payload.get("output", "")
        if isinstance(output, dict):
            output = json.dumps(output)
        block = {"type": "tool_result", "tool_use_id": payload.get("call_id"), "content": output, "is_error": payload.get("is_error", False)}
    else:
        return row
    return {"timestamp": row.get("timestamp"), "message": {"content": [block]}}


def _stamp(row: dict[str, Any]) -> str:
    try:
        return feed._utc(row.get("timestamp"))
    except feed.FeedError:
        return ""


def store(path: Path, since: str = "") -> dict[str, Any]:
    """Store every mapped tool reply in the transcript; return counts and the sources that stored nothing."""
    pairs = tool_map()
    if not pairs:
        raise feed.FeedError("no tool map (fix: careerbrew config set feed_tool_sources 'mcp__chat__*=chat, mcp__tickets__*=tickets')")
    path = Path(path).expanduser()
    if not path.is_file():
        raise feed.FeedError("the transcript file does not exist (fix: pass the session's .jsonl file)")
    folder = path.resolve().parent
    cut = _since(since) if since else ""
    sources = list(dict.fromkeys(src for _g, src in pairs))
    replies = dict.fromkeys(sources, 0)
    stored = dict.fromkeys(sources, 0)
    pointers = {"followed": 0, "refused": 0}
    tools: dict[str, str] = {}
    items: dict[str, dict[str, Any]] = {}
    quiet: set[str] = set()  # sources whose raw search reply had no items: pulled and stored, nothing new
    errors = skipped = 0
    failed_sources: set[str] = set()
    recognized = False
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        row = _read(line)
        if row is None:
            continue
        recognized |= isinstance(row.get("message"), dict) or row.get("type") in {"response_item", "session_meta", "event_msg", "turn_context"}
        row = _normalized(row)
        for b in _blocks(row, "tool_use"):
            src = _source(str(b.get("name", "")), pairs)
            if src and b.get("id"):
                tools[str(b["id"])] = src
        for b in _blocks(row, "tool_result"):
            src = tools.get(str(b.get("tool_use_id", "")))
            if not src or (cut and _stamp(row) < cut):
                continue
            replies[src] += 1
            if b.get("is_error"):
                errors += 1
                failed_sources.add(src)
                continue
            text = _text(b.get("content"))
            m = _POINTER.search(text) if len(text) < 2000 else None
            if m:
                target = (folder / m.group(1).rstrip(".,;:)")).resolve()
                if target.is_relative_to(folder) and target.is_file():
                    pointers["followed"] += 1
                    text = target.read_text(encoding="utf-8", errors="replace")
                else:
                    pointers["refused"] += 1
                    failed_sources.add(src)
                    continue
            data = _unwrap(_parse(text))
            raw = feed.raw_events(data, src)
            if raw is not None:
                evs, n_skipped, _src = raw
                skipped += n_skipped
                items.update((ev["id"], ev) for ev in evs)
                if not evs and not n_skipped:
                    quiet.add(src)
                continue
            for obj in _candidates(data):
                ev = _event(obj, src)
                if ev is None:
                    skipped += 1
                    continue
                items[ev["id"]] = ev
    if not recognized:
        raise feed.FeedError("unsupported transcript format (use Claude/native JSONL, or careerbrew work feed add)")
    r = feed.ingest(list(items.values()), source_kind="transcript", sources=quiet) if items or quiet else {"new": 0, "updated": 0}
    for ev in items.values():
        stored[ev["source"]] += 1
    gaps = [s for s in sources if not stored[s] and s not in quiet]
    for source in failed_sources | {s for s in gaps if replies[s]}:
        feed.note_failure(source)
    out = {"new": r["new"], "updated": r["updated"], "already": len(items) - r["new"] - r["updated"], "skipped": skipped, "errors": errors, "replies": replies, "stored": stored, "pointers": pointers, "gaps": gaps}
    events.emit("work.feed.transcript", status="gap" if gaps else "ok", new=r["new"], updated=r["updated"], skipped=skipped, errors=errors, gaps=len(gaps), story="us-61")
    return out
