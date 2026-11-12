"""Turn each long run's results into a run page (`report.Run`): Blockers, Next, Done.

One builder per run: `start` (`careerbrew` with no command), `review`, `work_collect`, `day_plan`, `work_day` (`day render`),
`checkup`, `machine_ready`, `machine_new_user`, `plugin_new`, `repo_checkup` (the section logs
`scripts/checkup.sh` keeps), and `from_log` (any `== section` log handed to `scripts/report-html.sh`).
Each CLI run's `headline` holds the counts its last text line used to print, so the one summary line
still answers first.
Labels, evidence, reasons, and commands hold careerbrew's own words, counts, ids, and commands:
what the CLI already prints to an agent. Business text (a task title, a question's text) goes
into `detail`, which only the page shows. Nothing here keeps state; each call builds a new Run.
"""

from __future__ import annotations

import re
from collections import Counter
from pathlib import Path
from typing import Any

from . import REPO, privacy
from .report import Item, Run
from .workday.timing import shown

MAX_NEXT = 12


def command_in(text: str) -> str:
    """The first `backticked` command in a sentence, or the text after "fix:", or ""."""
    m = re.search(r"`([^`]+)`", text)
    if m:
        return m.group(1).strip()
    m = re.search(r"fix:\s*(?:run\s+)?([^()]+)", text)
    return m.group(1).strip().rstrip(".") if m else ""


def _cap(r: Run, more: Item | None = None) -> None:
    if len(r.next) > MAX_NEXT:
        extra = len(r.next) - MAX_NEXT + (1 if more else 0)
        r.next = r.next[: MAX_NEXT - (1 if more else 0)]
        if more:
            more.label = more.label.format(n=extra)
            r.next.append(more)


# ---------- careerbrew (start) ----------


def start(steps: list[Item], log: dict[str, Any], inbox_items: list[dict[str, Any]], stale: list[dict[str, Any]]) -> Run:
    done_today = sum(len(v) for v in log["done"].values())
    r = Run("start", "careerbrew: what matters now")
    r.done = list(steps)
    r.done.append(
        Item(
            "Rebuilt today's log",
            id="day-log",
            evidence=f"{done_today} done today, {len(log['planned'])} planned for {log['tomorrow']}, {len(log['blockers'])} blocker(s), {len(log.get('risks', []))} due soon",
            command="careerbrew day log",
        )
    )
    for b in (log.get("details") or {}).get("blockers", []):
        r.blockers.append(Item(f"Task {b['task']} is overdue", id=f"overdue-{b['task']}", why=f"it was due {b['due']} and is still open", command=f"careerbrew work done {b['task']} <hours>", detail=b.get("title", "")))
    proposals = [i for i in inbox_items if i["type"] == "proposal"]
    questions = [i for i in inbox_items if i["type"] == "question"]
    for i in questions:
        r.next.append(Item(f"Answer question {i['id']} ({i['kind']})", id=i["id"], command=f'careerbrew day answer {i["id"]} "<answer>"', detail=i.get("text", "")))
    for i in proposals:
        detail = " ".join(x for x in (i.get("what") or i.get("text", ""), i.get("why", "")) if x)
        if i["kind"] == "goal":
            r.next.append(Item(f"Approve or dismiss goal proposal {i['id']} (web app only)", id=i["id"], command="careerbrew open --view today", detail=detail))
        else:
            r.next.append(Item(f"Accept or dismiss proposal {i['id']} ({i['kind']})", id=i["id"], command=f"careerbrew day accept {i['id']}", detail=detail))
    for s in stale[:3]:
        r.next.append(Item(f"Re-read project {s['project']}'s {s['kind']} source", id=f"stale-{s['project']}", command=f"careerbrew work project check {s['project']} {s['ref']}"))
    r.next.append(Item("See what to work on right now, and why", id="day-next", command="careerbrew day next"))
    _cap(r, Item("{n} more in the inbox", id="inbox", command="careerbrew day inbox"))
    r.counts = {"done_today": done_today, "planned": len(log["planned"]), "overdue": len(r.blockers), "proposals": len(proposals), "questions": len(questions), "stale_sources": len(stale)}
    r.headline = f"for you: {len(proposals)} proposal(s), {len(questions)} question(s); {done_today} task(s) done today, {len(log['planned'])} planned for {log['tomorrow']}"
    r.commands = ["careerbrew"]
    return r


