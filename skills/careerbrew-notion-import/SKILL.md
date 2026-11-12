---
name: careerbrew-notion-import
description: Copies the user's Notion career workspace (career pages, jobs board, flashcards, skills, STAR stories, job search plan, past-job 1:1 notes) into careerbrew's private data with the Notion connector, read-only, then runs `careerbrew import notion`. Use when the user says "import my Notion", "merge my Notion career pages", "sync my flashcards from Notion", or after they edit those pages in Notion. Don't use to write anything to Notion, on a corporate-role machine, or for live two-way sync.
---

# Notion import

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

1. Check the role: `uv run careerbrew machine show`. On a corporate machine, stop: the import is personal-only.
   Run only on the owner's own machine. In a cloud or remote session (a container, not the owner's laptop), stop: fetched page text would sit on someone else's disk.
2. Find the roots with the Notion connector's search: the "Career" page and, if the user wants it, the past-job section (1:1 notes). Fetch each root and list its pages and databases. Count each database's rows.
3. Fetch read-only into `<data root>/imports/notion/`, one file per kind, in the row shapes of the example schema at the top of `src/careerbrew/notion_import.py`. Large databases go to parallel subagents, each with one output file. Save every 20–50 rows. Set `"truncated": true` when Notion skips blocks. Skip template rows (a page named like "Job template"): they are not jobs.
4. Write `manifest.json`: `{"source": "notion", "fetched_at": <now>, "root": "Career", "counts": {<kind>: <rows>}}`. The counts must equal the live Notion counts.
5. Run `uv run careerbrew import notion`, then `uv run careerbrew import notion --check`. It must print N/N.
6. Report the counts per kind and the truncated pages only. Never quote page content, names, or salaries in chat.
7. Never write to Notion. Never commit `private/`.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-notion-import/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
