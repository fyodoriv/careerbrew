"""rust-11 Python oracle for project imports, source identity and memory sync.

Every case uses an invocation-owned synthetic home, fixed fixtures and a pinned
clock. Reports contain typed synthetic values only. No production route, owner
file or live account is used.
"""

from __future__ import annotations

import contextlib
import io
import json
import os
import stat
import sys
from pathlib import Path
from typing import Any

from careerbrew import cli, config, store
from careerbrew.work import evidence, feed, memory_sync, project_import, projects, research

CASES = (
    "source-identity",
    "project-import",
    "memory-in",
    "memory-out",
    "memory-removal",
    "research-cli",
)
FIX = Path(__file__).resolve().parents[1] / "fixtures"


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


def _invoke(args: list[str]) -> dict[str, Any]:
    output = io.StringIO()
    with contextlib.redirect_stdout(output):
        rc = cli.main(args)
    return {"exit": rc, "stdout": output.getvalue()}


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.base / "keys" / "backup.key"),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "off",
                "CAREERBREW_NOW": "2026-10-21T16:00:00+00:00",
                "CAREERBREW_TEST_OWNER": "1",
                "CAREERBREW_HTTP_FIXTURES": str(FIX / "http"),
                "CB_T": "synthetic",
                "TZ": "UTC",
            }
        )


def _source_identity(base: Path) -> dict[str, Any]:
    Workspace(base, "source-identity")
    refs = [
        "t001",
        "#C0123",
        "#billing-eng",
        "PROJ-7",
        "https://tracker.example.com/browse/PROJ-9",
        "https://github.com/acme/billing/pull/41/files?review=1#comment",
        "https://github.example.com/acme/billing/issues/7?x=1",
        "https://github.com/acme/billing/tree/main/src",
        "https://docs.google.com/document/d/doc-prd-1/edit",
        "https://example.slack.com/archives/C0123/p123",
        "https://example.com/post",
        "acme/billing",
    ]
    detected = [research.detect(ref) for ref in refs]
    projects.add("billing")
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add("search")
    projects.add_source("search", "repo", "acme/search")
    closest = research.match_project("tracker", "PROJ-9")
    projects.add_source("search", "tracker", "PROJ-2")
    tied = _error(lambda: research.match_project("tracker", "PROJ-9"))
    return {
        "detected": detected,
        "items": [research.github_item(ref) for ref in refs],
        "closest": closest,
        "tied": tied,
        "bad": _error(lambda: research.detect("not a source")),
        "after": [research._after(stamp, "2026-10-05T09:00:00+00:00") for stamp in ("2026-10-05T06:00:00.000-0400", "2026-10-05T04:00:00.000-0400", "", "bad", "2026-10-05T09:00:00")],
    }


def _project_view(name: str) -> dict[str, Any]:
    row = projects._load()[name]
    return {
        "title": row.get("title", ""),
        "group": row.get("group", ""),
        "parent": row.get("parent", ""),
        "links": row.get("links", []),
        "sources": [[s["kind"], s["ref"], bool(s.get("checked_at")), s.get("status", "")] for s in row.get("sources", [])],
    }


