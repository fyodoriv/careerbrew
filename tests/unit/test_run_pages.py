"""Run pages: long runs end with one HTML page (Done, Blockers, Next) and a content-free summary for agents."""

import json
import os
import shutil
import subprocess
import urllib.error
import urllib.request
from pathlib import Path

import pytest

from careerbrew import report, runs
from careerbrew.cli import main

from .conftest import CANARIES, FIXTURES, readme_repo

ROOT = Path(__file__).resolve().parents[2]
CANARY = CANARIES[0]


def _run(**kw):
    r = report.Run("review", "A synthetic run", **kw)
    r.done = [report.Item("Measured 3 metrics", id="m", evidence="2 KEEP, 1 PIVOT", command="careerbrew review")]
    r.blockers = [report.Item("estimate_ratio is PIVOT", id="estimate_ratio", why="value 4.0, pivot > 2.0", command="careerbrew review", detail=f"title {CANARY}")]
    r.next = [report.Item("Split the big task <script>alert(1)</script>", id="split", command="careerbrew open --view week", detail=f"detail {CANARY}")]
    r.logs = [("Gate", f"log text {CANARY}")]
    return r


def _summary_text(home: Path) -> str:
    return "".join(p.read_text() for p in (home / "out" / "reports").glob("*.json"))


# ---------- the renderer ----------


def test_page_is_self_contained_with_done_blockers_next_in_light_and_dark():
    page = report.render(_run())
    for heading in ('id="h-done">Done', 'id="h-blockers">Blockers', 'id="h-next">Next'):
        assert heading in page
    assert "<script" not in page and "&lt;script&gt;" in page, "text is escaped; the page runs no script"
    for external in ("<link", " src=", "url(", "@import", "http://", "https://"):
        assert external not in page, f"no network: {external}"
    assert "prefers-color-scheme: dark" in page and ':root[data-theme="dark"]' in page and 'not([data-theme="light"])' in page
    assert "<code>careerbrew open --view week</code>" in page, "each Next item shows its exact command"
    assert "body{margin:0;background:var(--bg)" in page, "the body has an explicit background"


def test_what_blocks_you_comes_first_then_next_then_done():
    # The blind design judge (2026-10-22): the verdict says "fix the blockers first", but Done listed first and
    # pushed the blockers below the fold. The sections follow the jump links: Blockers, Next, Done.
    page = report.render(_run())
    at = [page.index(f'id="h-{s}"') for s in ("blockers", "next", "done")]
    assert at == sorted(at), "Blockers, then Next, then Done"


def test_a_run_page_names_days_and_times_in_words_not_iso():
    # US-76 visual review (2026-10-22): the plan's page read "Plan for 2026-10-19" and "2026-10-19T09:00:00-04:00".
    # The page reads in words; the run's data (the summary, the CLI line) keeps the ISO day.
    r = report.Run("day-plan", "Plan for 2026-10-19")
    r.at = "2026-10-19T09:00:00-04:00"
    r.headline = "plan for 2026-10-19: 2 meeting(s)"
    page = report.render(r)
    assert "plan for Mon Oct 19: 2 meeting(s)" in page, "a date before a colon reads in words too"
    assert "<h1>Plan for Mon Oct 19</h1>" in page
    assert ">Mon Oct 19, 09:00" in page
    assert report.summary(r)["title"] == "Plan for 2026-10-19"


def test_summary_is_content_free_and_detail_stays_on_the_page(home, capsys):
    page = report.write(_run())
    assert page and page.parent == home / "out" / "reports" and page.name.startswith("review-")
    html = page.read_text()
    assert html.count(CANARY) == 3, "detail and logs are on the page"
    assert CANARY not in _summary_text(home), "never in the summary"
    assert main(["report", "latest", "--json"]) == 0
    out = capsys.readouterr().out
    s = json.loads(out)
    assert CANARY not in out
    assert s["status"] == "blocked" and s["personal"] is True and s["run"] == "review"
    assert s["next"] == [{"id": "split", "label": "Split the big task <script>alert(1)</script>", "command": "careerbrew open --view week"}]
    assert s["blockers"][0] == {"id": "estimate_ratio", "label": "estimate_ratio is PIVOT", "why": "value 4.0, pivot > 2.0", "command": "careerbrew review"}
    assert (home / "out" / "reports" / "latest.html").read_text() == html


