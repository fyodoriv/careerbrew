"""The work-day runner behind /work (issue #257, US-94): the plan, status rows, and the page."""

from __future__ import annotations

import json
import subprocess
import threading
from pathlib import Path

import pytest
import yaml

from careerbrew import cli, config, plugins, report, runs
from careerbrew.workday import plan, status, steps

from .conftest import CANARIES

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture(autouse=True)
def pinned(monkeypatch):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-28T09:00+00:00")


def _plugin(tmp_path: Path, work_steps) -> Path:
    d = tmp_path / "plugin-steps"
    d.mkdir()
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({"name": "steps-co", "version": "1.0.0", "work_steps": work_steps}))
    return d


def test_the_plan_is_careerbrew_then_settings_then_plugins_first_declaration_wins(tmp_path):
    cli.main(["init"])
    config.set_value("work_steps", "standup, /review-prs")
    plugins.add(str(_plugin(tmp_path, ["/review-prs", {"step": "team-sync"}])))
    p = plan.build()
    assert [(s["step"], s["from"]) for s in p["steps"]] == [
        ("/careerbrew", "careerbrew"),
        ("/standup", "settings"),
        ("/review-prs", "settings"),
        ("/team-sync", "plugin:steps-co"),
    ]
    assert p["steps"][0]["path"].endswith("commands/careerbrew.md"), "careerbrew's own command is found in the repo"
    assert p["dashboard"]["title"] == "Work day Wed Oct 28"
    assert [r["status"] for r in p["dashboard"]["rows"]] == ["waiting"] * 4
    assert p["machine"] == "personal" and "draft" in p["rule"]


def test_bad_step_declarations_are_refused(tmp_path):
    with pytest.raises(ValueError, match="work_steps"):
        config.set_value("work_steps", "/ok, not a step")
    with pytest.raises(plugins.PluginError, match="work_steps"):
        plugins.add(str(_plugin(tmp_path, "/not-a-list")))
    with pytest.raises(steps.StepError, match="command or skill name"):
        steps.normalize("/a:..:..:secrets")
    with pytest.raises(steps.StepError, match="without `step:`"):
        steps.validate([{"name": "/x"}], "plugin:x")


def test_resolve_finds_a_skill_before_a_command_and_reads_user_only(tmp_path):
    (tmp_path / "skills" / "standup").mkdir(parents=True)
    (tmp_path / "skills" / "standup" / "SKILL.md").write_text("---\nname: standup\ndisable-model-invocation: true\n---\n")
    (tmp_path / "commands").mkdir()
    (tmp_path / "commands" / "standup.md").write_text("# standup\n")
    found = steps.resolve("/standup", [tmp_path])
    assert found["path"].endswith("skills/standup/SKILL.md") and found["user_only"] is True
    assert steps.resolve("/missing", [tmp_path]) == {"path": None, "user_only": False, "description": ""}


def test_update_writes_one_row_and_refuses_bad_input():
    names = ["/careerbrew", "/standup"]
    status.update("standup", names, "done", summary="synthetic summary", link="https://example.com/pr/1")
    status.update("/standup", names, needs_you="pick one")
    rows = status.rows(names)
    assert rows[0]["status"] == "waiting"
    assert rows[1] | {"at": ""} == status.blank("/standup") | {"status": "done", "summary": "synthetic summary", "needs_you": "pick one", "link": "https://example.com/pr/1", "ended": "2026-10-28T09:00:00+00:00"}
    with pytest.raises(ValueError, match="not in today's plan"):
        status.update("/nope", names, "done")
    with pytest.raises(ValueError, match="unknown status"):
        status.update("/standup", names, "finished")
    with pytest.raises(ValueError, match="http"):
        status.update("/standup", names, link="file:///etc/hosts")
    with pytest.raises(ValueError, match="at most 200"):
        status.update("/standup", names, summary="x" * 201)


def test_parallel_updates_never_lose_a_row():
    names = [f"/s{i}" for i in range(8)]
    threads = [threading.Thread(target=status.update, args=(n, names, "done")) for n in names]
    for t in threads:
        t.start()
    for t in threads:
        t.join()
    assert [r["status"] for r in status.rows(names)] == ["done"] * 8


