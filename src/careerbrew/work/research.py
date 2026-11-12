"""Research for work projects: absorb what the user hands over, and refresh stale sources on checkup.

absorb(<link | file | task id | #channel>) detects the kind, reads it through the matching
connector, adds it as a source with `checked_at`, and appends a short dated summary to the
project memory (the `--memory` path the user chose). refresh() does the same for every stale
source: what changed since `checked_at`, plus proposed new sources (for example child epics).

Privacy: summaries go only to the memory path; the encrypted record keeps refs, dates, and a
one-line status. When reading business data is off (`read=False`: no terminal and no plugin
`agent_may_read_business`), nothing is read: absorb stores the ref and date only, refresh leaves
the source stale with that reason. A reference (a link that opens a sign-in page, a url on a host
outside `allowed_hosts`, or a source the user marked) is never read and never stale (issues #70, #101);
a url the agent reads (`agent_reads: [url]`) is never fetched. Every request goes through the connector host policy.
A memory source is read by `memory_sync` (it updates the project from the file) and never copied into
itself; its status stays the one from the file. Every memory write goes through `memory_sync.append`.
Summaries are deterministic (key lines and counts), so nothing leaves the machine to summarize.

changes() answers `work project check <name> --since` (issue #109): for each source, did it change
since its own `checked_at`? It stamps nothing and keeps nothing: a local file's modified time, or
the connector's change list reduced to a count. With reading off, only local files are compared.
"""

from __future__ import annotations

import http.client
import re
import urllib.error
import urllib.parse
from datetime import UTC, datetime, timedelta
from pathlib import Path
from typing import Any

from .. import calendar, connectors, events, paths, secrets, store
from . import evidence, projects

KEY_LINE = re.compile(r"\b(decid\w*|agreed|scope|deadline|due|launch|ship|blocked|risk|owner|action|todo|open question|\d{4}-\d{2}-\d{2})\b|\?\s*$", re.I)
MAX_NOTES = 8
NO_CHANGE_DATE = ("url",)  # a web page carries no change date, so `changes` never fetches one
READING_OFF = "reading business data is off here (run it in your terminal, or a plugin sets agent_may_read_business)"


class ReadError(RuntimeError):
    """A source could not be read; the message is the reason recorded on it."""


def github_item(ref: str) -> list[str] | None:
    """The owner/repo/item path for GitHub PR and issue links, else None."""
    u = urllib.parse.urlsplit(ref)
    parts = [part for part in u.path.split("/") if part]
    if (u.hostname == "github.com" or (u.hostname or "").startswith("github.")) and len(parts) >= 4 and parts[2] in ("pull", "issues") and parts[3].isdigit():
        return parts[:4]
    return None


def detect(ref: str) -> tuple[str, str]:
    """(kind, normalized ref) for anything the user hands over."""
    r = ref.strip()
    if re.fullmatch(r"t\d{3,}", r):
        return "task", r
    if r.startswith("#") and re.fullmatch(r"#[\w.-]+", r):
        return "chat", r[1:] if re.fullmatch(r"#[CGD][A-Z0-9]+", r) else r
    if re.fullmatch(r"[A-Z][A-Z0-9]+-\d+", r):
        return "tracker", r
    if "://" in r:
        u = urllib.parse.urlsplit(r)
        if m := re.search(r"/browse/([A-Z][A-Z0-9]+-\d+)", u.path):
            return "tracker", m.group(1)
        host = u.hostname or ""
        if host == "github.com" or host.startswith("github."):
            parts = [p for p in u.path.split("/") if p]
            if item := github_item(r):
                return "url", u._replace(path="/" + "/".join(item), query="", fragment="").geturl()
            if len(parts) >= 2:
                return "repo", f"{parts[0]}/{parts[1]}"
        if (u.hostname or "") == "docs.google.com" and (m := re.search(r"/d/([\w-]+)", u.path)):
            return "doc", m.group(1)
        if (u.hostname or "").endswith("slack.com") and (m := re.search(r"/archives/(\w+)", u.path)):
            return "chat", m.group(1)
        return "url", r
    if Path(r).expanduser().is_file():
        return "doc", str(Path(r).expanduser().resolve())
    if re.fullmatch(r"[\w.-]+/[\w.-]+", r):
        return "repo", r
    raise ReadError(f"cannot tell what {r!r} is: give a link, a file path, a task id (t001), a #channel, a ticket key, or owner/repo")


