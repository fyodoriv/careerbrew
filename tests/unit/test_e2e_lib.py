"""The e2e stories must not depend on timing.

`printf | grep -q` under `set -o pipefail` fails when grep exits on an early
match and printf gets SIGPIPE: `expect` then reports a needle that is there,
and `refute` misses one that is there.

A story that writes data and then runs Playwright must unset CAREERBREW_HOME:
the web seed keeps a set data root, so the browser test sees the story's data.

The gate runs every browser test: one story runs the whole Playwright suite with
no `-g` filter (US-72), on its own port. Before, the stories ran filtered subsets,
and a test that matched no filter never ran.

The control inventory (web/e2e/controls.spec.ts) maps each control to a test in
web/e2e/controls.json; each named test must exist.

Playwright 1.59 writes a compile-cache entry in place. A run that starts beside
another one on a cold cache can read an entry while it is still empty: that spec
then lists 0 tests, with exit 0 and no error, and a story that counts the list
fails on a short count (task us75-list-count-cold-start). Each browser story
compiles into a cache of its own, and `pw_total` names a spec the list left out.
"""

import json
import os
import re
import subprocess
from pathlib import Path

import pytest

E2E = Path(__file__).resolve().parents[2] / "tests" / "e2e"
LIB = E2E / "lib.sh"
WEB_E2E = E2E.parents[1] / "web" / "e2e"
# The needle comes first, then about 2.7 MB of short lines, so grep -q exits long before the writer is done.
BIG = 'big="needle"$\'\\n\'"$(seq 400000)"'


def run(body: str, tmp_path: Path) -> subprocess.CompletedProcess[str]:
    script = f'source "{LIB}"\n{BIG}\n{body}\n'
    return subprocess.run(
        ["bash", "-c", script],
        capture_output=True,
        text=True,
        env={"PATH": "/usr/bin:/bin", "TMPDIR": str(tmp_path)},
        timeout=60,
    )


def test_expect_finds_an_early_needle_in_a_large_output(tmp_path):
    r = run('expect "$big" needle "early needle"; echo OK', tmp_path)
    assert r.returncode == 0 and r.stdout.strip() == "OK", r.stderr[-300:]


def test_refute_catches_an_early_needle_in_a_large_output(tmp_path):
    r = run('refute "$big" needle "early needle"; echo MISSED', tmp_path)
    assert r.returncode == 1 and "FAIL: early needle (found: needle)" in r.stderr[-300:]
    assert "MISSED" not in r.stdout


def test_a_story_that_writes_data_runs_playwright_on_its_own_seed():
    writes = re.compile(r"(^|[\s(;&|\"])(cb|\S*careerbrew) ")
    bad = []
    for story in sorted(E2E.glob("us-*.sh")):
        wrote = False
        for n, line in enumerate(story.read_text().splitlines(), 1):
            if line.lstrip().startswith("#"):
                continue
            if "playwright test" in line and wrote and "-u CAREERBREW_HOME" not in line:
                bad.append(f"{story.name}:{n}")
            wrote = wrote or bool(writes.search(line))
    assert bad == [], f"run Playwright with env -u CAREERBREW_HOME after the story writes data: {bad}"


def test_each_browser_story_compiles_its_specs_in_a_cache_of_its_own(tmp_path):
    script = f'source "{LIB}"\nbrowser_story\nprintf "%s\\n%s\\n" "${{PWTEST_CACHE_DIR:-}}" "$SCRATCH"\n'
    r = subprocess.run(["bash", "-c", script], capture_output=True, text=True, env={"PATH": "/usr/bin:/bin", "TMPDIR": str(tmp_path)}, timeout=60)
    assert r.returncode == 0, r.stderr[-300:]
    cache, scratch = r.stdout.splitlines()
    assert cache.startswith(scratch + "/"), "browser_story must set PWTEST_CACHE_DIR inside the story's own SCRATCH folder"


