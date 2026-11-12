"""When a checkup last ran on this machine, and at which commits (issue #232).

One JSON file per checkup: `${XDG_STATE_HOME:-~/.local/state}/checkups/<checkup>.json` with
`finished_at` (ISO UTC), `result` (green | degraded | blocked), and `commits` ({repo folder name: short SHA}).
Plugin checkups write their own file in the same folder. It holds repo facts only, never business data.

  python -m careerbrew.checkup_runs last <checkup> [repo ...]
  python -m careerbrew.checkup_runs record <checkup> <result> [repo ...]
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from datetime import datetime
from pathlib import Path
from typing import Any

from . import calendar

RESULTS = ("green", "degraded", "blocked")
_NAME = re.compile(r"^[a-z0-9][a-z0-9-]{0,59}$")


def folder() -> Path:
    return Path(os.environ.get("XDG_STATE_HOME") or Path.home() / ".local" / "state") / "checkups"


def _file(checkup: str) -> Path:
    if not _NAME.match(checkup):
        raise ValueError(f"checkup name {checkup!r}: use lowercase letters, digits, and dashes")
    return folder() / f"{checkup}.json"


def _git(repo: str, *args: str) -> str:
    r = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True, check=False)
    return r.stdout.strip() if r.returncode == 0 else ""


def commits(repos: list[str]) -> dict[str, str]:
    """Short HEAD per repo, keyed by the repo's folder name; a folder that is not a git repo is left out."""
    out = {}
    for repo in repos:
        sha = _git(repo, "rev-parse", "--short", "HEAD")
        if sha:
            out[Path(_git(repo, "rev-parse", "--show-toplevel") or repo).name] = sha
    return out


def read(checkup: str) -> dict[str, Any] | None:
    try:
        return json.loads(_file(checkup).read_text())
    except (OSError, ValueError):
        return None


def record(checkup: str, result: str, repos: list[str]) -> dict[str, Any]:
    if result not in RESULTS:
        raise ValueError(f"result {result!r}: use one of {', '.join(RESULTS)}")
    row = {"finished_at": calendar.now_utc().isoformat(timespec="seconds"), "result": result, "commits": commits(repos)}
    path = _file(checkup)
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(f".{path.name}.{os.getpid()}.tmp")
    tmp.write_text(json.dumps(row, indent=2, sort_keys=True) + "\n")
    tmp.replace(path)
    return row


def _day(stamp: str) -> str:
    when = datetime.fromisoformat(stamp).astimezone(calendar.local_tz())
    return f"{when:%a %b} {when.day} {when:%H:%M}"


def last_lines(checkup: str, repos: list[str]) -> list[str]:
    """What to print at the start: the last run's time and result, then per repo its commit and how many commits since."""
    row = read(checkup)
    if not row:
        return [f"no earlier {checkup} checkup on this machine"]
    lines = [f"last {checkup} checkup: {_day(row['finished_at'])} ({row.get('result', '?')})"]
    now = commits(repos)
    for repo, sha in sorted((row.get("commits") or {}).items()):
        path = next((r for r in repos if Path(_git(r, "rev-parse", "--show-toplevel") or r).name == repo), "")
        since = _git(path, "rev-list", "--count", f"{sha}..HEAD") if path else ""
        tail = f"{since} commit(s) since, now at {now.get(repo, '?')}" if since else "not checked this time"
        lines.append(f"  {repo} at {sha}: {tail}")
    return lines


def main(argv: list[str]) -> int:
    if len(argv) >= 2 and argv[0] == "last":
        print("\n".join(last_lines(argv[1], argv[2:] or ["."])))
        return 0
    if len(argv) >= 3 and argv[0] == "record":
        record(argv[1], argv[2], argv[3:] or ["."])
        return 0
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
