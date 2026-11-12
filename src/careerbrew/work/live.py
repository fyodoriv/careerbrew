"""The live feed: a generic pull-store-triage cycle over source adapters (issue #267).

An agent (the `live-feed` skill) or the owner runs the feed: `work live start`, then `work live cycle` every
few minutes, `work live status`, and `work live stop`. The logic is generic; a company plugin supplies only its
source config (hosts, channels) and its team roster (`live: {sources, team}` in its manifest).

Source config: `<data root>/work/live-sources.yaml` (the owner's, encrypted at rest), then each
installed plugin's `live.sources` file; a name the owner sets wins over a plugin's. One source:
  - name: chat              # the feed source name: short, lowercase
    kind: chat              # chat | tickets | code | mail | calendar | paging
    required: true          # missing, invalid or stale acquisition fails (`not_stored`)
    adapter: file           # file: the agent saves the tool's raw reply to `inbox` each cycle
    inbox: ~/live/chat.json #   (an object with a list, a list, JSON lines, or content blocks)
    # adapter: connector    # connector: a configured connector (`careerbrew connect slack`)
    # connector: slack
    pull: {hosts: [chat.example.com], channels: [team-x]}   # what to pull; company data, shown only
                            # where business data may show

The cycle contract (`cycle()`), in order:
  1. lock     one cycle at a time (`work/live/.cycle.lock`, an fcntl lock the OS frees on exit);
  2. pull     validate each current reply; unchanged identity/content is not new intake;
              file acquisition age uses modification time, never event dates or poll time;
  3. store    every event through `feed.ingest` (originals encrypted, apart from the analysis);
  4. triage   the feed's rules run inside `ingest` (the owner's first, then the built-in ones);
  5. verdict  one per event stored this cycle (`VERDICTS`, `verdict()`);
  6. actions  the next commands, from the verdicts and the gaps (`_actions()`);
  7. mark     `mark_cycle()`: `not_stored` lists required sources without healthy acquisition.
              Validated unchanged replies within their finite limit pass; a non-empty list fails the cycle, and the
              agent pulls those sources again. Cursors move only for sources that stored.

PM draft rules (`reaches_pm()`): an event reaches the PM draft only when it is your own action
(triage `mine`), a rule marks it evidence, and it is not personal. Only its title and link go;
never message text, never a noise, fyi, or ask event.

Lifecycle: state is `work/live/state.json` (running flag, cursors, inbox fingerprints, counts) and
`work/live/cycles.jsonl` (the last `KEEP_CYCLES` cycle records: ids, counts, verdicts, no text).
This module owns both; nothing is kept in process memory between calls. The state lives until
the data root is removed; `stop` keeps the cursors so a later `start` continues.
"""

from __future__ import annotations

import contextlib
import fcntl
import hashlib
import json
import re
from collections.abc import Iterator
from datetime import UTC, datetime
from pathlib import Path
from typing import Any

from .. import config, events, paths, plugins, store
from . import feed

KINDS = ("chat", "tickets", "code", "mail", "calendar", "paging")
ADAPTERS = ("file", "connector")
VERDICTS = ("answer", "done", "read", "skip", "evidence", "log", "personal")
KEEP_CYCLES = 200
_NAME = re.compile(r"^[a-z0-9][a-z0-9._-]{0,39}$")
_SOURCE_KEYS = {"name", "kind", "required", "adapter", "inbox", "connector", "pull", "max_age_minutes"}


class LiveError(ValueError):
    pass


def _dir() -> Path:
    return paths.sub("work", "live", "state.json").parent


def config_path() -> Path:
    return paths.sub("work", "live-sources.yaml")


def state_path() -> Path:
    return _dir() / "state.json"


def cycles_path() -> Path:
    return _dir() / "cycles.jsonl"


# ---------- source config ----------


