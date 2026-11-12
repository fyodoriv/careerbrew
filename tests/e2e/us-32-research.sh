#!/usr/bin/env bash
# US-32: fresh information: fetch a public page with a timestamp, see when it changes, list stale sources, see AI policies.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cp -R "$FIX/http" "$SCRATCH/http"; export CAREERBREW_HTTP_FIXTURES="$SCRATCH/http"
expect "$(cb research page https://careers.example.com/interview-process)" "unchanged or first fetch" "first fetch"
expect "$(cb research page https://careers.example.com/interview-process)" "unchanged" "same page"
echo "<html><body>New process: two technical rounds and no take-home.</body></html>" > "$SCRATCH/http/careers.example.com/interview-process.html"
expect "$(cb research page https://careers.example.com/interview-process)" "CHANGED since last fetch" "page change detected"
expect "$(cb research page http://careers.example.com/x 2>&1 || true)" "https" "error case: only https"
expect "$(cb research stale --days -1)" "older than -1 days" "stale listing runs"
expect "$(cb research stale)" "0 source(s) older than 180 days" "all sources fresh today"
expect "$(cb interview Datadog --show)" "AI tools in this process:" "AI policy shown with its date"
echo "ok US-32 research"