def _tokens(kind: str, ref: str) -> set[str]:
    """What makes two refs related: the repo owner, the tracker project key, the host, the channel."""
    if kind == "tracker":
        return {"tracker:" + ref.split("-")[0]}
    if kind == "repo":
        return {"repo:" + ref.split("/")[0].lower(), "repo:" + ref.lower()}
    if kind == "url":
        u = urllib.parse.urlsplit(ref)
        tokens = {"host:" + (u.hostname or "")}
        if item := github_item(ref):
            tokens |= _tokens("repo", "/".join(item[:2]))
        return tokens
    if kind == "chat":
        return {"chat:" + ref.lower()}
    return {f"{kind}:{ref}"}


def match_project(kind: str, ref: str, text: str = "") -> str:
    """The closest project; refuses on no match or a tie, naming the candidates."""
    mine = _tokens(kind, ref)
    scores: dict[str, int] = {}
    for name, p in projects._load().items():
        if p.get("archived_at"):
            continue  # an archived project takes nothing new (issue #173); name it to absorb into it anyway
        score = sum(len(mine & _tokens(s["kind"], s["ref"])) for s in p["sources"])
        if name in ref.lower() or (text and re.search(rf"\b{re.escape(name)}\b", text, re.I)):
            score += 1
        if score:
            scores[name] = score
    if not scores:
        raise projects.ProjectError(f"no project matches {ref} (fix: name it: careerbrew work project absorb <project> {ref})")
    best = max(scores.values())
    top = sorted(n for n, s in scores.items() if s == best)
    if len(top) > 1:
        raise projects.ProjectError(f"{ref} matches {', '.join(top)} equally (fix: name one: careerbrew work project absorb <project> {ref})")
    return top[0]


def _connector(kind: str) -> dict[str, Any]:
    allowed = connectors.plugins.effective_policy().get("allowed_connectors")
    if allowed is not None and kind not in allowed:
        raise ReadError(f"{kind} is not in the plugin policy allowed_connectors")
    for c in evidence.sources_list():
        if c.get("kind") == kind:
            return c
    raise ReadError(f"no {kind} connector (fix: careerbrew connect {kind})")


def _key_lines(text: str) -> list[str]:
    lines = [ln.strip(" -*#\t") for ln in text.splitlines()]
    return [ln[:200] for ln in lines if ln and KEY_LINE.search(ln)][:MAX_NOTES]


def _after(stamp: Any, since: str | None) -> bool:
    """True when `stamp` is later than `since` (no `since`: everything is new). Times, not strings, are
    compared, since tools write different offsets; a stamp it cannot parse counts as new, so no change is missed."""
    if not since:
        return True
    try:
        a, b = (datetime.fromisoformat(str(x)) for x in (stamp, since))
    except ValueError:
        return True
    a, b = (x if x.tzinfo else x.replace(tzinfo=UTC) for x in (a, b))
    return a > b


def _search_day(since: str | None, back: int) -> str | None:
    """The UTC day of `since`, `back` days earlier, for a search that filters by whole day. Slack and Jira read a
    bare date in the searcher's own time zone (up to a day off UTC), and Slack's `after:` leaves out the named day;
    so the window opens early and `_after` keeps only what is later than `since`. None: no date filter."""
    if not since:
        return None
    try:
        t = datetime.fromisoformat(since)
    except ValueError:
        return None
    return ((t if t.tzinfo else t.replace(tzinfo=UTC)).astimezone(UTC).date() - timedelta(days=back)).isoformat()


