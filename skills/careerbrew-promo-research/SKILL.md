---
name: careerbrew-promo-research
description: Researches a company's promotion criteria and turns them into a careerbrew ladder YAML. Uses public sources (progression.fyi, the company engineering blog, levels.fyi level names) or an internal ladder the user pastes, then validates it with `careerbrew ladder import`. Use when the user says "research promotion criteria", "what does it take to get to staff here", or "import our ladder". Don't use to write the promotion doc itself (use careerbrew-work-companion) or to store an internal ladder in this repo.
---

# Promotion research

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

**Gate for work data:** anything you read with your own tools (work chat, tickets, mail, links, internal docs) goes to your provider. Run this flow on work data only when the user's employer approved that provider for that data. If the user has not said so, ask first. The plugin policy field `agent_may_read_business` limits what careerbrew itself shows or writes for an agent (command output, watcher research, project memory notes).

1. Ask for the company and target level. Ask whether an internal ladder doc exists.
2. Public company: search progression.fyi and the engineering blog. Note each source URL.
3. Internal ladder: the content belongs in the corporate plugin repo (`careerbrew-<org>`, `provides.ladders`), never in this repo. Write it there, or to `private/ladders/` for a one-off import.
4. Map each criterion to a competency: id, name, weight (1–3, how much promo committees weigh it), effort (1–3), signals (tags and title keywords seen in evidence), expected evidence count per level, and how_to_close.
5. `uv run careerbrew ladder import <file>`. Fix every validation error it prints.
6. Run `uv run careerbrew work gaps --ladder <id> --level <level>`. Ask the user to sanity-check the top 3 gaps against what their manager says.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-promo-research/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
