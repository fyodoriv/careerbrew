"""Seven registered histories for the local Python test selector."""

from __future__ import annotations

import json
import os
import shutil
import signal
import subprocess
import sys
import time
from pathlib import Path

import pytest

SCRIPT = Path(__file__).resolve().parents[2] / "scripts" / "tests_affected.py"
A = "tests/unit/test_alpha.py"
B = "tests/unit/test_beta.py"
C = "tests/unit/test_other.py"


class Repo:
    def __init__(self, root: Path):
        self.root = root / "repo"
        self.root.mkdir()
        self.git_bin = shutil.which("git")
        assert self.git_bin is not None
        home, config, template = (root / name for name in ("home", "config", "templates"))
        for directory in (home, config, template):
            directory.mkdir()
        self.env = {
            "HOME": str(home),
            "XDG_CONFIG_HOME": str(config),
            "PATH": os.pathsep.join((str(Path(self.git_bin).parent), os.defpath)),
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TEMPLATE_DIR": str(template),
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_ALLOW_PROTOCOL": "",
            "LC_ALL": "C",
            "PYTHONDONTWRITEBYTECODE": "1",
            "PYTEST_DISABLE_PLUGIN_AUTOLOAD": "1",
        }
        self.git("init", "-q")
        self.git("config", "core.hooksPath", os.devnull)
        self.git("config", "commit.gpgsign", "false")
        self.git("config", "user.name", "Synthetic Test")
        self.git("config", "user.email", "fixture@example.test")

    def git(self, *args: str) -> str:
        return subprocess.run(
            [self.git_bin, *args],
            cwd=self.root,
            env=self.env,
            capture_output=True,
            text=True,
            check=True,
            timeout=15,
        ).stdout.strip()

    def write(self, path: str, content: str) -> None:
        target = self.root / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(content, encoding="utf-8")

    def commit(self) -> str:
        self.git("add", "-A")
        self.git("commit", "-qm", "synthetic history")
        return self.git("rev-parse", "HEAD")

    def baseline(self, alpha: str = "", beta: str = "", other: str = "") -> str:
        for path, body in ((A, alpha), (B, beta), (C, other)):
            self.write(path, body + "\n")
        return self.commit()

    def invoke(self, *args: str, audit: str | None = None) -> subprocess.CompletedProcess[str]:
        argv = [sys.executable, "-B", str(SCRIPT), *args]
        if audit is not None:
            argv = [sys.executable, "-B", "-c", audit, str(SCRIPT), *args]
        return subprocess.run(
            argv,
            cwd=self.root,
            env=self.env,
            capture_output=True,
            text=True,
            check=False,
            timeout=20,
        )

    def assert_modules(self, *expected: str, ref: str | None = None) -> None:
        result = self.invoke(*([ref] if ref is not None else []))
        assert result.returncode == 0, result.stderr
        assert result.stderr == ""
        assert result.stdout.splitlines() == sorted(expected)


@pytest.fixture
def repo(tmp_path: Path) -> Repo:
    return Repo(tmp_path)


def test_union_changes_and_ref(repo: Repo) -> None:
    repo.write(".gitignore", "ignored.md\n")
    for name in ("notes with spaces.md", "working.md", "committed.md"):
        repo.write(name, "before")
    base = repo.baseline(
        'name = "notes with spaces.md"',
        'name = "working.md"',
        'names = ("committed.md", "ignored.md", "untracked.md")',
    )
    repo.write("committed.md", "after")
    repo.commit()
    repo.write("notes with spaces.md", "staged")
    repo.git("add", "notes with spaces.md")
    repo.assert_modules(A)
    repo.write("working.md", "unstaged")
    repo.assert_modules(A, B)
    repo.write("ignored.md", "ignored")
    repo.assert_modules(A, B)
    repo.assert_modules(A, B, C, ref=base)
    repo.write("untracked.md", "untracked")
    repo.assert_modules(A, B, C)


def test_test_module_helper_and_deletion(repo: Repo) -> None:
    repo.write("tests/unit/helper.py", "value = 1\n")
    repo.baseline()
    repo.write(A, "# edited test\n")
    repo.assert_modules(A)
    repo.write("tests/unit/helper.py", "value = 2\n")
    repo.assert_modules(A, B, C)
    repo.git("checkout", "--", "tests/unit/helper.py")
    (repo.root / B).unlink()
    repo.assert_modules(A, C)


def test_literal_join_glob_and_skill(repo: Repo) -> None:
    repo.baseline(
        'direct = "docs/one.md"\njoined = ROOT / "docs" / "two.md"',
        'pattern = (ROOT / "docs" / "recipes").glob("*.md")',
        'pattern = (ROOT / "docs" / "nested").rglob("*.md")\nskill = "skills/local-review"',
    )
    for changed, expected in (
        ("docs/one.md", A),
        ("docs/two.md", A),
        ("docs/recipes/sample.md", B),
        ("docs/nested/direct.md", C),
        ("docs/nested/deeper/sample.md", C),
        ("skills/local-review/notes.md", C),
    ):
        repo.write(changed, "changed")
        repo.assert_modules(expected)
        (repo.root / changed).unlink()
    repo.write("docs/recipes/deeper/no.md", "outside single-segment glob")
    repo.assert_modules()


