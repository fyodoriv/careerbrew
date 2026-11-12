"""The day inbox: proposals careerbrew makes from new context, and questions it asks to learn more.

Stored in `<data root>/day/inbox.json` (encrypted at rest). `regenerate()` runs after each watcher
cycle that saw something new; items have stable ids, so a dismissed or answered item never comes back.

Proposals (the owner accepts or dismisses each; nothing changes on its own):
  agent-task   a task an agent added (with its why); shows at once     -> planned only once accepted
  plugin       a plugin's own suggestion: what, why, and on-accept text -> adds its task, if it names one
               (`careerbrew day suggest`; stored in `day/suggestions.jsonl`, shows at once)
  task-done    an open task whose words match work shipped today       -> marks it done
  new-task     a ticket assigned to you that no task tracks             -> adds a task
  source       a related source a project refresh found (child issue)   -> adds it to the project
  goal         a promotion gap, or the current goal's date has passed   -> adds an upcoming goal
               (web UI only: only the owner sets goals, see goals.py)
Each proposal says where it came from (`added_by`), its links, where Accept lands it, its project and that
project's rank in `project_priority`, and what Accept and Dismiss change (`explain`, `describe`; issue #147).
`inbox_group_min` or more alike proposals (same kind, project, and source) form one group: Accept all and
Dismiss all decide each of them (`decide_group`; both ask to confirm, and decide only the rows the owner confirmed). An ask that data careerbrew already
reads shows done elsewhere expires (status `expired`, kept, never deleted; `reopen` brings it back), or
says "maybe done" when unsure (`done_elsewhere.py`).
Questions (the answer is applied):
  impact       shipped today with no measured impact     -> an evidence note with the impact
  project      a ticket or repo that no project lists    -> added to the named project
  competency   an open task with no competency           -> task competencies (promotion bonus)
  memory       a root project with no notes path         -> the project's memory path; its children
               (asked once per project tree)                use it unless they have their own
"""

from __future__ import annotations

import contextlib
import fcntl
import re
from collections.abc import Iterator
from datetime import date, timedelta
from typing import Any

import yaml

from .. import calendar, config, connectors, events, goals, paths, plugins, secrets, signals, store
from ..work import evidence, priorities, project_priority, projects, promo
from . import done_elsewhere, results

SHIPPED = {"commit", "pr", "github", "jira"}
GOAL_ONLY_IN_UI = "goal"
TEXT_MAX = 500  # a title shows whole, never cut (issue #147); this bounds only the stored copy
EXPIRED_SHOWN = 100  # the newest expired suggestions the web app lists under "Done elsewhere"


def _full(text: Any) -> str:
    return " ".join(str(text or "").split())[:TEXT_MAX]


class InboxError(ValueError):
    pass


def path():
    return paths.sub("day", "inbox.json")


def load() -> list[dict[str, Any]]:
    return (store.read_json(path(), {}) or {}).get("items", [])


def _save(items: list[dict[str, Any]]) -> None:
    store.write_json(path(), {"items": items})


@contextlib.contextmanager
def _locked() -> Iterator[None]:
    """One writer at a time (the watcher, the web app, the CLI): read-modify-write under a file lock."""
    with paths.sub("day", "inbox.lock").open("a") as fh:
        fcntl.flock(fh, fcntl.LOCK_EX)
        try:
            yield
        finally:
            fcntl.flock(fh, fcntl.LOCK_UN)


def _id(kind: str, ref: str) -> str:
    return f"{kind}-{store.digest(kind, ref)[:8]}"


def _words(text: str) -> set[str]:
    return {w for w in re.findall(r"[a-z0-9]{4,}", text.lower()) if w not in {"with", "from", "that", "this", "into", "feat", "fix", "chore"}}


def _refs(text: str) -> list[tuple[str, str]]:
    return [("tracker", k) for k in re.findall(r"\b[A-Z][A-Z0-9]+-\d+\b", text)]


def _assigned_tickets() -> list[tuple[str, str]]:
    """Open tickets assigned to you (Jira), as (key, summary). Empty when no Jira connector or on error."""
    for c in evidence.sources_list():
        if c.get("kind") != "jira":
            continue
        try:
            base = c["base_url"].rstrip("/")
            auth = connectors._jira_auth(c.get("email", ""), secrets.resolve(c["token"]))
            jql = "assignee = currentUser() AND statusCategory != Done"
            data = connectors.http_json(f"{base}/rest/api/3/search/jql?jql={jql.replace(' ', '%20')}&fields=summary&maxResults=50", {"Authorization": auth})
        except Exception:  # noqa: BLE001 (a broken connector must not stop the inbox)
            return []
        return [(i["key"], (i.get("fields") or {}).get("summary", "")) for i in data.get("issues", [])]
    return []


