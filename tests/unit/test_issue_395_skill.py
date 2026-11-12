from pathlib import Path

from careerbrew.workday import plan


def test_work_day_skill_uses_executable_loop_stop_hint():
    skill = (Path(__file__).resolve().parents[2] / "skills/careerbrew-work-day/SKILL.md").read_text()
    assert skill.count("careerbrew day loop stop --id <id>") == 1
    assert "careerbrew day loop stop <id>" not in skill


def test_end_of_day_plan_uses_executable_loop_stop_hint():
    assert any("careerbrew day loop stop --id <id>" in step for step in plan.END_OF_DAY)
