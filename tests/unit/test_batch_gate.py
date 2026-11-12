"""scripts/batch-gate.sh: one full gate per batch of ready PRs, with no loss of stability (task grind-batch-gate).

Each test runs the real script in a tiny repo cloned from a local bare "origin", with stub
scripts/check.sh, scripts/setup.sh, and e2e stories in the PR trees, and a stub `gh` and
`heavy-run` first on PATH. The stub gh squash-merges into the local bare repo, so no test
calls the real gh or pushes anywhere outside the temp folder.
"""

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SCRIPT = ROOT / "scripts" / "batch-gate.sh"

CHECK_SH = """#!/usr/bin/env bash
mode="${1:-full}"
echo "$mode $(git rev-parse 'HEAD^{tree}') $(ls | tr '\\n' ,)" >> "$STUB_LOG"
[ "$mode" = fast ] && [ -e breaks-fast ] && exit 1
[ "$mode" = full ] && [ -e breaks-full ] && exit 1
[ "$mode" = full ] && [ -e half-a ] && [ -e half-b ] && exit 1
if [ "$mode" = full ] && [ -n "${MOVE_MAIN:-}" ]; then
  git -c core.hooksPath=/dev/null push -q origin HEAD:refs/heads/main
fi
exit 0
"""
SETUP_SH = '#!/usr/bin/env bash\necho setup >> "$STUB_LOG"\n'
STORY_SH = '#!/usr/bin/env bash\necho "story $0" >> "$STUB_LOG"\n[ ! -e breaks-story ]\n'
HEAVY_RUN = '#!/usr/bin/env bash\necho "heavy-run $*" >> "$STUB_LOG"\nexec "$@"\n'

# The stub gh: `pr view` reads $STUB_GH/pr-<n>.json; `pr merge` squash-merges refs/pull/<n>/head into
# the local bare origin's main (plus a stray file when the PR json asks for one), as GitHub would.
GH = r"""#!/usr/bin/env python3
import json, os, subprocess, sys, tempfile
d = os.environ["STUB_GH"]
args = sys.argv[1:]
with open(os.path.join(d, "calls.log"), "a") as f:
    f.write(" ".join(args) + "\n")
def pr(n):
    with open(os.path.join(d, f"pr-{n}.json")) as f:
        data = json.load(f)
    if os.path.exists(os.path.join(d, f"merged-{n}")):
        data["state"] = "MERGED"
    return data
if args[:2] == ["pr", "view"]:
    print(json.dumps(pr(args[2])))
elif args[:2] == ["pr", "merge"]:
    n = args[2]
    data = pr(n)
    if data.get("stub_merge_fails"):
        sys.exit("stub gh: the merge is blocked")
    if "--match-head-commit" in args and args[args.index("--match-head-commit") + 1] != data["headRefOid"]:
        sys.exit("stub gh: head moved")
    git = ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=gh", "-c", "user.email=gh@example.com", "-c", "commit.gpgsign=false"]
    with tempfile.TemporaryDirectory() as t:
        run = lambda *a: subprocess.run([*git, *a], cwd=t, check=True, capture_output=True)
        run("clone", "-q", os.environ["STUB_ORIGIN"], ".")
        run("fetch", "-q", "origin", f"refs/pull/{n}/head")
        run("merge", "--squash", "FETCH_HEAD")
        if data.get("stub_extra"):
            open(os.path.join(t, "stray.txt"), "w").write("added by the merge\n")
            run("add", "stray.txt")
        run("commit", "-q", "-m", f"squash #{n}")
        run("push", "-q", "origin", "HEAD:main")
    open(os.path.join(d, f"merged-{n}"), "w").close()
else:
    sys.exit("stub gh: unexpected call " + " ".join(args))
"""


