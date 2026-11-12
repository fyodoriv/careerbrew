"""Issue #260: the work day starts with one refresh (fast-forward only), a shell preflight, and one sign-in probe.

Every checkout here is a temp git repo with a temp bare remote; the sign-in probe is a fake (`true` / `false`).
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path

import pytest
import yaml

from careerbrew import cli, config, paths, plugins, runs
from careerbrew.workday import plan, start, status, steps


@pytest.fixture(autouse=True)
def pinned(monkeypatch, tmp_path):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")
    monkeypatch.setattr(steps, "_roots", lambda: [paths.REPO_ROOT, tmp_path / "agent-home"])  # never the owner's ~/.claude skills, so no real checkout is refreshed
    monkeypatch.setenv("GIT_CONFIG_GLOBAL", "/dev/null")  # never the owner's hooks or settings
    monkeypatch.setenv("GIT_CONFIG_NOSYSTEM", "1")
    for k in ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"):
        monkeypatch.delenv(k, raising=False)


def git(where: Path, *args: str) -> str:
    return subprocess.run(["git", "-C", str(where), "-c", "user.name=Synthetic", "-c", "user.email=synthetic@example.com", *args], capture_output=True, text=True, check=True).stdout.strip()


def remote_with_clone(tmp_path: Path, name: str) -> tuple[Path, Path]:
    """(bare remote, a clone of it with one commit on main tracking origin/main)."""
    bare = tmp_path / f"{name}.git"
    subprocess.run(["git", "init", "--quiet", "--bare", "-b", "main", str(bare)], check=True)
    clone = tmp_path / name
    subprocess.run(["git", "clone", "--quiet", str(bare), str(clone)], check=True, capture_output=True)
    git(clone, "checkout", "--quiet", "-b", "main")
    (clone / "README.md").write_text("synthetic\n")
    git(clone, "add", "README.md")
    git(clone, "commit", "--quiet", "-m", "first")
    git(clone, "push", "--quiet", "-u", "origin", "main")
    return bare, clone


def push_new_commit(tmp_path: Path, bare: Path, n: int = 1) -> str:
    """Another clone pushes `n` commits, so every other clone is behind. Returns the remote's new head."""
    other = tmp_path / f"other-{bare.stem}-{n}"
    subprocess.run(["git", "clone", "--quiet", str(bare), str(other)], check=True, capture_output=True)
    for i in range(n):
        (other / f"new-{i}.txt").write_text(f"{i}\n")
        git(other, "add", ".")
        git(other, "commit", "--quiet", "-m", f"new {i}")
    git(other, "push", "--quiet", "origin", "main")
    return git(other, "rev-parse", "HEAD")


def _plugin(tmp_path: Path, work_steps, folder: Path | None = None) -> Path:
    cli.main(["init"])
    d = folder or tmp_path / "plug"
    (d / "skills").mkdir(parents=True, exist_ok=True)
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "start-co", "version": "1.0.0", "work_steps": work_steps, "provides": {"skills": "skills"}}))
    plugins.add(str(d))
    return d


def test_the_plan_json_starts_with_the_refresh_and_preflight(tmp_path, capsys):
    cli.main(["init"])
    config.set_value("work_checkouts", "")
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    p = json.loads(capsys.readouterr().out)
    assert list(p)[0] == "start_of_day", "the plan opens with the start of the day"
    sod = p["start_of_day"]
    assert sod["command"] == "careerbrew day start" and "Before stage 1" in sod["rule"]
    assert sod["traps"].endswith("skills/careerbrew-work-day/shell-traps.md") and start.TRAPS.is_file(), "one shared traps reference every step loads"
    assert sod["signin"]["status"] == "not run" and sod["ran_at"] == ""
    assert p["flow"].startswith("First run `start_of_day`")


