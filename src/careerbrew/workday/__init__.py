"""The work-day runner behind `/work` (issue #257): one ordered plan for the day, one status row per step, one page.

careerbrew owns every work-day feature because each one is career work: it reads or writes the
owner's work record. agentbrew only deploys careerbrew's `commands/work.md`.

Layout (each follow-up issue extends one module, never the CLI glue):
- `steps.py`: where steps come from and what each one runs. Built-in steps first, then the
  `work_steps` setting, then each installed plugin's `work_steps:` manifest field (by plugin
  name). A step is a name or a mapping; its order fields (#258: scope, after, long_running, uses)
  are read in `steps.entry`.
- `stages.py`: which steps run at once (#258): `after` orders stages, a shared `uses` entry or an
  overlapping `scope` splits them, a step that declares neither runs alone, and long-running steps
  start last. Each step gets its `stage` and a `why`.
- `plan.py`: `build()` turns the steps into the day plan that `careerbrew day run --json` prints:
  the machine rule, the steps, how to run them, the questions rule, watch, end of day, and the
  dashboard (rows, statuses, the commands that update and render it).
- `status.py`: the day's status rows in `<data root>/day/work-<day>.json` (encrypted at rest like
  other day files), one row per plan step; `careerbrew day update <step>` writes one under the
  file's lock, so parallel subagents never lose a row.
- `gate.py`: the truth rules (#259): a step's `verify` proves `done`, a running step past its
  `timeout` gets one retry (`careerbrew day check`) and then fails, and a blocked row carries its
  question while a failed row carries an error class and a next action.
- the page: `runs.work_day()` turns the plan and its rows into a run page (`report.finish`),
  which `careerbrew day render` writes; business text (a summary, a needs-you line, a link) goes
  into each item's `detail`, so it shows on the page only, never in the agent summary.

Lifecycle: nothing is kept in process memory; every call reads the settings, the plugin lock,
and the day's status file again.
"""
