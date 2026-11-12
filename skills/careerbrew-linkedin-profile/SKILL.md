---
name: careerbrew-linkedin-profile
description: Produces the public-safe LinkedIn Experience block from the user's work results and helps polish its tone without adding confidential detail. Use when the user says "update my LinkedIn", "write my LinkedIn experience", "summarize my results for LinkedIn", or at quarter end. Don't use to apply for jobs, to post anything (careerbrew never posts), or for the internal promotion doc (use careerbrew-work-companion).
---

# LinkedIn profile

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

Make this block from work results only if the employer's confidentiality policy allows it; if the user has not said so, ask first. Agent-provider gate: before you read work data with your own tools, follow rule 1 of `careerbrew-best-practices`.

1. `uv run careerbrew work linkedin --role "<title>, <team>" --show`. The block already passed the public leak check.
2. Read the "review before posting" list aloud. For each suspect word, ask: is it internal? If yes, add it to sensitive terms and re-run.
3. Offer tone edits only: stronger verbs, one number per bullet if the user has it. Never add codenames, customers, internal tools, or numbers the policy forbids.
4. Remind the user to paste it themselves; careerbrew never posts.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-linkedin-profile/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