def test_a_spec_read_back_empty_from_the_compile_cache_fails_the_count_by_name(tmp_path):
    """The race, without the timing: a cold list fills a cache, then one spec's entry is emptied, as a parallel run
    sees it mid-write. The next list leaves that spec out with exit 0 and no error; pw_total names it."""
    env = {k: v for k, v in os.environ.items() if k != "NODE_OPTIONS"}
    env |= {"TMPDIR": str(tmp_path), "PWTEST_CACHE_DIR": str(tmp_path / "cache"), "CAREERBREW_E2E_OUT": str(tmp_path / "out")}
    script = f'source "{LIB}"\ncd "$ROOT/web"\ntotal="$(pw_total visual -c playwright.visual.config.ts)"\necho "total=$total"\n'

    def count() -> subprocess.CompletedProcess[str]:
        return subprocess.run(["bash", "-c", script], capture_output=True, text=True, env=env, timeout=180)

    cold = count()
    assert cold.returncode == 0 and re.fullmatch(r"total=\d+", cold.stdout.strip()), cold.stderr[-600:]
    assert int(cold.stdout.strip().split("=")[1]) >= 90
    [entry] = (tmp_path / "cache").rglob("*_us24daynextspec.js")  # Playwright honors PWTEST_CACHE_DIR
    entry.write_text("")
    short = count()
    assert short.returncode == 1, "Playwright no longer drops a spec whose cache entry is empty (fixed upstream in 1.62, microsoft/playwright#41739): remove PWTEST_CACHE_DIR from browser_story in tests/e2e/lib.sh, and this test"
    assert "FAIL: Playwright lists a test from every spec file in visual/ (none from: us-24-day-next.spec.ts)" in short.stderr
    assert "Total: " in short.stderr, "the failure shows the list it read"


RECEIPT_DISCOVERY = 'env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS PLAYWRIGHT_JSON_OUTPUT_FILE="$SCRATCH/discovery.json" npx playwright test --list --reporter=json >/dev/null || fail "machine-readable test discovery"'


def receipt_story_valid(lines: list[str]) -> bool:
    active = [line.strip() for line in lines if line.strip() and not line.lstrip().startswith("#")]
    direct = [line for line in active if "playwright test" in line and "--list" in line]
    count = 'total="$(unset CAREERBREW_HOME CAREERBREW_PUBLIC_CARDS; pw_total e2e)"'
    compare = '"$ROOT/.venv/bin/python" "$ROOT/scripts/browser-test-receipts.py" "$SCRATCH/discovery.json" "$SCRATCH/execution.json" || fail "each discovered browser test passed once"'
    return direct == [RECEIPT_DISCOVERY] and active.count(count) == 1 and active.count(compare) == 1


def test_receipt_discovery_exception_is_narrow():
    lines = [
        RECEIPT_DISCOVERY,
        'total="$(unset CAREERBREW_HOME CAREERBREW_PUBLIC_CARDS; pw_total e2e)"',
        '"$ROOT/.venv/bin/python" "$ROOT/scripts/browser-test-receipts.py" "$SCRATCH/discovery.json" "$SCRATCH/execution.json" || fail "each discovered browser test passed once"',
    ]
    assert receipt_story_valid(lines)
    assert not receipt_story_valid(lines + [RECEIPT_DISCOVERY])
    assert not receipt_story_valid([RECEIPT_DISCOVERY.replace(" --list", " -g subset --list"), *lines[1:]])
    assert not receipt_story_valid([lines[0], "# " + lines[1], lines[2]])
    assert not receipt_story_valid([lines[0], lines[1], "# " + lines[2]])
    assert not receipt_story_valid([lines[0], lines[1].replace("pw_total e2e", "pw_total e2e -g subset"), lines[2]])
    assert not receipt_story_valid([lines[0], lines[1], "echo " + lines[2]])


def test_a_story_counts_its_browser_tests_with_pw_total():
    direct = []
    for story in sorted(E2E.glob("us-*.sh")):
        lines = story.read_text().splitlines()
        if story.name == "us-72-every-control.sh":
            assert receipt_story_valid(lines), story.name
            continue
        for n, line in enumerate(lines, 1):
            if not line.lstrip().startswith("#") and "playwright test" in line and "--list" in line:
                direct.append(f"{story.name}:{n}")
    assert direct == [], f"count the tests with pw_total (tests/e2e/lib.sh), which names a spec the list left out: {direct}"
    assert "pw_total " in (E2E / "us-75-visual-stories.sh").read_text()


