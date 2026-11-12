import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def test_open_work_lists_names_and_counts_only(tmp_path):
    # A scratch repo, not the real one: on the real repo the run grew with its branches (77 branches: 71 s at load
    # ~200; 131 branches: over 300 s in the gate on 2026-10-29), so a machine-state test kept timing out.
    origin, repo = tmp_path / "origin.git", tmp_path / "repo"
    _git(tmp_path, "init", "-q", "--bare", "-b", "main", str(origin))
    _git(tmp_path, "clone", "-q", str(origin), str(repo))
    for k, v in (("user.email", "t@example.com"), ("user.name", "T"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")):
        _git(repo, "config", k, v)
    (repo / "scripts").mkdir()
    (repo / "scripts" / "open-work.sh").write_bytes((ROOT / "scripts" / "open-work.sh").read_bytes())
    (repo / "scripts" / "open-work.sh").chmod(0o755)
    (repo / "a.txt").write_text("a\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "base")
    _git(repo, "push", "-q", "origin", "main")
    _git(repo, "checkout", "-qb", "feat/x")
    (repo / "b.txt").write_text("b\n")
    _git(repo, "add", "b.txt")
    _git(repo, "commit", "-qm", "x")
    _git(repo, "checkout", "-q", "main")
    env = {"PATH": "/usr/bin:/bin:/opt/homebrew/bin", "HOME": str(tmp_path)}
    out = subprocess.run([str(repo / "scripts" / "open-work.sh")], capture_output=True, text=True, timeout=60, check=True, env=env).stdout
    assert out.strip(), "always prints something ('none' or items)"
    assert all(line.startswith(("- ", "->", "(", "none")) for line in out.splitlines())
    assert "feat/x" in out


def test_checkup_runs_open_work():
    assert "scripts/open-work.sh" in (ROOT / "scripts" / "checkup.sh").read_text()


def _git(cwd, *args):
    r = subprocess.run(["git", *args], cwd=cwd, capture_output=True, text=True)
    assert r.returncode == 0, f"git {' '.join(args)}: {r.stderr}"
    return r.stdout


def test_a_squash_merged_branch_is_not_open_work(tmp_path):
    """Issue #174: GitHub squash-merges each PR, so the branch's own commits never reach main. Its changes are in main,
    so it is not open work; a branch with a change main lacks still is, on this machine and on origin."""
    origin, repo = tmp_path / "origin.git", tmp_path / "repo"
    _git(tmp_path, "init", "-q", "--bare", "-b", "main", str(origin))
    _git(tmp_path, "clone", "-q", str(origin), str(repo))
    for k, v in (("user.email", "t@example.com"), ("user.name", "T"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")):
        _git(repo, "config", k, v)
    (repo / "scripts").mkdir()
    (repo / "scripts" / "open-work.sh").write_bytes((ROOT / "scripts" / "open-work.sh").read_bytes())
    (repo / "scripts" / "open-work.sh").chmod(0o755)
    (repo / "a.txt").write_text("a\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "base")
    _git(repo, "push", "-q", "origin", "main")
    for branch, text in (("feat/merged", "merged\n"), ("feat/open", "open\n")):
        _git(repo, "switch", "-qc", branch, "main")
        (repo / f"{branch.split('/')[1]}.txt").write_text(text)
        _git(repo, "add", "-A")
        _git(repo, "commit", "-qm", "step 1")
        (repo / f"{branch.split('/')[1]}.txt").write_text(text + "more\n")
        _git(repo, "commit", "-qam", "step 2")
        _git(repo, "push", "-q", "origin", branch)
    _git(repo, "switch", "-q", "main")
    _git(repo, "merge", "-q", "--squash", "feat/merged")  # what GitHub's squash merge writes to main
    _git(repo, "commit", "-qm", "feat: merged (#1)")
    (repo / "merged.txt").write_text("Merged\nmore\n")  # main changes the same lines later: a three-way merge conflicts
    _git(repo, "commit", "-qam", "fix: capital (#2)")
    _git(repo, "switch", "-qc", "feat/reverted", "main~2")  # merged, then reverted on main: open again
    (repo / "r.txt").write_text("r\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: r")
    _git(repo, "switch", "-q", "main")
    _git(repo, "merge", "-q", "--squash", "feat/reverted")
    _git(repo, "commit", "-qm", "feat: r (#3)")
    _git(repo, "revert", "--no-edit", "HEAD")
    _git(repo, "push", "-q", "origin", "main")
    _git(repo, "branch", "-q", "feat/local-only", "feat/merged")  # no upstream: was "? unpushed commit(s)"
    out = subprocess.run([str(repo / "scripts" / "open-work.sh")], capture_output=True, text=True, timeout=60, check=True, env={"PATH": "/usr/bin:/bin:/opt/homebrew/bin", "HOME": str(tmp_path)}).stdout
    assert "feat/open" in out and "feat/reverted" in out, out
    assert "merged" not in out and "local-only" not in out, out


def test_a_branch_that_landed_through_other_prs_is_likely_landed_and_counts_are_numbers(tmp_path):
    """Issue #255: a branch whose work landed as two squash PRs, with main rewording a line since, matches no patch id
    and no merged head. It is listed as likely landed with its share, not as work to continue. A branch with no
    upstream prints a number of commits, never "?"."""
    origin, repo = tmp_path / "origin.git", tmp_path / "repo"
    _git(tmp_path, "init", "-q", "--bare", "-b", "main", str(origin))
    _git(tmp_path, "clone", "-q", str(origin), str(repo))
    for k, v in (("user.email", "t@example.com"), ("user.name", "T"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")):
        _git(repo, "config", k, v)
    (repo / "scripts").mkdir()
    (repo / "scripts" / "open-work.sh").write_bytes((ROOT / "scripts" / "open-work.sh").read_bytes())
    (repo / "scripts" / "open-work.sh").chmod(0o755)
    (repo / "a.txt").write_text("a\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "base")
    _git(repo, "push", "-q", "origin", "main")
    x = "".join(f"x line {i}\n" for i in range(10))
    y = "".join(f"y line {i}\n" for i in range(10))
    _git(repo, "switch", "-qc", "fix/split", "main")
    (repo / "x.txt").write_text(x)
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: x")
    (repo / "y.txt").write_text(y)
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: y")
    _git(repo, "switch", "-q", "main")
    (repo / "x.txt").write_text(x)  # landed as its own squash PR
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: x (#4)")
    (repo / "y.txt").write_text(y.replace("y line 9", "y line nine"))  # and the rest, reworded
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: y (#5)")
    _git(repo, "push", "-q", "origin", "main")
    _git(repo, "switch", "-qc", "feat/pushed", "main")  # its pushed tip landed; the local tip adds more
    z = "".join(f"z line {i}\n" for i in range(10))
    (repo / "z.txt").write_text(z)
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: z")
    _git(repo, "push", "-q", "-u", "origin", "feat/pushed")
    (repo / "w.txt").write_text("new work\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: w")
    _git(repo, "switch", "-q", "main")
    (repo / "z.txt").write_text(z + "z footer\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: z (#6)")
    _git(repo, "push", "-q", "origin", "main")
    _git(repo, "switch", "-qc", "refactor/move", "main")  # moves lines main already had: none landed
    (repo / "x.txt").write_text("".join(f"  x line {i}\n" for i in reversed(range(10))))
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "refactor: reverse and indent x")
    _git(repo, "switch", "-qc", "fix/odd-name", "main")  # a file name that would run a command if unquoted
    (repo / "q$(touch${IFS}PWNED)'.txt").write_text("odd\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: odd name")
    _git(repo, "switch", "-qc", "chore/rm", "main")  # only deletes lines: no share to judge
    _git(repo, "rm", "-q", "a.txt")
    _git(repo, "commit", "-qm", "chore: rm a")
    _git(repo, "switch", "-qc", "scratch", "main")  # never pushed, with a change main lacks
    (repo / "s.txt").write_text("scratch only\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "chore: scratch")
    _git(repo, "switch", "-q", "main")
    out = subprocess.run([str(repo / "scripts" / "open-work.sh")], capture_output=True, text=True, timeout=60, check=True, env={"PATH": "/usr/bin:/bin:/opt/homebrew/bin", "HOME": str(tmp_path)}).stdout
    assert "- branch fix/split: likely landed (95% of its added lines are on main): look, then delete the branch" in out, out
    assert "- branch scratch: 1 commit(s) not in main, never pushed" in out, out
    assert "- branch chore/rm: 1 commit(s) not in main, never pushed" in out, out
    assert "- branch refactor/move: 1 commit(s) not in main, never pushed" in out, out
    assert "- branch fix/odd-name: 1 commit(s) not in main, never pushed" in out, out
    assert not list(tmp_path.rglob("PWNED")), "a file name ran as a command"
    assert "- branch feat/pushed: 1 unpushed commit(s)" in out and "feat/pushed: likely landed" not in out, out
    assert "?" not in out, out


def test_a_never_pushed_branch_can_be_retired_with_an_archive_tag_and_shows_again_on_a_new_commit(tmp_path):
    """Issue #277: a never-pushed branch whose work landed reworded (62% of its lines on main) blocked every checkup,
    and the only way out was a forced branch delete, which agent hooks block. Its item names a safe retire command:
    a tag archive/<branch> on its tip hides it, and a new commit on the branch lists it again."""
    origin, repo = tmp_path / "origin.git", tmp_path / "repo"
    _git(tmp_path, "init", "-q", "--bare", "-b", "main", str(origin))
    _git(tmp_path, "clone", "-q", str(origin), str(repo))
    for k, v in (("user.email", "t@example.com"), ("user.name", "T"), ("commit.gpgsign", "false"), ("core.hooksPath", "/dev/null")):
        _git(repo, "config", k, v)
    (repo / "scripts").mkdir()
    (repo / "scripts" / "open-work.sh").write_bytes((ROOT / "scripts" / "open-work.sh").read_bytes())
    (repo / "scripts" / "open-work.sh").chmod(0o755)
    (repo / "a.txt").write_text("a\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "base")
    _git(repo, "push", "-q", "origin", "main")
    _git(repo, "switch", "-qc", "fix/reworded", "main")  # never pushed; main got the same fix in other words
    (repo / "f.txt").write_text("".join(f"fix line {i}\n" for i in range(10)))
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: f")
    _git(repo, "switch", "-qc", "feat/remote", "main")  # pushed only: the origin copy is retired the same way
    (repo / "r.txt").write_text("remote\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: r")
    _git(repo, "push", "-q", "origin", "feat/remote")
    _git(repo, "switch", "-q", "main")
    _git(repo, "update-ref", "-d", "refs/heads/feat/remote")  # only the origin copy is left
    (repo / "f.txt").write_text("".join(f"Fix line number {i}\n" for i in range(10)))
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "fix: f, reworded (#7)")
    _git(repo, "push", "-q", "origin", "main")

    def run():
        out = subprocess.run([str(repo / "scripts" / "open-work.sh")], capture_output=True, text=True, timeout=60, check=True, env={"PATH": "/usr/bin:/bin:/opt/homebrew/bin", "HOME": str(tmp_path)}).stdout
        assert all(line.startswith(("- ", "->", "(", "none")) for line in out.splitlines()), out
        return out

    out = run()
    hint = "- branch fix/reworded: 1 commit(s) not in main, never pushed (if its work landed elsewhere, retire it: git tag archive/fix/reworded fix/reworded)"
    assert hint in out, out
    _git(repo, "tag", "archive/fix/reworded", "fix/reworded")
    _git(repo, "tag", "archive/feat/remote", "origin/feat/remote")
    out = run()
    assert "- branch fix/reworded" not in out and "- branch feat/remote" not in out, out
    assert "(retired: branch fix/reworded matches tag archive/fix/reworded; a new commit lists it again)" in out, out
    assert "(retired: branch feat/remote matches tag archive/feat/remote; a new commit lists it again)" in out, out
    assert out.rstrip().endswith("none"), out
    _git(repo, "switch", "-q", "fix/reworded")
    (repo / "g.txt").write_text("new work after retiring\n")
    _git(repo, "add", "-A")
    _git(repo, "commit", "-qm", "feat: g")
    _git(repo, "switch", "-q", "main")
    out = run()
    assert "- branch fix/reworded: 2 commit(s) not in main, never pushed" in out and "retired: branch fix/reworded" not in out, out