def candidates(day: date | None = None) -> list[dict[str, Any]]:
    day = day or results.today()
    rows = [r for r in results.load(day) if r.get("id") and str(r.get("title") or "").strip()]  # skip malformed rows
    shipped = [r for r in rows if r.get("kind") in SHIPPED]
    tasks = [t for t in priorities.load() if priorities.is_open(t)]
    out: list[dict[str, Any]] = []

    worked = [r for r in rows if r.get("kind") in SHIPPED | {"note"}]  # a note saying you finished it counts too
    for t in tasks:
        tw = _words(t["title"])
        for r in worked:
            if t["id"] in r.get("title", "") or (tw and len(tw & _words(r.get("title", ""))) / len(tw) >= 0.6):
                out.append(
                    {
                        "id": _id("task-done", t["id"]),
                        "type": "proposal",
                        "kind": "task-done",
                        "text": f"Mark {t['id']} done: matching work shipped today ({_full(r['title'])})",
                        "why": f'today you shipped "{_full(r["title"])}", which matches task {t["id"]} ({_full(t["title"])})',
                        "data": {"task": t["id"], "title": _full(t["title"])},
                    }
                )
                break

    out += [_agent_task_item(t) for t in priorities.load() if t.get("status") == "proposed"]
    out += [_feed_rule_item(p) for p in signals.feed_rule_proposals()]
    out += [_plugin_item(r) for r in _suggestion_rows(installed_only=True)]

    tracked = " ".join(t["title"] for t in tasks)
    for key, summary in _assigned_tickets():
        if key not in tracked:
            out.append(
                {
                    "id": _id("new-task", key),
                    "type": "proposal",
                    "kind": "new-task",
                    "text": f"Add a task for {key}, assigned to you: {_full(summary)}",
                    "why": f"{key} is assigned to you in Jira and still open, but no task in Week tracks it",
                    "data": {"title": _full(f"{key}: {summary}"), "key": key},
                }
            )

    for name, p in projects._load().items():
        for x in [] if p.get("archived_at") else p.get("proposed", []):
            out.append(_source_item(name, x))

    gs = goals.load()
    current = gs.get("upcoming")
    passed = [g for g in gs.get("queue", []) if g.get("date") and g["date"] < day.isoformat()]
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    try:
        gaps = promo.gaps(promo.assess(settings.get("ladder", "generic-swe"), settings.get("target_level", "senior")))
    except promo.LadderError:
        gaps = []
    if (passed and not current) or not gs.get("queue"):
        if gaps:
            g = gaps[0]
            when = (day + timedelta(days=90)).isoformat()
            out.append(
                {
                    "id": _id("goal", g["id"]),
                    "type": "proposal",
                    "kind": GOAL_ONLY_IN_UI,
                    "text": f"New goal: close the {g['name']} gap by {when} (top promotion gap, {g['coverage']:.0%} covered)",
                    "why": f"{g['name']} is your top promotion gap: your evidence covers {g['coverage']:.0%} of it, and " + ("your last goal's date has passed" if passed else "you have no upcoming goal"),
                    "data": {"kind": "promotion", "title": f"Close the {g['name']} gap", "date": when, "minutes_per_day": 30},
                }
            )

    project_refs = {s["ref"] for p in projects._load().values() for s in p["sources"]}
    note_text = " ".join(e.get("title", "") + " " + str(e.get("impact", "")) for e in evidence.load() if e.get("kind") == "note")
    recorded = [t for t in priorities.load() if (t.get("measured_impact") or {}).get("result")]  # issue #214: answered from the record
    recorded_links = {u for t in recorded for u in t["measured_impact"].get("evidence", [])}
    recorded_titles = " ".join(t["title"] for t in recorded)
    for r in shipped:
        title = r.get("title", "")
        from_record = str(r.get("url") or r.get("ref") or "") in recorded_links or (title[:40] and title[:40] in recorded_titles)
        if r.get("kind") in ("pr", "github", "jira") and title[:40] not in note_text and not from_record:
            out.append({"id": _id("impact", r["id"]), "type": "question", "kind": "impact", "text": f"What was the impact of: {_full(title)}? (a number if you have one)", "data": {"title": title[:200], "source": r["kind"]}})
        if project_refs or projects._load():
            for kind, ref in _refs(title):
                if ref not in project_refs and projects._load():
                    out.append({"id": _id("project", ref), "type": "question", "kind": "project", "text": f"Which project is {ref} part of? (answer with a project name)", "data": {"kind": kind, "ref": ref}})
    ids = ", ".join(sorted(_competency_ids()))
    for t in tasks:
        if not t.get("competencies"):
            out.append(
                {
                    "id": _id("competency", t["id"]),
                    "type": "question",
                    "kind": "competency",
                    "text": f"Which promotion competency does {t['id']} ({_full(t['title'])}) show? Answer one or more of: {ids} (comma-separated)",
                    "data": {"task": t["id"], "source": str(t.get("source") or "user")},  # who added the task: a signal key (issue #185)
                }
            )
    every = projects._load()
    # archived: not watched (issue #173); a promise (issue #268) is tracked by its link and needs no notes file
    waiting = [n for n, p in every.items() if not p.get("archived_at") and p.get("kind") != "promise" and not projects.notes_path(n, every)]
    for name in waiting:
        if projects.root(name, every) != name:
            continue  # a child uses its root's notes: ask once per project tree, not once per imported row
        under = sum(1 for n in waiting if n != name and projects.root(n, every) == name)
        tail = f" and the {under} project{'s' if under > 1 else ''} under it" if under else ""
        out.append({"id": _id("memory", name), "type": "question", "kind": "memory", "text": f"Where should notes for project {name}{tail} go? (a file path)", "data": {"project": name}})
    return out


