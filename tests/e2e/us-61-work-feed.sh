#!/usr/bin/env bash
# US-61: one local work feed for Slack asks, PagerDuty pages, and Outlook mail and meetings:
# originals encrypted and apart from the analysis, triage by rule, personal events never in the PM update.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-21T16:00" CAREERBREW_NOTIFY=off CAREERBREW_HTTP_FIXTURES="$FIX/http/feed" CB_E2E_TOKEN=test-token-not-real
cb init >/dev/null
pm() { # the PM draft, read through the real CLI (the email route prints it as a mail link)
  cb day send --via email | head -1 | "$ROOT/.venv/bin/python" -c 'import sys, urllib.parse as u; print(u.unquote(sys.stdin.read().split("&body=", 1)[1]).strip())'
}
# 1. Error cases: an empty feed says how to start; a bad line or an unknown plugin writes nothing and names the fix.
expect "$(cb work feed show)" "no events yet" "an empty feed says how to start"
printf '{"id": "x", "title": "no time"}\n' > "$SCRATCH/bad.jsonl"
expect "$(cb work feed add "$SCRATCH/bad.jsonl" 2>&1 || true)" "line 1: missing at" "error case: a bad line is named with its fix"
expect "$(cb work feed add "$SCRATCH/bad.jsonl" --plugin nope 2>&1 || true)" "not installed" "error case: an unknown plugin is refused"
data_write work/feed-rules.yaml <<< 'rules: [{when: {chanel: x}, triage: noise}]'
expect "$(cb work feed rules 2>&1 || true)" "unknown key chanel" "error case: a bad rule names the fix"
# 2. Connect the three tools (recorded fixtures, no real account) and pull them into the feed.
for k in slack pagerduty outlook; do cb connect "$k" --ref env:CB_E2E_TOKEN --no-verify >/dev/null; done
data_write work/feed-rules.yaml <<'YAML'
rules:
  - {name: family, when: {text: [dentist]}, personal: true}
  - {name: alerts, when: {source: slack, channel: 'alerts-*'}, triage: noise}
YAML
expect "$(cb work feed pull)" "pulled 3 source(s): 16 new, 0 updated, 0 error(s)" "three tools write into one store"
show="$(cb work feed show)"
expect "$show" "act=6, fyi=1, noise=2, mine=5; open asks=3" "triage by rule, open asks counted"
expect "$show" "personal=2" "the private meeting and the dentist message are personal"
expect "$(cb work feed rules)" "yours     family" "your rules run first"
# 3. Originals are ciphertext on disk, apart from the analysis, which keeps no message text.
grep -q "PROJECTZEBRA" "$CAREERBREW_HOME/work/feed/originals.jsonl" && fail "original stored in plaintext"
expect "$(data_cat work/feed/originals.jsonl)" "PROJECTZEBRA" "the original is kept (encrypted) for re-triage"
refute "$(data_cat work/feed/events.jsonl)" "PROJECTZEBRA" "the analysis keeps no message text"
refute "$(cb work feed show --show)" "PROJECTZEBRA" "titles stay off a piped stdout"
# 4. A day: the watcher turns your handled page into evidence; personal events stay out of the PM update.
cb day watch once >/dev/null
body="$(pm)"
expect "$(printf '%s' "$body" | tr 'A-Z' 'a-z')" "acknowledged page: checkout latency over slo (1 incident)" "your own page handling reaches the PM update"
expect "$body" "led design review" "the meeting you organized is named (issue #239)"
expect "$body" "1 meeting," "the other meeting is counted; the private one is not"
refute "$body" "Dentist" "a private calendar entry never reaches the PM update"
expect "$(data_cat day/log-2026-10-21.md)" "Design review" "the day log is written"
refute "$(data_cat day/log-2026-10-21.md)" "Dentist" "nor the day log"
# 5. Mark an event personal: it leaves evidence and the PM update, and stays out.
expect "$(cb work feed mark pagerduty:R1ACK personal)" "1 evidence row(s) removed" "marking drops its evidence"
refute "$(pm | tr 'A-Z' 'a-z')" "checkout latency" "a marked event leaves the PM update"
cb work collect >/dev/null
refute "$(pm | tr 'A-Z' 'a-z')" "checkout latency" "the owner's mark sticks"
event_seen work.feed.pull
event_seen work.feed.mark
grep -q "PROJECTZEBRA" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
# re-run the rules over kept originals; an original never prints to a piped stdout
expect "$(cb work feed retriage)" "triaged " "retriage runs the rules again"
expect "$(cb work feed open pagerduty:R1ACK)" "hidden: stdout is not a terminal" "an original stays off a piped stdout"
# 6. An agent's live feed (issue #234): store the tool replies from its session transcript; a silent source fails.
expect "$(cb work feed transcript "$SCRATCH/none.jsonl" 2>&1 || true)" "no tool map" "error case: no tool map names the fix"
cb config set feed_tool_sources 'mcp__chat__*=chat, mcp__tickets__*=tickets, mcp__mail__*=mail' >/dev/null
mkdir -p "$SCRATCH/session" && cp -R "$FIX/transcripts/feed-cycle" "$SCRATCH/session/"
sed "s#@DIR@#$SCRATCH/session#g" "$FIX/transcripts/feed-cycle.jsonl" > "$SCRATCH/session/feed-cycle.jsonl"
set +e; out="$(cb work feed transcript "$SCRATCH/session/feed-cycle.jsonl")"; rc=$?; set -e
[ "$rc" = 1 ] || fail "a source that stored nothing must fail the cycle (rc=$rc)"
expect "$out" "stored 4 new event(s), 0 updated, 0 already stored, 1 skipped" "chat and the large saved ticket reply are stored"
expect "$out" "gap  mail: nothing stored this cycle" "the failed mail search is a gap"
refute "$out" "CANARYCHAT" "no reply text reaches the agent"
expect "$(cb work feed transcript "$SCRATCH/session/feed-cycle.jsonl" || true)" "0 new event(s), 0 updated, 4 already stored" "a rerun stores nothing twice"
grep -q "CANARYTICKET" "$CAREERBREW_HOME/work/feed/originals.jsonl" && fail "a tool reply stored in plaintext"
event_seen work.feed.transcript
# 7. A live-feed cycle (issue #254): a source pulled but never stored fails the cycle; a raw search reply stores as-is.
cb work feed cycle >/dev/null || true  # start a clean cycle
expect "$(cb work feed cycle --require 'Bad Name' 2>&1 || true)" "short lowercase name" "error case: a bad source name names the fix"
cat > "$SCRATCH/jira.json" <<'JSON'
{"total": 3, "tickets": [
  {"key": "PROJ-41", "summary": "Fix the export", "status": "In Progress", "updated": "2026-10-21T14:00:00Z", "description": "CANARYRAW"},
  {"key": "PROJ-42", "fields": {"summary": "Write the docs", "status": {"name": "To Do"}, "updated": "2026-10-21T14:10:00Z"}},
  {"key": "PROJ-43", "summary": "no time"}]}