def playwright_tests() -> list[str]:
    """Every browser test as Playwright greps it: "<spec file> <title>".

    A `${...}` in a template title (the width loop) becomes 0.
    """
    call = re.compile(r"^\s*test\(", re.M)
    title = re.compile(r"""^\s*test\((["'`])(.+?)\1\s*,""", re.M)
    out = []
    for spec in sorted(WEB_E2E.glob("*.spec.ts")):
        text = spec.read_text()
        assert "test.describe(" not in text, f"{spec.name}: a describe title is part of what -g matches; teach this check to read it"
        titles = [re.sub(r"\$\{[^}]*\}", "0", m.group(2)) for m in title.finditer(text)]
        assert len(titles) == len(call.findall(text)), f"{spec.name}: a test( call whose title this check cannot read"
        out += [f"{spec.name} {t}" for t in titles]
    return out


def story_runs() -> list[tuple[str, re.Pattern[str] | None, list[str]]]:
    """Each `playwright test` call in a story: (story, its -g filter or None, the spec files it names)."""
    grep = re.compile(r"""(?:-g|--grep)[ =](?:"([^"]*)"|'([^']*)'|(\S+))""")
    runs = []
    for story in sorted(E2E.glob("us-*.sh")):
        for line in story.read_text().splitlines():
            if line.lstrip().startswith("#") or "playwright test" not in line:
                continue
            args = line.split("playwright test", 1)[1]
            g = grep.search(args)
            # Playwright's -g is a case-insensitive regular expression over "<file> <title>".
            pattern = re.compile(next(p for p in g.groups() if p is not None), re.I) if g else None
            runs.append((story.name, pattern, re.findall(r"e2e/([\w.-]+\.spec\.ts)", args)))
    return runs


def test_the_check_reads_every_browser_test_and_story_run():
    tests = playwright_tests()
    assert len(tests) >= 35 and "app.spec.ts at 0 px every view fits its width without sideways scrolling (issue #67)" in tests
    runs = {(name, p.pattern if p else None, tuple(files)) for name, p, files in story_runs()}
    assert ("us-58-action-feedback.sh", None, ("feedback.spec.ts", "blocks.spec.ts")) in runs
    assert any(name == "us-36-plan-ahead.sh" and p == "Week plans ahead" for name, p, _ in runs)


def test_the_gate_runs_the_whole_browser_suite_on_its_own_port():
    whole = [name for name, p, files in story_runs() if p is None and not files]
    assert whole, "no tests/e2e/us-*.sh runs `npx playwright test` without -g or a spec file, so a new browser test would not run in the gate"
    for name in whole:
        text = (E2E / name).read_text()
        assert re.search(r"^browser_story\b", text, re.M), f"{name}: call browser_story first, so the suite gets its own CAREERBREW_E2E_PORT"
        assert "-u CAREERBREW_HOME" in text, f"{name}: run Playwright with env -u CAREERBREW_HOME, so it seeds its own data root"


def test_titles() -> tuple[set[str], list[re.Pattern[str]]]:
    """Every browser test as controls.json names it ("<spec file> › <title>"), and one pattern per title made in a
    loop (`${v.name}: …`), which matches each name the loop gives it, as controls.spec.ts reads them."""
    title = re.compile(r"""^\s*test\((["'`])(.+?)\1\s*,""", re.M)
    literal: set[str] = set()
    loops: list[re.Pattern[str]] = []
    for spec in sorted(WEB_E2E.glob("*.spec.ts")):
        for m in title.finditer(spec.read_text()):
            name = f"{spec.name} › {m.group(2)}"
            if m.group(1) == "`" and "${" in name:
                loops.append(re.compile("^" + "[^›]+".join(map(re.escape, re.split(r"\$\{[^}]*\}", name))) + "$"))
            else:
                literal.add(name)
    return literal, loops


test_titles.__test__ = False  # type: ignore[attr-defined]  # a helper, not a test