def _ts(stamp: Any) -> str:
    """A Slack `ts` (seconds since 1970) as an ISO time; "" when it is not one, so `_after` counts it as new."""
    try:
        return datetime.fromtimestamp(float(stamp), UTC).isoformat()
    except (TypeError, ValueError, OverflowError, OSError):
        return ""


def _file_changes(kind: str, ref: str, since: str | None) -> int | None:
    """1 when a local doc or memory file changed after `since`, 0 when not (or not written yet), None when
    the source is not a local file. It reads the file's modified time only."""
    if kind not in ("doc", "memory"):
        return None
    p = _local(ref)
    if kind == "memory" and not p.exists():
        return 0
    if not p.is_file():
        return None
    return int(_after(datetime.fromtimestamp(p.stat().st_mtime, UTC).isoformat(timespec="seconds"), since))


def read(kind: str, ref: str, since: str | None) -> dict[str, Any]:
    """{"title", "notes": [...], "proposed": [(kind, ref)], "changed": how many changes since `since`, or None
    when the kind has no change date}; raises ReadError with the reason."""
    try:
        return _read(kind, ref, since)
    except ReadError:
        raise
    except connectors.ConnectorError as e:
        raise ReadError(str(e)) from None
    except secrets.SecretError as e:
        raise ReadError(str(e)) from None
    except urllib.error.HTTPError as e:
        raise ReadError(f"{kind} answered {e.code}") from None
    except (OSError, ValueError, KeyError, http.client.HTTPException) as e:  # HTTPException: IncompleteRead, BadStatusLine, LineTooLong
        raise ReadError(f"{kind} could not be read ({type(e).__name__})") from None


def _memory_synced() -> bool:
    from . import memory_sync  # local import: memory_sync reads this module's detect()

    return memory_sync.enabled()


def _local(ref: str) -> Path:
    p = Path(ref).expanduser()
    return p if p.is_absolute() else paths.data_root() / p