def test_rows_are_kept_per_day():
    status.update("/careerbrew", ["/careerbrew"], "done")
    from datetime import date

    assert status.rows(["/careerbrew"], date(2026, 10, 29))[0]["status"] == "waiting"


def test_the_page_puts_row_text_in_detail_only(tmp_path):
    cli.main(["init"])
    config.set_value("work_steps", "/standup, /review-prs, /deploy")
    names = [s["step"] for s in steps.declared()]
    status.update("/careerbrew", names, "done", summary=f"met {CANARIES[0]}")
    status.update("/standup", names, "running", needs_you=f"approve the {CANARIES[1]} draft")
    status.update("/deploy", names, "failed", link="https://example.com/ci/9", error_class="network")
    r = runs.work_day(plan.build())
    assert [i.id for i in r.done] == ["/careerbrew"]
    assert [i.id for i in r.blockers] == ["/deploy"]
    assert [i.id for i in r.next] == ["ask-/standup", "/review-prs"]
    page = report.write(r)
    assert page and CANARIES[0] in page.read_text() and CANARIES[1] in page.read_text()
    summary = json.dumps(report.summary(r))
    assert all(c not in summary for c in CANARIES), "row text never reaches the agent summary"
    assert "example.com/ci" not in summary
    assert r.counts["needs_you"] == 1 and r.status == "blocked"


def test_day_run_json_hides_row_text_from_an_agent(capsys, monkeypatch):
    cli.main(["init"])
    cli.main(["day", "update", "/careerbrew", "--status", "done", "--summary", f"saw {CANARIES[0]}"])
    capsys.readouterr()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["day", "run", "--json"]) == 0
    out = capsys.readouterr().out
    assert CANARIES[0] not in out
    row = json.loads(out)["dashboard"]["rows"][0]
    assert row == {"step": "/careerbrew", "status": "done", "at": "2026-10-28T09:00:00+00:00", "needs_you": False, "started": "", "attempt": 0, "overdue": False, "verify": "", "error_class": "", "next": ""} | {
        "ended": "2026-10-28T09:00:00+00:00",
        "seconds": None,
        "median": None,
        "slow": False,
    }
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    cli.main(["day", "run", "--json"])
    assert CANARIES[0] in capsys.readouterr().out, "the owner (or an allowed agent) sees the text"


def test_errors_name_the_story(capsys):
    cli.main(["init"])
    assert cli.main(["day", "update", "/nope", "--status", "done"]) == 2
    assert "breaks US-94" in capsys.readouterr().out
    assert cli.main(["day", "update", "/careerbrew"]) == 2
    assert "nothing to change" in capsys.readouterr().out


def test_work_ships_from_careerbrews_commands_folder():
    text = (ROOT / "commands" / "work.md").read_text()
    assert "careerbrew day run --json" in text and "skills/careerbrew-work-day/SKILL.md" in text
    assert "./commands" in (ROOT / "Agentfile.yaml").read_text()
    assert "pluginReview" not in text and "plugin review" not in text.lower()


def test_vision_and_readme_state_the_ownership():
    vision = (ROOT / "docs" / "VISION.md").read_text()
    assert "No hosted service; a local web app only." in vision
    assert "no web UI" not in vision
    assert "work-day runner" in vision and "career work" in vision
    readme = (ROOT / "README.md").read_text()
    assert "/work" in readme and "career work" in readme


def test_a_plugins_own_command_and_skill_are_found(tmp_path, monkeypatch):
    # A plugin's step runs from the plugin: its installed command, or a skill in its own folder (review of #283).
    monkeypatch.setenv("CAREERBREW_COMMANDS_DIR", str(tmp_path / "cmds"))
    d = _plugin(tmp_path, ["/team-sync", "/acme-review"])
    (d / "commands").mkdir()
    (d / "commands" / "team-sync.md").write_text("# team sync\n")
    (d / "skills" / "acme-review").mkdir(parents=True)
    (d / "skills" / "acme-review" / "SKILL.md").write_text("---\nname: acme-review\n---\n")
    manifest = yaml.safe_load((d / plugins.MANIFEST).read_text())
    (d / plugins.MANIFEST).write_text(yaml.safe_dump({**manifest, "provides": {"commands": "commands", "skills": "skills"}}))
    plugins.add(str(d))
    found = {s["step"]: s["path"] for s in plan.build()["steps"]}
    assert found["/team-sync"] and found["/team-sync"].endswith("team-sync.md"), found
    assert found["/acme-review"] and found["/acme-review"].endswith("acme-review/SKILL.md"), found