# ---------- review ----------


def review(rows: list[dict[str, Any]], dashboard: Path | None = None) -> Run:
    from . import review as rv

    goals = rv.goals()
    by = Counter(row["status"] for row in rows)
    r = Run("review", "Outcome review: KEEP, WATCH, PIVOT")
    r.done.append(Item(f"Measured {len(rows)} outcome metric(s)", id="measure", evidence=f"{by['KEEP']} KEEP, {by['WATCH']} WATCH, {by['PIVOT']} PIVOT, {by['no-data']} no data", command="careerbrew review"))
    for row in rows:
        hib = row.get("higher_is_better", True)
        keep, piv = (">=", "<") if hib else ("<=", ">")
        bar = f"value {row['value']}, keep {keep} {row['success']}, pivot {piv} {row['pivot']}"
        action = str(goals.get(row["metric"], {}).get("pivot_action", ""))
        if row["status"] == "KEEP":
            r.done.append(Item(f"{row['metric']} is KEEP", id=row["metric"], evidence=bar))
        elif row["status"] == "no-data":
            r.done.append(Item(f"{row['metric']}: no data yet", id=row["metric"], evidence=f"keep {keep} {row['success']}, pivot {piv} {row['pivot']}"))
        elif row["status"] == "PIVOT":
            r.blockers.append(Item(f"{row['metric']} is PIVOT", id=row["metric"], why=f"{bar}. {action}".strip(), command=command_in(action) or "careerbrew review"))
            r.next.append(Item(f"Act on {row['metric']}: {action}".rstrip(": "), id=row["metric"], command=command_in(action) or "careerbrew review"))
    for row in rows:
        if row["status"] == "WATCH":
            action = str(goals.get(row["metric"], {}).get("pivot_action", ""))
            r.next.append(Item(f"Watch {row['metric']} (value {row['value']}, keep at {row['success']})", id=row["metric"], command=command_in(action) or "careerbrew review"))
    if dashboard:
        r.done.append(Item("Wrote the dashboard", id="dashboard", evidence=str(dashboard), command="careerbrew review --html"))
    if not r.next:
        r.next.append(Item("Measure again next week", id="again", command="careerbrew review"))
    r.counts = {"metrics": len(rows), "keep": by["KEEP"], "watch": by["WATCH"], "pivot": by["PIVOT"], "no_data": by["no-data"]}
    r.headline = f"{len(rows)} metric(s): {by['KEEP']} KEEP, {by['WATCH']} WATCH, {by['PIVOT']} PIVOT, {by['no-data']} no data"
    r.commands = ["careerbrew review" + (" --html" if dashboard else "")]
    return r


# ---------- work collect ----------


def work_collect(res: dict[str, Any], memory: dict[str, Any] | None = None) -> Run:
    """`memory` is `memory_sync.sync_out`'s result: counts only, never a project name or a note."""
    r = Run("work-collect", "Evidence collect")
    r.done.append(Item("Collected work evidence", id="collect", evidence=f"{res['new']} new, {res['total']} total, {res['errors']} source error(s)", command="careerbrew work collect"))
    mem = memory or {}
    parts = [f"{mem['notes']} note(s) written to {mem['projects']} project(s)"] if mem.get("notes") else []
    parts += [f"{mem['removed']} note(s) of events now personal removed"] if mem.get("removed") else []
    if parts:
        r.done.append(Item("Updated project memory files", id="memory", evidence="; ".join(parts)))
    for n, reason in enumerate(res.get("reasons", []), 1):
        kind, _, why = reason.partition(": ")
        r.blockers.append(Item(f"Source {kind} failed", id=f"source-{n}", why=why or reason, command=command_in(why) or "careerbrew connectors list"))
    if r.blockers:
        r.next.append(Item("Fix the failing source(s), then collect again", id="again", command="careerbrew work collect"))
    if res["new"]:
        r.next.append(Item(f"Update the promotion case with {res['new']} new item(s)", id="promo", command="careerbrew work promo"))
    r.next.append(Item("See what is still missing, by priority", id="gaps", command="careerbrew work gaps"))
    r.counts = {"new": res["new"], "total": res["total"], "source_errors": res["errors"], "memory_notes": mem.get("notes", 0), "memory_removed": mem.get("removed", 0)}
    r.headline = f"evidence: {res['new']} new, {res['total']} total, {res['errors']} source error(s)"
    r.commands = ["careerbrew work collect"]
    return r


