#!/usr/bin/env bash
# US-78: a team roster keeps teammates' events in the work feed as worth a look, and every kept link gets researched (issue #180).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-10-21T16:00"
# Error cases first: a bad roster entry and an unknown teammate name their fix.
expect "$(cb work team add '  ' 2>&1 || true)" "needs a name" "error case: a teammate needs a name"
expect "$(cb work team remove nobody 2>&1 || true)" "is not on your team" "error case: an unknown teammate"
expect "$(cb work team list)" "no teammates yet" "an empty roster names the add command"
data_write work/feed-rules.yaml <<< 'rules: [{name: chatter, when: {source: slack, kind: message}, triage: noise}]'
cat > "$SCRATCH/feed.jsonl" <<'JSONL'
{"id": "slack:C1:1", "source": "slack", "kind": "message", "at": "2026-10-21T14:00:00Z", "title": "a teammate in #eng", "actor": "other", "actor_id": "U0SAM", "text": "spec is ready <https://docs.example.com/d/1|spec> and https://sso.example.com/app"}
{"id": "slack:C1:2", "source": "slack", "kind": "message", "at": "2026-10-21T14:05:00Z", "title": "someone in #eng", "actor": "other", "actor_id": "U0ANA", "text": "lunch?"}
JSONL
cb work feed add "$SCRATCH/feed.jsonl" >/dev/null
expect "$(cb work feed show)" "noise=2" "before the roster both are chatter"
expect "$(cb work team add 'Sam Lee' --chat U0SAM --login samlee)" "1 changed" "adding a teammate retriages the feed"
out="$(cb work feed show)"
expect "$out" "fyi=1" "the teammate's message is worth a look"
expect "$out" "noise=1" "the other message is still chatter"
refute "$(cb work team list)" "Sam Lee" "an agent never sees a teammate's name"
expect "$(cb work team list --json)" '"chat": true' "an agent sees which ids are set"
# Every link in a kept event is researched, or the research is refused.
expect "$(cb work feed links --pending)" "2 link(s) not researched yet" "two links to research"
printf '[{"url": "https://docs.example.com/d/1", "summary": "the export spec, signed off"}]' > "$SCRATCH/half.json"
out="$(cb work feed research slack:C1:1 "$SCRATCH/half.json" 2>&1 || true)"
expect "$out" "refused: 1 link(s) not researched (link 2 of 2" "error case: research must cover every link"
refute "$out" "sso.example.com" "error case: a refusal never prints a link to an agent"
printf '[{"url": "https://docs.example.com/d/1", "summary": "the export spec, signed off"}, {"url": "https://sso.example.com/app", "opened": false, "reason": "sign-in page"}]' > "$SCRATCH/all.json"
expect "$(cb work feed research slack:C1:1 "$SCRATCH/all.json")" "2 link(s) researched (1 opened, 1 not opened" "research every link"
expect "$(cb work feed links --pending)" "no kept event with links to research" "nothing left to research"
expect "$(cb work team feed)" "1 teammate event(s) in the last 7 day(s): 0 match a project" "the team feed counts teammate events by project"
refute "$(cb work team feed)" "export spec" "an agent never sees a teammate event's title"
refute "$(cb work team remove nobody 2>&1 || true)" "Sam Lee" "error case: an unknown name never lists the roster"
expect "$(cb work team remove 'sam lee')" "1 changed" "removing a teammate retriages the feed"
expect "$(cb work feed show)" "noise=2" "back to chatter"
event_seen work.team
event_seen work.feed.research
echo "ok US-78 team feed"