def _competency_ids() -> set[str]:
    """The competency ids of the owner's ladder: what a competency question takes as its answer."""
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    return {c["id"] for c in promo.ladders().get(settings.get("ladder", "generic-swe"), {}).get("competencies", [])}


def _feed_rule_item(p: dict[str, Any]) -> dict[str, Any]:
    """A feed rule the owner's marks suggest (issue #185): accept writes it to work/feed-rules.yaml."""
    where = f"#{p['value']}" if p["field"] == "channel" else p["value"]
    does = "as fyi, not asks" if p["kind"] == "not-ask" else "as personal"
    return {
        "id": _id("feed-rule", f"{p['kind']}:{p['source']}:{p['field']}:{p['value']}"),
        "type": "proposal",
        "kind": "feed-rule",
        "text": f"Treat {p['source']} messages from {where} {does}",
        "why": f"you marked {p['count']} of them {'not an ask' if p['kind'] == 'not-ask' else 'personal'}",
        "data": {"rule": p["rule"], "where": where, "source": p["source"]},
    }


def _agent_task_item(t: dict[str, Any]) -> dict[str, Any]:
    source, why = str(t.get("source") or "agent"), _full(t.get("why")) or "no reason given"
    return {
        "id": _id("agent-task", t["id"]),
        "type": "proposal",
        "kind": "agent-task",
        "text": f"Plan {t['id']}: {_full(t['title'])} (added by {source}: {why})",
        "why": f"{'an agent' if source == 'agent' else source} added it: {why}",
        "data": {"task": t["id"], "title": _full(t["title"]), "source": source, "why": why},
    }


def _source_item(name: str, x: dict[str, Any]) -> dict[str, Any]:
    """A related source a refresh or the memory file found for project `name`: the ticket's own title and why it belongs."""
    title, parent = _full(x.get("title")), str(x.get("parent") or "")
    if parent:
        why = f"{x['ref']} is a child of {parent}, which project {name} already follows; adding it makes its work count for {name}"
    elif x.get("via") == "memory":
        why = f"project {name}'s memory file names {x['ref']}, but it is not one of the project's sources yet"
    else:
        why = f"a refresh of project {name} found {x['ref']} linked to one of its sources"
    return {
        "id": _id("source", f"{name}:{x['ref']}"),
        "type": "proposal",
        "kind": "source",
        "text": f"Add {x['kind']} {x['ref']} to project {name}" + (f": {title}" if title else ""),
        "why": why,
        "data": {"project": name, "kind": x["kind"], "ref": x["ref"], "title": title, "parent": parent, "via": str(x.get("via") or "")},
    }


def suggestions_path():
    return paths.sub("day", "suggestions.jsonl")


def _suggestion_rows(installed_only: bool = False) -> list[dict[str, Any]]:
    """Plugin suggestions, one per (plugin, ref): the last write wins. Each row carries `origin: plugin:<name>`."""
    names = {p["name"] for p in plugins.installed()} if installed_only else None
    rows: dict[tuple[str, str], dict[str, Any]] = {}
    for r in store.read_jsonl(suggestions_path()):
        if "plugin" not in r:  # the planner's own `{at, task}` rows share this file (issue #186)
            continue
        if names is None or r["plugin"] in names:
            rows[(r.get("origin", ""), r.get("ref", ""))] = r
    return list(rows.values())


def _plugin_item(r: dict[str, Any]) -> dict[str, Any]:
    return {
        "id": _id("plugin", f"{r['plugin']}:{r['ref']}"),
        "type": "proposal",
        "kind": "plugin",
        "origin": r["origin"],
        "text": f"{r['what']} (suggested by plugin {r['plugin']})",
        "why": r["why"],
        "data": {k: r.get(k, "") for k in ("plugin", "ref", "what", "accept", "task", "link")},
    }


def _offer(item: dict[str, Any]) -> str:
    """Show one new proposal now, without a full regenerate (no connector calls). Caller holds the lock."""
    items = load()
    old = next((i for i in items if i["id"] == item["id"]), None)
    if old is None:
        items.append({**item, "status": "open", "created_at": calendar.now().isoformat(timespec="seconds")})
        status = "new"
    elif old["status"] == "open":
        old.update(item)  # the same ref with new words: show the new words
        status = "updated"
    else:
        return "decided"  # a dismissed or accepted item never comes back
    _save(items)
    return status


def offer_task(t: dict[str, Any]) -> dict[str, str]:
    """An agent just proposed task `t`: put it in the inbox at once, not after the next watcher cycle (issue #71)."""
    item = _agent_task_item(t)
    with _locked():
        status = _offer(item)
    events.emit("day.inbox.suggest", status=status, source_kind="agent")
    return {"id": item["id"], "status": status}


