"""Field data: what careerbrew really did on this machine, from its own content-free event log.

Every CLI run logs `cli.run` (command, status, ms) or `cli.error` (command, error class).
`careerbrew metrics field` turns them into this machine's totals: runs, success rate, run time,
and time saved. The machine label is a salted hash: a random salt kept in the private data
folder plus the host name, hashed; the host name is never written. This command sends nothing:
the aggregate is a local file. `scripts/metrics.sh proof` copies its totals (numbers only)
into `docs/proof/` when the owner runs it.
"""

from __future__ import annotations

import hashlib
import json
import platform
import secrets
from pathlib import Path
from typing import Any

from . import events, paths, store, time_saved


def machine_label() -> str:
    salt_file = paths.sub("field-salt")
    if not salt_file.is_file():
        store.write_text(salt_file, secrets.token_hex(16))
    salt = (store.read_text(salt_file) or "").strip()
    return "m-" + hashlib.sha256(f"{salt}:{platform.node()}".encode()).hexdigest()[:10]


def _rows(log: Path) -> list[dict[str, Any]]:
    if not log.is_file():
        return []
    out = []
    for line in log.read_text(encoding="utf-8").splitlines():
        try:
            row = json.loads(line)
        except ValueError:
            continue
        if row.get("event") in ("cli.run", "cli.error"):
            out.append(row)
    return out


def compute(log: Path | None = None) -> dict[str, Any]:
    """This machine's real-use totals. Counts, rates, seconds, minutes, and ids only."""
    log = log or events.log_path()
    rows = _rows(log)
    # Field recording starts with the first run that logged its time (`ms`).
    timed = [r for r in rows if r.get("event") == "cli.error" or "ms" in r]
    ok = sum(r.get("event") == "cli.run" and r.get("status") == "ok" for r in timed)
    failed = len(timed) - ok
    seconds = [r["ms"] / 1000 for r in timed if isinstance(r.get("ms"), int | float)]
    saved = time_saved.compute(log)
    return {
        "machine": machine_label(),
        "since": min((r["ts"][:10] for r in timed), default=None),
        "runs": len(timed),
        "runs_ok": ok,
        "runs_failed": failed,
        "success_rate": round(100 * ok / len(timed), 1) if timed else None,
        "mean_seconds": round(sum(seconds) / len(seconds), 2) if seconds else None,
        "action_runs": saved["runs_total"],
        "minutes_saved": saved["minutes_saved_total"],
        "minutes_saved_by_week": {w["week"]: w["minutes_saved"] for w in saved["weeks"]},
        "measured_baselines": saved["measured_baselines"],
        "baselines": len(saved["baselines"]),
    }


def out_path() -> Path:
    return paths.sub("out", "field.json")


def write(data: dict[str, Any]) -> Path:
    path = out_path()
    store.write_json_if_changed(path, data)
    return path
