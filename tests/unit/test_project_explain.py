"""Issue #53: a layered, cited project explanation and its deterministic checker."""

import re
from pathlib import Path

from careerbrew.work import explain, projects

ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / "skills" / "careerbrew-project-explainer"
EXAMPLE = SKILL / "references" / "example-careerbrew.md"


def test_real_project_example_passes_the_checker():
    assert explain.check(EXAMPLE.read_text()) == []


def _good() -> str:
    return EXAMPLE.read_text()


def test_missing_citation_wrong_order_and_trailing_subbullet_fail():
    bad = _good().replace("flags) [S5].", "flags).\n  - see [S5]")
    assert any("no citation" in p for p in explain.check(bad))
    swapped = _good().replace("## Gotchas", "## Tmp").replace("## Read next", "## Gotchas").replace("## Tmp", "## Read next")
    assert any("headings must be exactly" in p for p in explain.check(swapped))


def test_sources_rules_kind_locator_unused_and_leads_only():
    t = _good().replace("| S5 | primary | src/careerbrew/work/ | 2026-10-21 |", "| S5 | lead | src/careerbrew/work/ | 2026-10-21 |\n| S6 | maybe | ??? | |")
    ps = explain.check(t)
    assert any("backed only by leads" in p for p in ps)
    assert any("S6: Kind" in p for p in ps) and any("S6: Locator" in p for p in ps)
    assert any("S6: Read" in p for p in ps) and any("S6: unused" in p for p in ps)


def test_containers_why_and_one_minute_rules():
    t = _good().replace("| Deploys as |", "| Runs as |").replace("| decided | the owner | 2026-10-18 | [S1] |\n| A rule", "| proposed | the owner | 2026-10-18 | [S1] |\n| A rule")
    t = t.replace("| Local-first, models named in one file | proposed", "| considering local-first | decided")
    t = t.replace("- Status:", "  - Status:")
    ps = explain.check(t)
    assert any("Deploys as" in p for p in ps)
    assert any("considering" in p for p in ps)
    assert any("exactly 3 flat bullets" in p for p in ps)


def test_acronym_defined_inline_glossary_does_not_count():
    t = _good().replace("KEEP (met its target) state", "KEEP state")
    assert any("define KEEP" in p for p in explain.check(t))


def test_short_depth_headings():
    keep = set(explain.SHORT)
    parts = re.split(r"(?m)^(?=## )", _good())
    short = "".join(p for p in parts if not p.startswith("## ") or explain._base(p[3:].split("\n")[0].strip()) in keep)
    short = short.replace("| S2 | primary | docs/VISION.md@c334531 | c334531 |\n", "")
    ps = explain.check(short, short=True)
    assert not any("headings" in p for p in ps), ps


def test_scaffold_lists_the_project_sources_and_fails_until_filled(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "data"))
    projects.add("billing", "", "7d")
    projects.add_source("billing", "tracker", "PROJ-1")
    doc = explain.scaffold("billing")
    assert "| S1 | primary | PROJ-1 | TODO |" in doc
    assert explain.check(doc)


def test_skill_rules_frontmatter_length_and_one_level_references():
    for skill in sorted((ROOT / "skills").iterdir()):
        if not skill.is_dir() or skill.name.startswith("."):
            continue  # local agent folders such as skills/.claude are git-ignored, not skills
        md = skill / "SKILL.md"
        text = md.read_text()
        assert re.match(r"^---\nname: [\w-]+\ndescription: .+\n---\n", text), f"{skill.name}: frontmatter"
        assert len(text.splitlines()) < 500, f"{skill.name}: body over 500 lines"
        refs = skill / "references"
        if refs.exists():
            assert not any(p.is_dir() for p in refs.iterdir()), f"{skill.name}: references nest deeper than one level"