# ---------- day plan ----------


def day_plan(plan: dict[str, Any]) -> Run:
    r = Run("day-plan", f"Plan for {plan['day']}")
    sched = plan["schedule"]
    r.done.append(
        Item(
            f"Planned {plan['day']}",
            id="plan",
            evidence=f"{len(sched)} block(s) placed, {len(plan['meetings'])} meeting(s), {plan['free_minutes']} free min, {plan['focus_minutes']} focus min, estimates x{plan['calibration']}",
            command=f"careerbrew day plan --date {plan['day']}",
        )
    )
    for s in sched:
        r.done.append(Item(f"{s['start'][11:16]}-{s['end'][11:16]} {s['task']}" + (" (carried over)" if s.get("carry_over") else ""), id=str(s["task"]), evidence=str(s.get("reason", "")), detail=str(s.get("title", ""))))
    for n, err in enumerate(plan.get("calendar_errors", []), 1):
        r.blockers.append(Item(f"Calendar {err.split(':')[0]} could not be read", id=f"calendar-{n}", why=err, command="careerbrew data edit day/calendars.yaml"))
    if sched:
        r.next.append(Item(f"Start {sched[0]['task']} at {sched[0]['start'][11:16]}", id="day-next", command="careerbrew day next"))
    for t in plan.get("unplaced", []):
        r.next.append(Item(f"No room today for {t}: split it or move its due date", id=f"unplaced-{t}", command="careerbrew open --view week"))
    if not plan.get("calendar_connected", True):
        r.next.append(Item("Connect a calendar, so meetings block time", id="calendar", command="careerbrew data edit day/calendars.yaml"))
    _cap(r)
    r.counts = {"blocks": len(sched), "meetings": len(plan["meetings"]), "free_minutes": plan["free_minutes"], "focus_minutes": plan["focus_minutes"], "unplaced": len(plan.get("unplaced", []))}
    r.headline = f"plan for {plan['day']}: {len(plan['meetings'])} meeting(s), {plan['free_minutes']} free min, {plan['focus_minutes']} focus min, estimates x{plan['calibration']}"
    r.commands = ["careerbrew day plan"]
    return r


