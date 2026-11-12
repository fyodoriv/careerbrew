#!/usr/bin/env bash
# US-53: every action runs from the CLI, agents, and the web app; the PM update goes to Slack by default.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-16T16:00" CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_SLACK="synthetic"
cb init >/dev/null
# Agents: the catalog lists every command with its arguments.
out="$(cb actions --json)"
expect "$out" '"name": "day.send"' "agents see day.send"
expect "$out" '"terminal_only": "the secret goes to a hidden prompt' "terminal-only reason listed"
# Slack is the default; without a destination or connector, Send names the fix.
cb work note "feat: shipped the export" --tags delivery >/dev/null
expect "$(cb day send --yes 2>&1 || true)" "config set pm_slack" "error case: no destination"
cb config set pm_slack '#billing-pm' >/dev/null
expect "$(cb day send --yes 2>&1 || true)" "connect slack" "error case: Slack not connected"
cb connect slack --ref env:CB_SLACK --no-verify >/dev/null
expect "$(cb day send 2>&1 || true)" "add --yes" "error case: no send without --yes"
expect "$(cb day send --yes)" "sent to Slack #billing-pm" "posted to Slack"
event_seen day.pm.send
# Email stays an option.
cb config set pm_send_via email >/dev/null
cb config set pm_email pm@example.com >/dev/null
expect "$(cb day send)" "mailto:pm%40example.com" "email draft link"
# The same local synthetic facts ingest through either host's session shape.
cb config set feed_tool_sources 'mcp__mail__*=mail' >/dev/null
cat > "$SCRATCH/native.jsonl" <<'NATIVE'
{"type":"response_item","timestamp":"2026-10-16T15:59:00Z","payload":{"type":"function_call","name":"mcp__mail__search","call_id":"fixture","arguments":"{}"}}
{"type":"response_item","timestamp":"2026-10-16T15:59:00Z","payload":{"type":"function_call_output","call_id":"fixture","output":[{"type":"input_text","text":"{\"messages\":[{\"id\":\"host-fixture\",\"at\":\"2026-10-16T15:59:00Z\",\"title\":\"CANARY-HOST-FEED\"}]}"}]}}
NATIVE
out="$(cb work feed transcript "$SCRATCH/native.jsonl")"
expect "$out" 'stored 1 new event(s)' "native transcript stores facts"
refute "$out" 'CANARY-HOST-FEED' "native transcript output stays content-free"
cat > "$SCRATCH/claude.jsonl" <<'CLAUDE'
{"timestamp":"2026-10-16T15:59:00Z","message":{"content":[{"type":"tool_use","name":"mcp__mail__search","id":"fixture"}]}}
{"timestamp":"2026-10-16T15:59:00Z","message":{"content":[{"type":"tool_result","tool_use_id":"fixture","content":"{\"messages\":[{\"id\":\"host-fixture\",\"at\":\"2026-10-16T15:59:00Z\",\"title\":\"CANARY-HOST-FEED\"}]}"}]}}
CLAUDE
expect "$(cb work feed transcript "$SCRATCH/claude.jsonl")" '1 already stored' "Claude facts deduplicate native facts"
echo "ok US-53 actions and slack"
