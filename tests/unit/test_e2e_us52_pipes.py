"""Exercise the daily-log story selections with a large synthetic inbox."""

import re
import subprocess
from pathlib import Path

import pytest


@pytest.mark.parametrize("name", ["id", "gid", "qid", "pid"])
def test_story_selection_reads_large_inbox_without_broken_pipe(tmp_path, name):
    story = Path(__file__).resolve().parents[1] / "e2e" / "us-52-daily-log.sh"
    lines = story.read_text().splitlines()
    selection = next(line for line in lines if re.match(rf"^{name}=", line))
    inbox = tmp_path / "inbox"
    inbox.write_text(
        "proposal task-done-synthetic task-done\nproposal goal-synthetic goal\nquestion competency-synthetic competency\nproposal plugin-synthetic plugin\nproposal task-done-later task-done\nproposal goal-later goal\nquestion competency-later competency\nproposal plugin-later plugin\n"
        + "filler synthetic row\n" * 100000
    )
    script = f'set -euo pipefail\ninbox="$(cat "$1")"\ninbox_path="$1"\ncb() {{ cat "$inbox_path"; }}\n{selection}\nprintf "%s\\n" "${{{name}}}"\n'
    result = subprocess.run(["bash", "-c", script, "synthetic", str(inbox)], capture_output=True, text=True, check=False)
    assert result.returncode == 0, result.stderr
    assert result.stdout.strip() == {"id": "task-done-synthetic", "gid": "goal-synthetic", "qid": "competency-synthetic", "pid": "plugin-synthetic"}[name]


@pytest.mark.parametrize("name", ["qid", "pid"])
def test_story_selection_propagates_inbox_failure(name):
    story = Path(__file__).resolve().parents[1] / "e2e" / "us-52-daily-log.sh"
    selection = next(line for line in story.read_text().splitlines() if re.match(rf"^{name}=", line))
    script = f'set -euo pipefail\ncb() {{ printf "question synthetic competency\\nproposal synthetic plugin\\n"; return 7; }}\n{selection}\n'
    result = subprocess.run(["bash", "-c", script], capture_output=True, text=True, check=False)
    assert result.returncode == 7
