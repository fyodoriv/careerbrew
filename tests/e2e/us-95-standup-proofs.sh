#!/usr/bin/env bash
# US-95: the standup carries only real updates: a row with no update is hidden (one count line), and with a live
# proof file a row shows only when its check passed with fresh proofs; failed rows go to the command output only.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-19T09:40" CAREERBREW_NOTIFY=off  # a Monday, before the 10:00 standup: since Friday 10:00
cb init >/dev/null
"$ROOT/.venv/bin/python" -c 'from careerbrew import store
from careerbrew.work import evidence
store.append_jsonl(evidence.evidence_path(), [
  {"id": "pr:acme/shop#12", "kind": "pr", "repo": "acme/shop", "title": "PROJ-21 add the export endpoint", "url": "https://github.example.com/acme/shop/pull/12", "at": "2026-10-16T14:00:00-04:00", "tags": [], "origin": "user"},
  {"id": "pr:acme/shop#9", "kind": "pr", "repo": "acme/shop", "title": "PROJ-40 close the ledger", "url": "https://github.example.com/acme/shop/pull/9", "at": "2026-10-14T14:00:00-04:00", "tags": [], "origin": "user"},
  {"id": "pr:acme/shop#14", "kind": "pr", "repo": "acme/shop", "title": "PROJ-60 retry the webhook", "url": "https://github.example.com/acme/shop/pull/14", "at": "2026-10-16T15:00:00-04:00", "tags": [], "origin": "user"}])'
cat > "$SCRATCH/rows.json" <<'JSON'
[{"title": "Order export", "status": "In review", "owner": "me", "keys": ["PROJ-21"]},
 {"title": "Billing ledger", "status": "Done", "owner": "me", "keys": ["PROJ-40"]},
 {"title": "Search ranking", "status": "In progress", "owner": "Ana", "keys": ["PROJ-50"]},
 {"title": "Webhook retries", "status": "In progress", "owner": "me", "keys": ["PROJ-60"]}]
JSON
# 1. Rows with no update are hidden, yours too and one done before the window: one count line instead.
out="$(cb work standup --rows "$SCRATCH/rows.json")"
expect "$out" "rows: 2 shown (2 yours, 0 matched by your work), 2 with no update hidden, updated 09:40" "rows with no update are hidden, and it says when the rows were passed"
page="$CAREERBREW_HOME/out/standup.html"
expect "$(cat "$page")" "2 rows with no update hidden." "the page has one count line"
refute "$(cat "$page")" "Billing ledger" "a row done before the window stays off the page"
expect "$(cat "$page")" "· updated 09:40</p>" "the page says when the rows were passed"
# 2. Error case: a bad proof file is refused, names the fix, and writes nothing.
printf '{"Order export": {"verdict": "maybe", "proofs": []}}' > "$SCRATCH/bad.json"
expect "$(cb work standup --proof "$SCRATCH/bad.json" 2>&1 || true)" "verdict is pass or fail" "error case: an unknown verdict"
[ -e "$CAREERBREW_HOME/work/standup-notes.json" ] && fail "a bad proof file wrote notes"
# 3. A live proof file: one fresh pass, one stale proof, one with no quote, and a fail verdict with the agent's reason.
cat > "$SCRATCH/proof.json" <<'JSON'
{"Order export": {"verdict": "pass", "proofs": [{"source": "github", "url": "https://github.example.com/acme/shop/pull/12", "checked_at": "2026-10-19T09:35", "quote": "Merged on Friday"}]},
 "Search ranking": {"verdict": "pass", "proofs": [{"source": "chat", "url": "https://chat.example.com/archives/C1/p2", "checked_at": "2026-10-19T08:10", "quote": "Ana: shipped the ranking flag"}]},
 "Webhook retries": {"verdict": "pass", "proofs": [{"source": "github", "url": "https://github.example.com/acme/shop/pull/14", "checked_at": "2026-10-19T09:30", "quote": ""}]},
 "Billing ledger": {"verdict": "fail", "proofs": [], "reason": "PROJECTZEBRA was reopened"}}
JSON
out="$(cb work standup --proof - < "$SCRATCH/proof.json")"
expect "$out" "agent notes: 0 talking points, 0 extra actions, 4 proof checks (kept until the next standup)" "the proofs are kept"
expect "$out" "proof check: 1 passed, 3 failed (failed rows stay off the page; standup_proof_minutes 60)" "only the fresh, complete pass shows"
expect "$out" "  failed row 2: verdict is fail; no proof" "a fail verdict and no proof"
expect "$out" "  failed row 3: proof 1 is older than 60 minutes" "a stale proof fails"
expect "$out" "  failed row 4: proof 1 has no quote" "a proof with no quote fails"
refute "$out" "Webhook retries" "a failed row's title stays off a piped stdout"
refute "$out" "PROJECTZEBRA" "the agent's reason stays off a piped stdout"
html="$(cat "$page")"
expect "$html" '<li class="proof"><span class="source">Checked Mon Oct 19, 09:35 in github:</span> <a href="https://github.example.com/acme/shop/pull/12" rel="noreferrer noopener">Merged on Friday</a></li>' "the proof link is listed under its row"
for gone in "Search ranking" "Webhook retries" "Billing ledger" "older than" "no quote" "PROJECTZEBRA"; do
  refute "$html" "$gone" "a failed row and its reason never reach the page"
done
# 4. The proof goes stale as time passes: the same file fails at a later build; a longer limit is a setting.
expect "$(CAREERBREW_NOW=2026-10-19T10:40 cb work standup)" "proof check: 0 passed, 4 failed" "a proof older than the limit fails at the next build"
expect "$(cat "$page")" "No row passed the proof check." "the page says no row passed"
cb config set standup_proof_minutes 0 >/dev/null 2>&1 && fail "standup_proof_minutes takes 1 to 1440"
cb config set standup_proof_minutes 90 >/dev/null
expect "$(CAREERBREW_NOW=2026-10-19T10:40 cb work standup)" "proof check: 1 passed, 3 failed" "a longer limit keeps the proof fresh"
# 5. The issue comment: a plugin clears the rows with an empty list; the page says no row passed and when.
printf '[]' > "$SCRATCH/none.json"
out="$(cb work standup --rows "$SCRATCH/none.json")"
expect "$out" "no row passed the proof check (updated 09:40)" "an empty list clears the rows"
refute "$out" "no rows to report on yet" "a cleared list is not 'no rows yet'"
expect "$(cat "$page")" "No row passed the proof check." "the page says no row passed"
event_seen work.standup.notes
event_seen work.standup
grep -q -e "PROJECTZEBRA" -e "Order export" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
grep -q "Merged on Friday" "$CAREERBREW_HOME/work/standup-notes.json" && fail "proofs stored in plaintext"
echo "ok US-95 standup proofs"