def test_latest_is_the_newest_write_and_old_pages_are_pruned(home):
    ids = [report.write(report.Run("review", f"run {n}")).stem for n in range(report.KEEP_PER_RUN + 2)]  # type: ignore[union-attr]
    report.write(report.Run("day-plan", "a plan"))
    kept = sorted(p.stem for p in (home / "out" / "reports").glob("review-*.html"))
    assert len(kept) == report.KEEP_PER_RUN and ids[0] not in kept and ids[-1] in kept
    assert report.latest()["run"] == "day-plan", "same pinned second: write order decides"
    assert report.resolve("review")["id"] == ids[-1]
    assert report.resolve("../../etc/passwd") is None and report.page_html("/etc/passwd") is None


def test_the_setting_turns_pages_off(home, capsys):
    assert main(["config", "set", "run_pages", "false"]) == 0
    main(["review"])
    assert "page:" not in capsys.readouterr().out
    assert not list((home / "out" / "reports").glob("*.html")) if (home / "out" / "reports").exists() else True
    assert main(["report", "latest"]) == 1
    assert "run_pages setting must be on" in capsys.readouterr().out


def test_the_write_event_is_content_free(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    report.write(_run())
    rows = [json.loads(x) for x in (home / "logs" / "events.jsonl").read_text().splitlines()]
    ev = next(r for r in rows if r["event"] == "report.write")
    assert ev["command"] == "review" and ev["status"] == "blocked" and ev["blockers"] == 1 and ev["next"] == 1
    assert CANARY not in json.dumps(rows)


# ---------- each long run ends with its page ----------


def _last_two(out: str) -> list[str]:
    return out.strip().splitlines()[-2:]


def test_start_page_lists_overdue_tasks_questions_and_the_next_commands(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T10:00")
    from careerbrew.day import inbox
    from careerbrew.work import priorities

    main(["init"])
    priorities.add(f"Ship the {CANARY} export", 4, 4, 1, [], due="2026-10-10")
    inbox.regenerate()
    assert main(["--no-watch", "--no-web", "--no-refresh"]) == 0
    page_line, line = _last_two(capsys.readouterr().out)
    assert page_line.startswith("page: ") and "/out/reports/start-" in page_line
    assert line.startswith("start: blocked: ") and "agents: careerbrew report latest --json" in line
    s = report.latest("start")
    assert s["blockers"][0]["label"] == "Task t001 is overdue" and s["blockers"][0]["command"] == "careerbrew work done t001 <hours>"
    questions = [x for x in s["next"] if x["label"].startswith("Answer question ")]
    assert questions and all("careerbrew day answer " in x["command"] for x in questions)
    assert s["next"][-1]["command"] == "careerbrew day next"
    assert CANARY not in json.dumps(s) and CANARY in Path(page_line[6:]).read_text()


def test_review_page_puts_pivot_metrics_under_blockers(home, capsys):
    from careerbrew.work import priorities

    main(["init"])
    priorities.add("a", 3, 3, 1, [])
    priorities.done("t001", 4)
    main(["review"])
    assert _last_two(capsys.readouterr().out)[1].startswith("review: blocked: ")
    s = report.latest("review")
    pivot = next(b for b in s["blockers"] if b["id"] == "estimate_ratio")
    assert pivot["label"] == "estimate_ratio is PIVOT" and "Split tasks" in pivot["why"]
    assert s["done"][0]["label"].startswith("Measured ") and s["counts"]["pivot"] >= 1
    assert any(x["label"].startswith("Act on estimate_ratio") for x in s["next"])


def test_work_collect_page_names_each_source_error_and_its_fix(home, monkeypatch, capsys):
    from careerbrew.work import evidence

    monkeypatch.setattr(evidence, "collect", lambda: {"new": 2, "total": 7, "errors": 1, "reasons": ["gh: gh failed (fix: run `gh auth status`): no auth"]})
    assert main(["work", "collect"]) == 1
    assert _last_two(capsys.readouterr().out)[1].startswith("work-collect: blocked: evidence: 2 new, 7 total, 1 source error(s); 1 done, 1 blocker(s)")
    s = report.latest("work-collect")
    assert s["blockers"] == [{"id": "source-1", "label": "Source gh failed", "why": "gh failed (fix: run `gh auth status`): no auth", "command": "gh auth status"}]
    assert [x["command"] for x in s["next"]] == ["careerbrew work collect", "careerbrew work promo", "careerbrew work gaps"]
    assert s["done"][0]["evidence"] == "2 new, 7 total, 1 source error(s)"


def test_day_plan_page_shows_each_block_with_its_title_on_the_page_only(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T08:00")
    from careerbrew.work import priorities

    main(["init"])
    (home / "day" / "calendars.yaml").write_text(f"calendars: [{{kind: ics, path: '{FIXTURES}/calendar/work.ics'}}]\n")
    priorities.add(f"Write the {CANARY} design doc", 3, 3, 1, ["design"])
    main(["day", "plan"])
    page_line = _last_two(capsys.readouterr().out)[0]
    s = report.latest("day-plan")
    assert s["done"][0]["label"] == "Planned 2026-10-19" and "meeting(s)" in s["done"][0]["evidence"]
    assert any(d["id"] == "t001" for d in s["done"]) and s["next"][0]["command"] == "careerbrew day next"
    assert CANARY not in json.dumps(s) and CANARY in Path(page_line[6:]).read_text()


def test_checkup_and_machine_ready_pages_have_one_item_per_check(home, capsys):
    from careerbrew import checkup

    main(["checkup", "--offline"])
    out = capsys.readouterr().out.strip().splitlines()
    assert out[-1].startswith("checkup: ") and out[-2].startswith("page: ")
    s = report.latest("checkup")
    assert len(s["done"]) + len(s["blockers"]) == len(checkup.STEPS)
    assert all(b["command"] for b in s["blockers"]), "every blocker names its fix"
    main(["machine", "ready"])
    s = report.latest("machine-ready")
    assert s["counts"]["checks"] == len(s["done"]) + len(s["blockers"]) and s["next"][-1]["command"] == "careerbrew machine ready"


def _only_page_and_summary(out: str, run: str) -> str:
    lines = out.strip().splitlines()
    assert len(lines) == 2, f"{run} prints its page path and one summary line instead of its step text:\n{out}"
    assert lines[0].startswith("page: ") and f"/out/reports/{run}-" in lines[0] and lines[1].startswith(f"{run}: ")
    return lines[1]


@pytest.mark.parametrize(
    ("args", "run", "headline"),
    [
        (["review"], "review", "metric(s): "),
        (["work", "collect"], "work-collect", "evidence: 0 new, 0 total, 0 source error(s)"),
        (["day", "plan"], "day-plan", "plan for 2026-10-19: "),
        (["checkup", "--offline"], "checkup", "checkup: "),
        (["machine", "ready"], "machine-ready", "corporate readiness: "),
        (["--no-watch", "--no-web", "--no-refresh"], "start", "for you: "),
    ],
)
def test_each_long_run_prints_only_its_page_path_and_one_summary_line(home, monkeypatch, capsys, args, run, headline):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-19T08:00")
    main(["init"])
    capsys.readouterr()
    main(args)
    line = _only_page_and_summary(capsys.readouterr().out, run)
    assert line.startswith(f"{run}: {report.latest(run)['status']}: ") and headline in line.split("; ")[0], "the key counts come first"
    main(["config", "set", "run_pages", "false"])
    capsys.readouterr()
    main(args)
    out = capsys.readouterr().out
    assert "page: " not in out and len(out.strip().splitlines()) >= 1, "with pages off, the step text prints as before"


def test_work_collect_puts_its_project_memory_notes_on_the_page_not_stdout(home, monkeypatch, capsys):
    from careerbrew.work import memory_sync

    monkeypatch.setattr(memory_sync, "sync_out", lambda allowed: {"notes": 3, "projects": 2, "removed": 1, "errors": 0, "reason": ""})
    main(["work", "collect"])
    _only_page_and_summary(capsys.readouterr().out, "work-collect")
    s = report.latest("work-collect")
    memory = [d for d in s["done"] if d["id"] == "memory"]
    assert memory and memory[0]["evidence"] == "3 note(s) written to 2 project(s); 1 note(s) of events now personal removed"
    assert (s["counts"]["memory_notes"], s["counts"]["memory_removed"]) == (3, 1)
    main(["config", "set", "run_pages", "false"])
    capsys.readouterr()
    main(["work", "collect"])
    assert "project memory: 3 note(s) written to 2 project(s)" in capsys.readouterr().out, "with pages off, the step text prints as before"


def test_report_latest_prints_the_page_items_as_text(home, capsys):
    report.write(_run())
    assert main(["report", "latest"]) == 0
    out = capsys.readouterr().out
    assert "done     Measured 3 metrics: 2 KEEP, 1 PIVOT" in out
    assert "blocker  estimate_ratio is PIVOT: value 4.0, pivot > 2.0 -> fix: careerbrew review" in out
    assert "next 1. Split the big task <script>alert(1)</script> -> careerbrew open --view week" in out
    assert CANARY not in out, "detail stays on the page"


def test_plugin_new_page_lists_each_token_step_and_keeps_the_company_name_out_of_stdout(home, tmp_path, capsys):
    from careerbrew import wizard

    rc = main(["plugin", "new", "Globex Corp", "--dir", str(tmp_path / "globex"), "--connect", "jira,github", "--jira-url", "https://jira.globex.example.com", "--github-user", "me", "--no-prompt"])
    assert rc == 0
    line = _only_page_and_summary(capsys.readouterr().out, "plugin-new")
    assert line.startswith("plugin-new: blocked: plugin created and installed: 2 connector(s), 2 token(s) to type; ")
    assert "globex" not in line.lower(), "the company name is a sensitive term"
    s = report.latest("plugin-new")
    pfx = wizard.service_prefix("globex-corp")
    tokens = [x for x in s["next"] if x["id"].startswith("token-")]
    assert [x["command"] for x in tokens] == [f"careerbrew secret set {pfx}.jira", f"careerbrew secret set {pfx}.github"]
    assert all("create it at https://" in x["label"] for x in tokens), "each token step says where to create it"
    assert s["blockers"][0]["id"] == "jira-email" and s["blockers"][0]["command"] == "careerbrew data edit work/connectors.yaml"
    assert {x["command"] for x in s["next"]} >= {"careerbrew plugin refresh", "careerbrew machine ready"}
    text = json.dumps(s).lower()
    assert "globex-corp" not in text and "globex corp" not in text and "plugin [redacted]" in text, "the summary is redacted like stdout"


def test_machine_new_user_page_has_token_steps_readiness_blockers_and_fixes(home, tmp_path, capsys):
    main(["plugin", "new", "Globex Corp", "--dir", str(tmp_path / "plugin"), "--connect", "jira", "--jira-url", "https://jira.globex.example.com", "--no-prompt"])
    repos = tmp_path / "apps"
    for name in ("svc-a", "svc-b"):
        (repos / name).mkdir(parents=True)
        subprocess.run(["git", "init", "-q", str(repos / name)], check=True)
    capsys.readouterr()
    rc = main(["machine", "new-user", "--repos", str(repos), "--email", "peer@example.com", "--name", "Pat Example"])
    line = _only_page_and_summary(capsys.readouterr().out, "machine-new-user")
    assert line.startswith("machine-new-user: blocked: 2 git repo(s) as evidence sources, 1 connector(s), 1 token(s) to type; corporate readiness: ")
    s = report.latest("machine-new-user")
    assert rc == 1 and s["status"] == "blocked" and s["counts"]["failed"] == len(s["blockers"]) > 0
    assert s["done"][0]["id"] == "setup" and s["done"][0]["evidence"].startswith("2 git repo(s) as evidence sources, connectors jira")
    assert s["next"][0]["id"] == "token-jira" and s["next"][0]["command"].startswith("careerbrew secret set careerbrew.p-"), "the token steps come first"
    assert all(b["command"] for b in s["blockers"]), "every failed readiness check names its fix"
    assert s["next"][-1]["command"] == f"gh issue create -R {readme_repo()}" and s["next"][-2]["command"] == "careerbrew machine ready"
    assert "peer@example.com" not in json.dumps(s)


def test_switching_to_corporate_removes_run_pages(home):
    from careerbrew import machine

    main(["init"])
    report.write(_run())
    machine.set_role("corporate", "")
    assert not list((home / "out" / "reports").glob("*")), "run pages may quote job-hunt items"


# ---------- scripts/checkup.sh ----------

STUBS = {
    "open-prs.sh": "printf 'OPEN  #101 feat: run pages (branch feat/x, updated 2026-10-22)\\n      next: review, then merge when the gate is green\\n'\necho '-> finish open PRs before new work: gh pr checkout <number>'\nexit 1",
    "doctor.sh": "echo 'ok    git installed'\necho 'FAIL  gitleaks missing -> fix: brew install gitleaks'\nexit 1",
    "check.sh": "echo 'check.sh: docs cached (inputs unchanged)'\necho 'check.sh: full gate passed'",
    "metrics.sh": "echo 'step runs avg_s'\necho 'unit 3 4.0'",
    "open-work.sh": "echo none",
}


def _stub_repo(tmp_path: Path, checkup_sh: str | None = None) -> Path:
    repo = tmp_path / "repo"
    (repo / "scripts").mkdir(parents=True)
    (repo / "scripts" / "checkup.sh").write_text(checkup_sh or (ROOT / "scripts" / "checkup.sh").read_text())
    shutil.copy(ROOT / "scripts" / "report-html.sh", repo / "scripts" / "report-html.sh")  # the one renderer's shell entry
    for name, body in STUBS.items():
        (repo / "scripts" / name).write_text("#!/usr/bin/env bash\n" + body + "\n")
        (repo / "scripts" / name).chmod(0o755)
    (repo / ".venv").symlink_to(ROOT / ".venv")
    (repo / "TASKS.md").write_text("# Tasks\n\n## P0\n\n- [ ] Fix the broken export story\n  - **ID**: export-fix\n\n## P1\n")
    (repo / ".metrics").mkdir()
    (repo / ".metrics" / "events.jsonl").write_text('{"event":"x","ok":false,"story":"us-07"}\n' * 2)
    return repo


def _checkup_sh(repo: Path, tmp_path: Path) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "WRAP_UP_SKILL": str(tmp_path / "none"), "NEW_PROJECT_SKILL": str(tmp_path / "none")}
    return subprocess.run(["bash", str(repo / "scripts" / "checkup.sh")], capture_output=True, text=True, env=env, check=False, timeout=120)


def test_checkup_sh_writes_its_page_and_prints_the_path_and_one_summary_line(home, tmp_path):
    r = _checkup_sh(_stub_repo(tmp_path), tmp_path)
    assert r.returncode == 1, r.stderr
    lines = r.stdout.strip().splitlines()
    assert len(lines) == 2, r.stdout
    assert lines[0].startswith(f"page: {home}/out/reports/repo-checkup-") and lines[1].startswith("repo-checkup: blocked: ")
    assert ".. Health" in r.stderr, "progress goes to stderr while the sections run"
    s = report.latest("repo-checkup")
    assert s["personal"] is False, "repo facts only: an agent may publish this page"
    assert s["next"][0] == {"id": "pr-101", "label": "Finish PR #101 (feat: run pages): review, then merge when the gate is green", "command": "gh pr checkout 101"}
    blockers = {b["label"]: b for b in s["blockers"]}
    assert blockers["Health: gitleaks missing"]["command"] == "brew install gitleaks"
    assert "Story us-07 is broken" in blockers and "1 open pull request(s) to finish first" in blockers
    assert any(x["label"] == "P0 task: Fix the broken export story" and x["command"] == "/next-task" for x in s["next"])
    assert {d["label"] for d in s["done"]} >= {"Gate passed", "No open work", "Speed report"}
    assert "scripts/doctor.sh" in s["commands"] and s["counts"]["exit_code"] == 1
    page = Path(lines[0][6:]).read_text()
    assert "check.sh: full gate passed" in page and "<summary>Gate (cached by input hash" in page, "each section's text is folded under Details"
    assert (tmp_path / "repo" / ".metrics" / "checkup.html").read_text() == page, "a copy stays at .metrics/checkup.html"


# The wrap-up skill's open-work.sh prints the repo path, then one `  <kind>     <text>` line per item
# (its say() is printf '  %-9s %s\n'), and exits 1 when anything is open. checkup.sh adds its "->" line.
SKILL_OPEN_WORK = (
    "#!/usr/bin/env bash\n"
    'echo "$PWD"\n'
    "printf '  %-9s %s\\n' dirty '2 uncommitted or untracked file(s) on main'\n"
    "printf '  %-9s %s\\n' branch 'feat/x: 3 commit(s) not on main'\n"
    "printf '  %-9s %s\\n' pr '#12 ready, checks red, mergeable feat/x: synthetic title'\n"
    "exit 1\n"
)


def test_open_work_from_the_wrap_up_skill_is_a_blocker_with_next_steps(home, tmp_path):
    """Bug: the skill's indented `  <kind>  <text>` lines were not items, so open work showed as "No open work"."""
    skill = tmp_path / "wrap-up"
    (skill / "scripts").mkdir(parents=True)
    (skill / "scripts" / "open-work.sh").write_text(SKILL_OPEN_WORK)
    repo = _stub_repo(tmp_path)
    env = {**os.environ, "WRAP_UP_SKILL": str(skill), "NEW_PROJECT_SKILL": str(tmp_path / "none")}
    r = subprocess.run(["bash", str(repo / "scripts" / "checkup.sh")], capture_output=True, text=True, env=env, check=False, timeout=120)
    assert r.returncode == 1, r.stderr
    s = report.latest("repo-checkup")
    done = {d["id"]: d for d in s["done"]}
    assert done["open-work-skill"]["label"] == "Listed 3 open work item(s)", "open work is never reported as none"
    assert done["open-work-skill"]["evidence"] == "1 dirty, 1 branch, 1 pr"
    blocker = next(b for b in s["blockers"] if b["id"] == "open-work-skill")
    assert blocker["label"] == "3 open work item(s) to finish first" and "draft PR" in blocker["why"]
    steps = {x["label"]: x["command"] for x in s["next"]}
    assert steps["Continue: branch feat/x: 3 commit(s) not on main"] == "git switch feat/x"
    assert steps["Continue: pr #12 ready, checks red, mergeable feat/x: synthetic title"] == "gh pr checkout 12"
    assert steps["Continue: dirty 2 uncommitted or untracked file(s) on main"] == "git status"


def test_open_work_skill_that_fails_without_items_is_a_blocker(tmp_path):
    logs = tmp_path / "logs"
    logs.mkdir()
    for ext, text in ((".title", "Open work"), (".cmd", "wrap-up skill: scripts/open-work.sh"), (".log", "open-work: not a git checkout: .\n"), (".rc", "2")):
        (logs / f"01-open-work-skill{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    assert not any(d.id == "open-work-skill" for d in r.done), "a failed check never says there is no open work"
    assert [(b.label, b.why) for b in r.blockers] == [("Open work could not be checked", "open-work: not a git checkout: .")]


def test_branches_open_work_finds_likely_landed_are_cleanup_not_blockers(tmp_path):
    """Issue #255: a branch whose work landed through other PRs blocked the checkup. scripts/open-work.sh tags it
    "likely landed"; the page lists it, and its worktree from the wrap-up skill, as cleanup, and blocks only on the rest."""
    logs = tmp_path / "logs"
    logs.mkdir()
    skill = "~/repo\n  branch    feat/old: 1 commit(s) not on main\n  worktree  ~/wt/old [feat/old]: 1 commit(s) not on main, 0 uncommitted file(s)\n  branch    feat/new: 2 commit(s) not on main\n"
    own = "- branch feat/old: likely landed (95% of its added lines are on main): look, then delete the branch\n- branch feat/new: 2 commit(s) not in main\n-> continue these before starting a new task (finish, or wrap up into a draft PR)\n"
    for sid, title, cmd, log, rc in (
        ("01-open-work-skill", "Open work", "wrap-up skill: scripts/open-work.sh", skill, "1"),
        ("02-open-work", "Open work", "scripts/open-work.sh", own, "0"),
    ):
        for ext, text in ((".title", title), (".cmd", cmd), (".log", log), (".rc", rc)):
            (logs / f"{sid}{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    blocker = next(b for b in r.blockers if b.id == "open-work-skill")
    assert blocker.label == "1 open work item(s) to finish first" and blocker.command == "git switch feat/new"
    labels = {x.label: x.command for x in r.next}
    assert labels["Clean up: branch feat/old: likely landed (95% of its added lines are on main)"] == "git log --oneline main..feat/old"
    assert labels["Clean up: worktree ~/wt/old [feat/old]: likely landed"] == "git log --oneline main..feat/old"
    assert "Continue: branch feat/old: 1 commit(s) not on main" not in labels


def test_only_likely_landed_open_work_does_not_block_the_checkup(tmp_path):
    logs = tmp_path / "logs"
    logs.mkdir()
    for sid, cmd, log, rc in (
        ("01-open-work-skill", "wrap-up skill: scripts/open-work.sh", "~/repo\n  branch    feat/old: 1 commit(s) not on main\n", "1"),
        ("02-open-work", "scripts/open-work.sh", "- branch feat/old: likely landed (91% of its added lines are on main): look, then delete the branch\n", "0"),
    ):
        for ext, text in ((".title", "Open work"), (".cmd", cmd), (".log", log), (".rc", rc)):
            (logs / f"{sid}{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    assert not [b for b in r.blockers if b.id.startswith("open-work")], r.blockers


def test_a_landed_branchs_worktree_with_uncommitted_files_still_blocks(tmp_path):
    """Review of #255: uncommitted files are work, even when the worktree's branch landed."""
    logs = tmp_path / "logs"
    logs.mkdir()
    for sid, cmd, log, rc in (
        ("01-open-work-skill", "wrap-up skill: scripts/open-work.sh", "~/repo\n  worktree  ~/wt/old [feat/old]: 1 commit(s) not on main, 2 uncommitted file(s)\n", "1"),
        ("02-open-work", "scripts/open-work.sh", "- branch feat/old: likely landed (91% of its added lines are on main): look, then delete the branch\n", "0"),
    ):
        for ext, text in ((".title", "Open work"), (".cmd", cmd), (".log", log), (".rc", rc)):
            (logs / f"{sid}{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    assert [b.label for b in r.blockers if b.id == "open-work-skill"] == ["1 open work item(s) to finish first"]
    assert not [x for x in r.next if x.label.startswith("Clean up: worktree")]


def test_without_the_skill_a_landed_branch_is_cleanup_and_the_rest_blocks(tmp_path):
    """Review of #255: with no wrap-up skill, a likely-landed branch still shows, as cleanup."""
    logs = tmp_path / "logs"
    logs.mkdir()
    own = "- branch feat/old: likely landed (95% of its added lines are on main): look, then delete the branch\n- branch feat/new: 2 commit(s) not in main\n"
    for ext, text in ((".title", "Open work"), (".cmd", "scripts/open-work.sh"), (".log", own), (".rc", "1")):
        (logs / f"01-open-work{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    labels = {x.label: x.command for x in r.next}
    assert labels["Clean up: branch feat/old: likely landed (95% of its added lines are on main)"] == "git log --oneline main..feat/old"
    assert [b.label for b in r.blockers if b.id == "open-work"] == ["1 open work item(s) to finish first"]


def test_a_never_pushed_branch_offers_a_retire_command_and_a_retired_one_does_not_block(tmp_path):
    """Issue #277: a never-pushed branch whose work landed reworded blocked every checkup, and its only exit was a forced
    delete. The page offers the archive-tag retire command; once open-work.sh reports it retired, neither the branch
    nor its clean worktree from the wrap-up skill blocks."""
    logs = tmp_path / "logs"
    logs.mkdir()
    skill = "~/repo\n  branch    fix/old: 8 commit(s) not on main\n  branch    feat/new: 2 commit(s) not on main\n"
    own = "- branch fix/old: 8 commit(s) not in main, never pushed (if its work landed elsewhere, retire it: git tag archive/fix/old fix/old)\n- branch feat/new: 2 commit(s) not in main\n"
    for sid, cmd, log, rc in (("01-open-work-skill", "wrap-up skill: scripts/open-work.sh", skill, "1"), ("02-open-work", "scripts/open-work.sh", own, "0")):
        for ext, text in ((".title", "Open work"), (".cmd", cmd), (".log", log), (".rc", rc)):
            (logs / f"{sid}{ext}").write_text(text)
    r = runs.repo_checkup(logs, 1)
    labels = {x.label: x.command for x in r.next}
    assert labels["Retire: branch fix/old (only if its work landed elsewhere)"] == "git tag archive/fix/old fix/old"
    assert [b.label for b in r.blockers if b.id == "open-work-skill"] == ["2 open work item(s) to finish first"]

    skill = "~/repo\n  branch    fix/old: 8 commit(s) not on main\n  worktree  ~/wt/old [fix/old]: 8 commit(s) not on main, 0 uncommitted file(s)\n"
    own = "(retired: branch fix/old matches tag archive/fix/old; a new commit lists it again)\nnone\n"
    (logs / "01-open-work-skill.log").write_text(skill)
    (logs / "02-open-work.log").write_text(own)
    r = runs.repo_checkup(logs, 1)
    assert not [b for b in r.blockers if b.id.startswith("open-work")], r.blockers
    assert not [x for x in r.next if "fix/old" in x.label], r.next


def test_checkup_sh_prints_the_sections_as_text_when_pages_are_off(home, tmp_path):
    main(["config", "set", "run_pages", "false"])
    r = _checkup_sh(_stub_repo(tmp_path), tmp_path)
    assert r.returncode == 1 and "== Health" in r.stdout and "FAIL  gitleaks missing" in r.stdout
    assert r.stdout.rstrip().endswith("checkup: needs work (see above)") and "page:" not in r.stdout
    assert report.latest() is None


# ---------- the web app and `open --report` ----------


@pytest.fixture
def api(home):
    from careerbrew import server

    srv, _t = server.serve_in_thread(0, "tok")
    port = srv.server_address[1]

    def call(path, token="tok"):
        req = urllib.request.Request(f"http://127.0.0.1:{port}{path}", headers={"X-Careerbrew-Token": token, "Host": f"127.0.0.1:{port}"})
        try:
            with urllib.request.urlopen(req, timeout=10) as resp:
                return resp.status, json.loads(resp.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"null")

    yield call
    srv.shutdown()
    srv.server_close()


def test_web_app_serves_run_pages_behind_its_token(api):
    page = report.write(_run())
    assert api("/api/reports", token="wrong")[0] == 401
    assert api("/api/reports/latest", token="wrong")[0] == 401
    status, data = api("/api/reports")
    assert status == 200 and [s["id"] for s in data["reports"]] == [page.stem] and CANARY not in json.dumps(data)
    status, one = api(f"/api/reports/{page.stem}")
    assert status == 200 and one["summary"]["id"] == page.stem and one["html"] == page.read_text()
    assert api("/api/reports/latest")[1]["summary"]["id"] == page.stem
    assert api("/api/reports/..%2F..%2Fsettings.yaml")[0] == 404


def test_the_api_event_names_a_run_page_route_without_its_id(api, home, monkeypatch):
    page = report.write(_run())
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    api("/api/reports")
    api(f"/api/reports/{page.stem}")
    rows = [json.loads(line) for line in (home / "logs" / "events.jsonl").read_text().splitlines()]
    assert [r["route"] for r in rows if r["event"] == "web.api"] == ["reports", "reports.page"]
    assert page.stem not in json.dumps(rows), "a path parameter never reaches the log"


def test_open_report_latest_opens_the_newest_page(home, monkeypatch, capsys):
    import webbrowser

    from careerbrew import server

    page = report.write(_run())
    opened: list[str] = []
    monkeypatch.setattr(webbrowser, "open", lambda url: opened.append(url) or True)
    monkeypatch.setattr(server, "app_url", lambda port, view=None: f"http://127.0.0.1:{port}/#/{view}&token=t")
    monkeypatch.setattr(server, "dist_exists", lambda: True)
    assert main(["open", "--report", "latest"]) == 0
    assert opened[-1].endswith(f"#/reports/{page.stem}&token=t")
    assert "token" not in capsys.readouterr().out, "the token goes only to the browser"
    monkeypatch.setattr(server, "dist_exists", lambda: False)
    assert main(["open", "--report", "review"]) == 0 and opened[-1] == page.as_uri()
    assert main(["open", "--report", "nope-20261019-090000"]) == 1


def test_agent_commands_end_by_showing_the_page_and_asking_with_its_next_items():
    start = (ROOT / "commands" / "careerbrew.md").read_text()
    check = (ROOT / "commands" / "careerbrew-checkup.md").read_text()
    for text in (start, check):
        assert "careerbrew report latest" in text and "--json" in text, "agents read the content-free summary"
        assert "Next items as the options" in text, "then ask the user which next action to take"
    assert "careerbrew open --report start" in start and "Never publish it" in start, "a personal page is opened locally"
    assert "may publish the page file as an Artifact" in check and '"personal": false' in check, "the repo checkup page holds no personal data"
    assert "Bash(uv run careerbrew report:*)" in check, "the checkup command may read the summary"


def test_command_in_finds_the_fix():
    assert runs.command_in("Run `careerbrew work collect` and add notes") == "careerbrew work collect"
    assert runs.command_in("could not list open PRs: tls -> fix: gh auth status (outside the agent sandbox)") == "gh auth status"
    assert runs.command_in("Split tasks over 4 hours") == ""


def test_the_shell_stub_harness_matches_the_real_scripts():
    """The stubs stand in for real scripts: if one is renamed, this test names it."""
    for name in STUBS:
        assert (ROOT / "scripts" / name).exists(), name
    assert shutil.which("bash")
