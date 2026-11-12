---
name: careerbrew-job-hunt
description: Runs the job-hunt pipeline. It scans job boards, filter by criteria, propose Want scores, collect ratings, refit weights, and build the season strategy (learn, practice companies, target, dream). Use when the user wants a new job, asks "find me jobs", "which companies should I apply to", "rate these jobs", or "what is my job search plan". Don't use on a corporate-role machine, for interview practice itself (use careerbrew-interview-drill), or for questions about the current job.
---

# Job hunt

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

The constraint that shapes everything: each company gives about one interview loop per season. The order of applications matters more than their count.

1. **Criteria first.** If `private/jobs/criteria.yaml` or `sources.yaml` is empty, interview the user for: titles, minimum level, remote or locations, minimum salary, companies to exclude, and 10–30 companies to watch. Find each company's public ATS slug (Greenhouse, Lever, Ashby) from its careers page. Write them to `sources.yaml`.
2. `uv run careerbrew jobs scan`, then `uv run careerbrew jobs list --top 15`. Present the list with the top Want reasons.
3. **Ask for ratings** (1–5) on at least 8 postings, mixing high and low scores. Record each with `jobs rate`. Then run `uv run careerbrew jobs learn` and report the agreement before and after.
4. `uv run careerbrew jobs strategy`. Explain the phase, the gates, and the practice companies chosen for skill transfer. Gaps became drill items.
5. Keep the funnel honest: after each real interview stage, `uv run careerbrew jobs log <company> <tier> <stage>`.

Rules:
- Never apply or message anyone for the user. Draft only.
- Never suggest a dream company before the dream gate passes. Explain the cost: the loop is gone for the season.
- Each claim about fit cites the posting's skills or the Want features.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-job-hunt/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