def test_day_render_names_the_story_on_a_bad_step_list(capsys, monkeypatch):
    monkeypatch.setattr(plan, "build", lambda *a, **k: (_ for _ in ()).throw(steps.StepError("bad step")))
    assert cli.main(["day", "render"]) != 0
    assert "breaks US-94" in capsys.readouterr().out


def test_a_malformed_status_file_reads_as_empty():
    from careerbrew import store

    store.write_json(status.path_for(status.today()), ["not", "an", "object"])
    assert status.stored() == {}


def test_default_resolution_includes_shared_and_native_skills(tmp_path, monkeypatch):
    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    for folder, name in [(".codex", "native-only"), (".agents", "shared-only")]:
        target = tmp_path / folder / "skills" / name / "SKILL.md"
        target.parent.mkdir(parents=True)
        target.write_text("---\ndescription: Native fixture.\n---\n")
        assert steps.resolve(name)["path"] == f"~/{folder}/skills/{name}/SKILL.md"


def test_persisted_legacy_step_ids_resolve_namespaced_own_skills(tmp_path, monkeypatch):
    monkeypatch.setattr(steps.paths, "REPO_ROOT", tmp_path)
    target = tmp_path / "skills" / "careerbrew-learn-project" / "SKILL.md"
    target.parent.mkdir(parents=True)
    target.write_text("---\nname: careerbrew-learn-project\ndescription: Fixture.\n---\n")
    legacy = steps.resolve("/learn-project", [tmp_path])
    explicit = steps.resolve("/careerbrew-learn-project", [tmp_path])
    assert legacy["path"] == explicit["path"] == str(target)


@pytest.mark.parametrize("host", [".claude", ".codex"])
def test_plugin_step_uses_declared_canonical_resource_not_host_shadow(tmp_path, monkeypatch, host):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    root = tmp_path / "canonical-plugin"
    canonical = root / "skills" / "private-review" / "SKILL.md"
    canonical.parent.mkdir(parents=True)
    canonical.write_text("---\ndescription: Synthetic canonical resource\n---\n")
    shadow = tmp_path / host / "skills/private-review/SKILL.md"
    shadow.parent.mkdir(parents=True)
    shadow.write_text("Synthetic unrelated owner\n")
    monkeypatch.setattr(plugins, "installed", lambda: [{"origin": "plugin:fixture", "root": str(root), "manifest": {"provides": {"skills": "skills"}}}])
    found = steps.resolve("/private-review", origin="plugin:fixture")
    assert Path(found["path"]).expanduser() == canonical
    canonical.unlink()
    absent = steps.resolve("/private-review", origin="plugin:fixture")
    assert absent["path"] is None
    assert "plugin source unavailable" in absent["source_error"]


@pytest.mark.parametrize("host", [".claude", ".codex"])
@pytest.mark.parametrize("selection", ["commands/team-check.md", "commands/team-*.md"])
def test_plugin_command_file_selection_resolves_without_exporting_siblings(tmp_path, monkeypatch, host, selection):
    monkeypatch.setenv("HOME", str(tmp_path))
    monkeypatch.setattr(Path, "home", lambda: tmp_path)
    root = tmp_path / "plugin"
    (root / "commands").mkdir(parents=True)
    selected = root / "commands/team-check.md"
    selected.write_text("Synthetic selected command\n")
    (root / "commands/other-owner.md").write_text("Synthetic unselected command\n")
    (tmp_path / host).mkdir()
    monkeypatch.setattr(plugins, "installed", lambda: [{"origin": "plugin:fixture", "root": str(root), "manifest": {"provides": {"commands": selection}, "work_steps": ["/team-check", "/other-owner"]}}])
    planned = {s["step"]: s for s in steps.declared()}
    assert Path(planned["/team-check"]["path"]).expanduser() == selected
    assert planned["/other-owner"]["path"] is None
    assert "source_error" in planned["/other-owner"]