def check_source(s: Any, where: str) -> dict[str, Any]:
    """One source in the store's shape; the error names the field and the fix."""
    if not isinstance(s, dict):
        raise LiveError(f"{where}: a source is a mapping with name, kind, and adapter")
    unknown = sorted(set(s) - _SOURCE_KEYS)
    if unknown:
        raise LiveError(f"{where}: unknown key {unknown[0]} (allowed: {', '.join(sorted(_SOURCE_KEYS))})")
    name = str(s.get("name") or "").strip()
    if not _NAME.match(name):
        raise LiveError(f"{where}: name {name[:40]!r} must be a short lowercase name such as chat or tickets")
    kind = str(s.get("kind") or "")
    if kind not in KINDS:
        raise LiveError(f"{where} ({name}): kind must be one of {', '.join(KINDS)}")
    adapter = str(s.get("adapter") or "file")
    if adapter not in ADAPTERS:
        raise LiveError(f"{where} ({name}): adapter must be one of {', '.join(ADAPTERS)}")
    if adapter == "file" and not str(s.get("inbox") or "").strip():
        raise LiveError(f"{where} ({name}): a file adapter needs inbox: the file the agent saves the tool's reply to")
    if adapter == "connector" and not str(s.get("connector") or "").strip():
        raise LiveError(f"{where} ({name}): a connector adapter needs connector: a kind such as slack (careerbrew connect <kind>)")
    pull = s.get("pull") or {}
    if not isinstance(pull, dict):
        raise LiveError(f"{where} ({name}): pull is a mapping such as {{hosts: [...], channels: [...]}}")
    age = s.get("max_age_minutes")
    if "max_age_minutes" in s and (type(age) is not int or not 1 <= age <= 10080):
        raise LiveError(f"{where} ({name}): max_age_minutes must be an integer from 1 to 10080")
    return {
        **({"max_age_minutes": age} if age is not None else {}),
        "name": name,
        "kind": kind,
        "required": bool(s.get("required", False)),
        "adapter": adapter,
        "inbox": str(s.get("inbox") or ""),
        "connector": str(s.get("connector") or ""),
        "pull": pull,
    }


def _read_sources(data: Any, where: str) -> list[dict[str, Any]]:
    rows = (data or {}).get("sources") if isinstance(data, dict) else None
    if rows is None:
        return []
    if not isinstance(rows, list):
        raise LiveError(f"{where}: sources is a list")
    out = [check_source(s, f"{where} source {n}") for n, s in enumerate(rows, 1)]
    names = [s["name"] for s in out]
    dup = next((n for n in names if names.count(n) > 1), None)
    if dup:
        raise LiveError(f"{where}: source {dup} is listed twice")
    return out


def plugin_files(key: str) -> list[tuple[str, Path]]:
    """(origin, path) of each installed plugin's `live.<key>` file (key: sources or team)."""
    out = []
    for p in plugins.installed():
        rel = (p["manifest"].get("live") or {}).get(key)
        if rel:
            out.append((str(p["origin"]), Path(p["root"]) / str(rel)))
    return out


def _yaml(path: Path, where: str) -> Any:
    import yaml

    try:
        return store.read_yaml(path, {})
    except yaml.YAMLError:
        raise LiveError(f"{where} is not YAML (fix: sources: [{{name, kind, adapter, ...}}])") from None


def sources() -> list[dict[str, Any]]:
    """Every source: the owner's first, then each plugin's; a name the owner sets wins."""
    out = [{**s, "origin": "user"} for s in _read_sources(_yaml(paths.data_root() / "work/live-sources.yaml", "work/live-sources.yaml"), "work/live-sources.yaml")]
    seen = {s["name"] for s in out}
    for origin, path in plugin_files("sources"):
        for s in _read_sources(_yaml(path, f"{origin} live sources"), f"{origin} live sources"):
            if s["name"] not in seen:
                out.append({**s, "origin": origin})
                seen.add(s["name"])
    return out


