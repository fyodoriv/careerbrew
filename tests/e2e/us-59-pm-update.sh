#!/usr/bin/env bash
# US-59: the PM update has one line per project in priority order, naming its work (issue #214); the plan and Next step follow the same priority;
# the web app copies it in one click from the top of Today and Dashboard.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
export CAREERBREW_NOW="2026-10-16T16:00" CAREERBREW_NOTIFY=off
cb init >/dev/null
commits() { # commits <repo dir> <count>: commits by me@example.com on the pinned day
  mkdir -p "$1"; git init -q -b main "$1"
  for i in $(seq 1 "$2"); do
    echo "$i" > "$1/f$i.txt"; git -C "$1" add .
    GIT_AUTHOR_DATE="2026-10-16T12:$(printf '%02d' "$i"):00" GIT_COMMITTER_DATE="2026-10-16T12:$(printf '%02d' "$i"):00" \
      git -C "$1" -c user.email=me@example.com -c user.name=Me -c commit.gpgsign=false commit -q --no-verify -m "feat: $(basename "$1") step $i (#$i)"
  done
}
pm() { # the PM draft, read through the real CLI (the email route prints it as a mail link)
  cb day send --via email | head -1 | "$ROOT/.venv/bin/python" -c 'import sys, urllib.parse as u; print(u.unquote(sys.stdin.read().split("&body=", 1)[1]).strip())'
}
commits "$SCRATCH/agentbrew" 12
commits "$SCRATCH/dotfiles" 8
commits "$SCRATCH/billing" 3
cat > "$CAREERBREW_HOME/work/sources.yaml" <<YAML
sources:
  - {kind: git, path: '$SCRATCH/agentbrew', authors: [me@example.com]}
  - {kind: git, path: '$SCRATCH/dotfiles', authors: [me@example.com]}
  - {kind: git, path: '$SCRATCH/billing', authors: [me@example.com]}
