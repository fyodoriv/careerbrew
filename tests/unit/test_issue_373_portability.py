"""The PM proof uses its checkout's tools, not the original author's paths."""

import re
import shlex
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def synthetic_runner(tmp_path, *, node_ok=True):
    repo = tmp_path / "checkout"
    scripts = repo / "tests" / "safety"
    scripts.mkdir(parents=True)
    shutil.copy(ROOT / "tests/safety/run_issue_373_ui.sh", scripts / "run_issue_373_ui.sh")
    (scripts / "issue373_ui_child.sh").write_text('printf "%s\\n%s\\n%s\\n" "${ISSUE373_NODE:-missing}" "${ISSUE373_PYTHON:-missing}" "${ISSUE373_BROWSER:-missing}" > "$ISSUE373_RUN/selected"\n')
    python = repo / ".venv" / "bin" / "python"
    python.parent.mkdir(parents=True)
    python.write_text("#!/bin/sh\nexit 0\n")
    python.chmod(0o755)
    (repo / "web").mkdir()
    tools = tmp_path / "alternate-tools"
    tools.mkdir()
    browser = tools / "browser"
    browser.write_text("#!/bin/sh\nexit 0\n")
    browser.chmod(0o755)
    node = tools / "node"
    node.write_text("#!/bin/sh\n" + (f"printf '%s\\n' {shlex.quote(str(browser))}\n" if node_ok else "exit 1\n"))
    node.chmod(0o755)
    env = {"HOME": str(tmp_path), "PATH": f"{tools}:/usr/bin:/bin"}
    result = subprocess.run(["/bin/bash", str(scripts / "run_issue_373_ui.sh"), "geometry"], env=env, capture_output=True, text=True, timeout=20)
    selected = list((repo / ".metrics" / "issue373" / "ui-runs").glob("geometry.*/selected"))
    return result, selected, node, python, browser


def test_runner_selects_alternate_checkout_tools(tmp_path):
    result, selected, node, python, browser = synthetic_runner(tmp_path)
    assert result.returncode == 0, result.stderr
    assert len(selected) == 1
    assert selected[0].read_text().splitlines() == [str(node), str(python), str(browser)]


def test_runner_refuses_missing_browser_discovery(tmp_path):
    result, selected, *_ = synthetic_runner(tmp_path, node_ok=False)
    assert result.returncode != 0
    assert not selected


def test_proof_sources_do_not_pin_an_owners_installation():
    files = ["tests/safety/run_issue_373_ui.sh", "tests/safety/issue373_ui_child.sh", "tests/safety/issue373_web_server.sh", "tests/safety/issue373_metadata.py", "web/playwright.issue373.config.ts"]
    for name in files:
        source = (ROOT / name).read_text()
        assert not re.search(r"/(?:Users|home)/[^/\s]+/", source), name
        assert ".venv/lib/python3.12/" not in source, name
