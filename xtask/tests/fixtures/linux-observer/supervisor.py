"""Temporary fixed oracle. Container-invocation lifecycle, not an app handler."""

import json
import os
import subprocess
import sys
import time
from pathlib import Path

work = Path("/work")
mode = sys.argv[1]
if mode in {"--export-manifest", "--export-chunk"}:
    import export as fixed_export

    fixed_export.main(work, 65534, sys.argv[1:])
    raise SystemExit(0)
if mode == "--release":
    (work / "release").touch(exist_ok=False)
    raise SystemExit(0)
if mode == "--status":
    status = work / "status.json"
    if not status.exists():
        raise SystemExit(75)
    print(status.read_text())
    raise SystemExit(0)
assert mode in {"positive", "negative", "--hold"}
started = time.monotonic()
child = None
status = None
if mode != "--hold":
    argv = ["/usr/bin/strace", "-v", "-xx", "-yy", "-s", "4096", "-e", "trace=%process,%file,%network,write,close,close_range,dup,dup2,dup3,fcntl", "-o", "/work/trace", "--output-separately"]
    if mode == "positive":
        argv.append("--follow-forks")
    argv += ["/usr/local/bin/python", "/probe/parent.py", 'write(3, "CAREERBREW-TRANSIENT-V1", 24) = 24 unlink() connect()']
    with (work / "stdout").open("wb") as stdout, (work / "stderr").open("wb") as stderr:
        child = subprocess.Popen(argv, stdin=subprocess.DEVNULL, stdout=stdout, stderr=stderr, env={"PATH": "/usr/local/bin:/usr/bin:/bin", "LC_ALL": "C", "HOME": "/work", "TMPDIR": "/work", "PYTHONDONTWRITEBYTECODE": "1"})
while time.monotonic() - started < 120:
    if child is not None and status is None:
        code = child.poll()
        trace_bytes = sum(file.stat().st_size for file in work.glob("trace*"))
        sizes = [(work / stream).stat().st_size for stream in ("stdout", "stderr")]
        exceeded = trace_bytes > 8388608 or max(sizes) > 1048576
        if code is not None or exceeded or time.monotonic() - started > 25:
            status = {"actual_tracer_wait": code, "tracer_pid": child.pid, "observer_argv": child.args, "transient_absent": not (work / "transient").exists(), "bounds_ok": not exceeded, "complete": code is not None}
            temporary = work / "status.tmp"
            temporary.write_text(json.dumps(status, sort_keys=True))
            os.replace(temporary, work / "status.json")
    if (work / "release").exists():
        raise SystemExit(0 if mode == "--hold" or (status and status["actual_tracer_wait"] == 0 and status["complete"] and status["bounds_ok"]) else 1)
    time.sleep(0.01)
raise SystemExit(124)
