#!/usr/bin/env bash
# US-44: cards get full explanations through a checked publish, personal ones stay private, and the deck reads as a Library study guide.
source "$(dirname "$0")/lib.sh"
"$ROOT/.venv/bin/careerbrew" init >/dev/null
cb cards add "What is a queue?" "fifo" --tags "Computer Science" >/dev/null
cb cards add "What did I ship at my last job?" "my story" --tags Behavioural >/dev/null
out="$(cb cards explain-queue)"; expect "$out" "explain queue: 2 card(s)" "both cards are queued"
json="$(cb cards explain-queue --json)"; refute "$json" "my story" "a behavioural answer never leaves"
cp "$FIX/public-cards/out.yaml" "$SCRATCH/out.yaml"
printf -- '- source_id: %s\n  skip: true\n  reason: personal\n' "$(printf '%s' "$json" | "$ROOT/.venv/bin/python" -c 'import json,sys; print(next(i["source_id"] for i in json.load(sys.stdin) if "ship" in i["question"]))')" >> "$SCRATCH/out.yaml"
out="$(cb cards publish "$SCRATCH/out.yaml")"; expect "$out" "published 1 card(s), skipped 1 as personal" "publish"
[ ! -e "$SCRATCH/out.yaml" ] || fail "the generated file is removed after publish"
out="$(cb cards explain-queue)"; expect "$out" "personal: explain privately: 1" "the personal question comes back for a private explanation"
"$ROOT/.venv/bin/python" - "$FIX/public-cards/out.yaml" "$SCRATCH/private.yaml" "$(printf '%s' "$json" | "$ROOT/.venv/bin/python" -c 'import json,sys; print(next(i["source_id"] for i in json.load(sys.stdin) if "ship" in i["question"]))')" <<'PY'
import sys, yaml
[row] = yaml.safe_load(open(sys.argv[1]))
row.update(source_id=sys.argv[3], question="What did I ship at my last job?", private=True)
open(sys.argv[2], "w").write(yaml.safe_dump([row], sort_keys=False))
PY
out="$(cb cards publish "$SCRATCH/private.yaml")"; expect "$out" "1 private explanation(s) saved" "private explanation saved"
if grep -rq "What did I ship" "$CAREERBREW_PUBLIC_CARDS"; then fail "a personal question reached the public deck"; fi
out="$(cb cards explain-queue)"; expect "$out" "explain queue: 0 card(s)" "nothing left to write"
out="$(cb cards public-check)"; expect "$out" "0 problem(s)" "the deck meets the bar"
event_seen cards.publish
printf -- '- source_id: x\n  question: What is a stack?\n  answer: Last in, first out collection of items.\n  topic: computer science\n  explanation: too short\n' > "$SCRATCH/bad.yaml"
out="$(cb cards publish "$SCRATCH/bad.yaml" || true)"; expect "$out" "not published" "a card below the bar is refused"
echo "ok US-44 public study deck"