def save_sources(text: str) -> list[dict[str, Any]]:
    """Check a source config (YAML or JSON text) and save it as the owner's; one bad source saves nothing."""
    import yaml

    try:
        data = yaml.safe_load(text)
    except yaml.YAMLError:
        raise LiveError("the source config is not YAML or JSON (fix: sources: [{name, kind, adapter, ...}])") from None
    rows = _read_sources(data, "the source config")
    if not rows:
        raise LiveError("the source config has no sources (fix: sources: [{name: chat, kind: chat, adapter: file, inbox: ...}])")
    store.write_yaml(config_path(), {"sources": rows})
    return rows


# ---------- state ----------


def _state() -> dict[str, Any]:
    st = store.read_json(state_path(), {}) or {}
    st.setdefault("running", False)
    st.setdefault("cursors", {})
    st.setdefault("seen", {})
    st.setdefault("cycles", 0)
    st.setdefault("followed", [])
    st.setdefault("attached", {})
    st.setdefault("validated", {})
    return st


def _save_state(st: dict[str, Any]) -> None:
    store.write_json(state_path(), st)


def _now() -> str:
    from ..calendar import now_utc

    return now_utc().strftime("%Y-%m-%dT%H:%M:%SZ")


@contextlib.contextmanager
def cycle_lock() -> Iterator[None]:
    """One cycle at a time; a second one fails at once instead of waiting."""
    fh = (_dir() / ".cycle.lock").open("a+")
    try:
        try:
            fcntl.flock(fh, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            raise LiveError("another live cycle is running (wait for it to finish, then run the next one)") from None
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)
    finally:
        fh.close()


def start(text: str = "") -> dict[str, Any]:
    """Mark the feed running (with a new source config when given) and return what each source should pull."""
    if text:
        save_sources(text)
    srcs = sources()
    if not srcs:
        raise LiveError("no live sources (fix: careerbrew work live start --sources <file.yaml>, or install a plugin with live sources)")
    st = _state()
    st["running"], st["started_at"] = True, _now()
    _save_state(st)
    events.emit("work.live.start", status="ok", sources=len(srcs), required=sum(1 for s in srcs if s["required"]))
    return {"sources": [{**s, "since": st["cursors"].get(s["name"], "")} for s in srcs]}


def stop() -> dict[str, Any]:
    st = _state()
    was = bool(st["running"])
    st["running"], st["stopped_at"] = False, _now()
    _save_state(st)
    events.emit("work.live.stop", status="ok", was_running=was)
    return {"was_running": was, "cycles": st["cycles"]}


# ---------- adapters ----------


def _file_stamp(path: Path) -> list[int]:
    st = path.stat()
    return [st.st_dev, st.st_ino, st.st_size, st.st_mtime_ns, st.st_ctime_ns]


def _read_inbox(src: dict[str, Any], seen: str | None) -> tuple[list[dict[str, Any]] | None, str | None, int, float | None]:
    """Validate current bytes even when unchanged; only a validated content/stat match skips intake."""
    from . import transcript

    path = Path(src["inbox"]).expanduser()
    if not path.is_file():
        return None, None, 0, None
    stamp = _file_stamp(path)
    payload = path.read_bytes()  # the agent's plaintext scratch reply, never an encrypted data file
    if _file_stamp(path) != stamp:
        raise LiveError("the inbox changed while reading (fix: finish saving the reply, then run the cycle)")
    fp = hashlib.sha256(json.dumps(stamp).encode() + payload).hexdigest()
    data = _reply_data(payload.decode("utf-8"))
    raw = feed.raw_events(data, src["name"])
    if raw is not None and (raw[0] or not raw[1]):
        items, skipped = raw[:2]
    else:
        items, skipped = [], 0
        for obj in transcript._candidates(data):
            ev = transcript._event(obj, src["name"])
            if ev is None:
                skipped += 1
            else:
                items.append(ev)
        if not items and data != []:
            raise LiveError("the inbox has no usable events (fix: save a supported quiet reply)")
    return (None if seen == fp else items), fp, skipped, stamp[3] / 1_000_000_000


