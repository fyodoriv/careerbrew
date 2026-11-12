#!/usr/bin/env bash
# US-83: redo a real merged PR graded by its own tests, and walk a code tour that fades.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-26T09:00+00:00"
G=(git -c core.hooksPath=/dev/null -c commit.gpgsign=false -c user.name=T -c user.email=t@example.com)
repo="$SCRATCH/toyrepo"
mkdir -p "$repo/src/toy" "$repo/tests"
"${G[@]}" init -q -b main "$repo"
printf 'def add(a, b):\n    return a + b\n' >"$repo/src/toy/__init__.py"; printf '# Toy\n' >"$repo/README.md"
"${G[@]}" -C "$repo" add -A && "${G[@]}" -C "$repo" commit -qm "chore: start"
printf 'def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return add(n, n)\n' >"$repo/src/toy/__init__.py"
printf 'from toy import double\n\n\ndef test_double():\n    assert double(4) == 8\n' >"$repo/tests/test_double.py"
"${G[@]}" -C "$repo" add -A && "${G[@]}" -C "$repo" commit -qm "feat: double a number (#7)"
printf '# Toy\nHello.\n' >"$repo/README.md"; "${G[@]}" -C "$repo" commit -qam "docs: say hello (#8)"
cb learn start toy --repo "$repo" >/dev/null
expect "$(cb learn replay list toy)" "#7     1 test file(s)  feat: double a number" "small PRs listed with their tests"
expect "$(cb learn replay start toy 8 2>&1 || true)" "nothing can grade it" "error case: a PR without tests is refused"
expect "$(cb learn replay start toy 7)" "these tests now fail: tests/test_double.py" "task started at the PR's parent"
work="$CAREERBREW_HOME/learn/toy/replay/7"
if cb learn replay check toy 7 >"$SCRATCH/o" 2>&1; then fail "the task passed before the change"; fi
expect "$(cat "$SCRATCH/o")" "PR #7: fail" "error case: unchanged code fails the PR's tests"
printf 'def add(a, b):\n    return a + b\n\n\ndef double(n):\n    return 2 * n\n' >"$work/src/toy/__init__.py"
expect "$(cb learn replay check toy 7)" "PR #7: pass" "the change passes the PR's own tests"
[ -z "$(git -C "$repo" status --porcelain)" ] || fail "replay wrote into the learned repo"
expect "$(cb learn replay measure --repo "$repo")" "replay coverage: 1/2" "coverage counts only PRs whose tests fail before and pass after"
cat >"$SCRATCH/concepts.yaml" <<'YAML'
concepts:
  - {id: double, section: core, title: Doubling, cites: [repo:toyrepo/src/toy/__init__.py], questions: [{bloom: apply, prompt: "double(3)?", answer: "6"}]}
YAML
cb learn concepts toy "$SCRATCH/concepts.yaml" >/dev/null
printf '{"repo": "toyrepo", "title": "Doubling", "steps": [{"file": "src/toy/__init__.py", "line": 99, "description": "x"}]}' >"$SCRATCH/bad.json"
expect "$(cb learn tour add toy doubling "$SCRATCH/bad.json" 2>&1 || true)" "line must be 1 to" "error case: a tour line past the end is refused"
printf '{"repo": "toyrepo", "title": "Doubling", "steps": [{"file": "src/toy/__init__.py", "line": 5, "description": "double reuses add.", "concept": "double"}]}' >"$SCRATCH/tour.json"
expect "$(cb learn tour add toy doubling "$SCRATCH/tour.json" --consume)" "not yet approved" "tour added"
expect "$(cb learn tour export toy doubling)" "unreviewed, 0/1 step(s) faded" "an unapproved tour says so"
cb learn answer toy double --result correct --confidence high --level understand >/dev/null
cb learn answer toy double --result correct --confidence high --level apply >/dev/null
expect "$(cb learn tour approve toy doubling --by maintainer)" "approved at" "a maintainer approves it"
expect "$(cb learn tour export toy doubling)" "approved, 1/1 step(s) faded" "a mastered step fades to a prediction"
expect "$(cat "$CAREERBREW_HOME/out/tours/toy-doubling.tour")" "Predict before you read" "the CodeTour file asks to predict"
event_seen learn.replay
event_seen learn.tour
echo "ok US-83 learn replay and tour"
