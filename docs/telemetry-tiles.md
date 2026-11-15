# Telemetry tiles

The product tiles for careerbrew's field telemetry (US-89), one section per tile. Each tile names its question, the exact HogQL query, and the events and fields it reads. The tiles live on the PostHog dashboard **careerbrew: product and performance**, the same way agentbrew and dotfiles keep theirs: one "Product: <title>" table per tile section below (every section between the command list and the tech tiles), plus the four tech tiles in the last section. This file names them by title only (no key, no person, no dashboard link). `tests/unit/test_telemetry_tiles.py` checks that every query reads only events careerbrew sends and that the command list equals the code.

What the events hold, and what they never hold, is in [`observability.md`](observability.md) ("Field telemetry") and the exit row in [`privacy.md`](privacy.md). A tile with no data yet shows "empty until <condition>"; it is never filled with a made-up event.

## User activity: the fixed command list

An install counts as active only on a `cli.run` event whose `command` is one a person runs. Background work never counts: the watcher, `telemetry send`, the web app's reads (`web.api`), and loops. The list is `telemetry.USER_COMMANDS` in `src/careerbrew/telemetry.py`; keep this copy equal to it:

`start`, `open`, `review`, `checklist`, `day.log`, `day.plan`, `day.start`, `day.end`, `day.send`, `day.check`, `day.next`, `cards.review`, `cards.grade`, `drill.session`, `drill.answer`, `exercises.submit`, `teach`, `teach.next`, `jobs.scan`, `jobs.log`, `jobs.choose`, `work.task`, `work.collect`, `work.standup`, `work.note`, `work.promo`, `learn.start`, `prep.check`.

The queries below write it as `USER_COMMANDS`; paste the list in its place as `('start', 'open', ...)`. In the same way, `WEB_ACTS` stands for `telemetry.WEB_ACTS`, pasted as an array: `['inbox.accept', 'inbox.dismiss', ...]`.

## Active installs per week

- **Question**: how many installs did a person use each week?
- **Reads**: `cli.run` (`command`, `distinct_id`, `timestamp`).
- **Empty until**: the first install that said yes runs a listed command.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, count(DISTINCT distinct_id) AS active_installs
FROM events
WHERE event = 'cli.run' AND properties.command IN USER_COMMANDS AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week
ORDER BY week
```

## New installs per week

- **Question**: how many new installs said yes each week?
- **Reads**: `careerbrew.install` (`distinct_id`, `timestamp`).
- **Empty until**: the first setup followed by a yes.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, count(DISTINCT distinct_id) AS new_installs
FROM events
WHERE event = 'careerbrew.install' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week
ORDER BY week
```

## Setup success rate and median setup time

- **Question**: what share of setups finish with a passing doctor, and how long does a setup take?
- **Reads**: `careerbrew.install` (`ok`, `duration_ms`, `tier`).
- **Covers**: only setups that got as far as a working `careerbrew` command ([`metrics.md`](metrics.md)).
- **Empty until**: the first setup followed by a yes.

```sql
SELECT properties.tier AS tier,
       count() AS installs,
       round(avg(toFloat(properties.ok = true)), 3) AS success_rate,
       quantile(0.5)(toFloat(properties.duration_ms)) / 1000 AS median_seconds
FROM events
WHERE event = 'careerbrew.install' AND timestamp > now() - INTERVAL 90 DAY
GROUP BY tier
```

## 14-day retention

- **Question**: of the installs active in a week, how many are active again two weeks later?
- **Reads**: `cli.run` (`command`, `distinct_id`, `timestamp`).
- **Empty until**: an install has been active for at least three weeks.
- **Why `nullIf`**: PostHog's ClickHouse fills a LEFT JOIN row with no match with `''`, not NULL, so a plain `count(DISTINCT w2.distinct_id)` counts the empty string and shows 100% retention for a week nobody came back.

```sql
WITH weekly AS (
  SELECT DISTINCT distinct_id, toStartOfWeek(timestamp, 1) AS week
  FROM events
  WHERE event = 'cli.run' AND properties.command IN USER_COMMANDS AND timestamp > now() - INTERVAL 26 WEEK
)
SELECT w0.week AS cohort_week,
       count(DISTINCT w0.distinct_id) AS active,
       count(DISTINCT nullIf(w2.distinct_id, '')) AS active_two_weeks_later,
       round(count(DISTINCT nullIf(w2.distinct_id, '')) / count(DISTINCT w0.distinct_id), 3) AS retention
FROM weekly AS w0
LEFT JOIN weekly AS w2 ON w2.distinct_id = w0.distinct_id AND w2.week = w0.week + INTERVAL 2 WEEK
WHERE w0.week <= toStartOfWeek(now(), 1) - INTERVAL 2 WEEK
GROUP BY cohort_week
ORDER BY cohort_week
```

