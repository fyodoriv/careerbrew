#!/usr/bin/env bash
# US-38: import a Notion career bundle; every kind lands in its feature; re-import is idempotent; bad manifests are refused.
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
out="$(cb import notion --dir "$FIX/notion-bundle")"
expect "$out" "pages 3, jobs 4, flashcards 4, plan 2, skills 3, stories 1, past_job 2" "every kind imported"
expect "$(cb import notion --dir "$FIX/notion-bundle" --check)" "19/19 items present" "check passes"
cb import notion --dir "$FIX/notion-bundle" >/dev/null
expect "$(cb cards stats)" "cards 4 " "re-import does not duplicate cards"
expect "$(cb library list)" "How to pass a system design interview?" "library lists pages"
grep -q '"notion.import"' "$CAREERBREW_HOME/logs/"*.jsonl || fail "log event notion.import"
if grep -rq "Qqperson" "$CAREERBREW_HOME/logs"; then fail "a person's name reached the logs"; fi
cp -R "$FIX/notion-bundle" "$SCRATCH/bad" && sed -i.bak 's/"jobs": 4/"jobs": 9/' "$SCRATCH/bad/manifest.json"
expect "$(cb import notion --dir "$SCRATCH/bad" 2>&1 || true)" "jobs: manifest says 9" "error case: incomplete fetch refused"
# print one library page by a short id prefix; an unknown id names the fix
pid="$(cb library list | awk '/How to pass a system design interview/{print $2; exit}')"
expect "$(cb library show "$pid")" "# How to pass a system design interview?" "a page prints by its short id"
expect "$(cb library show nope-404 2>&1 || true)" "careerbrew library list" "error case: an unknown page names the fix"
echo "ok US-38 notion import"
