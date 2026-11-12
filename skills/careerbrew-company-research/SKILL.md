---
name: careerbrew-company-research
description: Researches a company's public interview loop, values, local presence, and remote policy, and writes a sourced knowledge/companies/<id>.yaml that passes scripts/check-sources.py. Every fact cites an official page first (careers site, interview guide, engineering blog), a well-known public guide second. Use when the user says "research how X interviews", "add X to the interview guides", or when `careerbrew interview` shows the shared default loop for a target company. Don't use for promotion criteria (use careerbrew-promo-research) or to guess a loop without sources.
---

# Company research

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. Search the company's careers site and official interview guide first. Open each page you cite. For pages that need JavaScript, follow the `careerbrew-browser-research` skill.
2. Build the loop from the shared step ids in `knowledge/interview/steps.yaml` only. Add 1–2 sentences of company notes per step.
3. Write the facts to a research batch file, `knowledge/research/interview-research-<batch>.yaml`, in the same shape as the existing batches. Each loop step, tip, `local_presence`, and `remote_policy` lists its source URLs. Put AI-in-interview rules in `knowledge/research/ai-policy-research.yaml`.
4. Never hand-edit `knowledge/sources.yaml` or `knowledge/companies/*.yaml`. `scripts/import-research.py` generates them, and a rerun drops hand edits.
5. Run `uv run python scripts/import-research.py`, then `uv run python scripts/check-sources.py`. Fix every FAIL line.
6. Facts you cannot source are left out. Say which ones in your report.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-company-research/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