def work_day(plan: dict[str, Any]) -> Run:
    """The `/work` dashboard (issue #257): done rows, blocked or failed rows as blockers, then what is left.

    A blocked row waits on the owner; a failed row names its error class and next action (#259).
    Each ended row says how long it took, and a slow one names its median (#263).

    Labels hold the step name and status only; a row's summary, needs-you line, and link go into `detail`."""
    dash = plan["dashboard"]
    r = Run("work-day", dash["title"])
    update = "careerbrew day update {step} --status done"

    abouts = {s["step"]: s.get("about", "") for s in plan.get("steps", [])}  # issue #311: what each step does, under its row

    def detail(row: dict[str, Any]) -> str:
        parts = [abouts.get(row["step"], ""), row.get("summary", ""), f"Needs you: {row['needs_you']}" if row.get("needs_you") else "", row.get("link", "")]
        return " · ".join(p for p in parts if p)

    def took(row: dict[str, Any]) -> str:
        if row.get("seconds") is None:
            return ""
        return f"took {shown(row['seconds'])}" + (f" (slow: median {shown(row['median'])})" if row.get("slow") else "")

    for row in dash["rows"]:
        step, st = row["step"], row["status"]
        if st == "done":
            ev = [f"updated {row['at'][11:16]}" if row.get("at") else "", took(row)]
            r.done.append(Item(f"{step} done", id=step, evidence=", ".join(e for e in ev if e), command=step, detail=detail(row)))
        elif st == "blocked":
            r.blockers.append(Item(f"{step} blocked", id=step, why=f"blocked: waits on you; next: {row.get('next') or 'answer the ask'}" + (f"; {took(row)}" if took(row) else ""), command=update.format(step=step), detail=detail(row)))
        elif st == "failed":
            cls = row.get("error_class") or "tool"
            r.blockers.append(Item(f"{step} failed", id=step, why=f"failed ({cls}): {row.get('next') or 'run it again'}" + (f"; {took(row)}" if took(row) else ""), command=step, detail=detail(row)))
    for q in (plan.get("start_of_day") or {}).get("questions", []):  # #260: the day's one sign-in ask, first
        r.next.append(Item("Sign in once for today's sign-in steps", id=f"ask-{q['id']}", command="careerbrew day start", detail=q["text"]))
    for q in plan.get("question_records", []):  # #261: each saved question with its age; a row's own ask shows with its row below
        if q["id"] == "signin" or q.get("source") == "row":
            continue
        opts = ", ".join(q.get("options", []))
        parts = [q["text"], f"options: {opts}" if opts else "", f"pick: {q['pick']}" if q.get("pick") else ""]
        r.next.append(Item(f"Question from {q['step']} ({q['age']} old)", id=f"ask-{q['id']}", command="careerbrew day question recheck", detail=" · ".join(p for p in parts if p)))
    for row in dash["rows"]:
        if row["needs_you"] and row["status"] not in ("blocked", "failed"):
            r.next.append(Item(f"{row['step']} needs you", id=f"ask-{row['step']}", command=update.format(step=row["step"]), detail=detail(row)))
    for row in dash["rows"]:
        if row["status"] in ("waiting", "running") and not row["needs_you"]:
            verb = "Finish" if row["status"] == "running" else "Retry" if row.get("retry") else "Run"
            r.next.append(Item(f"{verb} {row['step']}", id=row["step"], command=row["step"], detail=detail(row)))
    _cap(r, Item("{n} more step(s)", id="more", command="careerbrew day run"))
    c = dash["counts"]
    slow = sum(1 for row in dash["rows"] if row.get("slow"))
    spent = sum(row["seconds"] for row in dash["rows"] if isinstance(row.get("seconds"), int))
    r.counts = {**c, "slow": slow, "seconds": spent}
    r.headline = f"work day {plan['day']}: {c['done']} done, {c['running']} running, {c['waiting']} waiting, {c['blocked']} blocked, {c['failed']} failed, {c['needs_you']} need(s) you, {slow} slow" + (
        f", {shown(spent)} of steps" if spent else ""
    )
    r.commands = ["careerbrew day run", "careerbrew day render", "careerbrew day history"]
    return r


# ---------- checkup and machine ready ----------


def _checks(run: str, title: str, rows: list[dict[str, Any]], label_key: str, again: str) -> Run:
    r = Run(run, title)
    for n, row in enumerate(rows, 1):
        if row["ok"]:
            r.done.append(Item(str(row[label_key]), id=f"check-{n}"))
        else:
            fix = str(row.get("fix") or "")
            r.blockers.append(Item(str(row[label_key]), id=f"check-{n}", why="this check failed", command=fix))
            if fix:
                r.next.append(Item(f"Fix: {row[label_key]}", id=f"check-{n}", command=fix))
    r.next.append(Item("Run it again after the fixes" if r.blockers else "Run it again next week", id="again", command=again))
    r.counts = {"checks": len(rows), "passed": len(r.done), "failed": len(r.blockers)}
    r.commands = [again]
    return r


def checkup(rows: list[dict[str, Any]], offline: bool = False) -> Run:
    r = _checks("checkup", "careerbrew checkup", rows, "step", "careerbrew checkup" + (" --offline" if offline else ""))
    r.headline = f"checkup: {r.counts['passed']}/{r.counts['checks']} passed"
    return r


def machine_ready(rows: list[dict[str, Any]]) -> Run:
    r = _checks("machine-ready", "Corporate readiness", rows, "check", "careerbrew machine ready")
    r.headline = f"corporate readiness: {r.counts['passed']}/{r.counts['checks']}"
    return r


# ---------- machine new-user and plugin new ----------


BUG_REPORT = Item(
    "If careerbrew misbehaves, report it from here only if company policy allows, after `careerbrew privacy scan --file <draft>` passes (this clone is pull-only); else report it or open a PR from your personal machine",
    id="report-bug",
    command=f"gh issue create -R {REPO}",
)


def _token_steps(todo: list[str]) -> list[Item]:
    """One Next item per token the owner types in their own terminal (never in chat)."""
    from . import connectors

    out = []
    for cmd in todo:
        tool = cmd.rsplit(".", 1)[-1]
        g = connectors.GUIDE.get(tool, {})
        where = f": create it at {g['token_page']} (it looks like {g['looks_like']})" if g else ""
        out.append(Item(f"Type your {tool} token in your terminal{where}", id=f"token-{tool}", command=cmd))
    return out


