# Telemetry tiles

The product tiles for careerbrew's field telemetry (US-89), one section per tile. Each tile names its question, the exact HogQL query, and the events and fields it reads. The owner makes the tiles in PostHog from these queries; this file names them by title only (no key, no person, no dashboard link).

What the events hold, and what they never hold, is in [`observability.md`](observability.md) ("Field telemetry") and the exit row in [`privacy.md`](privacy.md). A tile with no data yet shows "empty until <condition>"; it is never filled with a made-up event.

## User activity: the fixed command list

An install counts as active only on a `cli.run` event whose `command` is one a person runs. Background work never counts: the watcher, `telemetry send`, the web app's reads (`web.api`), and loops. The list is `telemetry.USER_COMMANDS` in `src/careerbrew/telemetry.py`; keep this copy equal to it:

`start`, `open`, `review`, `checklist`, `day.log`, `day.plan`, `day.start`, `day.end`, `day.send`, `day.check`, `day.next`, `cards.review`, `cards.grade`, `drill.session`, `drill.answer`, `exercises.submit`, `teach`, `teach.next`, `jobs.scan`, `jobs.log`, `jobs.choose`, `work.task`, `work.collect`, `work.standup`, `work.note`, `work.promo`, `learn.start`, `prep.check`.

The queries below write it as `USER_COMMANDS`; paste the list in its place as `('start', 'open', ...)`.

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

```sql
WITH weekly AS (
  SELECT DISTINCT distinct_id, toStartOfWeek(timestamp, 1) AS week
  FROM events
  WHERE event = 'cli.run' AND properties.command IN USER_COMMANDS AND timestamp > now() - INTERVAL 26 WEEK
)
SELECT w0.week AS cohort_week,
       count(DISTINCT w0.distinct_id) AS active,
       count(DISTINCT w2.distinct_id) AS active_two_weeks_later,
       round(count(DISTINCT w2.distinct_id) / count(DISTINCT w0.distinct_id), 3) AS retention
FROM weekly AS w0
LEFT JOIN weekly AS w2 ON w2.distinct_id = w0.distinct_id AND w2.week = w0.week + INTERVAL 2 WEEK
WHERE w0.week <= toStartOfWeek(now(), 1) - INTERVAL 2 WEEK
GROUP BY cohort_week
ORDER BY cohort_week
```

## Error rate per command

- **Question**: which commands fail most often, and how slow are they?
- **Reads**: `cli.run` (`command`, `status`, `duration_ms`) and `cli.error` (`command`, `error`).
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
- **Empty until**: the first full ISO week after a yes ends and the watcher runs.

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
- **Reads**: `workday.update` (`step_kind`, `seconds`, `status`; a row that ended).
- **Empty until**: an install that said yes ends a `/work` step.

```sql
SELECT properties.step_kind AS kind, properties.status AS ended_as, count() AS steps,
       quantile(0.5)(toFloat(properties.seconds)) AS median_seconds,
       quantile(0.9)(toFloat(properties.seconds)) AS p90_seconds
FROM events
WHERE event = 'workday.update' AND properties.seconds IS NOT NULL AND timestamp > now() - INTERVAL 8 WEEK
GROUP BY kind, ended_as
ORDER BY kind, ended_as
```

## Upgrades per week

- **Question**: how fast do installs move to a new careerbrew version?
- **Reads**: `careerbrew.upgrade` (`from_version`, `app_version`).
- **Empty until**: an install that said yes runs a new careerbrew version.

```sql
SELECT toStartOfWeek(timestamp, 1) AS week, properties.from_version AS from_version, properties.app_version AS to_version, count(DISTINCT distinct_id) AS installs
FROM events
WHERE event = 'careerbrew.upgrade' AND timestamp > now() - INTERVAL 26 WEEK
GROUP BY week, from_version, to_version
ORDER BY week, installs DESC
```
