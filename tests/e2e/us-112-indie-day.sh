#!/usr/bin/env bash
# US-112: independent mode. Each personal repo is an income bet: no hypothesis means a question, not work; sessions
# go to the bets whose work pays back; a bet under its pivot line is held; the week's cost per outcome is measured.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-11-19T10:00+00:00"
cb init >/dev/null

repo_git() { # repo_git <repo> <git args>
  git -C "$SCRATCH/apps/$1" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false "${@:2}"
}
commit() { # commit <repo> <iso time> <subject>
  GIT_AUTHOR_DATE="$2" GIT_COMMITTER_DATE="$2" repo_git "$1" commit -q --allow-empty -m "$3"
}
for r in alpha beta gamma; do
  mkdir -p "$SCRATCH/apps/$r"
  repo_git "$r" init -q
  printf '# Tasks\n\n## P1\n\n- [ ] A blocked one\n  - **ID**: blocked-1\n  - **Blocked**: owner step\n\n- [ ] The first open one\n  - **ID**: open-%s\n' "$r" >"$SCRATCH/apps/$r/TASKS.md"
  repo_git "$r" add -f TASKS.md
done
commit alpha "2026-11-18T15:00+00:00" "feat: a synthetic feature"
commit alpha "2026-11-18T16:00+00:00" "fix: a synthetic fix"
commit alpha "2026-11-18T17:00+00:00" "docs: upkeep is not an outcome"
commit beta "2026-11-18T15:00+00:00" "chore: upkeep only"
commit gamma "2026-11-10T15:00+00:00" "perf: a synthetic speedup"
cb config set indie_roots "$SCRATCH/apps" >/dev/null

# 1. Every repo becomes a bet with no agent time until the owner says how it leads to income.
expect "$(cb indie scan)" "bets: 3 (3 new, 0 with no folder)" "scan proposes one bet per repo"
expect "$(cb indie bets)" "bets: 3, 0 active, 3 waiting for a hypothesis" "a new bet waits for its hypothesis"
plan="$(cb indie run --json)"
expect "$plan" '"sessions": []' "error case: a bet with no hypothesis gets no session"
expect "$plan" '"questions": ["alpha", "beta", "gamma"]' "each one becomes a question for the owner"
expect "$plan" '"coverage": {"repos": 3, "with_next_action": 3}' "every repo has a next action"
event_seen indie.run

# 2. The owner types the goal and the income; agents only ever see a percentage.
expect "$(cb indie goal)" "goal: not set" "no goal yet"
cb indie goal --monthly 43217 --by 2027-12-31 >/dev/null
out="$(cb indie income --month 2026-10 --amount 4321.7)"
expect "$out" "10.0% of the target" "income shows as a share of the target"
refute "$out$(cb indie goal --json)$(cb indie run --json)$(cb indie review --json)" "4321" "privacy: no amount leaves"

# 3. Only the owner activates a bet, and only with a full hypothesis.
expect "$(cb indie decide alpha activate --yes 2>&1 || true)" "lacks kind, hypothesis, metric, success, pivot" "error case: a bet with no hypothesis cannot start"
for r in alpha beta gamma; do
  cb indie bet "$r" --kind product --hypothesis "CANARYBET people pay for $r" --metric "weekly users" --success 10 --pivot 3 >/dev/null
done
expect "$(cb indie decide alpha activate 2>&1 || true)" "add --yes after the owner says so" "error case: an agent cannot decide for the owner"
for r in alpha beta gamma; do expect "$(cb indie decide "$r" activate --yes)" "bet $r: active" "the owner activates $r"; done
refute "$(cb indie bet alpha)$(cb indie bet alpha --json)$(cb indie run --json)" "CANARYBET" "privacy: a hypothesis never reaches an agent"

# 4. Sessions go to the bets whose work pays back.
cb config set work_session_cap 2 >/dev/null
plan="$(cb indie run --json)"
expect "$plan" '"sessions": ["alpha", "gamma"]' "untried bets: the one with recent outcomes first"
expect "$plan" '"id": "open-alpha"' "the next action is the first unblocked task"
refute "$plan" '"id": "blocked-1"' "a blocked task is never the next action"
expect "$(cb work session open alpha --id sess-alpha)" "session sess-alpha opened" "the bet's session is a project session"
cb work session ask sess-alpha "Ship the synthetic change?" >/dev/null
cb work session handoff sess-alpha "Shipped the synthetic change; next: measure" >/dev/null
export CAREERBREW_NOW="2026-11-19T12:00+00:00"
expect "$(cb indie end --hours beta=2)" "indie day recorded: 2 bet(s), 4.0 agent-hour(s), 1 ask(s)" "the day's hours and asks are recorded"
plan="$(cb indie run --json)"
expect "$plan" '"sessions": ["gamma", "alpha"]' "the untried bet is tried, then the one that paid back; the one that did not waits"
event_seen indie.end

# 5. A bet under its pivot line twice in a row is held until the owner decides.
expect "$(cb indie measure alpha --value 12)" "bet alpha: KEEP" "a reading past the success line"
cb indie measure gamma --value 1 >/dev/null
expect "$(cb indie measure gamma --value 2)" "held until the owner decides" "two readings under the pivot line"
plan="$(cb indie run --json)"
expect "$plan" '"sessions": ["alpha", "beta"]' "the held bet loses its session"
expect "$plan" "careerbrew indie decide gamma persevere|pivot|kill --yes" "the plan asks the owner to decide"
expect "$(cb indie decide gamma kill --yes)" "bet gamma: killed" "the owner ends the bet"
event_seen indie.measure

# 6. The weekly review: outcomes and what each one cost.
review="$(cb indie review --weeks 2 --json)"
expect "$review" '"week_ending": "2026-11-19", "outcomes": 2, "agent_hours": 4.0, "asks": 1, "hours_per_outcome": 2.0, "asks_per_outcome": 0.5' "this week: 2 outcomes, 2 agent-hours each"
expect "$review" '"agent_hours": null' "a week with no recorded hours is no-data, never zero"
expect "$(cb indie review)" "indie-review: " "the review ends with a run page"
expect "$(cb report latest)" "Week to 2026-11-19: 2 outcome(s)" "the page lists each week"
expect "$(cb indie run)" "indie-day: " "the day ends with a run page"
event_seen indie.review
echo "ok US-112"
