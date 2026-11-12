"""Project sessions keep their goal and register real watches (#430, #437)."""

from pathlib import Path

import yaml

ROOT = Path(__file__).resolve().parents[2]


def contracts():
    return [(ROOT / path).read_text().lower() for path in ("skills/careerbrew-work-day/SKILL.md", "commands/work.md")]


def test_owner_reply_has_ranked_actionable_next_steps():
    for text in contracts():
        assert "ranked next steps" in text
        assert "approve or pick" in text


def test_external_waits_are_watched_without_inventing_coverage():
    for text in contracts():
        assert "watch each external wait" in text
        assert "unwatched" in text
        assert "existing work-day loop" in text


def test_session_returns_to_goal_after_a_subtask():
    for text in contracts():
        assert "after each subtask" in text
        assert "goal status" in text
        assert "next goal step" in text


def test_behavior_evals_cover_the_three_scenarios():
    path = ROOT / "evals/cases/026-project-sessions.yaml"
    cases = {case["id"]: case for case in yaml.safe_load_all(path.read_text()) if case}
    for case_id in ("work-day-project-session-next-steps", "work-day-project-session-keeps-watching", "work-day-project-session-return-to-goal"):
        assert cases[case_id]["skill"] == "careerbrew-work-day"
        assert cases[case_id]["input"] and cases[case_id]["expect"]


def test_generated_plan_gives_sessions_the_same_contract(home):
    from careerbrew.workday import plan

    text = plan.build()["sessions"].lower()
    for phrase in ("after each subtask", "goal status", "next goal step", "ranked next steps", "approve or pick", "watch each external wait", "existing work-day loop", "unwatched", "skip quiet projects", "batched ask", "handoff"):
        assert phrase in text
