#!/usr/bin/env bash
# US-107: one long-lived session per project, talking through careerbrew: claims, one question queue, a cap, and one handoff per project (issue #274).
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-10-28T09:00+00:00"
cb init >/dev/null
for p in billing ledger search; do cb work project add "$p" >/dev/null; done
cb config set work_session_cap 2 >/dev/null

# 1. One session per project, reused all day, under the cap.
expect "$(cb work session open billing --id sess-a1)" "session sess-a1 opened and recorded as a session loop" "the agent opens a project's session"
cb work session open ledger --id sess-b2 >/dev/null
expect "$(cb work session open billing)" "session sess-a1 reused" "later the same day the project gets its live session back"
out="$(cb work session open billing --id sess-two 2>&1 || true)"
expect "$out" "already has a live session today" "error case: a second session for one project"
expect "$out" 'careerbrew work session handoff sess-a1 "<status and next step>"' "the conflict explains how to record a handoff"
expect "$out" "careerbrew work session end sess-a1" "the conflict gives the exact end command"
out="$(cb work session open search --id sess-search 2>&1 || true)"
expect "$out" "2 of 2 sessions are live (work_session_cap)" "error case: a session past the cap"
expect "$out" "breaks US-107" "the refusal names the story"
event_seen work.session

# 2. Sessions write through the store.
expect "$(cb work agent event billing-bot "CANARYSESSION drafting the release reply" --session sess-a1)" "event stored: billing-bot running (run sess-a1)" "a session's step is an agent event under its project"
cb work project update billing project "Release moved to Friday" >/dev/null || fail "a session's project change is a project update"
cb work project update ledger approval "Synthetic PR pending" >/dev/null || fail "a session can label an approval note"
expect "$(cb work project rules-set billing --yes --text 'SYNTHETIC-RULE-438')" "standing rules saved" "a project can save session rules"
out="$(cb work project rules billing --json 2>&1 || true)"
expect "$out" "business data" "restricted session output refuses the rule"
refute "$out" "SYNTHETIC-RULE-438" "restricted output does not leak the rule"
expect "$(cb work project rules-set billing --yes --clear)" "standing rules saved" "a project can clear its rules"
expect "$(cb work project list --json)" '"kind": "approval"' "the latest project update exposes its approval kind"
expect "$(cb work session ask sess-a1 'Send the synthetic release reply?' --option send --option hold --pick hold --recheck true)" "saved for session:sess-a1" "a session saves its question for the batched ask"
cb work session ask sess-b2 'Merge the synthetic ledger PR?' --recheck false >/dev/null

# 3. Claims: the second session skips a claimed draft.
printf 'Looks good, approved.\n' >"$SCRATCH/draft-a.txt"
printf 'Please rename the flag.\n' >"$SCRATCH/draft-b.txt"
expect "$(cb work claim take slack-draft:C0SYN/1700000000.000100 --session sess-a1)" "claimed slack-draft:C0SYN/1700000000.000100 for session sess-a1" "a session claims the shared draft before it writes"
expect "$(cb draft replace C0SYN/1700000000.000100 --file "$SCRATCH/draft-a.txt" --session sess-a1)" "draft saved and read back" "the holder writes the draft"
set +e
out="$(cb draft replace C0SYN/1700000000.000100 --file "$SCRATCH/draft-b.txt" --session sess-b2 2>&1)"; rc=$?
set -e
[ "$rc" = 1 ] || fail "a write to another session's claimed draft must exit 1 (got $rc)"
expect "$out" "claimed by session sess-a1" "error case: the other session skips the claimed draft"
expect "$(cb work claim take slack-draft:C0SYN/1700000000.000100 --session sess-b2 2>&1 || true)" "skip it" "error case: another session cannot take a held claim"
expect "$(cb work claim list)" "claims: 1 held" "the claims list"
refute "$(cb work claim list)" "C0SYN" "privacy: an agent never sees a claimed resource"
event_seen work.claim
expect "$(cb work claim release slack-draft:C0SYN/1700000000.000100 --session sess-a1)" "released" "the holder releases it"
expect "$(cb work claim release slack-draft:C0SYN/1700000000.000100 --session sess-a1 2>&1 || true)" "holds no claim" "error case: releasing twice"

# Project sessions use one canonical claim for each GitHub PR.
expect "$(cb work claim take 'pr:Example/Tools#17' --session sess-a1)" "claimed pr:example/tools#17" "the session claims a canonical PR"
expect "$(cb work claim take 'pr:example/tools#17' --session sess-b2 2>&1 || true)" "skip it" "another session skips the claimed PR"
expect "$(cb work claim release 'pr:EXAMPLE/TOOLS#17' --session sess-a1)" "released" "the holder releases the same PR"

# 4. The main session's one view and one ask.
out="$(cb work session list)"
expect "$out" "sessions: 2 of 2 live" "the main session reads every session in one list"
refute "$out" "billing" "privacy: an agent never sees a project name"
refute "$(cb work session list --json)" "CANARYSESSION" "privacy: the JSON hides event titles"
expect "$(cb day question recheck)" "questions: 1 to ask, 1 dropped as closed" "every session's questions are in the one batched ask"

