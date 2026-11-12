"""Issue #223: the project explainer is a core skill, with a deep mode and PRD rules."""

import re
import subprocess
from pathlib import Path

import pytest

from careerbrew.work import explain, prd, projects

ROOT = Path(__file__).resolve().parents[2]
SKILL = ROOT / "skills" / "careerbrew-project-explainer"
EXAMPLE = SKILL / "references" / "example-careerbrew.md"
PRD = SKILL / "references" / "example-prd.md"


def _prd() -> str:
    return PRD.read_text()


def _deep() -> str:
    run = "## How it runs\n\n- Start everything with `uv run careerbrew`: it starts the watcher and the web app [S1].\n\n"
    return EXAMPLE.read_text().replace("## Invariants and boundaries", run + "## Invariants and boundaries")


# ---------- the skill is in core ----------


def test_the_explainer_is_its_own_core_skill_and_learn_project_hands_off_to_it():
    text = (SKILL / "SKILL.md").read_text()
    assert re.match(r"^---\nname: careerbrew-project-explainer\ndescription: .*Use when.*Don't use.*\n---\n", text)
    for mode in ("## Base", "## Deep mode", "## PRD rules"):
        assert mode in text
    assert "project-explainer" in (ROOT / "skills" / "careerbrew-learn-project" / "SKILL.md").read_text()


def test_the_core_skill_names_no_company_host():
    """Company data (project names, source hosts) stays in plugin config."""
    allowed = {"example.com", "github.com", "lucid.co", "matklad.github.io", "www.diataxis.fr", "learn.chatgpt.com", "www.computingatschool.org.uk", "platform.claude.com", "www.construx.com"}
    for f in SKILL.rglob("*.md"):
        for host in re.findall(r"https?://([\w.-]+)", f.read_text()):
            assert host in allowed or host.endswith(".example.com"), f"{f.name}: {host}"


# ---------- deep mode ----------


def test_the_example_passes_at_every_depth():
    assert explain.check(EXAMPLE.read_text()) == []
    assert explain.check(_deep(), deep=True) == []


def test_deep_mode_needs_a_how_it_runs_section():
    ps = explain.check(EXAMPLE.read_text(), deep=True)
    assert any("How it runs" in p and "headings" in p for p in ps)


def test_deep_mode_names_the_command_each_step_ran():
    ps = explain.check(_deep().replace("with `uv run careerbrew`", "with the start command"), deep=True)
    assert any("How it runs: name the command" in p for p in ps)


# ---------- PRD rules ----------


def test_the_example_prd_passes():
    assert prd.check(_prd()) == []


def test_prd_needs_requirements_and_decisions_sections():
    ps = prd.check(_prd().replace("## Requirements", "## Asks").replace("## Decisions", "## Choices"))
    assert any("headings must be" in p and "Requirements" in p and "Decisions" in p for p in ps)


def test_prd_decisions_are_decided_or_proposed_with_who_and_when():
    ps = prd.check(_prd().replace("| decided | the owner | 2026-10-27 | [S1] |\n| Estimates", "| maybe | the owner | 2026-10-27 | [S1] |\n| Estimates"))
    assert any("Decisions: State must be decided or proposed" in p for p in ps)
    ps = prd.check(_prd().replace("| Decision | State | Who | When | Source |", "| Decision | State | Owner | When | Source |"))
    assert any("Decisions: the table needs State, Who, and When columns" in p for p in ps)


@pytest.mark.parametrize("unit", ["3 days", "2 sprints", "5 points", "40 hours", "2 weeks"])
def test_prd_estimates_are_in_dev_weeks(unit):
    ps = prd.check(_prd().replace("| 0.5 dev weeks |", f"| {unit} |"))
    assert any("R1: estimate in dev weeks" in p for p in ps), ps


def test_prd_needs_a_plan_slot_in_dev_weeks():
    assert any("Plan slot" in p for p in prd.check(_prd().replace("Plan slot: 1 dev week [S1]\n", "")))
    assert any("Plan slot" in p for p in prd.check(_prd().replace("Plan slot: 1 dev week", "Plan slot: 1 sprint")))


def test_prd_pushes_back_when_the_slot_is_smaller_than_the_estimate():
    no_pushback = re.sub(r"## Pushback\n\n.*?\n\n(?=## )", "", _prd(), flags=re.S)
    ps = prd.check(no_pushback)
    assert any("Pushback: the requirements need 2.5 dev weeks but the plan slot is 1 dev week" in p for p in ps), ps
    # A slot that fits the estimate needs no pushback.
    assert prd.check(no_pushback.replace("Plan slot: 1 dev week", "Plan slot: 3 dev weeks")) == []
    # A pushback that never names the gap in dev weeks does not count.
    vague = re.sub(r"(?m)^- The requirements add up.*$", "- The plan is too big: cut something [S1].", _prd())
    assert any("Pushback: name the gap" in p for p in prd.check(vague))


def test_prd_open_questions_name_who_can_answer_and_every_claim_cites():
    assert any("Open questions: each question names who can answer" in p for p in prd.check(_prd().replace("Who: the owner", "ask around")))
    assert any("no citation" in p for p in prd.check(_prd().replace("| Cut R2 when the slot stays 1 dev week | proposed | the owner | 2026-10-27 | [S1] |", "| Cut R2 when the slot stays 1 dev week | proposed | the owner | 2026-10-27 | |")))
    assert any("S2: unused" in p for p in prd.check(_prd().replace(" [S2]", " [S1]")))


def test_prd_scaffold_lists_the_project_sources_and_the_slot_then_fails_until_filled(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "data"))
    projects.add("billing", "", "7d")
    projects.add_source("billing", "tracker", "PROJ-1")
    doc = prd.scaffold("billing", slot=4)
    assert "Plan slot: 4 dev weeks" in doc and "| S1 | primary | PROJ-1 | TODO |" in doc
    assert prd.check(doc)


def test_the_cli_writes_and_checks_a_prd(tmp_path):
    env = {"CAREERBREW_HOME": str(tmp_path / "home"), "PATH": "/usr/bin:/bin"}
    cb = [str(ROOT / ".venv" / "bin" / "careerbrew")]
    subprocess.run([*cb, "init"], env=env, check=True, capture_output=True)
    subprocess.run([*cb, "work", "project", "add", "billing"], env=env, check=True, capture_output=True)
    out = subprocess.run([*cb, "work", "project", "prd", "billing", "--slot", "2"], env=env, capture_output=True, text=True)
    assert out.returncode == 0 and "prd-billing.md" in out.stdout
    bad = subprocess.run([*cb, "work", "project", "prd", "billing", "--check", str(tmp_path / "home" / "out" / "prd-billing.md")], env=env, capture_output=True, text=True)
    assert bad.returncode == 1 and "prd check:" in bad.stdout
    good = subprocess.run([*cb, "work", "project", "prd", "billing", "--check", str(PRD)], env=env, capture_output=True, text=True)
    assert good.returncode == 0 and "prd check: pass" in good.stdout