def _read(kind: str, ref: str, since: str | None) -> dict[str, Any]:
    if why := projects.no_reader(ref, kind):
        raise ReadError(why)  # fetching it would read a sign-in page, not the content (issue #70)
    if projects.agent_reads(kind) and not projects.local_file(kind, ref):
        raise ReadError(f"the agent reads {kind} sources itself (plugin agent_reads), so careerbrew never fetches them")  # issues #101, #184
    if kind == "memory" and _memory_synced():
        from . import memory_sync

        memory_sync.sync_in()  # it reads the file into the project; key lines copied into the same file would loop
        return {"title": Path(ref).name, "notes": [], "proposed": [], "changed": _file_changes(kind, ref, since)}
    if (changed := _file_changes(kind, ref, since)) is not None:
        if not changed:  # unchanged, or memory notes not written yet: nothing to read
            return {"title": Path(ref).name, "notes": [], "proposed": [], "changed": 0}
        p = _local(ref)
        text = store.read_text(p) if p.is_relative_to(paths.data_root()) else p.read_text(encoding="utf-8", errors="replace")
        return {"title": p.name, "notes": _key_lines(text or ""), "proposed": [], "changed": 1}
    if kind == "tracker":
        c = _connector("jira")
        base, auth = c["base_url"].rstrip("/"), connectors._jira_auth(c.get("email", ""), secrets.resolve(c["token"]))
        issue = connectors.http_json(f"{base}/rest/api/3/issue/{ref}?fields=summary,status,updated,duedate", {"Authorization": auth})
        f = issue.get("fields", {})
        notes = [f"status: {(f.get('status') or {}).get('name', '?')}; updated {str(f.get('updated', ''))[:10]}" + (f"; due {f['duedate']}" if f.get("duedate") else "")]
        day = _search_day(since, 1)  # JQL `updated >= day` starts at the day's midnight in the Jira user's time zone
        jql = urllib.parse.quote(f"parent = {ref}" + (f" AND updated >= '{day}' ORDER BY updated DESC" if day else ""))
        kids = connectors.http_json(f"{base}/rest/api/3/search/jql?jql={jql}&fields=summary,status,updated&maxResults=50", {"Authorization": auth}).get("issues", [])
        kids = [k for k in kids if _after((k.get("fields") or {}).get("updated"), since)]  # a child's update does not move its parent's
        notes += [f"child {k['key']}: {(k.get('fields', {}).get('status') or {}).get('name', '?')}" for k in kids[:MAX_NOTES]]
        changed = int(_after(f.get("updated"), since)) + (len(kids) if since else 0)
        titles = {k["key"]: str((k.get("fields") or {}).get("summary") or "") for k in kids}  # a proposal shows its ticket's title (issue #147)
        return {"title": f.get("summary", ref), "notes": notes, "proposed": [("tracker", k["key"]) for k in kids], "titles": titles, "changed": changed}
    if kind == "repo":
        c = _connector("github")
        api = c.get("api", "https://api.github.com").rstrip("/")
        pulls = connectors.http_json(f"{api}/repos/{ref}/pulls?state=all&sort=updated&direction=desc&per_page=50", {"Authorization": f"Bearer {secrets.resolve(c['token'])}"})
        fresh = [pr for pr in pulls if not since or str(pr.get("updated_at", "")) > since]
        merged = [pr for pr in fresh if pr.get("merged_at")]
        notes = [f"{len(fresh)} PR(s) updated, {len(merged)} merged"] + [f"PR #{pr['number']} {'merged' if pr.get('merged_at') else pr.get('state', '')}: {pr.get('title', '')[:120]}" for pr in fresh[: MAX_NOTES - 1]]
        return {"title": ref, "notes": notes, "proposed": [], "changed": len(fresh)}
    if kind == "doc":
        c = _connector("gdocs")
        tok = {"Authorization": f"Bearer {secrets.resolve(c['token'])}"}
        meta = connectors.http_json(f"https://www.googleapis.com/drive/v3/files/{ref}?fields=name,modifiedTime", tok)
        comments = connectors.http_json(f"https://www.googleapis.com/drive/v3/files/{ref}/comments?fields=comments(content,modifiedTime,resolved)&pageSize=50", tok).get("comments", [])
        fresh = [x for x in comments if not since or str(x.get("modifiedTime", "")) > since]
        notes = [f"modified {str(meta.get('modifiedTime', ''))[:10]}; {len(fresh)} new comment(s), {sum(not x.get('resolved') for x in fresh)} open"]
        notes += [f"comment: {x.get('content', '')[:160]}" for x in fresh if not x.get("resolved")][: MAX_NOTES - 1]
        return {"title": meta.get("name", ref), "notes": notes, "proposed": [], "changed": int(_after(meta.get("modifiedTime"), since)) + len(fresh)}
    if kind == "chat":
        c = _connector("slack")
        day = _search_day(since, 2)  # `after:` leaves out the named day, read in the searcher's own time zone
        q = f"in:{ref}" + (f" after:{day}" if day else "")
        data = connectors.http_json(f"https://slack.com/api/search.messages?query={urllib.parse.quote(q)}&count=50&sort=timestamp", {"Authorization": f"Bearer {secrets.resolve(c['token'])}"})
        if not data.get("ok", False):
            raise ReadError(f"slack error: {data.get('error', 'unknown')}")
        matches = [m for m in (data.get("messages") or {}).get("matches", []) if _after(_ts(m.get("ts")), since)]
        return {"title": ref, "notes": [f"{len(matches)} message(s) since the last check"] + [m.get("permalink", "") for m in matches[: MAX_NOTES - 1] if m.get("permalink")], "proposed": [], "changed": len(matches)}
    if kind == "url":
        connectors.check_host(ref)
        u = urllib.parse.urlsplit(ref)
        if parts := github_item(ref):
            c = _connector("github")
            api = c.get("api", "https://api.github.com").rstrip("/")
            api_host = urllib.parse.urlsplit(api).hostname
            if api_host != u.hostname and (u.hostname, api_host) != ("github.com", "api.github.com"):
                raise ReadError("the GitHub connector host does not match this item host")
            endpoint = "pulls" if parts[2] == "pull" else "issues"
            item = connectors.http_json(f"{api}/repos/{parts[0]}/{parts[1]}/{endpoint}/{parts[3]}", {"Authorization": f"Bearer {secrets.resolve(c['token'])}"})
            state = "merged" if endpoint == "pulls" and item.get("merged_at") else item.get("state", "unknown")
            label = "PR" if endpoint == "pulls" else "issue"
            changed = int(_after(item.get("updated_at"), since))
            return {"title": item.get("title", ref), "notes": [f"{label} #{parts[3]} {state}: {item.get('title', '')[:120]}"] if changed else [], "proposed": [], "changed": changed}
        html = connectors.http_get(ref).decode("utf-8", errors="replace")
        title = re.search(r"<title>(.*?)</title>", html, re.S | re.I)
        text = re.sub(r"<[^>]+>", "\n", html)
        return {"title": (title.group(1).strip() if title else ref)[:120], "notes": _key_lines(text), "proposed": [], "changed": None}
    raise ReadError(f"no reader for {kind}")