# 5. End of day: one handoff per project, then archive.
out="$(cb work session end --all 2>&1 || true)"
expect "$out" "write its handoff first" "error case: a session with no handoff stays open"
expect "$(cb work session handoff sess-a1 'Reply drafted; next: the owner sends it')" "handoff written for session sess-a1" "a session writes its project's handoff"
cb work session handoff ledger 'PR closed; nothing left' >/dev/null
expect "$(cb day handoff show --today)" "Project sessions (2)" "one handoff item per project"
expect "$(cb day loop end 2>&1 || true)" "careerbrew work session end sess-a1" "end of day names each session's end command"
expect "$(cb work session end --all)" "sessions: 2 ended, 0 still open" "end archives every session that wrote its handoff"
expect "$(cb work session list --all)" "ended" "the archive keeps ended sessions"
expect "$(cb day loop end)" "0 loop(s) running" "no session loop outlives the day"
out="$(cb day run --json)"
expect "$out" "runs only the work process" "the day plan carries the session contract"
expect "$out" "full scope" "sessions read full tickets, not only their brief"
expect "$out" "one at a time, in order" "sessions deliver tasks in order"
expect "$out" "shipped and verified" "a plan alone is not delivery"
expect "$out" "foreseeable approvals" "sessions batch approvals and continue independent work"
expect "$out" "each scope item marked done or not done" "reports keep unfinished scope visible"
export CAREERBREW_NOW="2026-10-29T08:00+00:00"
expect "$(cb day handoff show)" "Project sessions (2)" "the next morning starts from each project's handoff"
# Sent Tomorrow items get coverage even without ranked projects (#331).
export CAREERBREW_NOW="2026-11-02T09:00:00-04:00"
uv run python - "$SCRATCH/sent-pm.txt" <<'PM_PROMISES'
import sys
from pathlib import Path
from careerbrew.day import pm_learn
pm_learn.remember("2026-10-30", "Today", "Today")
Path(sys.argv[1]).write_text("Today\n- Delivered prototype\nTomorrow\n- Deliver billing export\n- Review RFC\n- Share hiring notes\n")
PM_PROMISES
expect "$(cb day pm-sent --day 2026-10-30 "$SCRATCH/sent-pm.txt")" "learned" "sent Tomorrow bullets are recorded"
expect "$(cb work promise list)" "promises: 3 open" "every sent bullet becomes a promise"
cb day pm-sent --day 2026-10-30 "$SCRATCH/sent-pm.txt" >/dev/null
expect "$(cb work promise list)" "promises: 3 open" "repeated saves create no duplicates"
for heading in "Tomorrow (Mon)" "Tomorrow - Monday" "Next week"; do
  uv run python - "$SCRATCH/sent-pm.txt" "$heading" <<'PM_HEADING'
import sys
from pathlib import Path
Path(sys.argv[1]).write_text(
    f"Today\n- Delivered prototype\n{sys.argv[2]}\n- Deliver billing export\n- Review RFC\n- Share hiring notes\nRisks\n- Do not record this\n"
)
PM_HEADING
  cb day pm-sent --day 2026-10-30 "$SCRATCH/sent-pm.txt" >/dev/null
  expect "$(cb work promise list)" "promises: 3 open" "named future section records each promise once"
  uv run python - <<'PM_HEADING_CHECK'
from careerbrew.work import promises

assert {row["title"] for row in promises.open_list()} == {
    "Deliver billing export", "Review RFC", "Share hiring notes"
}
PM_HEADING_CHECK
done
out="$(cb day run --json)"
expect "$out" "pm_promises" "the plan exposes promise coverage"
refute "$out" "Deliver billing export" "restricted plan keeps promise text private"
uv run python - "$out" <<'PM_COVERAGE'
import json, sys
from careerbrew.day import log
from careerbrew.work import sessions
from careerbrew.workday import plan
from datetime import date
public = json.loads(sys.argv[1])["pm_promises"]
assert not public["missing"] and len(public["items"]) == 3
assert all(r["due"] == "2026-11-02" for r in public["items"])
row = plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]
sessions.open_session(row["session_project"], "pm-session")
assert plan.build(date(2026, 11, 2))["pm_promises"]["items"][0]["covered"]
text = "\n".join(log.pm_lines({"day": "2026-11-02", "done": {}, "details": {}}, learn=False))
assert all(t in text for t in ("Deliver billing export", "Review RFC", "Share hiring notes"))
PM_COVERAGE
# A quiet project stays out of new sessions until its return date.
cb work project add quiet-test >/dev/null
cb work project set quiet-test --quiet-until 2026-11-04 >/dev/null
expect "$(cb work project list)" "quiet until 2026-11-04" "the saved quiet date is visible"
expect "$(cb work session open quiet-test --id quiet-session 2>&1 || true)" "project is quiet" "a quiet project gets no new session"
cb work project set quiet-test --quiet-until none >/dev/null
expect "$(cb work session open quiet-test --id quiet-session)" "session quiet-session opened" "clearing the date resumes the project"
# The next day's opened session can resume its prior-day handoff without leaking it in restricted output.
cb config set work_session_cap 3 >/dev/null
cb work project add resume-test >/dev/null
cb work session open resume-test --id prior-session >/dev/null
cb work session handoff prior-session 'SYNTHETIC-SESSION-RESUME' >/dev/null
cb work session end prior-session >/dev/null
export CAREERBREW_NOW="2026-11-03T09:00:00-04:00"
out="$(cb work session open resume-test --id resumed-session --json)"
refute "$out" "SYNTHETIC-SESSION-RESUME" "restricted output hides the prior handoff"
uv run python - <<'PRIOR_HANDOFF'
from careerbrew.work import sessions
assert sessions.open_session("resume-test", "resumed-session")["brief"] == "SYNTHETIC-SESSION-RESUME"
PRIOR_HANDOFF
echo "ok US-107 project sessions"
