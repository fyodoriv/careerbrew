#!/usr/bin/env bash
# US-98: reply drafts fit the thread's newest state (issue #269) and are saved safely (clear, save, read back, lock).
source "$(dirname "$0")/lib.sh"
cb init >/dev/null
cat >"$SCRATCH/thread.json" <<'JSON'
{"messages": [
  {"ts": "1700000001.000100", "text": "Synthetic PR for the new flag is up.", "mine": false},
  {"ts": "1700000002.000100", "text": "Deploy started, will update when done.", "mine": false}
]}
JSON
printf 'Could you rename the flag before we ship?\n' >"$SCRATCH/change.txt"
out="$(cb draft fit --thread "$SCRATCH/thread.json" --draft "$SCRATCH/change.txt" --seen 1700000001.000100)"
expect "$out" "draft: ack (a release has started: approve or acknowledge; your draft asked for changes" "a started deploy turns a change request into an ack"
expect "$out" "text: Looks good, approved." "the ack text"
expect "$out" "stale: 1 newer message(s)" "the draft was written before the newest message"
refute "$out" "rename the flag" "privacy: the draft text is not echoed"
expect "$(cb draft fit --thread "$SCRATCH/thread.json" --draft "$SCRATCH/change.txt" --major 'the migration drops a table')" "text: Please hold the release: the migration drops a table" "a major issue stops it in one line"
printf '{"messages": [{"ts": "1", "text": "Here is the doc."}, {"ts": "2", "text": "thanks!"}]}' >"$SCRATCH/thanks.json"
expect "$(cb draft fit --thread "$SCRATCH/thanks.json" --json)" '"emoji": "pray"' "a one-word thanks gets an emoji suggestion"
printf 'Looks good, approved. Thanks for shipping this.\n' >"$SCRATCH/ack.txt"
expect "$(cb draft replace C9/1700000002.000100 --file "$SCRATCH/ack.txt")" "draft saved and read back: 47 characters match" "replace clears, saves, and reads back"
expect "$(cb draft replace C9/1700000002.000100 --file "$SCRATCH/ack.txt")" "stored locally, not saved in the chat tool" "replace states where the draft was saved"
expect "$(cb draft replace C9/1700000002.000100 --file "$SCRATCH/ack.txt" --json)" '"uses": "slack-draft:C9/1700000002.000100"' "the uses entry a step declares"
expect "$(cb draft replace C9/1700000002.000100 --file "$SCRATCH/ack.txt" --json)" '"chat_tool_saved": false' "the JSON distinguishes local storage from the chat tool"
event_seen draft.replace
expect "$(cb draft check --sent "$SCRATCH/ack.txt" --read-back "$SCRATCH/ack.txt")" "the read-back matches" "check: same text"
set +e
bad="$(cb draft check --sent "$SCRATCH/ack.txt" --read-back "$SCRATCH/change.txt" 2>&1)"; rc=$?
set -e
expect "$rc" "1" "error case: a read-back mismatch exits 1"
expect "$bad" "breaks US-98" "error case: the mismatch names the story"
refute "$bad" "rename the flag" "privacy: the mismatch prints no text"
expect "$(cb draft replace '../etc' --file "$SCRATCH/ack.txt" 2>&1 || true)" "draft target" "error case: a bad target is refused"
expect "$(cb draft fit --thread "$SCRATCH/missing.json" 2>&1 || true)" "could not read the thread file" "error case: a missing thread file"
cat >"$SCRATCH/dir.json" <<'JSON'
{"channels": [{"id": "C1", "owns": ["sso group"], "members": ["U1", "U2"]}],
 "threads": [{"channel": "C1", "url": "https://chat.example.com/C1/p1", "text": "Need the sso group"}]}
JSON
printf 'Bo is blocked: missing the sso group, permission denied\n' >"$SCRATCH/ask.txt"
expect "$(cb draft route --ask "$SCRATCH/ask.txt" --directory "$SCRATCH/dir.json" --teammate U2)" "link the open thread https://chat.example.com/C1/p1 in C1" "an access ask with an open thread links it (issue #314)"
printf 'Same here for <@U9>: the sso group please\n' >"$SCRATCH/access.txt"
set +e
bad="$(cb draft judge --draft "$SCRATCH/access.txt" --channel C1 --directory "$SCRATCH/dir.json" --new-thread 2>&1)"; rc=$?
set -e
expect "$rc" "1" "error case: the judge fails a bad draft"
expect "$bad" "mentions U9, who is not a member of C1" "a non-member mention fails the judge"
expect "$bad" "duplicates the open thread" "a second thread fails the judge"
printf 'Same here: the sso group please\n' >"$SCRATCH/access.txt"
expect "$(cb draft judge --draft "$SCRATCH/access.txt" --channel C1 --directory "$SCRATCH/dir.json")" "draft judge: passes for C1" "a fixed draft passes"
echo "ok US-98 reply drafts"