def test_a_behind_checkout_is_fast_forwarded_before_stage_1(tmp_path, capsys):
    bare, clone = remote_with_clone(tmp_path, "app")
    new_head = push_new_commit(tmp_path, bare, 2)
    cli.main(["init"])
    config.set_value("work_checkouts", str(clone))
    assert [c["path"] for c in plan.build()["start_of_day"]["checkouts"]] == [str(clone.resolve())], "careerbrew's own checkout is left out when the setting leaves it out"
    capsys.readouterr()
    assert cli.main(["day", "start"]) == 0
    out = capsys.readouterr().out
    assert git(clone, "rev-parse", "HEAD") == new_head, "the behind checkout is at the remote's head"
    assert "fast-forwarded 2 commit(s)" in out and "1 fast-forwarded, 0 not touched" in out
    sod = plan.build()["start_of_day"]
    assert sod["refresh"][0]["status"] == "fast-forwarded" and sod["refresh"][0]["after"] == new_head[:12]
    assert sod["ran_at"] == "2026-10-28T09:00:00+00:00"
    assert all(r["status"] == "waiting" for r in plan.build()["dashboard"]["rows"]), "the refresh runs before any step starts"


def test_local_work_and_a_diverged_branch_are_reported_and_never_touched(tmp_path):
    bare, dirty = remote_with_clone(tmp_path, "dirty")
    push_new_commit(tmp_path, bare)
    (dirty / "README.md").write_text("my unsaved edit\n")
    before_dirty = git(dirty, "rev-parse", "HEAD")
    bare2, diverged = remote_with_clone(tmp_path, "diverged")
    push_new_commit(tmp_path, bare2)
    (diverged / "mine.txt").write_text("local\n")
    git(diverged, "add", "mine.txt")
    git(diverged, "commit", "--quiet", "-m", "local work")
    before_diverged = git(diverged, "rev-parse", "HEAD")
    _bare3, ahead = remote_with_clone(tmp_path, "ahead")
    (ahead / "mine.txt").write_text("local\n")
    git(ahead, "add", "mine.txt")
    git(ahead, "commit", "--quiet", "-m", "unpushed")
    git(ahead, "checkout", "--quiet", "--detach")
    lone = tmp_path / "lone"
    lone.mkdir()
    subprocess.run(["git", "init", "--quiet", str(lone)], check=True)
    got = {p.name: start.refresh_one(str(p))["status"] for p in (dirty, diverged, ahead, lone, tmp_path / "gone")}
    assert got == {"dirty": "local changes", "diverged": "diverged", "ahead": "detached", "lone": "no upstream", "gone": "missing"}
    assert (dirty / "README.md").read_text() == "my unsaved edit\n" and git(dirty, "rev-parse", "HEAD") == before_dirty
    assert git(diverged, "rev-parse", "HEAD") == before_diverged
    git(ahead, "checkout", "--quiet", "main")
    assert start.refresh_one(str(ahead))["status"] == "local commits"
    assert start.refresh_one(str(tmp_path))["status"] == "not a git checkout"


def test_the_plan_lists_each_steps_source_and_commit_and_watches_one_checkout_per_remote(tmp_path, monkeypatch):
    bare, folder = remote_with_clone(tmp_path, "plugin-src")
    (folder / "skills" / "team-sync").mkdir(parents=True)
    (folder / "skills" / "team-sync" / "SKILL.md").write_text("---\nname: team-sync\n---\n")
    (folder / "skills" / "notes").mkdir(parents=True)
    (folder / "skills" / "notes" / "SKILL.md").write_text("---\nname: notes\n---\n")
    _plugin(tmp_path, [{"step": "/team-sync", "scope": "team"}], folder)
    git(folder, "add", ".")
    git(folder, "commit", "--quiet", "-m", "plugin")
    git(folder, "push", "--quiet")
    plugins.refresh()
    config.set_value("work_checkouts", "careerbrew")
    assert {s["step"]: s for s in plan.build()["steps"]}["/careerbrew"]["source"]["commit"], "careerbrew's own step names its commit"
    # An agent's skills folder links a skill into a managed clone of the plugin's repo (as agentbrew installs it).
    managed = tmp_path / "managed-clone"
    subprocess.run(["git", "clone", "--quiet", str(bare), str(managed)], check=True, capture_output=True)
    agent = tmp_path / "agent"
    (agent / "skills").mkdir(parents=True)
    (agent / "skills" / "notes").symlink_to(managed / "skills" / "notes")
    monkeypatch.setattr(steps, "_roots", lambda: [agent])
    config.set_value("work_steps", "/notes")
    p = plan.build()
    by = {s["step"]: s for s in p["steps"]}
    assert by["/team-sync"]["source"] == {"path": str(folder.resolve()), "commit": git(folder, "rev-parse", "--short=12", "HEAD")}, "a plugin step's source is the plugin's folder, not the installed copy"
    assert by["/notes"]["source"]["path"] == str(managed.resolve()), "a linked skill's source is the clone it links into"
    sod = p["start_of_day"]
    paths_ = {c["path"]: c for c in sod["checkouts"]}
    assert paths_[str(folder.resolve())]["for"] == ["plugin:start-co", "step:/team-sync"]
    assert paths_[str(managed.resolve())] == {"path": str(managed.resolve()), "for": ["step:/notes"], "commit": paths_[str(folder.resolve())]["commit"], "duplicate_of": str(folder.resolve())}, (
        "the managed clone is refreshed but is a duplicate of the dev checkout"
    )
    assert str(folder.resolve()) in sod["watch"] and str(managed.resolve()) not in sod["watch"], "one folder per remote is watched: the dev checkout"
    assert str(bare) not in json.dumps(p), "the remote URL is never shown"


