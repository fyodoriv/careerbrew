# Corporate plugins

A plugin layers company content on top of careerbrew without putting it in this repo. Name the repo `careerbrew-<org>` and keep it private.

Plugin source files and their Git checkout are plaintext. `plugin new --dir` refuses protected data-root folders before writing. Choose a folder outside the data root, such as the default `~/careerbrew-plugins/`.

## Shape

```yaml
# careerbrew-plugin.yaml at the plugin root
name: acme                      # ^[a-z][a-z0-9-]{1,39}$
version: 1.0.0
requires: ">=0.1.0"             # minimum careerbrew version
provides:                       # every path must exist and stay inside the plugin
  ladders: ladders/*.yaml       # promotion ladders (schema: knowledge/ladders/generic-swe.yaml)
  job_sources: job-sources.yaml # extra ATS boards: {sources: [{kind, board, company, tier}]}
  knowledge: knowledge/*.yaml   # extra drill items: {items: [...]} (schema: knowledge/topics.yaml)
  rubrics: rubrics/*.yaml       # extra interview rubrics
  evidence_sources: evidence.yaml  # shared sources only; per-person values (email, user, authors, home paths) fail `machine ready` (move them with `machine split-plugin-evidence`)
  rules: rules.md               # house rules the agent reads
  prompts: prompts/
  skills: skills/
  commands: commands/           # command sources (*.md): agentbrew deploys the exact selection to both agent hosts
sensitive_terms: sensitive-terms.txt   # codenames, customers, people: blocked from every exit
checkup:                        # extra `careerbrew checkup` steps; run only with `plugin add --allow-exec`
  - {name: vpn, run: [scripts/check-vpn.sh]}
connector_defaults:              # fill `careerbrew connect <kind>` flags the user leaves out (flags still win)
  jira: {base_url: https://jira.example.com}
  github: {api: https://github.example.com/api/v3, token_page: https://github.example.com/settings/personal-access-tokens/new}
live:                            # the live feed's company data only (issue #267): see "Live feed sources and roster"
  sources: live/sources.yaml
  team: live/team.yaml
work_steps: [/team-sync, {step: /review-prs, uses: [browser:sso]}]   # /work steps after careerbrew's and the owner's (`work_steps` setting); a name declared earlier keeps its place; fields: "Work steps" below
agent_reads: [slack, gdocs]      # kinds the agent reads with its own tools (what it reads goes to the agent's provider: get employer approval first, see privacy.md): checkup labels these sources "agent reads this"; add `url` for links careerbrew may not fetch (an app behind single sign-on)
detect:
  env: [ACME_SSO_TOKEN]         # `plugin list` shows which signals fire
policy:
  allowed_routes: [local]       # model routes this company allows
  require_encrypted_backup: true
  backup_targets: [~/Backups/careerbrew]
  agent_may_read_business: false   # true only if the company approved your AI provider
  public_metrics: true             # false: numbers are removed from the public LinkedIn block; `plugin new` writes false on a corporate machine
  metrics_share: false             # forbid the opt-in weekly metrics file on this machine
```

## Guarantees

| Property | How |
|---|---|
| No code runs on install | install copies files and validates the manifest. A `command` evidence source runs only after `plugin add --allow-exec` |
| Several plugins at once | resource ids are checked; a clash fails with the fix |
| Strictest policy wins | allowed routes and backup targets intersect; booleans that restrict OR together |
| Tamper detection | the lock file stores a SHA-256 of the plugin tree; `plugin verify` re-hashes it |
| Commands never clobber | a plugin command never overwrites owner files; removal prunes only its fragment-owned command sources |
| Removal | each JSONL record a plugin source stores carries `origin: plugin:<name>`; `plugin remove --purge-data` deletes those records, the inbox items they made, and the derived outputs (`out/`, `cache/`, strategy). It does not delete what you or the live cycle built from them: reply and review drafts, and project updates in `work/projects.yaml`. Delete those by hand, or delete the data folder |
| No company content in the base repo | `careerbrew privacy scan` runs in the gate with every installed plugin's terms |

## Live feed sources and roster

