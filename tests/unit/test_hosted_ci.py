"""The hosted gate workflow keeps the pins the gate's frozen fixtures need."""

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
WORKFLOW = ROOT / ".github/workflows/gate-part.yml"  # each part of ci.yml runs this workflow
REFERENCE = ROOT / "xtask/tests/fixtures/capture/http-reference-telemetry-v4.json"


def test_the_runner_python_is_the_one_in_the_frozen_http_reference():
    frozen = set(re.findall(r"Python/(\d+\.\d+\.\d+)", REFERENCE.read_text()))
    assert len(frozen) == 1, frozen
    pinned = re.findall(r'python-version: "([^"]+)"', WORKFLOW.read_text())
    assert pinned == [frozen.pop()]


def test_each_set_of_pictures_is_compared_only_where_it_was_drawn():
    story = (ROOT / "tests/e2e/us-75-visual-stories.sh").read_text()
    guard = story.index('if [ "$platform" != "$drawn_here" ]; then')
    # The story checker and the test count run before the skip; the comparison runs after it.
    assert story.index("check-stories.py") < guard and story.index("pw_total visual") < guard
    assert guard < story.index("--reporter=line")
    # A developer Mac compares the darwin pictures; only the hosted switch selects the linux ones.
    assert story.index("drawn_here=darwin") < story.index('[ "${CAREERBREW_VISUAL_PICTURES:-}" = hosted ] && drawn_here=linux') < guard
    assert "CAREERBREW_VISUAL_PICTURES: hosted" in WORKFLOW.read_text()
    assert "scripts/visual-pictures.sh fetch" in (ROOT / "docs/web.md").read_text()


def test_no_playwright_config_lets_playwright_fetch_into_the_checkout():
    # On GitHub Actions, in a pull request, Playwright 1.59 runs `git fetch origin <base> --depth=1`
    # to record a diff. That makes the checkout shallow, and US-40 then bundles a `main` with no parents.
    configs = sorted(p for p in (ROOT / "web").rglob("playwright*.config.ts") if "node_modules" not in p.parts)
    assert len(configs) >= 4, configs
    for config in configs:
        assert "captureGitInfo: { commit: false, diff: false }" in config.read_text(), config.relative_to(ROOT)


def test_every_playwright_browser_install_has_a_time_limit_and_retries():
    # A hung `playwright install --with-deps` held a gate part for its whole
    # 60-minute job limit (2026-11-19). Each try is bounded, and a failed or
    # stuck first try runs once more.
    for name in ("gate-part.yml", "visual-pictures.yml"):
        text = (ROOT / ".github" / "workflows" / name).read_text(encoding="utf-8")
        assert text.count("run: npx playwright install --with-deps chromium") == 2, name
        assert text.count("timeout-minutes: 10") >= 2, name
        assert "continue-on-error: true" in text, name
        assert "if: steps.browser.outcome != 'success'" in text, name