def _project_import(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "project-import")
    sheet = FIX / "projects" / "team-goals.csv"
    dry = project_import.run(str(sheet), dry_run=True)
    dry_empty = projects._load() == {}
    first = project_import.run(str(sheet))
    initial = {name: _project_view(name) for name in ("billing", "checkout-analytics", "payments")}
    projects.check("billing", "BILL-1", "epic 3 of 5")
    again = workspace.base / "again.csv"
    again.write_text("name,title,group,parent,links,trackers\nbilling,Billing revamp phase 2,,,,BILL-1 BILL-9\npayments,Payments platform,q4-cutline,,,PAY-1\n")
    second = project_import.run(str(again))
    checked = _project_view("billing")
    cutline = project_import.run(str(FIX / "projects" / "cutline.json"))
    sheets = [
        project_import.sheet_csv_url("https://docs.google.com/spreadsheets/d/abc_1-x/edit?usp=sharing#gid=42"),
        project_import.sheet_csv_url("https://docs.google.com/spreadsheets/d/e/2PACX-pub/pubhtml"),
    ]
    remote = project_import.run("https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit#gid=0")
    bad = workspace.base / "bad.csv"
    bad.write_text("name,title,parent,links\nalpha,Alpha,beta,\nbeta,Beta,alpha,\ngamma,Gamma,nowhere,gamma\n,No name,,\nalpha,Again,,\ndelta,Delta,,ghost\n")
    bad_error = _error(lambda: project_import.run(str(bad)))
    before = projects._load()
    bad_cli = _invoke(["work", "project", "import", str(bad)])
    unchanged = projects._load() == before
    cli_result = _invoke(["work", "project", "import", str(sheet), "--dry-run"])
    os.environ["CAREERBREW_ENCRYPT"] = "on"
    private = workspace.base / "private.csv"
    private.write_text("name,title\nzebra,PROJECTZEBRA rollout\n")
    private_cli = _invoke(["work", "project", "import", str(private)])
    sealed = projects.path().read_bytes().startswith(b"CBV1\n")
    canary_hidden = "PROJECTZEBRA" not in private_cli["stdout"]
    return {
        "dry": dry,
        "dry_empty": dry_empty,
        "first": first,
        "initial": initial,
        "second": second,
        "checked": checked,
        "cutline": cutline,
        "sheets": sheets,
        "remote": remote,
        "bad_error": bad_error,
        "bad_cli": bad_cli,
        "unchanged": unchanged,
        "cli": cli_result,
        "private_cli": private_cli,
        "sealed": sealed,
        "canary_hidden": canary_hidden,
        "bad_inputs": [_error(lambda value=value: project_import.run(value)) for value in (str(workspace.base / "missing.csv"), "https://sheets.example.com/goals.csv")],
    }


def _memory_in(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "memory-in")
    mem = workspace.base / "agent-memory" / "billing-research-log.md"
    mem.parent.mkdir(parents=True)
    mem.write_text("# billing research log\n\nNotes.\n")
    projects.add("search")
    projects.add("billing", memory=str(mem))
    projects.add_source("billing", "tracker", "PROJ-1")
    initial = memory_sync.sync_in()
    edit = "\nstatus: schema frozen, migration next week\ntrackers: PROJ-7\nchannels: #eng-billing\nlinks: search, no-such-project\nPROJ-44 is blocked on the UTF-8 fix (ISO-8601 dates)\n- design doc: https://docs.google.com/document/d/doc-billing-9/edit\n- repo: [billing](https://github.com/acme/billing/pull/41).\n"
    mem.write_text(mem.read_text() + edit)
    changed = memory_sync.sync_in()
    repeat = memory_sync.sync_in()
    stamp = mem.stat().st_mtime
    os.utime(mem, (stamp, stamp + 3600))
    touched = memory_sync.sync_in()
    mem.write_text(mem.read_text() + "trackers: PROJ-8\n")
    os.utime(mem, (stamp, stamp + 3600))
    same_mtime = memory_sync.sync_in()
    research.write_memory("billing", "2026-10-21 doc doc-billing-9: PRD", ["Decision: freeze on 2026-10-20 https://example.com/owned"])
    own_write = memory_sync.sync_in()
    mem.write_text(mem.read_text() + "status: owner final\n- repo: https://github.com/acme/ledger\n")
    owner_after = memory_sync.sync_in()
    sources = sorted([s["kind"], s["ref"]] for s in projects._load()["billing"]["sources"])
    memory = next(s for s in projects._load()["billing"]["sources"] if s["kind"] == "memory")
    config.set_value("project_memory_sync", "false")
    mem.write_text(mem.read_text() + "trackers: PROJ-10\n")
    research.write_memory("billing", "2026-10-21 doc other: PRD", ["see https://example.com/off"])
    config.set_value("project_memory_sync", "true")
    resumed = memory_sync.sync_in()
    resumed_sources = sorted([s["kind"], s["ref"]] for s in projects._load()["billing"]["sources"])
    return {
        "initial": initial,
        "changed": changed,
        "repeat": repeat,
        "touched": touched,
        "same_mtime": same_mtime,
        "own_write": own_write,
        "owner_after": owner_after,
        "sources": sources,
        "links": projects._load()["billing"].get("links", []),
        "status": memory["status"],
        "resumed": resumed,
        "resumed_sources": resumed_sources,
    }


