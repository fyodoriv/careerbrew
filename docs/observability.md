# Observability

How to see what the program did, what broke, and how fast it is, without
reading user data. Built in from the first commit, not added later.

## Logs

- Gate and e2e timings: `.metrics/events.jsonl`. App events: `private/logs/events.jsonl` (writer: `src/careerbrew/events.py`). Neither is committed.
- Fields: `ts`, `event` (`component.action`, for example `setup.done`), `story`
  (the user story id, such as `US-01`, when the event belongs to one), `ok`,
  `seconds`, and ids or counts only.
- Content-free: never log user text, file contents, or secrets. Log a hash or a
  count instead. An error message that could quote user content is dropped.
  Enforce it in the writer: a string field is kept only when its key is in
  `events.ALLOWED_STRING_KEYS` and its value matches `^[a-z0-9_.:-]{1,48}$`;
  any other string field is dropped without a warning. A new string field
  (such as `action`) needs its key in that list, with fixed values only.
  `test_work_project_event_fields_are_allow_listed_short_ids` fails when a
  literal string field would be dropped or an `action` is not a fixed short id.
- Tests turn the log off with `CAREERBREW_EVENTS=off` (or point `CAREERBREW_HOME` at a temp dir), so
  a test run never writes the user's log. Assert that in a test.

