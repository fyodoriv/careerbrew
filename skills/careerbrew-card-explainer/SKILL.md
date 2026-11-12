---
name: careerbrew-card-explainer
description: Grows and improves the public study deck in knowledge/cards/. It writes a detailed, easy-to-follow explanation for every interview flashcard, keeps personal questions private, and rewrites cards when the prompt improves, a card fails the bar, or learners keep failing it. Use when the user says "explain my flashcards", "improve the study cards", "grow the public deck", or after a Notion import adds cards. Don't use to review cards (use careerbrew-interview-drill) or on a corporate-role machine.
---

# Card explainer

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. `uv run careerbrew cards explain-queue` shows how many cards need writing or rewriting and why.
2. Split the queue into batches of 40–90 cards by topic (`--topic`, `--offset`, `--count`). Large queues go to parallel subagents, one batch and one output file each. Every subagent reads `prompts/card-explainer.md` first and follows it exactly.
3. A subagent runs `uv run careerbrew cards explain-queue --json --topic <t> --offset <n> --count <k>` and writes its cards as a YAML list to `private/tutor/explain/out-<batch>.yaml` (save every 10 cards). It writes in its own words, never copies the hint, and marks personal questions `skip: true`. Personal means the owner's own employers, named people, internal projects, or the owner's own experience; a question that only names a well-known company (its published values, "Why X?") is public knowledge and gets a public card.
4. Run `uv run careerbrew cards publish private/tutor/explain/out-<batch>.yaml` per file. It lints every card against the bar and blocks personal data (your leak terms, name, employer); fix what it names and publish again. It deletes the file on success. A personal-data finding names a term id; reword so no word contains the term, even inside a longer word (the gate's leak scan matches inside words).
5. Personal questions come back as `"private": true` items: write them with `private: true` (never skip). They are saved encrypted in `private/tutor/private-explanations.yaml`, shown in Practice and in Library under "My personal questions, explained", and never reach `knowledge/cards/`.
6. Audit before and after: before publishing a generated batch, and after every large round, independent subagents audit line by line (list and check every claim, run every runnable example, run leetcode solutions on edge cases, recompute every number). They fact-check each topic file (claims correct and current, code runs, easy to follow, nothing personal) and write corrected cards with `rewrite: <id>`; publish them the same way. Count reviewed and corrected per topic.
7. `uv run careerbrew cards public-check` must report 0 problems. Run `scripts/check.sh fast`, then commit `knowledge/cards/` (public, reviewed).
8. Report counts only: published, skipped as personal, rewritten, left in the queue.

## Improving the cards

- Change `prompts/card-explainer.md` only to raise the bar (process notes go in this skill). Its hash is stored on every card, so the next `explain-queue` lists every card written with the old prompt.
- A card learners grade "again" 3 or more times comes back with "make it clearer".
- A reader's report ("Report a problem" in the web app) comes back first, with the reader's note as the reason; human review is the last word on quality, so treat every report as a defect to fix.
- `lint` in `src/careerbrew/tutor/public.py` holds the machine-checkable bar; add a rule there when a review finds a repeated defect.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-card-explainer/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