YAML
cb day watch once >/dev/null
expect "$(cb day log)" "23 done" "a busy day: 23 commits seen"
# 1. Error case: a bad priority is refused and names the fix.
expect "$(cb config set project_priority 'Billing=x' 2>&1 || true)" "lowercase letters" "error case: bad project name refused"
expect "$(cb work priority remove nope 2>&1 || true)" "unknown project nope" "error case: unknown project refused"
expect "$(cb work priority show)" "no project priority yet" "empty ranking says how to start"
# 2. Rank the projects; tasks and today's work match by repo, ticket key, or link.
cb work priority set tooling --match agentbrew --match dotfiles >/dev/null
cb work priority set billing --rank 1 --match billing --match 'BILL-*' >/dev/null
cb work task "Polish the sync output" --hours 1 --link https://github.example.com/example-org/dotfiles/issues/3 >/dev/null
cb work task "BILL-7 Close the ledger" --hours 1 >/dev/null
out="$(cb work priority show)"
expect "$out" "1. billing" "billing is rank 1"
expect "$out" "2. tooling" "tooling is rank 2"
expect "$out" "20 items today" "tooling owns 20 of today's commits"
# 3. The PM update: one line per project, in priority order, each naming its work, no ids.
body="$(pm)"
[ "$(printf '%s\n' "$body" | wc -l)" -eq 2 ] || fail "PM update is not one line per project: $body"
expect "$(printf '%s\n' "$body" | head -1)" "billing: billing step 1, billing step 2, and billing step 3 (3 commits); next: close the ledger." "rank 1 first, each result named"
expect "$(printf '%s\n' "$body" | sed -n 2p)" "and 12 more (20 commits in 2 repos (agentbrew, dotfiles)); next: polish the sync output." "related work named, then grouped and counted"
refute "$body" "#" "no PR numbers"
refute "$body" "BILL-7" "no ticket keys"
refute "$body" "t00" "no task ids"
# 4. The plan and Next step follow the priority.
cb day plan --date 2026-10-19 >/dev/null
plan="$(cb report latest)"  # the run page as text
expect "$(printf '%s\n' "$plan" | grep -m1 ' t00')" "t002" "billing's task is planned first"
expect "$plan" "project billing (priority 1 of 2)" "the plan says why"
expect "$(CAREERBREW_NOW=2026-10-19T10:00 cb day next)" "why: project billing (priority 1 of 2)" "Next step names the project priority"
# 5. Priorities change over time: move tooling up, and the update and the plan follow.
cb work priority set tooling --rank 1 >/dev/null
expect "$(pm | head -1)" "tooling:" "the update follows the new order"
cb day plan --date 2026-10-19 >/dev/null
expect "$(cb report latest | grep -m1 ' t00')" "t001" "the plan follows the new order"
# 6. A word shaped like a ticket key stays: only keys of a named ticket project (BILL-*) are ids.
mkdir -p "$SCRATCH/shop"; git init -q -b main "$SCRATCH/shop"; echo 1 > "$SCRATCH/shop/f.txt"; git -C "$SCRATCH/shop" add .
GIT_AUTHOR_DATE="2026-10-16T13:00:00" GIT_COMMITTER_DATE="2026-10-16T13:00:00" \
  git -C "$SCRATCH/shop" -c user.email=me@example.com -c user.name=Me -c commit.gpgsign=false commit -q --no-verify -m "fix: decode UTF-8 file names"
{ data_cat work/sources.yaml; echo "  - {kind: git, path: '$SCRATCH/shop', authors: [me@example.com]}"; } | data_write work/sources.yaml
cb day watch once >/dev/null
body="$(pm)"
expect "$body" "shop: decode UTF-8 file names (1 commit)." "UTF-8 is a word, not a ticket key"
refute "$body" "UTF:" "no line for a UTF ticket project"
# 6b. A PR number or ticket key outside a link is refused (issue #227): the reader has nothing to click.
out="$(cb day send --via email --text 'billing: closed the ledger (#12).' 2>&1 || true)"
expect "$out" "1 line(s) name a PR or ticket with no link" "error case: a bare PR number is refused"
refute "$out" "mailto:" "nothing is drafted"
expect "$(cb day send --via email --text 'billing: closed the ledger ([#12](https://github.example.com/o/r/pull/12)).')" "mailto:" "a linked PR number passes"
# 6c. Issue #239: a meeting you led, your launch post, and your check-in in a group thread reach the update;
#     a recurring series you organize is counted, not named. Sources: the calendar and the work feed only.
cat > "$SCRATCH/led.ics" <<'ICS'
BEGIN:VCALENDAR
VERSION:2.0
PRODID:-//careerbrew//test//EN
BEGIN:VEVENT
UID:plan@example.com
DTSTART;TZID=Etc/GMT+4:20261016T100000
DTEND;TZID=Etc/GMT+4:20261016T110000
ORGANIZER:mailto:me@example.com
SUMMARY:Billing planning with design
END:VEVENT
BEGIN:VEVENT
UID:sync@example.com
DTSTART;TZID=Etc/GMT+4:20261012T093000
DTEND;TZID=Etc/GMT+4:20261012T094500
RRULE:FREQ=DAILY;BYDAY=MO,TU,WE,TH,FR
ORGANIZER:mailto:me@example.com
SUMMARY:Billing sync
END:VEVENT
END:VCALENDAR
ICS
echo "calendars: [{kind: ics, path: '$SCRATCH/led.ics', me: [me@example.com]}]" > "$CAREERBREW_HOME/day/calendars.yaml"
cb work feed add - >/dev/null <<'JSON'
{"id": "slack:C9:1.0001", "source": "slack", "kind": "message", "at": "2026-10-16T14:00:00Z", "title": "You wrote in #billing-news", "actor": "me", "channel": "billing-news", "url": "https://chat.example.com/archives/C9/p1", "thread": "slack:C9:1.0001", "text": "Ledger export is now live"}
JSON
cb work priority set billing --match 'billing*' >/dev/null  # the channel billing-news and the meeting's title
rm -f "$CAREERBREW_HOME/day/cursors.json"  # the calendar source waits 15 minutes between reads: read it now
cb day watch once >/dev/null
body="$(pm)"
expect "$(printf '%s\n' "$body" | grep '^billing:')" "billing: launched ([announced in #billing-news](https://chat.example.com/archives/C9/p1)), led billing planning with design" "a launch leads its project line, then the meeting you led"
expect "$body" "1 meeting" "the recurring sync is counted, not named"
refute "$body" "Billing sync" "a series is no news"
event_seen work.priority
# Content checks inspect the actual text override and keep its details private (#336).
out="$(cb day send --text 'billing: all tickets closed, no action needed.' 2>&1 || true)"
expect "$out" "no-action" "no-action status line is flagged"
# Sent-message edits also teach on a cycle with no new results.
"$ROOT/.venv/bin/python" - <<'PY'
from careerbrew import config, store
from careerbrew.day import pm_learn
from careerbrew.work import feed