def _seed_items() -> None:
    store.write_jsonl(
        feed.events_path(),
        [
            {
                "id": "slack:C0BILL:1792594800.000300",
                "source": "slack",
                "kind": "mention",
                "at": "2026-10-21T14:00:00Z",
                "title": "Mention from sam in #eng-billing",
                "channel": "eng-billing",
                "url": "https://example.slack.com/archives/C0BILL/p1",
                "to_me": True,
                "triage": "act",
            },
            {"id": "pagerduty:Q9", "source": "pagerduty", "kind": "page", "at": "2026-10-21T15:00:00Z", "title": "Page: Invoice export failing", "url": "https://example.pagerduty.com/incidents/Q9", "to_me": True, "triage": "act"},
            {"id": "slack:C0RAND:2", "source": "slack", "kind": "message", "at": "2026-10-21T14:10:00Z", "title": "ana in #random", "channel": "random", "triage": "fyi"},
            {"id": "slack:C0BILL:3", "source": "slack", "kind": "message", "at": "2026-10-21T14:20:00Z", "title": "monitor in #eng-billing", "channel": "eng-billing", "triage": "noise"},
            {"id": "slack:D0PAT:4", "source": "slack", "kind": "dm", "at": "2026-10-21T14:30:00Z", "title": "PROJ-1 retro moved", "personal": True, "triage": "fyi"},
        ],
    )
    store.write_jsonl(
        evidence.evidence_path(),
        [
            {"id": "jira:PROJ-7", "kind": "jira", "title": "PROJ-7: Freeze the schema", "at": "2026-10-20T12:00:00+00:00", "url": "https://jira.example.com/browse/PROJ-7"},
            {"id": "pr:acme/billing#41", "kind": "pr", "title": "feat: invoice export", "at": "2026-10-19T10:00:00Z", "url": "https://github.com/acme/billing/pull/41"},
            {"id": "slack:1792594800.000300", "kind": "slack", "title": "Recognition in #eng-billing", "at": "2026-10-21T14:00:00Z", "url": "https://example.slack.com/archives/C0BILL/p1"},
            {"id": "jira:OPS-9", "kind": "jira", "title": "OPS-9: Reindex", "at": "2026-10-20T12:00:00+00:00", "url": "https://jira.example.com/browse/OPS-9"},
            {"id": "jira:PROJ-3", "kind": "jira", "title": "PROJ-3: Old work", "at": "2026-09-01T12:00:00+00:00", "url": "https://jira.example.com/browse/PROJ-3"},
        ],
    )


def _notes(path: Path) -> list[str]:
    return [line for line in path.read_text().splitlines() if line.startswith("- 2026-")]


def _memory_out(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "memory-out")
    mem = workspace.base / "billing.md"
    mem.write_text("# billing\n")
    projects.add("billing", memory=str(mem))
    projects.add_source("billing", "tracker", "PROJ-1")
    projects.add_source("billing", "chat", "#eng-billing")
    projects.add_source("billing", "repo", "acme/billing")
    projects.add_source("billing", "url", "https://github.com/acme/billing/pull/41")
    projects.add("other", memory=str(workspace.base / "other.md"))
    projects.add_source("other", "url", "https://github.com/acme/billing/pull/4")
    config.set_value("project_priority", "billing=*invoice*")
    _seed_items()
    denied = memory_sync.sync_out(allowed=False)
    first = memory_sync.sync_out(allowed=True)
    second = memory_sync.sync_out(allowed=True)
    lines = _notes(mem)
    other_text = (workspace.base / "other.md").read_text() if (workspace.base / "other.md").exists() else ""
    state_path = memory_sync.state_path()
    before_state = state_path.read_bytes()
    idle = memory_sync.sync_out(allowed=True)
    idle_state = state_path.read_bytes() == before_state
    state_path.unlink()
    rebuilt = memory_sync.sync_out(allowed=True)
    cli_allowed = _invoke(["work", "project", "sync"])
    projects.add("ledger", parent="billing")
    projects.add_source("ledger", "tracker", "LED-4")
    store.write_jsonl(
        evidence.evidence_path(), [*store.read_jsonl(evidence.evidence_path()), {"id": "jira:LED-4", "kind": "jira", "title": "LED-4: Ledger cutover", "at": "2026-10-20T13:00:00Z", "url": "https://jira.example.com/browse/LED-4"}]
    )
    child = memory_sync.sync_out(allowed=True)
    child_header = "(project ledger)" in mem.read_text()
    return {
        "denied": denied,
        "first": first,
        "second": second,
        "lines": lines,
        "other_unmatched": "pull/4" not in other_text and "invoice export" not in other_text,
        "idle_state": idle_state,
        "idle": idle,
        "rebuilt": rebuilt,
        "cli": cli_allowed,
        "child": child,
        "child_header": child_header,
        "own_write_reread": memory_sync.sync_in(),
    }


