---
name: careerbrew-browser-research
description: Gathers fresh, sourced facts about companies, their hiring process, interview steps, and AI-in-interview policies with a real browser, then turns them into careerbrew knowledge that passes the sources gate. Follows the playwright-best-practices and agent-browser skills for reliable page reading. Use when the user says "research how X hires", "is X's interview info up to date", "what AI can I use at X", when `careerbrew research stale` lists sources, or when `careerbrew jobs interview-gaps` lists companies. Don't use to scrape logged-in sites, to get past bot checks or CAPTCHAs, or for promotion criteria (use careerbrew-promo-research).
---

# Browser research

1. Load `careerbrew-best-practices`. Then load `playwright-best-practices` and `agent-browser` (or the built-in browser skill) for how to drive the browser well: read by role and text, wait for content rather than sleeping, take a snapshot before acting.
2. Pick the targets: `careerbrew research stale` (old sources, pipeline first) or `careerbrew jobs interview-gaps` (companies without a loop).
3. For plain HTML pages, try `careerbrew research page <url>` first: it records the text hash and `fetched_at`, and flags changes. Use the browser only when the page needs JavaScript.
4. In the browser, open the official careers or interview page first, then the engineering blog, then well-known guides. Read the text with `get_page_text` or a snapshot. Never sign in, never bypass a bot check, never use cookies from the user's accounts.
5. Write facts to `knowledge/research/interview-research-<batch>.yaml` (companies: loop steps from the shared step ids, notes marked official or third-party, tips, values, sources) and AI policies to `knowledge/research/ai-policy-research.yaml`, each with `checked: <today>`.
6. Run `uv run python scripts/import-research.py` and `uv run python scripts/check-sources.py`. Fix every FAIL line. Then `careerbrew changes` so plans pivot on what changed.
7. Report what changed since the last check, and what could not be verified.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-browser-research/LESSONS.md` per run: which sites blocked automation, which selectors or pages worked, how long it took. If a site keeps failing, add it to the "known hard sites" list below and add an eval case.

Known hard sites (from past runs): Glassdoor and Indeed show bot checks (skip them); Medium and some company blogs need the browser rather than a static fetch.
