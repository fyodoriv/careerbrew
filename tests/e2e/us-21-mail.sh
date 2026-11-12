#!/usr/bin/env bash
# US-21: read Outlook, Fastmail, and Proton mail metadata to catch interview emails (never bodies, never sends).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_G="e2e" CB_FM="e2e"
cat > "$CAREERBREW_HOME/mail.yaml" <<YAML
accounts:
  - {name: work, kind: graph, token: env:CB_G, machine: any}
  - {name: personal, kind: jmap, token: env:CB_FM}
  - {name: proton, kind: imap, host: 127.0.0.1, port: 1, user: env:CB_G, password: env:CB_G}
YAML
out="$(cb mail scan)"
expect "$out" "interview=1" "Outlook interview email classified"
expect "$out" "linkedin_alert=1" "Fastmail LinkedIn alert classified"
expect "$out" "account error proton" "error case: Proton Bridge not running is reported, not fatal"
[ -s "$CAREERBREW_HOME/jobs/inbox.jsonl" ] || fail "interview email saved for follow-up"
expect "$(cb machine set corporate 2>&1 || true)" "job-hunt data found" "a machine with job-hunt mail data cannot become corporate"
export CAREERBREW_HOME="$SCRATCH/corp"
cb init >/dev/null
cb machine set corporate >/dev/null
cp "$SCRATCH/data/mail.yaml" "$CAREERBREW_HOME/mail.yaml"
out="$(cb mail scan)"
expect "$out" "corporate machine: job-hunt mail is not processed here" "corporate machine skips job mail"
echo "ok US-21 mail"
