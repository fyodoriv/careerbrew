#!/usr/bin/env bash
# US-69: push one project forward: `work project check <name> --since` names what changed since each source's last check, stamping nothing.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_T="synthetic" CAREERBREW_NOW="2026-10-05T09:00+00:00"
cb init >/dev/null
for k in github gdocs slack; do cb connect "$k" --ref env:CB_T --user me --no-verify >/dev/null; done
cb connect jira --ref env:CB_T --base-url https://jira.example.com --email me@example.com --no-verify >/dev/null
# Error cases first: an unknown project names its fix, and --since never takes --status.
expect "$(cb work project check nope --since 2>&1 || true)" "careerbrew work project add nope" "error case: unknown project names the fix"
cb work project add billing --memory work/billing-notes.md >/dev/null
expect "$(cb work project check billing --since 2>&1 || true)" "never read memory   work/billing-notes.md" "a never-checked source must be read"
expect "$(cb work project check billing --since --status x 2>&1 || true)" "stamps nothing" "error case: --since with --status is refused"
# A fixture project: a repo and a doc that changed on 2026-10-06, a file last edited 2026-10-01, and a sign-in link.
printf '# Kickoff\nDecision: freeze the schema\n' > "$SCRATCH/kickoff.md"
touch -t 202610010900 "$SCRATCH/kickoff.md"
source_file="$(cd "$SCRATCH" && pwd -P)/kickoff.md"
cb work project source billing repo acme/billing >/dev/null
cb work project source billing doc doc-prd-1 >/dev/null
cb work project source billing doc "$SCRATCH/kickoff.md" >/dev/null
cb work project source billing url https://outlook.office.com/mail/inbox/id/AAQkSYNTH >/dev/null
for ref in acme/billing doc-prd-1 "$SCRATCH/kickoff.md"; do cb work project check billing "$ref" >/dev/null; done
# Business reading is off (no terminal, no plugin policy): local files are still compared; remote ones are unknown.
out="$(cb work project check billing --since)"
expect "$out" "unchanged  doc      $source_file" "reading off: a local file is still compared"
expect "$out" "unknown    repo     acme/billing  why: reading business data is off" "reading off: a remote source is unknown, with the reason"
expect "$out" "reference  url      https://outlook.office.com" "a reference is skipped"
# The owner allows agents to read business data through a plugin policy.
mkdir -p "$SCRATCH/p" && printf 'name: allow\nversion: 0.1.0\npolicy: {agent_may_read_business: true}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
export CAREERBREW_NOW="2026-10-07T09:00+00:00"
out="$(cb work project check billing --since)"
expect "$out" "changed    repo     acme/billing  1 change(s) since 2026-10-05 09:00 UTC" "a repo with a PR updated since the check"
expect "$out" "changed    doc      doc-prd-1  2 change(s)" "a doc edited and commented since the check"
expect "$out" "unchanged  doc      $source_file" "a file not edited since the check"
expect "$out" "read these 3 of 4" "only the changed and never-read sources need a read"
# Nothing was stamped: the sources keep their 2026-10-05 check.
expect "$(cb work project show billing)" "repo     acme/billing  2d ago" "--since stamps nothing"
# Read the repo, stamp it, and it is unchanged on the next pass.
cb work project check billing acme/billing --status "PR 42 open" >/dev/null
expect "$(cb work project check billing acme/billing --since)" "unchanged  repo     acme/billing" "a source read after its last change is unchanged"
# A chat source: the fixture's reply is at 05:20 UTC on 2025-09-16. Checked earlier that day, it counts; checked after it, it does not.
export CAREERBREW_NOW="2025-09-16T01:00+00:00"
cb work project source billing chat '#billing-eng' >/dev/null
cb work project check billing '#billing-eng' >/dev/null
expect "$(cb work project check billing '#billing-eng' --since)" "changed    chat     #billing-eng  1 change(s) since 2025-09-16 01:00 UTC" "a chat reply later on the day of the check is a change"
export CAREERBREW_NOW="2025-09-16T06:00+00:00"
cb work project check billing '#billing-eng' >/dev/null
expect "$(cb work project check billing '#billing-eng' --since)" "unchanged  chat     #billing-eng" "a chat read after its last reply is unchanged"
event_seen work.project
echo "ok US-69 project push"