| Event | When | Fields |
|---|---|---|
| `check.step` | each step of `scripts/check.sh` | `name`, `seconds`, `ok` |
| `cli.run`, `cli.error` | every command | `command`, `status` / `error` (exception class); `cli.run` also `ms` (duration) and, for an action that replaces manual work, `saves` (its id in `time_saved.ACTIONS`; read by `careerbrew metrics time-saved` and `metrics field`) |
| `web.timing` | the web app shows a PM update, a standup, or a suggestion (`open`), and on Copy, Send, or Accept (`POST /api/timing`) | `saves` (`pm_update`, `standup`, `inbox`), `step` (`open`, `copy`, `send`, `accept`), `seconds` (open: the wait for the data; the others: your visible-tab time since the open, or since the last Accept); read by `careerbrew metrics time-saved` |
| `workday.update` | `careerbrew day update` wrote a `/work` status row (US-94) | `status` (the row's status), `needs_you` (true or false), `error_class` (a failed row's class, else empty; US-100); never the step name or its text |
| `workday.verify` | `careerbrew day update <step> --status done` ran the step's `verify` command (US-100) | `ok` (true or false); never the command, its output, or the step name |
| `workday.start` | `careerbrew day start` refreshed the checkouts, checked the shell, and probed sign-in (US-102) | `checkouts`, `fast_forwarded`, `not_touched`, `traps` (counts), `signin` (ok, failed, no probe, none needed); never a path, a commit, or a step name |
| `workday.question` | a work-day question record was saved (`action` `add`) or closed (`close`) (US-104) | `action`, `source` (`agent`, `owner`, `row`, `careerbrew`, `plugin`), `recheck` (`none`, `url`, `signin`, `command`), `options` (a count); never the step name, the text, or the recheck |
| `workday.recheck` | `careerbrew day question recheck` ran the batched ask (US-104) | `checked`, `ask`, `dropped`, `agent`, `unknown` (counts) |
| `workday.loop` | a work-day watch pass ran or was skipped, a loop was recorded, or end of day ran (`careerbrew day loop`, US-103) | `status` (`ok`, `failed`, `skipped`, `added`, `ended`), `steps`, `failed`, `seconds`, `interval`, `stopped`, `left` (counts), `kind_owned`; never a step name, a command, or a loop id |
| `workday.end` | the work day switched to wrap-up by clock (`phase` `wrap-up`) or ended (`ended`, `careerbrew day end` or the clock) (US-106) | `status` (`wrap-up`, `ended`, `held`, `moved`), `action` (`remind`, `end`, `control`), `by_clock` (true: the clock ended the day), `reminded` (true: the pop-up showed), `prs`, `questions`, `loops_left` (counts), `pm_edited`; never item text or the draft |
| `workday.morning` | `careerbrew day start` reran each item of the last handoff (US-106) | `items`, `closed`, `open` (counts) |
| `workday.timeout` | `careerbrew day check` found a running step past its timeout (US-100) | `retry` (true: set back for its one retry; false: marked failed), `status`; never the step name |
| `draft.fit` | `careerbrew draft fit` judged a reply draft (US-98) | `status` (the kind: `none`, `emoji`, `ack`, `stop`, `reply`), `stale`, `deploy_started` (true or false); never thread or draft text |
| `draft.replace` | `careerbrew draft replace` saved a reply draft (US-98) | `status` (`ok`, `mismatch`, `not_cleared`, `busy`, `claimed`: another project session claimed the target, US-107), `chars` or `line` (counts only); never the target or the text |
| `telemetry.send` | each telemetry send (US-89) | `status`, `count`, `error` |
| `jobs.source`, `jobs.rate`, `jobs.learn`, `jobs.strategy`, `jobs.application` | job-hunt steps; a missing optional source credential records `jobs.source` status `missing_secret` | `source_kind`, `status`, `tier`; never the credential, reference or error payload |
| `tutor.attempt` | each graded drill | `status` (FSRS rating), `score` |
| `tutor.voice` | a voice or typed answer graded by `drill speak` or the web app | `status` (`audio` or `typed`), `words` (count only), `score` |
| `cards.add`, `cards.review` | a flashcard added or graded | `status` (the grade) |
| `notion.import` | a Notion bundle imported | `status`, `items` (count only) |
| `cards.publish` | public study cards published | `status`, `items` (cards published) |
| `teach.step` | a teacher step finished | `status` (outcome), `seconds` |
| `teach.feedback` | written feedback for the teacher | `status` |
| `exercise.submit` | an exercise attempt graded | `status` (pass or fail), `seconds` |
| `exercise.sync` | solved problems synced from a public LeetCode profile | `status`, `items` |
| `goals.update` | the owner changed goals | `status` (upcoming or long_term), `items` |
| `jobs.choose` | the owner chose a job | `tier`, `status` |
| `approvals.request`, `approvals.decide` | an agent asked; the owner decided | `status` |
| `data.encrypt` | encryption at rest turned on or off | `status` (`on` or `off`), `items` (files changed) |
| `data.show` | `careerbrew data show` read one data file | `status` (`text`, `keys`, or `hidden`: business text refused), `items` (lines or key paths printed) |
| `data.set` | profile fields updated without an editor, or update refused | `status` (`ok` or `fail`), `items` (fields updated; success only) |
| `work.collect`, `work.collect.error`, `work.note`, `work.promo`, `work.plan` | work steps | `new`, `errors`, `coverage`, `source_kind`, `story` |
| `connector.run`, `secret.missing` | each connector call | `source_kind`, `items` |
| `connector.reconnect` | Microsoft refused an Outlook refresh token: the owner must run `connect outlook` | `source_kind`, `status` |
| `connector.feed` | a connector read for the work feed | `source_kind`, `items` |
| `work.feed.ingest`, `work.feed.pull`, `work.feed.error` | events stored in the work feed; a pull of every feed connector; a source that failed | `source_kind`, `new`, `updated`, `pruned`, `sources`, `errors`, `error`, `story` |
| `work.agent.event` | a background agent reported one step (`work agent event`, issue #272); `status` is running, done, or failed | `status`, `story` |
| `work.session` | a project session was opened, reused, or refused, asked a question, wrote its handoff, or ended (`careerbrew work session`, US-107) | `action` (`open`, `ask`, `handoff`, `end`), `status` (`ok`, `reused`, `refused`, `missing_handoff`), `error` (`cap`, `one_per_project`), `live`, `ended`, `missing` (counts); never a project name, a session id, or text |
| `work.claim` | a project session took, renewed, or released a claim, or found it held (`careerbrew work claim`, US-107) | `status` (`taken`, `renewed`, `held`, `released`, `none`), `minutes`; never the resource or the session |
| `work.feed.cycle` | an explicit direct batch ended (`work feed cycle`); `stored` counts physical stored sources, while `not_stored` counts required completion-coverage gaps, including unknown clocks; `status` is `gap` when coverage fails | `status`, `required`, `stored`, `not_stored` (counts), `story` |
| `work.feed.transcript` | tool replies stored from an agent session transcript (issue #234); `status` is `gap` when a mapped source stored nothing | `status`, `new`, `updated`, `skipped`, `errors`, `gaps` (a count), `story` |
| `work.live.listen` | a live cycle did the follow-up work for its new events (issue #266, US-101): drafts, link research, review notes, project updates | `status`, `events`, `needs_you` (counts), `story` |
| `work.live.start`, `work.live.stop`, `work.live.cycle`, `work.live.error` | the live feed started (issue #267) or stopped; one cycle closed (`status` is `failed` when required acquisition is missing, invalid, unknown or stale; unchanged fresh validated replies do not count as a new store); one source's adapter failed | `status`, `sources`, `required`, `was_running`, `not_stored` (a count), `stored`, `source_kind`, `error`, `story` |
| `work.standup`, `work.standup.rows` | the standup report was built and its page written; a plugin or agent passed the rows | `status`, `rows` (rows shown; for `.rows`, rows passed), `hidden` (rows with no update), `actions`, `on_rows`, `groups`, `gated`, `passed`, `failed` (rows that passed or failed the proof check, issue #271) |
| `work.standup.notes` | an agent passed talking points, extra actions, or proofs for the current standup window (issues #229, #271) | `status`, `talk` (talking points kept), `extra` (extra actions kept), `gated` (proofs were passed: rows are gated on them), `proofs` (rows with a proof check) |
| `work.review_ask`, `work.review_ask.check` | a review ask was built for a PR (issue #270); a draft was checked | `status` (`ready`, `short`; for `.check`: `ok`, `refused`), `needed`, `named`, `groups` (counts), `files`, `source` (`branch protection`, `rulesets`, or `unknown`) |
| `work.team` | a teammate was added (`add`), had ids filled in (`set`), or was removed (`remove`) from the roster; a plugin's roster file is not YAML and was skipped (`plugin-roster`, `status` `error`) | `status`, `action`, `count` |
| `work.feed.replies` | the Slack reader read the replies of the threads it found (`count`, `skipped` threads it could not read), or stopped (`error`, such as `missing_scope` or `ratelimited`) | `status`, `count`, `skipped`, `error` |
| `work.feed.team_search` | a teammate search stopped on an error (`error`, such as `ratelimited`); the pull keeps what it found | `status`, `error` |
| `work.feed.research` | what each link of one kept event says was stored | `status`, `count`, `opened` |
| `work.feed.retriage`, `work.feed.mark` | the rules ran again over kept originals; the owner marked an event personal or work, or an ask `not-ask`, `done`, or `ask` again (`action`, issue #185) | `status`, `count`, `changed`, `personal`, `dropped`, `action` |
| `backup.create`, `backup.restore` | backups | `targets`, `files` |
| `plugin.add`, `plugin.remove` | plugin lifecycle | `status`, `purged` |
| `machine.split_evidence` | per-person sources and connectors moved from a plugin's evidence file to private files | `status` (`ok`, `none`, `dry_run`), `moved`, `skipped` (plugins with no folder here), `left` (plugin files still flagged) |
| `checkup` | `careerbrew checkup` finished | `status`, `failed` |
| `report.public` | `careerbrew report public` built the repo health export, or refused it | `status` (`ok` or `refused`), `steps`, `days` (counts) |
| `report.write` | a long run wrote its run page (Blockers, Next, Done) | `command` (the run: `repo-checkup`, `start`, `review`, `work-collect`, `day-plan`, `checkup`, `machine-ready`, `machine-new-user`, `plugin-new`), `status` (`ok` or `blocked`), `done`, `blockers`, `next` (counts) |
| `work.focus` | `careerbrew work focus` ranked the horizons | `status`, `horizons` |
| `work.ladder_grade` | a PM update was graded against the next ladder level (`day pm-grade`) | `status`, `lines`, `strong`, `none` (counts only), `story` |
| `work.task` | a task was added (`work task`, the web app, or an accepted suggestion) | `status` (`open` or `proposed`), `source_kind` (`user`, `agent`, `proposal`), `links`, `web_links` (counts: links given, and how many are full http(s) URLs the web app links), `story` |
| `work.task` | a task was added (`work task`, the web app, or an accepted suggestion), regraded (`action` `regrade`), or its impact recorded (`action` `impact`, `work impact`) | `status` (`open` or `proposed`), `source_kind` (`user`, `agent`, `proposal`), `links`, `web_links` (counts: links given, and how many are full http(s) URLs the web app links), `story` |
| `day.log` | the daily log was rebuilt | `status`, `items` |
| `day.inbox`, `day.inbox.accept`, `day.inbox.answer` | proposals and questions regenerated, accepted, or answered | `status`, `items`, `kind` |
| `day.inbox.suggest` | an agent task or a plugin suggestion was put in the inbox at once | `status` (`new`, `updated`, `decided`), `source_kind` (`agent`, `plugin`) |
| `day.inbox.group` | Accept all or Dismiss all decided a group of alike suggestions (issue #147) | `status` (`ok`, or `partial` when a member failed and stays open), `action` (`accept`, `dismiss`), `source_kind` (the group's kind: `source`, `new-task`, `agent-task`, `plugin`), `items` (decided), `failed`, `story` (`us-74`) |
| `day.inbox.expire` | a cycle found asks that data careerbrew already reads shows done elsewhere (issue #147) | `status`, `expired` (hidden, kept), `maybe` (left open, marked maybe done), `story` |
| `day.inbox.reopen` | the owner brought back a suggestion hidden as done elsewhere | `status`, `story` |
| `config.set` | a setting was changed | `status` |
| `web.untracked` | a start of the web app (`careerbrew open`, `careerbrew`, a notification click) stopped careerbrew servers on its port that `day/web.json` did not track (issue #118) | `status` (`stopped`), `count` |
| `web.token` | a new web sign-in token was made (issue #141); never the token itself | `status` (`first`; `request`: `web rotate-token`; `age`: past `web_token_max_days`; `adopted`: the per-run token an older version kept in `day/web.json`) |
| `web.session` | a tab signed in (issue #141) | `route` (`set`: a tab with the token got the cookie; `restore`: a tab got the token back from the cookie; `open`: the sign-in page's button opened a signed-in tab), `status` (`ok`, `refused`), `code` (the HTTP status of a refusal), `cookie` (the request carried the sign-in cookie) |
| `notify.check` | a notification check (each watcher cycle, or `notify check`) | `status` (`sent`, `scheduled`, `not_sent`, `quiet`, `cap`, `batch`, `off`, `ok`: nothing new to pop up), `source_kind` (the notifier: `alerter`, `terminal-notifier`, `osascript`, `none`), `new`, `sent`, `batched`, `held`, `capped`; for detached alerter, `sent` means the presenter was scheduled, not that display was confirmed |
| `notify.act` | the owner acted on a notification | `status` (`read`, `accept`, `dismiss`, `snooze`, `open`), `source_kind` (`popup`, `ui`, `cli`) |
| `notify.remind` | a one-off reminder pop-up showed (the work day's wrap-up, US-106) | `status`, `source_kind` |
| `notify.error` | the notifier failed, or a click came after the item was decided elsewhere | `source_kind`, `error`, `story` |
| `metrics.weekly`, `metrics.weekly.skip` | the opt-in weekly metrics file was written, or one metric could not be measured | `status`, `items`, `source_kind`, `error` |
| `day.pm.send` | the PM update was posted to Slack | `status`, `source_kind` |
| `day.pm.draft` | the owner saved (or reset) the day's PM update; `status: refused` when a save or send named a PR or ticket key outside a link (issue #227) | `status`, `edited`, `unlinked` (count of lines) |
| `day.pm.learned` | after an edit was saved or a sent update was learned (issue #238): how many PM lessons the edits make (lines cut, labels renamed, parts cut, kinds of added lines) over how many edited days | `status`, `source_kind` (`web` or `sent`), `cut`, `rename`, `parts`, `added`, `count` |
| `signals.reset` | the owner reset one feedback signal (`careerbrew signals reset`) | `status`, `count` |
| `work.project` | a project was added, got a source, was checked or edited (`set`), its standing rules changed (`rules`), a sheet was imported, a source was marked a reference or tracked again (`reference`), `check --since` listed what changed (`since`; `count` is how many sources changed), a project was archived or watched again (`archive`, `unarchive`), or a change on it was noted (`update`) | `status`, `action`, `count`, `added`, `updated`; never standing rule text |
| `work.promise` | promises you made were found in new feed events (`track`; `count` is how many; `status` is `error` with the error's type when the projects file could not be written, and the feed events are kept), added by hand (`add`), synchronized from sent Tomorrow bullets (`pm-update`; count is the sent bullet count, emitted only when promise records change), worked (`research`, `draft`, `status`), or closed (`done`, `dropped`) | `status`, `action`, `count`, `error` |
| `work.prep` | a prep project's gate was marked (`gate`; `step` is `tool`, `links`, or `questions`, `undo` true when opened again) or a quiz result was stored (`quiz`; `status` is `fail` when it had a miss) | `status`, `action`, `step`, `undo`, `asked`, `missed` |
| `work.priority` | a project was ranked, moved, or removed in `project_priority` | `status`, `action`, `projects` |
| `work.memory.in` | a changed project memory file updated its project (only when a file changed or failed to read) | `status`, `files`, `sources`, `proposed`, `links`, `count` (status lines), `errors` |
| `work.memory.out`, `work.memory.forget` | work items written into project memory files as dated notes; notes of events that became personal removed | `status`, `notes`, `projects`, `errors`, `dropped` |
| `learn.start`, `learn.concepts`, `learn.answer` | a learn project staged its sources, imported its concepts, or recorded one answer (US-80) | `status` (an answer's result), `mode`, `sources`, `changed`, `concepts` (counts) |
| `learn.replay`, `learn.tour` | a merged PR became a replay task, its tests were run on your change, or replay coverage was measured; a code tour was added, approved, or exported | `action`, `status` (`pass`/`fail`, or a tour's `approved`/`unreviewed`/`stale`), `files`, `prs`, `share`, `steps`, `faded` |
| `work.explain` | a project explanation outline was written, or a doc was checked | `status`, `problems` (-1 when only the outline was written) |
| `work.prd` | a PRD outline was written, or a PRD was checked (issue #223) | `status`, `problems` (-1 when only the outline was written) |
| `sync.export`, `sync.import` | machine sync | `source_kind` (role), `items` |
| `llm.call`, `llm.unavailable` | model routing | `route` |
| `review.run` | outcome review | `pivots` |
| `work.linkedin` | LinkedIn block built | `bullets`, `route` |
| `day.cycle`, `day.source.error`, `day.watch.start`, `day.watch.stop` | each watcher cycle and lifecycle; an invalid sent-message receipt is preserved and ignored with `source_kind: pm-receipt`, `error: InvalidReceipt` (never its contents) | `added`, `cpu_ms`, `rss_mb`, `source_kind`, `error`, `story` |
| `day.plan` | daily plan built | `tasks`, `meetings` |
| `web.api` | the web app's API answered a read | `status`, `route` (the view, such as `daylog` or `feed.asks`; a company, page name, or run page id in the path is never logged), `cached` (answered from memory), `ms` |
| `web.warm` | the web server built its warm views at start or after new data (issue #142) | `status` (`ok`, or `error` when a warm cycle failed), `views`, `rebuilt` (how many were not in memory yet), `ms` |
| `calendar.error`, `mail.fetch`, `mail.error` | calendar and mail reads | `source_kind`, `messages`, `error` |
| `jobs.linkedin.import` | LinkedIn export imported | `positions`, `saved`, `applied` |
| `prep.check` | preparation check run | `score`, `gaps` |

A cold PM read can create an empty sent-day control lock. That file is not a
business input and must not force a second cache miss. The version scanner
excludes only an empty canonical lock directly under `day/`; real sent-source
and receipt edits still invalidate the response. `test_pm_sent_cache.py` pins
this boundary without reading or logging lock contents.

## Field telemetry (US-89)

The event log is also the source of field telemetry: `src/careerbrew/telemetry.py` sends its allowlisted fields to PostHog EU (`/batch/`) when a key is set. Turn off: `careerbrew telemetry off` or `DO_NOT_TRACK=1`. The send obeys the plugin host policy (`allowed_hosts`). See the exit row in [`privacy.md`](privacy.md). Tests set `CAREERBREW_POSTHOG_KEY=""` and a dead `CAREERBREW_TELEMETRY_URL`, so no test reads the Keychain key or sends anything.

## Errors

- Every caught error writes an event with `ok: false`, the `story` it breaks, and an error code.
- External error reporting (for example Sentry) sits behind one adapter and is
  off unless `ERROR_REPORTER` is set in `.env`. Local file first.
- A failure that maps to a user story is a broken story: fix it before new work.
- This profile keeps personal data: errors stay local unless `docs/privacy.md` allows a service.

## Metrics

Each metric has a goal and a pivot (the value at which we change approach).
Show missing data as `(stub)`, never as a silent zero. Some sources answer 0
for a thing that does not exist (for example `gh` for an unknown label): check
that the thing exists first. Pin the exact query in the Command column; one
missing qualifier can change the number several times over.

| Metric | Goal | Pivot | Command |
|---|---|---|---|
| Success metric: share of outcome metrics in KEEP (see `docs/VISION.md`) | >= 75% | < 50% after 12 weeks | `uv run careerbrew review` |
| Capability coverage (build) | 17/17 | < 15/17 | `scripts/metrics.sh capability-coverage` |
| Errors per day, by story | 0 broken stories | any story failing 2 days running | `grep '"cli.error"' private/logs/events.jsonl \| cut -c1-11 \| sort \| uniq -c` |
| Speed of `work collect` and `jobs scan` (wall time) | < 10 s with cache | > 60 s | `time uv run careerbrew work collect` |
| Watcher CPU per cycle (p95) | < 500 ms | > 2000 ms | `grep '"day.cycle"' private/logs/events.jsonl` (field `cpu_ms`) |
| Watcher peak memory | < 120 MB | > 300 MB | same events, field `rss_mb` |
| Notifications acted on (opened, accepted, or dismissed) per pop-up | >= 0.5 | < 0.5 after 2 weeks: pop up pages only, keep the rest in the web app | `grep -c '"notify.act"' private/logs/events.jsonl` against the sum of `sent` in `"notify.check"` events |
| Web view shown again (a cached repeat, US-64) | < 100 ms | > 1000 ms | `uv run python scripts/api-timing.py` (seed); `grep '"web.api"' private/logs/events.jsonl` (fields `cached`, `ms`) |
| Warm views answering their first open from memory, at start and after new data (US-64, issue #142) | 14/14 | < 12/14 | `uv run python scripts/api-timing.py` (seed); `grep '"web.warm"' private/logs/events.jsonl` (fields `rebuilt`, `ms`) |
| Gate time (`scripts/check.sh`) | under the gate budget | 2x the budget | `scripts/metrics.sh` |
| Cost per run (tokens or money) | $0 (local route or fallback) | any paid call without an ADR | `grep '"route": "paid"' private/logs/events.jsonl \| wc -l` |
| LLM judge agreement with the owner's labels (US-66) | >= 85% on >= 20 labels | < 70%: switch to code graders per criterion | `evals/run.sh agreement` |

Compare the main metrics with the closest competitor in `docs/VISION.md` → Prior art.

## Health check

`scripts/doctor.sh` prints one `ok` or `FAIL` line per check, each with its fix.
In code, make the fix part of the failing check's type, so a check without a
fix does not compile (or fails a test).
Add a check for every new dependency and for each way the main story can break.

## Checkup and caching

`scripts/checkup.sh` is the one repo-wide checkup. It ends with a run page
(`private/out/reports/repo-checkup-*.html`: Blockers, Next, Done, and each
section's text folded under Details) and prints only the page path and one
summary line; `careerbrew report latest --json` gives agents the same facts with
no page text. `scripts/check.sh` skips a
cache-eligible step that passed while its declared inputs are unchanged (an input hash, like the
nx cache). The leak scan, format, lint, `types`, unit, exercises, `web-types`, e2e and Rust steps do not reuse cache entries.
The `types` step runs each available root checker; an unavailable Python checker
is still skipped. The separately named `web-types` and e2e steps also run each time: synthetic
external-input changes can make their old cached passes stale. With per-task caching (Nx, Turborepo), list every file a task
reads as its input, including root files a package's tests open, and prove it:
edit one file and check that the tasks reading it miss the cache. Cache hits log `"cached":true`. Use `NO_CACHE=1` after a tool
upgrade. For big suites, add affected-only test selection per stack
(`references/stacks.md` in the new-project skill).

The input hash covers file content only (tracked and untracked files that git
does not ignore). It is the same for `fast` and full mode, and the same before
and after `git add` or `git commit`. So the pre-push hook's `scripts/check.sh
fast` reuses eligible checks on the same files. Format, lint and unit checks (Python and root
JavaScript), exercise checks, web type checks, end-to-end tests, Rust checks, and `leaks` always
run because their external inputs are not fully keyed. The `rust-inventory` step checks tracked
source assignments against the staged Git index on every Rust gate. A changed file reruns each affected cached step. Sensitive
terms and plugin sources can change outside the tracked tree. The `secrets`
step (gitleaks reads commits) also keys on `HEAD`, so each new commit
is scanned. The key copies the index with its mtime (`cp -p`): git re-reads a
file as new as the index (racy git), so a same-size edit in the second of a
`git add` still changes the key. Each cacheable step has one
file in `.metrics/cache/`, scoped to one checkout or worktree
(`tests/unit/test_gate_cache.py`). `scripts/input-key.sh` computes the key.

Some steps have inputs, like nx targets: `inputs_of` in `scripts/check.sh`
gives them git pathspecs that leave out notes no code reads (`TASKS.md`,
`RECURRING.md`, `docs/learnings/`), and `scripts/input-key.sh <pathspec>...`
hashes only the matching files. So a notes-only change re-runs the doc steps
and `unit` (which reads every file). End-to-end and web type checks still run;
other eligible steps can reuse a pass. `scripts/check.sh inputs <step>...` prints a step's pathspecs.
`tests/unit/test_gate_inputs.py` greps every code file such a step runs for a
left-out path and for walks over the whole repo, and fails on any hit not on its
reviewed list. This check does not qualify external runtime inputs. A cached step logs
`check.step` with `cached: true`, as before. `.metrics/e2e-results.json` still
uses the key over every file.

`scripts/e2e.sh` records each story file's result (`ok` or `fail`) under the
same key in `.metrics/e2e-results.json`, only when the key did not change
during the run. `scripts/metrics.sh capability-coverage` runs each distinct story once per
report, even after a green gate. It shares a result only among duplicate rows
in that report; external inputs can change between runs without changing the
repository key. It prints the number of distinct stories run
(`tests/unit/test_e2e_results.py`). Other saved-result consumers still report
their recorded run and are not qualified as fresh by this rule.

## Speed review

Each week (`RECURRING.md`), run `scripts/metrics.sh`. If a step got slower or
the gate is over its budget, file a task to make it faster.

- **Gate budget**: `scripts/check.sh` finishes in under 120 seconds on this machine.