def _x(path: Path, body: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(body)
    path.chmod(0o755)


class Batch:
    def __init__(self, tmp: Path):
        self.tmp = tmp
        self.origin = tmp / "origin.git"
        self.repo = tmp / "repo"
        self.gh = tmp / "gh"
        self.gh.mkdir()
        self.log = tmp / "stub.log"
        self.log.touch()
        gitconfig = tmp / "gitconfig"
        gitconfig.write_text("[user]\n\tname = Test\n\temail = test@example.com\n[core]\n\thooksPath = /dev/null\n[commit]\n\tgpgsign = false\n[init]\n\tdefaultBranch = main\n")
        bin_dir = tmp / "bin"
        _x(bin_dir / "gh", GH)
        _x(bin_dir / "heavy-run", HEAVY_RUN)
        # BASH_ENV (set by some dotfiles) would run first in every bash and could put the real gh back first on PATH.
        self.env = {k: v for k, v in os.environ.items() if not k.startswith("GIT_") and k not in {"BASH_ENV", "ENV"}}
        self.env.update(
            PATH=f"{bin_dir}{os.pathsep}{self.env['PATH']}",
            GIT_CONFIG_GLOBAL=str(gitconfig),
            STUB_GH=str(self.gh),
            STUB_ORIGIN=str(self.origin),
            STUB_LOG=str(self.log),
            BATCH_GATE_WORKTREE=str(tmp / "scratch"),
            BATCH_GATE_RETRY_SECONDS="0",
        )
        self.git("init", "-q", "--bare", str(self.origin), cwd=tmp)
        self.git("clone", "-q", str(self.origin), str(self.repo), cwd=tmp)
        _x(self.repo / "scripts" / "check.sh", CHECK_SH)
        _x(self.repo / "scripts" / "setup.sh", SETUP_SH)
        _x(self.repo / "scripts" / "batch-gate.sh", SCRIPT.read_text())
        (self.repo / "README.md").write_text("synthetic\n")
        (self.repo / ".gitignore").write_text(".metrics/\n")
        self.git("add", "-A")
        self.git("commit", "-q", "-m", "base")
        self.git("push", "-q", "origin", "HEAD:main")

    def git(self, *args: str, cwd: Path | None = None) -> str:
        r = subprocess.run(["git", *args], cwd=cwd or self.repo, env=self.env, capture_output=True, text=True, check=True)
        return r.stdout.strip()

    def pr(self, n: int, files: dict[str, str], *, draft: bool = False, extra: bool = False, blocked: bool = False) -> None:
        """Make PR <n>: a branch off origin/main that adds <files>, pushed as refs/pull/<n>/head."""
        self.git("checkout", "-q", "-B", f"pr-{n}", "origin/main")
        for path, body in files.items():
            _x(self.repo / path, body)
        self.git("add", "-A")
        self.git("commit", "-q", "-m", f"pr {n}")
        sha = self.git("rev-parse", "HEAD")
        self.git("push", "-q", "origin", f"HEAD:refs/pull/{n}/head")
        self.git("checkout", "-q", "--detach", "origin/main")
        data = {"number": n, "state": "OPEN", "isDraft": draft, "baseRefName": "main", "headRefOid": sha, "url": f"https://github.example.com/owner/repo/pull/{n}", "stub_extra": extra, "stub_merge_fails": blocked}
        (self.gh / f"pr-{n}.json").write_text(json.dumps(data))

    def run(self, *args: str, **env: str) -> subprocess.CompletedProcess[str]:
        return subprocess.run(
            ["bash", str(self.repo / "scripts" / "batch-gate.sh"), *args],
            cwd=self.repo,
            env={**self.env, **env},
            capture_output=True,
            text=True,
            check=False,
            timeout=120,
        )

    def merges(self) -> list[str]:
        calls = (self.gh / "calls.log").read_text().splitlines() if (self.gh / "calls.log").exists() else []
        return [c.split()[2] for c in calls if c.startswith("pr merge")]

    def gates(self, mode: str) -> list[str]:
        return [line for line in self.log.read_text().splitlines() if line.startswith(f"{mode} ")]

    def main_files(self) -> set[str]:
        self.git("fetch", "-q", "origin", "main")
        return set(self.git("ls-tree", "--name-only", "-r", "origin/main").splitlines())


@pytest.fixture
def b(tmp_path):
    return Batch(tmp_path)


def _summary(r: subprocess.CompletedProcess[str]) -> str:
    return r.stdout.strip().splitlines()[-1]


def test_a_passing_batch_runs_one_full_gate_then_merges_in_order(b):
    b.pr(11, {"a.txt": "a\n"})
    b.pr(12, {"b.txt": "b\n", "tests/e2e/us-99-b.sh": STORY_SH})
    r = b.run("11", "12")
    assert r.returncode == 0, r.stdout + r.stderr
    assert _summary(r).startswith("batch-gate: passed: one full gate, then squash-merged #11 #12"), r.stdout
    assert b.merges() == ["11", "12"], "merged in the order given"
    assert len(b.gates("fast")) == 2 and len(b.gates("full")) == 1, "each PR's fast gate alone, then one full gate"
    assert sum(line.startswith("story ") for line in b.log.read_text().splitlines()) == 1, "PR 12's own story ran alone"
    assert "heavy-run scripts/check.sh" in b.log.read_text(), "the full gate queues on the Mac-wide lock"
    assert {"a.txt", "b.txt"} <= b.main_files()
    calls = (b.gh / "calls.log").read_text()
    assert "--match-head-commit" in calls and "--admin" not in calls


def test_a_dry_run_gates_but_merges_nothing(b):
    b.pr(13, {"c.txt": "c\n"})
    r = b.run("--dry-run", "13")
    assert r.returncode == 0, r.stdout + r.stderr
    assert "passed (dry run)" in _summary(r) and b.merges() == []


def test_a_failing_batch_merges_nothing_and_bisects_to_the_pr_that_fails_alone(b):
    b.pr(21, {"a.txt": "a\n"})
    b.pr(22, {"breaks-full": "x\n"})
    b.pr(23, {"c.txt": "c\n"})
    before = b.main_files()
    r = b.run("21", "22", "23")
    assert r.returncode == 1, r.stdout + r.stderr
    s = _summary(r)
    assert "nothing merged" in s and "fails alone: #22" in s and "#21" not in s.split("fails alone")[1], s
    assert b.merges() == [] and b.main_files() == before
    # all three, then halves [21,22] and [23], then [21] and [22]
    assert len(b.gates("full")) == 5


def test_prs_that_fail_only_together_are_named_and_nothing_merges(b):
    b.pr(31, {"half-a": "a\n"})
    b.pr(32, {"half-b": "b\n"})
    r = b.run("31", "32")
    assert r.returncode == 1
    assert "fails only together: #31+#32" in _summary(r) and b.merges() == []


def test_a_pr_that_conflicts_leaves_the_batch_and_the_rest_merge_after_one_full_gate(b):
    # A conflict once read as a gate failure; both bisect halves passed, so "fails only together" merged nothing.
    b.pr(81, {"a.txt": "a\n"})
    b.pr(82, {"README.md": "from 82\n"})
    b.pr(83, {"README.md": "from 83\n", "tests/e2e/us-97-c.sh": STORY_SH})
    r = b.run("81", "82", "83")
    assert r.returncode == 0, r.stdout + r.stderr
    s = _summary(r)
    assert "squash-merged #81 #82" in s and "not merged: #83 conflicts with main + the PRs before it" in s, s
    assert b.merges() == ["81", "82"]
    assert len(b.gates("full")) == 1, "one full gate, on the PRs that merge"
    assert len(b.gates("fast")) == 2 and "story " not in b.log.read_text(), "the conflicting PR costs no solo run"


def test_a_batch_where_every_pr_after_the_first_conflicts_still_merges_the_first(b):
    b.pr(84, {"README.md": "from 84\n"})
    b.pr(85, {"README.md": "from 85\n"})
    r = b.run("84", "85")
    assert r.returncode == 0 and b.merges() == ["84"], r.stdout + r.stderr
    assert "not merged: #85 conflicts" in _summary(r)


def test_a_conflict_in_a_dry_run_is_named_too(b):
    b.pr(86, {"README.md": "from 86\n"})
    b.pr(87, {"README.md": "from 87\n"})
    r = b.run("--dry-run", "86", "87")
    assert r.returncode == 0 and b.merges() == []
    assert "would squash-merge #86" in _summary(r) and "#87 conflicts" in _summary(r)


def test_a_left_out_pr_is_named_when_another_fails_alone_or_in_the_full_gate(b):
    b.pr(88, {"breaks-fast": "x\n"})
    b.pr(89, {"README.md": "from 89\n"})
    b.pr(90, {"README.md": "from 90\n"})
    s = _summary(b.run("88", "89", "90"))
    assert "#88 fails its own fast gate" in s and "not merged: #90 conflicts" in s and ";;" not in s, s
    b.pr(91, {"breaks-full": "x\n"})
    b.pr(92, {"README.md": "from 92\n"})
    b.pr(93, {"README.md": "from 93\n"})
    r = b.run("91", "92", "93")
    s = _summary(r)
    assert r.returncode == 1 and "fails alone: #91" in s and "#92" not in s.split("fails alone")[1].split("not merged")[0], s
    assert "not merged: #93 conflicts" in s and b.merges() == [], s


def test_a_pr_that_fails_its_own_fast_gate_stops_the_batch_before_the_full_gate(b):
    b.pr(33, {"a.txt": "a\n"})
    b.pr(34, {"breaks-fast": "x\n"})
    r = b.run("33", "34")
    assert r.returncode == 1 and "#34 fails its own fast gate" in _summary(r)
    assert b.gates("full") == [] and b.merges() == []


def test_a_pr_whose_own_story_fails_alone_stops_the_batch(b):
    b.pr(35, {"breaks-story": "x\n", "tests/e2e/us-98-x.sh": STORY_SH})
    r = b.run("35")
    assert r.returncode == 1 and "#35 fails its own fast gate or e2e stories" in _summary(r)
    assert b.gates("full") == []


def test_a_tree_mismatch_after_a_merge_stops_and_reruns_the_full_gate_on_main(b):
    b.pr(41, {"a.txt": "a\n"}, extra=True)
    b.pr(42, {"b.txt": "b\n"})
    r = b.run("41", "42")
    assert r.returncode == 3, r.stdout + r.stderr
    s = _summary(r)
    assert "differs from the gated tree" in s and "main gate passed" in s and "merged: #41" in s and "not merged: #42" in s, s
    assert b.merges() == ["41"], "no further merge after a mismatch"
    full = b.gates("full")
    assert len(full) == 2 and "stray.txt" in full[-1], "the second full gate ran on main's real tree"


def test_main_moving_after_the_gate_merges_nothing(b):
    b.pr(43, {"a.txt": "a\n"})
    r = b.run("43", MOVE_MAIN="1")
    assert r.returncode == 3 and "main moved since the gate, so nothing merged" in _summary(r)
    assert b.merges() == []


def test_a_merge_that_fails_midway_gates_main_before_anything_else(b):
    b.pr(71, {"a.txt": "a\n"})
    b.pr(72, {"b.txt": "b\n"}, blocked=True)
    b.pr(73, {"c.txt": "c\n"})
    r = b.run("71", "72", "73")
    assert r.returncode == 3, r.stdout + r.stderr
    s = _summary(r)
    assert "#72 did not merge (state OPEN)" in s and "main gate passed" in s and "merged: #71" in s and "not merged: #72 #73" in s, s
    assert b.merges() == ["71", "72"] and len(b.gates("full")) == 2, "main, now holding only #71, got its own full gate"
    assert "-R github.example.com/owner/repo --squash" in (b.gh / "calls.log").read_text(), "gh merges by repo, so it touches no local branch"


def test_a_first_merge_that_fails_means_nothing_merged(b):
    b.pr(74, {"a.txt": "a\n"}, blocked=True)
    r = b.run("74")
    assert r.returncode == 1 and "#74 did not merge (state OPEN), so nothing merged" in _summary(r)


@pytest.mark.parametrize(
    "path",
    ["scripts/check.sh", "scripts/input-key.sh", "scripts/e2e.sh", "tests/e2e/lib.sh", "web/playwright.config.ts", "web/playwright.visual.config.ts"],
)
def test_a_pr_that_touches_a_gate_file_is_refused(b, path):
    b.pr(51, {"a.txt": "a\n"})
    b.pr(52, {path: "#!/usr/bin/env bash\nexit 0\n"})
    r = b.run("51", "52")
    assert r.returncode == 2 and f"#52 touches {path} (ship it alone)" in _summary(r)
    assert b.merges() == [] and b.gates("fast") == [] and b.gates("full") == []


def test_a_pr_that_renames_a_gate_file_is_refused(b):
    b.git("checkout", "-q", "-B", "pr-55", "origin/main")
    b.git("mv", "scripts/check.sh", "scripts/gate.sh")
    b.git("commit", "-q", "-m", "rename")
    sha = b.git("rev-parse", "HEAD")
    b.git("push", "-q", "origin", "HEAD:refs/pull/55/head")
    b.git("checkout", "-q", "--detach", "origin/main")
    data = {"number": 55, "state": "OPEN", "isDraft": False, "baseRefName": "main", "headRefOid": sha, "url": "https://github.example.com/owner/repo/pull/55"}
    (b.gh / "pr-55.json").write_text(json.dumps(data))
    r = b.run("55")
    assert r.returncode == 2 and "#55 touches scripts/check.sh" in _summary(r), _summary(r)


def test_more_than_four_prs_and_drafts_are_refused(b):
    r = b.run("1", "2", "3", "4", "5")
    assert r.returncode == 2 and "a batch holds at most 4" in _summary(r)
    b.pr(53, {"a.txt": "a\n"}, draft=True)
    r = b.run("53")
    assert r.returncode == 2 and "#53 is a draft" in _summary(r)
    assert b.merges() == []


def test_the_script_never_pushes_force_pushes_or_bypasses_review():
    text = SCRIPT.read_text()
    code = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
    assert "git push" not in code and "--force" not in code and "--admin" not in code and "rebase" not in code


def test_the_stub_gh_is_the_gh_a_bash_script_finds(b):
    """Bug found here: BASH_ENV from dotfiles put the real gh back first on PATH inside bash."""
    assert shutil.which("gh", path=b.env["PATH"]) == str(b.tmp / "bin" / "gh")
    r = subprocess.run(["bash", "-c", "command -v gh"], env=b.env, capture_output=True, text=True, check=True)
    assert r.stdout.strip() == str(b.tmp / "bin" / "gh")