def machine_new_user(res: dict[str, Any]) -> Run:
    """`careerbrew machine new-user`: role, profile, repos, connectors, then readiness (the same checks as `machine ready`)."""
    r = _checks("machine-new-user", "Set up on this corporate machine", res["readiness"], "check", "careerbrew machine ready")
    tools = list(res["tools"])
    r.done.insert(0, Item(f"Set up for you with plugin {res['plugin']}", id="setup", evidence=f"{res['repos']} git repo(s) as evidence sources, connectors {', '.join(tools) or 'none'}", command="careerbrew machine new-user"))
    r.next = [*_token_steps(res["todo"]), *r.next, BUG_REPORT]
    passed, checks = r.counts["passed"], r.counts["checks"]
    r.counts = {"repos": res["repos"], "connectors": len(tools), "tokens_to_type": len(res["todo"]), "checks": checks, "passed": passed, "failed": r.counts["failed"]}
    r.headline = f"{res['repos']} git repo(s) as evidence sources, {len(tools)} connector(s), {len(res['todo'])} token(s) to type; corporate readiness: {passed}/{checks}"
    r.commands = ["careerbrew machine new-user", "careerbrew machine ready"]
    return r


def plugin_new(res: dict[str, Any], tools: list[str], needs_email: bool) -> Run:
    """`careerbrew plugin new`: create, install, and connect a company plugin in one step."""
    r = Run("plugin-new", "New company plugin")
    p = res["plugin"]
    r.done.append(Item(f"Created plugin {p['name']} and installed it", id="plugin", evidence=f"at {res['folder']} ({p['origin']})", command="careerbrew plugin list"))
    r.done.append(Item(f"Connected {', '.join(tools) or 'no tools'}", id="connectors", evidence="policy: local model only, no export off this machine, agents may not read business data", command="careerbrew connectors list"))
    r.next = _token_steps(res["todo"])
    if needs_email:
        r.blockers.append(
            Item("Jira needs your Atlassian login email", id="jira-email", why="the Jira connector signs in with it; rerun with --email you@example.com, or add `email:` to its entry", command="careerbrew data edit work/connectors.yaml")
        )
        r.next.append(Item("Add your Atlassian login email to the jira connector", id="jira-email", command="careerbrew data edit work/connectors.yaml"))
    r.next.append(Item(f"Fill {res['folder']}/ladders/*.yaml with the real promotion criteria, then reinstall", id="ladder", command="careerbrew plugin refresh"))
    r.next.append(Item("Check this machine is ready for work mode", id="ready", command="careerbrew machine ready"))
    r.counts = {"connectors": len(tools), "tokens_to_type": len(res["todo"])}
    r.headline = f"plugin created and installed: {len(tools)} connector(s), {len(res['todo'])} token(s) to type"
    r.commands = ["careerbrew plugin new"]
    return r


# ---------- scripts/checkup.sh ----------


def _lines(text: str, prefix: str) -> list[str]:
    return [line[len(prefix) :].strip() for line in text.splitlines() if line.startswith(prefix)]


# The wrap-up skill's open-work.sh prints one `  <kind>     <text>` line per item (its say() is printf '  %-9s %s').
SKILL_ITEM = re.compile(r"^  (dirty|do-not-stage|worktree|stash|unpushed|branch|pr|unknown) +(\S.*)$")


def _skill_items(text: str) -> list[tuple[str, str]]:
    home = str(Path.home())
    return [(m.group(1), m.group(2).strip().replace(home, "~")) for m in map(SKILL_ITEM.match, text.splitlines()) if m]


# scripts/open-work.sh tags a branch whose work landed through other PRs (issue #255).
LANDED = re.compile(r"^- branch (\S+): (likely landed \(\d+% of its added lines are on \S+\))")


def _landed(logs: Path) -> dict[str, str]:
    """Branch name -> its "likely landed (N% ...)" note, from scripts/open-work.sh's section log."""
    found: dict[str, str] = {}
    for log in logs.glob("*-open-work.log"):
        for m in map(LANDED.match, privacy.redact(_read(log)).splitlines()):
            if m:
                found[m.group(1)] = m.group(2)
    return found


