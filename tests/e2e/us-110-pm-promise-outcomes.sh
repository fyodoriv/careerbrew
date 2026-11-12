#!/usr/bin/env bash
# US-110: the owner can review and accept a move without duplicating a promise.
source "$(dirname "$0")/lib.sh"
browser_story

cb init >/dev/null
export CAREERBREW_NOW="2026-11-03T09:00:00-04:00"
"$ROOT/.venv/bin/python" - "$FIX/pm-promise-outcomes/skipped-workday.json" <<'PY'
import json
import os
import sys
from pathlib import Path

from careerbrew.day import pm_learn

fixture = json.loads(Path(sys.argv[1]).read_bytes())
os.environ["CAREERBREW_NOW"] = fixture["first_clock"]
pm_learn.save_sent(fixture["first_day"], fixture["first_sent"], receipt=fixture["first_receipt"])
os.environ["CAREERBREW_NOW"] = fixture["next_clock"]
pm_learn.save_sent(fixture["next_day"], fixture["repeated_sent"], receipt=fixture["next_receipt"])
PY

out="$(cb work promise outcomes --json)"
proposal="$(printf '%s' "$out" | "$ROOT/.venv/bin/python" -c '
import json
import sys
[row] = json.load(sys.stdin)["outcomes"]
assert row["state"] == "moved" and row["due"] == "2026-11-04"
assert row["id"] and row["promise"].startswith("promise-")
print(row["id"])
')"
expect "$(CAREERBREW_TEST_OWNER= cb work promise accept "$proposal" --yes </dev/null || true)" "needs owner confirmation" "an agent cannot accept a move"
expect "$(CAREERBREW_TEST_OWNER=1 cb work promise accept "$proposal" || true)" "needs owner confirmation" "an owner must confirm the move"
if [ "$(uname -s)" = Darwin ]; then
  accepted="$(HOME="$CAREERBREW_CLIENT_HOME" script -q /dev/null "$ROOT/.venv/bin/careerbrew" work promise accept "$proposal" --yes </dev/null)"
else
  accepted="$(HOME="$CAREERBREW_CLIENT_HOME" script -qec "'$ROOT/.venv/bin/careerbrew' work promise accept '$proposal' --yes" /dev/null </dev/null)"
fi
expect "$accepted" "outcome accepted" "the owner accepts the move from a terminal"
cb work promise stats --days 7 --json | "$ROOT/.venv/bin/python" -c '
import json
import sys
counts = json.load(sys.stdin)
assert counts["made"] == counts["moved"] == 1
assert counts["kept_on_time"] == counts["kept_late"] == 0
'
# Correct the accepted source, then exercise the real owner form against this
# story's synthetic data root. The browser config starts its own server.
HOME="$CAREERBREW_CLIENT_HOME" "$ROOT/.venv/bin/python" - "$FIX/pm-promise-outcomes/skipped-workday.json" <<'PY'
import json
import sys
from pathlib import Path
from careerbrew.day import pm_learn

fixture = json.loads(Path(sys.argv[1]).read_bytes())
corrected = fixture["repeated_sent"].replace("Read the synthetic follow-up", "Read the amended synthetic follow-up")
pm_learn.save_sent(fixture["next_day"], corrected, receipt=fixture["next_receipt"])
PY
cd "$ROOT/web"
export CAREERBREW_PROMISE_E2E_HOME="$CAREERBREW_HOME"
total="$(pw_total promise-e2e -c playwright.promise.config.ts)"
out="$(env -u CAREERBREW_HOME npx playwright test -c playwright.promise.config.ts --reporter=line 2>&1)" || { echo "$out"; fail "promise decision browser test"; }
expect "$out" "$total passed" "the owner can review and resolve the changed decision"
echo "ok US-110 PM promise outcomes"
