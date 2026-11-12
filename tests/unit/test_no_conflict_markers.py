"""No merge-conflict marker reaches a tracked file, and no union merge hides a conflict on the learnings page."""

import re
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
# git's markers: "<<<<<<< ours", "||||||| base" (diff3 style), ">>>>>>> theirs".
# A lone "=======" is left out on purpose: it is also a Markdown heading underline.
MARKER = re.compile(r"^(<{7}|\|{7}|>{7})( |$)")


def _tracked_text_files():
    names = subprocess.run(["git", "ls-files", "-z"], cwd=ROOT, check=True, capture_output=True).stdout.split(b"\0")
    for name in filter(None, names):
        path = ROOT / name.decode()
        if not path.is_file():
            continue
        data = path.read_bytes()
        if b"\0" in data[:4096]:
            continue
        yield path, data.decode("utf-8", errors="replace")


def test_no_tracked_file_holds_a_conflict_marker():
    found = [f"{path.relative_to(ROOT)}:{n}: {line[:40]}" for path, text in _tracked_text_files() for n, line in enumerate(text.splitlines(), 1) if MARKER.match(line)]
    assert not found, "conflict marker lines left from a merge:\n" + "\n".join(found)


def test_the_learnings_page_has_no_union_merge():
    # Each learning is its own file now (test_learnings_table.py). On the page itself, a union merge of a
    # branch from before the split silently brought back all 81 rows that had moved into their own files.
    attributes = ROOT / ".gitattributes"
    assert "merge=union" not in (attributes.read_text() if attributes.exists() else "")
