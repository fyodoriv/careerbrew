"""373 invocation metadata only; no app import, process launch or signal.

Owned-source hashes are observed-current metadata, not outside-review authority.
Missing lifecycle evidence is BLOCK, never a mutated-source inference.
"""

from __future__ import annotations

import hashlib
import json
import os
import re
import sys
import time
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
SOURCES = (
    "src/careerbrew/day/log.py",
    "src/careerbrew/day/pm_learn.py",
    "src/careerbrew/server.py",
    "src/careerbrew/signals.py",
    "src/careerbrew/cli.py",
    "src/careerbrew/store.py",
    "web/src/daylog.tsx",
    "web/src/api.ts",
    "web/src/dates.ts",
    "web/src/views.tsx",
    "web/src/daylog.test.tsx",
    "web/src/api.test.ts",
    "web/e2e/pm-copy.spec.ts",
    "web/playwright.config.ts",
    "web/package.json",
    "web/package-lock.json",
    "tests/unit/test_issue_185_pm.py",
    "tests/unit/test_issue_238.py",
    "tests/unit/test_issue_336.py",
    "tests/unit/test_pm_sent_reconciliation.py",
    "tests/unit/test_pm_sent_cache.py",
    "tests/unit/test_pm_send.py",
    "tests/unit/conftest.py",
    "tests/e2e/us-59-pm-update.sh",
    "tests/e2e/lib.sh",
    "docs/privacy.md",
    "docs/user-stories.md",
    "web/node_modules/react-markdown/package.json",
    "web/node_modules/react-markdown/lib/index.js",
    "web/node_modules/remark-gfm/package.json",
    "tests/unit/test_issue_373.py",
    "tests/unit/test_issue_373_portability.py",
    "tests/safety/issue373_server.py",
    "tests/safety/run_issue_373_ui.sh",
    "tests/safety/issue373_ui_child.sh",
    "tests/safety/issue373_web_server.sh",
    "tests/safety/issue373_metadata.py",
    "web/issue373/pm-cut.spec.ts",
    "web/e2e/comfort-geometry.ts",
    "web/e2e/comfort-health-boundary.ts",
    "web/e2e/comfort-targets.spec.ts",
    "web/e2e/comfort-tag.spec.ts",
    "web/playwright.issue373.config.ts",
    "web/vite.issue373.config.ts",
    "web/vite.config.ts",
    "web/node_modules/playwright/lib/plugins/webServerPlugin.js",
    "web/node_modules/playwright-core/lib/server/utils/processLauncher.js",
    "web/node_modules/playwright-core/lib/server/registry/index.js",
    "web/node_modules/playwright/lib/runner/testRunner.js",
    "web/node_modules/playwright-core/browsers.json",
    "web/node_modules/playwright/package.json",
    "web/node_modules/playwright-core/package.json",
    "web/node_modules/@axe-core/playwright/package.json",
    "web/node_modules/vite/package.json",
    ".venv/pyvenv.cfg",
)
SCENARIOS = {
    "primary": ["ascii"],
    "dates": ["ascii", "unicode", "legacy-missing", "legacy-invalid", "legacy-naive", "legacy-unavailable"],
    "comfort": ["crowded"],
    "geometry": ["crowded"],
    "proof-before": ["crowded"],
    "proof-after": ["crowded"],
}


def source_hashes(include_installed=False):
    names = {name for name in SOURCES if include_installed or ("node_modules" not in Path(name).parts and ".venv" not in Path(name).parts)}
    if include_installed:
        names.add(f".venv/lib/python{sys.version_info.major}.{sys.version_info.minor}/site-packages/careerbrew.pth")
    for base, suffixes in ((ROOT / "src/careerbrew", {".py"}), (ROOT / "web/src", {".ts", ".tsx", ".css"})):
        for path in base.rglob("*"):
            if path.is_file() and path.suffix in suffixes:
                if path.is_symlink() or not path.resolve().is_relative_to(base):
                    raise RuntimeError("public-source-path-escape")
                names.add(str(path.relative_to(ROOT)))
    return {name: hashlib.sha256((ROOT / name).read_bytes()).hexdigest() for name in sorted(names)}


def write(path, value):
    with path.open("x") as stream:
        stream.write(json.dumps(value, indent=2, sort_keys=True) + "\n")
        stream.flush()
        os.fsync(stream.fileno())


