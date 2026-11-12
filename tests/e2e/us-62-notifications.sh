#!/usr/bin/env bash
# US-62: opt-in notifications about important new events: each says what happened, why, and the next step;
# repeats merge, a burst is one pop-up, quiet hours hold them, and a click acts (Accept, Snooze, Open).
# The fake notifier (tests/fixtures/notifier) stands in for alerter and terminal-notifier: nothing shows.
source "$(dirname "$0")/lib.sh"
browser_story  # own port and output folder: browser stories run in parallel
export CAREERBREW_NOW="2026-10-21T10:00" CAREERBREW_NOTIFY=on FAKE_NOTIFIER_LOG="$SCRATCH/notifier.log"
mkdir -p "$SCRATCH/bin"
ln -s "$FIX/notifier/fake-notifier.sh" "$SCRATCH/bin/alerter"
ln -s "$FIX/notifier/fake-notifier.sh" "$SCRATCH/bin/terminal-notifier"
export PATH="$SCRATCH/bin:$PATH"
popups() { grep -c '^--- ' "$FAKE_NOTIFIER_LOG" 2>/dev/null || true; }
cb init >/dev/null
printf 'PROJECTZEBRA\n' > "$CAREERBREW_HOME/sensitive-terms.txt"
# 1. Error cases: bad rules are refused with the fix; off until you opt in.
expect "$(cb config set notify_quiet_hours 25:00-08:00 2>&1 || true)" "use HH:MM-HH:MM" "error case: bad quiet hours name the format"
expect "$(cb config set notify_min_priority 9 2>&1 || true)" "from 1 to 5" "error case: a priority out of range is refused"
expect "$(cb notify check)" "notifications are off" "off by default (opt-in)"
# 2. Opt in with the rules; the notifier is named, so a missing one never falls back to a real pop-up.
for kv in "notify_events true" "notify_backend alerter" "notify_quiet_hours 19:00-08:00" "notify_batch_minutes 10"; do cb config set $kv >/dev/null; done
expect "$(cb notify status)" "pop-ups: alerter: buttons Open, Accept, Dismiss, and Snooze" "status names how pop-ups show"
# 3. A burst: a page, a DM, two mentions in one thread (priority 3, below the default 4), and a bot.
cat > "$SCRATCH/burst.jsonl" <<JSON
{"id": "pagerduty:P1", "source": "pagerduty", "kind": "page", "status": "triggered", "at": "2026-10-21T13:40:00Z", "title": "Page: PROJECTZEBRA checkout down", "to_me": true, "thread": "pagerduty:P1"}
{"id": "slack:D1:1.0", "source": "slack", "kind": "dm", "at": "2026-10-21T13:30:00Z", "title": "DM from $CANARY_PERSON", "to_me": true, "thread": "slack:D1"}
{"id": "slack:C1:2.0", "source": "slack", "kind": "mention", "at": "2026-10-21T13:31:00Z", "title": "Mention in #release", "to_me": true, "thread": "slack:C1:2.0"}
{"id": "slack:C1:2.5", "source": "slack", "kind": "mention", "at": "2026-10-21T13:32:00Z", "title": "Mention again in #release", "to_me": true, "thread": "slack:C1:2.0"}
{"id": "slack:C2:3.0", "source": "slack", "kind": "message", "actor": "bot", "at": "2026-10-21T13:33:00Z", "title": "Deploy bot"}
JSON
cb work feed add "$SCRATCH/burst.jsonl" >/dev/null
expect "$(cb notify check --wait)" "2 new, 2 open; 1 pop-up (2 together)" "a burst of important events is one pop-up"
[ "$(popups)" = 1 ] || fail "one pop-up for the burst (got $(popups))"
expect "$(cat "$FAKE_NOTIFIER_LOG")" "2 new for you: 1 page, 1 direct message" "the pop-up counts what happened"
expect "$(cat "$FAKE_NOTIFIER_LOG")" "Most urgent: PagerDuty: a page is assigned to you. Next: open careerbrew to act on each one." "it says the most urgent one and the next step"
for secret in PROJECTZEBRA "$CANARY_PERSON" "Mention in" token; do refute "$(cat "$FAKE_NOTIFIER_LOG")" "$secret" "a pop-up holds no titles, names, or token"; done
# 4. Repeats: the same events again never pop up twice.
expect "$(cb notify check --wait)" "0 new, 2 open; nothing new to pop up" "repeats are de-duplicated"
[ "$(popups)" = 1 ] || fail "no second pop-up for the same events"
list="$(cb notify list)"
expect "$list" "why: it was sent to you directly and you have not replied. next: reply in Slack" "each notification says why and the next step"
refute "$list" "$CANARY_PERSON" "titles stay off a piped stdout"
# 5. Quiet hours: held, then one "while you were away" pop-up after them (pinned clock).
export CAREERBREW_NOW="2026-10-21T21:00"
printf '%s\n' '{"id": "slack:D2:4.0", "source": "slack", "kind": "dm", "at": "2026-10-22T00:50:00Z", "title": "late DM", "to_me": true, "thread": "slack:D2"}' | cb work feed add - >/dev/null
expect "$(cb notify check --wait)" "no pop-up: quiet hours (19:00-08:00); 1 arrive together after them" "quiet hours hold a pop-up"
[ "$(popups)" = 1 ] || fail "nothing pops up in quiet hours"
export CAREERBREW_NOW="2026-10-22T08:30"
cb notify check --wait >/dev/null
[ "$(popups)" = 2 ] || fail "the held one pops up after quiet hours"
expect "$(cat "$FAKE_NOTIFIER_LOG")" "Slack: a direct message asks you" "the held DM arrives after quiet hours"
# 6. Interactive: a click on Accept decides an agent's suggestion in the inbox.
export CAREERBREW_NOW="2026-10-22T15:00" FAKE_NOTIFIER_CLICK=Accept
cb config set notify_min_priority 3 >/dev/null
cb config set project_priority "nightjar=*rollout*" >/dev/null  # a codename that is not in sensitive-terms.txt
cb work task "Review the rollout checklist" --source agent --why "the release is Thursday" >/dev/null
cb notify check --wait >/dev/null
expect "$(cat "$FAKE_NOTIFIER_LOG")" "Accept,Dismiss,Snooze" "a suggestion's pop-up offers Accept, Dismiss, and Snooze"
expect "$(cat "$FAKE_NOTIFIER_LOG")" "it is on your rank-1 project" "the pop-up says it is on the rank-1 project"
refute "$(cat "$FAKE_NOTIFIER_LOG")" "nightjar" "a pop-up never names the project"
expect "$(cb notify list --all)" "[done] p4 careerbrew: an agent proposes a task" "the click closed the notification (p3 +1 for the rank-1 project)"
refute "$(cb day inbox)" "agent-task" "Accept decided the suggestion in the inbox"
unset FAKE_NOTIFIER_CLICK
# 6b. Error case: a late Accept on a suggestion decided in the inbox since it showed changes nothing.
cb config set notify_min_priority 4 >/dev/null  # the inbox's own goal suggestion (3) stays in the web app
cb work task "Update the rollout runbook" --source agent --why "it is out of date" >/dev/null  # 3, +1 for the rank-1 project
cb notify check --wait >/dev/null
late="$(cb notify list | sed -nE 's/^- (n-[0-9a-f]+) .*an agent proposes a task.*/\1/p' | head -1)"
[ -n "$late" ] || fail "an open suggestion notification"
cb day dismiss "$(cb day inbox | awk '/agent-task/{print $2; exit}')" >/dev/null
expect "$(cb notify act "$late" accept 2>&1 || true)" "already decided in the inbox; nothing changed" "error case: a late Accept changes nothing"
# 7. Snooze from the CLI (the web app and the pop-up run the same action).
dm="$(cb notify list | sed -nE 's/^- (n-[0-9a-f]+) .*direct message.*/\1/p' | head -1)"
[ -n "$dm" ] || fail "an open DM notification to snooze"
expect "$(cb notify act "$dm" snooze)" "snoozed until 16:00" "Snooze hides it for notify_snooze_minutes"
# 8. terminal-notifier: a click runs `careerbrew notify open <id>`, which carries no token.
export CAREERBREW_NOW="2026-10-22T15:20"  # past the batch window of the last pop-up
cb config set notify_backend terminal-notifier >/dev/null
cb config set notify_min_priority 4 >/dev/null  # back to the default: the inbox's own goal suggestion (3) stays in the web app
printf '%s\n' '{"id": "github:pr-7", "source": "github", "kind": "review-request", "at": "2026-10-22T18:55:00Z", "title": "Review PR 7", "to_me": true, "thread": "github:pr-7"}' | cb work feed add - >/dev/null
cb notify check >/dev/null
last="$(awk '/^--- /{n=NR} {line[NR]=$0} END{for (i=n; i<=NR; i++) print line[i]}' "$FAKE_NOTIFIER_LOG")"
expect "$last" "--- terminal-notifier" "terminal-notifier shows the pop-up"
expect "$last" "GitHub: a review is requested from you" "a review request notifies"
expect "$last" "careerbrew notify open n-" "a click opens that notification in the web app"
refute "$last" "token" "the click command carries no token"
event_seen notify.check
event_seen notify.act
grep -q "PROJECTZEBRA" "$CAREERBREW_HOME/logs/events.jsonl" && fail "event log holds content"
grep -q "PROJECTZEBRA" "$CAREERBREW_HOME/day/notifications.json" && fail "notifications stored in plaintext"
# 9. The web app's Notifications view: a deep link opens one, and Accept, Snooze, and Dismiss work (Playwright over its own seed).
cd "$ROOT/web"
out="$(env -u CAREERBREW_HOME -u CAREERBREW_NOW npx playwright test --reporter=line e2e/notifications.spec.ts 2>&1)" || { echo "$out"; fail "web Notifications view"; }
expect "$out" "5 passed" "every notification is actionable in the web app"
[ -s "$CAREERBREW_E2E_OUT/notifications.png" ] || fail "screenshot of the Notifications view"
# 10. notify open: the click command marks it read and opens its view in the web app
cd "$ROOT"
trap 'web_stop; rm -rf "$SCRATCH"' EXIT
fake_browser
nid="$(cb notify list | sed -nE 's/^- (n-[0-9a-f]+) .*/\1/p' | head -1)"
[ -n "$nid" ] || fail "an open notification to open"
expect "$(cb notify open "$nid")" "opened the web app in your browser: #/notifications/$nid" "a click opens that notification"
expect "$(cat "$FAKE_BROWSER_LOG")" "#/notifications/$nid&token=" "the browser gets the deep link"
expect "$(cb notify list)" "$nid [read]" "the opened one is marked read"
web_stop
echo "ok US-62 notifications"
