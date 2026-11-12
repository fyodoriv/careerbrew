"""Every careerbrew action runs in an end-to-end test (goal 2026-10-22: "cover every single feature").

An action is covered when a story script in `tests/e2e/` runs it as a command (`cb <words>` or
`.venv/bin/careerbrew <words>`, outside a comment). A browser spec does not count: a mocked answer or an
opened form names an action without running it (the web path is the controls inventory's job, US-72).
The registry comes from the CLI parser, so a new command fails here until a story runs it. A command
that must not run for real in a story (a hidden secret prompt) is covered by its refusal case: the
matcher cannot tell an error-only run from a full one, so read the case before trusting the count.
"""

import re
from pathlib import Path

from careerbrew import actions

ROOT = Path(__file__).resolve().parents[2]


def _stories() -> str:
    lines = (line for p in sorted((ROOT / "tests" / "e2e").glob("*.sh")) for line in p.read_text(encoding="utf-8").splitlines())
    return "\n".join(line for line in lines if not line.lstrip().startswith("#"))  # a comment is not a run


def covered(name: str, stories: str) -> bool:
    words = r"\s+".join(re.escape(w) for w in name.split("."))
    return bool(re.search(rf"(?:(?<![\w\"'-])cb|bin/careerbrew\"?)\s+{words}(?![\w-])", stories))


def test_every_action_runs_in_an_e2e_test():
    stories = _stories()
    missing = sorted(a["name"] for a in actions.catalog() if not covered(a["name"], stories))
    assert not missing, f"actions no e2e test runs (add a case to the story that owns each): {missing}"


def test_the_matcher_needs_the_whole_command():
    stories = 'cb jobs scan\nout="$(cb work feed show)"\nexec "$ROOT/.venv/bin/careerbrew" serve --port 1\n'
    assert covered("jobs.scan", stories)
    assert covered("work.feed.show", stories)
    assert covered("serve", stories)
    assert not covered("work.feed", "cb work feed-x"), "a longer word is another command"
    assert not covered("jobs.log", stories), "a command nobody runs is missing"
    assert not covered("notify.open", 'expect "$last" "careerbrew notify open n-" "the pop-up names it"'), "a string is not a run"
