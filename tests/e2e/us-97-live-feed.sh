#!/usr/bin/env bash
# US-97: the live feed runs end to end in core with a fixture source config and no plugin (issue #267):
# start, one cycle (pull, store, triage, verdict per event, actions, mark), status, stop; a required source
# without healthy acquisition fails the cycle; a plugin supplies only its source config and its roster.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-25T16:00" CAREERBREW_NOTIFY=off
cb init >/dev/null
# Each synthetic save has acquisition metadata in the same clock domain as the story.
stamp_reply() {
  "$ROOT/.venv/bin/python" - "$@" <<'PYTHON'
import os
import sys
from careerbrew.calendar import now_utc
at = now_utc().timestamp()
for path in sys.argv[1:]:
    os.utime(path, (at, at))
PYTHON
}
# 1. Error cases: no sources names the fix; a bad source config saves nothing; a cycle before start is refused.
expect "$(cb work live start 2>&1 || true)" "no live sources" "error case: no sources names the fix"
printf 'sources: [{name: chat, kind: pager, adapter: file, inbox: x}]\n' > "$SCRATCH/bad.yaml"
expect "$(cb work live start --sources "$SCRATCH/bad.yaml" 2>&1 || true)" "kind must be one of" "error case: a bad kind names the allowed kinds"
expect "$(cb work live cycle 2>&1 || true)" "the live feed is stopped" "error case: a cycle before start is refused"
# 2. Start with the fixture config (no plugin installed): each source and its cursor.
mkdir -p "$SCRATCH/inbox"
sed "s#@DIR@#$SCRATCH/inbox#g" "$FIX/live/sources.yaml" > "$SCRATCH/sources.yaml"
out="$(cb work live start --sources "$SCRATCH/sources.yaml")"
expect "$out" "live feed running: 3 source(s), 2 required" "start lists the sources"
refute "$out" "team-alpha" "pull channels stay off a piped stdout"
expect "$out" "pull hosts and channels hidden" "and it says so"
# 3. A cycle with only chat pulled: stored and triaged, but the required tickets source stored nothing.
cp "$FIX/live/chat.json" "$SCRATCH/inbox/chat.json"
stamp_reply "$SCRATCH/inbox/chat.json"
set +e; out="$(cb work live cycle)"; rc=$?; set -e
[ "$rc" = 1 ] || fail "a required source that stored nothing must fail the cycle (rc=$rc)"
expect "$out" "cycle 1 failed: 3 event(s) stored" "the cycle stores the chat reply as returned"
expect "$out" "not_stored  tickets" "not_stored names the silent required source"
refute "$out" "not_stored  mail" "an optional source is never not_stored"
expect "$out" "answer=1" "the ask to you gets the answer verdict"
expect "$out" "skip=1" "the bot's message is skipped"
expect "$out" "1 skipped" "a row with no id, time, or title is skipped"
expect "$out" "pull tickets again" "the next action re-pulls the gap"
# 4. Validated unchanged chat is healthy without new intake; tickets and mail arrive.
data_write work/feed-rules.yaml <<< 'rules: [{name: shipped, when: {source: tickets, actor: me}, evidence: true}]'
cp "$FIX/live/tickets.json" "$SCRATCH/inbox/tickets.json"
cp "$FIX/live/mail.json" "$SCRATCH/inbox/mail.json"
stamp_reply "$SCRATCH/inbox/tickets.json" "$SCRATCH/inbox/mail.json"
set +e; out="$(cb work live cycle)"; rc=$?; set -e
[ "$rc" = 0 ] || fail "validated unchanged chat stays healthy within its acquisition limit (rc=$rc)"
expect "$out" "healthy; unchanged" "unchanged healthy is not a new pull"
refute "$out" "not_stored  chat" "fresh unchanged chat is not a gap"
expect "$out" "evidence=1" "your shipped ticket is evidence under the PM draft rule"
expect "$out" "1 item(s) reach the PM draft" "and the next action says so"
printf ' ' >> "$SCRATCH/inbox/chat.json"  # the agent saved a new reply (same events)
printf ' ' >> "$SCRATCH/inbox/tickets.json"
stamp_reply "$SCRATCH/inbox/chat.json" "$SCRATCH/inbox/tickets.json"
out="$(cb work live cycle)" || fail "a cycle where every required source stored must pass: $out"
expect "$out" "cycle 3 ok" "the cycle passes"
expect "$out" "chat       3 item(s), 0 new" "a re-pull of the same events stores nothing twice"
# 5. Status: running, the last cycle, each source's cursor; JSON for agents.
st="$(cb work live status)"
expect "$st" "live feed: running; 3 cycle(s); last 3 ok" "status shows the last cycle"
expect "$st" "tickets    tickets  required  file; since 2026-10-21T14:20:00Z" "the tickets cursor is the newest event stored"
expect "$(cb work live status --json)" "\"running\": true" "status --json for agents"
expect "$(cb work live status --json)" '"status": "healthy"' "status includes current acquisition health"
# 6. The feed holds the events (analysis without text); the PM rule lets only your own evidence through.
show="$(cb work feed show)"
expect "$show" "4 event(s)" "every fresh event reached the feed"
refute "$(data_cat work/feed/events.jsonl)" "LIVEZEBRA" "the analysis keeps no message text"
refute "$(data_cat work/live/cycles.jsonl)" "LIVEZEBRA" "nor the cycle record"
grep -q "LIVEZEBRA" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
# 7. A plugin supplies only its source config and its roster; the owner's source of the same name wins.
mkdir -p "$SCRATCH/plug/live"
cat > "$SCRATCH/plug/careerbrew-plugin.yaml" <<'YAML'
name: acme-live
version: 0.1.0
live: {sources: live/sources.yaml, team: live/team.yaml}
YAML
printf 'sources:\n  - {name: paging, kind: paging, required: false, adapter: file, inbox: %s/inbox/paging.json}\n  - {name: chat, kind: chat, adapter: file, inbox: /nowhere.json}\n' "$SCRATCH" > "$SCRATCH/plug/live/sources.yaml"
printf 'team:\n  - {name: Sam Lee, chat: U0SAM}\n' > "$SCRATCH/plug/live/team.yaml"
cb plugin add "$SCRATCH/plug" >/dev/null
st="$(cb work live status)"
expect "$st" "paging     paging   optional  file" "the plugin's source joins"
expect "$st" "(plugin:acme-live)" "and names its plugin"
expect "$st" "chat       chat     required  file" "the owner's chat source wins over the plugin's"
printf '[{"id": "P1", "at": "2026-10-25T15:58:00Z", "title": "paged: disk full on db-1", "actor": "bot", "actor_id": "U0SAM"}]' > "$SCRATCH/inbox/paging.json"
stamp_reply "$SCRATCH/inbox/paging.json"
cb work live cycle >/dev/null || true
expect "$(data_cat work/feed/events.jsonl)" "Sam Lee" "the plugin's roster marks the teammate's page"
# 8. An unchanged reply beyond the acquisition limit fails without renewal.
export CAREERBREW_NOW="2026-10-25T16:16"
set +e; out="$(cb work live cycle)"; rc=$?; set -e
[ "$rc" = 1 ] || fail "required acquisitions expire even while polling (rc=$rc)"
expect "$out" "stale; unchanged" "polling does not renew acquisition"
expect "$out" "not_stored  chat" "expired chat requires a real pull"
expect "$out" "not_stored  tickets" "expired tickets require a real pull"
# 9. Stop keeps the cursors; a cycle after stop is refused.
expect "$(cb work live stop)" "live feed stopped after 5 cycle(s)" "stop reports the cycles"
expect "$(cb work live cycle 2>&1 || true)" "the live feed is stopped" "a stopped feed runs no cycle"
expect "$(cb work live status)" "since 2026-10-21T14:20:00Z" "cursors are kept after stop"
event_seen work.live.start
event_seen work.live.cycle
event_seen work.live.stop
echo "PASS us-97"
