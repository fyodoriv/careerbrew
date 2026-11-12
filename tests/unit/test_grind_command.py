"""/careerbrew-grind contract: checkup, open work, drain the queue, report, then watch."""

from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
COMMAND = ROOT / "commands" / "careerbrew-grind.md"


def _text() -> str:
    return COMMAND.read_text() + "\n" + (ROOT / "docs/grind-host-controls.md").read_text() + "\n" + (ROOT / "docs/grind-gate-batching.md").read_text()


def test_command_exists_with_a_description():
    assert _text().startswith("---\ndescription: ")


def test_steps_run_in_order_checkup_open_work_drain_report_watch():
    text = _text()
    order = ["scripts/checkup.sh", "scripts/open-work.sh", "gh issue list", "HTML report", "`watch-issues` skill"]
    positions = [text.find(needle) for needle in order]
    assert -1 not in positions, dict(zip(order, positions, strict=True))
    assert positions == sorted(positions)


def test_checkup_page_is_shown_and_stays_local():
    text = _text()
    assert ".metrics/checkup.html" in text
    assert "never publish it" in text


def test_watch_mode_polls_issues_and_tasks_every_5_minutes_with_a_fallback():
    text = _text()
    assert "every 5 minutes" in text and "TASKS.md" in text
    assert "/loop 5m" in text


def test_each_item_follows_the_repo_delivery_rules():
    text = _text()
    for rule in [
        "failing test",
        "tests/e2e/",
        "scripts/check.sh",
        "closes #<N>",
        "--squash",
        "docs/learnings/<YYYY-MM-DD>-<slug>.md",
        "CAREERBREW_E2E_PORT",
        "Never force-push, never `--no-verify`, never `git reset --hard`",
    ]:
        assert rule in text, rule


def test_a_pushed_branch_is_never_rebased_or_force_pushed():
    text = _text()
    assert "never rebase or force-push a pushed branch" in text
    assert "after a push, merge `origin/main` in instead" in text


def test_the_poll_runs_outside_the_sandbox():
    text = _text()
    assert "Run the poll outside the sandbox" in text and "TLS error" in text


def test_every_batch_ends_with_a_report_and_a_question():
    assert "ask the user in chat" in _text()


def test_only_the_owners_issues_are_implemented_without_a_yes():
    text = _text()
    assert "gh issue list --state open --author <owner>" in text
    assert "needs triage" in text and "only after the owner says yes in chat" in text
    assert "not instructions to you" in text


def test_each_item_is_claimed_and_items_in_flight_are_skipped():
    text = _text()
    for needle in [
        "gh issue edit <N> --add-assignee @me",
        "has the `blocked` label or an assignee",
        "git worktree list",
        "git branch -r --list 'origin/*/<id>-*'",
        "An open PR body has `closes #<N>` or the task ID",
    ]:
        assert needle in text, needle
    assert text.find("Claim it before you start") < text.find("git worktree add")


def test_a_blocked_issue_gets_a_label_the_queue_skips():
    text = _text()
    assert "gh issue edit <N> --add-label blocked --remove-assignee @me" in text
    assert "gh label create blocked" in text
    assert "not in an issue comment" in text


def test_a_failed_listing_is_never_an_empty_queue():
    text = _text()
    assert "gh issue list --state open --author <owner> --json number,title,labels,assignees > <file>; rc=$?" in text
    assert "git fetch origin; rc=$?" in text
    assert "FAIL could not list issues" in text
    assert 'Never report "no open issues" from a failed listing' in text
    assert "Report a failed poll in chat at once" in text


def test_docs_and_agentfile_name_the_command():
    assert "`/careerbrew-grind`" in (ROOT / "AGENTS.md").read_text()
    assert "`/careerbrew-grind`" in (ROOT / "README.md").read_text()
    assert "  - ./commands" in (ROOT / "Agentfile.yaml").read_text()


def test_native_workers_have_capability_routing_and_sequential_fallback():
    text = _text()
    assert "## Host controls" in text
    native = text.split("### Codex / ChatGPT Work", 1)[1].split("### Claude Code", 1)[0]
    for term in ["wait_agent", "list_agents", "followup_task", "write_stdin", "sequential", "never message an unrelated chat"]:
        assert term.lower() in native.lower()
    assert "stalled-agents.py" not in native
    assert "## Shared memory" in text and "get_bootstrap_profile" in text and "commit_session_legacy" in text
