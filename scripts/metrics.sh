#!/usr/bin/env bash
# Report the repo's own speed from .metrics/*.jsonl (written by scripts/check.sh
# and scripts/e2e.sh). Use it in the weekly review (RECURRING.md) to find the
# slowest step and make it faster.
set -euo pipefail
cd "$(dirname "$0")/.."

# proof: measure the README's claims (docs/proof/metrics.json, a history per metric)
# and rewrite the README Proof section from it. Run it after a green scripts/check.sh,
# so the e2e results are fresh. PROOF_EVENTS adds more events files (colon-separated).
# PROOF_FIELD=1 also copies this machine's real-use totals (for your own copy; the committed file holds none).
if [ "${1:-}" = proof ]; then
  uv run --quiet python scripts/proof.py collect
  uv run --quiet python scripts/proof.py render
  exec uv run --quiet python scripts/proof.py check
fi

# capability-coverage: run the e2e test named for each capability in docs/metrics.md
# and print passing/total. The build metric for the goal contract.
# Run each distinct story once in this report. Saved passes do not bind the
# external runtime inputs a story may read, so they cannot establish a fresh pass.
if [ "${1:-}" = capability-coverage ]; then
  rows="$(grep -E '^\| [0-9]+ \|' docs/metrics.md)"
  total="$(printf '%s\n' "$rows" | wc -l | tr -d ' ')"
  pass=0 ran=0
  results=""  # "<story file> ok|fail" lines for the story files counted here
  has() { case $'\n'"$1"$'\n' in *$'\n'"$2"$'\n'*) return 0 ;; esac; return 1; }
  while IFS= read -r row; do
    n="$(printf '%s' "$row" | awk -F'|' '{gsub(/ /,"",$2); print $2}')"
    t="$(printf '%s' "$row" | grep -oE 'tests/e2e/us-[A-Za-z0-9_.-]+\.sh')"
    if has "$results" "$t ok"; then ok=1
    elif has "$results" "$t fail"; then ok=0
    else
      ran=$((ran + 1))
      if bash "$t" >/dev/null 2>&1; then ok=1; results="$results"$'\n'"$t ok"
      else ok=0; results="$results"$'\n'"$t fail"; fi
    fi
    if [ "$ok" = 1 ]; then pass=$((pass + 1)); echo "ok    capability $n ($t)"; else echo "FAIL  capability $n ($t)"; fi
  done <<< "$rows"
  echo "capability-coverage: ran $ran distinct story file(s)"
  echo "capability-coverage: $pass/$total"
  [ "$pass" = "$total" ]
  exit $?
fi

shopt -s nullglob
files=(.metrics/*.jsonl)
if [ ${#files[@]} -eq 0 ]; then echo "metrics: no data yet; run scripts/check.sh"; exit 0; fi

# Each line: {"ts":"...","event":"step","name":"test","seconds":12,"ok":true}
printf '%-14s %6s %8s %8s %6s\n' step runs avg_s max_s fails
cat "${files[@]}" | awk -F'"' '
  { name=""; secs=""; okv="";
    for (i = 1; i < NF; i++) {
      if ($i == "name") name = $(i + 2);
      if ($i == "seconds") { s = $(i + 1); gsub(/[^0-9.]/, "", s); secs = s + 0 }  # a number: as text "97.0" > "151.9"
      if ($i == "ok") { o = $(i + 1); okv = (o ~ /true/) }
    }
    if (name == "") next;
    n[name]++; sum[name] += secs; if (secs > max[name]) max[name] = secs; if (!okv) bad[name]++
  }
  END {
    for (k in n) printf "%-14s %6d %8.1f %8.1f %6d\n", k, n[k], sum[k] / n[k], max[k], bad[k];
  }' | sort -k3 -nr
