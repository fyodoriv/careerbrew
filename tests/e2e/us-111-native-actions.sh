#!/usr/bin/env bash
# US-111: a per-action native route with rollback. `careerbrew actions --json` prints the same bytes on the
# Python route, through careerbrew-native, and after a rollback (rust-03, docs/rust-migration.md).
source "$(dirname "$0")/lib.sh"
export COLUMNS=80 LINES=24  # the native help and registry are pinned at 80 columns
unset CAREERBREW_NATIVE CAREERBREW_NATIVE_BIN CAREERBREW_ROUTE_ACTIONS

command -v cargo >/dev/null || fail "cargo is needed to build careerbrew-native (scripts/setup.sh installs the Rust toolchain)"
(cd "$ROOT" && cargo build --release --quiet --locked -p careerbrew-cli) || fail "careerbrew-native did not build"
native="$ROOT/target/release/careerbrew-native"
[ -x "$native" ] || fail "no $native after the build"
# A wrapper that marks each native run, then runs the real binary with the same argv.
wrapper="$SCRATCH/bin/careerbrew-native"
printf '#!/bin/sh\necho ran >> "%s"\nexec "%s" "$@"\n' "$SCRATCH/native-ran" "$native" > "$wrapper"
chmod +x "$wrapper"

# 1. Python (the committed route table: every action python).
grep -qx "actions python" "$ROOT/config/native-routes.txt" || fail "the committed route for actions is not python"
cb actions --json > "$SCRATCH/python.json"
[ ! -e "$SCRATCH/native-ran" ] || fail "the default route ran the native binary"
[ "$(wc -c < "$SCRATCH/python.json")" -gt 1000 ] || fail "python actions --json printed too little"

# 2. Native: the env override routes actions to careerbrew-native.
CAREERBREW_ROUTE_ACTIONS=native CAREERBREW_NATIVE_BIN="$wrapper" cb actions --json > "$SCRATCH/native.json"
[ "$(wc -l < "$SCRATCH/native-ran")" -eq 1 ] || fail "the native route did not run careerbrew-native once"
cmp -s "$SCRATCH/python.json" "$SCRATCH/native.json" || fail "native actions --json differs from the python route"

# 3. Rollback: the kill switch, a python override, and a missing binary each print the same bytes from Python.
CAREERBREW_ROUTE_ACTIONS=native CAREERBREW_NATIVE=off CAREERBREW_NATIVE_BIN="$wrapper" cb actions --json > "$SCRATCH/off.json"
CAREERBREW_ROUTE_ACTIONS=python CAREERBREW_NATIVE_BIN="$wrapper" cb actions --json > "$SCRATCH/override.json"
[ "$(wc -l < "$SCRATCH/native-ran")" -eq 1 ] || fail "a rollback still ran careerbrew-native"
CAREERBREW_ROUTE_ACTIONS=native CAREERBREW_NATIVE_BIN="$SCRATCH/no-such-binary" cb actions --json > "$SCRATCH/missing.json"
for mode in off override missing; do
  cmp -s "$SCRATCH/python.json" "$SCRATCH/$mode.json" || fail "rollback mode $mode differs from the python route"
done
event_seen cli.native_fallback
refute "$(cat "$CAREERBREW_HOME/logs/events.jsonl")" "no-such-binary" "the fallback event names no path"

# Error case: native refuses argv outside its pilot (exit 64); Python runs it and logs the refusal.
printf '#!/bin/sh\necho "native: unsupported argv" >&2\nexit 64\n' > "$SCRATCH/bin/refuse"
chmod +x "$SCRATCH/bin/refuse"
CAREERBREW_ROUTE_ACTIONS=native CAREERBREW_NATIVE_BIN="$SCRATCH/bin/refuse" cb actions --json > "$SCRATCH/refused.json"
cmp -s "$SCRATCH/python.json" "$SCRATCH/refused.json" || fail "a native refusal did not fall back to the python output"
event_seen cli.native_unsupported
echo "ok US-111"
