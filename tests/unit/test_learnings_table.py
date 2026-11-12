"""The learnings log: one file per change in docs/learnings/, read as one table (task learnings-per-pr-files).

Every PR used to append a row to one table in docs/learnings.md, so each merge made every open PR
conflict on GitHub. Now each PR adds its own file and `scripts/learnings-table.py` builds the table.
"""

import importlib.util
import os
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "learnings-table.py"
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
PAGE = "docs/learnings.md"

_spec = importlib.util.spec_from_file_location("learnings_table", SCRIPT)
assert _spec and _spec.loader
lt = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(lt)


def _learning(change: str, predicted: str = "0 → 1", observed: str = "1", lesson: str = "keep it") -> str:
    return f"# {change}\n\n## Predicted\n\n{predicted}\n\n## Observed\n\n{observed}\n\n## Lesson\n\n{lesson}\n"


def _rows(text: str) -> list[str]:
    return [line for line in text.splitlines() if line.startswith("|")][2:]


def _git(cwd: Path, *args: str, check: bool = True) -> subprocess.CompletedProcess[str]:
    env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_")}
    return subprocess.run([*GIT, *args], cwd=cwd, env=env, capture_output=True, text=True, check=check)


@pytest.fixture
def root(tmp_path):
    (tmp_path / "docs" / "learnings").mkdir(parents=True)
    (tmp_path / PAGE).write_text("# Learnings\n\nOne file per change in learnings/.\n")
    return tmp_path


def test_learnings_rows_form_one_unbroken_table():
    assert lt.check(ROOT) == []
    lines = lt.table(lt.entries(ROOT)[0]).splitlines()
    assert lines[0] == "| Date | Change | Predicted | Observed | Lesson |"
    bad = [line[:60] for line in lines[2:] if len(lt.BARE_PIPE.split(line)) != 7]
    assert not bad, f"rows that do not have 5 cells render broken: {bad}"


def test_the_generated_table_holds_every_row_of_the_last_hand_kept_table():
    # The newest commit whose docs/learnings.md still held the rows by hand: none of them may be lost.
    log = _git(ROOT, "log", "--format=%H", "--", PAGE, check=False)
    old: list[str] = []
    for commit in log.stdout.split():
        old = _rows(_git(ROOT, "show", f"{commit}:{PAGE}").stdout)
        if old:
            break
    if not old:
        pytest.skip("no commit with the hand-kept learnings table in this clone (a shallow clone?)")
    assert len(old) >= 81
    new = [lt.unescaped(row) for row in _rows(lt.table(lt.entries(ROOT)[0]))]
    missing = Counter(old) - Counter(new)
    assert not missing, f"{sum(missing.values())} old row(s) lost or changed: {[row[:60] for row in missing]}"


def test_two_prs_that_each_add_a_learning_merge_without_a_conflict(tmp_path):
    repo = tmp_path / "repo"
    # A tree with no learning yet (the public release tree) has no folder; each branch then makes it.
    if (ROOT / "docs" / "learnings").is_dir():
        shutil.copytree(ROOT / "docs" / "learnings", repo / "docs" / "learnings")
    (repo / "docs").mkdir(parents=True, exist_ok=True)
    shutil.copy(ROOT / PAGE, repo / PAGE)
    (repo / "scripts").mkdir()
    shutil.copy(SCRIPT, repo / "scripts" / SCRIPT.name)
    _git(repo, "init", "-q", "-b", "main")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "main")
    # Same day, slugs next to each other: the worst case for any shared, sorted table.
    for branch in ("alpha", "beta"):
        _git(repo, "switch", "-q", "-c", branch, "main")
        (repo / "docs" / "learnings").mkdir(exist_ok=True)
        (repo / "docs" / "learnings" / f"2026-10-23-{branch}.md").write_text(_learning(f"synthetic change {branch}"))
        done = subprocess.run([sys.executable, str(repo / "scripts" / SCRIPT.name), "--check"], capture_output=True, text=True, check=False)
        assert done.returncode == 0, done.stdout
        _git(repo, "add", "-A")
        _git(repo, "commit", "-qm", f"add {branch}")
    merged = _git(repo, "merge-tree", "--write-tree", "alpha", "beta", check=False)
    assert merged.returncode == 0, merged.stdout
    files = _git(repo, "ls-tree", "-r", "--name-only", merged.stdout.split()[0], "docs/learnings").stdout.split()
    assert {"docs/learnings/" + "2026-10-23-alpha.md", "docs/learnings/" + "2026-10-23-beta.md"} <= set(files)


