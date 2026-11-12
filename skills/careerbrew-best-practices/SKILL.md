---
name: careerbrew-best-practices
description: The careerbrew house rules every other careerbrew skill loads first - privacy, sources and freshness, machine roles, never sending, measuring outcomes, and how to iterate at the end. Use when starting any careerbrew action, or when the user asks "how should careerbrew handle X". Don't use on its own for a task - load it, then follow the task's skill.
---

# careerbrew best practices

Load this first. Every other skill builds on it.

1. **Privacy.** Learn about private data from CLI output (aggregates, ids, paths). Do not open `private/work/` or `private/out/promo-*` unless the company plugin sets `agent_may_read_business: true`. Never paste private data into chat, issues, or public text. Run `careerbrew privacy scan --staged` before committing. **Agent-provider gate:** anything you read with your own tools (work chat, tickets, mail, links, internal docs) goes to your provider. Use a flow on work data only when the user's employer approved that provider for that data. If the user has not said so, ask first.
2. **Sources and freshness.** Every company fact, interview step, tip, AI policy, and strategy rule cites a source in `knowledge/sources.yaml` with a `checked` date. Prefer official pages. If a fact cannot be sourced, leave it out and say so. Facts older than 180 days are re-checked (`careerbrew research stale`).
3. **Machine role.** Job-hunt work happens only on a personal machine. On a corporate machine, do work-mode tasks only.
4. **Never send.** careerbrew drafts; the user sends, applies, posts, or replies.
5. **Measure.** Prefer a number from `careerbrew review` over an opinion. When a metric says PIVOT, propose the pivot action.
6. **Shared first.** Before company-specific advice, check what loops share: `careerbrew interview-patterns`.
7. **Iterate (end of every skill).** Append one dated line to the skill's `LESSONS.md`: what worked, what failed, and why. If the skill should behave differently next time, add or update a case in `evals/cases/` and change the skill text.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append to `skills/careerbrew-best-practices/LESSONS.md` when a rule above was missing or wrong, and propose the rule change to the user.
