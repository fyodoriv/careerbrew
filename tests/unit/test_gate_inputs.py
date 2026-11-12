"""Gate step inputs (task gate-step-inputs): a step skips on a cache hit only when no file it reads changed.

scripts/check.sh gives some steps nx-style inputs: git pathspecs that leave out notes (TASKS.md,
RECURRING.md, docs/learnings/). A cache hit is a false green if a step reads a path its inputs
leave out. So this test greps all code a step runs for the left-out paths, and for walks over the
whole repo, and fails on any hit that is not on a reviewed list with its reason.
"""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
CODE = re.compile(r"\.(py|sh|ts|tsx|js|mjs|cjs|json|ya?ml|toml)$|^hooks/|^lefthook")
# A walk, search, or copy of a folder: a hit when the line names the repo root, or a story runs it after `cd "$ROOT"`.
WALK = re.compile(
    r"git (-C \S+ )?(ls-files|ls-tree|grep|archive|bundle|clone)\b|\"ls-files\"|ls-tree|rglob\(|os\.walk|listdir|scandir|copytree\("
    r"|cp -[a-zA-Z]*[rR]|\btar\b|rsync|\bfind\b|grep -[a-zA-Z]*[rR]|\brg\b|glob\(\"\*\*|readdirSync|fs\.cp"
)
# A listing of the repo root itself: TASKS.md and RECURRING.md sit there.
ROOT_LIST = re.compile(r"\b(ROOT|REPO_ROOT)\)?\.(r?glob|iterdir)\(|listdir\(\s*(str\()?(ROOT|REPO_ROOT)|\"\$\{?ROOT\}?\"/\*")
OTHER_VAR = re.compile(r"\$\{?(?!ROOT\b)[A-Za-z_]")
CD_ROOT = re.compile(r"^\s*cd \"?\$\{?ROOT\}?\"?\s*$")
RUNS_PYTEST = re.compile(r"-m pytest|\buv run [^'\"]*\bpytest\b|(^|[;&|(]\s*)pytest\b")
AT_ROOT = re.compile(r"\bROOT\b|REPO_ROOT|\$\{?ROOT\b|\$\{?root\b|\brepo\b|\"\.\"|\s\.\s|__dirname")

# Mentions of a left-out path in code that some excluding step runs, each checked by hand.
MENTIONS = {
    ("scripts/check.sh", "*"): "the gate itself: its docs, learnings, leaks, and tasks steps read every file",
    ("scripts/input-key.sh", "TASKS.md"): "a comment: an example pathspec",
    ("scripts/metrics.sh", "RECURRING.md"): "a comment",
    ("scripts/checkup.sh", "TASKS.md"): "reads TASKS.md of the repo it runs in; tests and stories run it only in a temp repo with its own TASKS.md",
    ("scripts/checkup.sh", "RECURRING.md"): "a comment",
    ("scripts/learnings-table.py", "docs/learnings"): "run by the learnings step and test_learnings_table.py (unit), which read every file; no story runs it",
    ("src/careerbrew/judge.py", "TASKS.md"): "comments and a message that name a task",
}
# Walks over the whole repo in code that some excluding step runs, each checked by hand.
WALKS = {
    "tests/e2e/us-01-setup.sh": "copies the repo, then runs scripts/setup.sh and doctor.sh in the copy; those scripts are in this test's grep",
    "tests/e2e/us-26-sources.sh": "copies knowledge/, config/, and scripts/: none holds a left-out path",
    "tests/e2e/us-40-corporate-rehearsal.sh": "bundles the committed main branch (never in any cache key) and reads only its push remote and checkup state",
    "tests/e2e/us-43-data-show.sh": "snapshots the files of its data folder ($CAREERBREW_HOME), not the repo",
    "tests/e2e/us-80-learn-project.sh": "lists the files of its temp repo ($repo) to prove learning writes nothing there",
    "src/careerbrew/machine.py": "walks a job-hunt data folder (_data_files), not the repo",
    "src/careerbrew/learn/project.py": "reads docs/**/*.md of the repo given to `learn start --repo`; no story gives it this repo (checked below)",
    "src/careerbrew/privacy.py": (
        "scan_repo greps every tracked file for sensitive terms (stories: `privacy scan`, `machine ready`). With no terms only paths count;"
        " a term written in a tracked file finds that file whatever the notes hold, and a term made at runtime is in no note,"
        " so a note never turns a scan a story expects clean into a finding"
    ),
}


def _git_files() -> list[str]:
    # Untracked files too: the gate runs before a new script is committed.
    args = ["git", "ls-files", "--cached", "--others", "--exclude-standard"]
    return sorted(set(subprocess.run(args, cwd=ROOT, check=True, capture_output=True, text=True).stdout.splitlines()))


