#!/usr/bin/env bash
# US-93: a prep project is done only after its readiness gate: tool, links, questions, quiz (issue #276).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
export CAREERBREW_NOW="2026-11-02T09:00+00:00"
# Error cases first: a bad kind, a bad day, a gate on a plain project, and a hand-marked links gate name their fix.
expect "$(cb work project add demo --kind talk 2>&1 || true)" "unknown project kind talk" "error case: a bad kind"
expect "$(cb work project add demo --kind prep --event-on 'next tuesday' 2>&1 || true)" "looks like 2026-11-03" "error case: a bad event day"
cb work project add plain >/dev/null
expect "$(cb work project gate plain tool 2>&1 || true)" "careerbrew work project set plain --kind prep" "error case: a plain project has no gates"
# A prep project starts with four open gates, each with its fix.
out="$(cb work project add demo --kind prep --event-on 2026-11-03)"
expect "$out" "done only when these 4 gates pass" "add --kind prep creates the gates"
expect "$out" "fix: careerbrew work project gate demo tool" "each open gate names its fix"
expect "$(cb work project gate demo quiz 2>&1 || true)" "not marked by hand: careerbrew work project quiz demo" "error case: the quiz follows from its results"
cb work project source demo url https://example.com/rubric >/dev/null
expect "$(cb work project gate demo links https://example.com/other 2>&1 || true)" "has no link https://example.com/other" "error case: an unknown link"
cb work project check demo >/dev/null
expect "$(cb work project show demo)" "0 of 1 read by you" "a check stamp is not the owner reading the link"
expect "$(cb work project gate demo tool)" "1 of 4 gates pass" "mark the tool gate"
expect "$(cb work project gate demo questions)" "2 of 4 gates pass" "mark the questions gate"
# The event day's plan starts with the open gates.
CAREERBREW_NOW=2026-11-03T08:00+00:00 cb day plan >/dev/null
python3 -c 'import json,sys; t=[s["task"] for s in json.load(sys.stdin)["schedule"]]; assert t[:2]==["gate:demo:links","gate:demo:quiz"], t' <<<"$(data_cat day/plan-2026-11-03.json)" || fail "the event day starts with the open gates"
expect "$(CAREERBREW_NOW=2026-11-03T08:00+00:00 cb day next)" "Close this prep gate now" "on the event day the next step is an open gate"
# A quiz with misses keeps the gate open; a re-quiz must ask every miss again.
out="$(cb work project quiz demo --asked 10 --missed 2)"
expect "$out" "score 80% (8 of 10), 2 miss(es)" "the quiz result is stored as counts"
expect "$out" "careerbrew work project quiz demo --asked 2 --missed <n>" "it says how to re-quiz the misses"
expect "$(cb work project quiz demo --asked 1 --missed 0 2>&1 || true)" "--asked is at least 2" "error case: a re-quiz asks each miss again"
expect "$(cb work project archive demo 2>&1 || true)" "demo is not ready: 2 of 4 gate(s) open (links, quiz); fix: careerbrew work project gate demo links https://example.com/rubric" "error case: archive is refused while gates are open"
expect "$(cb work project quiz demo --asked 2 --missed 0)" "3 of 4 gates pass" "a clean re-quiz passes the quiz gate"
expect "$(cb work project gate demo links https://example.com/rubric)" "READY: 4 of 4 gates pass" "reading every link makes it ready"
expect "$(cb work project list)" "prep ready 4/4" "the list shows readiness"
python3 -c 'import json,sys; d=json.load(sys.stdin); q=d[0]["prep"]["quiz"]; assert q=={"at":"2026-11-02T09:00:00+00:00","asked":2,"missed":0,"score":1.0}, q' <<<"$(cb work project list --json)" || fail "the JSON list holds a content-free quiz result"
expect "$(cb work project archive demo)" "demo archived" "a ready prep project is archived"
cb work project add cancelled --kind prep >/dev/null
expect "$(cb work project archive cancelled --force)" "cancelled archived" "--force archives a cancelled event"
event_seen work.prep
echo "ok US-93 prep readiness"