def test_a_failed_signin_probe_yields_one_question_and_moves_signin_steps_later(tmp_path, capsys):
    _plugin(tmp_path, [{"step": "/review", "uses": ["browser:sso"]}, {"step": "/drafts", "uses": ["browser:sso", "slack-draft:team"]}, {"step": "/notes", "scope": "notes"}, {"step": "/after-review", "scope": "x", "after": "/review"}])
    config.set_value("work_checkouts", "")
    before = {s["step"]: s["stage"] for s in plan.build()["steps"]}
    assert before["/review"] == before["/notes"] == 2, "without a failed probe, sign-in steps run in their declared place"
    config.set_value("work_signin_probe", "false")
    capsys.readouterr()
    assert cli.main(["day", "start"]) == 0
    out = capsys.readouterr().out
    assert "sign-in probe failed" in out and out.count("needs you:") == 1
    p = plan.build()
    qs = p["start_of_day"]["questions"]
    assert len(qs) == 1 and qs[0]["id"] == "signin" and qs[0]["steps"] == ["/review", "/drafts"], "one question for the whole day"
    st = {s["step"]: s["stage"] for s in p["steps"]}
    assert st["/review"] > st["/notes"] and st["/drafts"] > st["/notes"], "sign-in steps go after the others"
    assert st["/after-review"] > st["/review"], "a step after a sign-in step still waits for it"
    assert "sign-in probe failed" in {s["step"]: s for s in p["steps"]}["/review"]["why"]
    page = runs.work_day(p)
    assert page.next[0].id == "ask-signin" and page.next[0].command == "careerbrew day start", "the page asks it first"
    config.set_value("work_signin_probe", "true")
    cli.main(["day", "start"])
    p = plan.build()
    assert p["start_of_day"]["questions"] == [] and {s["step"]: s["stage"] for s in p["steps"]} == before, "once signed in, the plan is as declared"


def test_no_probe_and_no_signin_steps_ask_nothing():
    assert start.signin([{"step": "/a", "uses": []}], "false") == ({"status": "none needed", "steps": [], "why": ""}, [])
    rec, qs = start.signin([{"step": "/a", "uses": ["browser:sso"]}], "")
    assert rec["status"] == "no probe" and qs == []


def test_the_preflight_names_each_trap_and_its_safe_form():
    answers = {"sed": (0, "sed (GNU sed) 4.9"), "bash": (0, "3"), "sh": (0, "/usr/bin/python3")}
    got = {c["check"]: c for c in start.preflight({"SHELL": "/bin/zsh"}, runner=lambda argv, seconds=10: answers[argv[0]])}
    assert set(got) == {"shell", "sed", "bash", "python3", "push guard", "goal and cron", "installed-command", "command-paths", "plan-steps", "step-files"}
    assert (got["shell"]["value"], got["shell"]["status"]) == ("zsh", "warn") and "${=F}" in got["shell"]["safe"]
    assert got["sed"]["value"] == "GNU" and "sed -i ''" in got["sed"]["safe"]
    assert (got["bash"]["value"], got["bash"]["ok"]) == ("3", False) and "declare -A" in got["bash"]["safe"]
    assert "uv run python -m unittest" in got["python3"]["safe"]
    bsd = {c["check"]: c for c in start.preflight({"SHELL": "/bin/bash"}, runner=lambda argv, seconds=10: (1, "illegal option") if argv[0] == "sed" else (0, "/synthetic/bin/sed") if argv[-1] == "command -v sed" else (0, "5"))}
    assert bsd["sed"]["value"] == "BSD" and bsd["bash"]["ok"] and bsd["shell"]["ok"]
    traps = start.TRAPS.read_text()
    for needle in ("${=F}", "declare -A", "sed -i ''", "$VAR", "uv run python -m unittest", ".venv/bin/python"):
        assert needle in traps, needle


