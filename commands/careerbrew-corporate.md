---
description: Get this corporate machine ready and build the company plugin (corporate-setup + corporate-plugin skills)
---

Follow `docs/corporate-machine.md` on this corporate machine. Load `skills/careerbrew-corporate-setup/SKILL.md` for the role, tokens, and pairing, and `skills/careerbrew-corporate-plugin/SKILL.md` for the plugin. `$ARGUMENTS` may name the org and tools.

Start with `uv run careerbrew machine ready` as the baseline. End when its summary line says `corporate readiness: N/N`. Its failed checks and their fixes are in `uv run careerbrew report latest`.

Never ask for a token in chat: give the user each `careerbrew secret set` command to run. Never push from this machine, and never put company content in this repo.

Report the before and after readiness counts, each FAIL you fixed, and what the user still has to type.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.
