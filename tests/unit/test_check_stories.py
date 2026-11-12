"""scripts/check-stories.py: a story with a web surface needs approved pictures (US-75).

Each story names its web views (`**Web views**: Today, Week`, `all`, or `none`). A story with web views needs
`web/visual/us-NN-*.spec.ts` and its pictures at desktop and phone width; a story that says `none` but talks about
the web app needs the reason (`none (...)`), so a forgotten web surface cannot pass as "none".
"""

from __future__ import annotations

import importlib.util
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "check-stories.py"


def _module():
    spec = importlib.util.spec_from_file_location("check_stories", SCRIPT)
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


STORY = """## US-{num}: {title}

- **Mode**: platform
- **Host contract**: dual-host-shared-memory
- **As** a user, **I want** a thing, **so that** it helps.
- **Acceptance criteria**:
  - Given one, when two, then three.
  - Given four, when five, then six{extra}.
- **Metric**: things. **Target**: 1.
- **Skill**: `skills/careerbrew-start-session`
{web}
| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-{num}-thing.sh` | `cli.run` |
"""


def repo(tmp_path: Path, stories: list[dict[str, str]], specs: dict[str, list[str]] | None = None) -> Path:
    """A minimal repo: the stories, their skill and e2e files, the view names, and visual specs with their pictures."""
    (tmp_path / "docs").mkdir(parents=True)
    (tmp_path / "docs" / "user-stories.md").write_text("# User stories\n\n" + "\n".join(STORY.format(**{"extra": "", "web": "", **s}) for s in stories))
    (tmp_path / "skills" / "careerbrew-start-session").mkdir(parents=True)
    (tmp_path / "skills" / "careerbrew-start-session" / "SKILL.md").write_text("<!-- careerbrew:dual-host-shared-memory -->\nUse careerbrew where, then docs/agent-hosts.md.\n")
    (tmp_path / "config").mkdir()
    (tmp_path / "config" / "goals.yaml").write_text("metrics: {}\n")
    (tmp_path / "tests" / "e2e").mkdir(parents=True)
    for s in stories:
        (tmp_path / "tests" / "e2e" / f"us-{s['num']}-thing.sh").write_text(f"# US-{s['num']}\n")
    (tmp_path / "web" / "src").mkdir(parents=True)
    (tmp_path / "web" / "src" / "router.ts").write_text('{ id: "today", label: "Today" },\n{ id: "week", label: "Week" },\n{ id: "jobs", label: "Job hunt" },\n')
    shots = tmp_path / "web" / "visual" / "__screenshots__"
    shots.mkdir(parents=True)
    for spec, pictures in (specs or {}).items():
        (tmp_path / "web" / "visual" / spec).write_text('test("x", async () => { await shot(page, "plan"); });\n')
        (shots / spec).mkdir()
        for p in pictures:
            (shots / spec / p).write_bytes(b"png")
    return tmp_path


def problems(root: Path) -> list[str]:
    return _module().check(root)