# A never-pushed branch the owner retired with an archive tag, and the retire command for one not yet retired (issue #277).
RETIRED = re.compile(r"^\(retired: branch (\S+) matches tag archive/")
RETIRE = re.compile(r"^- branch (\S+): .*never pushed \(if its work landed elsewhere, retire it: (git tag archive/\S+ \S+)\)$")


def _retired(logs: Path) -> set[str]:
    """Branch names scripts/open-work.sh reported as retired."""
    return {m.group(1) for log in logs.glob("*-open-work.log") for m in map(RETIRED.match, privacy.redact(_read(log)).splitlines()) if m}


def _landed_branch(kind: str, text: str, landed: dict[str, str] | set[str]) -> str | None:
    """The branch a wrap-up skill item names, when open-work.sh found it likely landed (or retired)."""
    if kind == "branch":
        name = text.split(":", 1)[0].strip()
    elif kind == "worktree" and (m := re.search(r" \[([^\]]+)\]", text)):
        if (d := re.search(r"(\d+) uncommitted file", text)) and int(d.group(1)) > 0:
            return None  # uncommitted files are work, whatever the branch
        name = m.group(1)
    else:
        return None
    return name if name in landed else None


def _continue_command(kind: str, text: str, fallback: str) -> str:
    """The exact command to pick up one open work item the wrap-up skill listed."""
    m = re.match(r"#(\d+)", text)
    if kind == "pr" and m:
        return f"gh pr checkout {m.group(1)}"
    if kind == "branch":
        return f"git switch {text.split(':', 1)[0].strip()}"
    if kind == "worktree":
        return f"cd {text.split(' [', 1)[0].strip()}"
    if kind == "stash":
        return "git stash list"
    if kind in ("dirty", "do-not-stage", "unpushed"):
        return "git status"
    if kind == "unknown":  # the skill could not check: gh is missing or signed out, or git fetch failed
        return "gh auth status" if text.startswith("gh") else "git fetch"
    return fallback


