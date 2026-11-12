#!/usr/bin/env bash
# End-to-end tests: one runnable file per user story case in tests/e2e/.
# Each test drives the real program the way a user does, with synthetic data,
# and checks the screen or output, the files, and the log event.
# Every story file tests/e2e/us-*.sh runs (helpers without the us- prefix do not).
# E2E_COLD=1 also re-installs dependencies, like a stranger on a new machine.
#
# Story files run in parallel, E2E_JOBS at a time (default: the CPU count;
# E2E_JOBS=1 runs them one by one). Each file prints one line; its log prints
# only when it fails, and every file runs even after a failure.
# Stories that run Playwright take longest, so they start first. Each one calls
# browser_story (tests/e2e/lib.sh) for a port and an output folder of its own.
# A story with a line "# e2e: serial: <reason>" flaked beside the others, so it
# runs alone after them.
# Each file's result (ok or fail) goes to .metrics/e2e-results.json under the
# input hash (scripts/input-key.sh). Capability coverage runs its own fresh
# stories because external runtime inputs are not bound. Results are written only when
# the inputs did not change during the run, and never outside a git checkout.
set -euo pipefail
cd "$(dirname "$0")/.."
key="$(scripts/input-key.sh 2>/dev/null)" || key=""

shopt -s nullglob
tests=(tests/e2e/us-*.sh)
[ ${#tests[@]} -gt 0 ] || { echo "e2e: no tests/e2e/us-*.sh yet" >&2; exit 1; }

serial=() browser=() other=()
for t in "${tests[@]}"; do
  if grep -q 'npx playwright' "$t"; then
    grep -q '^browser_story' "$t" || { echo "e2e: $t runs Playwright without browser_story, so it would share a port and web/test-results with the others (fix: call browser_story after sourcing lib.sh)" >&2; exit 1; }
  fi
  if grep -q '^# e2e: serial' "$t"; then serial+=("$t")
  elif grep -q 'npx playwright' "$t"; then browser+=("$t")
  else other+=("$t")
  fi
done
jobs="${E2E_JOBS:-$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)}"
E2E_LOGS="$(mktemp -d "${TMPDIR:-/tmp}/careerbrew-e2e-logs.XXXXXX")"
trap 'rm -rf "$E2E_LOGS"' EXIT
export E2E_LOGS

run_file() { # run_file <file>: run one story file; write <name>.log and <name>.result to $E2E_LOGS
  local log start r
  log="$E2E_LOGS/$(basename "$1" .sh)"; start=$(date +%s)
  if bash "$1" > "$log.log" 2>&1; then
    r=ok
  else
    local child_status=$?
    r=FAIL
    printf '%s\n' "$child_status" > "$log.status"
  fi
  echo "$r" > "$log.result"
  echo "e2e: $r $1 ($(( $(date +%s) - start )) s)"
}
export -f run_file

if [ $(( ${#browser[@]} + ${#other[@]} )) -gt 0 ]; then
  printf '%s\0' ${browser[@]+"${browser[@]}"} ${other[@]+"${other[@]}"} |
    xargs -0 -n 1 -P "$jobs" bash -c 'run_file "$1"' _ || true
fi
for t in ${serial[@]+"${serial[@]}"}; do run_file "$t"; done

if [ -n "$key" ] && [ "$(scripts/input-key.sh 2>/dev/null)" = "$key" ]; then
  mkdir -p .metrics
  saved="$(mktemp .metrics/e2e-results.XXXXXX)"
  sep=""
  { printf '{"key": "%s", "results": {' "$key"
    for t in "${tests[@]}"; do
      r=fail; [ "$(cat "$E2E_LOGS/$(basename "$t" .sh).result" 2>/dev/null)" = ok ] && r=ok
      printf '%s\n"%s": "%s"' "$sep" "$t" "$r"; sep=","
    done
    printf '\n}}\n'
  } > "$saved"
  mv "$saved" .metrics/e2e-results.json
fi

failed=()
for t in "${tests[@]}"; do
  log="$E2E_LOGS/$(basename "$t" .sh)"
  [ "$(cat "$log.result" 2>/dev/null)" = ok ] && continue
  failed+=("$t")
  echo "---- log of $t" >&2
  if [ -f "$log.status" ] && child_status=$(<"$log.status") && [[ "$child_status" =~ ^([1-9]|[1-9][0-9]|1[0-9][0-9]|2[0-4][0-9]|25[0-5])$ ]]; then
    printf 'story exit status: %s\n' "$child_status" >&2
  else
    echo "story exit status: unavailable" >&2
  fi
  cat "$log.log" >&2 2>/dev/null || echo "(it did not run)" >&2
done
if [ ${#failed[@]} -gt 0 ]; then
  echo "e2e: ${#failed[@]} of ${#tests[@]} file(s) failed: ${failed[*]}" >&2
  exit 1
fi
echo "e2e: ${#tests[@]} file(s) passed"