def main():
    if sys.argv[1:] not in (["entry"], ["terminal"]):
        raise SystemExit("BLOCKED: fixed metadata phase required")
    run = Path(os.environ["ISSUE373_RUN"])
    mode = os.environ["ISSUE373_MODE"]
    if mode not in SCENARIOS or run.is_symlink() or not run.resolve().is_relative_to(ROOT / ".metrics/issue373/ui-runs") or (run / ".owned-issue373-ui").read_text() != "registered373\n":
        raise SystemExit("BLOCKED: invalid owned metadata root")
    phase = sys.argv[1]
    if phase == "entry":
        node = Path(os.environ["ISSUE373_NODE"]).resolve(strict=True)
        browser = Path(os.environ["ISSUE373_BROWSER"]).resolve(strict=True)
        write(
            run / "launch-header.json",
            {
                "schema": 1,
                "stage": "before-build-and-app-import",
                "mode": mode,
                "started_monotonic_ns": time.monotonic_ns(),
                "metadata_process_identity": {"pid": os.getpid(), "ppid": os.getppid(), "pgid": os.getpgid(0), "sid": os.getsid(0)},
                "source_basis": "observed-current",
                "source_sha256": source_hashes(include_installed=True),
                "environment_keys": sorted(os.environ),
                "scenarios": SCENARIOS[mode],
                "node": {"resolved": str(node), "sha256": hashlib.sha256(node.read_bytes()).hexdigest()},
                "browser": {"path": str(browser), "sha256": hashlib.sha256(browser.read_bytes()).hexdigest()},
                "python": {"path": sys.executable, "sha256": hashlib.sha256(Path(sys.executable).read_bytes()).hexdigest()},
                "argv": {
                    "playwright": [str(node), "node_modules/playwright/cli.js", "test", "--config", "playwright.issue373.config.ts"],
                    "build": [str(node), "node_modules/vite/bin/vite.js", "build", "--config", "vite.issue373.config.ts", "--configLoader", "runner"],
                    "servers": [[str(ROOT / ".venv/bin/python"), "-I", "-B", str(ROOT / "tests/safety/issue373_server.py"), case, str(run)] for case in SCENARIOS[mode]],
                },
                "bounds": {"whole_Playwright_including_build_startup_ms": 600000, "server_build_startup_ms": 180000, "test_ms": 90000, "expect_ms": 15000, "server_SIGINT_grace_ms": 3000},
                "ownership": "maintained Playwright webServer and browser processLauncher; no new signal target/cleanup adapter",
                "execution_result": None,
            },
        )
        return 0
    errors, preserved, cases = [], None, {}
    rc = 2
    try:
        header = json.loads((run / "launch-header.json").read_text())
        if time.monotonic_ns() - header["started_monotonic_ns"] > 600_000_000_000:
            errors.append("whole-invocation-budget-exceeded")
        preserved = source_hashes(include_installed=True) == header["source_sha256"]
        if not preserved:
            errors.append("source-drift")
        rc = int((run / "playwright.exit").read_text())
        for case in SCENARIOS[mode]:
            ready = json.loads((run / case / "ready.json").read_text())
            terminal = json.loads((run / case / "terminal.json").read_text())
            if ready["identity"] != terminal["identity"] or not terminal["server_closed"] or terminal["forbidden_attempts"] or terminal["outcome"] != "owned-Playwright-SIGINT":
                errors.append("server-boundary-or-close:" + case)
            cases[case] = {"identity": ready["identity"], "port": ready["port"], "terminal": terminal, "ready_sha256": hashlib.sha256((run / case / "ready.json").read_bytes()).hexdigest()}
        raw = (run / "playwright.stderr").read_text()
        launched = set(re.findall(r"<launched> pid=(\d+)", raw))
        closed = set(re.findall(r"\[pid=(\d+)\] <process did exit:", raw))
        if not launched or launched != closed:
            errors.append("Playwright-processLauncher-launch-exit-correspondence")
        proof_paths = sorted((run / "playwright").rglob("boundary-terminal.json"))
        expected = 8 if mode == "geometry" else 1 if mode in ("primary", "dates") else 4
        if len(proof_paths) != expected:
            errors.append("missing-case-boundary-record")
        for path in proof_paths:
            boundary = json.loads(path.read_text())
            if boundary["attempts"] or boundary["errors"]:
                errors.append("browser-boundary")
    except BaseException as error:
        errors.append(type(error).__name__)
    write(
        run / "terminal.json",
        {
            "schema": 1,
            "mode": mode,
            "actual_Playwright_exit": rc,
            "runner_exit": 2 if errors else rc,
            "source_preserved": preserved,
            "errors": errors,
            "servers": cases,
            "scope": "retained owned server/browser exit evidence; no retrospective process census/group-absence claim",
        },
    )
    return 2 if errors else rc


if __name__ == "__main__":
    raise SystemExit(main())