def suggest(plugin: str, what: str, why: str, accept: str, ref: str = "", task: str = "", link: str = "") -> dict[str, Any]:
    """A plugin's own suggestion with its what, why, and on-accept text (issue #71). Nothing applies until the owner accepts."""
    if plugin not in {p["name"] for p in plugins.installed()}:
        raise InboxError(f"plugin {plugin} is not installed (see `careerbrew plugin list`)")
    what, why, accept, ref, task, link = (" ".join(str(x).split()) for x in (what, why, accept, ref, task, link))
    missing = [n for n, v in (("--what", what), ("--why", why), ("--accept", accept)) if not v]
    if missing:
        raise InboxError(f"a suggestion needs {', '.join(missing)}: say what it is, why, and what Accept does")
    row = {
        "origin": f"plugin:{plugin}",
        "plugin": plugin,
        "ref": (ref or what)[:120],
        "what": what[:200],
        "why": why[:300],
        "accept": accept[:300],
        "task": task[:200],
        "link": link[:300],
        "at": calendar.now().isoformat(timespec="seconds"),
    }
    with _locked():
        rows = [r for r in store.read_jsonl(suggestions_path()) if (r.get("origin"), r.get("ref")) != (row["origin"], row["ref"])]
        store.write_jsonl(suggestions_path(), [*rows, row])
        item = _plugin_item(row)
        status = _offer(item)
    events.emit("day.inbox.suggest", status=status, source_kind="plugin")
    return {**item, "status": status}


def plugin_suggestions(plugin: str = "") -> list[dict[str, str]]:
    """Each plugin suggestion's id, plugin, ref, and the owner's decision, so a plugin can read it back (content-free)."""
    every = load()
    decided = {i["id"]: i["status"] for i in every}
    counts = signals.inbox_counts(every)
    out = []
    for r in _suggestion_rows():
        if plugin and r.get("plugin") != plugin:
            continue
        iid = _plugin_item(r)["id"]
        # `weight`: how the owner's decisions on this plugin's suggestions rank its next ones (issue #185)
        out.append({"id": iid, "plugin": r["plugin"], "ref": r["ref"], "status": decided.get(iid, "open"), "weight": signals.plugin_weight(counts, r["plugin"])})
    return out


def purge_origin(origin: str) -> int:
    """Delete inbox items that carry this origin (a removed plugin's suggestions, open or decided)."""
    with _locked():
        items = load()
        keep = [i for i in items if i.get("origin") != origin]
        if len(keep) != len(items):
            _save(keep)
    return len(items) - len(keep)


def regenerate(day: date | None = None) -> dict[str, int]:
    """Merge fresh candidates with stored items: keep decisions, drop open items that no longer apply."""
    fresh = {c["id"]: c for c in candidates(day)}  # slow (connectors): outside the lock
    sigs = done_elsewhere.signals() if config.get("inbox_expire_done") else None  # reads evidence and the feed: outside the lock too
    with _locked():
        return _merge(fresh, sigs)


def _merge(fresh: dict[str, dict[str, Any]], sigs: list[dict[str, Any]] | None = None) -> dict[str, int]:
    stored = {i["id"]: i for i in load()}
    items = [i for i in stored.values() if i["status"] != "open" or i["id"] in fresh]
    now = calendar.now()
    limit = int(config.get("questions_per_day"))
    open_q = sum(1 for i in items if i["status"] == "open" and i["type"] == "question")
    decided = signals.inbox_counts(list(stored.values()))  # the owner's decisions (issue #185)
    order = sorted(fresh, key=lambda cid: -signals.weight_of(decided.get(signals.key(fresh[cid]))))  # the budget goes to what they answer
    for cid in order:
        c = fresh[cid]
        if cid in stored:
            if stored[cid]["status"] == "open":  # new words (a whole title, a better why) show on an item already open
                stored[cid].update({k: c[k] for k in ("text", "why", "data") if k in c})
            continue
        if c["type"] == "question":
            if open_q >= limit or signals.is_muted(decided.get(signals.key(c))):
                continue
            open_q += 1
        items.append({**c, "status": "open", "created_at": now.isoformat(timespec="seconds")})
    done = done_elsewhere.check(items, now, sigs) if sigs is not None else {"expired": 0, "maybe": 0}
    _save(items)
    counts = {"proposals": sum(1 for i in items if i["status"] == "open" and i["type"] == "proposal"), "questions": sum(1 for i in items if i["status"] == "open" and i["type"] == "question")}
    events.emit("day.inbox", status="ok", items=counts["proposals"] + counts["questions"])
    if done["expired"] or done["maybe"]:
        events.emit("day.inbox.expire", status="ok", expired=done["expired"], maybe=done["maybe"], story="us-74")
    return counts