def test_two_rows_appended_to_one_shared_table_conflict(tmp_path):
    # The control: the measurement above can see a conflict. The old log conflicted this way after every merge.
    repo = tmp_path / "repo"
    repo.mkdir()
    (repo / "log.md").write_text("| Date | Change |\n|---|---|\n| 2026-10-22 | old |\n")
    _git(repo, "init", "-q", "-b", "main")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "main")
    for branch in ("alpha", "beta"):
        _git(repo, "switch", "-q", "-c", branch, "main")
        with (repo / "log.md").open("a") as f:
            f.write(f"| 2026-10-23 | {branch} |\n")
        _git(repo, "commit", "-qam", branch)
    merged = _git(repo, "merge-tree", "--write-tree", "alpha", "beta", check=False)
    assert merged.returncode == 1 and "CONFLICT" in merged.stdout


def test_a_row_written_into_the_page_fails_the_check_and_split_moves_it(root):
    row = "| 2026-10-23 | synthetic change (task synthetic-split) | 0 → 1 | 1 | keep it |"
    page = root / PAGE
    page.write_text(page.read_text() + "\n| Date | Change | Predicted | Observed | Lesson |\n|---|---|---|---|---|\n" + row + "\n")
    problems = lt.check(root)
    assert len(problems) == 1 and "--split" in problems[0] and f"{PAGE}:" in problems[0]
    made = lt.split(root)
    assert [p.name for p in made] == ["2026-10-23-synthetic-split.md"]
    assert lt.check(root) == []
    assert "|" not in page.read_text(), "the table's header lines go too"
    assert _rows(lt.table(lt.entries(root)[0])) == [row]


def test_split_only_removes_a_row_that_already_has_its_file(root):
    # A merge of a branch from before the split can bring every old row back into the page.
    (root / "docs" / "learnings" / "2026-10-22-kept.md").write_text(_learning("kept change"))
    old = _rows(lt.table(lt.entries(root)[0]))[0]
    page = root / PAGE
    page.write_text(page.read_text() + old + "\n")
    assert lt.split(root) == []
    assert sorted(p.name for p in (root / "docs" / "learnings").iterdir()) == ["2026-10-22-kept.md"]
    assert lt.check(root) == []


def test_a_pipe_in_a_learning_is_escaped_so_its_row_keeps_five_cells(root):
    (root / "docs" / "learnings" / "2026-10-23-pipes.md").write_text(_learning("pipes", lesson="strip the diff3 `|||||||` line"))
    [row] = _rows(lt.table(lt.entries(root)[0]))
    assert len(lt.BARE_PIPE.split(row)) == 7
    assert "`\\|\\|\\|\\|\\|\\|\\|`" in row


def test_line_breaks_inside_a_section_become_spaces(root):
    (root / "docs" / "learnings" / "2026-10-23-wrapped.md").write_text(_learning("wrapped", observed="first line\nsecond line\n\nnext paragraph"))
    [row] = _rows(lt.table(lt.entries(root)[0]))
    assert "| first line second line next paragraph |" in row


def test_the_table_lists_the_newest_learning_first(root):
    for name in ("2026-10-20-old.md", "2026-10-23-new-b.md", "2026-10-23-new-a.md"):
        (root / "docs" / "learnings" / name).write_text(_learning(name))
    changes = [lt.BARE_PIPE.split(row)[2].strip() for row in _rows(lt.table(lt.entries(root)[0]))]
    assert changes == ["2026-10-23-new-a.md", "2026-10-23-new-b.md", "2026-10-20-old.md"]


@pytest.mark.parametrize(
    ("name", "text", "says"),
    [
        ("notes.md", _learning("x"), "name it <YYYY-MM-DD>-<slug>.md"),
        ("2026-10-23-Upper.md", _learning("x"), "name it"),
        ("2026-13-40-bad-date.md", _learning("x"), "is not a date"),
        ("2026-10-23-no-title.md", _learning("x").split("\n", 2)[2], "no '# <what changed>'"),
        ("2026-10-23-missing.md", _learning("x").split("## Lesson")[0], "needs the sections"),
        ("2026-10-23-order.md", "# x\n\n## Observed\n\n1\n\n## Predicted\n\n0\n\n## Lesson\n\nl\n", "in that order"),
        ("2026-10-23-empty.md", _learning("x", observed=""), "'## Observed' is empty"),
        ("2026-10-23-extra.md", _learning("x") + "\n## Notes\n\nmore\n", "unexpected heading"),
        ("2026-10-23-stray.md", "intro\n\n" + _learning("x"), "text outside a section"),
    ],
)
def test_a_learning_that_breaks_the_shape_fails_the_check(root, name, text, says):
    (root / "docs" / "learnings" / name).write_text(text)
    problems = lt.check(root)
    assert any(says in p for p in problems), problems
    assert lt.entries(root)[0] == [], "a broken learning stays out of the table"


def test_the_gate_runs_the_learnings_check():
    assert "step learnings uv run --quiet python scripts/learnings-table.py --check" in (ROOT / "scripts" / "check.sh").read_text()