def repo_checkup(logs: Path, rc: int, title: str = "careerbrew checkup") -> Run:
    """The repo checkup from the section logs scripts/checkup.sh keeps: NN-<id>.title, .cmd, .log, .rc."""
    r = Run("repo-checkup", title, personal=False, failed=rc != 0)
    r.verdict = "healthy" if rc == 0 else "needs work: fix the blockers first"
    first: list[Item] = []
    later: list[Item] = []
    passed = 0
    open_prs: list[str] = []
    landed = _landed(logs)
    retired = _retired(logs)
    for t in sorted(logs.glob("*.title")):
        base = t.with_suffix("")
        sid = base.name.split("-", 1)[1] if "-" in base.name else base.name
        title = t.read_text(encoding="utf-8").strip()
        cmd = _read(base.with_suffix(".cmd")).strip()
        text = privacy.redact(_read(base.with_suffix(".log")))
        code_text = _read(base.with_suffix(".rc")).strip()
        code = int(code_text) if code_text.lstrip("-").isdigit() else 1
        passed += code == 0
        r.logs.append((f"{title} (exit {code})", text))
        if cmd:
            r.commands.append(cmd)
        last = next((x.strip() for x in reversed(text.splitlines()) if x.strip()), "")
        if sid == "open-prs":
            prs = re.findall(r"(?m)^OPEN\s+#(\d+) (.*?) \(branch [^\n]*\n\s+next: (.+)$", text)
            open_prs = [n for n, _t, _s in prs]
            if code == 0:
                r.done.append(Item("No open pull requests", id="open-prs", evidence=last, command=cmd))
            elif prs:
                r.done.append(Item(f"Listed {len(prs)} open pull request(s)", id="open-prs", evidence=", ".join(f"#{n}" for n in open_prs), command=cmd))
                r.blockers.append(Item(f"{len(prs)} open pull request(s) to finish first", id="open-prs", why="new work waits until open pull requests merge (open work first)", command=f"gh pr checkout {open_prs[0]}"))
                first += [Item(f"Finish PR #{n} ({ttl}): {step}", id=f"pr-{n}", command=f"gh pr checkout {n}") for n, ttl, step in prs]
            else:
                line = next((x for x in text.splitlines() if x.startswith("FAIL")), last)
                r.blockers.append(Item("Open pull requests could not be listed", id="open-prs", why=line.removeprefix("FAIL").strip(), command=command_in(line) or "gh auth status"))
        elif sid == "open-work-skill":
            listed = _skill_items(text)
            found = [(k, x) for k, x in listed if not _landed_branch(k, x, retired)]
            if listed and not found:
                r.done.append(Item(f"No open work: {len(listed)} item(s) on retired branches", id=sid, evidence=", ".join(sorted(retired)), command=cmd))
            elif found:
                kinds = Counter(k for k, _ in found)
                r.done.append(Item(f"Listed {len(found)} open work item(s)", id=sid, evidence=", ".join(f"{n} {k}" for k, n in kinds.items()), command=cmd))
                steps: list[Item] = []
                for n, (k, x) in enumerate(found, 1):
                    if b := _landed_branch(k, x, landed):
                        what = f"{x.split(':', 1)[0]}: {landed[b]}" if k == "branch" else f"{x.split(']', 1)[0]}]: likely landed"
                        later.append(Item(f"Clean up: {k} {what}", id=f"{sid}-{n}", command=f"git log --oneline main..{b}"))
                    else:
                        steps.append(Item(f"Continue: {k} {x}", id=f"{sid}-{n}", command=_continue_command(k, x, cmd)))
                first += steps
                if steps:
                    r.blockers.append(Item(f"{len(steps)} open work item(s) to finish first", id=sid, why="continue open work before new work: finish, or wrap it up as a draft PR", command=steps[0].command))
            elif code != 0:
                r.blockers.append(Item("Open work could not be checked", id=sid, why=last, command=cmd))
            elif "wrap-up skill not installed" in text:
                r.done.append(Item("Wrap-up skill not installed: showed git status only", id=sid, evidence=last, command="git status --short"))
            else:
                r.done.append(Item("No open work (wrap-up skill)", id=sid, evidence=last, command=cmd))
        elif sid == "open-work":
            items = _lines(text, "- ")
            if not items:
                r.done.append(Item("No open work", id=sid, evidence=last, command=cmd))
            else:
                r.done.append(Item(f"Listed {len(items)} open work item(s)", id=sid, evidence=last, command=cmd))
                open_items = [(n, x) for n, x in enumerate(items, 1) if not LANDED.match(f"- {x}")]
                first += [Item(f"Continue: {x}", id=f"{sid}-{n}", command=cmd) for n, x in open_items]
                shown = {i.command for i in later}
                for n, x in enumerate(items, 1):
                    if (m := LANDED.match(f"- {x}")) and (c := f"git log --oneline main..{m.group(1)}") not in shown:
                        later.append(Item(f"Clean up: branch {m.group(1)}: {m.group(2)}", id=f"{sid}-{n}", command=c))
                    if m := RETIRE.match(f"- {x}"):
                        later.append(Item(f"Retire: branch {m.group(1)} (only if its work landed elsewhere)", id=f"{sid}-retire-{n}", command=m.group(2)))
                if code != 0 and open_items:
                    r.blockers.append(Item(f"{len(open_items)} open work item(s) to finish first", id=sid, why="continue open work before new work: finish, or wrap it up as a draft PR", command=cmd))
        elif sid == "health":
            oks = _lines(text, "ok ")
            r.done.append(Item(f"Health: {len(oks)} check(s) ok", id="health", evidence=f"{len(oks)} ok, {len(_lines(text, 'FAIL'))} failed", command=cmd))
            for n, line in enumerate(_lines(text, "FAIL"), 1):
                what, _, fix = line.partition("-> fix:")
                r.blockers.append(Item(f"Health: {what.strip()}", id=f"health-{n}", why="a health check failed", command=fix.strip() or cmd))
        elif sid == "gate":
            cached = len(re.findall(r"(?m)^check\.sh: \S+ cached", text))
            if code == 0:
                r.done.append(Item("Gate passed", id="gate", evidence=f"{last} ({cached} step(s) cached)", command=cmd))
            else:
                m = re.search(r"step '([^']+)' failed", text)
                r.blockers.append(Item("Gate failed", id="gate", why=f"step {m.group(1)} failed (its output is under Details)" if m else last, command="scripts/check.sh"))
        elif sid == "broken-stories":
            broken = re.findall(r'(?m)^\s*(\d+)\s+"story":"([^"]+)"', text)
            if not broken:
                r.done.append(Item("No broken stories", id="broken-stories", evidence=last, command=cmd))
            for count, story in broken:
                r.blockers.append(Item(f"Story {story} is broken", id=f"story-{story}", why=f"{count} failed event(s); a broken story is a P0 task", command=f'grep \'"story":"{story}"\' .metrics/events.jsonl'))
        elif sid == "last-run":
            head, *rest = text.strip().splitlines() or [""]
            r.done.append(Item(head[:1].upper() + head[1:], id="last-run", evidence="; ".join(x.strip() for x in rest), command=cmd))
        elif sid == "speed":
            r.done.append(Item("Speed report", id="speed", evidence=last, command=cmd))
        elif sid == "p0":
            tasks = _lines(text, "- ")
            if not tasks:
                r.done.append(Item("No open P0 tasks", id="p0", command=cmd))
            later += [Item(f"P0 task: {x}", id=f"p0-{n}", command="/next-task") for n, x in enumerate(tasks, 1)]
        elif sid == "audit":
            m = re.search(r"-> run it: (.+)", text)
            if m:
                later.append(Item("Monthly requirements audit is due", id="audit", command=m.group(1).strip()))
            else:
                r.done.append(Item("Monthly requirements audit not due" if "skipped" not in text else "Monthly audit skipped (new-project skill not installed)", id="audit", command=cmd))
        elif code == 0:
            r.done.append(Item(title, id=sid, evidence=last, command=cmd))
        else:
            r.blockers.append(Item(title, id=sid, why=last, command=cmd))
    fixes = [Item(f"Fix: {b.label}", id=f"fix-{b.id}", command=b.command) for b in r.blockers if b.id not in ("open-prs", "open-work-skill", "open-work")]
    seen: set[str] = set()
    r.next = [i for i in [*first, *fixes, *later] if not (i.label in seen or seen.add(i.label))]
    _cap(r, Item("{n} more: see the page", id="more", command="careerbrew open --report latest"))
    r.counts = {"sections": len(r.logs), "passed": passed, "open_prs": len(open_prs), "blockers": len(r.blockers), "exit_code": rc}
    return r