def _steps() -> list[str]:
    text = (ROOT / "scripts" / "check.sh").read_text(encoding="utf-8")
    return sorted(set(re.findall(r"\bstep ([a-z][a-z0-9-]*) ", text)))


def _left_out() -> dict[str, set[str]]:
    """{step: the paths its inputs leave out}, from `scripts/check.sh inputs`."""
    out = subprocess.run(["bash", "scripts/check.sh", "inputs", *_steps()], cwd=ROOT, check=True, capture_output=True, text=True).stdout
    left: dict[str, set[str]] = {}
    for line in out.splitlines():
        step, _, specs = line.partition("\t")
        for spec in specs.split():
            # Only exclusions: this test proves a path is not read, so it cannot prove a positive input list complete.
            assert spec.startswith(":(exclude)"), f"{step}: input {spec!r} is not an exclusion; this test cannot prove it"
            left.setdefault(step, set()).add(spec.removeprefix(":(exclude)").rstrip("/"))
    return left


def _story_run_tests() -> set[str] | None:
    """The unit test modules an e2e story runs, with conftest.py and the modules they import; None: all of them."""
    unit = ROOT / "tests" / "unit"
    run: set[str] = set()
    for story in sorted((ROOT / "tests" / "e2e").glob("*.sh")):
        for line in story.read_text(encoding="utf-8").splitlines():
            if not RUNS_PYTEST.search(line) or line.lstrip().startswith(("#", "printf", "echo", "expect", "refute")):
                continue
            names = re.findall(r"tests/unit/(\w+)\.py", line)
            if not names:
                return None  # a whole folder
            run |= set(names) | {"conftest"}
    todo = list(run)
    while todo:
        path = unit / f"{todo.pop()}.py"
        for name in re.findall(r"^\s*(?:from (?:tests\.unit)?\.?(\w+) import|import (?:tests\.unit\.)?(\w+))", path.read_text(encoding="utf-8") if path.exists() else "", re.M):
            mod = next(n for n in name if n)
            if (unit / f"{mod}.py").exists() and mod not in run:
                run.add(mod)
                todo.append(mod)
    return run


def _scope(rel: str, steps: list[str]) -> set[str]:
    """The gate steps that may run this file. Unknown files: every step."""
    if rel.startswith("tests/unit/"):
        run = _story_run_tests()
        return {"unit"} | ({"e2e"} if run is None or Path(rel).stem in run else set())
    if rel.startswith("tests/e2e/") or rel.startswith(("web/e2e/", "web/visual/")):
        return {"e2e"}
    return set(steps)


def _needle(path: str) -> re.Pattern[str]:
    # The whole path, or its last part on its own (catches ROOT / "docs" / "learnings").
    last = re.escape(path.rsplit("/", 1)[-1])
    return re.compile(rf"{re.escape(path)}|(?<![\w.-]){last}(?![\w.-])")


def _code_files() -> list[str]:
    return [f for f in _git_files() if CODE.search(f) and (ROOT / f).is_file() and "/node_modules/" not in f]


def test_some_heavy_steps_leave_out_notes_and_unit_reads_everything():
    left = _left_out()
    assert {"TASKS.md", "RECURRING.md", "docs/learnings"} <= left.get("e2e", set()), left
    assert "exercises" in left and "web-build" in left, left
    # A conflict marker in any tracked file fails test_no_conflict_markers.py, so unit reads every file.
    assert "unit" not in left, left
    for step in ("docs", "learnings", "leaks", "tasks", "secrets"):
        assert step not in left, f"{step} checks notes or every file, so it must read every file"


def test_no_step_reads_a_path_its_inputs_leave_out():
    steps, left = _steps(), _left_out()
    found = []
    for rel in _code_files():
        text = (ROOT / rel).read_text(encoding="utf-8", errors="replace")
        excluding = {s: left[s] for s in _scope(rel, steps) if s in left}
        for path in set().union(*excluding.values()) if excluding else ():
            if (rel, "*") in MENTIONS or (rel, path) in MENTIONS:
                continue
            for n, line in enumerate(text.splitlines(), 1):
                if _needle(path).search(line):
                    who = sorted(s for s, paths in excluding.items() if path in paths)
                    found.append(f"{rel}:{n}: names {path}, which {', '.join(who)} leave(s) out: {line.strip()[:80]}")
    assert not found, "a step reads a path its inputs leave out, so its cache hit could be a false green. Fix: drop the exclusion in inputs_of (scripts/check.sh), or add a reviewed entry to MENTIONS here:\n" + "\n".join(found)


