#!/usr/bin/env bash
# US-114: bet options. When the owner is not sure of a hypothesis, the skill shows five options that five judges
# all pass. This story checks the fixed parts: the skill's contract, and that an option becomes a bet only through
# the owner's own save and yes. Synthetic text only.
source "$(dirname "$0")/lib.sh"
export CAREERBREW_NOW="2026-11-19T10:00+00:00"
cb init >/dev/null
skill="$(cat "$ROOT/skills/careerbrew-bet-options/SKILL.md")"

# 1. The skill's contract: five named judges, only passing options shown, the owner picks.
for judge in "Demand judge" "Income judge" "Measure judge" "Solo judge" "Skeptic"; do
  expect "$skill" "**$judge**" "the skill names the $judge"
done
expect "$skill" "Never pad the list with a candidate that failed a judge" "error case: a failed candidate is never shown"
expect "$skill" "Never save or activate an option the owner did not pick" "the owner picks"
expect "$(cat "$ROOT/skills/careerbrew-indie/SKILL.md")" "careerbrew-bet-options" "the indie day points at the options skill"

# 2. An option is a draft: the bet stays a question until the owner's pick is saved and activated with a yes.
mkdir -p "$SCRATCH/apps/alpha"
git -C "$SCRATCH/apps/alpha" init -q
git -C "$SCRATCH/apps/alpha" -c user.name=t -c user.email=t@example.com -c commit.gpgsign=false commit -q --allow-empty -m "feat: a synthetic feature"
cb config set indie_roots "$SCRATCH/apps" >/dev/null
cb indie scan >/dev/null
expect "$(cb indie run --json)" '"questions": ["alpha"]' "no hypothesis yet: a question"
cb indie bet alpha --kind sponsors --hypothesis "CANARYOPTION teams fund the tool" --metric "sponsors" --success 5 --pivot 1 >/dev/null
expect "$(cb indie run --json)" '"questions": ["alpha"]' "a saved option is still a question until the owner activates it"
expect "$(cb indie decide alpha activate 2>&1 || true)" "add --yes after the owner says so" "error case: an agent cannot activate an option"
expect "$(cb indie decide alpha activate --yes)" "bet alpha: active" "the owner's yes activates the picked option"
plan="$(cb indie run --json)"
expect "$plan" '"questions": []' "the picked bet is no longer a question"
refute "$plan$(cb indie bet alpha --json)" "CANARYOPTION" "privacy: the picked hypothesis text never prints"
event_seen indie.decide
echo "ok US-114"
