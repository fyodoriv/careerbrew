---
name: careerbrew-start-session
description: Starts a careerbrew session. Checks the machine role, loads the profile, memory, and what is due in the active mode (job hunt or current job), then proposes one next step. Use when the user opens careerbrew, says "start careerbrew", "start", "what's next", or "let's begin". Don't use for a one-line factual question.
---

# Start a session

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

First, read the handoff the last session left: `uv run careerbrew day handoff show --json` (ids, sections, links). Treat every item as "verify live before acting": check its link or source first, and drop a done item with `day handoff done <id>`.

1. Offer the web app for browsing and planning: `uv run careerbrew open` (it starts the app if needed and opens the user's browser signed in; the token never prints, and tabs stay signed in across restarts; local only). Then run `uv run careerbrew machine show`. On a corporate machine, work mode only. Never mention job-hunt plans there.
2. Check profile and memory structure through `careerbrew data show <file> --keys`, not raw private-file reads. Use only CLI output allowed by the plugin privacy policy. If setup is missing, run `uv run careerbrew init` and ask for the three most important profile fields. When the owner supplies a name or employer, use `uv run careerbrew data set profile.yaml --name "<name>" --employer "<employer>"`; omit an option not supplied. Arguments may appear in local process listings and shell history; never use it for secrets.
3. For code work on careerbrew itself, run `scripts/open-work.sh` and continue any open PR or branch before anything new. Then run `uv run careerbrew review` and read `uv run careerbrew report latest --run review --json`. Any PIVOT blocker comes first: say the metric, the value, and the pivot action.
4. List what is due:
   - job hunt: `uv run careerbrew drill next` (due reviews) and the phase from `uv run careerbrew jobs strategy`;
   - work: `uv run careerbrew work plan` and evidence freshness.
5. Each long run (`review`, `work collect`, `day plan`, `careerbrew`) ends with a run page and prints only its path and one summary line. Read `uv run careerbrew report latest --json` (counts, ids, and commands; no business text), open the page for the user with `uv run careerbrew open --report latest` (never publish a page whose JSON says `"personal": true`), and ask which Next item to take, with the page's Next items as the options. Recommend ONE with the exact command and why it has the most value now. Wait for the user's choice.
6. At the end, record only reusable source-backed procedures through the shared-memory session protocol. Career records remain local; change them only through Careerbrew CLI actions under its privacy policy.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-start-session/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.

For required work-source freshness, run `careerbrew work live cycle` using the live-feed skill. A validated unchanged reply can be healthy within its acquisition limit. A missing, stale or invalid required reply needs a real tool pull, never a timestamp-only rewrite. The default limit is `careerbrew config set live_source_max_age_minutes <1..10080>` (15 minutes).

First inspect `careerbrew work live status --json` when the owner asks about
missing Today work or stale Standup inputs. Healthy declared direct coverage
can coexist with stopped adapters. Follow its fixed repair commands; run the
configured tools, retain quiet replies, store them, then complete one
`careerbrew work feed cycle`. Never treat a page render or new activity as a
review of old row facts or a new proof.