def _reply_data(text: str) -> Any:
    """Strict JSON/JSONL and saved text blocks; parsing garbage never manufactures a quiet list."""
    from . import transcript

    for _ in range(8):
        try:
            data = json.loads(text)
        except json.JSONDecodeError:
            try:
                data = [json.loads(line) for line in text.splitlines() if line.strip()]
            except json.JSONDecodeError:
                raise LiveError("the inbox is not JSON (fix: save the tool's reply exactly as returned)") from None
            if not data:
                raise LiveError("the inbox is empty (fix: save a supported quiet reply)") from None
        if isinstance(data, dict) and (data.get("ok") is False or data.get("isError") is True or data.get("error")):
            raise LiveError("the inbox reports a tool error (fix: pull the source successfully again)")
        if isinstance(data, list) and data and all(isinstance(b, dict) and b.get("type") in {"text", "input_text"} for b in data):
            text = transcript._text(data)
            continue
        return data
    raise LiveError("the inbox has too many nested text wrappers (fix: save the tool reply directly)")


def _identity(src: dict[str, Any]) -> str:
    """Only a digest of source configuration goes in durable validation state."""
    bound = {**src, "max_age_minutes": src.get("max_age_minutes", config.get("live_source_max_age_minutes"))}
    if src["adapter"] == "file":
        bound["inbox"] = str(Path(src["inbox"]).expanduser().resolve())
    return hashlib.sha256(json.dumps(bound, sort_keys=True, separators=(",", ":")).encode()).hexdigest()


def _observe_clock(st: dict[str, Any], at: datetime) -> bool:
    """The data-root highwater never moves back; callers retain reversal for this cycle invocation."""
    try:
        previous = datetime.fromisoformat(st.get("checked_at") or at.isoformat())
        if previous.tzinfo is None or at < previous:
            return False
    except (ValueError, TypeError):
        return False
    st["checked_at"] = at.isoformat()
    return True


def _source_health(st: dict[str, Any], src: dict[str, Any], row: dict[str, Any], acquired: float | None, at: datetime, *, accept: bool = True, clock_valid: bool = True) -> None:
    """Data-root live receipts live until replacement/removal; the existing cycle lock owns all updates."""
    limit = src.get("max_age_minutes", config.get("live_source_max_age_minutes"))
    if type(limit) is not int or not 1 <= limit <= 10080:
        raise LiveError("live_source_max_age_minutes must be an integer from 1 to 10080")
    row.update(health="unknown", max_age_minutes=limit, checked_at=at.isoformat(), acquired_at="", age_seconds=None)
    if row["error"]:
        row["health"] = "error"
        return
    if src["adapter"] == "file" and acquired is None:
        row["health"] = "missing"
        return
    try:
        acquisition = datetime.fromtimestamp(acquired, UTC) if acquired is not None else at
    except (ValueError, TypeError, OverflowError, OSError):
        return
    age = (at - acquisition).total_seconds()
    row.update(acquired_at=acquisition.isoformat(), age_seconds=age)
    if age < 0 or not clock_valid:
        return
    if accept and row["stored"]:
        st["validated"][src["name"]] = {"identity": _identity(src), "fingerprint": row.get("fingerprint"), "acquired_at": acquisition.isoformat(), "accepted_at": at.isoformat()}
    receipt = st["validated"].get(src["name"], {})
    if receipt.get("identity") != _identity(src) or receipt.get("fingerprint") != row.get("fingerprint") or receipt.get("acquired_at") != acquisition.isoformat():
        return
    row["health"] = "healthy" if age <= limit * 60 else "stale"


def _read_connector(src: dict[str, Any]) -> list[dict[str, Any]]:
    from .. import connectors
    from . import evidence

    if src["connector"] not in connectors.FEED_READERS:
        raise LiveError(f"connector {src['connector']} has no feed reader (feed readers: {', '.join(connectors.FEED_READERS)})")
    cfg = next((c for c in evidence.sources_list() if c.get("kind") == src["connector"]), None)
    if cfg is None:
        raise LiveError(f"connector {src['connector']} is not connected (fix: careerbrew connect {src['connector']})")
    return [{**it, "source": src["name"]} for it in connectors.feed(cfg)]