## Error rate per command

- **Question**: which commands fail most often, and how slow are they?
- **Reads**: `cli.run` (`command`, `status`, `duration_ms`) and `cli.error` (`command`, `error`, the lowercase exception class).
- **Note**: before careerbrew 0.1.0's fix of 2026-11-15, `cli.error` lost its `error` field and a bare `careerbrew` run had no `command`; older rows group under an empty command.
- **Empty until**: the first command after a yes.

```sql
SELECT properties.command AS command,
       count() AS runs,
       round(countIf(event = 'cli.error' OR properties.status != 'ok') / count(), 3) AS error_rate,
       quantile(0.5)(toFloat(properties.duration_ms)) AS median_ms,
       quantile(0.95)(toFloat(properties.duration_ms)) AS p95_ms
FROM events
WHERE event IN ('cli.run', 'cli.error') AND timestamp > now() - INTERVAL 30 DAY
GROUP BY command
HAVING runs >= 5
ORDER BY error_rate DESC
```

## Weekly outcome medians

- **Question**: in a typical week, how much does an install get done with careerbrew?
- **Reads**: `careerbrew.week` (`days_active`, `cards_reviewed`, `exercises_solved`, `work_days_started`, `evidence_items`).
- **Empty until**: an ISO week that ends after the yes is over, and the next daily send runs (any ordinary command or the watcher).

```sql
SELECT toStartOfWeek(timestamp, 1) AS sent_week,
       count(DISTINCT distinct_id) AS installs,
       quantile(0.5)(toFloat(properties.days_active)) AS days_active,
       quantile(0.5)(toFloat(properties.cards_reviewed)) AS cards_reviewed,
       quantile(0.5)(toFloat(properties.exercises_solved)) AS exercises_solved,
       quantile(0.5)(toFloat(properties.work_days_started)) AS work_days_started,
       quantile(0.5)(toFloat(properties.evidence_items)) AS evidence_items
FROM events
WHERE event = 'careerbrew.week' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY sent_week
ORDER BY sent_week
```

## Job funnel per week

- **Question**: how many applications reach each stage each week?
- **Reads**: `jobs.application` (`status`: `applied`, `screen`, `onsite`, `offer`, `rejected`, or `withdrawn`; `tier`).
- **Empty until**: an install that said yes logs an application with `careerbrew jobs log`.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, properties.status AS stage, count() AS applications, count(DISTINCT distinct_id) AS installs
FROM events
WHERE event = 'jobs.application' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week, stage
ORDER BY week, stage
```

## Web views used per week

- **Question**: which web app views do installs open?
- **Reads**: `web.view` (`route`, a name from `telemetry.VIEW_ROUTES`; at most one per view per day per server process).
- **Empty until**: an install that said yes opens the web app.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, properties.route AS view, count(DISTINCT distinct_id) AS installs, count() AS view_days
FROM events
WHERE event = 'web.view' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week, view
ORDER BY week, installs DESC
```

## Work-day step durations

- **Question**: how long does a `/work` step take, for careerbrew's own step and for workflow steps?
- **Reads**: `workday.update` (`step_kind`, `seconds`, `status`, `changed`; `seconds` leaves only on the move from `running` to done, blocked, or failed, so it is run time, never owner wait; a later edit sends `changed` false, and a timeout sends its own move).
- **Decision**: a step kind whose p90 is far above its median, or whose median rises after a release, gets a timeout or a split next.
- **Empty until**: an install that said yes ends a `/work` step.

```sql
SELECT properties.step_kind AS kind, properties.status AS ended_as, count() AS steps,
       quantile(0.5)(toFloat(properties.seconds)) AS median_seconds,
       quantile(0.9)(toFloat(properties.seconds)) AS p90_seconds
FROM events
WHERE event = 'workday.update' AND properties.changed = true AND properties.status IN ('done', 'blocked', 'failed') AND properties.seconds IS NOT NULL AND timestamp > now() - INTERVAL 8 WEEK
GROUP BY kind, ended_as
ORDER BY kind, ended_as
```