def write_memory(name: str, header: str, notes: list[str]) -> bool:
    """Append a dated section to the project's notes (its own, or the nearest parent's). Under the data root it stays encrypted.
    It goes through memory_sync, which records the hash it wrote so the watcher never reads it back."""
    from . import memory_sync  # local import: memory_sync reads this module's detect()

    every = projects._load()
    mem = projects.notes_path(name, every)
    if not mem or not notes:
        return False
    if not every[name].get("memory"):
        header += f" (project {name})"  # a parent's notes file: say which project these notes are for
    return memory_sync.append(mem, header, notes)


def absorb(ref: str, name: str | None = None, read_allowed: bool = True) -> dict[str, Any]:
    kind, norm = detect(ref)
    text = ""
    if kind == "doc" and Path(norm).is_file() and read_allowed:
        text = Path(norm).read_text(encoding="utf-8", errors="replace")[:20000]
    name = name or match_project(kind, norm, text)
    if kind == "task":
        projects.link_task(name, norm)
        return {"project": name, "kind": "task", "ref": norm, "read": False, "notes": 0, "memory": False}
    all_projects = projects._load()
    p = projects._get(all_projects, name)
    if not any(s["kind"] == kind and s["ref"] == norm for s in p["sources"]):
        p = projects.add_source(name, kind, norm)
    result = {"project": name, "kind": kind, "ref": norm, "read": False, "notes": 0, "memory": False, "reason": ""}
    src = next(s for s in p["sources"] if s["kind"] == kind and s["ref"] == norm)
    if why := projects.reference_reason(src):
        result["reason"] = f"kept as a reference ({why}): careerbrew never reads it and it never goes stale"
        return result
    if not read_allowed:
        projects.check(name, norm, "absorbed: ref only (reading business data is off)")
        result["reason"] = "reading business data is off here: stored the ref and date only (run it in your terminal, or a plugin sets agent_may_read_business)"
        return result
    try:
        got = read(kind, norm, None)
    except ReadError as e:
        projects.mark_unreadable(name, norm, str(e))
        result["reason"] = str(e)
        return result
    stamp = calendar.now_utc().date().isoformat()
    result.update(read=True, notes=len(got["notes"]), memory=write_memory(name, f"{stamp} {kind} {norm}: {got['title'][:80]}", got["notes"]))
    projects.check(name, norm, f"absorbed: {len(got['notes'])} note(s)")
    events.emit("work.project", status="ok", action="absorb", count=len(got["notes"]))
    return result