# ---------- verdicts, PM rules, actions ----------


def reaches_pm(row: dict[str, Any]) -> bool:
    """The PM draft rule, the same test as `feed.evidence_items` (the PM draft's feed input): `feed.analyze`
    sets `evidence` only for your own action (triage `mine`) that a rule marks evidence and that is not personal."""
    return bool(row.get("evidence")) and not row.get("personal")


def verdict(row: dict[str, Any], open_ids: set[str]) -> str:
    """What to do with one stored event: answer an open ask, note it done, read it, skip it, or log it."""
    if row.get("personal"):
        return "personal"
    t = row.get("triage")
    if t == "act":
        return "answer" if row["id"] in open_ids else "done"
    if t == "fyi":
        return "read"
    if t == "noise":
        return "skip"
    return "evidence" if reaches_pm(row) else "log"


def _actions(not_stored: list[str], counts: dict[str, int], research: int, followups: list[dict[str, Any]] | None = None) -> list[str]:
    out = [f"pull {s} again, then run: careerbrew work live cycle" for s in not_stored]
    acts = [a for f in followups or [] for a in f["actions"]]
    replies, reviews = sum(a.startswith("reply draft saved") for a in acts), sum(a.startswith("review notes drafted") for a in acts)
    if replies or reviews:
        out.append(f"{replies} reply draft(s) and {reviews} review note(s) saved, never sent: read each in your terminal (careerbrew draft show <target>), rewrite it (careerbrew draft replace <target> --file <file>), then send it yourself")
    if opened := sum(a.startswith("review session opened") for a in acts):
        out.append(f"{opened} new review session(s): start a fresh reviewer for each (skill live-feed, Review sessions); it writes the review to a file and saves a reply draft, and must never post or send")
    if counts.get("answer"):
        out.append(f"answer {counts['answer']} open ask(s): careerbrew work feed show --open --show (to your terminal)")
    if research:
        out.append(f"research {research} link(s): careerbrew work feed links --pending")
    if counts.get("evidence"):
        out.append(f"{counts['evidence']} item(s) reach the PM draft: careerbrew day send --via email (review before you send)")
    return out


# ---------- the cycle ----------


def mark_cycle(
    st: dict[str, Any],
    srcs: list[dict[str, Any]],
    results: dict[str, dict[str, Any]],
    verdicts: dict[str, str],
    research: int,
    followups: list[dict[str, Any]] | None = None,
    goal_updates: list[dict[str, Any]] | None = None,
    clock_valid: bool = True,
) -> dict[str, Any]:
    """Close a cycle: `not_stored` is required unhealthy coverage; only actual stores move cursors."""
    from ..calendar import now_utc

    checked_at = now_utc()
    clock_valid = _observe_clock(st, checked_at) and clock_valid
    for src in srcs:
        row = results[src["name"]]
        _source_health(st, src, row, row["_acquired"], checked_at, accept=False, clock_valid=clock_valid)
    not_stored = [s["name"] for s in srcs if s["required"] and results[s["name"]]["health"] != "healthy"]
    counts = {v: sum(1 for x in verdicts.values() if x == v) for v in VERDICTS}
    by_name = {s["name"]: s for s in srcs}
    for name, r in results.items():
        if r["stored"]:
            if r.get("cursor"):
                st["cursors"][name] = max(st["cursors"].get(name, ""), r["cursor"])
            if r.get("fingerprint"):
                st["seen"][name] = {"identity": _identity(by_name[name]), "fingerprint": r["fingerprint"]}
    st["cycles"] += 1
    rec = {
        "n": st["cycles"],
        "at": checked_at.isoformat(),
        "status": "failed" if not_stored else "ok",
        "not_stored": not_stored,
        "degraded": [s["name"] for s in srcs if not s["required"] and results[s["name"]]["health"] != "healthy"],
        "sources": {k: {**{x: v for x, v in r.items() if x not in {"fingerprint", "_acquired"}}, "source_identity": _identity(by_name[k])} for k, r in results.items()},
        "verdicts": verdicts,
        "counts": counts,
        "followups": followups or [],
        "goals": goal_updates or [],
        "actions": _actions(not_stored, counts, research, followups),
        "review_sessions": sum(a.startswith("review session opened") for f in followups or [] for a in f["actions"]),
    }
    st["last"] = {k: rec[k] for k in ("n", "at", "status", "not_stored", "counts")}
    fresh = _state()  # a start or stop during the cycle wins: they do not wait for the cycle lock
    st.update({k: fresh[k] for k in ("running", "started_at", "stopped_at") if k in fresh})
    _save_state(st)
    rows = store.read_jsonl(cycles_path())[-(KEEP_CYCLES - 1) :] + [rec]
    store.write_jsonl(cycles_path(), rows)
    events.emit("work.live.cycle", status=rec["status"], sources=len(srcs), not_stored=len(not_stored), stored=sum(r["new"] + r["updated"] for r in results.values()), story="us-97")
    return rec


