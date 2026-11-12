#!/usr/bin/env bash
# US-65: a standup report since the last standup: one page with, per row you report on,
# a talking point first, then its status, then the evidence; one line per action.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-19T09:00" CAREERBREW_NOTIFY=off  # a Monday, before the 10:00 standup
cb init >/dev/null
commit() { # commit <repo dir> <ISO time> <subject>: a commit by me@example.com at that time
  [ -d "$1/.git" ] || { mkdir -p "$1"; git init -q -b main "$1"; }
  echo "$3" >> "$1/log.txt"; git -C "$1" add .
  GIT_AUTHOR_DATE="$2" GIT_COMMITTER_DATE="$2" git -C "$1" -c user.email=me@example.com -c user.name=Me -c commit.gpgsign=false commit -q --no-verify -m "$3"
}
# 1. Error cases: a bad setting or a bad row is refused and names the fix; with no rows it says how to pass them.
expect "$(cb config set standup_time 25:00 2>&1 || true)" "HH:MM" "error case: a bad standup time"
expect "$(cb config set standup_tz Mars/Base 2>&1 || true)" "IANA time zone" "error case: an unknown zone"
printf '[{"title": "ok"}, {"status": "x"}]' > "$SCRATCH/bad.json"
expect "$(cb work standup --rows "$SCRATCH/bad.json" 2>&1 || true)" "row 2: missing title" "error case: a row with no title is named"
expect "$(cb work standup)" "no rows to report on yet" "no rows yet: it says how a plugin or agent passes them"
# 2. Your work: commits, the same PR from gh (a synthetic gh), a feed event, a note, and a plugin row with no offset.
commit "$SCRATCH/shop" 2026-10-15T16:00:00-04:00 "feat: add the order filter (#10)"  # before Friday's standup
commit "$SCRATCH/shop" 2026-10-16T14:00:00-04:00 "PROJ-21 add the export endpoint (#12)"
commit "$SCRATCH/shop" 2026-10-16T15:00:00-04:00 "fix: decode UTF-8 file names"
commit "$SCRATCH/agentbrew" 2026-10-17T11:00:00-04:00 "feat: export skill for agents"
mkdir -p "$SCRATCH/bin"
cat > "$SCRATCH/bin/gh" <<'SH'
#!/bin/sh
echo '[{"number": 12, "title": "PROJ-21 add the export endpoint", "url": "https://github.example.com/acme/shop/pull/12", "createdAt": "2026-10-16T17:30:00Z", "closedAt": "2026-10-16T18:30:00Z", "state": "MERGED", "repository": {"nameWithOwner": "acme/shop"}}]'
SH
chmod +x "$SCRATCH/bin/gh"
cat > "$CAREERBREW_HOME/work/sources.yaml" <<YAML
sources:
  - {kind: git, path: '$SCRATCH/shop', authors: [me@example.com]}
  - {kind: git, path: '$SCRATCH/agentbrew', authors: [me@example.com]}
  - {kind: gh}
YAML
expect "$(PATH="$SCRATCH/bin:$PATH" cb work collect)" "0 source error(s)" "commits and the PR are collected"
cb work feed add - >/dev/null <<'JSON'
{"id": "gh:merge:12", "source": "github", "kind": "merge", "actor": "me", "at": "2026-10-16T18:30:00Z", "title": "Merged pull request #12", "url": "https://github.example.com/acme/shop/pull/12"}
{"id": "jira:t1", "source": "jira", "kind": "transition", "actor": "me", "at": "2026-10-16T19:00:00Z", "title": "Moved a ticket to review", "project": null}
JSON
cb work note "Paired with Sam on PROJ-31 retries" >/dev/null
"$ROOT/.venv/bin/python" -c 'from careerbrew import store
from careerbrew.work import evidence
store.append_jsonl(evidence.evidence_path(), [{"id": "acme:7", "kind": "external", "title": "Moved the ledger job to the new queue PROJ-40", "at": "2026-10-16T10:30:00", "tags": [], "origin": "plugin:acme"}])'
# 3. A plugin passes the rows; a tooling group never matches rows.
cat > "$SCRATCH/rows.json" <<'JSON'
{"ticket_projects": ["PROJ"],
 "rows": [
  {"title": "Order export", "status": "In review", "owner": "me", "keys": ["PROJ-21"], "aliases": ["export"]},
  {"title": "Checkout retries", "status": "In progress", "owner": "Sam Lee", "keys": ["PROJ-31"]},
  {"title": "Billing ledger", "status": "To do", "owner": "me", "keys": ["PROJ-40"]},
  {"title": "Search ranking", "status": "In progress", "owner": "Ana", "keys": ["PROJ-50"]}],
 "groups": [{"label": "Tooling", "repos": ["agentbrew", "dotfiles"], "never_rows": true}]}