def _runtime_source_repo(tmp_path, monkeypatch, declarations):
    from careerbrew import agent_workflows

    repo = tmp_path / "own-repo"
    repo.mkdir()
    (repo / "Agentfile.yaml").write_text(yaml.safe_dump(declarations))
    monkeypatch.setattr(steps.paths, "REPO_ROOT", repo)
    monkeypatch.setenv("HOME", str(tmp_path / "client-home"))
    monkeypatch.setattr(plugins, "installed", lambda: [])

    def forbidden(*args, **kwargs):
        pytest.fail("runtime source lookup must not invoke external CLI or parse Agentbrew state")

    monkeypatch.setattr(subprocess, "run", forbidden)
    monkeypatch.setattr(agent_workflows, "primary_agent_roots", forbidden)
    return repo


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_own_declared_resources_without_mirrors(tmp_path, monkeypatch, kind):
    from careerbrew import agent_workflows

    declaration = {"commands": ["./entrypoints/review.md"] if kind == "command" else [], "skillDirectories": ["./agent-skills"] if kind == "skill" else []}
    repo = _runtime_source_repo(tmp_path, monkeypatch, declaration)
    target = repo / ("entrypoints/review.md" if kind == "command" else "agent-skills/careerbrew-extra/SKILL.md")
    target.parent.mkdir(parents=True)
    target.write_text("---\ndescription: Canonical declared fixture.\n---\n")
    name = "/review" if kind == "command" else "/careerbrew-extra"
    assert steps.resolve(name)["path"] == str(target)
    outside = tmp_path / "unrelated-owner.md"
    outside.write_text("Synthetic unrelated owner content\n")
    inventory = agent_workflows.repository_workflows

    def retarget(checkout):
        found = inventory(checkout)
        target.unlink()
        target.symlink_to(outside)
        return found

    monkeypatch.setattr(steps, "repository_workflows", retarget, raising=False)
    read = Path.read_bytes

    def no_outside(path):
        assert path.resolve() != outside, "retargeted declared source body was read"
        return read(path)

    monkeypatch.setattr(Path, "read_bytes", no_outside)
    assert steps.resolve(name)["path"] is None


def test_own_selected_sources_do_not_widen(tmp_path, monkeypatch):
    repo = _runtime_source_repo(tmp_path, monkeypatch, {"commands": ["./entrypoints/review.md"], "skillDirectories": []})
    folder = repo / "entrypoints"
    folder.mkdir()
    (folder / "review.md").write_text("Synthetic selected source\n")
    sibling = folder / "other.md"
    sibling.write_text("Synthetic unselected sibling\n")
    read = Path.read_text

    def no_sibling(path, *args, **kwargs):
        assert path != sibling, "unselected sibling body was read"
        return read(path, *args, **kwargs)

    monkeypatch.setattr(Path, "read_text", no_sibling)
    assert steps.resolve("/other")["path"] is None


def test_own_declared_source_escape_is_not_read(tmp_path, monkeypatch):
    outside = tmp_path / "outside.md"
    outside.write_text("Synthetic outside content\n")
    _runtime_source_repo(tmp_path, monkeypatch, {"commands": [str(outside)], "skillDirectories": []})
    text, binary = Path.read_text, Path.read_bytes
    reads = []

    def guard(read):
        def checked(path, *args, **kwargs):
            if path.resolve() == outside:
                reads.append(path)
                pytest.fail("outside declared body must not be read")
            return read(path, *args, **kwargs)

        return checked

    monkeypatch.setattr(Path, "read_text", guard(text))
    monkeypatch.setattr(Path, "read_bytes", guard(binary))
    assert steps.resolve("/outside")["path"] is None
    assert reads == []


