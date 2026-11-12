---
name: careerbrew-weekly-review
description: Runs careerbrew's self-learning loop once a week. Measures every outcome metric against pre-registered thresholds, explains KEEP, WATCH, and PIVOT, proposes the pivot actions, refits Want weights, and proposes memory clean-ups for approval. Use when the user says "weekly review", "how am I doing", "is this working", or on the weekly cadence. Don't use for a single drill or a single task.
---

# Weekly review

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. `uv run careerbrew review --html`, then `uv run careerbrew report latest` (the run prints only its page path and one summary line). Show the table: metric, value, status, pivot action. KEEP and no-data metrics are `done` lines, PIVOT metrics are blockers, and WATCH metrics are next items.
2. For each PIVOT, propose the pivot action as one concrete task with a date. Never quietly lower a threshold. A threshold change is a separate, explicit decision with a reason in `private/goals.yaml`.
3. Job hunt: if 5+ new ratings exist, run `uv run careerbrew jobs learn`. Then rebuild the strategy.
4. Work: compare promo coverage with last week (`gap_closure`). If flat, ask which competency the manager wants to see.
5. Propose memory and knowledge clean-ups in `private/memory/MEMORY.md` for the user to approve. Never rewrite silently.
6. Run `uv run careerbrew backup create`.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-weekly-review/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.

Team metrics (opt-in): if the user wants their team lead to see what careerbrew gains them, show the numbers first with `uv run careerbrew metrics weekly --dry-run`, then `uv run careerbrew config set metrics_share true` only after they agree. Only numbers leave, through agentbrew's `metrics share`; careerbrew sends nothing itself.