Required live-source names also apply to explicit direct batches. A producer
must cover those names and `feed_tool_sources`, then complete `work feed cycle`.
An optional adapter remains optional unless its canonical name is separately
declared by the tool map. See [coverage and source health](integrations.md#direct-coverage-and-source-health).
No plugin field starts a second health poller or renews acquisition on a GET.

The live feed (`careerbrew work live`, [integrations.md](integrations.md#work-feed)) is generic: the cycle, cursors, required sources, triage, verdicts, PM draft rules, and the cycle lock live in careerbrew. A plugin adds only its company data, in two files named under `live:` in its manifest (each must be a file inside the plugin):

```yaml
# live/sources.yaml: the plugin's sources; one the owner sets with the same name wins
sources:
  - name: chat                 # the feed source name: short, lowercase
    kind: chat                 # chat | tickets | code | mail | calendar | paging
    required: true             # missing, invalid or stale acquisition fails (not_stored)
    adapter: file              # the agent saves the tool's raw reply to inbox each cycle
    inbox: ~/live/chat.json
    max_age_minutes: 15        # optional integer 1..10080; fallback: live_source_max_age_minutes (15)
    pull: {hosts: [chat.example.com], channels: [team-x]}   # what the agent pulls; shown only where business data may show
  - name: pages
    kind: paging
    adapter: connector         # a configured connector: careerbrew connect pagerduty
    connector: pagerduty       # reads with the connector's own config; `pull` and the cursor guide only an agent's file pulls
```

```yaml
# live/team.yaml: the roster, same fields as work/team.yaml. A chat id here makes the feed read that coworker's messages; it needs company policy approval.
team:
  - {name: Sam Lee, chat: U0SAM, login: samlee, email: sam@example.com}
```

- The roster is a plaintext plugin file (in the plugin's source and in its installed copy), not an encrypted data record. Add it only when company policy allows a shared file with teammates' names, chat ids, logins, and emails.
- The plugin's roster joins the owner's for triage (`team.roster()`); a name on the owner's roster wins, and `work team add|remove` change only the owner's file.
- Events a plugin source stores carry `origin: plugin:<name>`, so `plugin remove --purge-data` deletes them.
- Test it with no plugin first: `careerbrew work live start --sources <file.yaml>` takes the same source shape (`tests/fixtures/live/sources.yaml`).

A file source validates its current reply on every cycle. An unchanged valid reply
remains healthy at acquisition age <= `max_age_minutes`. It is not a new pull
or store. Acquisition uses file modification time, never event time or poll
time. The settings-table fallback is `live_source_max_age_minutes` (15).
YAML booleans, floats, strings and values outside 1..10080 are refused.
Keep required sources required. Missing, unreadable, malformed, unusable, future
or clock-unknown input cannot use a previous good receipt. Payload validation
uses its actual parse/adapter-return time. All required acquisition is checked
again after cycle postprocessing, so a reply that expires during the cycle
fails. `validated_at` and final health `checked_at` name these distinct instants. A connector needs
a successful invocation each cycle. Optional problems appear in `degraded`
without failing required coverage. Owner source names still win over plugins.

## Reply drafts a plugin step writes

A plugin step that writes chat reply drafts uses careerbrew's draft rules instead of its own (issue #269, US-98). It runs `careerbrew draft fit` on the thread's newest state, then saves with `careerbrew draft replace`, or reads its chat tool's draft back and runs `careerbrew draft check`. Python plugin code can call `careerbrew.work.drafts.replace(target, text, backend)` with a backend of three calls (`clear`, `save`, `read`) around its chat tool. The step declares `uses: slack-draft:<target>` so the runner keeps two writers of one target in different stages (issue #258).

## Problems a plugin command finds

A plugin slash command that runs a checkup can mirror the last step of `/careerbrew-checkup`. It lists each problem it found, searches the plugin's own tracker for a duplicate first, files the rest there, and reports the ids. A careerbrew problem goes to careerbrew through `/careerbrew-checkup`'s step, whose drafts must pass `careerbrew privacy scan --file` (it blocks every plugin's terms), so company content never reaches this repo.

## Compared with agentbrew team overlays

agentbrew allows one team at a time and trusts its Agentfile. careerbrew allows several plugins and adds hash verification, exec opt-in, policy merging, and data purge. It keeps the same litmus test: a contributor outside the company can clone the base repo and use it fully.

## What careerbrew owns versus agentbrew

- careerbrew owns the user's work record and everything built from it: the day log, the PM update, a standup report, evidence, promotion docs, and the work feed with its web views.
- agentbrew only deploys careerbrew's commands; it never owns them.
- Litmus test: if it reads or writes the user's career data, it is careerbrew.
- So a company's career workflow (its standup format, its PM update rules) goes in the plugin's `commands/` or `skills/`, not in an agentbrew team overlay.

## Work steps

The owner can order a deployed command without giving a plugin access to a host command. For an empty setting, set `work_steps` to a JSON array, for example `careerbrew config set work_steps '[{"step":"/example-check","scope":["example-work"],"after":["/example-prep"]}]'`. `config set` replaces the whole list: read `careerbrew config` first and retain existing entries when adding or updating one. Comma-separated names still work. Plugin steps must be shipped under that plugin's `provides`; they never fall back to host commands. Every long-running step needs a `scope` or `uses` entry when it shares the plan with another long-running step.

A plugin adds `/work` steps under `work_steps:` (issue #257). A step is a name, or a mapping with `step:` and the fields below (issue #258). `careerbrew day run --json` shows `stages` as an earliest-start view. The runner uses `careerbrew day check --json` after each completion to start its `ready` steps without waiting for unrelated stage peers (issue #308). Each step's `ready_when` lists effective `after` dependencies, `scope_free`, `resources_free`, `runs_alone`, and `signin_required`; `why` explains safety waits. A dependency releases on done or blocked, never failed. The ready list is one safe batch: mark its steps running before the next dispatch check.

| Field | What it says | Example |
|---|---|---|
| `step` | the slash command or skill to run | `step: /review-prs` |
| `after` | wait until these steps are done or blocked (a name not in today's plan is ignored) | `after: /step-one` (step two waits for step one) |
| `scope` | paths or service names the step changes; two steps whose scopes overlap (equal, or one path inside the other) never run at the same time | `scope: [~/code/<repo>, tickets]` |
| `uses` | named resources the step needs alone: `browser:<name>`, `slack-draft:<target>`, `file:<path>`, `checkout:<path>`; two steps that list the same entry never run at the same time | `uses: [browser:sso, slack-draft:team]` |
| `longRunning` (or `long_running`) | the step never ends (a live feed); it starts after its own dependencies and after finite work that must release its resources, then keeps running | `longRunning: true` |
| `operatorTurn` (or `operator_turn`) | a quick dispatcher run in the main session in the owner's initial turn, after `day start` and before waiting on background results; default timeout 3 minutes; cannot be long-running or depend on background work except `/careerbrew` | `operatorTurn: true` |
| `verify` | a one-line command (run without a shell, in the folder and PATH the runner calls careerbrew from; use `sh -c '...'` for a shell, and a full path for a tool not on PATH) whose exit code proves the step's result (issue #259); the row is `done` only when it exits 0, else `failed` (class `tool`); its output is never read | `verify: careerbrew report latest --run checkup --json` |
| `cycle` | a one-line command (run like `verify`) that the work-day watch loop runs on each pass (`careerbrew day loop pass`, issue #264), with `CAREERBREW_WATCH_PHASE=skip` set, so the step skips its own "watch until cancelled" phase; its output is never read | `cycle: careerbrew report latest --run checkup --json` |
| `watch` | what re-runs the cycle: `repo:<path>` (a local or remote ref moved), `feed:<source>` (the live feed stored something new from it), `file:<path>` (its size or time changed); without it the cycle runs every pass | `watch: [feed:chat, repo:~/code/<repo>]` |
| `timeout` | whole minutes the step may run (1 to 1440; not for a `longRunning` step, which never times out; default: 3 for operator-turn dispatchers, otherwise the `work_step_timeout` setting, 30); `careerbrew day check` then sets it back for one retry, and marks it `failed` (class `tool`) after the second timeout | `timeout: 45` |
| `about` | one plain line, at most 15 words, that says what the step does (issue #311); without it, the first sentence of the step file's `description:` is used, including YAML folded descriptions. `careerbrew day run --json` hides non-built-in descriptions when business reading is off; the day page shows them locally. `careerbrew day run` warns about a step with neither | `about: Draft today's standup from yesterday's work` |

```yaml
work_steps:
  - {step: /feed, longRunning: true}
  - {step: /step-one, scope: repo, uses: ["checkout:~/code/<repo>"]}
  - {step: /step-two, scope: report, after: /step-one}
  - {step: /review, uses: ["browser:sso"]}
  - {step: /drafts, uses: ["browser:sso", "slack-draft:team"]}
```

With careerbrew's `/careerbrew` first, that day runs in 4 stages: `/careerbrew`; `/step-one` and `/review`; `/step-two` and `/drafts`; then `/feed`.

- A step that declares neither `scope` nor `uses` runs alone, in its declared place. A name-only `work_steps` setting entry is such a step, and so is `/careerbrew`, which refreshes what later steps read.
- Operator-turn dispatchers appear in `start_of_day.operator_turn`. Run their quick startup work inline before background waits. This field does not create a host capability: if the available controls refuse dispatch, record the blocker. Add `scope` or `uses` to declare their resources. A failed sign-in puts browser dispatchers in `start_of_day.operator_turn_blocked`: record them blocked with the existing sign-in ask and continue independent work, rather than retrying them from a background-started turn.
- Refused: an unknown field (a typo such as `use:` would drop a lock), an `after` loop, a step `after` a long-running one, and two long-running steps that share a resource.
- One owner per checkout or PR per day: after a step works on one, it records it with `careerbrew day update <step> --handled checkout:<path> --handled pr:<repo>#<number>`. The plan's `handled` lists each item with its step; a later step skips them, and a second step's claim is refused.
- A step that reads project sources runs `after` the step that refreshes them; a refresh skips a source another step checked since the stale list was read.
- Blocked versus failed (issue #259): `blocked` waits on the owner, so `careerbrew day update <step> --status blocked` needs `--needs-you "<the ask>"`; `failed` means something broke, so it needs `--error-class env|auth|network|conflict|tool`. Each such row gets a generic `next` action in `careerbrew day run --json`.
- Timing (issue #263): a row's change to done, blocked, or failed stamps `ended` and `seconds` (from `started`); a new start clears them. The day's final status and duration per step go to `day/work-history.json`, and a finished step that took more than `work_slow_percent` (200) percent of its median (its done runs on up to 10 earlier days, from 3 of them) is `slow` on the page. `careerbrew day history` reads it.
- Crash safety: the runner marks a row `running` when its step starts, with `started` and `attempt`. A runner that crashes leaves that row; the next `careerbrew day run` shows it `overdue` once past its timeout, and `careerbrew day check` retries or fails it.
- Retry covers a timeout only: a runner or subagent that crashes leaves a `running` row, which times out and gets its one retry. A step that reports `failed` itself stays failed; its `next` says how to run it again.
- `careerbrew day run --json` shows an agent only whether a step has a `verify` or a `cycle`, not the command, and only the kinds of its `watch` inputs, unless business data may show.
- The watch loop (issue #264): one pass at a time (a second one says `skipped: previous cycle running`), every 1.2 x the median of the last 10 passes, and the pass prints its next re-arm time. A `cycle` runs only for a plugin added with `--allow-exec`, and only when a `watch` input changed since its last good run. Every loop is recorded (the watch loop, a running live feed, and a running day watcher by themselves; a cron, monitor, background agent, or session with `careerbrew day loop add`); `careerbrew day loop end` stops careerbrew's own and exits 0 only when none is left running.
- A `verify` command runs as the owner, and only for a plugin added with `careerbrew plugin add <src> --allow-exec`, like a command evidence source; without it, `day update <step> --status done` is refused and names that fix. Install only plugins you trust. The `work_steps` setting takes names only; a step's `verify` and `timeout` come from a plugin manifest.
- Use generic names in a public example; a company's real targets stay in its plugin.

### Start of the day

`careerbrew day start` runs once, before stage 1 (issue #260). `careerbrew day run --json` opens with `start_of_day`: what it refreshes and what it found today.

- Refresh: it fast-forwards careerbrew's own checkout (when the `work_checkouts` setting lists `careerbrew`, the default) and each other path in that setting, each plugin's source folder, each `checkout:<path>` a step `uses`, and the git checkout each step's file resolves to (a managed clone, say). Fast-forward only: a checkout with local changes, local commits, a diverged branch, no upstream, or a detached HEAD is reported and never touched. After a plugin's folder moves, run `careerbrew plugin refresh` to install it.
- Sources: each step lists its `source` (the checkout and commit its file comes from; for a plugin step, the plugin's folder). Two folders with one remote are both refreshed, but only one is watched (`watch`): the dev checkout before a clone a step links into. The remote URL is never shown.
- Shell traps: the preflight names the shell, the sed flavor, the bash version, `python3`, and the push guard, each with its safe form. Every step reads `skills/careerbrew-work-day/shell-traps.md` first. A step that runs tests names its interpreter: a `verify` that starts with bare `python` or `python3` is refused.
- MCP connectors (macOS, issues #312 and #422): the preflight also reads each MCP server in the agent config (`~/.claude.json`; `CAREERBREW_CLAUDE_CONFIG` overrides) as the desktop app sees it through launchd. A `${VAR}` in a url or header whose launchd value is empty, a localhost url whose port does not answer, or a stdio command missing on the launchd `PATH` gets a row with its fix. For a supported Claude GitHub connector, it also reports when the launchd credential differs from the GitHub CLI credential. The CLI lookup removes `GH_TOKEN` and `GITHUB_TOKEN` from its environment, bounds its output and time, and shows no credential value. A failed lookup cannot establish a mismatch.
- Privacy: an agent (`day run --json`, `day start`) sees careerbrew's own checkout as a path and every other checkout, and each `checkout:` path a step declares, as its label (`plugin:<name>`, `uses:<step>`, `step:<step>`, `setting`), since a folder name may name the company. A plugin-owned step runs only its declared plugin's canonical resource (`path`), not a same-name global skill or legacy mirror. A missing resource returns `source_error`; mark that step failed and repair the source. Private `provides.skills` remain work-step resources on both hosts, not globally installed `$skill` entries. Published repository skills are separately namespaced `careerbrew-*` and deployed through `skillDirectories`. A plugin step's `uses: checkout:<path>` lets careerbrew run `git fetch` and `git merge --ff-only` there, never a plugin command.
- Sign-in: a step that `uses` a `browser:<name>` needs sign-in. The owner's `work_signin_probe` setting is a command whose exit 0 means the shared browser is signed in (run like `verify`: no shell, output never read). When it fails, the day gets one question record. Operator-turn browser dispatchers stay blocked until sign-in succeeds; other browser steps (and steps `after` them) run after independent work. A plugin cannot set the probe: only the owner's setting runs, and the preflight runs fixed commands only.

### Questions

A step never asks the user itself (issue #261). It saves each question as a record and goes on: `careerbrew day question add <step> "<the ask>" --option <a> --pick <a> --recheck <check>`. After the last step, `careerbrew day question recheck` runs every recheck, drops the closed items, and lists the rest with their age for one batched ask.

- A record is `{id, step, text, options, pick, createdAt, recheck}`, kept in the day's status file (encrypted). Open records carry over to the next morning. A blocked row's `--needs-you` and the sign-in ask are records too.
- `recheck`: a command whose exit 0 means still open (no shell, output never read), or an `http(s)://` link that careerbrew never fetches (an agent checks it and runs `careerbrew day question close <id>`). Empty means it stays open until closed.
- A question raised by a plugin's step (or with `--plugin <name>`) has a plugin recheck: its command runs only when the plugin was added with `--allow-exec`. Otherwise the item stays in the ask as `unknown`, with the fix.

## Slash commands

A plugin's `commands/` folder is the source of truth for its slash commands, never a hand copy in an agent config folder.

- `plugin add` and `careerbrew plugin refresh` declare the exact installed canonical command files in a private Agentfile under the data root and run agentbrew from that ownership scope. Agentbrew emits both host formats; no second command copy is created.
- To change a command, edit it in the plugin's source, then run `careerbrew plugin refresh`. Agentbrew must support individual command-file sources; missing or failed sync is an error, not success.
- Do not edit the installed copy: the next refresh replaces it.
- Do not copy a command by hand into `.claude/commands/`, `~/.claude/commands/`, or another agent's folder. A hand copy drifts from the plugin. One in `.claude/commands/` with the same name also blocks `plugin add`, because careerbrew never overwrites a command it does not own.
- careerbrew's own commands follow the same rule: `commands/` in this repo is the source, and agentbrew deploys it from `Agentfile.yaml`.

## Inbox suggestions

A plugin feed (its skill, slash command, or `checkup` step) proposes something to the owner with `careerbrew day suggest`. It never adds a fake task to get into the inbox.

```bash
careerbrew day suggest --plugin acme --ref doc-7 \
  --what "Review the roadmap doc" --why "the doc changed and names you as a reviewer" \
  --accept "Plans time to review it before Friday" [--task "Review the roadmap doc"] [--link <url>]
careerbrew day suggestions --plugin acme   # id, status (open, accepted, dismissed), ref
```

- The suggestion shows in the inbox at once (Today's "careerbrew suggests" section and `careerbrew day inbox`), with the plugin's own What, Why, and Accept text.
- careerbrew adds what it really changes to the Accept text: with `--task`, Accept adds that task to Week; without it, Accept only records the decision, which the plugin reads back with `day suggestions`.
- `--ref` is a stable id from the feed (default: the `--what` text). The same ref never shows twice, and a decided one never comes back. Sending an open ref again updates its words.
- Only an installed plugin can suggest. Rows go to `day/suggestions.jsonl` with `origin: plugin:<name>`. The day planner writes its own `{at, task}` rows to the same file; each reader skips the other's rows (issue #186). After `plugin remove`, its open suggestions drop at the next watcher cycle; `--purge-data` deletes them all, open or decided.
- The reply prints only the id, never the text, so an agent sees no business content.
- Three or more suggestions from one plugin with the same `--accept` text show as one group (issue #147): "N suggestions from plugin acme", with Accept all, Review each, and Dismiss all; each one is still decided on its own, so `day suggestions` reads each decision back.
- Pass a `--link` (the PR, the ticket) or put the PR (`owner/repo#12`) or ticket key in `--what`: when the feed or evidence careerbrew already reads shows it done (a review the owner gave, the PR merged, CI green), the suggestion leaves the list (`day suggestions` reads its status as `expired`; the owner can bring it back) or says "maybe done". `--ref` matched to a feed event's id or thread counts too.

## Feed events

A plugin that reads a tool careerbrew has no connector for sends its events to the core work feed instead of keeping its own store:

```bash
careerbrew work feed add events.jsonl --plugin acme   # or - to read stdin
```

- One JSON object per line: `id`, `at` (ISO time), `title`, and optionally `source`, `kind`, `actor` (`me`, `other`, `bot`), `to_me`, `channel`, `from`, `status`, `importance`, `url`, `thread`, `tags`, `project` (a title, or `{title}`; null is no project; the standup report matches rows by it), `text`, `personal`. The whole line is kept as the original (encrypted); the analysis keeps the facts, not the text.
- careerbrew triages each event by the owner's rules and the built-in ones, keeps `personal` events out of evidence, the day log, and the PM update, and turns the owner's own actions into evidence when a rule says so.
- A review requested from the owner is `kind: review-request` with `to_me: true`: it is an ask, and an opted-in owner gets a priority 4 notification for it (`docs/integrations.md`, Notifications).
- Ids get the plugin's name as a prefix. One bad line writes nothing. `plugin remove --purge-data` deletes the plugin's events and originals.

## Standup rows

A plugin that knows the team board (Jira, Linear, a sheet) passes the rows its owner reports on at standup. careerbrew owns the window, the matching, and the page; the plugin keeps no script for them:

```bash
my-board-export | careerbrew work standup --rows -   # or a JSON or YAML file
```

- A JSON or YAML list of rows, or `{rows: [...], groups: [...], ticket_projects: [PROJ]}`. A row has `title` and optionally `status`, `owner` (`me`, or a name in the owner's `standup_me` setting, is the owner), `mine`, `keys` (ticket keys such as `PROJ-12`), `project`, `repos` (repo aliases), `aliases` (whole words), and `url`.
- A group has `label` and at least one of `project`, `keys`, `repos`, `aliases`; `never_rows: true` makes it take its work before any row (tooling repos). Work that matches nothing is Other work.
- Ticket keys are read only for configured projects (the rows' key prefixes and `ticket_projects`), so `UTF-8` is never a key. A `project` may be null, or `{title: null}`: that is no project.
- A row shows only when it has an update in the window: work matched it, an `--extra` action names it, a `--talk` point is set, or (with `--proof`) its live check passed. A row with no update is hidden, the owner's too, and one count line says how many (issue #271).
- An empty list (`[]`) clears the rows: use it when no row passed your check. The page and the Standup view then say "No row passed the proof check", with the time the rows were last passed ("updated 09:40").
- One bad row writes nothing and names the row. The last good rows are kept encrypted (`work/standup.json`) for the next run and the web app's Standup view. The CLI prints counts and the page path (`out/standup.html`); titles print only with `--show` to the owner's terminal.

An agent or plugin can add what careerbrew does not see (issue #229). All inputs are optional, JSON or YAML, and only one of `--rows`, `--talk`, `--extra`, and `--proof` may read stdin:

```bash
careerbrew work standup --talk talk.json --extra extra.json --proof proof.json
```

- `--talk`: `{row title: string or [bullet, ...]}`. Each row takes 1 to 5 nonempty bullets, at most 200 characters each after whitespace is folded; a string is one bullet with the same limit. A list replaces that row's generated speaking bullets. The page and Standup view show a list for multiple bullets, or a paragraph for one, marked "written by an agent". Copy standup nests them under the row title. The generated fallback uses up to three latest evidence items. The API adds `talking_points`; its flat `talking_point` remains for older clients. A row that is not yours shows when it has one.
- `--extra`: `[{title, at, kind, url?, row?}]`: actions from sources careerbrew does not collect (`kind` such as `slack`, `meeting`, `mail`). Each `at` must be inside the standup window. An action with `row` joins that row's evidence ("via an agent"); one without it is matched like other work. They count as new work, so a row with no other work gets a talking point from them.
- `--proof` (issue #271): `{row title: {verdict: pass|fail, proofs: [{source, url, checked_at, quote}], reason}}`, the result of a live check (GitHub, chat, the tracker) made in this run, right before the build. With it, a row shows only when its verdict is `pass`, it has at least one proof, every proof has a source, an http(s) `url`, and a `quote`, and every `checked_at` is after the window start, no older than `standup_proof_minutes` (default 60) before the build, and not in the future. The gate runs at every build, so a proof goes stale with time. Each proof link is listed under its row. A failed row never reaches the page or the view: the CLI prints `failed row N: <reasons>`, and the row's title and your `reason` only with `--show` to the owner's terminal. This replaces passing proofs as `--extra` actions.
- Every row title must be a row passed with `--rows`. A bad input writes nothing and names the fix. Passing one input keeps the others.
- All three are kept encrypted in `work/standup-notes.json` for the current standup window only. A report in a new window ignores them, and `work standup` deletes them. The CLI prints only their counts.

Project priority is a core setting, not plugin content: a plugin skill ranks projects with `careerbrew work priority set <project> --rank <n> --match <glob>` (stored as `project_priority`), never with its own priority file. The PM update, the plan, and Next step read only that setting.

After you edit a local plugin folder, run `careerbrew plugin refresh` to reinstall it. The folder path stays in the private lock. To set up a corporate laptop from scratch, follow [corporate-machine.md](corporate-machine.md).

A worked synthetic example lives in `tests/fixtures/plugin-acme/`.

Host controls, shared memory, native feed adapters and verified legacy migration: [Agent hosts](agent-hosts.md).