JSON
expect "$(cb work feed add "$SCRATCH/jira.json" --source jira)" "added 2 event(s), updated 0, 1 skipped" "a raw ticket search reply stores one event per ticket"
expect "$(data_cat work/feed/events.jsonl)" "jira:PROJ-41:2026-10-21T14:00:00Z:In Progress" "the id is key + updated + status"
refute "$(data_cat work/feed/events.jsonl)" "CANARYRAW" "the ticket's text stays in the encrypted original"
set +e; out="$(cb work feed cycle --require chat,jira,mail)"; rc=$?; set -e
[ "$rc" = 1 ] || fail "a required source that stored nothing must fail the cycle (rc=$rc)"
expect "$out" "stored 1 of 4 required source(s)" "only jira was stored this cycle"
expect "$out" "not stored  chat:" "chat is named"
expect "$out" "not stored  mail:" "mail is named"
echo '{"messages": []}' > "$SCRATCH/mail.json"
cb work feed add "$SCRATCH/mail.json" >/dev/null
cb work feed add "$SCRATCH/jira.json" --source jira >/dev/null
printf '{"messages": []}\n' > "$SCRATCH/quiet-chat.json"
cb work feed add "$SCRATCH/quiet-chat.json" --source chat >/dev/null
cb work feed add "$SCRATCH/jira.json" --source tickets >/dev/null
expect "$(cb work feed cycle --require jira,mail --json)" '"not_stored": []' "a quiet mail reply and a rerun of stored tickets both count as stored"
event_seen work.feed.cycle
# #406: declared direct completion certifies coverage without starting adapters.
expect "$(cb work live status --json)" '"status": "healthy"' "complete declared direct coverage is healthy"
expect "$(cb work live status --json)" '"running": false' "direct coverage leaves adapters stopped"
browser_story
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW -u CAREERBREW_HTTP_FIXTURES npx playwright test live-source-health.spec.ts --reporter=line 2>&1)" || { echo "$out"; fail "source health notice and actual expiry"; }
printf '%s\n' "$out"
echo "ok US-61 work feed"