def test_a_verify_with_a_bare_python_is_refused():
    for bare in ("python3 -m unittest", "python -m pytest", "env python3 -m unittest", "python3.12 -m unittest", "/usr/bin/env -S VAR=1 python3 -m x", "sh -c 'python3 -m unittest'"):
        with pytest.raises(steps.StepError, match="names its interpreter"):
            steps.entry({"step": "/t", "verify": bare}, "plugin:x")
    assert steps.entry({"step": "/t", "verify": ".venv/bin/python -m unittest"}, "plugin:x")["verify"]
    assert steps.entry({"step": "/t", "verify": "uv run python -m unittest"}, "plugin:x")["verify"]


def test_the_start_record_survives_row_updates_and_settings_are_checked(tmp_path):
    cli.main(["init"])
    config.set_value("work_checkouts", "")
    cli.main(["day", "start"])
    at = status.start_record()["at"]
    cli.main(["day", "update", "/careerbrew", "--status", "running"])
    assert status.start_record()["at"] == at and status.rows(["/careerbrew"])[0]["status"] == "running"
    cli.main(["day", "start"])
    assert status.rows(["/careerbrew"])[0]["status"] == "running", "a second start keeps the rows"
    with pytest.raises(ValueError, match="full path"):
        config.set_value("work_checkouts", "relative/path")
    with pytest.raises(ValueError, match="unclosed quote"):
        config.set_value("work_signin_probe", "sh -c 'oops")


def test_an_agent_sees_a_plugin_folder_as_its_label_and_a_url_plugin_has_no_source(tmp_path, capsys):
    from .conftest import CANARIES

    _bare, folder = remote_with_clone(tmp_path, f"careerbrew-{CANARIES[1].lower()}")
    _bare2, other = remote_with_clone(tmp_path, f"{CANARIES[1].lower()}-tools")
    _plugin(tmp_path, [{"step": "/team-sync", "scope": "team"}, {"step": "/tidy", "uses": [f"checkout:{other}"]}, {"step": "/tidy-too", "uses": [f"checkout:{other}"]}], folder)
    git(folder, "add", ".")
    git(folder, "commit", "--quiet", "-m", "plugin")
    config.set_value("work_checkouts", f"careerbrew, {other}")
    assert str(folder.resolve()) in [c["path"] for c in plan.build()["start_of_day"]["checkouts"]], "the owner's own plan has the folder"
    capsys.readouterr()
    cli.main(["day", "run", "--json"])
    cli.main(["day", "start"])
    cli.main(["day", "start", "--json"])
    out = capsys.readouterr().out
    assert CANARIES[1].lower() not in out.lower(), "an agent never sees the plugin's folder name"
    assert "plugin:start-co" in out and "setting" in out
    assert start.shown(paths.REPO_ROOT.resolve()) in out or str(paths.REPO_ROOT) in out, "careerbrew's own checkout still shows as a path"
    lock = {"start-co": ""}  # installed from a URL: no local folder
    assert start.step_source({"step": "/team-sync", "from": "plugin:start-co", "path": str(folder / "skills" / "x" / "SKILL.md")}, lock) is None, "no source rather than careerbrew's own checkout"
    copied = plugins.commands_dir() / "team-sync.md"
    copied.parent.mkdir(parents=True, exist_ok=True)
    copied.write_text("# copied\n")
    assert start.step_source({"step": "/team-sync", "from": "settings", "path": str(copied)}, {}) is None, "a plugin's copied command is not a source checkout"


def test_the_plan_shows_the_preflight_before_the_day_starts():
    cli.main(["init"])
    config.set_value("work_checkouts", "")
    checks = plan.build()["start_of_day"]["preflight"]
    assert {c["check"] for c in checks} >= {"shell", "sed", "bash"} and status.start_record() == {}, "shown live, and nothing written"


