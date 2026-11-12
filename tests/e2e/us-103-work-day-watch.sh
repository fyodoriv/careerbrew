#!/usr/bin/env bash
# US-103: one work-day watch loop: a pass under a lock, an interval from measured passes, and no loop left after end of day (issue #264).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-29T09:00+00:00"
cb init >/dev/null
# Missing source coverage fails in both CLI modes before any step is configured.
for mode in text json; do
  set +e
  if [ "$mode" = json ]; then out="$(cb day loop pass --json)"; else out="$(cb day loop pass)"; fi
  rc=$?
  set -e
  [ "$rc" = 1 ] || fail "missing coverage must fail $mode (rc=$rc)"
done
cb config set feed_tool_sources 'mcp__chat__*=chat' >/dev/null
printf '{"messages": []}\n' > "$SCRATCH/quiet.json"
cb work feed add "$SCRATCH/quiet.json" --source chat >/dev/null
cb work feed cycle >/dev/null
mkdir -p "$SCRATCH/plugin-watch" "$SCRATCH/plugin-noexec"
echo one >"$SCRATCH/input.txt"
cat >"$SCRATCH/plugin-watch/careerbrew-plugin.yaml" <<YAML
name: watch-co
version: 1.0.0
work_steps:
  - {step: /repo-watch, scope: repo, cycle: "sh -c 'echo synthetic-cycle-output; echo x >> $SCRATCH/ran'", watch: ["file:$SCRATCH/input.txt"]}
YAML
cat >"$SCRATCH/plugin-noexec/careerbrew-plugin.yaml" <<YAML
name: noexec-co
version: 1.0.0
work_steps:
  - {step: /no-exec, scope: other, cycle: "touch $SCRATCH/noexec-ran"}
YAML
cb plugin add "$SCRATCH/plugin-watch" --allow-exec >/dev/null
cb plugin add "$SCRATCH/plugin-noexec" >/dev/null
out="$(cb day loop pass)"
expect "$out" "/repo-watch ok" "a pass runs a step's cycle"
expect "$out" "next pass at" "the pass names its next re-arm time"
expect "$out" "1.2 x the median pass" "the interval comes from measured passes"
expect "$out" "/no-exec blocked" "error case: a plugin cycle without --allow-exec is blocked"
refute "$out" "synthetic-cycle-output" "privacy: a cycle's output is never printed"
[ ! -e "$SCRATCH/noexec-ran" ] || fail "a plugin cycle ran without --allow-exec"
expect "$(cb day loop pass)" "/repo-watch unchanged" "a step whose watch input did not change is not run again"
echo two >"$SCRATCH/input.txt"
expect "$(cb day loop pass)" "/repo-watch ok" "a changed watch input runs the step again"
[ "$(grep -c x "$SCRATCH/ran")" = 2 ] || fail "the cycle ran $(grep -c x "$SCRATCH/ran") times, not 2"
# Another pass holds the lock: a second one exits at once.
"$ROOT/.venv/bin/python" -c "import fcntl,sys,time; f=open(sys.argv[1],'a+'); fcntl.flock(f,fcntl.LOCK_EX); open(sys.argv[2],'w').close(); time.sleep(20)" \
  "$CAREERBREW_HOME/day/.loop.lock" "$SCRATCH/held" &
holder=$!
for _ in $(seq 1 100); do [ -e "$SCRATCH/held" ] && break; sleep 0.1; done
expect "$(cb day loop pass)" "skipped: previous cycle running" "error case: a pass that starts while one runs is skipped"
kill "$holder" 2>/dev/null || true
wait "$holder" 2>/dev/null || true
event_seen workday.loop
cb day loop add --kind cron --id cron-7f3a --step /repo-watch >/dev/null
expect "$(cb day loop status)" "cron cron-7f3a for /repo-watch" "every loop is recorded"
set +e
out="$(cb day loop end)"
rc=$?
set -e
expect "$rc" "1" "end of day exits 1 while a loop still runs"
expect "$out" "careerbrew day loop stop --id cron-7f3a" "error case: end of day names the loop the agent must stop"
expect "$out" "stopped watch day-loop" "end of day stops careerbrew's own loop"
cb day loop stop --id cron-7f3a >/dev/null
expect "$(cb day loop end)" "0 loop(s) running" "after each loop stops, end of day leaves none running"
expect "$(cb day loop end 2>&1 || true)" "0 loop(s) running" "end of day can run again"
expect "$(cb day loop add --kind cron --id 'has space' 2>&1 || true)" "breaks US-103" "error case: a bad loop id is refused and names the story"
echo "ok US-103 work-day watch loop"
