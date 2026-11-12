"""Two processes update the watcher state at once (CLI pause/resume vs the running watcher): no crash, no lost update."""

import subprocess
import sys

from careerbrew.day import watch

WRITER = """
import sys
from careerbrew.day import watch
key = sys.argv[1]
for i in range(1, 151):
    st = watch.read_state()
    watch.write_state(**{key: i})
"""


def test_concurrent_state_writers_lose_nothing(home):
    watch.write_state(state="on")
    procs = [subprocess.Popen([sys.executable, "-c", WRITER, k], stderr=subprocess.PIPE, text=True) for k in ("a_count", "b_count")]
    errs = [p.communicate(timeout=120)[1] for p in procs]
    assert all(p.returncode == 0 for p in procs), errs
    st = watch.read_state()
    assert st["a_count"] == 150 and st["b_count"] == 150, st
    assert st["state"] == "on"


def test_update_state_is_atomic_read_modify_write(home):
    watch.write_state(n=0)
    code = "from careerbrew.day import watch\nfor _ in range(100):\n    watch.update_state(lambda s: {'n': s.get('n', 0) + 1})\n"
    procs = [subprocess.Popen([sys.executable, "-c", code], stderr=subprocess.PIPE, text=True) for _ in range(3)]
    errs = [p.communicate(timeout=120)[1] for p in procs]
    assert all(p.returncode == 0 for p in procs), errs
    assert watch.read_state()["n"] == 300
