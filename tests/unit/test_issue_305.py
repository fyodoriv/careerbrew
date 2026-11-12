"""Work preflight diagnoses stale commands and missing plan resources (#305)."""

from careerbrew.workday import start


def test_preflight_exposes_workflow_checks():
    rows = start.preflight()
    assert {"installed-command", "command-paths", "plan-steps", "step-files"} <= {r["check"] for r in rows}


def test_stale_command_missing_path_legacy_steps_and_missing_files(tmp_path):
    from careerbrew.workday import workflow_check

    repo = tmp_path / "repo"
    (repo / "commands").mkdir(parents=True)
    (repo / "commands/work.md").write_text("Follow ~/missing-work-checkout/file.md\n```sh\n~/ignore-code-block\n```\n")
    target = tmp_path / "agent/commands/work.md"
    target.parent.mkdir(parents=True)
    target.write_text("yesterday")
    (repo / "Agentfile.yaml").write_text("workSteps: [/lost]\n")
    rows = workflow_check.check([{"step": "/careerbrew", "path": None}], repo=repo, agentfiles=[repo / "Agentfile.yaml"], report={"version": 1, "commands": []}, required_agents={"codex"})
    by = {r["check"]: r for r in rows}
    assert set(by) == {"installed-command", "command-paths", "plan-steps", "step-files"}
    assert all(not r["ok"] for r in rows)
    assert "preserve" in by["installed-command"]["safe"]
    assert "work_steps" in by["plan-steps"]["safe"]
    assert "ignore-code-block" not in str(rows)


def test_current_command_and_complete_plan_pass(tmp_path):
    from careerbrew.workday import workflow_check

    (tmp_path / "commands").mkdir()
    source = tmp_path / "commands/work.md"
    source.write_text("Current instructions\n")
    target = tmp_path / "installed.md"
    target.write_text("A transformed native wrapper that refers to its owned source.\n")
    agentfile = tmp_path / "Agentfile.yaml"
    agentfile.write_text("workSteps: [/careerbrew]\n")
    rows = workflow_check.check(
        [{"step": "/careerbrew", "path": str(source)}],
        repo=tmp_path,
        agentfiles=[agentfile],
        report={"version": 1, "commands": [{"sourcePath": str(source.resolve()), "targets": [{"agent": "codex", "path": str(target), "ready": True, "status": "ready"}]}]},
        required_agents={"codex"},
    )
    assert all(r["ok"] for r in rows)


def test_command_readiness_requires_the_selected_source_and_all_hosts(tmp_path):
    from careerbrew.workday import workflow_check

    (tmp_path / "commands").mkdir()
    source = tmp_path / "commands/work.md"
    source.write_text("Current instructions\n")
    target = tmp_path / "custom-host/wrapper.md"
    report = {"version": 1, "commands": [{"sourcePath": str(source), "targets": [{"agent": "codex", "path": str(target), "ready": True, "status": "ready"}]}]}
    rows = workflow_check.check([], repo=tmp_path, report=report, required_agents={"codex", "claude-code"})
    assert not rows[0]["ok"]
    report["commands"][0]["sourcePath"] = str(tmp_path / "another-source.md")
    assert not workflow_check.check([], repo=tmp_path, report=report, required_agents={"codex"})[0]["ok"]
    assert source.read_text() == "Current instructions\n"


def test_work_command_uses_its_declared_source_in_a_custom_folder(tmp_path):
    from careerbrew.workday import workflow_check

    folder = tmp_path / "entrypoints"
    folder.mkdir()
    source = folder / "work.md"
    source.write_text("Current custom instructions\n")
    (tmp_path / "Agentfile.yaml").write_text("commands: [./entrypoints/work.md]\n")
    report = {"version": 1, "commands": [{"sourcePath": str(source), "targets": [{"agent": "codex", "path": str(tmp_path / "custom-wrapper.md"), "ready": True, "status": "ready"}]}]}
    assert workflow_check.check([], repo=tmp_path, report=report, required_agents={"codex"})[0]["ok"]


def test_stale_installed_body_has_its_own_missing_path_check(tmp_path):
    from careerbrew.workday import workflow_check

    (tmp_path / "commands").mkdir()
    source = tmp_path / "commands/work.md"
    source.write_text("Current instructions without a broken path\n")
    # The upstream doctor supplies this custom target directory. No native names
    # are guessed, and its old resource still needs a path check while stale.
    target = tmp_path / "custom-target"
    (target / "resources").mkdir(parents=True)
    (target / "resources/old.md").write_text("Read ~/removed-checkout/old.md\n```sh\n~/ignored-in-code\n```\n")
    report = {"version": 1, "commands": [{"sourcePath": str(source), "targets": [{"agent": "codex", "path": str(target), "ready": False, "status": "kept"}]}]}
    before = (target / "resources/old.md").read_bytes()
    rows = {r["check"]: r for r in workflow_check.check([], repo=tmp_path, report=report, required_agents={"codex"})}
    assert not rows["installed-command"]["ok"]
    assert not rows["command-paths"]["ok"]
    assert rows["command-paths"]["value"] == "1 missing path(s)"
    assert (target / "resources/old.md").read_bytes() == before
    assert "removed-checkout" not in str(rows)
