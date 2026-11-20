"""The watcher starts at any nice level, 20 included (issue #280).

At nice 20 on macOS, `os.nice(0)` raises PermissionError, so the watcher crashed before it ran a cycle.
A zsh background job (+5) that runs `heavy-run` (+15) starts the whole unit run at nice 20.
"""

import os
import subprocess
import sys

import pytest

CHILD = """
import os, sys
os.setpriority(os.PRIO_PROCESS, 0, int(sys.argv[1]))
from careerbrew.day import watch
watch.loop(interval=1, max_cycles=1)
print("nice", os.getpriority(os.PRIO_PROCESS, 0))
"""


@pytest.mark.parametrize("start", [20, 3])
def test_the_watcher_runs_at_any_nice_level_and_never_runs_below_nice_10(home, start):
    # The watcher only raises its nice value; it never lowers it (that needs root).
    if os.getpriority(os.PRIO_PROCESS, 0) > start:
        pytest.skip(f"this run is already above nice {start}, and only root may lower it")
    out = subprocess.run([sys.executable, "-c", CHILD, str(start)], capture_output=True, text=True, timeout=60, env=os.environ.copy(), check=False)
    assert out.returncode == 0, out.stderr
    nice = int(out.stdout.split("nice", 1)[1])
    top = 19 if sys.platform.startswith("linux") else 20  # Linux stops at nice 19; macOS at 20
    assert nice >= 10 and nice >= min(start, top)
