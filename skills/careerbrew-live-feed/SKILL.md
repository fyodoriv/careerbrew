---
name: careerbrew-live-feed
description: Runs the generic live work feed in a loop for the user's workday. Each cycle pulls every configured source (chat, tickets, code host, mail, calendar, paging) with the agent's own tools, saves each raw reply to the source's inbox, then runs `careerbrew work live cycle`, which stores, triages, gives a verdict per event, lists next actions, and fails the cycle when a required source has missing, invalid or stale acquisition. Use when the user says "run the live feed", "watch my work tools", "start the live loop", "what came in", or a plugin command starts a live feed. Don't use to add a teammate (use careerbrew-work-companion), to plan the day (use careerbrew-day-planner), or for the background day watcher (use careerbrew-day-watch).
---

# Live feed

First, load `careerbrew-best-practices` (privacy, sources, machine role, never send, measure, iterate).

**Gate for work data:** anything you read with your own tools (work chat, tickets, mail, links, internal docs) goes to your provider. Run this flow on work data only when the user's employer approved that provider for that data. If the user has not said so, ask first. The plugin policy field `agent_may_read_business` limits what careerbrew itself shows or writes for an agent (command output, watcher research, project memory notes).

The cycle logic lives in careerbrew. A company plugin gives only its source config and roster ([docs/plugins.md](../../docs/plugins.md), "Live feed sources and roster"). Never copy hosts, channels, or names into this skill.

## Start

Read `uv run careerbrew work live status --json` first when checking source
freshness or missing Today/Standup work. Its `health` result separates coverage
from adapter running state. Healthy declared direct coverage does not require
starting stopped adapters. No-source, stopped, stale, error and unknown states
need their stated repair; do not invent a source or make one optional.

1. For adapter mode, run `uv run careerbrew work live start` (or `--sources <file.yaml>` when the user gives a config). It lists each source: kind, required or optional, its inbox or connector, and `since` (the cursor). For healthy direct mode, use the direct completion workflow below and leave stopped adapters stopped.
2. If it says "no live sources", ask the user for a source config or a plugin with live sources. Do not invent one.
3. If it says pull hosts and channels are hidden, ask the user which channels and hosts to pull, or to run it in their terminal. Never guess them.

## One cycle (at the interval the watch loop measures)

At each turn start while the recorded work-day loop is active, run `careerbrew day loop catch-up --json`. It runs a stored pass more than 5 minutes late even when a goal hook prevents idle-session cron delivery. It claims each due event once and uses the normal pass lock; never add a second timer. Pull any required source again when a caught-up cycle reports it missing.

The interval rule: a cycle takes minutes (about 18 in a real day), so a fixed "every 5 minutes" piles cycles up. Inside `/work`, the work-day watch loop runs `work live cycle` on each pass (`careerbrew day loop pass`) every 1.2 x the median measured pass, and prints the next re-arm time; never start a second timer for the feed. Outside `/work`, start the next cycle only after the last one ended, and wait at least as long as it took.

1. Pull: for each `file` source, call the agent tool for its kind with the cursor as the lower bound, and save the reply **exactly as returned** to its `inbox`, overwriting the last one. A quiet source still gets its empty reply saved (`{"messages": []}`): that is a stored, quiet source, not a gap. A `connector` source needs nothing from you.
   - A teammate search reads coworkers' messages and needs company policy approval: run it only for teammates the user added to the roster.
   - Send every search (each source, each teammate) in **one batch of parallel tool calls**, not one after another; they do not depend on each other. Then save all the replies and run one cycle.
   - A ticket search always gets bounds (an updated-after time, project keys, or issue keys). Use a relative cursor such as `updated >= -45m`, never a clock time: a time without a zone is read in the tracker's zone and misses new updates.
   - Save a calendar reply as returned (`{"events": [...]}`). careerbrew skips a record with no event id or no start time, because it has no stable id.
2. Run `uv run careerbrew work live cycle`. Read only its output: counts, verdicts, `not_stored`, and `next N.` lines. The `standup:` line says whether the cycle republished the Standup rows (a new event matched a row, the last publish was stale, or the standup is next); a `standup: failed` line is not a failed cycle: tell the user once, and rerun `careerbrew work standup` by hand if it repeats.
3. If it exits 1 with `not_stored  <source>`: the cycle failed. Pull those sources again, save the replies, and run the cycle again. Never report a failed cycle as quiet. After two failed tries for one source, tell the user which source and the error line.
An unchanged reply can remain healthy through its inclusive acquisition limit
(default 15 minutes; per-source `max_age_minutes`, or
`careerbrew config set live_source_max_age_minutes <1..10080>`). It is not a
new pull or store. Do not rewrite an old reply merely to renew its timestamp.
Pull with the real tool first, including a supported empty reply for quiet
sources. A `stale`, `unknown`, `missing` or `error` required source still needs
a successful pull. Never make it optional to hide the gap.