def _memory_removal(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "memory-removal")
    mem = workspace.base / "billing.md"
    mem.write_text("# billing\n")
    mem.chmod(0o600)
    projects.add("billing", memory=str(mem))
    projects.add_source("billing", "tracker", "PROJ-1")
    store.write_jsonl(feed.events_path(), [{"id": "slack:C0BILL:1", "source": "slack", "kind": "mention", "at": "2026-10-21T14:00:00Z", "title": "PROJ-1 review", "url": "https://example.com/item", "triage": "act"}])
    seeded = memory_sync.sync_out(allowed=True)
    rows = store.read_jsonl(feed.events_path())
    rows[0]["personal"] = True
    store.write_jsonl(feed.events_path(), rows)
    real = memory_sync._catch_up

    def agent_appends(key: str, text: str | None, st: dict[str, Any]) -> None:
        real(key, text, st)
        with mem.open("a") as fh:
            fh.write("trackers: PROJ-9\n")

    memory_sync._catch_up = agent_appends
    try:
        removed = memory_sync.sync_out(allowed=True)
    finally:
        memory_sync._catch_up = real
    kept_append = mem.read_text().endswith("trackers: PROJ-9\n")
    mode = stat.S_IMODE(mem.stat().st_mode)
    read_append = memory_sync.sync_in()

    rows[0]["personal"] = False
    store.write_jsonl(feed.events_path(), rows)
    memory_sync.state_path().unlink()
    memory_sync.sync_out(allowed=True)
    rows[0]["personal"] = True
    store.write_jsonl(feed.events_path(), rows)

    def owner_rewrites(key: str, text: str | None, st: dict[str, Any]) -> None:
        real(key, text, st)
        mem.write_text("# owner rewrite\n" + mem.read_text())

    memory_sync._catch_up = owner_rewrites
    try:
        refused = memory_sync.sync_out(allowed=True)
    finally:
        memory_sync._catch_up = real
    retry = memory_sync.sync_out(allowed=True)
    return {
        "seeded": seeded,
        "removed": removed,
        "kept_append": kept_append,
        "mode": mode,
        "read_append": read_append,
        "append_source": any(s["kind"] == "tracker" and s["ref"] == "PROJ-9" for s in projects._load()["billing"]["sources"]),
        "refused": refused,
        "owner_kept": mem.read_text().startswith("# owner rewrite\n"),
        "retry": retry,
        "final_has_note": "PROJ-1 review" in mem.read_text(),
    }


def _research_cli(base: Path) -> dict[str, Any]:
    workspace = Workspace(base, "research-cli")
    local = workspace.base / "kickoff.md"
    local.write_text("# notes\n- Decision: cut over on 2026-11-15\n")
    old = 1760000000
    os.utime(local, (old, old))
    projects.add("billing", memory="work/billing-notes.md")
    projects.add_source("billing", "doc", str(local))
    projects.add_source("billing", "url", "https://outlook.office.com/mail/x")
    projects.add_source("billing", "url", "https://github.com/acme/billing/pull/41")
    projects.check("billing", str(local))
    projects.check("billing", "https://github.com/acme/billing/pull/41")
    before = [(s["ref"], s.get("checked_at")) for s in projects._load()["billing"]["sources"]]
    changes = research.changes("billing")
    after = [(s["ref"], s.get("checked_at")) for s in projects._load()["billing"]["sources"]]
    absorbed = research.absorb("https://github.com/acme/billing/pull/41?query=ignored#fragment", "billing", read_allowed=False)
    cli_absorb = _invoke(["work", "project", "absorb", "billing", "https://github.com/acme/billing/issues/41"])
    cli_since = _invoke(["work", "project", "check", "billing", "--since"])
    bad_since = _invoke(["work", "project", "check", "billing", "--since", "--status", "x"])
    return {
        "changes": changes,
        "stamps_unchanged": before == after,
        "absorbed": absorbed,
        "cli_absorb": cli_absorb,
        "cli_since": cli_since,
        "bad_since": bad_since,
    }


def run_all(selected: str | None = None) -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust11-workload"
    base.mkdir(parents=True, exist_ok=True)
    functions = {
        "source-identity": _source_identity,
        "project-import": _project_import,
        "memory-in": _memory_in,
        "memory-out": _memory_out,
        "memory-removal": _memory_removal,
        "research-cli": _research_cli,
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
