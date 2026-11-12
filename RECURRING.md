# Recurring tasks

Work that fires on a cadence instead of waiting in `TASKS.md`. When a task
fires, do it, then bump its `**Last-fired**:` date in the same commit. These
tasks never close. Each one ends by filing tasks in `TASKS.md`, not by fixing
everything at once.

## P1

- [ ] Speed review: make the slowest step faster
  - **ID**: weekly-speed-review
  - **Cadence**: weekly
  - **Details**: Run `scripts/metrics.sh`. If the gate is over its budget in
    `docs/observability.md`, or a step got slower, file one task for the biggest
    win. Also review agent time if your agent logs it (agentbrew `perf-review` skill).

- [ ] Health checkup: doctor, errors by story, open questions
  - **ID**: weekly-checkup
  - **Cadence**: weekly
  - **Details**: Run `scripts/checkup.sh`, then `NO_CACHE=1 scripts/check.sh` once to
    catch tool drift the cache cannot see. A broken story becomes a P0 task. Re-check last
    week's learning files (a re-check is a new file). End with at most three questions for the
    owner (context, options, recommended option first), then apply the answers.

## P2

- [ ] Prior-art and kill-criteria review
  - **ID**: quarterly-vision-review
  - **Cadence**: quarterly
  - **Details**: Re-check `docs/VISION.md`: did a competitor ship the core?
    Does a kill criterion hold? Update the Prior art table and the metrics compare.