def explain(i: dict[str, Any]) -> dict[str, str]:
    """What a proposal is, why careerbrew suggests it, and exactly what Accept and Dismiss change (issue #62)."""
    d = i.get("data") or {}
    task = f"{d.get('task', '')} ({d['title']})" if d.get("title") and d.get("task") else str(d.get("task") or d.get("title") or "")
    keep = "Nothing changes; this suggestion is hidden and does not come back."
    by_kind = {
        "task-done": (f"Mark task {task} done", f"Sets task {d.get('task')} to done in Week, with its estimate as the time spent.", keep + " The task stays open."),
        "new-task": (
            f"New task: {d.get('title', '')}",
            "Adds this task to Week (priority 3, value 3, 1 hour, source: proposal), so the planner can schedule it.",
            keep + " No task is added.",
        ),
        "agent-task": (f"Plan task {task}", "Moves the task from proposed to open, so it shows in Week and the planner can schedule it.", "Sets the task to dismissed; it is never planned."),
        "source": (
            f"Add {d.get('kind')} {d.get('ref')} to project {d.get('project')}" + (f": {d['title']}" if d.get("title") else ""),  # the web's What line: the ticket's own title (issue #147)
            f"Adds {d.get('ref')} to the sources of project {d.get('project')}; its work then counts for that project.",
            keep + " The project stays as it is.",
        ),
        GOAL_ONLY_IN_UI: (
            f"New goal: {d.get('title', '')} by {d.get('date', '')}",
            f"Adds it to your upcoming goals (Goals view), with {d.get('minutes_per_day', 30)} minutes a day of practice.",
            keep + " Your goals stay as they are.",
        ),
        "feed-rule": (
            i.get("text", ""),
            f"Adds a rule to work/feed-rules.yaml (`careerbrew work feed rules` lists it) and triages the feed again: {d.get('source')} messages from {d.get('where')} {'stop being asks' if (d.get('rule') or {}).get('triage') == 'fyi' else 'stay out of evidence, the day log, and the PM update'}.",
            keep + " Your feed rules stay as they are.",
        ),
        "plugin": (
            str(d.get("what", "")),
            f"{str(d.get('accept', '')).rstrip('.')}. careerbrew "
            + (
                f'adds task "{d["task"]}" to Week (priority 3, value 3, 1 hour), so the planner can schedule it.'
                if d.get("task")
                else f"changes nothing else; plugin {d.get('plugin')} reads that you accepted it (`careerbrew day suggestions`)."
            ),
            keep + f" Plugin {d.get('plugin')} reads that you dismissed it.",
        ),
    }
    what, accept, dismiss = by_kind.get(i.get("kind", ""), (i.get("text", ""), "Applies the suggestion.", keep))
    return {"what": what, "why": i.get("why") or i.get("text", ""), "accept": accept, "dismiss": dismiss}


# Where Accept lands a suggestion: a view of the web app, so every row has a link to follow.
LANDS = {"task-done": ("Week", "#/week"), "new-task": ("Week", "#/week"), "agent-task": ("Week", "#/week"), "source": ("Work: Projects", "#/work"), GOAL_ONLY_IN_UI: ("Goals", "#/goals")}


class Context:
    """What `describe` reads once per list: ranked projects, tasks and their projects, Jira's and GitHub's web addresses.

    Lifecycle: one call's scope (`open_items`); nothing is kept between calls.
    """

    def __init__(self) -> None:
        self.ranked = project_priority.entries()
        self.tasks = {t["id"]: t for t in priorities.load()}
        self.task_projects = projects.task_projects()
        self.jira, self.github = _web_bases()
        self.group_min = int(config.get("inbox_group_min"))


def _web_bases() -> tuple[str, str]:
    """The ticket page base of the Jira connector and the web base of GitHub ("" when none), from their settings only."""
    jira = github = ""
    for c in evidence.sources_list():
        if c.get("kind") == "jira" and not jira:
            base = str(c.get("base_url") or "").rstrip("/")
            jira = "" if "api.atlassian.com" in base else base  # a scoped token's API address opens no ticket page
        elif c.get("kind") == "github" and not github:
            api = str(c.get("api") or "https://api.github.com").rstrip("/")
            github = "https://github.com" if api == "https://api.github.com" else api.removesuffix("/api/v3")
        elif c.get("kind") == "gh" and not github:
            github = f"https://{c['host']}" if c.get("host") else "https://github.com"
    return (jira if priorities.is_web(jira) else ""), (github if priorities.is_web(github) else "")


def describe(i: dict[str, Any], ctx: Context) -> dict[str, Any]:
    """Where a proposal came from, its links (http and https only), where Accept lands it, and its project and rank (issue #147)."""
    d, kind = i.get("data") or {}, i.get("kind", "")
    links: list[dict[str, str]] = []

    def add(label: str, url: str) -> None:
        if priorities.is_web(url) and all(x["url"] != url for x in links):
            links.append({"label": label or url, "url": url})

    task = ctx.tasks.get(str(d.get("task") or "")) or {}
    title, repo = "", ""
    if kind == "source":
        ref, rkind = str(d.get("ref") or ""), d.get("kind")
        if rkind == "tracker" and ctx.jira:
            add(ref, f"{ctx.jira}/browse/{ref}")
            if d.get("parent"):
                add(f"{d['parent']} (parent)", f"{ctx.jira}/browse/{d['parent']}")
        elif rkind == "repo" and ctx.github and re.fullmatch(r"[\w.-]+/[\w.-]+", ref):
            add(ref, f"{ctx.github}/{ref}")
            repo = ref
        else:
            add(ref, ref)
        title = f"{ref} {d.get('title') or ''}"
        added_by = f"careerbrew: project {d.get('project')}'s memory file" if d.get("via") == "memory" else f"careerbrew: research on project {d.get('project')}"
    elif kind == "new-task":
        key = str(d.get("key") or str(d.get("title") or "").split(":", 1)[0])
        if ctx.jira and re.fullmatch(r"[A-Z][A-Z0-9]+-\d+", key):
            add(key, f"{ctx.jira}/browse/{key}")
        title, added_by = str(d.get("title") or ""), "careerbrew: Jira, a ticket assigned to you"
    elif kind in ("agent-task", "task-done"):
        for x in priorities.links(task):
            add(x["label"], x["url"])
        title = str(task.get("title") or d.get("title") or "")
        added_by = "careerbrew: work you shipped today" if kind == "task-done" else ("an agent" if d.get("source", "agent") == "agent" else str(d.get("source")))
    elif kind == "plugin":
        for x in priorities.links({"links": [d["link"]]} if d.get("link") else {}):
            add(x["label"], x["url"])
        title, added_by = str(d.get("what") or ""), f"plugin {d.get('plugin')}"
    elif kind == GOAL_ONLY_IN_UI:
        title, added_by = str(d.get("title") or ""), "careerbrew: your promotion gaps"
    else:
        title, added_by = str(i.get("text") or ""), "careerbrew"
    repo = repo or next((r for x in links if (r := project_priority.repo_of(url=x["url"]))), "")
    project = str(d.get("project") or ctx.task_projects.get(str(d.get("task") or ""), "") or "")
    ranked = project_priority.match(project_priority.keys(title, repo, " ".join(x["url"] for x in links), project, es=ctx.ranked), ctx.ranked) if ctx.ranked else None
    lands = LANDS.get(kind) or (("Week", "#/week") if kind == "plugin" and d.get("task") else None)
    return {
        "added_by": added_by,
        "links": links,
        "lands": {"label": lands[0], "url": lands[1]} if lands else None,
        "project": project or (ranked["name"] if ranked else ""),
        "priority": ranked["rank"] if ranked else None,
    }


