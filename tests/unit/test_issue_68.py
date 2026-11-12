"""Issue #68: a manager-ready PM update, and one project priority setting that the PM update, the planner, and Next step use."""

import re
from datetime import date, datetime

import pytest

from careerbrew import calendar, config
from careerbrew.cli import main
from careerbrew.day import log as daylog
from careerbrew.day import planner, results
from careerbrew.work import focus, priorities, project_priority

FRIDAY = date(2026, 10, 16)
PRIORITY = "billing=PROJ-*|acme/billing|*billing*, tooling=*/agentbrew|*/dotfiles|*/minsky"


def _row(rid: str, kind: str, title: str, hour: int = 11) -> dict:
    return {"id": rid, "at": f"2026-10-16T{hour:02d}:00", "kind": kind, "title": title, "source": "evidence", "origin": "user"}


@pytest.fixture
def busy_day(home, monkeypatch):
    """42 work rows across 6 groups: the raw draft was one line per row (40+ lines)."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    rows = [_row(f"pr:example-org/{repo}#{n}", "pr", f"feat: {repo} step {n} (#{n})") for repo, k in (("agentbrew", 6), ("dotfiles", 5), ("minsky", 4)) for n in range(1, k + 1)]
    rows += [_row("pr:acme/billing#7", "pr", "Backfill old invoices for PROJ-55 (#7)"), _row("jira:PROJ-57", "jira", "PROJ-57: Close the ledger")]
    rows += [_row(f"git:careerbrew:{n:012x}", "commit", f"fix: careerbrew commit number {n}") for n in range(1, 26)]
    rows += [_row("pr:acme/docs#2", "pr", "docs: fix the typo in setup (#2)"), _row("pr:someone/website#3", "pr", "Fix the footer (#3)")]
    rows += [_row("meeting:1", "meeting", "Billing sync", 10), _row("meeting:2", "meeting", "1:1 with my manager", 14)]
    results.append(rows, FRIDAY)
    priorities.add("PROJ-60 Write the queue design doc", 3, 3, 1, [])
    priorities.add("Ship the export", 4, 4, 1, [], due="2026-10-14", links=["https://github.com/example-org/agentbrew/issues/3"])
    priorities.add("Update the setup guide", 2, 2, 1, [])
    config.set_value("project_priority", PRIORITY)
    return FRIDAY


def test_pm_update_is_one_line_per_project_in_priority_order_naming_its_work(busy_day):
    # Issue #214 dropped the 5-line cap and its shared Other line: a project merged into Other lost its own name, and a
    # line with 3 or more items gave only counts. Now each project has its line and names its results (MAX_NAMED, then "N more").
    body = daylog.pm_report(daylog.build(busy_day))["body"]
    lines = body.splitlines()
    assert [x.split(":", 1)[0] for x in lines] == ["billing", "tooling", "careerbrew", "docs", "website", "Other"], body
    assert "15 PRs in 3 repos (agentbrew, dotfiles, minsky)" in lines[1], "related work is grouped and counted"
    assert "agentbrew step 1, agentbrew step 2" in lines[1] and "and 7 more (15 PRs" in lines[1], "the real work is named, the rest counted"
    assert "and 17 more (25 commits)" in lines[2]
    assert "backfill old invoices" in lines[0] and "close the ledger" in lines[0] and "1 meeting" in lines[0], "plain titles, still specific"
    assert "next: write the queue design doc" in lines[0]
    assert "overdue: ship the export (due 2026-10-14)" in lines[1] and "next: ship the export" not in lines[1], "an overdue task is named once"
    assert lines[4] == "website: fix the footer (1 PR).", "a small project keeps its own line, no longer folded into Other"
    assert "update the setup guide" in lines[5]


def test_pm_update_has_no_ids_and_only_checkable_numbers(busy_day):
    body = daylog.pm_report(daylog.build(busy_day))["body"]
    assert not re.search(r"\bt\d{3}\b|\b[A-Z][A-Z0-9]+-\d+\b|#\d+", body), f"no task ids, ticket keys, or PR numbers:\n{body}"
    after_labels = [x.split(": ", 1)[1] for x in body.splitlines()]
    assert not any(re.search(r"\b(?:feat|fix|docs)(?:\([^)]*\))?:", x) for x in after_labels), f"plain English, no commit prefixes:\n{body}"
    dates = re.findall(r"\d{4}-\d{2}-\d{2}", body)
    assert dates == ["2026-10-14"], "the only date is the due date of the overdue task"
    numbers = set(re.findall(r"\d+", re.sub(r"\d{4}-\d{2}-\d{2}", "", body)))
    # 15 PRs, 3 repos, 25 commits, and 1 of each other kind: counts of rows anyone can open; the 1 in "1:1" is a meeting title.
    # Since issue #214 the line names its titles, so the fixture's "step 4" and "commit number 8" are words of a title, and
    # "7 more" and "17 more" count the rest of the 15 PRs and 25 commits: still only numbers anyone can check.
    titled = {str(n) for n in range(1, 26)}
    assert numbers <= {"1", "3", "15", "25", "7", "17"} | titled, f"only counts that can be checked, got {sorted(numbers)}"
    assert "score" not in body and "impact" not in body and "priority" not in body


def test_changing_the_priority_reorders_the_update(busy_day):
    config.set_value("project_priority", "tooling=*/agentbrew|*/dotfiles|*/minsky, billing=PROJ-*|acme/billing|*billing*")
    lines = daylog.pm_report(daylog.build(busy_day))["body"].splitlines()
    assert [x.split(":", 1)[0] for x in lines[:2]] == ["tooling", "billing"]


def test_done_rows_name_their_priority_project(busy_day):
    d = daylog.build(busy_day)
    shipped = {x["title"]: x for x in d["details"]["done"]["Shipped"]}
    assert shipped["feat: dotfiles step 2 (#2)"]["project"] == "tooling" and shipped["feat: dotfiles step 2 (#2)"]["repo"] == "example-org/dotfiles"
    assert shipped["PROJ-57: Close the ledger"]["project"] == "billing"


def test_project_priority_setting_validates_and_round_trips(home):
    value = config.set_value("project_priority", "billing=PROJ-*|acme/billing, tooling")
    assert value == ["billing=PROJ-*|acme/billing", "tooling"], "rank is the order; a bare name is its own pattern"
    assert config.set_value("project_priority", ",".join(value)) == value, "the web Settings editor joins a list with commas"
    for bad, why in (("Billing=x", "lowercase"), ("a=x, a=y", "twice"), ("a=", "no pattern"), ("a=x y", "pattern")):
        with pytest.raises(ValueError, match=why):
            config.set_value("project_priority", bad)
    assert config.set_value("project_priority", "") == []


def test_work_priority_commands_edit_one_project_at_a_time(home, capsys):
    assert main(["work", "priority", "show"]) == 0
    assert "no project priority yet" in capsys.readouterr().out
    assert main(["work", "priority", "set", "billing", "--match", "PROJ-*", "--match", "acme/billing"]) == 0
    assert main(["work", "priority", "set", "tooling", "--match", "*/agentbrew"]) == 0
    assert main(["work", "priority", "set", "tooling", "--rank", "1"]) == 0, "moving keeps the patterns"
    assert config.get("project_priority") == ["tooling=*/agentbrew", "billing=PROJ-*|acme/billing"]
    priorities.add("PROJ-9 Add retries", 3, 3, 1, [])
    capsys.readouterr()
    assert main(["work", "priority", "show"]) == 0
    out = capsys.readouterr().out
    assert "1. tooling" in out and "2. billing" in out and "1 open task" in out
    assert main(["work", "priority", "remove", "tooling"]) == 0
    assert config.get("project_priority") == ["billing=PROJ-*|acme/billing"]
    assert main(["work", "priority", "remove", "nope"]) == 2
    assert main(["work", "priority", "set", "x", "--rank", "0"]) == 2


@pytest.fixture
def two_tasks(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T10:00")
    a = priorities.add("PROJ-61 Add retries", 3, 3, 1, [])
    b = priorities.add("Polish the sync output", 3, 3, 1, [], links=["https://github.com/example-org/agentbrew/issues/9"])
    return a["id"], b["id"]


def test_planner_and_next_step_follow_the_project_priority(two_tasks):
    a, b = two_tasks
    assert [t["id"] for t in priorities.rank({}, today=FRIDAY)] == [a, b], "a tie without a priority keeps id order"
    config.set_value("project_priority", "tooling=*/agentbrew, billing=PROJ-*")
    ranked = priorities.rank({}, today=FRIDAY)
    assert [t["id"] for t in ranked] == [b, a]
    assert ranked[0]["parts"]["project"] == "tooling" and ranked[0]["parts"]["project_bonus"] > ranked[1]["parts"]["project_bonus"] > 0
    week = planner.plan_week(FRIDAY, 1)[0]
    assert [s["task"] for s in week["schedule"]][:2] == [b, a]
    now = datetime.fromisoformat("2026-10-16T10:00").replace(tzinfo=calendar.local_tz())
    nxt = planner.next_step(now)
    assert nxt["task"] == b and "project tooling (priority 1 of 2)" in nxt["reason"]
    why = next(h for h in focus.build(now)["horizons"] if h["id"] == "now")["items"][0]["why"]
    assert "project bonus 3 for tooling (priority 1 of 2)" in why, "the Dashboard quotes the bonus it used"
    config.set_value("project_priority", "billing=PROJ-*, tooling=*/agentbrew")
    assert [t["id"] for t in priorities.rank({}, today=FRIDAY)] == [a, b], "editing the setting changes the plan"


def test_titles_lose_ids_and_prefixes_but_keep_their_words(home):
    plain = daylog._plain
    assert plain("fix(web): no sideways scrolling at any width (closes #67) (#94)") == "no sideways scrolling at any width"
    assert plain("[PROJ-1] Add export") == "add export" and plain("PROJ-57: Close the ledger") == "close the ledger"
    assert plain("Backfill old invoices for PROJ-55") == "backfill old invoices" and plain("Ship PROJ-9 retries") == "ship retries"
    assert plain("API docs (utf-8 only)") == "API docs (utf-8 only)", "acronyms and ordinary words stay"
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["Reviewed 4 pull requests", "Reviewed 0 pull requests"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "github", "repo": ""}, {"kind": "github", "repo": "o/quiet"}]}}
    assert daylog.pm_lines(d) == ["Other: reviewed 4 PRs."], "a review row is a count; a line with nothing to say is dropped"


def test_utf_8_and_iso_8601_stay_in_the_pm_update(home, monkeypatch):
    """Task pm-plain-utf8: only keys of ticket projects that `project_priority` names are ids; UTF-8 and ISO-8601 are words."""
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T16:00")
    rows = [_row("git:shop:aaa111", "commit", "fix: decode UTF-8 file names"), _row("pr:acme/billing#8", "pr", "Parse ISO-8601 dates for PROJ-56 (#8)")]
    results.append([*rows, _row("gdocs:1", "gdocs", "UTF-8 migration plan")], FRIDAY)
    config.set_value("project_priority", PRIORITY)
    lines = daylog.pm_report(daylog.build(FRIDAY))["body"].splitlines()
    assert lines == ["billing: parse ISO-8601 dates (1 PR).", "shop: decode UTF-8 file names (1 commit).", "Other: UTF-8 migration plan (1 doc)."], "no UTF line: UTF-8 is not a ticket key"


def test_ticket_keys_come_from_named_ticket_projects_or_tracker_links(home):
    text = "fix: decode UTF-8 names in PROJ-12"
    assert "utf" not in project_priority.keys(text) and "proj" in project_priority.keys(text), "no project names a ticket project: any key but a standard"
    assert daylog._plain(text) == "decode UTF-8 names" and daylog._plain("Parse ISO-8601 dates") == "parse ISO-8601 dates"
    config.set_value("project_priority", "billing=OPS-*")
    assert "proj" not in project_priority.keys(text) and daylog._plain(text) == "decode UTF-8 names in PROJ-12", "only the named ticket project OPS is read"
    assert daylog._plain("OPS-3 Add retries") == "add retries"
    assert "proj" in project_priority.keys(text, link="https://jira.example.com/browse/PROJ-12"), "a tracker link names its ticket project"
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["PROJ-7: Restart the server"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "jira", "url": "https://jira.example.com/browse/PROJ-7", "repo": ""}]}}
    assert daylog.pm_lines(d) == ["PROJ: restart the server (1 ticket)."], "a ticket from the tracker keeps its line and loses its key"


@pytest.mark.parametrize("setting", ["platform=api-*|*/platform", "web-2", "sre=SRE", "billing=*billing*"])
def test_a_repo_glob_or_a_lone_bare_name_keeps_the_generic_strip(home, setting):
    """A lower-case glob with a dash (`api-*`, a project `web-2`) is a repo, and a lone bare `SRE` may be a repo or a team:
    none names a ticket project, so every key but a standard still leaves the manager-facing text."""
    config.set_value("project_priority", setting)
    assert project_priority.ticket_projects() == set()
    assert daylog._plain("PROJ-12: fix login; decode UTF-8") == "fix login; decode UTF-8"
    d = {"day": "2026-10-16", "tomorrow": "2026-10-19", "done": {"Shipped": ["PROJ-12: Fix login (#4)"]}, "planned": [], "blockers": []}
    d["details"] = {"done": {"Shipped": [{"kind": "github", "repo": "acme/api-gateway"}]}}
    assert daylog.pm_lines(d) == ["api-gateway: fix login (1 PR)."], "no ticket key in the PM update"


def test_a_bare_upper_case_name_is_a_ticket_project_next_to_a_key_shaped_one(home):
    config.set_value("project_priority", "billing=PROJ, ops=OPS-*|api-*")
    assert project_priority.ticket_projects() == {"OPS", "PROJ"}, "OPS-* names OPS; PROJ joins it; api-* is a repo"
    assert daylog._plain("OPS-3: retry UTF-8 reads for PROJ-12") == "retry UTF-8 reads"