def test_script_import_literal_and_data_reader(repo: Repo) -> None:
    repo.write("scripts/generate.py", 'DATA = ROOT / "config" / "input.tsv"\n')
    repo.write("scripts/read.sh", 'cat < "${ROOT}/config/shell data.tsv"\ncat < "$ROOT/ROOT-DATA.md"\n')
    repo.write("config/input.tsv", "before\n")
    repo.write("config/shell data.tsv", "before\n")
    repo.write("ROOT-DATA.md", "before\n")
    repo.baseline("from scripts import generate", 'source = "scripts/generate.py"', 'source = "scripts/read.sh"')
    repo.write("scripts/generate.py", '# change\nDATA = ROOT / "config" / "input.tsv"\n')
    repo.assert_modules(A, B)
    repo.git("checkout", "--", "scripts/generate.py")
    repo.write("config/input.tsv", "after\n")
    repo.assert_modules(A, B)
    repo.git("checkout", "--", "config/input.tsv")
    repo.write("config/shell data.tsv", "after\n")
    repo.assert_modules(C)
    repo.git("checkout", "--", "config/shell data.tsv")
    repo.write("ROOT-DATA.md", "after\n")
    repo.assert_modules(C)


def test_safe_fallback_and_unmatched_markdown(repo: Repo) -> None:
    repo.write("scripts/check.py", "value = 1\n")
    repo.baseline()
    repo.write("unmatched.md", "no readers")
    repo.assert_modules()
    repo.write("unknown.cfg", "no readers")
    repo.assert_modules(A, B, C)
    (repo.root / "unknown.cfg").unlink()
    repo.write("scripts/check.py", "if invalid syntax\n")
    repo.assert_modules(A, B, C)


def test_invalid_ref_and_forbidden_reads(repo: Repo, tmp_path: Path) -> None:
    repo.baseline('path = "private/secret.md"')
    result = repo.invoke("missing-ref-359")
    assert result.returncode != 0 and result.stdout == ""
    target = tmp_path / "forbidden.py"
    target.write_text('path = "unmatched.md"\n', encoding="utf-8")
    audit = """
import os, runpy, sys
def deny(event, args):
    if event == "open" and isinstance(args[0], (str, bytes)):
        path = os.fsdecode(args[0])
        if os.path.isabs(path):
            root = os.getcwd()
            if path == root:
                return
            prefix = root + os.sep
            path = path[len(prefix):] if path.startswith(prefix) else os.path.basename(path)
        if set(path.split("/")) & {"private", "secret.md", "unsafe.py", "test_symlink.py", "forbidden.py"}:
            raise RuntimeError("forbidden fixture read")
sys.addaudithook(deny)
sys.argv = sys.argv[1:]
runpy.run_path(sys.argv[0], run_name="__main__")
"""

    def assert_safe_fallback() -> None:
        result = repo.invoke(audit=audit)
        assert result.returncode == 0, result.stderr
        assert result.stdout.splitlines() == [A, B, C]

    repo.write("private/secret.md", "synthetic private input")
    assert_safe_fallback()
    (repo.root / "private/secret.md").unlink()
    (repo.root / "scripts").mkdir()
    (repo.root / "scripts/unsafe.py").symlink_to(target)
    assert_safe_fallback()
    (repo.root / "scripts/unsafe.py").unlink()
    (repo.root / "tests/unit/test_symlink.py").symlink_to(target)
    assert_safe_fallback()
    (repo.root / "tests/unit/test_symlink.py").unlink()
    (repo.root / "tests/conftest.py").symlink_to(target)
    blocked = repo.invoke("--run")
    assert blocked.returncode == 2 and "could not start" in blocked.stderr


def test_run_separate_python_processes_and_failure(repo: Repo) -> None:
    repo.write(".gitignore", "*.json\n*.overlap\n*.ready\n*.terminated\n.pytest_cache/\n")
    repo.baseline()
    child = """
import json, os, pathlib, sys, time
def test_registered_child():
    stem = pathlib.Path(__file__).stem
    pathlib.Path(stem + ".json").write_text(json.dumps({"argv": sys.orig_argv, "pid": os.getpid(), "python": sys.executable}))
    deadline = time.monotonic() + 10
    while not all(pathlib.Path(name + ".json").exists() for name in ("test_alpha", "test_beta")):
        assert time.monotonic() < deadline
        time.sleep(0.01)
    pathlib.Path(stem + ".overlap").touch()
    assert stem != "test_beta", "registered synthetic child failure"
"""
    repo.write(A, child)
    repo.write(B, child)
    result = repo.invoke("--run")
    assert result.returncode == 1, result.stderr
    records = []
    for module in (A, B):
        stem = Path(module).stem
        assert (repo.root / f"{stem}.overlap").is_file()
        record = json.loads((repo.root / f"{stem}.json").read_bytes())
        assert record["argv"][1:] == ["-I", "-B", "-m", "pytest", "-q", "--confcutdir", str(repo.root.resolve()), module]
        assert Path(record["python"]).resolve() == Path(sys.executable).resolve()
        records.append(record)
    assert records[0]["pid"] != records[1]["pid"]
    assert not (repo.root / "test_other.json").exists()
    repo.git("checkout", "--", B)
    repo.write(
        A,
        """
import pathlib, signal, time
def test_cancelled_child():
    def stopped(_number, _frame):
        pathlib.Path("child.terminated").touch()
        raise SystemExit(77)
    signal.signal(signal.SIGTERM, stopped)
    pathlib.Path("child.ready").touch()
    time.sleep(12)
    raise AssertionError("selector did not cancel its child")
""",
    )
    with subprocess.Popen([sys.executable, "-B", str(SCRIPT), "--run"], cwd=repo.root, env=repo.env, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL) as selector:
        try:
            deadline = time.monotonic() + 10
            while not (repo.root / "child.ready").exists():
                assert selector.poll() is None and time.monotonic() < deadline
                time.sleep(0.02)
            selector.send_signal(signal.SIGTERM)
            assert selector.wait(timeout=6) == 130
        finally:
            if selector.poll() is None:
                selector.terminate()
                selector.wait(timeout=8)
    assert (repo.root / "child.terminated").is_file()
