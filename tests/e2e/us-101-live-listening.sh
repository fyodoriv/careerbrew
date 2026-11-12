#!/usr/bin/env bash
# US-101: proactive listening all day until wrap-up (issue #266): every live cycle does the follow-up work for
# each new event, as drafts only: a reply draft per ask, research or a reason per link, review notes per review
# asked of you, the event attached to its project, and one update line per goal for today. Nothing is sent.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-25T16:00" CAREERBREW_NOTIFY=off
cb init >/dev/null
# 1. Error case: no draft saved yet for a target names the fix.
expect "$(cb draft show chat:a1 2>&1 || true)" "no draft saved for chat:a1" "error case: draft show names the fix"
# 2. A project the review link belongs to, and two goals for today.
cb work project add shop-api >/dev/null
cb work project source shop-api repo example-shop/api >/dev/null
data_write goals.yaml <<'YAML'
long_term: [{id: reviewer, text: Grow as a code reviewer, by: null}]
queue: [{id: api-rollout, kind: custom, title: Ship the API rollout, date: "2026-11-20", topics: [rollout], minutes_per_day: 30}]
history: []
YAML
# 3. Start the feed and run one cycle with the synthetic chat reply.
mkdir -p "$SCRATCH/inbox"
sed "s#@DIR@#$SCRATCH/inbox#g" "$FIX/live/listen/sources.yaml" > "$SCRATCH/sources.yaml"
cb work live start --sources "$SCRATCH/sources.yaml" >/dev/null
cp "$FIX/live/listen/chat.json" "$SCRATCH/inbox/chat.json"
"$ROOT/.venv/bin/python" - "$SCRATCH/inbox/chat.json" <<'PYTHON'
import os
import sys
from careerbrew.calendar import now_utc
at = now_utc().timestamp()
os.utime(sys.argv[1], (at, at))
PYTHON
out="$(cb work live cycle)" || fail "the cycle must pass: $out"
expect "$out" "cycle 1 ok: 6 event(s) stored" "the cycle stores the reply"
expect "$out" "needs you     answer   chat: reply draft saved: chat:a1" "a direct ask gets a reply draft"
expect "$out" "review notes drafted: review:" "a review asked of you gets review notes"
expect "$out" "project shop-api updated" "the review is attached to its project"
expect "$out" "noise         skip     chat: nothing: noise" "noise gets no work"
expect "$out" "1 link(s): 0 summarized, 1 with a reason" "a link gets a reason when it cannot be read here"
expect "$out" "goal api-rollout: 1 moved (your actions today), 2 other update(s); waits: 1 on you, 1 on others" "one line for the rollout goal"
expect "$out" "goal reviewer: nothing moved today; nothing waits" "and one for the goal nothing touched"
expect "$out" "saved, never sent" "the next action says drafts are never sent"
refute "$out" "LISTENZEBRA" "no message text on a piped stdout"
refute "$out" "Dee Moss" "nor the names a goal waits on"
# 4. The draft is in the local store; an agent sees its size, never its text.
show="$(cb draft show chat:a1)"
expect "$show" "never sent (text hidden" "draft show hides the text from an agent"
grep -q LISTENZEBRA "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
refute "$(data_cat work/live/cycles.jsonl)" "LISTENZEBRA" "the cycle record holds no message text"
expect "$(cb work feed links --pending)" "link(s) not researched yet" "the cycle's reasons stay pending for the agent"
# 5. The same events again are no new events: no work is done twice.
printf ' ' >> "$SCRATCH/inbox/chat.json"
"$ROOT/.venv/bin/python" - "$SCRATCH/inbox/chat.json" <<'PYTHON'
import os
import sys
from careerbrew.calendar import now_utc
at = now_utc().timestamp()
os.utime(sys.argv[1], (at, at))
PYTHON
out="$(cb work live cycle)" || fail "the second cycle must pass: $out"
refute "$out" "reply draft saved" "an event is followed up once"
# 6. Wrap-up: stop ends the listening.
expect "$(cb work live stop)" "live feed stopped after 2 cycle(s)" "stop at wrap-up"
event_seen work.live.listen
echo "PASS us-101"