def refresh(read_allowed: bool = True) -> dict[str, Any]:
    """Read every stale source since its checked_at; notes to memory, then stamp. Unreadable ones stay stale.

    A source checked today since the list was read (another `/work` step stamped it, issue #258) is skipped."""
    projects.normalize_sources()
    done = stale = skipped = 0
    today = calendar.now_utc().date().isoformat()
    proposed: list[dict[str, str]] = []
    for s in projects.stale_sources():
        p = projects._get(projects._load(), s["project"])
        src = next(x for x in p["sources"] if x["kind"] == s["kind"] and x["ref"] == s["ref"])
        if str(src.get("checked_at") or "")[:10] == today:
            skipped += 1
            continue
        if not read_allowed:
            projects.mark_unreadable(s["project"], s["ref"], "reading business data is off here (run checkup in your terminal)")
            stale += 1
            continue
        try:
            got = read(s["kind"], s["ref"], src.get("checked_at"))
        except ReadError as e:
            projects.mark_unreadable(s["project"], s["ref"], str(e))
            stale += 1
            continue
        stamp = calendar.now_utc().date().isoformat()
        write_memory(s["project"], f"{stamp} {s['kind']} {s['ref']}: changes since {str(src.get('checked_at') or 'first read')[:10]}", got["notes"])
        synced = s["kind"] == "memory" and _memory_synced()  # its status comes from the file's own `status:` line
        projects.check(s["project"], s["ref"], "" if synced else f"refreshed: {len(got['notes'])} note(s)")
        known = {(x["kind"], x["ref"]) for x in p["sources"]}
        for k, r in got["proposed"]:
            if (k, r) not in known:
                proposed.append({"project": s["project"], "kind": k, "ref": r, "title": got.get("titles", {}).get(r, ""), "parent": s["ref"]})
        done += 1
    if proposed:
        projects.add_proposals(proposed)
    events.emit("work.project", status="ok", action="refresh", count=done)
    return {"refreshed": done, "still_stale": stale, "skipped": skipped, "proposed": proposed}


def changes(name: str, ref: str | None = None, read_allowed: bool = True) -> list[dict[str, Any]]:
    """What changed in each source of a project (or one source) since its own `checked_at`, stamping nothing.

    One row per source: {kind, ref, since, state, count, reason}. `state` is "changed" (`count` > 0),
    "unchanged", "never read", "unknown" (`reason`: the kind has no change date, a read error, or reading
    is off), or "reference" (never read, never stale). Only dates and counts are kept; the event has counts only.
    """
    p = projects._get(projects._load(), name)
    hits = projects.select_sources(p["sources"], ref)
    if not hits:
        raise projects.ProjectError(f"{name} has no source {ref} (see `careerbrew work project show {name}`)" if ref else f"{name} has no sources yet (fix: `careerbrew work project source {name} <kind> <ref>`)")
    rows: list[dict[str, Any]] = []
    for s in hits:
        kind, since = s["kind"], s.get("checked_at")
        row: dict[str, Any] = {"kind": kind, "ref": s["ref"], "since": since, "count": None, "reason": ""}
        if why := projects.reference_reason(s):
            row.update(state="reference", reason=why)
        elif not since:
            row["state"] = "never read"
        else:
            n = _file_changes(kind, s["ref"], since)
            if n is None and kind in NO_CHANGE_DATE:
                row["reason"] = f"a {kind} source has no change date: read it"
            elif n is None and not read_allowed:
                row["reason"] = READING_OFF
            elif n is None:
                try:
                    n = read(kind, s["ref"], since).get("changed")
                except ReadError as e:
                    row["reason"] = str(e)
            row.update(state="unknown" if n is None else "changed" if n else "unchanged", count=n)
        rows.append(row)
    events.emit("work.project", status="ok", action="since", count=sum(r["state"] == "changed" for r in rows))
    return rows
