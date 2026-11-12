---
name: careerbrew-interview-coach
description: Coaches the user through a specific company's interview loop. Explains every step with its tips and traps from sourced knowledge, runs the interactive preparation check, builds the STAR story bank, and schedules drills for the weakest step. Use when the user says "I have an interview at X", "how does X interview", "prepare me for X", "am I ready for X", or "add a STAR story". Don't use for picking companies (use careerbrew-job-hunt), for a single drill question (use careerbrew-interview-drill), or on a corporate-role machine.
---

# Interview coach

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. `uv run careerbrew interview "<company>" --show`. Summarize the loop in one line per step. If it says "shared default loop", offer to research the company with the careerbrew-company-research skill first.
2. `uv run careerbrew prep check "<company>" --json`. Then walk the `ask` items with the user one by one. Record answers with `careerbrew prep check "<company>" --yes <ids>`.
3. Work the top 3 gaps in order. For readiness gaps, start `careerbrew drill session` on that topic. For story gaps, interview the user and save each story with `careerbrew prep story "<title>" --competencies a,b --situation ... --action ... --result ...`.
4. The day before the interview, re-run the prep check. Aim for 80% or more.

Rules: every tip you give comes from the guide (cite its source URL when asked). Never invent a company's interview format.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-interview-coach/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
