---
name: careerbrew-privacy-guard
description: Keeps the user's personal and business data inside careerbrew's private folder. Runs leak scans, explains a blocked commit or refused output, adds sensitive terms, and refuses to paste private data into chat, issues, or public text. Use when a commit is blocked by the leak check, when the user asks "is this safe to share", "add a codename to sensitive terms", "why was --show refused", or before publishing anything. Don't use for normal feature work or for writing the LinkedIn block itself (use careerbrew-linkedin-profile).
---

# Privacy guard

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. Run `uv run careerbrew privacy scan --staged` (or without `--staged` for the whole repo). Report findings by path and term id only; never print a term.
2. A finding under `private/` means the file must be unstaged: `git restore --staged <path>`.
3. A term finding means the text must change: replace the term with a neutral phrase, then re-run the scan.
4. To protect a new name: append it to `private/sensitive-terms.txt` (one per line), or, for company names, to the company plugin's `sensitive-terms.txt`.
5. `--show` refused: business content prints only in the user's own terminal or when the company plugin sets `agent_may_read_business: true`. Never work around it.

Never read `private/work/` or `private/out/promo-*` to answer a question unless the plugin policy allows agents to read business data.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-privacy-guard/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