def _group_key(i: dict[str, Any]) -> tuple[Any, ...] | None:
    """Alike proposals share this key: one ticket project's children, one agent's reason, one plugin's Accept."""
    d, kind = i.get("data") or {}, i.get("kind")
    if kind == "source":
        key: tuple[Any, ...] = (kind, d.get("project"), d.get("kind"))
    elif kind == "new-task":
        key = (kind, str(d.get("key") or d.get("title") or "").split("-", 1)[0])
    elif kind == "agent-task":
        key = (kind, d.get("source") or "agent", d.get("why") or i.get("why"))
    elif kind == "plugin":
        key = (kind, d.get("plugin"), d.get("accept"))
    else:
        return None  # a goal, or a task to mark done: each one on its own
    return (*key, i.get("project"), i.get("priority"))


def open_items() -> list[dict[str, Any]]:
    """Open questions, and open proposals with their explanation; alike proposals carry their `group` id."""
    every = load()
    weight = signals.weights(every)
    # What the owner keeps accepting first: a kind and source they keep dismissing sinks (issue #185). Stable, so
    # with no decisions the order is unchanged.
    items = sorted((i for i in every if i["status"] == "open"), key=lambda i: -weight.get(signals.key(i), 1.0))
    if not any(i["type"] == "proposal" for i in items):
        return items
    ctx = Context()
    out, by_key = [], {}
    for i in items:
        if i["type"] != "proposal":
            out.append(i)
            continue
        row = {**i, **explain(i), **describe(i, ctx)}
        if (key := _group_key(row)) is not None:
            by_key.setdefault(f"grp-{store.digest(*key)[:10]}", []).append(row)
        out.append(row)
    for gid, rows in by_key.items():
        if len(rows) >= ctx.group_min:
            for r in rows:
                r["group"] = gid
    return out


def _prepend_rule(text: str, rule: dict[str, Any], mine: list[dict[str, Any]]) -> str:
    """Put `rule` first under `rules:` and keep the rest of the owner's file (comments, names) as written."""
    lines = text.splitlines(keepends=True)
    at = next((n for n, line in enumerate(lines) if line.rstrip() == "rules:"), None)
    item = next((line for line in lines[at + 1 :] if line.lstrip().startswith("- ")), None) if at is not None else None
    if at is not None and item is not None:
        pad = item[: len(item) - len(item.lstrip())]
        new = "".join(pad + line if line.strip() else line for line in yaml.safe_dump([rule], sort_keys=False).splitlines(keepends=True))
        out = "".join(lines[: at + 1]) + new + "".join(lines[at + 1 :])
        own = (yaml.safe_load(out) or {}).get("rules")
        if isinstance(own, list) and own[:1] == [rule] and len(own) == len(mine) + 1:
            return out
    return yaml.safe_dump({"rules": [rule, *mine]}, sort_keys=False)


def _common_links(rows: list[dict[str, Any]]) -> list[dict[str, str]]:
    urls = set.intersection(*({x["url"] for x in r.get("links", [])} for r in rows))
    return [x for x in rows[0].get("links", []) if x["url"] in urls]