def cycle(read_allowed: bool = False) -> dict[str, Any]:
    """One cycle: lock, pull, store, triage, verdict per event, follow-up work per new event (issue #266,
    `listen.py`: drafts only), one update per goal for today, actions, then mark the cycle.
    `read_allowed`: the caller may read business data, so the cycle may open links (the CLI's `may_show_business()`)."""
    from .. import privacy

    with cycle_lock():
        st = _state()
        if not st["running"]:
            raise LiveError("the live feed is stopped (fix: careerbrew work live start)")
        srcs = sources()
        results: dict[str, dict[str, Any]] = {}
        stored_ids: list[str] = []
        from ..calendar import now_utc

        clock_valid = _observe_clock(st, now_utc())
        for s in srcs:
            r: dict[str, Any] = {"pulled": False, "stored": False, "items": 0, "new": 0, "updated": 0, "skipped": 0, "error": "", "unchanged": False, "validated_at": ""}
            acquired = None
            validation_at = None
            try:
                if s["adapter"] == "file":
                    binding = st["seen"].get(s["name"])
                    seen = binding.get("fingerprint") if isinstance(binding, dict) and binding.get("identity") == _identity(s) else None
                    items, fp, r["skipped"], acquired = _read_inbox(s, seen)
                    r["fingerprint"] = fp
                    r["unchanged"] = items is None and fp is not None
                else:
                    items = _read_connector(s)
                validation_at = now_utc()
                clock_valid = _observe_clock(st, validation_at) and clock_valid
                if items is not None or r["unchanged"]:
                    r["validated_at"] = validation_at.isoformat()
                if s["adapter"] == "connector":
                    acquired = validation_at.timestamp()
                if items is not None:
                    r["pulled"], r["items"] = True, len(items)
                    got = feed.ingest(items, s["origin"], source_kind=s["kind"]) if items else {"new": 0, "updated": 0}
                    r["new"], r["updated"], r["stored"] = got["new"], got["updated"], True
                    stored_ids += [feed.clean(it)["id"] for it in items]
                    r["cursor"] = max((feed.clean(it)["at"] for it in items), default="")
            except (RuntimeError, OSError, KeyError, ValueError, TypeError) as exc:  # LiveError, FeedError, ConnectorError
                r["error"] = privacy.redact(str(exc))[:200]
                r["error_kind"] = type(exc).__name__
                events.emit("work.live.error", source_kind=s["kind"], error=type(exc).__name__, story="us-97")
            if validation_at is None:
                validation_at = now_utc()
                clock_valid = _observe_clock(st, validation_at) and clock_valid
            r["_acquired"] = acquired
            _source_health(st, s, r, acquired, validation_at, clock_valid=clock_valid)
            results[s["name"]] = r
        rows = {row["id"]: row for row in feed.load()}
        mine = [rows[i] for i in dict.fromkeys(stored_ids) if i in rows]
        open_ids = {row["id"] for row in feed.open_asks(list(rows.values()), include_personal=True)}
        verdicts = {row["id"]: verdict(row, open_ids) for row in mine}
        followups, goal_rows = _listen(st, verdicts, read_allowed)
        fresh = {row["id"]: row for row in feed.load()}
        research = sum(len(feed.needs_research(fresh.get(row["id"], row))) for row in mine if row["triage"] != "noise" and not row.get("personal"))
        published = _standup(st, mine)
        rec = mark_cycle(st, srcs, results, verdicts, research, followups, goal_rows, clock_valid)
        rec["standup"] = published
        return rec