## Upgrades per week

- **Question**: how fast do installs move to a new careerbrew version: what share of each week's active installs runs each version?
- **Decision**: when 90% or more of active installs run version N or later, drop the shims for data formats older than N. A release still under half of active installs two weeks after it ships means `day start --update-only` should nudge harder.
- **Reads**: `cli.run` (`app_version`, stamped when the line was logged, not when it was sent; `command` from the fixed list). `careerbrew.upgrade` (`from_version`) also leaves, for a from-to view.
- **Empty until**: the first install that said yes runs a listed command. Until the version moves past `0.1.0` (task `release-version-bump`), every row is one version.

```sql
SELECT week, version, installs, round(installs / sum(installs) OVER (PARTITION BY week), 3) AS share
FROM (
  SELECT toStartOfWeek(timestamp, 1) AS week, properties.app_version AS version, count(DISTINCT distinct_id) AS installs
  FROM events
  WHERE event = 'cli.run' AND properties.command IN USER_COMMANDS AND timestamp > now() - INTERVAL 26 WEEK
  GROUP BY week, version
)
ORDER BY week, installs DESC
```

## Work days by version

- **Question**: of the `/work` steps a day ends with, what share did `/work` finish alone, and did a new careerbrew version raise or lower it?
- **Decision**: a version whose done share drops, whose stuck steps rise, or whose steps waiting on the owner rise, is a regression to read before the next change to `/work`.
- **Reads**: `workday.end` with `action` `end` (`steps_done`, `steps_blocked`, `steps_failed`, `steps_running`, `steps_waiting`, `needs_you`, `app_version`; counts only, from `workday.end.step_outcomes`). A step still `running` at day end hung or crashed: the `stuck` column.
- **Empty until**: an install that said yes ends a work day (`careerbrew day end`, or the clock). Until the version moves past `0.1.0` (task `release-version-bump`), every row is one version.

```sql
SELECT properties.app_version AS version, count(DISTINCT distinct_id) AS installs, count() AS work_days,
       sum(toFloat(properties.steps_done)) AS done, sum(toFloat(properties.steps_blocked)) AS blocked,
       sum(toFloat(properties.steps_failed)) AS failed, sum(toFloat(properties.steps_running)) AS stuck,
       sum(toFloat(properties.steps_waiting)) AS waiting,
       round(sum(toFloat(properties.steps_done)) / nullIf(sum(toFloat(properties.steps_done) + toFloat(properties.steps_blocked) + toFloat(properties.steps_failed) + toFloat(properties.steps_running)), 0), 3) AS done_share,
       quantile(0.5)(toFloat(properties.needs_you)) AS median_needs_you
FROM events
WHERE event = 'workday.end' AND properties.action = 'end' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY version
ORDER BY arrayMap(x -> toInt64OrZero(x), splitByChar('.', version))
```

## Work-day step failures by class

- **Question**: when a `/work` step fails, why: the environment, a sign-in, the network, a conflict, or the tool itself?
- **Decision**: the top class is the next `/work` fix (an `auth` lead means better sign-in checks at `day start`).
- **Reads**: `workday.update` with `status` `failed` and `changed` true (`step_kind`, `error_class`, one of `workday.gate.ERROR_CLASSES`), so a later edit of a failed row is not a second failure.
- **Empty until**: an install that said yes has a `/work` step fail.

```sql
SELECT properties.step_kind AS kind, properties.error_class AS failed_because, count() AS failures, count(DISTINCT distinct_id) AS installs
FROM events
WHERE event = 'workday.update' AND properties.status = 'failed' AND properties.changed = true AND timestamp > now() - INTERVAL 8 WEEK
GROUP BY kind, failed_because
ORDER BY failures DESC
```

## Web controls used

- **Question**: which web app controls did installs use in the last 8 weeks, which ones nobody used, and which ones refuse the click?
- **Decision**: a control with 0 clicks is a candidate to move behind a "⋯" menu; a high refused share is a control whose input the page lets through wrongly, and any error share is a bug to fix.
- **Reads**: `web.act` (`route`, a name from `telemetry.WEB_ACTS`; `status` `ok`, `refused`, or `error`; `duration_ms`). Every control in `WEB_ACTS` gets a row, so an unused one shows 0.
- **Empty until**: never empty: the rows are the control list. The counts stay 0 until an install that said yes clicks a web control that changes data.

