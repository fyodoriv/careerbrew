#!/usr/bin/env bash
# US-67: every long run ends with one HTML page (Blockers, Next, Done); agents read a content-free summary and ask what to do next.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-19T09:00"
cb init >/dev/null
cb work task "Ship the $CANARY_EMPLOYER export" --hours 1 --due 2026-10-10 >/dev/null
echo "calendars: [{kind: ics, path: '$FIX/calendar/work.ics'}]" > "$CAREERBREW_HOME/day/calendars.yaml"

# 1. Each long run prints its page path and one summary line instead of its step text.
for run in "review:review" "work collect:work-collect" "day plan:day-plan" "--no-watch --no-web --no-refresh:start" "checkup --offline:checkup" "machine ready:machine-ready"; do
  args="${run%%:*}"; name="${run##*:}"
  # shellcheck disable=SC2086 # the arguments split on purpose
  out="$(cb $args 2>/dev/null || true)"
  [ "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" = 2 ] || { printf '%s\n' "$out" >&2; fail "$name prints two lines"; }
  first="$(printf '%s\n' "$out" | head -1)"
  expect "$first" "page: " "$name prints its page path first"
  expect "$first" "/out/reports/$name-20261019-090000.html" "$name writes its page"
  expect "$(printf '%s\n' "$out" | tail -1)" "$name: " "$name ends with one summary line"
done
expect "$(cb report latest --run work-collect)" "done     Collected work evidence: 0 new, 0 total, 0 source error(s)" "the page's items print as text on request"
page="$CAREERBREW_HOME/out/reports/start-20261019-090000.html"
for section in 'id="h-done">Done' 'id="h-blockers">Blockers' 'id="h-next">Next' "prefers-color-scheme: dark"; do expect "$(cat "$page")" "$section" "the start page has $section"; done
refute "$(cat "$page")" "<script" "the page runs no script"
expect "$(cat "$page")" "$CANARY_EMPLOYER" "the owner's page shows the task title"

# 2. Agents: counts, ids, statuses, and commands only, never the title.
json="$(cb report latest --run start --json)"
expect "$json" '"label": "Task t001 is overdue"' "the overdue task is a blocker, by id"
expect "$json" '"command": "careerbrew day next"' "each next action has its exact command"
refute "$json" "$CANARY_EMPLOYER" "no business text in the agent summary"
expect "$(cb report list)" "6 run page(s)" "every run's page is kept"
event_seen report.write

# 3. scripts/checkup.sh: the whole repo checkup becomes one page; stdout is the path and one line.
repo="$SCRATCH/repo"; mkdir -p "$repo/scripts" "$repo/.metrics"
cp "$ROOT/scripts/checkup.sh" "$ROOT/scripts/report-html.sh" "$repo/scripts/"
ln -s "$ROOT/.venv" "$repo/.venv"
printf '#!/usr/bin/env bash\necho "OPEN  #7 feat: synthetic (branch feat/x, updated 2026-10-19)"\necho "      next: review, then merge when the gate is green"\nexit 1\n' > "$repo/scripts/open-prs.sh"
printf '#!/usr/bin/env bash\necho "ok    git installed"\n' > "$repo/scripts/doctor.sh"
printf '#!/usr/bin/env bash\necho "check.sh: full gate passed"\n' > "$repo/scripts/check.sh"
printf '#!/usr/bin/env bash\necho "metrics: no data yet"\n' > "$repo/scripts/metrics.sh"
printf '#!/usr/bin/env bash\necho none\n' > "$repo/scripts/open-work.sh"
chmod +x "$repo/scripts/"*.sh
# The wrap-up skill's open-work.sh format: the repo path, then `  <kind>     <text>` per item, exit 1.
mkdir -p "$SCRATCH/wrap-up/scripts"
printf '#!/usr/bin/env bash\necho "$PWD"\nprintf "  %%-9s %%s\\n" branch "feat/y: 2 commit(s) not on main"\nexit 1\n' > "$SCRATCH/wrap-up/scripts/open-work.sh"
out="$(WRAP_UP_SKILL="$SCRATCH/wrap-up" NEW_PROJECT_SKILL="$SCRATCH/none" bash "$repo/scripts/checkup.sh" 2>/dev/null || true)"
[ "$(printf '%s\n' "$out" | wc -l | tr -d ' ')" = 2 ] || { printf '%s\n' "$out" >&2; fail "checkup.sh prints two lines"; }
expect "$out" "repo-checkup: blocked: " "the summary line"
json="$(cb report latest --json)"
expect "$json" '"personal": false' "the repo checkup page holds no personal data"
expect "$json" '"command": "gh pr checkout 7"' "the open PR is the first next action"
expect "$json" '"label": "1 open work item(s) to finish first"' "open work from the wrap-up skill is a blocker"
expect "$json" '"command": "git switch feat/y"' "with the command to continue it"

# 4. Error case: with run_pages off, runs print text only.
cb config set run_pages false >/dev/null
out="$(cb review)"
refute "$out" "page:" "no page when the setting is off"
expect "$out" "no-data  " "with pages off, the step text prints as before"
expect "$(cb report latest --run nope 2>&1 || true)" "no run page for nope (runs with pages: checkup, day-plan, machine-ready, repo-checkup, review, start, work-collect)" "error case: an unknown run names the runs that have pages"
echo "ok US-67 run pages"