def test_shell_advisories_are_warnings(monkeypatch):
    monkeypatch.setattr(start.mcp_check, "check", lambda: [{"check": "mcp synthetic", "ok": False, "value": "failed", "safe": "repair"}])
    answers = {"sed": (0, "GNU sed"), "bash": (0, "5"), "sh": (0, "/synthetic/python3")}
    rows = {row["check"]: row for row in start.preflight({"SHELL": "/bin/zsh"}, runner=lambda argv, seconds=10: answers[argv[0]])}
    for name in ("shell", "sed", "python3", "push guard"):
        assert rows[name]["ok"] is True and rows[name]["status"] == "warn"
        assert rows[name]["safe"]
    assert rows["mcp synthetic"]["ok"] is False


def test_shell_advisory_variants_and_missing_python(monkeypatch):
    monkeypatch.setattr(start.mcp_check, "check", lambda: [])

    def run(argv, seconds=10):
        return (0, "/synthetic/bin/sed") if argv[-1] == "command -v sed" else {"sed": (1, "illegal option"), "bash": (0, "5"), "sh": (1, "")}[argv[0]]

    rows = {row["check"]: row for row in start.preflight({"SHELL": "/bin/bash"}, runner=run)}
    assert rows["shell"]["status"] == "ok" and rows["shell"]["ok"]
    assert rows["sed"]["value"] == "BSD" and rows["sed"]["status"] == "warn"
    assert rows["python3"]["value"] == "missing" and rows["python3"]["ok"] is False and rows["python3"]["status"] == "fail"


def test_warning_and_legacy_rows_remain_visible_in_day_start(monkeypatch, capsys):
    rows = [
        {"check": "shell", "value": "zsh", "ok": True, "status": "warn", "safe": "use an array"},
        {"check": "legacy", "value": "old", "ok": False, "safe": "legacy guidance"},
        {"check": "mcp synthetic", "value": "failed", "ok": False, "safe": "repair"},
        {"check": "normal", "value": "ok", "ok": True, "safe": "none"},
    ]
    got = {"refresh": [], "preflight": rows, "signin": {"status": "none needed"}, "questions": [], "handoff": {}}
    monkeypatch.setattr(plan, "build", lambda: {"steps": []})
    monkeypatch.setattr(start, "run", lambda declared: got)
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "start"]) == 0
    out = capsys.readouterr().out
    assert "warn  shell zsh: use an array" in out
    assert "legacy guidance" in out and "mcp synthetic failed: repair" in out
    assert "3 shell trap(s)" in out and "normal ok" not in out
    assert cli.main(["day", "start", "--json"]) == 0
    assert json.loads(capsys.readouterr().out)["preflight"] == rows
    assert start.view([], {"preflight": rows})["preflight"] == rows


def test_start_event_counts_warnings_and_failures(monkeypatch):
    from careerbrew import events
    from careerbrew.day import pm_promises
    from careerbrew.workday import end, questions

    rows = [{"check": "warn", "ok": True, "status": "warn"}, {"check": "legacy", "ok": False}, {"check": "ok", "ok": True}]
    sent = []
    monkeypatch.setattr(start, "checkouts", lambda declared: [])
    monkeypatch.setattr(start, "preflight", lambda: rows)
    monkeypatch.setattr(start, "signin", lambda declared, probe: ({"status": "none needed"}, []))
    monkeypatch.setattr(status, "save_start", lambda record: None)
    monkeypatch.setattr(questions, "close", lambda *args: None)
    monkeypatch.setattr(pm_promises, "reconcile", lambda day: None)
    monkeypatch.setattr(end, "morning", lambda: {})
    monkeypatch.setattr(events, "emit", lambda name, **fields: sent.append((name, fields)))
    start.run([])
    assert sent[-1][0] == "workday.start" and sent[-1][1]["traps"] == 2


@pytest.mark.parametrize(("probe", "lookup"), [((127, ""), (1, "")), ((127, ""), (0, "/synthetic/sed")), ((1, "unexpected error"), (0, "/synthetic/sed"))])
def test_missing_or_unknown_sed_is_a_failure(monkeypatch, probe, lookup):
    monkeypatch.setattr(start.mcp_check, "check", lambda: [])

    def run(argv, seconds=10):
        if argv[0] == "sed":
            return probe
        if argv[-1] == "command -v sed":
            return lookup
        return (0, "5")

    row = next(r for r in start.preflight({"SHELL": "/bin/bash"}, runner=run) if r["check"] == "sed")
    assert row["value"] == "unknown" and row["ok"] is False and row["status"] == "fail"
