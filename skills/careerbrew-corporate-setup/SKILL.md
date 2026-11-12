---
name: careerbrew-corporate-setup
description: Sets careerbrew up on a corporate computer safely. Sets the machine role, installs the company plugin, connects Slack, Google Docs, Jira, GitHub, and Zoom with tokens in the Keychain or 1Password, and pairs with the user's personal machine. Use when the user says "set up on my work laptop", "connect jira/slack/github/zoom/google docs", "install our company plugin", or "sync my machines". Don't use on a personal machine for job hunting, or to bypass a company policy.
---

# Corporate setup

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

A second person at the same company (the plugin already exists): clone, block pushes, `scripts/install.sh --role corporate`, then `uv run careerbrew machine new-user --plugin <plugin>`. It ends with a run page: relay the token steps from `uv run careerbrew report latest` (each `Type your <tool> token` item and its command); the user types each token. Done when its summary line says `corporate readiness: N/N`. Report careerbrew bugs with `gh issue create -R fyodoriv/careerbrew` (the clone is pull-only), only if company policy allows it, and only after `careerbrew privacy scan --file <draft>` passes on the draft text; else file it from your personal machine.

0. Follow `docs/corporate-machine.md` for getting the code (clone or git bundle) and blocking pushes from this machine.
1. `uv run careerbrew machine show`, then `uv run careerbrew machine set corporate`. If it refuses because of job-hunt data, the user first moves that data to the personal machine. Only when this install holds no employer data (no `work/`, `day/`, `ladders/`, `plugins/` content): `careerbrew backup create`, then restore on the personal machine. A backup holds the whole data folder, so never move one that also holds employer data; careerbrew has no selective export, so then the user copies only their own job-hunt records by hand, or deletes them here.
2. Install the company plugin: `uv run careerbrew plugin add <url-or-folder>`. Show `plugin list` and explain the effective policy in plain words. Add `--allow-exec` only if the user has read the plugin's command sources.
3. Connect each tool the policy allows. The user types each token into the hidden prompt in their own terminal; never ask for a token in chat:
   - `uv run careerbrew connect github --user <login> [--api https://<ghe>/api/v3]`
   - `uv run careerbrew connect jira --base-url <url> --email <email>` (Jira always needs the email)
   - `uv run careerbrew connect slack`, `connect gdocs`, `connect zoom`
   If the company mandates 1Password: `--ref op://<vault>/<item>/<field>`.
   Each `connect` prints where to create the token and, where the tool has a who-am-I endpoint, checks it there (a tool with none says `no check endpoint`: the first collect is its check). A FAIL line means nothing was saved: relay its fix. A scoped Atlassian token moves to the `api.atlassian.com` gateway by itself.
4. `uv run careerbrew work collect` and report the counts from its summary line and any errors with their fixes (the blockers in `uv run careerbrew report latest`).
5. If no company plugin exists yet, load `careerbrew-corporate-plugin` and follow it. It builds, fills, and refreshes the plugin.
6. Run `uv run careerbrew machine ready`. Fix each blocker in `uv run careerbrew report latest` until its summary line says N/N.
7. Pair with the personal machine only if the user wants settings (ladder, target level) to come over: the user runs `careerbrew pair init` in their own terminal and stores the key in a password manager, then `pair join` on the other machine. Say plainly that careerbrew never copies corporate data to your personal machine (ADR 0007); keep the data folder and backups out of iCloud, OneDrive, Dropbox, and Google Drive.

Ask before you read internal documents into chat. Never weaken a policy, never paste tokens into files or chat, and never suggest job-hunt work on this machine.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-corporate-setup/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