def _group_row(gid: str, rows: list[dict[str, Any]]) -> dict[str, Any]:
    first, n = rows[0], len(rows)
    d, kind = first.get("data") or {}, first["kind"]
    if kind == "source":
        project, parents = d.get("project"), {str((r.get("data") or {}).get("parent") or "") for r in rows}
        label = f"Add {n} {d.get('kind')}s to project {project}"
        why = f"each is a child of {next(iter(parents))}, which project {project} already follows" if len(parents) == 1 and "" not in parents else f"research on project {project} found each one linked to its sources"
        accept, dismiss = f"Adds all {n} to the sources of project {project}; their work then counts for {project}.", f"Hides all {n}; they do not come back. Project {project} stays as it is."
    elif kind == "new-task":
        prefix = str(d.get("key") or d.get("title") or "").split("-", 1)[0]
        label, why = f"Add {n} tasks for {prefix} tickets assigned to you", "each ticket is assigned to you in Jira and still open, but no task in Week tracks it"
        accept, dismiss = f"Adds {n} tasks to Week (priority 3, value 3, 1 hour each, source: proposal), so the planner can schedule them.", f"Hides all {n}; they do not come back. No task is added."
    elif kind == "agent-task":
        who = "an agent" if d.get("source", "agent") == "agent" else str(d.get("source"))
        label, why = f"Plan {n} tasks {who} added", f"{who} added them: {d.get('why') or 'no reason given'}"
        accept, dismiss = f"Moves all {n} tasks from proposed to open, so they show in Week and the planner can schedule them.", f"Sets all {n} tasks to dismissed; they are never planned."
    else:
        plugin = d.get("plugin")
        label, why = f"{n} suggestions from plugin {plugin}", f"plugin {plugin} suggests each one; Review each shows each one's reason"
        tasks = sum(1 for r in rows if (r.get("data") or {}).get("task"))
        accept = f"For each of the {n}: {str(d.get('accept') or '').rstrip('.')}." + (
            f" careerbrew adds {tasks} task(s) to Week (priority 3, value 3, 1 hour)." if tasks else f" careerbrew changes nothing else; plugin {plugin} reads that you accepted them."
        )
        dismiss = f"Hides all {n}; they do not come back. Plugin {plugin} reads that you dismissed them."
    return {
        "id": gid,
        "kind": kind,
        "count": n,
        "label": label,
        "why": why,
        "accept": accept,
        "dismiss": dismiss,
        "added_by": first.get("added_by", ""),
        "links": _common_links(rows),
        "lands": first.get("lands"),
        "project": first.get("project", ""),
        "priority": first.get("priority"),
        "created_at": min(str(r.get("created_at") or "") for r in rows),
        "ids": [r["id"] for r in rows],
    }


def groups(items: list[dict[str, Any]]) -> list[dict[str, Any]]:
    """The groups among `open_items()`: label, why, what Accept all and Dismiss all change, and member ids, oldest first."""
    by: dict[str, list[dict[str, Any]]] = {}
    for i in items:
        if i.get("group"):
            by.setdefault(i["group"], []).append(i)
    return [_group_row(gid, rows) for gid, rows in by.items()]


def expired_items() -> list[dict[str, Any]]:
    """Suggestions careerbrew hid because their source shows them done elsewhere, newest first: kept, never deleted."""
    rows = sorted((i for i in load() if i["status"] == "expired"), key=lambda i: str((i.get("expired") or {}).get("at") or ""), reverse=True)
    return [{**i, **explain(i)} for i in rows[:EXPIRED_SHOWN]]


def view() -> dict[str, Any]:
    """The web app's inbox: open items, their groups, and the suggestions hidden as done elsewhere."""
    items = open_items()
    return {"inbox": items, "groups": groups(items), "expired": expired_items()}


def _find(item_id: str) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    items = load()
    for i in items:
        if i["id"] == item_id:
            if i["status"] != "open":
                raise InboxError(f"{item_id} is already {i['status']}")
            return items, i
    raise InboxError(f"unknown item {item_id} (see `careerbrew day inbox`)")


def accept(item_id: str, from_ui: bool = False) -> dict[str, Any]:
    with _locked():
        return _accept(item_id, from_ui)


def _accept(item_id: str, from_ui: bool) -> dict[str, Any]:
    items, i = _find(item_id)
    if i["type"] != "proposal":
        raise InboxError(f'{item_id} is a question: answer it with `careerbrew day answer {item_id} "..."`')
    d = i["data"]
    if i["kind"] == GOAL_ONLY_IN_UI:
        if not from_ui:
            raise InboxError("goal suggestions are approved in the web app only (careerbrew, then the Today view)")
        goals.set_queue([*goals.load()["queue"], d])
    elif i["kind"] == "task-done":
        t = next((t for t in priorities.load() if t["id"] == d["task"]), None)
        priorities.done(d["task"], priorities.estimate(t or {}))
    elif i["kind"] == "new-task":
        priorities.add(d["title"], 3, 3, 1.0, [], source="proposal", why="a ticket assigned to you")
    elif i["kind"] == "agent-task":
        _set_task_status(d["task"], "open")
    elif i["kind"] == "source":
        p = projects._load().get(d["project"]) or {}
        if not any(s["kind"] == d["kind"] and s["ref"] == d["ref"] for s in p.get("sources", [])):  # added by hand meanwhile: nothing to do
            projects.add_source(d["project"], d["kind"], d["ref"])
    elif i["kind"] == "feed-rule":
        from ..work import feed

        mine = [{k: v for k, v in r.items() if k != "builtin"} for r in feed.rules() if not r.get("builtin")]  # refuses a broken file with its fix
        store.write_text(feed.rules_path(), _prepend_rule((store.read_text(feed.rules_path()) or "") if feed.rules_path().exists() else "", d["rule"], mine))  # first: a broader rule of yours must not hide it
        feed.retriage()
    elif i["kind"] == "plugin" and d.get("task"):
        priorities.add(d["task"], 3, 3, 1.0, [], source="proposal", why=f"plugin {d['plugin']}: {i.get('why', '')}", links=[d["link"]] if d.get("link") else [])
    i["status"] = "accepted"
    i["decided_at"] = calendar.now().isoformat(timespec="seconds")
    _save(items)
    events.emit("day.inbox.accept", status="ok", kind=i["kind"])
    return i


