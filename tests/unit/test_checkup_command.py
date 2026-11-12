"""/careerbrew-checkup contract: its last step files every problem found (issue #104)."""

from pathlib import Path

from .conftest import readme_repo

ROOT = Path(__file__).resolve().parents[2]
COMMAND = ROOT / "commands" / "careerbrew-checkup.md"
# The repo address comes from the README, so this test holds no owner name.
REPO = readme_repo()


def _text() -> str:
    return COMMAND.read_text()


def test_command_fits_agentbrew_hard_budget_in_utf8_bytes_and_js_units():
    """Agentbrew lintCommandBloat uses UTF-8 bytes despite its chars label."""
    content = _text()
    assert len(content.encode("utf-8")) <= 8000
    assert len(content.encode("utf-16-le")) // 2 <= 8000


def test_command_keeps_both_hosts_shared_memory_and_privacy_contract():
    content = _text()
    for clause in (
        "<!-- careerbrew:dual-host-shared-memory -->",
        "careerbrew where",
        "docs/agent-hosts.md",
        "Both Claude Code and ChatGPT Work/Codex",
        "shared-memory bootstrap, search",
        "session-record rules",
        "Keep career records local under the plugin privacy policy",
    ):
        assert clause in content, clause


def _last_step() -> str:
    text = _text()
    assert "## Last step: file every problem you found" in text
    return text.split("## Last step: file every problem you found", 1)[1]


def test_filing_is_the_last_step():
    text = _text()
    assert text.find("## Last step: file every problem you found") > text.find("scripts/checkup.sh")
    assert "\n## " not in _last_step(), "nothing comes after the last step"
    assert "file each problem found" in text.split("\n---\n", 1)[0], "the description names the step"


def test_it_lists_every_problem_in_careerbrew_or_the_setup():
    step = _last_step()
    assert "List every problem you found in careerbrew or your setup" in step
    assert "`Problems found: none`" in step


def test_it_searches_for_a_duplicate_before_it_files():
    step = _last_step()
    search = step.find("Search for a duplicate first")
    assert -1 < search < step.find("gh issue create") and search < step.find("agentbrew report")
    assert f'gh issue list -R {REPO} --state all --search "<2-4 key words>"' in step and "; rc=$?" in step
    assert "TASKS.md" in step


def test_a_failed_search_is_never_no_duplicate():
    step = _last_step()
    assert 'A nonzero `rc` is a failed search, not "no duplicate"' in step
    assert "outside the sandbox" in step
    assert "not filed: could not search for duplicates" in step


def test_a_match_is_reported_and_a_closed_match_is_a_regression():
    step = _last_step()
    assert "An open issue or task matches: do not file a new one" in step
    assert "Came back after #<N>." in step


def test_drafts_never_carry_private_data_and_pass_the_scan():
    step = _last_step()
    assert "Never include anything from `private/`" in step
    scan = step.find("uv run careerbrew privacy scan --file <draft>.title --file <draft>.md")
    assert -1 < scan < step.find("gh issue create") and scan < step.find("agentbrew report")
    assert "Never post a draft that has a finding" in step


def test_the_owner_files_in_this_repo_and_anyone_else_asks_first():
    step = _last_step()
    assert "gh api user -q .login" in step and f"gh repo view {REPO} --json owner -q .owner.login" in step
    assert f'gh issue create -R {REPO} --title "<title>" --body-file <draft>.md' in step
    assert "`report-upstream` skill" in step and "post only after they say yes" in step
    assert f"--repo {REPO}` (no `--yes`)" in step


def test_other_tools_and_company_plugins_go_to_their_own_repos():
    step = _last_step()
    assert "goes to that tool's repo through the `report-upstream` skill" in step
    assert "the plugin's own repo, never here" in step


def test_it_reports_one_id_per_problem():
    step = _last_step()
    for outcome in ["`filed #<N>`", "`duplicate of #<N>`", "`task <ID>`", "`not filed: <reason>`"]:
        assert outcome in step, outcome


def test_the_owner_fallback_is_a_task_in_the_task_backend():
    step = _last_step()
    assert "add a task to `TASKS.md` instead" in step


def test_read_only_lookups_need_no_prompt():
    head = _text().split("\n---\n", 1)[0]
    assert "Bash(uv run careerbrew privacy scan:*)" in head and "Bash(gh issue list:*)" in head
    assert "gh issue create" not in head, "posting is never pre-approved"
    assert "gh api" not in head, "`gh api` can write (-X PATCH), so it is never pre-approved"


def test_plugin_docs_point_plugin_commands_at_the_same_step():
    assert "last step of `/careerbrew-checkup`" in (ROOT / "docs" / "plugins.md").read_text()


def test_open_work_branches_are_checked_against_merged_prs_first():
    """Issue #174: 50 branches of merged PRs once read as open work; the agent checks before it says "continue"."""
    text = _text()
    assert "gh pr list --state merged" in text and "is not open work" in text
    assert "Bash(gh pr list:*)" in text.split("\n---\n", 1)[0], "a read-only lookup needs no prompt"
