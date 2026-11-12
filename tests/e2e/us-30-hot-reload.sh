#!/usr/bin/env bash
# US-30: update careerbrew's code while the watcher runs; it switches over between cycles and keeps state, pause, and results.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOTIFY=off CAREERBREW_CODE_DIR="$SCRATCH/code"
mkdir -p "$CAREERBREW_CODE_DIR"; echo "x = 1" > "$CAREERBREW_CODE_DIR/mod.py"
cb work note "feat: before update" --tags delivery >/dev/null
cb day watch on --interval 1 >/dev/null
for _ in $(seq 1 50); do grep -q "note=1" <<<"$(cb day watch status)" && break; sleep 0.2; done
pid1="$(cb day watch status | sed -n 's/.*pid: \([0-9]*\).*/\1/p')"
cb day watch pause >/dev/null
echo "x = 2" > "$CAREERBREW_CODE_DIR/mod.py"   # code update while running
for _ in $(seq 1 100); do grep -q "code reloads: 1" <<<"$(cb day watch status)" && break; sleep 0.2; done
st="$(cb day watch status)"
expect "$st" "code reloads: 1" "watcher reloaded the new code"
expect "$st" "state: paused" "pause survived the update"
expect "$st" "pid: $pid1" "same process (no restart gap)"
expect "$(cb day watch reload)" "next safe point" "explicit reload accepted"
for _ in $(seq 1 100); do grep -q "code reloads: 2" <<<"$(cb day watch status)" && break; sleep 0.2; done
cb day watch resume >/dev/null
cb work note "feat: after update" --tags delivery >/dev/null
for _ in $(seq 1 60); do grep -q "note=2" <<<"$(cb day watch status)" && break; sleep 0.2; done
expect "$(cb day watch status)" "today: note=2" "results continue after the update"
echo "def broken(:" > "$CAREERBREW_CODE_DIR/mod.py"   # a syntax error in the update
cb day watch reload >/dev/null
for _ in $(seq 1 60); do grep -q "last update:" <<<"$(cb day watch status)" && break; sleep 0.2; done
expect "$(cb day watch status)" "failed its import check" "error case: a broken update keeps the old code running"
expect "$(cb day watch off)" "still running: False" "off"
echo "ok US-30 hot reload"