def test_a_title_made_in_a_loop_matches_the_names_it_runs_under():
    literal, loops = test_titles()
    retry = "blocks.spec.ts › Today: loading label at once, an error with Retry, the last data on a reload, gaps between cards"
    assert retry not in literal and any(p.match(retry) for p in loops)
    assert not any(p.match(retry.replace("Retry", "Undo")) for p in loops)
    assert "inputs.spec.ts › Week: a new task's hours and due date reach the planner, and Add task waits for a title" in literal


def test_every_control_maps_to_a_browser_test_that_exists():
    coverage = json.loads((WEB_E2E / "controls.json").read_text(encoding="utf-8"))
    literal, loops = test_titles()
    named = [*coverage["controls"].values(), *(g["test"] for g in coverage["generated"])]
    assert len(coverage["controls"]) > 50 and named
    missing = sorted({t for t in named if t not in literal and not any(p.match(t) for p in loops)})
    assert missing == [], "controls.json names a test that web/e2e/*.spec.ts does not have"
    for g in coverage["generated"]:
        re.compile(g["match"])
        assert g["why"], f"{g['match']}: say why it is a pattern, not one entry per control"


def test_cli_home_is_scratch_scoped_without_changing_browser_home(tmp_path):
    fake = tmp_path / "fake-repo/.venv/bin"
    fake.mkdir(parents=True)
    command = fake / "careerbrew"
    command.write_text('#!/bin/sh\nprintf "%s\\n" "$HOME"\n')
    command.chmod(0o755)
    result = run(f'ROOT="{tmp_path / "fake-repo"}"\nexport HOME="synthetic-browser-home"\nclient_home="$(cb where)"\n[ "$client_home" = "$SCRATCH/client-home" ] && [ "$HOME" = "synthetic-browser-home" ]', tmp_path)
    assert result.returncode == 0, result.stderr


def _story_fixture(tmp_path, name):
    root = tmp_path / "story-source"
    folder = root / "tests/e2e"
    folder.mkdir(parents=True)
    (folder / name).write_text((E2E / name).read_text())
    (folder / "lib.sh").write_text(LIB.read_text())
    (root / "scripts").mkdir()
    (root / "evals").mkdir()
    (root / ".venv/bin").mkdir(parents=True)
    return root, folder / name


def _executable(path, body):
    path.write_text("#!/bin/sh\nset -eu\n" + body)
    path.chmod(0o755)


def _story_env(tmp_path):
    owner = tmp_path / "pretend-owner"
    owner.mkdir()
    bin_dir = owner / "bin"
    bin_dir.mkdir()
    launcher = bin_dir / "careerbrew"
    _executable(launcher, "echo pretend-owner\n")
    env = dict(os.environ, HOME=str(owner), CAREERBREW_BIN_DIR=str(bin_dir), PATH=f"{bin_dir}:{os.environ['PATH']}", TMPDIR=str(tmp_path))
    return owner, launcher, env