4. Act on the verdicts through the next-action lines:
   - `answer`: an open ask. Tell the user how many; titles go only to their terminal (`careerbrew work feed show --open --show`).
   - `read`: worth a look; mention the count.
   - `skip`: noise; say nothing.
   - `evidence`: reaches the PM draft (PM draft rule: your own action, marked evidence by a rule, not personal; title and link only). Offer `careerbrew day send --via email` for review. Never send without the user's yes.
   - `personal`: kept out of evidence and the PM draft. Say nothing about it.
   - links to research: follow `careerbrew work feed links --pending`, then `work feed research`.

## Listening contract (issue #266)

Listen from the start of the work day until wrap-up. Keep running cycles until the user wraps up, then stop (below). Each cycle already does the follow-up work for every new event and prints one `event N` line per event: its class (`needs you`, `worth knowing`, `noise`, `personal`), its verdict, and the actions taken.

1. **Reply drafts**: every open ask gets a draft at its target (`reply draft saved: <target>`), a holding reply or careerbrew's ack. Rewrite each one into a real reply: read the thread with your own tool, write the reply to a scratch file, then `careerbrew draft replace <target> --file <file>`. A `react :<emoji>:` line means an emoji fits, not a text. `draft kept` means the user already wrote one: leave it alone.
2. **Links**: the cycle opens links only when the user set `live_open_links` on; never turn it on yourself. A link with a summary is done. A link with a reason (`by: cycle`) stays in `careerbrew work feed links --pending`: open it with your own tools only when the gate for work data above allows it, and store what it says with `work feed research` (one entry for every link of the event).
3. **Reviews**: `review notes drafted: review:<digest>` holds a checklist and the link research. Do the review with your own tools and save your notes over it with `draft replace review:<digest> --file <file>`.
4. **Projects**: `project <name> updated` means the event was attached and the change recorded. Nothing more to do.
5. **Goals**: one `goal <id>:` line per goal for today. Tell the user what moved and what waits on them; names show only in their terminal.

**Draft only.** Never send, post, react, or approve anything yourself. Tell the user how many drafts wait and that they read each with `careerbrew draft show <target>` in their terminal, then send it themselves.

## Review sessions (issue #313)

When a cycle prints `N new review session(s)`, start one fresh-context reviewer per `review session opened: <key>` line, with no worktree. Give it only the PR link (`careerbrew work feed show --open --show` names it in your terminal) and these rules:

1. Review the PR with the review skill, and check each earlier review comment against the current diff.
2. Write the review to a local file. Never post it, never approve, never request changes.
3. Save a reply draft for the request's thread with `careerbrew draft replace <target> --file <file>`. Never send it.

A `review session kept` line means one is already open for that request: start nothing. The owner reads and sends.

## Status and stop

For direct agent-tool intake, run every configured tool, retain supported quiet
replies, store them through `work feed add` or `work feed transcript`, then
complete one `careerbrew work feed cycle`. The union of tool-map and required
live-source identities must be covered; `--require` can only add checks. An
arbitrary or partial batch cannot certify setup. A local batch is not proof of
fresh upstream fetching. Eligible own evidence reaches Today/done and Standup
without another connector collection. Personal items, noise and ordinary chat
do not become work done. A later required running-adapter error overrides an
older direct receipt; only a genuinely newer complete validation can recover.

Row facts show when the saved bundle was supplied, with a separate 60-minute
review policy. Stable titles and aliases do not expire at acquisition TTL.
Never renew row review or proof age by rendering a report or adding activity.

- `uv run careerbrew work live status` (`--json` for you): running or stopped, the last cycle, each cursor, next actions.
- `uv run careerbrew work live stop` at wrap-up, when the user stops work, or when they ask. Cursors stay, so the next start continues. Then delete the inbox files: they hold raw message text in plain form, outside careerbrew's encrypted store.

## Rules

- One cycle at a time: if the cycle says another one is running, wait for it; never delete the lock.
- Never paste a reply's message text into chat or a file in the repo. The inbox files are the user's scratch files; keep them in a scratch folder outside the repo, and overwrite each one every cycle.
- A source error prints in full only to the user's terminal; to an agent it prints its kind. Ask the user to run the cycle in their terminal when you need the detail.
- Never open `private/`. Learn about the feed from CLI output only.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

## Iterate

Append one dated line to `skills/careerbrew-live-feed/LESSONS.md`: what worked, what failed, and why. If the behavior should change, update this skill and add a case in `evals/cases/024-live-feed.yaml`.
