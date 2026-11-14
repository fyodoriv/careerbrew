"""The hosted gate workflow keeps the pins the gate's frozen fixtures need."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/ci.yml"
REFERENCE = ROOT / "xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json"


def test_the_runner_python_is_the_one_in_the_frozen_http_reference():
    frozen = set(re.findall(r"Python/(\d+\.\d+\.\d+)", REFERENCE.read_text()))
    assert len(frozen) == 1, frozen
    pinned = re.findall(r'python-version: "([^"]+)"', WORKFLOW.read_text())
    assert pinned == [frozen.pop()]


def test_the_runner_skips_only_the_picture_comparison_and_says_so():
    story = (ROOT / "tests/e2e/us-75-visual-stories.sh").read_text()
    guard = story.index('"${CAREERBREW_VISUAL_PICTURES:-}" = local-only')
    # The story checker and the test count run before the skip; the comparison runs after it.
    assert story.index("check-stories.py") < guard and story.index("pw_total visual") < guard
    assert guard < story.index("--reporter=line")
    assert "CAREERBREW_VISUAL_PICTURES: local-only" in WORKFLOW.read_text()
    assert "hosted-visual-pictures" in (ROOT / "docs/ci.md").read_text()


def test_no_playwright_config_lets_playwright_fetch_into_the_checkout():
    # On GitHub Actions, in a pull request, Playwright 1.59 runs `git fetch origin <base> --depth=1`
    # to record a diff. That makes the checkout shallow, and US-40 then bundles a `main` with no parents.
    configs = sorted(p for p in (ROOT / "web").rglob("playwright*.config.ts") if "node_modules" not in p.parts)
    assert len(configs) >= 4, configs
    for config in configs:
        assert "captureGitInfo: { commit: false, diff: false }" in config.read_text(), config.relative_to(ROOT)
