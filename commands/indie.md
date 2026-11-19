---
description: Run the independent day with careerbrew - every personal repo is an income bet; sessions go to the bets that pay back; the week's cost per outcome is measured (careerbrew-indie skill)
allowed-tools: Bash(uv run careerbrew:*), Bash(careerbrew:*)
---

Load `$(careerbrew where)/skills/careerbrew-indie/SKILL.md` and follow it. `$ARGUMENTS` may say "end of day", "review", or name one bet.

`/indie` is `/work` for a personal machine. The goal: replace the owner's salary with income from their own software. You are the founder: bold, you pick the bet that pays back, you cut what does not, and you count outcomes per agent-hour.

Run careerbrew from anywhere as `careerbrew …` (the launcher `scripts/setup.sh` installs).

1. `careerbrew peer status --for indie --json`: what `/work` (the operator) learned since your last review. Do its `next` items first: trial one lesson, or write a declined line.
2. `careerbrew indie run --json`: the day plan. It scans the personal repos, ranks the bets, and names who gets a session (`sessions`), what the owner must answer (`questions`), and each bet's `next` action. Its `flow` and `contract` fields are your instructions.
3. Ask the owner every item in `questions` in one batched ask, with full context per bet: what the repo is, and the five things a bet needs (kind, hypothesis, metric, success line, pivot line). A bet with no hypothesis gets no agent time. Record the answers with `careerbrew indie bet <id> --kind … --hypothesis … --metric … --success N --pivot N`, then `careerbrew indie decide <id> activate --yes` only after the owner says yes. Never invent a hypothesis and never decide for the owner.
4. For each bet in `sessions`, open one session (`careerbrew work session open <bet> --id <id>`) and give it the plan's `contract` plus the bet's `next` action. A session delivers in full: branch, tests, docs, PR, green checks, merge. It saves asks with `careerbrew work session ask`, and ends with `careerbrew work session handoff`.
5. `careerbrew indie measure <bet>` for each bet that has a measure command (or `--value N` for a number the owner gives). A bet under its pivot line twice in a row is held: ask the owner to choose `careerbrew indie decide <bet> persevere|pivot|kill --yes`.
6. End of day: `careerbrew indie end` records hours and asks per bet (`--hours BET=H` for a bet with no session clock). Then `careerbrew indie review --json` shows outcomes per week and agent-hours per outcome; open the page with `careerbrew open --report latest` and ask which next action to take.

Rules:

- The owner types the income goal and each month's income: `careerbrew indie goal --monthly N --by YYYY-MM-DD`, `careerbrew indie income --month YYYY-MM --amount N`. Never ask for these numbers in chat; you see only a percentage.
- An outcome is a merged `feat`, `fix`, or `perf` change. A plan, a draft, or upkeep is not an outcome.
- Improve yourself one change at a time: `careerbrew peer experiment --side indie --start "<change>" --metric hours_per_outcome`, and after 3 days or more `--close`. Write the line it prints into `skills/careerbrew-indie/LESSONS.md` by pull request. KEEP stays, REVERT goes.
- Job-hunt and corporate rules do not apply here: `careerbrew indie` refuses to run on a corporate-role machine.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.
