#!/usr/bin/env bash
# US-84: the owner's inbox decisions change later ones: a plugin they keep dismissing ranks lower, and `signals` says so (issue #185).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-21T10:00+00:00"
cb init >/dev/null
expect "$(cb signals)" "no decisions yet" "error case: no decisions, nothing changes"
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
cb plugin add "$SCRATCH/plugin-acme" >/dev/null
for n in 1 2 3; do
  cb day suggest --plugin acme --ref "doc-$n" --what "Synthetic idea $n" --why "a synthetic reason" --accept "Plans time" >/dev/null
  pid="$(cb day inbox | awk '$3=="plugin"{print $2; exit}')"
  [ -n "$pid" ] || fail "plugin suggestion $n shown in the inbox"
  cb day dismiss "$pid" >/dev/null
done
expect "$(cb signals)" "from (hidden)" "privacy: an agent sees counts and kinds, not source names"
# acme's policy keeps business text from agents: the weight still shows in the counts.
expect "$(cb signals --json)" '"weight": 0.4' "three dismissals lower the plugin's weight"
expect "$(cb signals --json)" '"dismissed": 3' "agents read the counts"
expect "$(cb day suggestions --plugin acme)" "acme: next suggestions rank with weight 0.4" "the plugin reads back what it taught careerbrew"
expect "$(cb signals reset plugin plugin:acme)" "3 decision(s) forgotten" "reset forgets a signal"
expect "$(cb signals)" "no decisions yet" "after the reset nothing is lowered"
expect "$(cb signals reset 2>&1 || true)" "reset needs a kind and a source" "error case: reset names its arguments"
# Smaller gaps: the owner regrades an accepted task, and approval notes become guidance agents read.
tid="$(cb work task "Review the synthetic roadmap" --hours 1 | sed -nE 's/^added (t[0-9]+).*/\1/p' | head -1)"
[ -n "$tid" ] || fail "a task to regrade"
expect "$(cb work task-set "$tid" --impact 5 --hours 2.5)" "impact 5, urgency 3, 2.5h" "the owner regrades a task"
expect "$(cb work task-set "$tid" 2>&1 || true)" "nothing to change" "error case: task-set needs a change"
expect "$(cb approvals guidance)" "no notes yet" "no decisions, no guidance"
# Issue #238: the PM update learns from the text the owner sent, never prints it, and needs a shown draft.
printf 'Led the planning meeting with design.\nLaunched the export: now live. SECRET-CANARY-238\n' >"$SCRATCH/sent.txt"
expect "$(cb day pm-sent "$SCRATCH/sent.txt" --day 2026-10-19)" "no draft was shown for 2026-10-19" "error case: no shown draft, nothing learned"
for d in 20 21; do
  CAREERBREW_NOW="2026-10-${d}T10:00+00:00" cb day send >/dev/null 2>&1 || true
  out="$(cb day pm-sent "$SCRATCH/sent.txt" --day "2026-10-$d")"
  expect "$out" "learned from the sent update" "the sent update for 2026-10-$d is learned"
  refute "$out" "SECRET-CANARY-238" "privacy: the sent text is never printed"
done
expect "$(cb day pm-sent "$SCRATCH/sent.txt" --day 2026-10-21)" "already learned" "the same sent text is learned once"
expect "$(cb signals --json)" '"kind": "added"' "two days of added lines become a lesson agents can count"
refute "$(cb signals --json)" "SECRET-CANARY-238" "privacy: signals hold no sent text"
echo "ok US-84 decision signals"
