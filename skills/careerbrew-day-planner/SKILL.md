---
name: careerbrew-day-planner
description: Plans the user's workday from facts and proposes the next task through the day. Reads meetings from Outlook or ICS calendars, ranks open tasks by deadline, impact, size, and promotion gaps, inflates estimates by the user's real estimate ratio, and time-blocks free slots. Use when the user says "plan my day", "what should I do next", "what now", "rank my tasks", or opens the laptop in the morning. Don't use for long-term career strategy (use job-hunt or work-companion) or on weekends unless asked.
---

# Day planner

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

Morning:
1. `uv run careerbrew day plan`, then `uv run careerbrew report latest` (the run prints only its page path and one summary line). Read back the plan in one short list from its `done` lines: time block, task id, reason. Name meetings and focus minutes (the summary line).
2. If tasks were left out (a "No room today" next item), ask whether to split the biggest one, or move it to tomorrow.
3. If a calendar blocker appears, give the fix (store the ICS link with `careerbrew secret set`, or reconnect Outlook).

During the day, when asked "what next" (or when the watcher notifies):
1. `uv run careerbrew day next`. Say the suggestion and its reason in two lines.
2. If the user picks something else, accept it; do not argue twice.
3. After a task ends: `uv run careerbrew work done <id> <actual hours>`. That keeps calibration honest.

When the user asks why a suggestion ranks low, or whether careerbrew learns from their choices, run `uv run careerbrew signals`: each line names a decision pattern and what it changes now (see docs/feedback-signals.md).

Rules: plan only inside work hours from settings.yaml; never add meetings; never invent tasks. Add one with `careerbrew work task` only after the user agrees.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-day-planner/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