```sql
SELECT c.control AS control, coalesce(a.installs, 0) AS installs, coalesce(a.clicks, 0) AS clicks,
       a.refused_share AS refused_share, a.error_share AS error_share, a.median_ms AS median_ms
FROM (SELECT arrayJoin(WEB_ACTS) AS control) AS c
LEFT JOIN (
  SELECT properties.route AS control, count(DISTINCT distinct_id) AS installs, count() AS clicks,
         round(countIf(properties.status = 'refused') / count(), 3) AS refused_share,
         round(countIf(properties.status = 'error') / count(), 3) AS error_share,
         quantile(0.5)(toFloat(properties.duration_ms)) AS median_ms
  FROM events
  WHERE event = 'web.act' AND timestamp > now() - INTERVAL 8 WEEK
  GROUP BY control
) AS a ON a.control = c.control
ORDER BY clicks DESC, control
```

## Work-day funnel and asks

- **Question**: of the work days `/work` starts, how many end, how often does it stop to ask the owner, do step proofs pass, and do steps time out?
- **Decision**: days that start but never end mean the wrap-up is lost (fix `day end` first); asks added faster than closed mean `/work` asks too much; a falling verify pass share or rising timeouts point at the step to fix next; sign-in failures at start mean the probe or the tokens need work before stage 1.
- **Reads**: `workday.question` `add` is sent once per new ask (a re-save of an open ask sends nothing). `workday.start` (`traps`, a count; `signin`, the sign-in probe result; `day start` can run more than once a day, so starts and ends count install-days), `workday.end` with `action` `end`, `workday.question` (`action` `add` or `close`), `workday.verify` (`ok`), `workday.timeout`. Counts and fixed verbs only.
- **Empty until**: an install that said yes runs `careerbrew day start`.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, count(DISTINCT distinct_id) AS installs,
       count(DISTINCT if(event = 'workday.start', concat(distinct_id, toString(toDate(timestamp))), NULL)) AS days_started,
       count(DISTINCT if(event = 'workday.end' AND properties.action = 'end', concat(distinct_id, toString(toDate(timestamp))), NULL)) AS days_ended,
       round(avgIf(toFloat(properties.traps), event = 'workday.start'), 2) AS traps_per_start,
       countIf(event = 'workday.start' AND properties.signin = 'failed') AS signin_failed,
       countIf(event = 'workday.question' AND properties.action = 'add') AS asks_added,
       countIf(event = 'workday.question' AND properties.action = 'close') AS asks_closed,
       round(countIf(event = 'workday.verify' AND properties.ok = true) / nullIf(countIf(event = 'workday.verify'), 0), 3) AS verify_pass_share,
       countIf(event = 'workday.timeout') AS timeouts
FROM events
WHERE event IN ('workday.start', 'workday.end', 'workday.question', 'workday.verify', 'workday.timeout') AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week
ORDER BY week
```

## Where commands run

- **Question**: do people run careerbrew in a terminal, from the web app, or through an agent such as `/work`?
- **Decision**: the surface most runs use gets the next polish first; a command only agents run needs its agent docs kept current.
- **Reads**: `cli.run` (`via`: `cli`, `ui`, or `agent`; `command` from the fixed list).
- **Empty until**: the first command after a yes.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, properties.via AS via, count(DISTINCT distinct_id) AS installs, count() AS runs
FROM events
WHERE event = 'cli.run' AND properties.command IN USER_COMMANDS AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week, via
ORDER BY week, runs DESC
```

## Tech tiles: command timings

- **Question**: which commands are slow, and did a version or runtime (Python now, Rust later) make them faster or slower?
- **Reads**: `cli.run` (`duration_ms`, `command`, `app_version`, `runtime`).
- **Tiles**: four PostHog trends over 30 days, one line per value: `careerbrew p50 ms by command` (median of `duration_ms`, broken down by `command`), `careerbrew p95 ms by command` (95th percentile), `careerbrew p50 ms by version` (by `app_version`), `careerbrew p50 ms by runtime` (by `runtime`). They are the same as the agentbrew and dotfiles timing tiles, so the Rust migration (`docs/rust-migration.md`) can compare runtimes on real use.
- **Empty until**: the first command after a yes.
