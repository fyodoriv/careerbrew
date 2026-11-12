#!/usr/bin/env bash
# Docs drift check: fails when docs and code disagree.
# 1. Every docs/*.md file has a row in docs/README.md.
# 2. Every story test (tests/e2e/us-*) named in docs/user-stories.md exists, and each one is named there.
# 3. Each story test names its story id inside (tests/e2e/us-01-x.sh says US-01), so a renamed
#    or moved test cannot drift silently.
#    Helpers and fixtures in tests/e2e/ without the us- prefix are ignored.
set -euo pipefail
cd "$(dirname "$0")/.."

fail=0
for f in docs/*.md; do
  base="$(basename "$f")"
  [ "$base" = README.md ] && continue
  grep -q "($base)" docs/README.md || { echo "check-docs: $f has no row in docs/README.md"; fail=1; }
done

for t in $(grep -oE 'tests/e2e/us-[A-Za-z0-9_./-]*[A-Za-z0-9_]' docs/user-stories.md | sort -u); do
  [ -e "$t" ] || { echo "check-docs: docs/user-stories.md names missing test $t"; fail=1; }
done
shopt -s nullglob
for t in tests/e2e/us-*; do
  [ -f "$t" ] || continue
  grep -q "$t" docs/user-stories.md || { echo "check-docs: $t is not listed in docs/user-stories.md"; fail=1; }
  id="$(basename "$t" | sed -nE 's/^us-([0-9]+).*/US-\1/p')"
  [ -z "$id" ] || grep -q "$id" "$t" || { echo "check-docs: $t does not name its story $id"; fail=1; }
done

[ "$fail" = 0 ] && echo "check-docs: docs match the code"
exit "$fail"
