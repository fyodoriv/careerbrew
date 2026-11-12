---
description: Run the work day with careerbrew - ordered steps from careerbrew, your settings, and plugins; one status row per step; one page for the day; end of day (careerbrew-work-day skill)
allowed-tools: Bash(uv run careerbrew:*), Bash(careerbrew:*)
---

Load `$(careerbrew where)/skills/careerbrew-work-day/SKILL.md` and follow it. `$ARGUMENTS` may say "end of day".

To label an approval note, run `careerbrew work project update <project> approval "<one line>"`. A note is not proof of approval for the current PR head.
To keep standing rules for a project session, run `careerbrew work project rules-set <project> --yes --text "<one line>"`. Read them with `careerbrew work project rules <project>` on each pass; use `careerbrew work project rules-set <project> --yes --clear` to remove them. Business-data policy controls who can read them.

`careerbrew draft replace <target> --file <file>` saves and checks a local careerbrew draft. To put it in a chat tool, save it there separately and read it back there.

First run `careerbrew day start --update-only --json`, before reading any plan. This fast-forwards clean source checkouts, repairs dependencies and the launcher, refreshes installed plugins, and syncs configured agent commands. If it fails, report its failed repair or untouched checkout and stop the affected work; never build a plan from stale inputs. When it returns `updated: true`, re-read `$(careerbrew where)/commands/work.md`, the careerbrew-work-day skill, and the changed plugin step files before continuing with the new text.

After the first-turn source refresh and reload, at the start of every active /work turn, run `careerbrew day loop catch-up --json`. A stored pass or wrap-up more than 5 minutes late runs now, even on a turn started by a goal hook. A due event is claimed once; keep one recorded timer. If the host says its goal hook is active, pass `CAREERBREW_GOAL_ACTIVE=1` to the start preflight; otherwise leave the fact unknown. To move today’s wrap-up use `careerbrew day wrap --at HH:MM`; to hold automatic wrap-up and clock ending use `careerbrew day wrap --hold`. The moved wrap-up and end must both finish before midnight. For later work, hold it and explicitly run `careerbrew day end` when ready. An explicit `careerbrew day end` means end now, including while held.

Run careerbrew from anywhere as `careerbrew …` (the launcher `scripts/setup.sh` installs). Start with `careerbrew day run --json`, then run `careerbrew day start` once before the first step (refresh, shell traps, one sign-in ask): the day plan (steps, the machine rule, flow, questions, watch, end of day, and the dashboard rows). Run `/careerbrew` inline, then the quick `start_of_day.operator_turn` dispatchers in the owner's initial turn before waiting on background results. Use `careerbrew day check --json` to start each ready step and recheck after each completion; stages are an earliest-start view, never barriers. Keep each step's row current with `careerbrew day update <step> --status <status>`, and write the day's page with `careerbrew day render`. A step never asks the user mid-run: it saves each question with `careerbrew day question add`, and `careerbrew day question recheck` drops the closed ones before the one batched ask. The day ends by clock: wrap-up starts at `work_end` minus `work_wrap_minutes`, and `careerbrew day end` (or the first watch pass after `work_end`) reads the PM draft, writes the handoff, and stops the loops. This session runs only the work process: give each pressing project one session per project, reused all day (`careerbrew work session open <project> --id <id>`), which writes its events, updates, questions, and claims through careerbrew and ends with its project's handoff (`careerbrew work session end --all`).

Agent-provider gate: before you read work data with your own tools, follow rule 1 of `careerbrew-best-practices`.

Before selecting project sessions, read `pm_promises`; if `needs_recording` is true, run `day start` and re-read the plan first. For an item marked `decision-needs-review`, ask the owner to review the listed decisions; do not start its session. Cover the other Tomorrow promises first, reuse matched sessions, and use each unmatched item’s `session_project`. Count these first against the session cap. If sent text is missing, read it with an authorized chat tool and record it with `careerbrew day pm-sent`. Today’s PM draft must name the outcome or blocker of every prior promise.

To review suggested sent-PM outcomes, run `careerbrew work promise outcomes --json`. Ask the owner to confirm each decision; the owner can run `careerbrew work promise accept <id> --yes`. For `decision-needs-review`, show the prior and current state and dates, then ask the owner to choose a listed keep, reopen, or drop id. Keep restores the prior decision; reopen uses the original due date. If a choice also creates or keeps a separate promise, show that consequence and its due date before confirmation. Never choose for them. `careerbrew work promise stats --days 7 --json` shows aggregate counts. A missing receipt, invalid record, or ambiguous match is not an approved outcome.

For project delivery, read each task's full ticket or issue scope. Deliver tasks one at a time, in order, with every scope item shipped and verified before the next task. Batch foreseeable approvals up front, record blockers with `careerbrew work session ask`, and continue independent work. A plan, dry run, or questions alone is not delivery. Report each scope item as done or not done with its verification or blocker.

After each subtask, state the goal status in one line and start or name the next goal step. End an owner reply with ranked next steps they can approve or pick now. If no owner action is needed, say so and name the next autonomous step; do not invent an ask. Before ending a turn, watch each external wait with a real monitor, scheduled wakeup, or the existing work-day loop with a recheck that covers it. Record new loops with `careerbrew day loop add`; do not create a second work-day timer. Continue independent work. If the host cannot register a watch, say the wait is unwatched and why. Stop watching when work is done, blocked on the owner, or the work day ends.

Before project sessions or delivery, read `careerbrew work project list --json`. Skip projects with `quiet: true`, including projects with open PM promises; report those promises as deferred. Resume from the saved handoff on the return date. Set a quiet period with `careerbrew work project set <name> --quiet-until YYYY-MM-DD`; clear it with `--quiet-until none`.

If a planned step has `source_error`, mark that step failed and repair its declared
plugin source. Do not substitute a same-name global skill or another plugin.

careerbrew owns `/work`: every work-day feature lives in careerbrew because it is career work. With an empty setting, add steps with `careerbrew config set work_steps "/a, /b"`; read `careerbrew config` and retain existing `work_steps` entries when adding one; `config set` replaces the list. With an empty list, use `careerbrew config set work_steps '[{"step":"/a","scope":["work"],"after":["/refresh"]}]'` to set scheduling fields; a company plugin adds steps under `work_steps:` in its manifest.

<!-- careerbrew:dual-host-shared-memory -->
## Agent host contract

Use `careerbrew where` to locate this source checkout, then read
`docs/agent-hosts.md` before this workflow. Both Claude Code and ChatGPT Work/Codex
use its native-control/sequential fallback and shared-memory bootstrap, search
and session-record rules. Keep career records local under the plugin privacy policy.
