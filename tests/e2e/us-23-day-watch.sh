#!/usr/bin/env bash
# US-23: a light background watcher fills my results-of-the-day list; I can pause and resume it any time.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOTIFY=off
cb work note "feat: shipped search" --tags delivery >/dev/null
expect "$(cb day watch on --interval 1)" "day watcher on" "toggle on"
for _ in $(seq 1 50); do grep -q note= <<<"$(cb day watch status)" && break; sleep 0.2; done
st="$(cb day watch status)"
expect "$st" "running: True" "running"
expect "$st" "today: note=1" "result appended"
python3 - "$st" <<'PY' || fail "cycle stays under 500 ms CPU and 120 MB"
import re, sys
cpu = int(re.search(r"cpu (\d+) ms", sys.argv[1]).group(1)); mem = float(re.search(r"memory ([\d.]+) MB", sys.argv[1]).group(1))
assert cpu < 500 and mem < 120, (cpu, mem)
PY
expect "$(cb day watch pause)" "paused" "pause"
cb work note "feat: while paused" --tags delivery >/dev/null; sleep 2
expect "$(cb day watch status)" "today: note=1" "nothing appended while paused"
expect "$(cb day watch resume)" "resumed" "resume"
for _ in $(seq 1 50); do grep -q note=2 <<<"$(cb day watch status)" && break; sleep 0.2; done
expect "$(cb day watch status)" "today: note=2" "resumed from checkpoint"
expect "$(cb day watch off)" "still running: False" "toggle off"
expect "$(cb day results)" "2 item(s)" "results list"
# The same results command shows remote and local timestamps on one local clock (#334).
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-results"
uv run python - "$SCRATCH/plugin-results" <<'RESULT_TIMES'
import sys
from datetime import date
from pathlib import Path
from careerbrew.day import results
plugin = Path(sys.argv[1])
manifest = plugin / "careerbrew-plugin.yaml"
manifest.write_text(manifest.read_text().replace("agent_may_read_business: false", "agent_may_read_business: true"))
day = date(2026, 10, 30)
results.append([
    {"id": "tz-note", "at": f"{day}T10:12:00", "kind": "note", "title": "Later local note"},
    {"id": "tz-pr", "at": f"{day}T14:11:00Z", "kind": "pr", "title": "Earlier remote change"},
], day)
RESULT_TIMES
cb plugin add "$SCRATCH/plugin-results" >/dev/null
cb config set timezone Etc/GMT+4 >/dev/null
out="$(cb day results --date 2026-10-30 --show)"
expect "$out" "10:11 [pr] Earlier remote change" "remote timestamp uses local time"
uv run python - "$out" <<'RESULT_ORDER'
import sys
text = sys.argv[1]
assert text.index("Earlier remote change") < text.index("Later local note"), text
RESULT_ORDER
echo "ok US-23 day watch"