def _read(p: Path) -> str:
    try:
        return p.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return ""


# ---------- any checkup-style log (scripts/report-html.sh) ----------

ANSI = re.compile(r"\x1b\[[0-9;]*[A-Za-z]")
SECTION = re.compile(r"^(?:==|##) (.*)$")
FAILED = re.compile(r"FAIL|✗|not ok|[Ff]ailed")
NEXT = re.compile(r"^\s*(?:->|Next:)\s*")


def from_log(title: str, text: str) -> Run:
    """A page from a plain log: `== <section>` or `## <section>` starts a section; a line with FAIL, ✗, "not ok",
    or "failed" makes its section a blocker; a line starting with `->` or `Next:` is a next step; the last
    `checkup: ...` line is the verdict. The text may hold anything, so the page counts as personal."""
    r = Run("log", title)
    sections: list[tuple[str, list[str]]] = []
    for line in ANSI.sub("", text).splitlines():
        m = SECTION.match(line)
        if m:
            sections.append((m.group(1).strip(), []))
            continue
        if sections:
            sections[-1][1].append(line)
        if NEXT.match(line):
            step = NEXT.sub("", line).strip()
            if step and all(x.label != step for x in r.next):
                r.next.append(Item(step, id=f"next-{len(r.next) + 1}", command=command_in(step)))
    verdicts = [x[len("checkup: ") :].strip() for x in ANSI.sub("", text).splitlines() if x.startswith("checkup: ")]
    r.verdict = verdicts[-1] if verdicts else "no verdict line"
    r.failed = not r.verdict.startswith("healthy")
    seen: set[str] = set()
    for n, (name, lines) in enumerate(sections, 1):
        r.logs.append((name, "\n".join(lines).strip()))
        if name in seen:
            continue
        seen.add(name)
        if any(FAILED.search(x) for x in lines):
            r.blockers.append(Item(name, id=f"section-{n}"))
        else:
            last = next((x.strip() for x in reversed(lines) if x.strip() and not x.startswith("checkup: ")), "")
            r.done.append(Item(name, id=f"section-{n}", evidence=last))
    r.counts = {"sections": len(sections), "blockers": len(r.blockers), "next_steps": len(r.next)}
    return r