def test_codex_prompt_only_step(tmp_path, monkeypatch):
    _runtime_source_repo(tmp_path, monkeypatch, {"commands": [], "skillDirectories": []})
    target = Path.home() / ".codex/prompts/review.md"
    target.parent.mkdir(parents=True)
    target.write_text("---\ndescription: Legacy prompt fixture.\n---\n")
    found = steps.resolve("/review")["path"]
    assert found is not None, "Codex prompt-only resource is missing"
    assert Path(found).expanduser() == target


def test_plugin_missing_source_does_not_use_host_shadow(tmp_path, monkeypatch):
    _runtime_source_repo(tmp_path, monkeypatch, {"commands": [], "skillDirectories": []})
    shadow = Path.home() / ".claude/commands/review.md"
    shadow.parent.mkdir(parents=True)
    shadow.write_text("Synthetic unrelated host mirror\n")
    read = Path.read_text

    def no_shadow(path, *args, **kwargs):
        assert path != shadow, "missing plugin source read another owner's host mirror"
        return read(path, *args, **kwargs)

    monkeypatch.setattr(Path, "read_text", no_shadow)
    monkeypatch.setattr(plugins, "installed", lambda: [{"origin": "plugin:fixture", "root": str(tmp_path / "missing-plugin"), "manifest": {"provides": {"commands": "commands/review.md"}}}])
    found = steps.resolve("/review", origin="plugin:fixture")
    assert found["path"] is None and "source_error" in found


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_default_nested_legacy_resource_preserves_directory_scope(tmp_path, monkeypatch, kind):
    from careerbrew.agent_workflows import repository_workflows

    declarations = {"commands": ["./commands"], "skillDirectories": ["./skills"]}
    repo = _runtime_source_repo(tmp_path, monkeypatch, declarations)
    (repo / "commands").mkdir()
    (repo / "commands/regular.md").write_text("Synthetic direct published command\n")
    published = repo / "skills/careerbrew-supported/SKILL.md"
    published.parent.mkdir(parents=True)
    published.write_text("Synthetic direct published skill\n")
    target = repo / ("commands/team/review.md" if kind == "command" else "skills/team/review/SKILL.md")
    target.parent.mkdir(parents=True)
    target.write_text("---\ndescription: Nested legacy resource fixture.\n---\n")
    assert target not in {entry.path for entry in repository_workflows(repo).entries}, "resource compatibility must not publish nested entries"
    assert steps.resolve("/team:review")["path"] == str(target)
    if kind == "command":
        declarations["commands"] = ["./commands/regular.md"]
    else:
        other = repo / "other-skills/careerbrew-other/SKILL.md"
        other.parent.mkdir(parents=True)
        other.write_text("Synthetic separately declared skill\n")
        declarations["skillDirectories"] = ["./other-skills"]
    (repo / "Agentfile.yaml").write_text(yaml.safe_dump(declarations))
    assert steps.resolve("/team:review")["path"] is None, "legacy resource fallback widened the declared scope"
    if kind == "command":
        sibling = target.with_name("other.md")
        sibling.write_text("Synthetic unselected nested sibling\n")
        declarations["commands"] = ["./commands/team/review.md"]
        (repo / "Agentfile.yaml").write_text(yaml.safe_dump(declarations))
        assert steps.resolve("/team:review")["path"] == str(target)
        assert steps.resolve("/team:other")["path"] is None


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_raw_nested_resource_names_stay_within_selected_scope(tmp_path, monkeypatch, kind):
    repo = _runtime_source_repo(tmp_path, monkeypatch, {"commands": ["./commands"], "skillDirectories": ["./skills"]})
    (repo / "commands").mkdir()
    (repo / "commands/regular.md").write_text("Synthetic published command\n")
    published = repo / "skills/careerbrew-supported/SKILL.md"
    published.parent.mkdir(parents=True)
    published.write_text("Synthetic published skill\n")
    for namespace in ("team", "team.v1"):
        safe = repo / (f"commands/{namespace}/review.md" if kind == "command" else f"skills/{namespace}/review/SKILL.md")
        safe.parent.mkdir(parents=True)
        safe.write_text("Synthetic safe resource\n")
        assert steps.resolve(f"/{namespace}:review")["path"] == str(safe)
    assert steps.resolve("/team::review")["path"] is not None, "internal empty segments remain compatible"
    escaped = repo / ("unselected.md" if kind == "command" else "unselected/SKILL.md")
    absolute = tmp_path / ("absolute-unselected.md" if kind == "command" else "absolute-unselected/SKILL.md")
    for target in (escaped, absolute):
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text("Synthetic unselected body\n")
    text, binary = Path.read_text, Path.read_bytes
    reads = []

    def guard(read):
        def checked(path, *args, **kwargs):
            if path.resolve() in {escaped, absolute}:
                reads.append(path)
            return read(path, *args, **kwargs)

        return checked

    monkeypatch.setattr(Path, "read_text", guard(text))
    monkeypatch.setattr(Path, "read_bytes", guard(binary))
    found = steps.resolve("/team:..:..:unselected")
    assert reads == [], "raw dot segments read an unselected body within the checkout"
    assert found["path"] is None
    raw_absolute = ":" + str(absolute.with_suffix("") if kind == "command" else absolute.parent).lstrip("/").replace("/", ":")
    assert steps.resolve(raw_absolute, [repo])["path"] is None
    assert steps.resolve("", [repo])["path"] is None
    assert reads == []


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_nested_resource_symlinks_cannot_leave_selected_directory(tmp_path, monkeypatch, kind):
    repo = _runtime_source_repo(tmp_path, monkeypatch, {"commands": ["./commands"], "skillDirectories": ["./skills"]})
    (repo / "commands").mkdir()
    (repo / "commands/regular.md").write_text("Synthetic published command\n")
    published = repo / "skills/careerbrew-supported/SKILL.md"
    published.parent.mkdir(parents=True)
    published.write_text("Synthetic published skill\n")
    selected_root = repo / ("commands" if kind == "command" else "skills")
    unselected_root = repo / "unselected-tree"
    invalid = unselected_root / ("review.md" if kind == "command" else "review/SKILL.md")
    invalid.parent.mkdir(parents=True)
    invalid.write_text("Synthetic unselected nested content\n")
    explicit = unselected_root / ("explicit.md" if kind == "command" else "explicit/SKILL.md")
    explicit.parent.mkdir(parents=True, exist_ok=True)
    explicit.write_text("Synthetic explicitly selected alias content\n")
    alias = selected_root / ("alias.md" if kind == "command" else "careerbrew-alias/SKILL.md")
    alias.parent.mkdir(parents=True, exist_ok=True)
    alias.symlink_to(explicit)
    team = selected_root / "team"
    team.symlink_to(unselected_root, target_is_directory=True)
    read = Path.read_bytes
    invalid_reads = []

    def observed(path):
        if path.resolve() == invalid:
            invalid_reads.append(path)
        return read(path)

    monkeypatch.setattr(Path, "read_bytes", observed)
    found = steps.resolve("/team:review")
    assert invalid_reads == [], "implicit resource symlink read outside the selected directory"
    assert found["path"] is None
    team.unlink()
    safe = team / ("review.md" if kind == "command" else "review/SKILL.md")
    safe.parent.mkdir(parents=True)
    safe.write_text("Synthetic resource inside selected directory\n")
    assert steps.resolve("/team:review")["path"] == str(safe)
    body_read = steps.read_workflow_bytes

    def retarget(path, checkout, **kwargs):
        if path == safe:
            team.rename(selected_root / "preserved-team")
            team.symlink_to(unselected_root, target_is_directory=True)
        return body_read(path, checkout, **kwargs)

    monkeypatch.setattr(steps, "read_workflow_bytes", retarget)
    assert steps.resolve("/team:review")["path"] is None
    assert invalid_reads == [], "resource pointer retarget escaped at the body read"
    if kind == "command":
        (repo / "Agentfile.yaml").write_text(yaml.safe_dump({"commands": ["./commands/alias.md"], "skillDirectories": ["./skills"]}))
    assert steps.resolve("/alias" if kind == "command" else "/careerbrew-alias")["path"] == str(alias)
