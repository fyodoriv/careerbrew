#!/usr/bin/env bash
# US-34: every careerbrew skill starts from best-practices and ends by iterating on itself.
source "$(dirname "$0")/lib.sh"
out="$(cd "$ROOT" && evals/run.sh 2>&1)" || { code=$?; printf '%s\n' "$out" >&2; exit "$code"; }
expect "$out" "0 failure(s)" "evals enforce best-practices first and Iterate last"
for f in "$ROOT"/skills/*/SKILL.md; do
  name="$(basename "$(dirname "$f")")"
  [ -f "$ROOT/skills/$name/LESSONS.md" ] || fail "$name has a LESSONS.md"
  grep -q "^## Iterate" "$f" || fail "$name ends with Iterate"
done
echo "ok US-34 skill practice"
