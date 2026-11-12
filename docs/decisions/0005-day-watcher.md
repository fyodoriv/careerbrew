# 5. The day watcher is a polling loop with checkpoints, not a daemon framework

- **Status**: accepted
- **Date**: 2026-10-18

## Context

A user may want careerbrew to watch the day's work tools and calendar and append results, as a toggle that is cheap and can pause or stop at any moment without losing or duplicating data.

## Decision

One Python process started by `careerbrew day watch on` (detached session, nice 10), holding an fcntl lock for single-instance safety. Control is a state file (on, paused, off) plus SIGUSR1 (wake) and SIGTERM (finish the source and exit). Each cycle runs sources one by one and checkpoints after each; result rows are appended idempotently by id with fsync. Polling happens only in work hours, with backoff from 5 to 30 minutes. CPU and memory per cycle are measured and logged. A launchd login item is optional and not installed by default.

## Consequences

A kill at any point loses at most the source in flight; the next run redoes it without duplicates. There is no push from tools (no webhooks), so freshness is minutes, not seconds. Adding a source means adding a function to `day/watch.py` SOURCES that is idempotent by id.

Update 2026-10-22 (issue #73): idempotent by id now means "one row per id, latest version". A row seen again with a newer kind or a corrected title replaces its stored copy (`store.upsert_jsonl`, under the file lock); a change in `at` alone does not. The first write of the day no longer wins forever. An id that older code stored twice (two writers raced before the lock) collapses to its last stored copy on the next write, and the last copy of an id in one batch wins, so the watcher settles instead of rebuilding every cycle.

Update 2026-10-22 (task watch-running-probe-race): the watcher still holds an exclusive lock, but a status probe (`watch.running()`) now tests it with a shared lock. Probes at once (the web server answers reads on threads) never see each other as a watcher, and a starting watcher waits up to `PROBE_WAIT_S` (2 s) for a probe in flight instead of refusing to start; another watcher is still refused at once (test: `tests/unit/test_watch_probe.py`).
