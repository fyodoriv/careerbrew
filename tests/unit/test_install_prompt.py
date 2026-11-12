"""Claude-first start, as in agentbrew and dotfiles: the README opens with one prompt that installs careerbrew through INSTALL.md and starts it."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
README = (ROOT / "README.md").read_text()
INSTALL = (ROOT / "INSTALL.md").read_text()


def _blocks(text: str) -> list[tuple[str, str]]:
    return re.findall(r"^```(\w*)\n(.*?)^```", text, re.M | re.S)


def test_the_readme_opens_with_one_agent_prompt_that_installs_or_updates_through_install_md():
    lang, prompt = _blocks(README)[0]
    assert lang == "text"
    assert "blob/HEAD/INSTALL.md" in prompt and "updates an existing clone" in prompt
    for step in (
        "only use careerbrew or also contribute",
        "scripts/doctor.sh",
        "fix every failing line",
        "propose adopt or merge",
        "extend careerbrew instead of building local copies",
        "start careerbrew for the first time (`/careerbrew`)",
        "offer an issue or PR",
    ):
        assert step in prompt, step
    assert README.index("```text") < README.index("## Web app")


def test_the_readme_follows_one_order_what_start_use_cases_features_then_the_rest():
    intro = README.split("```", 1)[0].split("\n", 1)[1].strip().split("\n\n")[0]
    assert 1 <= intro.count(". ") + 1 <= 3, intro
    marks = [README.index("```text"), README.index("```bash"), README.index("\n## Use cases\n"), README.index("\n## Features\n")]
    assert marks == sorted(marks)
    assert marks[-1] < min(README.index("\n## " + h) for h in ("Web app", "Quickstart"))


def test_the_one_line_setup_is_safe_to_rerun_and_ends_with_the_doctor():
    line = next(body for lang, body in _blocks(README) if lang == "bash").strip()
    assert "\n" not in line
    assert line.startswith("[ -d ~/apps/careerbrew ] || git clone ")
    assert "git pull --ff-only" in line and "scripts/install.sh" in line
    assert line.endswith("scripts/doctor.sh")


def test_install_md_carries_out_each_part_of_the_prompt():
    for part in (
        "pull --ff-only",
        "Use and contribute",
        "### 4b. Contributor setup",
        "## 5. Check and fix",
        "0 failed",
        "agentbrew sync",
        "## 6. Audit what the machine already has",
        "## 7. Start careerbrew for the first time",
        "/careerbrew",
        "**adopt**",
        "**merge**",
        "offer an issue or a PR",
    ):
        assert part in INSTALL, part
    steps = [int(n) for n in re.findall(r"^## (\d+)\. ", INSTALL, re.M)]
    assert steps == list(range(1, len(steps) + 1))
    for ref in re.findall(r"step (\d+)", INSTALL):
        assert int(ref) in steps, ref


def test_every_script_the_prompt_and_playbook_name_exists():
    for script in set(re.findall(r"scripts/[\w-]+\.sh", README[: README.index("## Web app")] + INSTALL)):
        assert (ROOT / script).is_file(), script


def test_host_start_prompt_keeps_one_claude_first_prompt_with_native_entry_fallback():
    prompts = [body for lang, body in _blocks(README) if lang == "text"]
    assert len(prompts) == 1
    assert "Start with Claude" in README
    for part in ("ChatGPT.app", "Work/Codex", "agentbrew commands list --json", "native", "careerbrew CLI", "configured shared memory", "context-dependent"):
        assert part in prompts[0], part
    assert "/prompts:careerbrew" not in prompts[0]


def test_host_start_prompt_playbook_explains_bounded_local_status_without_service_claims():
    for part in ("claude auth status --json", "codex login status", "two seconds", "unknown", "opted-out", "credentials", "service access", "ChatGPT.app", "configured shared memory"):
        assert part in INSTALL, part
