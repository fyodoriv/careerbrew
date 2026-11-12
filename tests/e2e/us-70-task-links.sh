#!/usr/bin/env bash
# US-70: every task row in the web app (Dashboard, Today, Next step, Week) shows the task's links, opened in a
# new tab, and its own why; only full http(s) URLs become links, and an old task's single `link` still works.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
cb init >/dev/null
# 1. An old task (before issue #143) keeps one `link` string: it reads as `links`, and its next save stores that shape.
data_write work/tasks.yaml <<'YAML'
tasks:
- {id: t001, title: Ping the owner, impact: 4, urgency: 4, estimate_h: 0.5, competencies: [], due: null, status: open, source: user, why: '', link: 'https://jira.example.com/browse/SHOP-9'}
YAML
cb work done t001 0.5 >/dev/null
stored="$(data_cat work/tasks.yaml)"
expect "$stored" "url: https://jira.example.com/browse/SHOP-9" "an old task's link is kept as links"
refute "$stored" "link: " "the old single-link field is gone after a save"
# 2. Error case: a value that is not a full http(s) URL is kept, with a warning that names the fix; a full URL,
#    labeled (LABEL=URL) or not, gives none. --link repeats.
out="$(cb work task "Tell the repo owner about the file" --hours 0.5 --why "asked in https://slack.example.com/archives/C0CHECKOUT" \
  --link "https://code.example.com/acme/shop/src/app.py" --link "ticket=https://jira.example.com/browse/SHOP-12" --link "#checkout" 2>&1)"
expect "$out" "warning: --link '#checkout' is not a full http(s):// URL" "a partial link warns and names the fix"
expect "$out" "added t002" "the task is still added"
[ "$(grep -c warning <<<"$out")" = 1 ] || { echo "$out"; fail "only the partial link warns"; }
stored="$(data_cat work/tasks.yaml)"
expect "$stored" "label: ticket" "a LABEL=URL link keeps its label"
expect "$stored" "url: '#checkout'" "a partial link is kept as given"
event_seen work.task
grep -q '"links": 3, .*"web_links": 2' "$CAREERBREW_HOME/logs/events.jsonl" || fail "work.task logs the link counts"
refute "$(cat "$CAREERBREW_HOME/logs/events.jsonl")" "example.com" "the event log holds counts, never a URL"
# 3. The web app on the seeded data root: the top task's links are anchors in Dashboard, Today, Next step, and Week,
#    each opens in a new tab, its why's URL is a link, and its javascript: value stays text.
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME npx playwright test --reporter=line e2e/task-links.spec.ts 2>&1)" || { echo "$out"; fail "task links e2e"; }
expect "$out" "1 passed" "task links open in a new tab on Dashboard, Today, and Week"
for s in dashboard today; do [ -s "$CAREERBREW_E2E_OUT/task-links-$s.png" ] || fail "screenshot of $s"; done
echo "ok US-70 task links"
