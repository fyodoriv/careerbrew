---
name: careerbrew-day-watch
description: Turns the background day watcher on, off, paused, or resumed, and explains its status. The watcher appends finished work and meetings to the results-of-the-day list, checkpoints after every source, and posts a macOS notification only when the next-task suggestion changes, plus opt-in notifications about important new events (pages, DMs, review requests, suggestions). Use when the user says "track my day", "start watching", "pause tracking", "stop the watcher", "is the watcher running", "what did I do today", "tell me when someone pages me", or "too many notifications". Don't use to plan the day (use careerbrew-day-planner) or to collect evidence once (use careerbrew-work-companion).
---

# Day watch

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

- Start: `uv run careerbrew day watch on`. Say it runs every 5 minutes inside work hours, backs off when nothing changes, and can be paused any time.
- Pause before a screen share or a focus block: `uv run careerbrew day watch pause`. Resume: `... resume`. It continues from the last checkpoint.
- Stop: `uv run careerbrew day watch off`.
- Status: `uv run careerbrew day watch status`. Report state, last cycle CPU and memory, and today's counts. If CPU per cycle is over 500 ms or memory over 120 MB, file a task with the numbers.
- End of day: `uv run careerbrew day results --show` in the user's own terminal (titles are business data). Offer to turn the best result into evidence with `careerbrew work note`.

## Notifications about important new events

- Turn on only when the user asks: `uv run careerbrew config set notify_events true`. Then `uv run careerbrew notify status` and report the rules (priority threshold, quiet hours, daily cap) and how pop-ups show here (alerter, terminal-notifier, or osascript).
- Too many: raise `notify_min_priority` (5: pages only), widen `notify_quiet_hours`, lower `notify_daily_cap`, or raise `notify_batch_minutes`. Too few: lower `notify_min_priority` to 3. Change one setting at a time with `careerbrew config set`, and say what it changes.
- Clickable pop-ups need `brew install terminal-notifier` (a click opens careerbrew) or alerter (buttons). Give the command; the user installs it.
- What is open: `uv run careerbrew notify list` (no titles to you). Accept or dismiss a suggestion with `careerbrew notify act <id> accept|dismiss` only when the user says so.

Never start the watcher without the user's request. Never install it as a login item unless the user asks.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-day-watch/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/`.
