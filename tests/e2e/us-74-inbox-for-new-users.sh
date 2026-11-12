#!/usr/bin/env bash
# US-74: the suggestions inbox explains itself, folds alike rows into one group, and drops asks already done elsewhere;
# Dashboard has a Getting started checklist. Synthetic data only: tickets PROJ-* from recorded Jira answers, repo acme/shop.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_HTTP_FIXTURES="$FIX/http" CB_T="synthetic" CAREERBREW_NOW="2026-10-20T09:00+00:00"
cb init >/dev/null
cb connect jira --ref env:CB_T --base-url https://jira.example.com --email me@example.com --no-verify >/dev/null
cb work project add syn-onboarding >/dev/null
cb work project source syn-onboarding tracker PROJ-7 >/dev/null
# The owner lets agents read business data, so this script (no terminal) sees the rows' text.
mkdir -p "$SCRATCH/p" && printf 'name: allow\nversion: 0.1.0\npolicy: {agent_may_read_business: true}\n' > "$SCRATCH/p/careerbrew-plugin.yaml"
cb plugin add "$SCRATCH/p" >/dev/null
# 1. Research finds three child tickets of PROJ-7: one group, with the parent as the reason and a link per ticket.
cb checkup >/dev/null 2>&1 || true
out="$(cb day inbox)"
expect "$out" "Add 3 trackers to project syn-onboarding" "alike research rows are one group"
expect "$out" "why: each is a child of PROJ-7, which project syn-onboarding already follows" "the group says why"
gid="$(grep -oE 'grp-[0-9a-f]{10}' <<<"$(grep 'Add 3 trackers' <<<"$out")" | head -1)"
[ -n "$gid" ] || fail "no group id in: $out"
each="$(cb day inbox --group "$gid")"
expect "$each" "Add tracker PROJ-101 to project syn-onboarding: Fix flaky payment retries" "Review each: the ticket's own title"
expect "$each" "from: careerbrew: research on project syn-onboarding" "each row names its source"
expect "$each" "link: PROJ-101 https://jira.example.com/browse/PROJ-101" "each row links its ticket"
expect "$each" "accept: Adds PROJ-101 to the sources of project syn-onboarding" "each row says what Accept will do"
expect "$each" "dismiss: Nothing changes" "each row says what Dismiss will do"
web="$("$ROOT/.venv/bin/python" -c 'from careerbrew import server; print("\n".join(i.get("what", "") for i in server.inbox_view()["inbox"] if i["kind"] == "source"))')"
expect "$web" "Add tracker PROJ-101 to project syn-onboarding: Fix flaky payment retries" "the web app's What line: the ticket's own title"
# 2. Error case: Accept all without --yes changes nothing.
set +e; out="$(cb day accept-all "$gid")"; rc=$?; set -e
[ "$rc" = 2 ] || fail "accept-all without --yes must exit 2 (got $rc)"
expect "$out" "not done: add --yes --count 3" "accept-all asks for --yes and the count it showed"
refute "$(cb work project show syn-onboarding)" "tracker  PROJ-101" "nothing added without --yes (a source line, not the proposed one)"
set +e; out="$(cb day accept-all "$gid" --yes --count 2)"; rc=$?; set -e
[ "$rc" = 1 ] || fail "accept-all with another count must exit 1 (got $rc)"
expect "$out" "now holds 3 suggestions, not 2" "the group changed since it was shown: nothing changes"
refute "$(cb work project show syn-onboarding)" "tracker  PROJ-101" "nothing added on a count mismatch"
expect "$(cb day accept-all "$gid" --yes --count 3)" "$gid: 3 accepted" "Accept all accepts each row"
shown="$(cb work project show syn-onboarding)"
for k in PROJ-101 PROJ-102 PROJ-103; do expect "$shown" "tracker  $k" "accepted tracker $k is a source"; done
refute "$shown" "proposed:" "none left proposed"
# 3. An agent's review ask; then a review the owner gave on that PR arrives in the work feed: the ask expires (kept).
proposal="$(env -u CAREERBREW_TEST_OWNER "$ROOT/.venv/bin/careerbrew" work task "Review acme/shop#12 before the release" --source agent --why "a synthetic review request" --link https://github.com/acme/shop/pull/12)"
item="$(grep -oE 'agent-task-[0-9a-f]{8}' <<<"$proposal" | head -1)"
[ -n "$item" ] || fail "the agent's ask is not in the inbox"
expect "$proposal" "careerbrew day accept $item" "the proposal names its exact inbox acceptance command"
expect "$(cb day inbox)" "$item" "the named proposal is already stored"
printf '%s\n' '{"id": "github:review:us74", "source": "github", "kind": "review", "actor": "me", "at": "2026-10-20T10:00:00Z", "title": "Reviewed the synthetic PR", "url": "https://github.com/acme/shop/pull/12"}' > "$SCRATCH/review.jsonl"
cb work feed add "$SCRATCH/review.jsonl" >/dev/null
out="$(cb day inbox)"
refute "$out" "$item" "a sure match leaves the open list"
expect "$out" "done elsewhere:" "the inbox says how many it hid"
expect "$(data_cat day/inbox.json)" "you gave a review (github)" "kept with its signal, never deleted"
expect "$(cb day reopen "$item")" "$item open" "Bring back"
expect "$(cb day inbox)" "$item" "the owner's choice wins on the next cycle"
# 4. Getting started: ticked from data; hidden on request.
out="$(cb checklist)"
expect "$out" "[x] Connect a work tool" "a done step is ticked"
expect "$out" "[ ] Turn on the day watcher: careerbrew day watch on" "an open step names its command"
cb config set first_run_checklist false >/dev/null
expect "$(cb checklist)" "hidden on Dashboard" "Hide checklist is a setting"
event_seen day.inbox.group
event_seen day.inbox.expire
# 5. Dismiss all: an agent proposes three tasks for one reason, one group; without --yes nothing changes.
for t in "Draft the drill page" "Record the drill demo" "Write the drill quiz"; do
  env -u CAREERBREW_TEST_OWNER "$ROOT/.venv/bin/careerbrew" work task "$t" --source agent --why "the synthetic drill sprint needs them" >/dev/null
