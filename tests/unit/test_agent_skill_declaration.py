"""Issue #328: every published Careerbrew skill has an explicit automatic host source."""

import importlib.util
import re
from pathlib import Path

import pytest
import yaml

from careerbrew.agent_workflows import repository_workflows

ROOT = Path(__file__).resolve().parents[2]


def test_local_skill_directory_is_declared_without_a_static_skill_list():
    data = yaml.safe_load((ROOT / "Agentfile.yaml").read_text())
    assert "./skills" in data.get("skillDirectories", [])
    assert "./skills" not in data.get("sources", [])


def test_published_skill_folders_and_frontmatter_have_an_unambiguous_namespace():
    inventory = repository_workflows(ROOT)
    assert inventory.problems == ()
    skills = [entry.path for entry in inventory.entries if entry.kind == "skill"]
    assert skills
    for path in skills:
        assert path.parent.name.startswith("careerbrew-"), path.parent.name
        front = yaml.safe_load(path.read_text().split("---", 2)[1])
        assert front["name"] == path.parent.name


def test_skill_loads_use_the_published_sibling_namespace():
    skills = [entry.path for entry in repository_workflows(ROOT).entries if entry.kind == "skill"]
    aliases = [path.parent.name.removeprefix("careerbrew-") for path in skills]
    for path in skills:
        body = path.read_text().split("---", 2)[-1]
        for alias in aliases:
            pattern = r"(?:load|run|use|follow|hand off to|with)(?: the)?\s+`" + re.escape(alias) + r"`"
            assert not re.search(pattern, body, re.I), (path.name, alias)


@pytest.mark.parametrize("variant,expected", [("canonical", 0), ("self", 0), ("missing-load", 1), ("missing-iterate", 1), ("late-section", 1)])
def test_evaluator_keeps_canonical_first_load_and_iterate_last(tmp_path, monkeypatch, capsys, variant, expected):
    from careerbrew import judge

    spec = importlib.util.spec_from_file_location("careerbrew_eval_structure", ROOT / "evals/run.py")
    assert spec and spec.loader
    runner = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(runner)
    name = "careerbrew-best-practices" if variant == "self" else "careerbrew-synthetic"
    folder = tmp_path / "skills" / name
    folder.mkdir(parents=True)
    first = "1. Do synthetic work.\n" if variant in {"self", "missing-load"} else "First, load `careerbrew-best-practices`.\n"
    host = "<!-- careerbrew:dual-host-shared-memory -->\n## Agent host contract\nUse careerbrew where and docs/agent-hosts.md.\n"
    tail = "" if variant == "missing-iterate" else "## Iterate\nRecord a reusable correction.\n"
    if variant == "late-section":
        tail += "## Unexpected later section\nDo not allow this after Iterate.\n"
    (folder / "SKILL.md").write_text(f"---\nname: {name}\ndescription: Use when testing. Don't use outside tests.\n---\n{first}\n{host}\n{tail}")
    (folder / "LESSONS.md").write_text("Synthetic lessons.\n")
    monkeypatch.setattr(runner, "ROOT", tmp_path)
    monkeypatch.setattr(judge.paths, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(runner, "load_cases", lambda: [{"id": kind, "skill": name, "should": kind, "input": "synthetic", "expect": ["A synthetic assertion"]} for kind in ("act", "refrain")])
    monkeypatch.setattr(judge, "read_labels", lambda: ([], []))
    monkeypatch.setenv("EVALS_LLM", "0")
    assert runner.run_all() == expected, capsys.readouterr().out


def test_every_published_host_contract_precedes_the_final_iterate_section():
    skills = [entry.path for entry in repository_workflows(ROOT).entries if entry.kind == "skill"]
    for path in skills:
        body = path.read_text().split("---", 2)[-1]
        assert "<!-- careerbrew:dual-host-shared-memory -->" in body, path
        assert "careerbrew where" in body and "docs/agent-hosts.md" in body, path
        assert body.rfind("## Agent host contract") < body.rfind("## Iterate"), path
        assert re.findall(r"(?m)^## .+$", body)[-1] == "## Iterate", path
