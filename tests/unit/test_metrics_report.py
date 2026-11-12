"""The speed report (scripts/metrics.sh): its step table."""

from __future__ import annotations

import json
import shutil
import subprocess
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _report(tmp_path: Path, rows: list[dict]) -> dict[str, list[str]]:
    (tmp_path / "scripts").mkdir()
    shutil.copy(ROOT / "scripts" / "metrics.sh", tmp_path / "scripts" / "metrics.sh")
    (tmp_path / ".metrics").mkdir()
    (tmp_path / ".metrics" / "gate.jsonl").write_text("".join(json.dumps(r) + "\n" for r in rows))
    out = subprocess.run(["bash", "scripts/metrics.sh"], cwd=tmp_path, capture_output=True, text=True, check=True).stdout
    return {line.split()[0]: line.split()[1:] for line in out.splitlines()[1:] if line.strip()}


def test_the_max_is_the_largest_number_not_the_last_in_text_order(tmp_path):
    # As text "97.0" sorts after "151.9", so a text compare showed a max below the average (2026-10-26).
    rows = [{"event": "step", "name": "e2e", "seconds": s, "ok": True} for s in (151.9, 97.0, 120.5)]
    runs, avg, mx, fails = _report(tmp_path, rows)["e2e"][:4]
    assert (runs, mx, fails) == ("3", "151.9", "0")
    assert float(avg) <= float(mx)