JSON
out="$(cb work standup --rows "$SCRATCH/rows.json" --show)"
expect "$out" "standup since Fri Oct 16, 10:00 (Etc/GMT+4), until Mon Oct 19, 09:00" "Monday counts from Friday's standup"
expect "$out" "rows: 3 shown (2 yours, 1 matched by your work), 1 with no update hidden" "your rows with work show; Sam's shows because your note matched it"
expect "$out" "your actions: 6 (3 on rows, 1 in Tooling, 2 in Other work)" "one line per action, tooling apart"
expect "$out" "titles hidden" "titles stay off a piped stdout"
refute "$out" "Order export" "no row title reaches an agent"
# 4. The page: per row a talking point, then status, then evidence; one line for the PR, its commit, and the feed event.
page="$CAREERBREW_HOME/out/standup.html"
html="$(cat "$page")"
[ "$(grep -o 'data-kinds="pr commit feed"' "$page" | wc -l | tr -d ' ')" = 1 ] || fail "the PR, its squash commit, and the feed event are not one line"
[ "$(grep -o 'class="action"' "$page" | wc -l | tr -d ' ')" = 6 ] || fail "six actions expected on the page"
expect "$html" 'href="https://github.example.com/acme/shop/pull/12"' "the merged line keeps the PR link"
expect "$html" '<p class="talk">Add the export endpoint (1 PR).</p>' "a talking point in plain words"
expect "$html" "Moved the ledger job to the new queue" "a plugin row with no offset is read in the standup zone, not dropped"
refute "$html" 'class="key">UTF-8' "UTF-8 is never a ticket key"
expect "$html" '<ul class="talk"><li>Decode UTF-8 file names (1 commit).</li><li>Moved a ticket to review (1 event).</li></ul>' "UTF-8 stays in its own speaking bullet; a null feed project is Other work"
refute "$html" "Search ranking" "a row nobody's work matched stays out"
"$ROOT/.venv/bin/python" - "$page" <<'PY'
import sys
s = open(sys.argv[1], encoding="utf-8").read()
row = s.split('class="row mine"><h2>Order export', 1)[1].split("</section>", 1)[0]
assert row.index('class="talk"') < row.index('class="status"') < row.index('class="evidence"'), "talking point, status, evidence"
assert "export skill" not in row, "tooling work never matches a row, even by word"
tooling = s.split("<h2>Tooling</h2>", 1)[1].split("</section>", 1)[0]
assert "feat: export skill for agents" in tooling
PY
# 5. Issue #229: an agent's talking point (--talk) and an action careerbrew does not collect (--extra), kept for this window.
python3 -c 'import json; print(json.dumps({"Order export": "x" * 201}))' > "$SCRATCH/long.json"
expect "$(cb work standup --talk "$SCRATCH/long.json" 2>&1 || true)" "at most 200 characters" "error case: a talking point that is too long"
printf '[{"title": "Ranking sync", "at": "2026-10-16T14:00:00", "kind": "meeting", "row": "Nope"}]' > "$SCRATCH/bad-extra.json"
expect "$(cb work standup --extra "$SCRATCH/bad-extra.json" 2>&1 || true)" "is not a row you report on" "error case: an action names a row that is not there"
[ -e "$CAREERBREW_HOME/work/standup-notes.json" ] && fail "a bad input wrote notes"
printf '{"Order export": "Shipped the export endpoint and showed it to support. Next: the CSV format."}' > "$SCRATCH/talk.json"
printf '[{"title": "Agreed the ranking weights with Ana", "at": "2026-10-16T14:00:00", "kind": "slack", "url": "https://slack.example.com/archives/C1/p1", "row": "Search ranking"}]' > "$SCRATCH/extra.json"
notes="$(cb work standup --talk "$SCRATCH/talk.json" --extra - < "$SCRATCH/extra.json")"
expect "$notes" "agent notes: 1 talking point, 1 extra action (kept until the next standup)" "both inputs are kept"
expect "$notes" "rows: 4 shown (2 yours, 2 matched by your work), 0 with no update hidden" "the agent's action shows Ana's row"
expect "$notes" "your actions: 7 (4 on rows" "the agent's action counts as new work"
refute "$notes" "ranking weights" "an agent's action stays off a piped stdout"
html="$(cat "$page")"
expect "$html" '<p class="talk">Shipped the export endpoint and showed it to support. Next: the CSV format. <span class="by">written by an agent</span></p>' "the agent's talking point replaces the generated line"
expect "$html" 'data-kinds="slack"' "the agent's action joins the evidence"
expect "$html" "via an agent" "and says where it came from"
grep -q "ranking weights" "$CAREERBREW_HOME/work/standup-notes.json" && fail "agent notes stored in plaintext"
# Issue #307: the CLI accepts structured bullets, and the page keeps each as a list item.
printf '%s' '{"Order export": ["Shipped the API (2 PRs)", "Next: the CSV format", "Ask: a review from the data team"]}' > "$SCRATCH/bullets.json"
cb work standup --talk "$SCRATCH/bullets.json" >/dev/null
html="$(cat "$page")"
expect "$html" '<ul class="talk"><li>Shipped the API (2 PRs)</li><li>Next: the CSV format</li><li>Ask: a review from the data team</li></ul>' "three speaking bullets stay separate in the page"
printf '%s' '{"Order export": ["1", "2", "3", "4", "5", "6"]}' > "$SCRATCH/six-bullets.json"
expect "$(cb work standup --talk "$SCRATCH/six-bullets.json" 2>&1 || true)" "1 to 5" "six bullets are refused with the fix"
# 6. During and after Monday's standup it counts from Friday's; with standup_window since-last, after it from Monday's,
#    a new window that clears the agent notes.
expect "$(CAREERBREW_NOW=2026-10-19T10:05 cb work standup)" "standup since Fri Oct 16, 10:00" "a run during the standup counts from the one before"
expect "$(CAREERBREW_NOW=2026-10-19T10:20 cb work standup)" "standup since Fri Oct 16, 10:00 (Etc/GMT+4), until Mon Oct 19, 10:20; compares against the Fri Oct 16 standup (standup_window previous-day)" "issue #200: after Monday's standup, still from Friday's"
cb config set standup_window bogus >/dev/null 2>&1 && fail "standup_window takes only previous-day or since-last"
cb config set standup_window since-last >/dev/null
after="$(CAREERBREW_NOW=2026-10-19T10:20 cb work standup)"
expect "$after" "standup since Mon Oct 19, 10:00" "since-last: after the standup, from it"
expect "$after" "rows: 0 shown (0 yours, 0 matched by your work), 4 with no update hidden" "issue #271: a row with no update is hidden, yours too"
expect "$after" "your actions: 0" "nothing since"
[ -e "$CAREERBREW_HOME/work/standup-notes.json" ] && fail "a new standup window keeps the agent notes"
grep -q "written by an agent" "$page" && fail "a new standup window keeps the agent's talking point"
event_seen work.standup.notes
event_seen work.standup
grep -q -e "Order export" -e "ranking weights" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
grep -q "Order export" "$CAREERBREW_HOME/work/standup.json" && fail "rows stored in plaintext"
# #406: source health is a separate read-only status; rendering keeps row facts age.
expect "$(cb work live status --json)" '"status": "missing"' "saved standup rows do not certify source setup"
echo "ok US-65 standup report"
