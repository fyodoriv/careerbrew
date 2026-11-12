#!/usr/bin/env bash
# US-46: coding exercises in curriculum order, run locally with time and quality grading, synced with solved work.
source "$(dirname "$0")/lib.sh"
"$ROOT/.venv/bin/careerbrew" init >/dev/null
out="$(cb exercises list)"; expect "$out" "  1  arrays and hashing" "the curriculum starts with arrays and hashing"
expect "$(printf '%s\n' "$out" | tail -1)" "150  bit manipulation" "and ends with bit manipulation"
printf 'def contains_duplicate(nums):\n    for i in range(len(nums)):\n        for j in range(i + 1, len(nums)):\n            if nums[i] == nums[j]:\n                return True\n    return False\n' > "$SCRATCH/slow.py"
out="$(cb exercises submit contains-duplicate "$SCRATCH/slow.py" --seconds 300)"
expect "$out" "5/5 tests" "correct answers pass the tests"
expect "$out" "complexity O(n): too slow" "an O(n^2) answer is caught by the scaling check"
printf 'def contains_duplicate(nums):\n    return len(set(nums)) != len(nums)\n' > "$SCRATCH/good.py"
expect "$(cb exercises submit contains-duplicate "$SCRATCH/good.py" --seconds 120)" "score 1.0" "an optimal answer scores full marks"
out="$(cb exercises list)"; expect "$(printf '%s\n' "$out" | sed -n 1p)" "solved" "progress is tracked"
expect "$(cb exercises sync valid-anagram unknown-slug)" "marked solved: 1 exercise(s)" "work solved elsewhere is skipped"
event_seen exercise.submit
# the gate check over one exercise file: its reference solution passes its own tests
expect "$(cb exercises check "$ROOT/knowledge/exercises/001-contains-duplicate.yaml")" "exercises: 1 checked, 0 problem(s)" "a reference solution passes"
echo "ok US-46 exercises"