def _set_task_status(task_id: str, status: str) -> None:
    tasks = priorities.load()
    for t in tasks:
        if t["id"] == task_id:
            t["status"] = status
    priorities.save(tasks)


def dismiss(item_id: str) -> dict[str, Any]:
    with _locked():
        return _dismiss(item_id)


def _dismiss(item_id: str) -> dict[str, Any]:
    items, i = _find(item_id)
    if i["kind"] == "agent-task":
        _set_task_status(i["data"]["task"], "dismissed")
    i["status"] = "dismissed"
    i["decided_at"] = calendar.now().isoformat(timespec="seconds")
    _save(items)
    return i


def decide_group(group_id: str, action: str, ids: list[str] | None = None, from_ui: bool = False) -> dict[str, Any]:
    """Accept all or Dismiss all: decide every open proposal of one group, each the way its own button would.

    `ids` are the members the owner saw when they confirmed (the group row's `ids`). The group id is a hash of the
    group's content, so rows added since (a later research refresh, the next agent task with the same reason) join
    it; only the confirmed ones are decided, and the rest stay open, counted in `left`. A member that fails (its
    project was removed meanwhile) stays open and is named in `failed`; the rest are decided.
    """
    if action not in ("accept", "dismiss"):
        raise InboxError("a group is accepted or dismissed")
    with _locked():
        members = [i for i in open_items() if i.get("group") == group_id]
        if not members:
            raise InboxError(f"unknown group {group_id} (see `careerbrew day inbox`)")
        left = 0
        if ids is not None:
            shown = set(ids)
            left = sum(1 for i in members if i["id"] not in shown)
            members = [i for i in members if i["id"] in shown]
            if not members:
                raise InboxError(f"none of the {len(shown)} suggestions you confirmed is still open in group {group_id}: look again (careerbrew day inbox)")
        decided, failed = 0, []
        stamp = f"{group_id}@{calendar.now().isoformat(timespec='seconds')}"  # one owner decision, however many rows (issue #185)
        for i in members:
            try:
                _accept(i["id"], from_ui) if action == "accept" else _dismiss(i["id"])
                decided += 1
            except (InboxError, ValueError) as exc:  # ProjectError is a ValueError
                failed.append({"id": i["id"], "error": str(exc)})
        _stamp_group(stamp, [i["id"] for i in members])
    status, kind = "partial" if failed else "ok", members[0]["kind"]
    if action == "accept":  # `action` is a literal short id in every event (test_projects.py)
        events.emit("day.inbox.group", status=status, action="accept", source_kind=kind, items=decided, failed=len(failed), story="us-74")
    else:
        events.emit("day.inbox.group", status=status, action="dismiss", source_kind=kind, items=decided, failed=len(failed), story="us-74")
    return {"group": group_id, "action": action, "kind": members[0]["kind"], "decided": decided, "failed": failed, "left": left}


def _stamp_group(stamp: str, ids: list[str]) -> None:
    """Mark the rows one Accept all or Dismiss all decided, so the signals count it once (issue #185)."""
    items = load()
    for i in items:
        if i["id"] in ids and i["status"] in ("accepted", "dismissed"):
            i["decided_group"] = stamp
    _save(items)


def reopen(item_id: str) -> dict[str, Any]:
    """Bring back a suggestion careerbrew hid as done elsewhere. The owner's choice wins: it is never hidden again."""
    with _locked():
        items = load()
        i = next((x for x in items if x["id"] == item_id), None)
        if i is None:
            raise InboxError(f"unknown item {item_id} (see `careerbrew day inbox`)")
        if i["status"] != "expired":
            raise InboxError(f"{item_id} is {i['status']}: only a suggestion careerbrew hid as done elsewhere comes back")
        i.update(status="open", keep=True)
        i.pop("expired", None)
        i.pop("maybe_done", None)
        _save(items)
    events.emit("day.inbox.reopen", status="ok", story="us-74")
    return i


def answer(item_id: str, text: str) -> dict[str, Any]:
    text = " ".join(text.split())
    if not text:
        raise InboxError("give an answer")
    with _locked():
        return _answer(item_id, text)


def _answer(item_id: str, text: str) -> dict[str, Any]:
    items, i = _find(item_id)
    if i["type"] != "question":
        raise InboxError(f"{item_id} is a proposal: accept or dismiss it")
    d = i["data"]
    if i["kind"] == "impact":
        evidence.note(d["title"], [], impact=text[:300])
    elif i["kind"] == "project":
        projects.add_source(text, d["kind"], d["ref"])
    elif i["kind"] == "competency":
        ids = [c.strip() for c in text.split(",") if c.strip()]
        known = _competency_ids()
        bad = [c for c in ids if c not in known]
        if bad:
            raise InboxError(f"unknown competencies {bad}; use {', '.join(sorted(known))}")
        tasks = priorities.load()
        for t in tasks:
            if t["id"] == d["task"]:
                t["competencies"] = ids
        priorities.save(tasks)
    elif i["kind"] == "memory":
        projects.set_memory(d["project"], text)
    i["status"] = "answered"
    i["decided_at"] = calendar.now().isoformat(timespec="seconds")
    _save(items)
    events.emit("day.inbox.answer", status="ok", kind=i["kind"])
    return i
