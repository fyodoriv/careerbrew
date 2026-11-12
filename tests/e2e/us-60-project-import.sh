#!/usr/bin/env bash
# US-60: import a team's goal list from a sheet (CSV, JSON, or a Google Sheets link), with parents and links between projects.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
export CAREERBREW_NOW="2026-10-01T09:00+00:00" CAREERBREW_HTTP_FIXTURES="$FIX/http"
# 1. Error case first: a bad sheet names every bad row, and writes nothing.
cat > "$SCRATCH/bad.csv" <<CSV
name,title,parent,links
alpha,Alpha,beta,
beta,Beta,alpha,
gamma,Gamma,nowhere,
CSV
out="$(cb work project import "$SCRATCH/bad.csv" 2>&1 || true)"
expect "$out" "nothing imported" "error case: a bad sheet is refused"
expect "$out" "parents loop: alpha -> beta -> alpha" "error case: a parent loop is named"
expect "$out" "row 4 (gamma): parent nowhere is not a project (fix:" "error case: an unknown parent names its row and the fix"
expect "$(cb work project list)" "no projects yet" "error case: nothing was written"
expect "$(cb work project import https://sheets.example.com/goals.csv 2>&1 || true)" "download the sheet as CSV" "error case: another site names the fix"
expect "$(cb work project import https://docs.google.com/spreadsheets/d/sheet-private/edit 2>&1 || true)" "the sheet is private" "error case: a private sheet names the fix"
# 2. A dry run checks every row and writes nothing.
out="$(cb work project import "$FIX/projects/team-goals.csv" --dry-run)"
expect "$out" "dry run: 6 row(s): 6 added" "dry run counts the rows"
expect "$(cb work project list)" "no projects yet" "dry run wrote nothing"
# 3. Import the CSV: titles, groups, parents, links, and trackers land in one command.
out="$(cb work project import "$FIX/projects/team-goals.csv")"
expect "$out" "6 row(s): 6 added, 0 updated, 0 unchanged, 5 tracker source(s) added" "import counts"
expect "$out" "ignored columns: owner" "unknown columns are named, not stored"
out="$(cb work project show billing)"
for part in "group: q4-cutline" "parent: payments" "children: invoices" "links: ledger, invoices" "linked from: ledger" "tracker  BILL-1"; do
  expect "$out" "$part" "show: $part"
done
refute "$out" "Billing revamp" "a title is business text: an agent without a terminal does not see it"
expect "$(cb work project list)" "parent payments" "list shows the parent"
expect "$(cb work project show checkout-analytics)" "parent: checkout" "a sheet name with spaces becomes a project name"
data_has "Billing revamp" || fail "the title is stored (encrypted)"
# The inbox asks where notes go once per root project (payments, checkout), not once per imported row.
memory_questions() { cb day inbox | awk '$3=="memory"' | wc -l | tr -d ' '; }
[ "$(memory_questions)" = 2 ] || fail "one notes question per root project, not one per row"
cb work project memory payments work/payments-notes.md >/dev/null
[ "$(memory_questions)" = 1 ] || fail "the root's notes path covers billing, invoices, and ledger"
# 4. A Google Sheets link re-imports the quarter's cutline: new rows are added, known rows take non-empty cells.
cb work project check billing BILL-1 --status "epic 3 of 5" >/dev/null
out="$(cb work project import 'https://docs.google.com/spreadsheets/d/sheet-cutline-q4/edit#gid=0')"
expect "$out" "2 row(s): 1 added, 1 updated" "the sheet link is read as CSV"
out="$(cb work project show billing)"
expect "$out" "parent: payments" "an empty cell keeps the parent"
expect "$out" "ok     tracker  BILL-1" "a re-import keeps the source's check"
expect "$out" "tracker  BILL-9" "a new tracker is added"
expect "$(cb work project show reports)" "parent: payments" "a new row from the sheet"
# 5. JSON rows, and one project by hand: set a parent and links; a loop is refused.
expect "$(cb work project import "$FIX/projects/cutline.json")" "2 added" "JSON rows"
expect "$(cb work project set payments --parent billing 2>&1 || true)" "parents loop" "error case: a loop by hand is refused"
expect "$(cb work project add search-ui --parent search --link checkout)" "project search-ui added" "add with a parent and a link"
expect "$(cb work project set search-ui --unlink checkout --link ledger --group q1-cutline)" "search-ui: parent search, links ledger, group q1-cutline" "set changes links and the group"
# 6. The web app's Work view shows the same tree (Playwright over its own seed, which imports the same sheet).
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW -u CAREERBREW_HTTP_FIXTURES npx playwright test --reporter=line -g "Work shows the projects imported" 2>&1)" || { echo "$out"; fail "web Work view projects"; }
expect "$out" "1 passed" "the Work view shows the project tree"
[ -s "$CAREERBREW_E2E_OUT/work-projects.png" ] || fail "screenshot of the Work view projects"
event_seen work.project
echo "ok US-60 project import"