day = "2026-10-16"
draft = "billing: shipped export.\ntooling: fixed CI."
config.set_value("pm_slack", "#team-updates")
pm_learn.remember(day, draft, draft)
def row(text):
    return {"id": "slack:C0PM:1.1", "source": "slack", "kind": "message", "at": day + "T21:00:00Z", "title": "You wrote in #team-updates", "actor": "me", "channel": "team-updates", "text": text}
feed.ingest([row("billing: shipped export. In draft")])
pm_learn.learn_sent(day)
feed.ingest([row("billing: shipped export. In draft; shared the runbook.")])
PY
cb day watch once >/dev/null
"$ROOT/.venv/bin/python" - <<'PY'
from careerbrew import store
from careerbrew.day import pm_learn

rows = store.read_jsonl(pm_learn.path())
assert len(rows) == 1 and rows[0]["body"] == "billing: shipped export. In draft; shared the runbook."
assert pm_learn.rules()["cut"] == []
store.write_json(pm_learn.path().with_name("pm-repeat-proof.json"), {"mtime": pm_learn.path().stat().st_mtime_ns})
PY
cb day watch once >/dev/null
"$ROOT/.venv/bin/python" - <<'PY'
from careerbrew import store
from careerbrew.day import pm_learn

before = store.read_json(pm_learn.path().with_name("pm-repeat-proof.json"))
assert pm_learn.path().stat().st_mtime_ns == before["mtime"]
PY
# 7. The web app (issue #145): Copy update next to the day's heading, at the top of Today and Dashboard, copies the
#    update in one click: rich Slack links with a plain fallback, the subject first for email, and the text
#    selected to copy by hand when the browser refuses the clipboard. It runs on its own seeded data root.
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW UV_OFFLINE=1 UV_NO_SYNC=1 UV_PYTHON_DOWNLOADS=never npm_config_offline=true npx --no-install --offline playwright test --reporter=line e2e/pm-copy.spec.ts 2>&1)" || { echo "$out"; fail "Copy update e2e"; }
expect "$out" "13 passed" "Copy update: six existing cases and seven rich link and fallback cases"
[ -s "$CAREERBREW_E2E_OUT/copy-update.png" ] || fail "screenshot of Copy update"
# 8. The real editor clears a learned cut flag before saving (issue #373).
# Keep the six Copy cases and their output unchanged. The second invocation
# has its own server, data and Playwright result directory.
copy_out="$CAREERBREW_E2E_OUT"
browser_story
pm_cut_world="$(cd "$SCRATCH" && pwd -P)/pm-cut-browser"
(umask 077; mkdir "$pm_cut_world"; printf 'US-59 isolated PM cut fixture\n' > "$pm_cut_world/.owner")
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW CAREERBREW_PM_CUT_WORLD="$pm_cut_world" CAREERBREW_E2E_OUT="$copy_out/pm-cut" npx --no-install playwright test \
  --config playwright.pm-cut.config.ts --reporter=line 2>&1)" || { echo "$out"; fail "PM cut editor e2e"; }
expect "$out" "1 passed" "PM cut editor clears the flag before saving"
# 9. Issue #407 geometry uses the crowded encrypted fixture and sole maintained
# Playwright lifecycle. The outer story already occupies its heavy-run slot.
cd "$ROOT"
geometry_log="$SCRATCH/comfort-geometry.log"
if ! bash tests/safety/run_issue_373_ui.sh geometry > "$geometry_log" 2>&1; then
  cat "$geometry_log"; fail "crowded target and plan-tag geometry"
fi
geometry_root="$(head -1 "$geometry_log" | cut -d' ' -f4-)"
expect "$(cat "$geometry_root/playwright.stdout")" "8 passed" "crowded geometry: targets and tags in four modes"
echo "ok US-59 PM update and project priority"
