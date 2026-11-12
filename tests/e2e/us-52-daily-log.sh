#!/usr/bin/env bash
# US-52: one command starts everything; the watcher keeps a daily log, proposals, and questions current.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-16T16:00" CAREERBREW_NOTIFY=off
# 1. One command, sane defaults (no web app or watcher process in CI).
out="$(cb --no-watch --no-web --no-refresh)"
expect "$out" "for you:" "careerbrew with no command starts and shows what matters"
expect "$(cb config)" "work_end           17:30" "config shows defaults"
expect "$(cb config set work_end 18:00)" "saved to settings.yaml" "config set"
expect "$(cb config set work_end 25:00 2>&1 || true)" "HH:MM" "error case: invalid setting refused"
# 2. Work happens; a watcher cycle sees it and rebuilds the log, proposals, and questions.
cb work task "Backfill old invoices" --hours 2 >/dev/null
cb work note "feat: Backfill old invoices for PROJ-55" --tags delivery >/dev/null
cb day watch once >/dev/null
out="$(cb day log)"
expect "$out" "1 done" "log rebuilt with today's work"
expect "$(cb day log --list)" "2026-10-16" "log kept for history"
# 3. Proposals: the matching task can be marked done; goal suggestions need the web app.
inbox="$(cb day inbox)"
expect "$inbox" "task-done" "task-done proposed"
id="$(printf '%s\n' "$inbox" | awk '/task-done/ && !seen++ {print $2}')"
expect "$(cb day accept "$id")" "accepted" "proposal accepted"
gid="$(printf '%s\n' "$inbox" | awk '$3=="goal" && !seen++ {print $2}')"
expect "$(cb day accept "$gid" 2>&1 || true)" "web app only" "error case: goal approval only in the web app"
# 4. Questions: answered and applied.
cb work task "Plan the migration" --hours 1 >/dev/null
cb day inbox >/dev/null
qid="$(cb day inbox | awk '$3=="competency" && !seen++ {print $2}')"
expect "$(data_cat day/inbox.json)" "Answer one or more of: collaboration, delivery, design" "a competency question lists the ids to answer with"
expect "$(cb day answer "$qid" telepathy 2>&1 || true)" "unknown competencies" "error case: bad answer refused"
expect "$(cb day answer "$qid" delivery)" "answered" "question answered"
# 5. A plugin adds its own explained suggestion; its task lands only when the owner accepts (issue #71).
cp -R "$FIX/plugin-acme" "$SCRATCH/plugin-acme"
cb plugin add "$SCRATCH/plugin-acme" >/dev/null
suggest=(day suggest --plugin acme --ref doc-7 --what "Review the synthetic roadmap doc" --why "the doc names you as a reviewer" --accept "Plans time to review it" --task "Review the synthetic roadmap doc")
expect "$(cb "${suggest[@]}")" "waits in the inbox" "plugin suggestion added"
refute "$(data_cat work/tasks.yaml)" "synthetic roadmap" "no task before the owner accepts"
pid="$(cb day inbox | awk '$3=="plugin" && !seen++ {print $2}')"
[ -n "$pid" ] || fail "plugin suggestion shown in the inbox"
expect "$(cb day accept "$pid")" "accepted" "plugin suggestion accepted"
expect "$(data_cat work/tasks.yaml)" "Review the synthetic roadmap doc" "accepting adds the plugin's task"
expect "$(cb day suggestions --plugin acme)" "accepted" "the plugin reads the decision back"
expect "$(cb "${suggest[@]}")" "already decided" "the same ref does not come back"
expect "$(cb day suggest --plugin nope --what x --why y --accept z 2>&1 || true)" "not installed" "error case: only an installed plugin suggests"
event_seen day.inbox.suggest
event_seen day.log
event_seen day.inbox
# the explicit start command, the settings table, and a reset to the default
expect "$(cb start --no-watch --no-web --no-refresh)" "for you:" "careerbrew start is the same one command"
expect "$(cb config show)" "work_end           18:00" "config show lists the saved value"
expect "$(cb config unset work_end)" "back to its default" "config unset"
expect "$(cb config show)" "work_end           17:30" "the default is back"
echo "ok US-52 daily log"
