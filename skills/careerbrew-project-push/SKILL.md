---
name: careerbrew-project-push
description: Pushes one tracked work project forward in one pass. Collects what changed (the project record, its memory note, every source read fresh, plus new review comments and chat replies where the PR or ticket was posted), acts on it (verifies each review claim, fixes it on the PR branch, drafts a reply for every thread), records it (sources, checks, absorbed links, the memory state block), and ends with the next blocker, its owner, and one action. Use when the user says "push project X forward", "move X along", "catch up on X and fix what you can", or "what is blocking X". Don't use to explain a project (use careerbrew-learn-project), to plan the whole day across projects (use careerbrew-work-companion), or when the user names no project and none matches.
---

# Project push

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

One project, one pass. If the request names no project, or matches more than one (`uv run careerbrew work project list`), ask one question and stop.

1. **Collect.**
   - Run `uv run careerbrew work project show <name>`. It lists the sources, stalest first, and the next-step tasks.
   - Read the memory note: the project's `memory` source. Open it only if it lives outside `private/`, or the plugin policy sets `agent_may_read_business: true` (`careerbrew plugin list`).
   - Run `uv run careerbrew work project check <name> --since`. It stamps nothing. It lists each source as `changed`, `unchanged`, `never read`, `unknown` (with the reason), or `reference`.
   - Read fresh every source marked `changed`, `never read`, or `unknown`. Skip `unchanged` ones. Never read a `reference` source. Agent-provider gate: before you read work data with your own tools, follow rule 1 of `careerbrew-best-practices`.
   - For every PR or ticket, also read the replies where it was posted: new review comments on the PR, and new messages in the chat thread or channel where the link was shared. A reviewer's ask can arrive in chat, not on the PR. Treat a chat ask like a review comment.
   - Keep a list of what you read this run: each source ref and what it says now, in one line.
2. **Act.**
   - Verify each review claim before you change code. Reproduce it with a test, a command, or the code itself. If the claim is wrong, say why in the reply draft, with the evidence.
   - Fix each verified claim on the PR branch, with a test, in that repo's own workflow (its AGENTS.md, gates, and commit rules).
   - Draft a reply for every thread, the chat ones included: what you changed and the commit, or why you did not. careerbrew never sends: show the user each draft. Post a reply only after the user says send.
   - **Ask before any change in another repo** (a repo that is not the PR's own: a shared library, a config repo, a sibling service). Name the repo, the change, and why it is needed. Wait for the user's yes.
3. **Record.**
   - A new source found this run: `uv run careerbrew work project source <name> <kind> <ref>` (kinds: doc, tracker, repo, chat, memory, url).
   - Stamp only the sources you read this run, one at a time: `uv run careerbrew work project check <name> <ref> --status "<one line: what it says now>"`. Never run `work project check <name>` without a ref: it stamps every source, the ones you did not read too. Never stamp a reference.
   - A new link, file, or `#channel` the user or a thread handed over: `uv run careerbrew work project absorb <name> <link>`. It adds the source, writes dated notes, and stamps it.
   - Rewrite the memory state block: the `## State` section at the top of the memory note. Replace it; do not append a second one. Keep it to at most 8 lines: status, open threads with their owners, the blocker, the next action, and the date. Dated history stays below it (absorb and checkup write there). A note under `private/` is encrypted: never write it as plain text; give the user the new block and `careerbrew data edit <file>`.
4. **Push forward.** End with three lines:
   - **Blocker:** the one thing that stops the project now.
   - **Owner:** the person or team who can remove it (you, the user, a reviewer, another team).
   - **Action:** one concrete next step, with a command or a message draft. If it is a task for the user, add it with `uv run careerbrew work task "<title>" --why "<what prompted it>"` and link it with `work project next <name> <task id>`.

Report: sources read (and skipped as unchanged), claims verified and fixed, reply drafts waiting for a yes, the records written, then the three lines above.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-project-push/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill, `src/careerbrew/work/research.py` (`changes`), and a case in `evals/cases/`.