def test_a_story_with_web_views_and_its_pictures_passes(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: Today, Week\n"}], {"us-01-plan.spec.ts": ["plan-desktop-darwin.png", "plan-phone-darwin.png"]})
    assert problems(root) == []


def test_a_story_with_web_views_and_no_visual_test_fails(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: Today\n"}])
    assert problems(root) == ["US-01: web views Today but no visual test web/visual/us-01-*.spec.ts (fix: write one, then run `npm run visual:update` in web/)"]


@pytest.mark.parametrize("missing", ["plan-desktop-darwin.png", "plan-phone-darwin.png"])
def test_a_picture_missing_at_one_width_fails(tmp_path, missing):
    pictures = [p for p in ["plan-desktop-darwin.png", "plan-phone-darwin.png"] if p != missing]
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: Today\n"}], {"us-01-plan.spec.ts": pictures})
    assert problems(root) == [f"US-01: web/visual/us-01-plan.spec.ts has no approved picture {missing} (fix: run `npm run visual:update` in web/ and review it)"]


def test_every_story_says_whether_it_has_web_views(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan"}])
    assert problems(root) == ["US-01: no **Web views** (the views it shows, `all`, or `none`)"]


def test_an_unknown_view_name_fails(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: Todya\n"}], {"us-01-plan.spec.ts": ["plan-desktop-darwin.png", "plan-phone-darwin.png"]})
    assert problems(root) == ["US-01: unknown web view Todya (views: Job hunt, Today, Week, Sign-in page)"]


def test_none_for_a_story_that_talks_about_the_web_app_needs_a_reason(tmp_path):
    talks = {"num": "01", "title": "Plan", "extra": ", in the web app", "web": "- **Web views**: none\n"}
    assert problems(repo(tmp_path / "a", [talks])) == ["US-01: says **Web views**: none but names the web app or a view; list its views, or say why in `none (...)`"]
    assert problems(repo(tmp_path / "b", [{**talks, "web": "- **Web views**: none (the web app only links to the CLI page)\n"}])) == []
    assert problems(repo(tmp_path / "c", [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])) == []


def test_pictures_of_a_story_without_web_views_or_without_a_spec_fail(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}], {"us-01-plan.spec.ts": ["plan-desktop-darwin.png", "plan-phone-darwin.png"]})
    (root / "web" / "visual" / "__screenshots__" / "us-02-gone.spec.ts").mkdir()
    assert problems(root) == [
        "web/visual/us-01-plan.spec.ts: US-01 says **Web views**: none (fix: list its views, or delete the spec and its pictures)",
        "web/visual/__screenshots__/us-02-gone.spec.ts: pictures of a spec that does not exist (fix: delete the folder)",
    ]


def test_a_future_story_without_cross_host_memory_acceptance_is_rejected(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Future action", "web": "- **Web views**: none\n"}])
    target = root / "docs" / "user-stories.md"
    target.write_text(target.read_text().replace("- **Host contract**: dual-host-shared-memory\n", ""))
    assert any("Host contract" in p for p in problems(root))


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_future_agent_workflow_without_host_contract_is_rejected(tmp_path, kind):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    command = root / "skills/careerbrew-start-session/SKILL.md" if kind == "skill" else root / "commands/new.md"
    command.parent.mkdir(exist_ok=True)
    command.write_text("---\ndescription: New workflow\n---\nDo work.\n")
    assert any(str(command.relative_to(root)) in p and "host contract" in p for p in problems(root))


@pytest.mark.parametrize("name", ["x-reserved", "long" * 20, "MixedCase", "two--dashes"])
def test_future_own_command_names_require_the_native_safe_contract(tmp_path, name):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    command = root / "commands" / (name + ".md")
    command.parent.mkdir()
    command.write_text("<!-- careerbrew:dual-host-shared-memory -->\n")
    assert any("native-safe" in p for p in problems(root))


@pytest.mark.parametrize("kind", ["command", "skill"])
def test_marker_only_workflow_cannot_omit_the_runtime_host_route(tmp_path, kind):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    target = root / "skills" / "careerbrew-start-session" / "SKILL.md"
    if kind == "command":
        target = root / "commands" / "new.md"
        target.parent.mkdir()
    target.write_text("<!-- careerbrew:dual-host-shared-memory -->\n")
    assert any("runtime host route" in p for p in problems(root))


def test_future_skill_must_use_the_careerbrew_public_namespace(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    target = root / "skills" / "unprefixed" / "SKILL.md"
    target.parent.mkdir()
    target.write_text("<!-- careerbrew:dual-host-shared-memory -->\ncareerbrew where; docs/agent-hosts.md\n")
    assert any("published skill namespace" in p for p in problems(root))


def test_future_unmapped_host_primitive_is_rejected(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    target = root / "commands" / "future.md"
    target.parent.mkdir()
    target.write_text("<!-- careerbrew:dual-host-shared-memory -->\ncareerbrew where; docs/agent-hosts.md\nCall mcp__unmapped_service__do_work.\n")
    assert any("unmapped host primitive" in p for p in problems(root))


@pytest.mark.parametrize("verb", ["load the", "follow the", "use the", "hand off to"])
def test_future_sibling_skill_load_requires_its_published_name(tmp_path, verb):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    target = root / "commands" / "future.md"
    target.parent.mkdir()
    target.write_text(f"<!-- careerbrew:dual-host-shared-memory -->\ncareerbrew where; docs/agent-hosts.md\n{verb} `start-session` skill.\n")
    assert any("unqualified sibling skill" in p for p in problems(root))


def _declare(root, commands, skill_dirs=None):
    import json

    (root / "Agentfile.yaml").write_text(json.dumps({"commands": commands, "skillDirectories": skill_dirs or ["./skills"]}))


def _portable_body():
    return "<!-- careerbrew:dual-host-shared-memory -->\nUse careerbrew where, then docs/agent-hosts.md.\n"


@pytest.mark.parametrize("selection", ["directory", "file"])
def test_declared_custom_command_sources_require_contract(tmp_path, selection):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    command = root / "entrypoints" / "portable.md"
    command.parent.mkdir()
    command.write_text("A workflow without the host contract.\n")
    _declare(root, ["./entrypoints" if selection == "directory" else "./entrypoints/portable.md"])
    assert any("host contract" in p and "portable.md" in p for p in problems(root))


@pytest.mark.parametrize("missing", ["route", "namespace"])
def test_declared_custom_skill_sources_require_contract(tmp_path, missing):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    name = "unprefixed" if missing == "namespace" else "careerbrew-extra"
    skill = root / "agent-skills" / name / "SKILL.md"
    skill.parent.mkdir(parents=True)
    skill.write_text(_portable_body() if missing == "namespace" else "<!-- careerbrew:dual-host-shared-memory -->\nNo route.\n")
    _declare(root, [], ["./skills", "./agent-skills"])
    wanted = "published skill namespace" if missing == "namespace" else "runtime host route"
    assert any(wanted in p and "agent-skills" in p for p in problems(root))


def test_declared_custom_sources_keep_selected_file_and_exclude_sibling(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    folder = root / "entrypoints"
    folder.mkdir()
    (folder / "selected.md").write_text(_portable_body())
    (folder / "unselected.md").write_text("Not a published workflow.\n")
    _declare(root, ["./entrypoints/selected.md"])
    assert problems(root) == []


@pytest.mark.parametrize("both_roles", [False, True])
def test_declared_custom_skill_named_command_keeps_command_role(tmp_path, both_roles):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    skill = root / "custom-workflows" / "careerbrew-role" / "SKILL.md"
    skill.parent.mkdir(parents=True)
    skill.write_text("---\nname: careerbrew-role\ndescription: Synthetic role fixture\n---\n" + _portable_body())
    _declare(root, ["./custom-workflows/careerbrew-role/SKILL.md"], ["./skills", "./custom-workflows"] if both_roles else ["./skills"])
    assert any("name is not native-safe" in p and "SKILL.md" in p for p in problems(root))


def test_declared_source_boundary_refuses_external_input(tmp_path, monkeypatch):
    root = repo(tmp_path / "repo", [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    external = tmp_path / "outside.md"
    external.write_text("Synthetic content that must not be read.\n")
    _declare(root, [str(external)])
    read = Path.read_text
    read_bytes = Path.read_bytes
    seen = []

    def guarded(path, *args, **kwargs):
        if path.resolve() == external.resolve():
            seen.append(path)
            raise AssertionError("external body read")
        return read(path, *args, **kwargs)

    def guarded_bytes(path):
        if path.resolve() == external.resolve():
            seen.append(path)
            raise AssertionError("external body read")
        return read_bytes(path)

    monkeypatch.setattr(Path, "read_text", guarded)
    monkeypatch.setattr(Path, "read_bytes", guarded_bytes)
    assert any("outside" in p for p in problems(root))
    assert seen == []


def test_declared_source_boundary_rechecks_retargeted_symlink(tmp_path, monkeypatch):
    root = repo(tmp_path / "repo", [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    inside = root / "actual-workflows"
    inside.mkdir()
    (inside / "portable.md").write_text(_portable_body())
    pointer = root / "current-workflows"
    pointer.symlink_to(inside, target_is_directory=True)
    _declare(root, ["./current-workflows"])
    assert problems(root) == []
    outside = tmp_path / "outside"
    outside.mkdir()
    (outside / "portable.md").write_text("Synthetic external body.\n")
    pointer.unlink()
    pointer.symlink_to(outside, target_is_directory=True)
    read = Path.read_text
    read_bytes = Path.read_bytes
    seen = []

    def guarded(path, *args, **kwargs):
        if path.resolve().is_relative_to(outside):
            seen.append(path)
            raise AssertionError("external body read")
        return read(path, *args, **kwargs)

    def guarded_bytes(path):
        if path.resolve().is_relative_to(outside):
            seen.append(path)
            raise AssertionError("external body read")
        return read_bytes(path)

    monkeypatch.setattr(Path, "read_text", guarded)
    monkeypatch.setattr(Path, "read_bytes", guarded_bytes)
    assert any("outside" in p for p in problems(root))
    assert seen == []


def test_declared_source_boundary_rejects_outside_alias(tmp_path, monkeypatch):
    root = repo(tmp_path / "repo", [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    folder = root / "entrypoints"
    folder.mkdir()
    source = folder / "selected.md"
    source.write_text("Protected synthetic body without a contract.\n")
    alias = tmp_path / "outside-alias"
    alias.symlink_to(folder, target_is_directory=True)
    _declare(root, [str(alias / source.name)])
    text, binary = Path.read_text, Path.read_bytes
    seen = []

    def guard(read):
        def guarded(path, *args, **kwargs):
            if path.resolve() == source:
                seen.append(path)
                raise AssertionError("unsupported declaration body read")
            return read(path, *args, **kwargs)

        return guarded

    monkeypatch.setattr(Path, "read_text", guard(text))
    monkeypatch.setattr(Path, "read_bytes", guard(binary))
    assert any("outside" in p for p in problems(root))
    assert seen == []


@pytest.mark.parametrize("content", ["", "# comment only\n"])
def test_declared_custom_empty_agentfile_uses_legacy_roots(tmp_path, content):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    (root / "Agentfile.yaml").write_text(content)
    assert problems(root) == []


@pytest.mark.parametrize("content", ["[]\n", "false\n"])
def test_declared_custom_nonmapping_agentfile_is_rejected(tmp_path, content):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    (root / "Agentfile.yaml").write_text(content)
    assert any("Agentfile" in p for p in problems(root))


def test_declared_custom_internal_command_alias_retains_basename(tmp_path):
    root = repo(tmp_path, [{"num": "01", "title": "Plan", "web": "- **Web views**: none\n"}])
    folder = root / "entrypoints"
    folder.mkdir()
    (folder / "body.md").write_text(_portable_body())
    (folder / "selected-alias.md").symlink_to("body.md")
    _declare(root, ["./entrypoints/../entrypoints/selected-alias.md"])
    commands = [entry for entry in _module().repository_workflows(root).entries if entry.kind == "command"]
    assert len(commands) == 1 and commands[0].path.name == "selected-alias.md"
    assert problems(root) == []