def _standup(st: dict[str, Any], new_rows: list[dict[str, Any]]) -> dict[str, Any]:
    """Republish the standup rows when a new event matches a row, the last publish is stale, or the standup is next (issue #315).

    The publish is the manual run's pipeline (`standup.run`: its proof gate decides each row); a failure is reported, never raised."""
    from .. import calendar  # local: calendar reaches connectors, which reach work
    from . import standup

    try:
        saved = standup.load_rows()
        if not saved.get("rows"):
            return {"status": "skipped", "reason": "no rows"}
        reason = standup.due(new_rows, saved, str(st.get("standup_at") or ""))
        if not reason:
            return {"status": "skipped", "reason": "fresh"}
        r = standup.run()
        st["standup_at"] = calendar.now().isoformat(timespec="seconds")
        return {"status": "published", "rows": len(r.get("rows") or []), "reason": reason}
    except Exception as exc:  # noqa: BLE001 - a failed publish must never stop the cycle (issue #315)
        events.emit("work.live.error", source_kind="standup", error=type(exc).__name__, story="us-97")
        return {"status": "failed", "error": type(exc).__name__}


def standup_line(r: dict[str, Any]) -> str:
    if r["status"] == "published":
        return f"standup: {r['rows']} rows published ({r['reason']})"
    if r["status"] == "failed":
        return f"standup: failed ({r['error']}); the cycle goes on"
    return f"standup: skipped, {r['reason']}"


def _listen(st: dict[str, Any], verdicts: dict[str, str], read_allowed: bool) -> tuple[list[dict[str, Any]], list[dict[str, Any]]]:
    """The follow-up work and the goal lines; a failure here is logged and leaves the stored events and the cycle mark intact."""
    from . import listen

    followups: list[dict[str, Any]] = []
    try:
        followups, attached = listen.follow_up(verdicts, set(st["followed"]), read_allowed)
        st["followed"] = [*st["followed"], *(f["id"] for f in followups)][-listen.KEEP_FOLLOWED :]
        st["attached"] = dict(list({**st["attached"], **attached}.items())[-listen.KEEP_FOLLOWED :])
        return followups, listen.goal_updates(st["attached"])
    except Exception as exc:  # noqa: BLE001 - follow-up work must never stop the cycle mark (issue #266)
        events.emit("work.live.error", source_kind="listen", error=type(exc).__name__, story="us-101")
        return followups, []


def status() -> dict[str, Any]:
    from . import health

    st = _state()
    srcs = sources()
    last = (store.read_jsonl(cycles_path()) or [{}])[-1]
    per = last.get("sources") or {}
    return {
        "running": st["running"],
        "started_at": st.get("started_at", ""),
        "cycles": st["cycles"],
        "last": st.get("last") or {},
        "actions": last.get("actions") or [],
        "health": health.snapshot(),
        "sources": [
            {"name": s["name"], "kind": s["kind"], "required": s["required"], "adapter": s["adapter"], "origin": s["origin"], "since": st["cursors"].get(s["name"], ""), "last_stored": (per.get(s["name"]) or {}).get("stored", False)}
            for s in srcs
        ],
    }