def test_setup_story_uses_private_home_without_preseeding_agents(tmp_path):
    root, story = _story_fixture(tmp_path, "us-01-setup.sh")
    owner, launcher, env = _story_env(tmp_path)
    before = launcher.read_bytes()
    receipt = tmp_path / "setup-receipt"
    env["SETUP_RECEIPT"] = str(receipt)
    _executable(
        root / "scripts/setup.sh",
        """
mkdir -p .metrics "$CAREERBREW_BIN_DIR"
preseeded=no
for p in "$HOME/.claude" "$HOME/.codex" "$HOME/.config/agentbrew/state.yaml"; do
  [ ! -e "$p" ] || preseeded=yes
done
printf '#!/bin/sh\\necho copied-launcher\\n' > "$CAREERBREW_BIN_DIR/careerbrew"
chmod +x "$CAREERBREW_BIN_DIR/careerbrew"
printf '%s\\n' "$HOME" "$CAREERBREW_BIN_DIR" "$(command -v careerbrew)" "$preseeded" > "$SETUP_RECEIPT"
echo 'doctor: all checks passed'
""",
    )
    result = subprocess.run(["bash", str(story)], env=env, capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stdout + result.stderr
    home, bin_dir, selected, preseeded = receipt.read_text().splitlines()
    assert launcher.read_bytes() == before, "setup changed the fake owner's launcher through inherited BIN_DIR"
    assert Path(home) != owner and Path(bin_dir).is_relative_to(Path(home))
    assert selected == str(Path(bin_dir) / "careerbrew"), "doctor must select the copied setup's launcher"
    assert preseeded == "no", "the fresh installation must not fake installed agent state"


@pytest.mark.parametrize("explicit", [False, True])
def test_web_seed_and_server_use_synthetic_client_home(tmp_path, explicit):
    root, _ = _story_fixture(tmp_path, "us-01-setup.sh")
    helper = root / "tests/e2e/web-seed-and-serve.sh"
    helper.write_text((E2E / helper.name).read_text())
    owner, _, env = _story_env(tmp_path)
    receipt = tmp_path / "server-receipt"
    child_home = tmp_path / "declared-client" if explicit else tmp_path / "browser-data/client-home"
    for name in ("CAREERBREW_CLAUDE_CONFIG", "CAREERBREW_CODEX_CONFIG"):
        target = owner / name
        target.write_text("owner sentinel")
        env[name] = str(target)
    env |= {"CAREERBREW_HOME": str(tmp_path / "browser-data/data"), "SERVER_RECEIPT": str(receipt)}
    env.pop("CAREERBREW_CLIENT_HOME", None)
    if explicit:
        env["CAREERBREW_CLIENT_HOME"] = str(child_home)
    body = """printf '%s\\n' "$HOME" "$CAREERBREW_CLAUDE_CONFIG" "$CAREERBREW_CODEX_CONFIG" >> "$SERVER_RECEIPT"\n"""
    _executable(root / ".venv/bin/python", body)
    _executable(root / ".venv/bin/careerbrew", body)
    result = subprocess.run(["bash", "-c", 'bash "$1" 8799; printf "%s\\n%s\\n%s\\n" "$HOME" "$CAREERBREW_CLAUDE_CONFIG" "$CAREERBREW_CODEX_CONFIG"', "fixture", str(helper)], env=env, capture_output=True, text=True, timeout=20)
    assert result.returncode == 0, result.stderr
    records = receipt.read_text().splitlines()
    for offset in (0, 3):
        assert records[offset] == str(child_home), "Python child inherited the browser owner's HOME"
        assert all(Path(p).is_relative_to(child_home) for p in records[offset + 1 : offset + 3])
    assert result.stdout.splitlines() == [env["HOME"], env["CAREERBREW_CLAUDE_CONFIG"], env["CAREERBREW_CODEX_CONFIG"]]
    assert all((owner / name).read_text() == "owner sentinel" for name in ("CAREERBREW_CLAUDE_CONFIG", "CAREERBREW_CODEX_CONFIG"))


@pytest.mark.parametrize("name,stage", [("us-01-setup.sh", "setup"), ("us-27-stories-skills.sh", "checker"), ("us-27-stories-skills.sh", "evals"), ("us-34-skill-practice.sh", "evals")])
def test_story_capture_keeps_output_and_child_exit(tmp_path, name, stage):
    root, story = _story_fixture(tmp_path, name)
    _, _, env = _story_env(tmp_path)
    env["TEST_FAIL_STAGE"] = stage
    failed = "echo synthetic-child-stdout\necho synthetic-child-stderr >&2\nexit 7\n"
    _executable(root / "scripts/setup.sh", failed)
    _executable(root / ".venv/bin/python", 'if [ "$TEST_FAIL_STAGE" = checker ]; then\n' + failed + "fi\necho '0 problem(s) job helper=1 job search=1'\n")
    _executable(root / "evals/run.sh", failed)
    result = subprocess.run(["bash", str(story)], env=env, capture_output=True, text=True, timeout=20)
    assert result.returncode == 7, result.stdout + result.stderr
    assert "synthetic-child-stdout" in result.stdout + result.stderr
    assert "synthetic-child-stderr" in result.stdout + result.stderr