def test_no_step_walks_the_whole_repo_unless_reviewed():
    steps, left = _steps(), _left_out()
    found = []
    for rel in _code_files():
        if rel in WALKS or rel in {"scripts/check.sh", "scripts/input-key.sh"} or not any(s in left for s in _scope(rel, steps)):
            continue
        at_root = rel.startswith("tests/e2e/") and rel.endswith(".sh")  # scripts/e2e.sh starts each story in the repo root
        for n, line in enumerate((ROOT / rel).read_text(encoding="utf-8", errors="replace").splitlines(), 1):
            if rel.endswith(".sh") and re.match(r"^\s*cd ", line):
                at_root = bool(CD_ROOT.match(line))
            if line.lstrip().startswith(("#", "echo ", "printf ")):
                continue
            # In the repo root, a walk with no path of its own, or a path other than $ROOT, reads the repo.
            bare = at_root and not OTHER_VAR.search(line)
            if ROOT_LIST.search(line) or (WALK.search(line) and (bare or AT_ROOT.search(line))):
                found.append(f"{rel}:{n}: {line.strip()[:100]}")
    assert not found, "code that a step with left-out inputs runs walks the whole repo, so it may read a left-out path. Fix: drop the exclusion in inputs_of (scripts/check.sh), or review it and add it to WALKS here:\n" + "\n".join(found)


def test_the_reviewed_lists_hold_no_stale_entry():
    files = set(_git_files())
    stale = [rel for rel, _ in MENTIONS if rel not in files] + [rel for rel in WALKS if rel not in files]
    assert not stale, f"reviewed entries for files that are gone: {stale}"
    for rel, path in MENTIONS:
        if path != "*":
            assert _needle(path).search((ROOT / rel).read_text(encoding="utf-8")), f"{rel} no longer names {path}: remove its entry"
    assert not any("learnings-table" in p.read_text(encoding="utf-8") for p in (ROOT / "tests" / "e2e").glob("*.sh")), "a story runs scripts/learnings-table.py, which reads docs/learnings/: its MENTIONS entry no longer holds"
    stories = {p.name: p.read_text(encoding="utf-8").splitlines() for p in (ROOT / "tests" / "e2e").glob("*.sh")}
    # A story may copy checkup.sh into a temp repo and run that copy; any other line runs the repo's own.
    runs_checkup = [f"{n}: {ln}" for n, lines in stories.items() for ln in lines if "checkup.sh" in ln and not ln.lstrip().startswith(("#", "cp ")) and "$repo/" not in ln and "checkup.sh prints" not in ln]
    assert not runs_checkup, f"a story runs scripts/checkup.sh in the repo, which reads TASKS.md: its MENTIONS entry no longer holds: {runs_checkup}"
    learns_repo = [f"{n}: {ln}" for n, lines in stories.items() for ln in lines if "learn start" in ln and re.search(r"--repo (\"?\$\{?ROOT|\.(\s|$)|\"?\$\(pwd\))", ln)]
    assert not learns_repo, f"a story learns this repo, which reads docs/learnings/: its WALKS entry no longer holds: {learns_repo}"


def test_the_grep_sees_a_path_built_from_parts_and_unit_tests_a_story_runs():
    assert _needle("docs/learnings").search('ROOT / "docs" / "learnings"')
    assert _needle("TASKS.md").search('cat "$ROOT/TASKS.md"')
    assert not _needle("docs/learnings").search("scripts/learnings-table.py")
    assert _scope("tests/unit/test_server.py", ["unit", "e2e"]) == {"unit", "e2e"}, "us-37 runs it"
    assert _scope("tests/unit/test_learn.py", ["unit", "e2e"]) == {"unit"}
    assert WALK.search('tar -C "$root" -cf - .') and AT_ROOT.search('tar -C "$root" -cf - .')
    for line in ("REPO_ROOT.iterdir()", "os.listdir(ROOT)", 'REPO_ROOT.glob("*.md")', 'for f in "$ROOT"/*.md; do'):
        assert ROOT_LIST.search(line), line
    for line in ('grep -rl x "$ROOT"', 'find "$ROOT" -name "*.md"', 'git -C "$ROOT" grep x', 'rg -l x "$ROOT"'):
        assert WALK.search(line) and AT_ROOT.search(line), line
    assert OTHER_VAR.search('cp -R "$FIX/a" "$SCRATCH/b"') and not OTHER_VAR.search("git ls-files | xargs cat")
    assert CD_ROOT.match('cd "$ROOT"') and not CD_ROOT.match('cd "$ROOT/web"')
    assert {"conftest", "test_server"} <= (_story_run_tests() or set())
