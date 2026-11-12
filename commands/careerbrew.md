---
description: Start careerbrew (watcher, web app, refresh), show the run page, and ask what to do next
allowed-tools: Bash(uv run careerbrew:*)
---

Run `uv run careerbrew` from the careerbrew repo. It starts everything with sane defaults: the day watcher (it updates the daily log on every new change from your work sources), the local web app in the background, and one refresh. Instead of text, it ends with a run page: it prints only the page path and one summary line (open proposals and questions, today's counts). Today's counts, open proposals and questions, overdue tasks, and stale project sources are on the page.

When the owner supplies a name or employer for setup, use `uv run careerbrew data set profile.yaml --name "<name>" --employer "<employer>"` without an editor. Omit either option if it was not supplied. Arguments may appear in local process listings and shell history; never use this action for secrets.

Then, answer first:

1. Read `uv run careerbrew report latest --run start --json`. It holds counts, ids, statuses, and each Next item's exact command, and no business text. Do not open the page file yourself: it holds the user's own data.
2. Show the page: run `uv run careerbrew open --report start` (it opens in the user's browser, in the web app's Reports view). Never publish it as an Artifact.
3. In two or three lines: the web app address (the Done item `web`), today's counts (done today, planned for tomorrow), and the blockers (overdue tasks) with their fix.
4. Ask the user which next actions to take, with the page's Next items as the options (each option: its label and its command; offer the first four). Then act on the answer:
   - an open question: ask it in plain words, then apply the answer with `uv run careerbrew day answer <id> "<answer>"`. Never invent an answer.
   - an open proposal: say what it would change; apply it only when the user says yes (`uv run careerbrew day accept <id>`, or `day dismiss <id>`). Goal proposals are approved in the web app only: point the user to the Today view.
   - a stale project source: offer to read it (`careerbrew work project show <name>`).

**Gate for work data:** anything you read with your own tools (work chat, tickets, mail, links, internal docs) goes to your provider. Run this flow on work data only when the user's employer approved that provider for that data. If the user has not said so, ask first. The plugin policy field `agent_may_read_business` limits what careerbrew itself shows or writes for an agent (command output, watcher research, project memory notes).

Then work the promises the user made, in parallel with the rest (issue #268). `uv run careerbrew work promise list --json` lists each open one with its `next` step: `research` (read the thread, its links, the code), `draft` (write the follow-up message as a draft; never send it), or `status` (tell the user where it stands). Start one background agent per open promise (at most four at once), each doing only its promise's next step, then recording it with `uv run careerbrew work promise step <name> <research|draft|status> "<one line>"`. Show every draft to the user before anything is sent. When the user says a promise is kept, run `uv run careerbrew work promise done <name>`; a wrong detection is `uv run careerbrew work promise dropped <name>`. Titles show only when the plugin policy sets `agent_may_read_business: true`; without it, give the user the names and let them read the titles in the Dashboard's Commitments block.

Every other thing careerbrew can do is in its action catalog: `uv run careerbrew actions` (or `--json` for the arguments). Run any of them as the CLI command it names; the web app's Actions view runs the same list. Actions marked "confirm" change or send something hard to undo: run them only after the user says yes. "Terminal only" actions need the user's own terminal (hidden prompts): give them the command.

Long runs (`review`, `work collect`, `day plan`, `day render`, `checkup`, `machine ready`, `machine new-user`, `plugin new`) end the same way, with two lines instead of their step text: read `uv run careerbrew report latest --json`, open the page with `uv run careerbrew open --report latest`, and ask which Next item to take.

To change a default the user asks for, use `uv run careerbrew config set <key> <value>` (see `uv run careerbrew config`). The PM update goes to Slack by default: show the user the draft (`uv run careerbrew day send`; its "why it matters" notes are for the user only, never in the sent text), and post it with `--yes` only after they say send.

If careerbrew itself fails, surprises you, or could clearly work better, offer once to report it to its author, who is glad to get every report. Follow the `report-upstream` skill (CONTRIBUTING.md "Report problems and ideas"): never include anything from `private/`, scrub with `agentbrew report --repo fyodoriv/careerbrew` (no `--yes`), show the user the safety checklist and the exact text, and post only after they say yes. Offer a fix PR only after a second yes.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.

For required work-source freshness, use `careerbrew work live cycle`. Validated unchanged replies remain healthy up to their acquisition limit; stale, missing or invalid required replies need a real pull. Use `careerbrew config set live_source_max_age_minutes <1..10080>` to set the default limit (15 minutes). Never rewrite old replies only to renew them.

When asked whether sources are current, why Today is empty, or why Standup
warns about old inputs, run `careerbrew work live status --json`. For declared
direct intake, pull the configured tools, retain quiet replies, store with
`work feed add` or `work feed transcript`, then complete `careerbrew work feed cycle`.
Its required set cannot be shrunk by `--require`. Healthy direct coverage does
not require starting stopped adapters. Row facts and proofs have separate ages;
do not refresh their review time merely by rendering a report.