done
out="$(cb day inbox)"
expect "$out" "why: an agent added them: the synthetic drill sprint needs them" "the agent's three tasks are one group"
dgid="$(grep -oE 'grp-[0-9a-f]{10}' <<<"$(grep 'Plan 3 tasks an agent added' <<<"$out" || true)" | head -1 || true)"
[ -n "$dgid" ] || fail "no group for the agent's three tasks in: $out"
set +e; out="$(cb day dismiss-all "$dgid")"; rc=$?; set -e
[ "$rc" = 2 ] || fail "dismiss-all without --yes must exit 2 (got $rc)"
expect "$out" "not done: add --yes --count 3" "dismiss-all asks for --yes and the count it showed"
expect "$(cb day dismiss-all "$dgid" --yes --count 3)" "$dgid: 3 dismissed" "Dismiss all dismisses each row"
refute "$(cb day inbox)" "synthetic drill sprint" "the dismissed rows are gone from the inbox"
# 6. The browser on its own seeded data root: rows explain themselves, Accept all asks first, Done elsewhere and Bring
#    back, filters and sort kept in the URL, every block says What is this?, and Getting started hides.
browser_story
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_PUBLIC_CARDS -u CAREERBREW_HTTP_FIXTURES -u CAREERBREW_NOW npx playwright test --reporter=line e2e/inbox.spec.ts 2>&1)" || { echo "$out"; fail "inbox e2e"; }
expect "$out" "6 passed" "6 browser cases"
[ -s "$CAREERBREW_E2E_OUT/inbox.png" ] || fail "screenshot inbox"
echo "ok US-74 inbox for new users"
