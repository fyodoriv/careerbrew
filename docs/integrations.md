# Integrations and machines

## Connectors

careerbrew reads your own activity from work tools and turns it into evidence (metadata only: title, date, link, tags). Evidence never holds message bodies or document text. The work feed is different: it keeps each event's original, a message's text included, encrypted for `feed_keep_days` days (default 30, see [Work feed](#work-feed)).

`careerbrew connect <tool>` is guided: it prints where to create the token, what it looks like, and its scope; asks for missing fields (Jira needs your email); for Jira, GitHub, Outlook and PagerDuty it checks the token on a who-am-I endpoint (for example Jira `/rest/api/3/myself`, GitHub `/user`) and saves nothing when it fails (`--no-verify` skips the check). The other connectors (for example Slack, Google Docs and Zoom) have no check endpoint: the token is saved unchecked, and the first `careerbrew work collect` tests it. A scoped Atlassian token is moved to `https://api.atlassian.com/ex/jira/<cloudId>` automatically. With `--api https://<host>/api/v3`, the GitHub token page shown is that Enterprise host's; a company plugin's `connector_defaults` fill the site URL, API, and token page. The Setup view in the web app lists the steps for every connector that is not ready.

| Tool | Command | What it reads | Token scope (read-only) |
|---|---|---|---|
| GitHub / GitHub Enterprise | `careerbrew connect github --user <login> [--api https://github.example.com/api/v3]` | your PRs (merged → delivery), count of PRs you reviewed | `repo:read` / fine-grained "Pull requests: read" |
| Jira (Cloud or Data Center) | `careerbrew connect jira --base-url https://jira.example.com [--email you@example.com]` | issues you resolved, every page; type → tag (bug → quality, epic → impact, incident → ownership); the token is checked on `/rest/api/3/myself` first, so a bad token is a source error, not 0 issues | API token / PAT, read |
| Slack | `careerbrew connect slack` (add the `chat:write` scope to send the PM update) | recognition signals: messages that thank or mention you (channel + date + permalink, no text) | user token, `search:read` |
| Google Docs (Drive) | `careerbrew connect gdocs` | docs you own; RFC/design → design, postmortem → ownership | `drive.metadata.readonly`; use a token from the OAuth client your admin approved |
| Zoom | `careerbrew connect zoom` | meetings you hosted; 1:1/mentoring → leadership, design review → design | `meeting:read` on a Server-to-Server OAuth app; a Zoom account admin must create or approve it |
| Outlook / Microsoft 365 | `careerbrew connect outlook`: sign in once with a device code (open the link, type the code); token check: Graph `/me`. See [Outlook sign-in](#outlook-sign-in) | meetings you organized (subject + date; same tags as Zoom; private entries skipped). The same connector also feeds the day calendar (Today's meetings, focus time, planner), `mail scan` metadata, and the work feed, with no `calendars.yaml` or `mail.yaml` entry | delegated `User.Read`, `Calendars.ReadBasic`, `Mail.ReadBasic` |
| PagerDuty | `careerbrew connect pagerduty` (token check: `/users/me`) | pages you acknowledged or resolved (incident title + date → ownership); the work feed also reads incidents assigned to you | a user API token (an account-wide token has no `/users/me`) |

Then run `careerbrew work collect`. Connector evidence feeds the promotion case like git evidence.

### Review asks (GitHub)

`careerbrew work review-ask <PR link>` drafts a review ask that names enough people for the PR to merge (issue #270, US-90). Through the GitHub connector it reads:

- the required approvals for the PR's base branch: branch protection (`/branches/<base>/protection`) and the branch's rulesets (`/rules/branches/<base>`); the higher one wins. A read the token may not make (403) or a branch with no rule (404) counts as 0;
- the changed files, `CODEOWNERS` (`.github/`, the root, then `docs/`, on the base branch), and the reviews of the last 20 closed PRs on the same base.

It names at least the required count, and never fewer than two people. Code owners of the changed paths come first (the last matching rule wins, as on GitHub), then recent reviewers of files in the same folders, then people the PR already asks. Group handles such as `@org/team` and the PR's author never count; groups are listed as FYI. Each person gets the paths to review. When too few people are found, the command fails and says how many to add with `--reviewer <login>`. `--check <file|->` checks a draft someone else wrote and refuses one that names too few people. Token scope: fine-grained "Pull requests: read" and "Contents: read" (for `CODEOWNERS`); branch protection also needs "Administration: read", and without it the rulesets alone decide.

### Outlook sign-in

`careerbrew connect outlook` uses the OAuth 2.0 device code flow (issue #199):

1. careerbrew needs an app registration's client ID: your company's approved app. Set it with `careerbrew config set outlook_client_id <client-id>` (and `outlook_tenant <tenant-id or domain>` if needed; default `organizations`), or let a company plugin set `connector_defaults.outlook.client_id` and `tenant`. `--client-id` and `--tenant` override both for one run. careerbrew ships no client ID; with none set, `connect` names the setting and stops. The app registration must allow public client flows and the delegated scopes below.
2. careerbrew prints a link and a code. Open the link, type the code, and sign in. careerbrew waits in the terminal (the web app refuses this action: it needs you at the keyboard).
3. careerbrew keeps only the refresh token, in the Keychain (`keychain:careerbrew.outlook.refresh`). The connector entry holds that reference with `auth: device`, `client_id`, and `tenant`.
4. Before each read (evidence, calendar, mail, work feed), careerbrew trades the refresh token for a one-hour access token, kept in the running process only, and stores the new refresh token Microsoft returns. The sign-in keeps working for weeks without you.
5. When Microsoft ends the sign-in (expired, revoked, password changed), the read fails with one step, `Reconnect Outlook: run careerbrew connect outlook in your terminal`, in Today's calendar note, the day plan's run page, the work feed's errors, and Setup. A new sign-in clears it.
6. Only `invalid_grant` and `interaction_required` mean the sign-in ended. Any other refusal (`unauthorized_client`, `invalid_client`, `invalid_scope`) names its code and says to check `outlook_client_id`, `outlook_tenant`, and the app registration, because a new sign-in cannot fix it. A page instead of JSON (a proxy) is a plain error, a network blip while you type the code keeps waiting, and a locked Keychain keeps the read going.

A manual token still works, only if your tenant's policy allows it: `careerbrew connect outlook --paste` takes a token your admin issued or allows (it expires after about an hour), and `--ref` takes an existing secret reference. Neither refreshes.

Fixtures: `tests/fixtures/http/login.microsoftonline.com/` answers by tenant, one tenant per scenario (`organizations` signs in, `synthetic-expired-tenant` refuses a refresh, `synthetic-declined-tenant` declines).

## Work feed

### Direct coverage and source health

`careerbrew work live status --json` includes the same health result as
`GET /api/feed/health`. It reports declared coverage, acquisition ages and limits,
adapter running state, fixed repair commands and the next expiry. Missing,
stopped, stale, error and unknown are distinct. A missing declaration never
passes merely because its required count is zero.

For direct intake, the producer runs the configured tools, retains supported
quiet replies, stores them through `work feed add` or `work feed transcript`,
then explicitly calls `work feed cycle`. Required identities are the union of
`feed_tool_sources` and required live-source names. `--require` adds checks; it
cannot remove configured coverage. A subset or arbitrary source cannot certify
setup. A local accepted batch proves local validated coverage, not that old
historical input was freshly fetched upstream.

The completion receipt has data-root lifecycle in encrypted
`work/feed/direct-cycle.json`. It binds current declarations, per-source intake
times and a check highwater. Health reads never renew it or start adapters.
The transient `feed-cycle.json` still records pending intake and is excluded
from view-change versions; the completed receipt is a tracked view input.
`stored` remains the physical stored-source set. Legacy `not_stored` names
required completion-coverage gaps, including unknown acquisition clocks.
With no persistent declaration, explicit `--require` retains its storage check
but does not make core health configured or healthy.

Completion reuses eligible evidence-to-results conversion and Standup's
existing due/publish and proof rules. It does not collect connectors again.
Personal items, noise and ordinary conversation stay out of work done. Stable
ids and versions keep local-day corrections idempotent. Today matches explicit
canonical source provenance; an id prefix or action date proves no acquisition.

Row facts have a separate 60-minute review policy based on when the saved row
bundle was supplied. Rendering and new activity never renew it. Stable titles,
aliases and definitions do not expire at the acquisition limit. Proof-age gates
remain unchanged. A later running-adapter required failure overrides an older
direct success; a strictly newer complete validated direct batch can recover.
Ambiguous clock ordering cannot establish a newer success.

The health GET reads existing encrypted config/state only. It performs no
intake, publish, network call or data write. Each mounted visible browser view
refreshes at its next expiry, with a one-minute bound, and on visibility return.
Timers stop while hidden and are removed on unmount. No clock enters the shared
data-version hash.

### Work feed inputs

`careerbrew work feed` is one local store for the rest of the day: Slack asks, PagerDuty pages, and Outlook mail and meetings ([ADR 0008](decisions/0008-work-feed-keeps-originals.md)). The watcher pulls it every 15 minutes; `careerbrew work feed pull` pulls now.

| Tool | What the feed reads (last `feed_days` days, default 2) | Scope |
|---|---|---|
| Slack | `search.messages` for `to:me` and `from:me` (change with `feed_queries` on the connector): DMs, mentions of you, and your own messages; for teammates with a chat id (`work team add --chat`; this reads coworkers' messages and needs company policy approval), `from:<@id>` and `<@id>`, 5 teammates per pull in turn (search allows about 20 calls a minute), best effort: a rate limit stops them and keeps the rest; then every reply of each thread found (`conversations.replies`, newest 30 threads, up to 300 replies each), each its own event whose `thread` is the parent and whose link is built from its ids; when search also returns a reply without its parent, use the known parent from the thread response and keep the search link | `search:read` (same token); `channels:history` and `groups:history` for thread replies (without `channels:history` the replies are skipped; a private channel without `groups:history` skips that thread; a `work.feed.replies` event names the error and the skips) |
| PagerDuty | incidents assigned to you, and your acknowledge and resolve log entries | the same user token |
| Outlook | meetings (organized by you or invites) and mail metadata: sender, recipients, subject, received time, importance, flag, conversation id, and the message link; never bodies | the same Graph token |
| A plugin | JSON lines: `careerbrew work feed add <file.jsonl> --plugin <name>` | the plugin's own |
| An agent's MCP tools | the tool replies in the agent's session transcript: `careerbrew work feed transcript <session.jsonl> [--since 30m]` (issue #234) | the agent's own; careerbrew calls no tool |

- **Live feed from an agent** (issue #234): an agent that pulls events with MCP tools (chat, ticket, or mail search) does not copy the replies by hand. Map each tool to a feed source once: `careerbrew config set feed_tool_sources 'mcp__chat__*=chat, mcp__tickets__*=tickets'` (globs). At the end of each cycle, run `careerbrew work feed transcript <session.jsonl> --since 30m`. It reads each mapped reply as returned: JSON, a JSON list, or JSON lines; each object in it (or in a list under one of its keys) with an id, a time, and a title is one event, its id prefixed with the source (`chat:C1-1001`). Common field names also work: `key`/`ts` for the id, `timestamp`/`created_at`/`ts` (an epoch time too) for the time, `subject`/`summary`/`text` for the title. A large reply that Claude Code saved to a file ("Full output saved to: <file>") is read from that file, but only when the file is inside the transcript's own folder. An event already stored is not stored twice, so a rerun is safe. It prints counts only. A mapped source with no event stored in the cycle (a failed call, a refused file, a reply with no event, or no call at all) prints `gap  <source>: nothing stored this cycle` and the command exits 1, so a silent gap fails the cycle.
- **Access blockers** (issue #314, `work/access.py`): a message blocked on a missing permission, policy, role, or group is work to resolve. `careerbrew draft route --ask <file> --directory <channels.json> [--teammate <id>] [--json]` finds the channel that owns the topic (`owns` words) and an open thread there that asks the same: `link` it instead of opening a second one, or `open` a draft in that channel; `tag` says whether to tag the teammate in the thread or link it from the original thread. `careerbrew draft judge --draft <file> --channel <id> --directory <channels.json> [--new-thread]` runs before a draft is saved and exits 1 for a channel that does not own the topic, a duplicate thread, or a mention of a non-member. Both read local files only and print ids and links, never message text; nothing is sent.
- **Proactive listening** (issue #266, US-101, `work/listen.py`): the feed listens from the start of the work day until wrap-up (`work live stop`). Every cycle does the follow-up work for each event it has not seen before, as **drafts only**: nothing is ever sent, posted, or reacted. It prints one line per new event with its class (`needs you`, `worth knowing`, `noise`, or `personal`), its verdict, and the actions taken. (1) Each open ask gets a reply draft in the local draft store (`draft replace`'s safe save), judged on the thread's newest state (`draft fit`): an emoji suggestion for a small ack, careerbrew's ack after a release started, else a holding reply to rewrite; a target that already holds a draft keeps it. (2) Each link in a kept event gets a one-line summary (read through the project research readers, under the host policy) or `opened: false` with the reason; a reason the cycle stored (`by: cycle`) stays in `work feed links --pending`, so the agent can still open the link with its own tools, only under the agent-provider gate ([privacy.md](privacy.md)). The cycle opens links only when you turn it on: `careerbrew config set live_open_links true` (default off; while it is off each link gets the reason "not opened: the live cycle opens links only with live_open_links on" and waits for your agent). When it is on, links are read only where business data may show; a link to a loopback, private, link-local, or reserved address (the host is resolved first; `localhost` too) is skipped with a reason, and a page is read with a 5 s timeout and at most 1 MB: both hold for a plain web page only. A link a configured connector knows (a GitHub item, a ticket, a document in a connected work tool) is read through that connector instead, under the host and connector policy, with its 30 s timeout and without the private-address check or the size cap; a link you marked as a project reference is never opened. One bad link or event fails alone (`failed: <error class>`), and the cycle is still marked. Each project gets at most one update per cycle. (3) A review asked of you gets review notes drafted to `review:<digest>`. It also opens one review session record per request (issue #313), keyed by the PR link (else the event id): a re-review ping on the same PR opens none. The cycle prints `review session opened: <key>` and a next-action line, and the agent starts a fresh-context reviewer per new session (skill live-feed, Review sessions); the reviewer writes the review to a file and saves a reply draft, and never posts. (4) The event is attached to the one project a link of it matches (else the project its label names), and the change is recorded as a project update. (5) One line per goal for today (queue goals not past, long-term goals not past their `by` day): your actions on it today, other updates, and what waits on you or on whom (names only where business data may show). Read a draft with `careerbrew draft show <target>` (the text shows only in your terminal or the web app), rewrite it with `draft replace`, and send it yourself.
- **Live feed** (issue #267, `careerbrew work live`): the generic cycle over source adapters, so a company plugin keeps only its source config and roster ([plugins.md](plugins.md#live-feed-sources-and-roster)). `work live start [--sources <file.yaml>]` saves a source config (or uses the plugin's) and lists what each source pulls and its cursor (`since`); `work live cycle` runs one cycle; `work live status [--json]`; `work live stop` (cursors are kept). The cycle: one at a time (a lock); pull each source (a `file` adapter reads the raw tool reply the agent saved to its `inbox`, only as new intake when its validated content and file identity changed since the last cycle; a `connector` adapter reads a configured connector with its own config, so `pull` and the cursor guide only an agent's `file` pulls); store through the feed (originals encrypted); triage by the feed's rules; one verdict per stored event (`answer` an open ask, `done`, `read`, `skip`, `evidence`, `log`, `personal`); next actions; then mark the cycle. `not_stored` retains its compatibility name but lists required sources without healthy validated acquisition (missing, unusable, stale or an error): the cycle prints `not_stored  <source>` and exits 1, and the agent pulls those sources again. An empty reply (`{"messages": []}`) is a quiet source, not a gap. A currently validated unchanged file remains healthy through its inclusive `max_age_minutes` limit (fallback setting `live_source_max_age_minutes`, default 15, integer 1..10080). Its `pulled` and `stored` stay false and it does not replay followups, advance the cursor or renew acquisition. File modification time measures acquisition; event time and cycle polling do not. Current missing, unreadable, malformed or unusable input overrides a prior good receipt. Future or backwards-before-recorded-check clocks are unknown. Old backlog may ingest while its required health fails. Per-source cycle JSON adds `unchanged`, `health`, `acquired_at`, `validated_at`, `checked_at`, `age_seconds` and `max_age_minutes`; optional problems are listed in `degraded`. Connector health requires actual successful invocation. `validated_at` is the current successful payload parse/adapter-return instant; `checked_at` is the final aggregate health instant after all adapters and followup/research/standup processing. Cycle `at` shares that final instant. File acquisition never moves during either evaluation. Completion rechecks every source for expiry without creating a receipt; a genuinely future validation cannot become accepted merely because time passes. The precise persisted check highwater includes cycle entry, every adapter completion and final evaluation; an observed reversal makes the invocation clock-unknown. PM draft rule: only your own action that a rule marks evidence, and not personal, reaches the PM draft, as its title and link. Standup republish (issue #315): after storing, the cycle republishes the standup rows with the manual run's pipeline and proof gate (`standup.run`, no page opened) when a new event matches a row (project, ticket key, or alias word), the last publish is over 60 minutes old, or the standup starts within 20 minutes; it prints `standup: <n> rows published (<reason>)`, `standup: skipped, fresh`, or `standup: failed (<error>); the cycle goes on`, and a failed publish never fails the cycle. Cycle records (`work/live/cycles.jsonl`) hold ids, counts, and verdicts, never text. The interval rule (issue #264): a real cycle takes about 18 minutes, so there is no fixed 5-minute timer; the work-day watch loop (`careerbrew day loop pass`) runs the cycle every 1.2 x the median of its last 10 measured passes (60 s to 1 h; 300 s before the first), and a pass that starts while one runs is skipped.
- **End each live-feed cycle** (issue #254): `careerbrew work feed cycle [--require chat,jira,mail] [--json]`. Every store into the feed (`work feed add`, `transcript`, `pull`) records its source for this cycle, also when every event was stored before. The command names each required source (default: the sources of `feed_tool_sources`) with nothing stored since the last cycle, `not stored  <source>: ...`, exits 1, and starts a new cycle. A non-empty `not_stored` is a failed cycle: pull those sources again. Source names only, never content. The record is `work/feed/feed-cycle.json`.
- **Raw tool replies** (issue #254): save a search reply exactly as the tool returned it and store it with `careerbrew work feed add <file> --source <name>`. An object with a `tickets` list (a ticket search; default source `jira`) stores one event per ticket, its id from key + updated + status (a status change is a new event; the ticket key is the thread); `summary`, `status`, and `updated` are read from the item or its `fields` object. An object with a `messages` list (a mail search; default source `mail`) stores one event per message, its id from the message id, its title from the subject, and `receivedDateTime`, `from`, `importance`, and a flag read too. An object with an `events` list (a calendar search; default source `calendar`, issue #316) stores one `meeting` event per entry, its id from the event id + its start (`start.dateTime` or `start.date`); an entry with no event id or no start has no stable id and is skipped. An item with no id, time, or title is skipped and counted. A reply with an empty list is a quiet source: stored, no gap. `work feed transcript` reads such replies the same way.
- **Agent activity: write as you go** (issue #272): a background agent (a review, a fix, a standup refresh) reports each step the moment it happens, never in a batch at the end of a cycle: `careerbrew work agent event <agent> "<what it did>" [--status running|done|failed] [--project <name>] [--url <link>] [--run <id>]`. One call stores one row in `work/agents/events.jsonl` (encrypted), so `/api/changes` moves and Work's Agent activity shows it within one poll (5 s), grouped by project, each run with its last event and link. `--status done` or `failed` ends the run; a run whose last `running` event is older than `agent_quiet_minutes` (default 60) shows as quiet. The same call over HTTP is `POST /api/actions/work.agent.event` with `{"values": {"agent", "title", "status", "project", "url", "run"}}`. `careerbrew work agent show [--json]` lists the runs; an agent sees counts, states, agents, and projects, never titles or links. Rows are kept `agent_keep_days` (default 7).
- **Sent PM promises** (#331): recording `day pm-sent` turns each Tomorrow bullet into a promise due the next configured workday, with source `pm-update` and high session priority. Wrapped continuation text stays with its bullet. Declared URL sources match their full target, including query, fragment and terminal punctuation; only scheme and host case are normalized. Uppercase URL schemes are accepted. Typed sources use their complete reference. Repository sources and ranked repository keys use one configured GitHub API origin (scheme, host and port), read from existing connector metadata without resolving secrets or making requests. The public API maps to github.com; with no GitHub connector, public GitHub remains the default. Multiple distinct configured origins leave a bare owner/repository ambiguous; exact URL sources still match independently. A name inside a URL is not an explicit project mention. Source matches and configured patterns attach a parent only when one eligible project matches; ambiguous bullets stay unmatched promises. Repeated saves and prose-case duplicates share one promise and one coverage row; URL scheme and host case normalize, while path, query and fragment case keep distinct promises. URL and prose identities use separate namespaces outside the legacy date slot, so prose that spells a serialized URL key cannot hide a commitment or inherit its closure. Proven older ids stay stable; an occupied prose id uses a separate stable fallback. New encrypted promise records keep an opaque full typed-identity digest, so the 200-character display limit cannot change repeat or closure identity. Read-only previews never backfill that metadata. Truncated legacy history is reused only when its expected id and available typed title evidence prove the target; otherwise it stays intact and the current bullet gets a distinct typed record. Legacy ids retain owner closure only when the stored URL-aware title proves the same target; no-URL ids stay unchanged. Malformed URLs keep their exact raw target as a distinct identity, so they cannot stop recording or previewing other commitments; editing the sent text retires removed bullets; restoring one reopens only an automatic retirement, while an owner’s explicit closure remains authoritative. Day start backfills older saved updates; before that write step, the read-only plan shows their items with `needs_recording: true`. `day run --json` exposes `pm_promises` from the previous workday, matched projects, unmatched promise projects, and live session coverage; restricted agent output omits text and project labels. The runner covers these before other projects, within the same session cap. PM promise extraction recognizes `Tomorrow`, a named weekday such as `Tomorrow (Mon)` or `Tomorrow - Monday`, and `Next week`; each uses the existing next-workday due date. The next generated PM draft names every prior promise and its recorded status; the owner’s saved draft still wins.

- **Project sessions** (issue #274, US-107, `work/sessions.py`): sessions read each task’s full ticket or issue scope and deliver tasks one at a time, in order, with every scope item shipped and verified before the next task. A plan or a list of questions alone is not delivery. They batch foreseeable approvals, record blockers with `work session ask`, continue independent work, and report each scope item as done or not done with verification or its blocker (#333). The main /work session runs only the work process, and each pressing project gets one long-lived session, reused all day. `careerbrew work session open <project> --id <session id>` records it (as a `session` loop too) or returns the live one; it refuses a second session for one project and a session past `work_session_cap` (default 3). On ordinary next-day reopening, the latest prior-day session's handoff is returned as a brief where business output is allowed. A newer session without a handoff does not revive an older one. Quiet resumption keeps its existing session-ID guard. This does not schedule a session or certify that its work is complete. If the old host session cannot respond, the conflict message gives the handoff and end commands needed before opening a replacement; provide the known status and next step. A next-day open returns the latest prior-day handoff only for the same identified project, and only where business output is allowed. Older name-only history is unverified and remains subject to `agent_keep_days`; inspect the session archive rather than assuming it belongs to a recreated project. A quiet return records its selected brief for retry. On end, a legacy session with no ownership token warns that its old loop is unverified: check the host loop, then use the printed `day loop stop` command if it has stopped. Day end accounts for live session records even if loop registration was interrupted. A session writes through careerbrew: steps with `work agent event --session <id>` (run and project set from the session), project changes with `work project update`, and questions with `work session ask <id>` into the one question store (#261), so the main session's `day question recheck` holds every session's asks. Before it writes a shared thread or draft it takes a claim, `careerbrew work claim take slack-draft:<target> --session <id>` (named like a step's `uses:` entry, #258): another session's take, and any `draft replace` (with another `--session` or none, the live cycle too) on the claimed target, is refused, so they skip it. Project sessions can claim a GitHub PR with `careerbrew work claim take pr:<owner>/<repo>#<number> --session <id>`. Owner and repository names ignore case. Work-step scheduling does not check session claims. A claim ends on `work claim release`, when its session ends, or `work_claim_minutes` (default 30) after it was last taken. `careerbrew work session list [--json]` is the main session's one view: each session's state (live, stale from an earlier day, ended), its last agent event, claims, open questions, and handoff. Wrap-up records one graceful request per project session: finish its current step, save work, update drafts, write a handoff and report. A session is not archived until its handoff timestamp is newer than the request. The text output of `work session list` flags a saved handoff that does not meet this check as stale or unverified. If `work session end --all` leaves that session open, write a fresh handoff with the command it prints. End of day: `work session handoff <id> "<text>"` writes one "Project sessions" item per project in the day's handoff, and `work session end --all` archives each session that wrote one and releases its claims (exit 1 while one has no current handoff).

  Set `work project set <name> --quiet-until YYYY-MM-DD` to pause a project until that local date, or use `none` to clear it. Quiet projects refuse session opens and new session questions. Existing questions stay open and are deferred from the batched ask. A returning session receives the last handoff as its brief when business data may show; that brief is not a new handoff. Save one line of standing rules with `work project rules-set <name> --yes --text <rules>`; read it with `work project rules <name>`. The session contract instructs each pass to read a snapshot and block that project's pass if business-data policy refuses the read. Replacing or clearing rules needs confirmation; an update during a pass appears on the next read. Project matching for notifications uses an explicit project, an exact item source, an unambiguous repository source on its configured GitHub host, or configured project-priority patterns. It cannot identify an unassociated event, or stop independent activity outside careerbrew.
- Each event is stored once by id, in two files: the original (encrypted, kept `feed_keep_days`, default 30) and the analysis (who acted, triage, personal flag, the deciding rule).
- Triage by rule: **act** (a DM or mention, a review requested from you (a plugin event of kind `review-request`), a page assigned to you, important or flagged mail), **fyi**, **noise** (bots, no-reply senders), **mine** (your own action). An ask stays open until your later reply or acknowledge in the same thread. An event with no chat thread (a review request on a PR, a mention on a ticket) takes its PR, merge request, issue, or ticket link as the thread, so your later review or comment on that PR or ticket closes the ask (issue #228). Any other link (a doc, a page) is no thread. `careerbrew work feed show [--show] [--open]` counts open asks and how many wait over `feed_ask_overdue_hours` work hours (default 8; only time from `work_start` to `work_end` on `work_days` counts). The web app's Today view lists them, newest first (the 50 newest, then Show all), each linked to its tool, with a Mark personal button ([web.md](web.md)).
- Teammates (issue #180): the teammate feed reads your coworkers' messages, so get company policy approval before you add a teammate. `careerbrew work team add "Sam Lee" --chat U0SAM --login samlee --email sam@example.com` keeps a roster (`work/team.yaml`, encrypted). An event by a teammate (the actor's chat id, login, email, or name) or one whose text names one (a whole-word name, `<@U0SAM>`, `@samlee`) carries `team` in its analysis and is never noise: a rule that would make it noise makes it **fyi** (rule `teammate`), unless that rule names `team` in its `when`. Rules can match `team: true` or `team: false`. Adding or removing a teammate triages the kept events again.
- `careerbrew work team feed [--days 7]` (it reads coworkers' messages and needs company policy approval) lists your teammates' kept events, newest first, with the project each one matches (its notes go to that project's memory file, as `<date> slack: Sam Lee: <title> <link>`) or none, and what each link says; titles and links print only to your terminal or the web app.
- Links: every link in a kept event's text (Slack's `<url|label>` and bare URLs) is in the analysis. `careerbrew work feed links --pending` lists the ones not researched yet (the links only to your terminal or the web app); `careerbrew work feed research <id> <file.json>` stores a one-line summary per link, or `opened: false` with the reason (a sign-in page), and is refused when it misses a link.
- Your rules run first. Edit them with `careerbrew data edit work/feed-rules.yaml`, then run `careerbrew work feed retriage`. `careerbrew work feed rules` lists the order and an example:

  ```yaml
  rules:
    - {name: alerts, when: {source: slack, channel: 'alerts-*'}, triage: noise}
    - {name: family, when: {text: [dentist, school]}, personal: true}
    - {name: helped, when: {source: slack, actor: me, channel: 'help-*'}, evidence: true}
  ```

  `when` keys: `source`, `kind`, `actor` (me, other, bot), `to_me`, `channel`, `from`, `status`, `importance`, `title`, `origin` (globs), and `text` (words in the original). A rule sets `triage`, `personal`, or `evidence`.
- **Not an ask, done** (issue #185): `careerbrew work feed mark <id> not-ask` (it never asked you) or `done` (handled elsewhere) closes an open ask; `ask` opens it again. Today's open asks have Not an ask and Done buttons with Undo. Three not-ask marks (or dismissed ask notifications) from one channel or sender propose an inbox suggestion to add a `triage: fyi` rule there; three personal marks from one place propose a `personal: true` rule. Accept writes the rule to `work/feed-rules.yaml` and triages the feed again.
- **Personal**: a private Outlook or ICS entry, a rule, or `careerbrew work feed mark <id> personal` (your mark wins; `work` undoes it). A personal event never becomes evidence, never enters the day log or a project's memory file, and never reaches the PM update. Your rules and marks also cover the same meeting in the day calendar and the same Slack message as recognition. Private calendar entries still block time in the planner.
- **Launches and check-ins** (issue #239): the analysis of your own channel post keeps one word, `signal`: `launch` when the post announces something ("launched", "announcing", "now live", "rolled out", "released") or is pinned, `check-in` when it is your reply in a channel thread that did not ask you. Never the text. The PM update puts a launch first on its project's line, with its link ("launched ([announced in #billing-news](...))"), and names your check-ins ("checked in on [#billing-initiative](...)"). A plugin's events get the same word. Run `careerbrew work feed retriage` to add it to kept events.
- **Your actions as evidence**: pages you acknowledged or resolved, and anything your rule marks `evidence`, go to work evidence on the next `work collect` or watcher cycle.

### Secure access on corporate machines

- **No token in any file.** `careerbrew connect` asks for the token with a hidden prompt and saves it to the macOS Keychain. Config stores only `keychain:careerbrew.<tool>`. Alternatives: `--ref op://Vault/Item/field` (1Password CLI) or `--ref env:NAME` (a secret manager that exports env).
- **A literal token is refused** in config, and a token piped over stdin is refused (it would land in shell history or an agent transcript).
- **Company policy** in the corporate plugin narrows what is allowed:

  ```yaml
  policy:
    allowed_connectors: [github, jira]          # others refuse to run, and `connect` refuses them first
    allowed_hosts: [github.example.com, jira.example.com]   # every work-tool request is host-checked; a project `url` source on another host is a reference you read
    secret_backends: [op]                       # e.g. the company mandates 1Password
    allowed_routes: [local]                     # careerbrew itself sends work data to no hosted LLM
    export_to_personal: none                    # ignored since ADR 0007: careerbrew never copies corporate data to your personal machine
    agent_may_read_business: false
  ```

  With several plugins, the strictest value wins.
- `careerbrew connectors list` shows each connector, its secret backend, and whether policy allows it.

## Corporate and personal machines

Set the role once per data folder: `careerbrew machine set corporate` or `careerbrew machine set personal`. The role lives in the data folder (`machine.yaml`), so a second data folder on the same machine needs its own setting. A corporate plugin's `detect` signals (env vars, or paths such as an MDM profile folder) make `careerbrew machine show` suggest the role.

| | Personal machine | Corporate machine |
|---|---|---|
| Job hunt (`jobs`, `drill`) | yes | **blocked**: job-hunt data never sits on employer hardware |
| Work mode, connectors | yes (from synced data) | yes |
| Example job files from `init` | created | skipped; `machine set corporate` removes untouched ones and refuses if real job-hunt data exists |
| Profile on this machine | full | keep it to `name`, `employer`, `sensitive` (fields that feed redaction) |

### Working together

1. On one machine: `careerbrew pair init`. It prints the key once (to a real terminal only; piped output gets a file path instead). Store the key in your password manager.
2. On the other machine: `careerbrew pair join` (hidden prompt).
3. Corporate → personal: nothing moves. `sync export` refuses on a corporate machine, and a personal machine refuses a corporate bundle ([ADR 0007](decisions/0007-corporate-data-stays-local.md)).
4. Personal → corporate: only settings (ladder choice, target level). A corporate machine refuses any bundle carrying other data.

Bundles are encrypted with the pairing key (Fernet). A changed file or a wrong key fails to import. There is no server and no network sync, so nothing leaves the two machines except the file you move.

## Job platforms (remote and nearby)

`careerbrew jobs preset` writes a remote-first source set and `regions_ok: [global]`. For jobs near you, add your country and city to the Adzuna source (`country`, `where`) and your place names to `locations_any` in `jobs/criteria.yaml`. Details and terms: [`knowledge/research/api-notes.md`](../knowledge/research/api-notes.md).

`jobs scan` skips a source when an allowed secret reference has no value. It prints a warning and saves postings from the sources that completed. The preset Adzuna warning gives both local `careerbrew secret set` commands. Custom references get a generic source-file repair instruction. If every source is skipped, the command fails and preserves previous postings. A successful empty source can still replace the old snapshot. Policy denials, invalid references and provider errors remain hard failures.

| Platform | Key | Cache | Terms careerbrew follows |
|---|---|---|---|
| Himalayas (optional `country`) | none | 12 h | link back, credit, never resubmit |
| Remotive | none | 6 h (≤ 4 fetches a day) | link back, credit |
| Remote OK | none | 6 h | keep the Remote OK job URL, credit |
| We Work Remotely RSS | none | 1 h | link to the job |
| HN "Who is hiring" | none | 24 h | posts that mention remote (set `must_mention` for your own places) |
| Adzuna (`country` required; optional `where`) | free `app_id` + `app_key` (`careerbrew secret set careerbrew.adzuna.id` / `.key`) | 6 h | personal research use |

The built-in region labels (`us`, `americas`, `europe`, `asia`, `global`) drop remote roles limited to regions you did not allow in `regions_ok`. No country label is built in: add your own label to `regions_ok` (for example `examplia`), and it matches a posting whose location holds that name as a whole word. To match more words, add `region_words: {examplia: [examplian]}` to `jobs/criteria.yaml`; your own labels are checked before the built-in ones. A criteria file that already lists a country label needs no edit: the label keeps matching by its name, and `region_words` adds its other word forms. A remote posting whose location is only a country name reads as `global` unless that country is one of your labels. LinkedIn, Indeed, Glassdoor, Wellfound, and Welcome to the Jungle have no usable public API and forbid scraping.

## LinkedIn

- `careerbrew jobs linkedin-import <export.zip>`: your own "Download your data" archive. It reads Positions, Skills, Saved Jobs, and Job Applications by header name.
- Job-alert emails from `jobalerts-noreply@linkedin.com` become postings during `careerbrew mail scan`.
- careerbrew never scrapes LinkedIn or uses its cookies.

## Mail (read-only metadata)

`private/mail.yaml` (shape: `profile/examples/mail.yaml`):

| Provider | How | Secret |
|---|---|---|
| Outlook / Microsoft 365 | Microsoft Graph `/me/messages`, scope `Mail.ReadBasic` | `keychain:careerbrew.graph` |
| Fastmail | JMAP with a read-only API token (Fastmail API tokens are for your own personal use; an app for other people must use OAuth) | `keychain:careerbrew.fastmail` |
| Proton Mail | Proton Mail Bridge IMAP on `127.0.0.1:1143` (paid plan) | `keychain:careerbrew.proton-bridge` |

`mail scan` reads only the message id, sender, subject, and date. The work feed's Outlook reader also reads recipients, importance, flag, conversation id, and the message link. Bodies are never fetched (`BODY.PEEK[HEADER.FIELDS ...]` on IMAP). Mail is never sent. Accounts carry `machine: personal | corporate | any`; a corporate-role machine reads only corporate/any accounts and never processes job-hunt mail.

## Calendar and the day

`private/day/calendars.yaml` (shape: `profile/examples/day/calendars.yaml`): Outlook via Graph `calendarView` (`Calendars.ReadBasic`), and any private ICS link (Google, Fastmail, Proton, iCloud). ICS links are secrets: a literal link is refused. An `outlook` connector (`careerbrew connect outlook`) adds its calendar automatically; the same token listed here is not read twice.

PM content (issue #336): GitHub/gh draft and state fields survive collection through the watcher and day's results; a metadata-only state change replaces the same result id. A closed PR without explicit merge evidence is not tagged as a delivery. Generated drafts drop explicit no-action status checks and mark source-proven draft work `In draft`. Source-proven open PRs stay `Open PR`; a closed PR without merge evidence says `merge not confirmed`. `day send` reports `no-action` and `draft-state` checks without changing the owner's text, including `--text` overrides. Restricted agent output carries check counts and names only. Discussion, hiring outcomes, lasting unblocks, and short Tomorrow deliverables follow the [work-companion rules](../skills/careerbrew-work-companion/SKILL.md#pm-content-rules-issue-336); missing source facts are never invented.

Meetings you led (issue #239): a meeting you organized (Graph `isOrganizer`; for ICS, an `ORGANIZER` that is one of the calendar's `me` addresses: `{kind: ics, path: ..., me: [you@example.com]}`) that is not one occurrence of a series and is not a standup is named on its project's PM update line ("led billing planning"); every other meeting is counted. The meeting's goal and outcome are not read yet: no meeting-notes source exists (task `pm-meeting-outcome`).

| Command | What it does |
|---|---|
| `careerbrew day plan` | time-blocks today from free slots, ranked tasks, calibration, and carry-over |
| `careerbrew day next` | the best next step right now, and why |
| `careerbrew day watch on/off/pause/resume/status` | the background watcher (ADR 0005) |
| `careerbrew day results [--show]` | the results-of-the-day list; `--show` displays times in your configured time zone, oldest first, with a blank time for an undated result |

The watcher reads evidence sources (git, gh, plugin connectors), calendars, and mail. Calendars and mail run at most every 15 minutes. On a personal machine, interview emails go into today's results and LinkedIn alerts become postings; on a corporate machine no job-hunt mail is kept. It runs only inside work hours (`work_start`/`work_end` in settings.yaml), backs off from 5 to 30 minutes when nothing changes, runs at nice 10, and logs CPU and memory per cycle.

Notifications (redacted; `CAREERBREW_NOTIFY=off` silences them):
- once each morning: "Today's plan is ready: N task(s) around M meeting(s)";
- a new interview email (count only, no subject);
- a changed next-task suggestion;
- opt-in (`careerbrew config set notify_events true`): important new events from the work feed and the inbox, with what happened, why it matters, and the next step. See [Notifications](#notifications-about-important-new-events).

### Notifications about important new events

Off until you turn them on: `careerbrew config set notify_events true` (or Turn on in the web app's Notifications view). After each watcher cycle, `careerbrew notify check` runs; you can run it yourself too.

If a detached notifier stops before confirming launch, Notifications says “delivery not confirmed.” The item stays open and is not automatically sent again, because it may already have appeared. Review the item there; snooze it to request a later retry, or mark it read or dismiss it.

| Priority | What |
|---|---|
| 5 | a page assigned to you that nobody has acknowledged |
| 4 | a direct message, a review requested from you |
| 3 | a mention, important or flagged mail, a page you acknowledged that is still open, an inbox suggestion (an agent's task, a plugin's suggestion, a new goal) |
| 2 | another ask your own rule marks `act`; a task that looks done; a new project source |

An item that belongs to your rank-1 project in `project_priority` gets +1. Only open asks (unanswered, not personal) and open suggestions count, and only for a day after they arrive.

| Setting | Default | What it does |
|---|---|---|
| `notify_events` | false | turns notifications on |
| `notify_min_priority` | 4 | the lowest priority that notifies |
| `notify_quiet_hours` | 19:00-08:00 | no pop-ups then; held ones arrive in one "While you were away" pop-up after (`none`: no quiet hours) |
| `notify_daily_cap` | 6 | the most pop-ups a day; past it, new ones wait in the web app (0: web app only) |
| `notify_batch_minutes` | 10 | events within this many minutes of the last pop-up arrive together in the next one |
| `notify_snooze_minutes` | 60 | Snooze hides a notification this long |
| `notify_backend` | auto | `auto` picks alerter, then terminal-notifier, then osascript; or name one |

Each pop-up says what happened, why it matters, and the next step, for example "Slack: a direct message asks you. Why: it was sent to you directly and you have not replied. Next: reply in Slack." It names kinds and sources only, never a title, a person, a project or plugin name, or message text: it says "it is on your rank-1 project" and "a plugin suggests something", and the web app names them. One thread is one notification, and so is one PR, merge request, issue, or ticket when the asks on it have no chat thread (`feed.thread_key`, issue #228): a repeat while it is unread is merged; a new ask after you handled it opens it again. Several new ones at once are one pop-up ("3 new for you: 1 page, 1 direct message, 1 review request").

| Notifier | Install | A click |
|---|---|---|
| alerter | `brew install vjeantet/tap/alerter` | buttons: Open, Accept and Dismiss (a suggestion), Snooze; a click on the text opens it |
| terminal-notifier | `brew install terminal-notifier` | opens that notification in the web app |
| osascript (built in) | none | a plain pop-up that does not open careerbrew |

Accept or Dismiss on a suggestion you already decided elsewhere (Today, the CLI) changes nothing and says so. A click runs `careerbrew notify open <id>`: it marks the notification read and opens `#/notifications/<id>` the way `careerbrew open` does, so the token is never in a pop-up or a command line. `scripts/doctor.sh` and `careerbrew notify status` say which notifier this Mac uses. Every notification is also in the web app's Notifications view with the same actions, so nothing needs macOS support. CLI: `careerbrew notify list [--show] [--all]`, `careerbrew notify act <id> read|accept|dismiss|snooze`.

`careerbrew jobs interview-gaps` lists the companies in your pipeline that still lack a sourced interview loop, best first, so the company-research skill covers every job you actually pursue.

## Project memory sync (both ways)

A project's memory file (`work project memory <name> <path>`, or `--memory` on add) and the project stay in sync. The watcher does it each cycle; `careerbrew work project sync` does it now. `careerbrew config set project_memory_sync false` turns both directions off.

Project source refs are normalized when added: Jira browse links become ticket keys, repository tree links become `owner/repo`, and Slack channel ids with a leading `#` lose that prefix (channel names keep it). Refresh and `work project sync` repair old raw refs and merge duplicates. Checking, comparing and changing a source's reference or unreadable status use the same kind-aware identity, so an original local path alias remains usable after its path is resolved. A saved absolute document path also matches its original directory-symlink alias when only the target file is missing; this lookup adds no source and does not read the missing document. An explicit reference-only choice is kept. Reference-only conversation pointers keep their exact thread URL; tracker keys still normalize. A link whose detected kind differs from the requested kind is refused with the correct kind; an explicit `url` remains a URL.

A GitHub PR or issue link tracks that one item as a `url` source. Absorb and live-link research read only that item's title and state through the matching GitHub connector; it does not add the whole repository. Repository links still track repository activity. Existing sources that already lost their item URL cannot be reconstructed: remove the unintended repo source and absorb each original item link again.

**Memory file → project.** The watcher hashes each memory file's text every cycle. Only a new hash (the content changed, whatever the mtime says) makes it read the file:

| In the memory file | Becomes |
|---|---|
| a link: Jira `/browse/KEY-1`, GitHub, Google Docs, Slack archive, any other page | a `tracker`, `repo`, `doc`, `chat`, or `url` source (the same reader as `work project absorb`); a link that opens a sign-in page is a reference |
| `trackers: PROJ-7, PROJ-8` | tracker sources |
| a ticket key whose prefix the project already tracks (`PROJ-44 is blocked`) | a tracker source; other ticket-shaped words (`UTF-8`) are skipped |
| `channels: #eng-billing` | a chat source |
| `links: search, ledger` | links to those projects (an unknown name is skipped) |
| `status: one line` (the last one wins) | the status of the project's memory source, which is stamped checked |

More than 20 new refs in one change: the rest are proposed (`work project show`). Lines careerbrew wrote itself (these notes, absorb, checkup research) are never read back: every write records them, also while the sync is off, and checkup research never copies a memory file into itself. In a file from before this sync, only careerbrew's section headers and the bullets in the exact shape it writes count as its own (a tracker's `status: ...; updated ...`, a dated work-tools note); a bullet you or your agent add under one of its sections is read. A write made while the sync is off does not hide an edit you made meanwhile: it is read once the sync is on.

**Work tools → memory file.** After careerbrew collects (the watcher, `work collect`, `work feed pull`, `work feed add`), every work-feed event (Slack, PagerDuty, Outlook mail and meetings, plugin events) and evidence item (Jira, GitHub, gh, git, Zoom, notes) from the last 7 days that matches a project goes into its memory file as one dated line with its link:

```markdown
## 2026-10-21 work tools: 2 new item(s)

- 2026-10-20 jira: PROJ-7: Freeze the billing schema <https://jira.example.com/browse/PROJ-7>
- 2026-10-21 slack: Mention from sam in #eng-billing <https://example.slack.com/archives/C0BILL/p1792594800000300>
```

- A match, strongest first: the exact tracker key; a link source (the repo, the Slack channel, the doc, the page) or a pattern of the project's `project_priority` entry; a tracker key prefix. An item goes to the best-matching project.
- A project with no memory file of its own (a child in an imported tree) uses its nearest parent's, as `work project absorb` does. Its notes go there under their own header, `## 2026-10-21 work tools: 2 new item(s) (project ledger)`. An item that the parent matches too is the parent's note, with no tag.
- Once per file: re-collecting adds nothing; a line already in the file is not added again.
- Never written: personal events (a private entry, a rule, your mark), noise (bots, no-reply senders), and anything into a `MEMORY.md` index. Mark an event personal later and its note is removed: the file keeps its mode, text your agent appends meanwhile is kept, and a file changed any other way is left alone until the next sync.
- Notes are business content in a file your agent reads, so they are written only when business reading is allowed: your terminal or the web app for a command, a plugin policy `agent_may_read_business` for the watcher (the same rule as checkup research). `work project sync` says when it wrote nothing for that reason.

## Project lists from a sheet (import, read-only)

`careerbrew work project import <source> [--dry-run]` adds or updates work projects from a team's goal list or a quarter's cutline.

| Source | How | Sign-in |
|---|---|---|
| CSV file | header row with `name` (required), `title`, `group`, `parent`, `links`, `trackers`; other columns are ignored and named | none |
| JSON file | a list of objects with the same keys, or `{"projects": [...]}`; `links` and `trackers` can be lists | none |
| Google Sheets link | any `https://docs.google.com/spreadsheets/d/<id>/...` link (edit, share, or published); careerbrew reads its CSV export for the same tab (`gid`) | none. Recommended: use File > Download > CSV and import the file. A link works only for a sheet with link sharing on; use one only when the sheet's owner and your company policy allow it |

`links` and `trackers` cells hold several values split by commas, semicolons, `|`, or spaces. Names are lowered with spaces turned into dashes, so `Checkout Analytics` becomes `checkout-analytics`. A parent or link must name a project in the sheet or already tracked. One bad row writes nothing. Re-importing is safe: an empty cell keeps the stored value, trackers are added as `tracker` sources, and no source (or its `checked_at`) is removed.

## LeetCode (opt-in, read-only)

`careerbrew exercises sync --leetcode <username>` reads the recent accepted submissions of a public LeetCode profile through LeetCode's public GraphQL endpoint (no login, no cookies; an unofficial endpoint that LeetCode may change; personal use). Only the ~20 most recent solves are public, so run it now and then; matches accumulate. `careerbrew exercises sync <slug> ...` marks problems solved without any network call. For a full sync of every solved problem (LeetCode shows per-problem history only to the logged-in user), copy the `LEETCODE_SESSION` and `csrftoken` cookies from your browser on leetcode.com into `.env` as `LEETCODE_SESSION` and `LEETCODE_CSRF`; they never leave your machine except to leetcode.com. The cookie expires after a few weeks; refresh it then. The full sync uses an unofficial endpoint with your own session cookie, it may be against the site's terms, it is off by default, and manual marking (`careerbrew exercises sync <slug> ...`) is the safe way.

## Notion

careerbrew does not sync with Notion. The `notion-import` skill reads your Notion career pages through the agent's Notion connector, writes a bundle to `<data root>/imports/notion/`, and runs `careerbrew import notion`. Nothing is ever written to Notion. Re-run the skill after you edit Notion; the import replaces what the last import wrote and keeps your practice history.

| Notion | careerbrew |
|---|---|
| Career pages, How-to guides, job search plan, saved articles | Library view |
| Jobs database | Jobs board in Job hunt; applied, rejected, and hired rows go to the application log (a company leaves the plan for 12 months after a real loop); a job's page notes go to Library (job notes) |
| Flashcards | the `notion` practice deck ([practice.md](practice.md)) |
| My skills | `profile.yaml` skills (added, never overwritten) |
| STAR stories | the interview story bank, readable in Library (interview stories) |
| Past-job 1:1 notes | Library (past job) plus a leadership evidence note; names become leak terms |

## A new job in one command

```bash
careerbrew plugin new "Acme Corp" --connect jira,github,slack --jira-url https://jira.acme.example.com --github-user you
```

It creates a private plugin repo in `~/careerbrew-plugins/careerbrew-acme-corp/` with a ladder to edit, sensitive terms, connectors, and safe policy, installs it, and asks for each token with a hidden prompt. Without a terminal, it prints one `careerbrew secret set careerbrew.p-<hash>.<tool>` command per token.


### Repair an empty GitHub header in Claude

The preflight can offer `careerbrew day start --fix --yes` for an HTTPS GitHub connector whose Authorization header uses an empty launchd `GITHUB_TOKEN`. Use `--yes` only after the owner approves changing that Claude configuration. The Actions view uses its existing confirmation for the same handler. This repair does not start the day. A connector registered in agentbrew must be changed in its source and synced; the repair refuses to edit that generated output. An unreadable ownership registry also blocks the repair. Only a successful launchd probe establishes an empty variable; a failed probe is unknown and cannot authorize a configuration change, even if it prints nothing.

It preserves the real target of a configuration symlink, file permissions and unrelated settings. A second run keeps the file unchanged. The repair rechecks source bytes and metadata before renaming, and stops if they changed. Its lock serializes repairs through this CLI; other client writers do not share that lock. The saved helper uses the resolved Python interpreter, so removing an invoking worktree’s virtual-environment launcher does not break it. It keeps the absolute installed gh entry point, including its stable symlink, and runs `gh auth token --hostname github.com` at connection time; the repair itself does not fetch a credential. Existing helpers and other endpoints keep their current settings.

Reconnect using an exposed host tool, then run `careerbrew day start` to recheck wiring. Verify one read-only GitHub MCP request through that client before calling the connection fixed. The configuration check alone cannot prove live authentication. When the host has no reconnect tool, or the client does not execute the helper, retain the read-only preflight and manual recovery step. Never put a token in chat or a task.

Claude documents [dynamic authentication headers](https://code.claude.com/docs/en/mcp#use-dynamic-headers-for-custom-authentication). This repair targets Claude JSON settings; the native Codex preflight remains read-only.

