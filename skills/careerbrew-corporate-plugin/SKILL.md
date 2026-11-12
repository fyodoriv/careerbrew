---
name: careerbrew-corporate-plugin
description: Builds and fills the company plugin on a corporate machine, then proves the machine ready. Creates `careerbrew-<org>` with `plugin new`, fills the real ladder, sensitive terms, policy, evidence sources, and house rules, reinstalls it with `plugin refresh`, and runs `careerbrew machine ready` until it prints N/N. Use when the user says "build our company plugin", "make the plugin for my new job", "fill the ladder for my company", or "is my work laptop ready". Don't use for machine role, tokens, or pairing (use careerbrew-corporate-setup), for a personal machine, or to store company content in this repo.
---

# Corporate plugin

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).
The runbook is `docs/corporate-machine.md`, step 3. The proof is `uv run careerbrew machine ready`.
If the role is not corporate yet, run `careerbrew-corporate-setup` first (steps 0–2).

1. Baseline: `uv run careerbrew machine ready`. Note the count in its summary line and each blocker in `uv run careerbrew report latest`; each one names its fix.
2. Create: `uv run careerbrew plugin new <org> --connect <tools> [--jira-url <url>] [--github-user <login>]`. It refuses a non-empty folder; never force it.
   With no terminal, its run page lists one `careerbrew secret set careerbrew.p-<id>.<tool>` step per tool (`uv run careerbrew report latest`). Give those commands to the user to run. Never ask for a token in chat.
3. Fill the plugin folder (outside this repo):
   - `ladders/<org>-eng.yaml`: load `careerbrew-promo-research` and write the company's real levels and expectations. Keep the `generic-swe` shape and the `id`. Agent-provider gate: before you read work data with your own tools, follow rule 1 of `careerbrew-best-practices`.
   - `sensitive-terms.txt`: the company name, codenames, customers, internal hosts. At least two lines.
   - `careerbrew-plugin.yaml` `policy`: match the company AI and data policy. Stricter is safer.
   - `evidence.yaml` and `rules.md`: internal git hosts and short house rules.
   - A feed that proposes something to the owner (a plugin skill, command, or checkup step) runs `careerbrew day suggest --plugin <org> --ref <stable id> --what ... --why ... --accept ...` (add `--task` when Accept should add a task). Never add a task the owner did not ask for to reach the inbox. See `docs/plugins.md`, "Inbox suggestions".
4. Set `name` and `employer` in `profile.yaml`, and `target_level` in `settings.yaml` to a level of the new ladder. Both files are in the data root.
5. Commit inside the plugin folder, then `uv run careerbrew plugin refresh`. Readiness reads the installed copy, so refresh after every edit.
6. `uv run careerbrew machine ready`. Fix each blocker and rerun until its summary line says `corporate readiness: N/N`. Report the before and after counts.

Ask before you read an internal document into chat. Never weaken a policy, never push the plugin to a personal host, and never copy company content into this repo. If the repo-term check fails, move the named content into the plugin.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-corporate-plugin/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
