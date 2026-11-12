# User stories

Each story is something you can do end to end. Every story has:
- a **Mode**: job search, job helper (your current job), or platform;
- acceptance criteria written as "Given …, when …, then …";
- an outcome **Metric** you feel (not a test count) with a **Target**;
- the **Skill** an agent follows for it;
- the **Web views** it shows (or `none`), and for those, approved pictures of its key screens in `web/visual/` (US-75);
- its e2e test, which names the story id.

`scripts/check-stories.py` (a gate step) fails when any of these is missing. `scripts/check-docs.sh` fails when this file and `tests/e2e/` drift apart. The persona throughout is one programmer who wants the best next role, ideally remote, and wants to grow at the current job until then.

## Shared host acceptance

Every story explicitly inherits `dual-host-shared-memory`:
- Given Claude Code or ChatGPT Work/Codex, when its CLI, agent or UI entry runs,
  then the existing action registry supplies the same action or a clear terminal-only
  reason; host controls use available native tools or a sequential alternative.
- Given shared semantic memory, when an agent starts and finishes a workflow,
  then it bootstraps/searches relevant procedures and records reusable decisions;
  private career records remain in Careerbrew under its existing policy.
- Given configured shared-memory tool failure, when an agent is asked to continue
  context-dependent work from docs alone, then it names the unavailable tool,
  blocks context-dependent readiness until repair, and offers deterministic CLI-only
  work. Intentionally unconfigured memory remains an explicit no-recall limit.
- Given a new story, command or skill without this contract, when the existing
  story gate runs, then it fails. Native account connections and privacy opt-ins
  are not copied; missing tools or authorization remain explicit limits.

See [Agent hosts](agent-hosts.md) for runtime, deployment and regression details.

# Platform

## US-01: Set up the project with one command

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a new user (or agent), **I want** to run one command, **so that** I can start in minutes.
- **Acceptance criteria**:
  - Given a fresh clone, when I run `scripts/setup.sh`, then it installs everything and ends with `doctor: all checks passed`.
  - Given a missing tool, when the doctor runs, then the failing line names the exact fix.
- **Metric**: time from clone to a passing `scripts/doctor.sh`. **Target**: under 5 minutes.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-01-setup.sh` | `check.step` |

## US-12: Install and remove a corporate plugin

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** an employee, **I want** to add my company's ladder, sources, terms, and policy, and remove them cleanly, **so that** company content never sits in the base repo.
- **Acceptance criteria**:
  - Given a plugin folder or git URL, when I run `plugin add`, then its ladder and policy apply and `plugin verify` reports it intact.
  - Given a changed plugin file, when I run `plugin verify`, then it reports TAMPERED.
  - Given an installed plugin, when I run `plugin remove --purge-data`, then its files, its origin-tagged records, and derived outputs are gone (drafts and project updates made from them stay).
- **Metric**: failed `plugin.add` events per month. **Target**: 0.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: Setup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-12-plugin.sh` | `plugin.add`, `plugin.remove` |

## US-13: Keep private and business data from leaking

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user, **I want** no term from my profile, terms file, or plugins to reach logs, output, backups, git, or a hosted model, **so that** I can use careerbrew at work safely.
- **Acceptance criteria**:
  - Given canary terms in my data, when every command runs, then no canary appears in output, logs, or backups.
  - Given a staged file with a sensitive term or under `private/`, when I commit, then the leak check blocks it without printing the term.
  - Given business content and a piped stdout, when I pass `--show`, then careerbrew refuses.
  - Given a draft issue (from the `/careerbrew-checkup` last step) that names a sensitive term or a `private/` path, when the agent runs `privacy scan --file` on it, then the scan fails without printing the term, and the agent posts only a draft that passes.
- **Metric**: leak findings from `careerbrew privacy scan` in the gate. **Target**: 0.
- **Skill**: `skills/careerbrew-privacy-guard`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-13-privacy.sh` | `cli.error` |

## US-15: Use a corporate and a personal machine together

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** an employee who job-hunts, **I want** corporate data to stay on the corporate machine, and job-hunt data never to reach it, **so that** both machines help me without risk.
- **Acceptance criteria**:
  - Given a corporate-role machine, when I run a job-hunt command, then it refuses and points to the personal machine.
  - Given paired machines, when I export from the corporate machine, then it refuses: careerbrew never copies corporate data to your personal machine, and a personal machine refuses a corporate bundle.
  - Given a corporate machine, when the data folder or a backup target is in a cloud-synced folder, or a model route is hosted, then readiness fails or the exit refuses.
  - Given a bundle from the personal machine, when the corporate machine imports it, then only known settings apply.
- **Metric**: job-hunt records found on corporate-role machines. **Target**: 0.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: none (US-37 shows how the web app hides job-hunt views on a corporate machine)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-15-machines-sync.sh` | `sync.export`, `sync.import` |

## US-22: Set up a plugin for a new job in one command

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** someone starting a new job, **I want** one command that creates, installs, and connects the company plugin, **so that** day one takes minutes, not an afternoon.
- **Acceptance criteria**:
  - Given a company name and tools, when I run `careerbrew plugin new <company> --connect jira,github`, then a private plugin repo is created with a ladder, terms, connectors, and safe policy, and it is installed.
  - Given the plugin, when setup finishes, then I see one copyable `careerbrew secret set` command per token, with no company name in it.
  - Given a non-empty target folder, when I run it again, then it refuses.
- **Metric**: `plugin_setup_minutes` in `careerbrew review` (first install to first collected evidence; `plugin refresh` keeps the first install time, and evidence older than it is skipped, so the value is never negative). **Target**: under 10.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-22-plugin-new.sh` | `plugin.add` |

## US-26: Trust every recommendation because it cites a source

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user, **I want** every strategy rule, threshold, interview tip, and company fact to cite a public source, **so that** I can check it before I bet my one interview on it.
- **Acceptance criteria**:
  - Given the knowledge files, when the gate runs `scripts/check-sources.py`, then any rule, tip, or company fact without a known source fails the gate.
  - Given a company guide, when I run `careerbrew interview <company> --show`, then each company-specific line shows its source URL.
- **Metric**: unsourced claims in the gate. **Target**: 0.
- **Skill**: `skills/careerbrew-company-research`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-26-sources.sh` | `check.step` |

## US-27: Every story has a skill and a test

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who works through an agent, **I want** each story to name the skill that handles it, with evals, **so that** the agent does the right thing for every request.
- **Acceptance criteria**:
  - Given the stories, when `scripts/check-stories.py` runs, then a story without a mode, criteria, metric, skill, or test fails the gate.
  - Given the skills, when `evals/run.sh` runs, then each skill has an act case and a refrain case, and CLI cases pass.
- **Metric**: stories without a skill or test. **Target**: 0.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-27-stories-skills.sh` | `check.step` |

# Job search mode

## US-02: Scan job boards and filter by my criteria

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** postings from public company boards filtered by my hard criteria, **so that** I only look at jobs I would take.
- **Acceptance criteria**:
  - Given Greenhouse, Lever, and Ashby boards, when I run `jobs scan`, then postings are normalized with salary, level, skills, and region.
  - Given my criteria, when the scan finishes, then I see how many passed and why the rest were rejected.
  - Given an unknown board kind, when I scan, then I get a clean error, not a traceback.
- **Metric**: share of `jobs.source` events with `ok:false` per week. **Target**: 0.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-02-jobs-scan-filter.sh` | `jobs.source` |

## US-03: Get a proposed Want score, rate jobs, and let it learn

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** a 0–100 Want score with reasons, and to rate jobs so the weights refit, **so that** the ranking matches what I actually want.
- **Acceptance criteria**:
  - Given filtered postings, when I run `jobs list`, then each shows its Want score and top reasons.
  - Given at least 5 ratings, when I run `jobs learn`, then new weights are kept only if they agree with my ratings at least as well.
  - Given an unknown posting id, when I rate it, then careerbrew refuses.
- **Metric**: Spearman agreement between Want and my ratings (`want_agreement`). **Target**: >= 0.6.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-03-want-rate-learn.sh` | `jobs.rate`, `jobs.learn` |

## US-04: Get a season strategy

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker with one loop per company per season, **I want** a plan that orders learning, practice companies, target companies, and dream companies, **so that** I never spend a dream loop unprepared.
- **Acceptance criteria**:
  - Given rated postings, when I run `jobs strategy`, then each company sits in exactly one tier and the current phase is marked.
  - Given readiness below the gate, when I ask about dream companies, then the plan keeps me in practice.
  - Given a company whose loop I already used, when the plan rebuilds, then that company is listed as used this season.
- **Metric**: share of dream applications sent with readiness >= 0.8. **Target**: 100%.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-04-strategy.sh` | `jobs.strategy` |

## US-05: Drill with spaced repetition and rubric grading

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** the right drill each day, graded on binary criteria, **so that** I improve on what I miss.
- **Acceptance criteria**:
  - Given a new item, when I run `drill next`, then the lesson comes before the question.
  - Given my answer, when I grade it with `drill answer --met`, then the FSRS rating and the missed criteria come back.
  - Given an unknown criterion, when I grade, then careerbrew refuses.
- **Metric**: retention (reviews not rated "again", 30 days). **Target**: >= 0.8.
- **Skill**: `skills/careerbrew-interview-drill`
- **Web views**: none (voice practice in Interviews grades drill items: US-57)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-05-drill.sh` | `tutor.attempt` |

## US-06: See my readiness as charts

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** one HTML file with mastery, score trend, and readiness over time, **so that** I know when I am ready.
- **Acceptance criteria**:
  - Given attempts, when I run `drill progress`, then a self-contained HTML file with SVG charts and table views is written.
  - Given no attempts, when I run it, then charts say "No data yet" instead of failing.
- **Metric**: readiness on core topics. **Target**: >= 0.7 before target companies.
- **Skill**: `skills/careerbrew-interview-drill`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-06-progress-chart.sh` | `cli.run` |

## US-17: Know every step of a company's interview, with tips

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** the full interview loop for a company, with what each step evaluates, tips, and traps, **so that** nothing in the loop surprises me.
- **Acceptance criteria**:
  - Given a seeded company, when I run `careerbrew interview <company> --show`, then every step of its sourced loop appears with company notes, tips, traps, and sources.
  - Given a company with no sourced loop, when I run it, then the shared default loop for my level appears, marked as the default.
  - Given my pipeline, when I run `careerbrew jobs interview-gaps`, then every pipeline company without a sourced loop is listed, best first, with the research step.
- **Metric**: `loops_sourced` in `careerbrew review` (share of dream and target companies with a sourced loop). **Target**: 100%.
- **Skill**: `skills/careerbrew-interview-coach`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-17-interview.sh` | `cli.run` |

## US-18: Check my preparation interactively before an interview

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** a step-by-step readiness check for a company's loop, **so that** I fix the biggest gap first.
- **Acceptance criteria**:
  - Given a company, when I run `careerbrew prep check <company>`, then every step's checklist is asked (or read from `--yes`), a readiness score is shown, and gaps are sorted by weight with the action that closes each.
  - Given drill readiness and my story bank, when the check runs, then those items are measured, not asked.
  - Given repeated checks, when I run it again, then the score trend shows.
- **Metric**: `prep_score` in `careerbrew review` (latest prep score, companies checked in 14 days). **Target**: >= 0.8.
- **Skill**: `skills/careerbrew-interview-coach`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-18-prep.sh` | `prep.check` |

## US-19: Find remote and nearby jobs on the best open platforms

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** jobs from Himalayas, Remotive, Remote OK, We Work Remotely, HN Who is hiring, and Adzuna, with remote roles limited to regions I did not allow filtered out, **so that** I see only roles in regions I allowed.
- **Acceptance criteria**:
  - Given the remote preset, when I run `jobs scan`, then postings from every platform are normalized, each credited and linked to its source page.
  - Given a missing optional source credential, when I run `jobs scan`, then it warns and saves available sources; if none completes, it fails and preserves previous postings.
  - Given remote roles limited to regions I did not allow, when criteria apply, then they are rejected with reason "region".
  - Given an existing sources file, when I run `jobs preset` again, then it refuses without `--force`.
- **Metric**: `eligible_postings` in `careerbrew review` (postings that pass criteria). **Target**: >= 30.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-19-job-platforms.sh` | `jobs.source` |

## US-20: Bring LinkedIn in without scraping

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a job seeker, **I want** my LinkedIn data export and job-alert emails to feed careerbrew, **so that** saved jobs, past applications, and alerts are in one place.
- **Acceptance criteria**:
  - Given my export zip, when I run `jobs linkedin-import`, then positions, skill suggestions, saved jobs, and applications are imported by header name, without duplicates on re-import.
  - Given a LinkedIn job-alert email, when I run `mail scan`, then it becomes a posting.
  - Given a missing archive, when I import, then I get a clean error.
- **Metric**: share of LinkedIn saved jobs present in `jobs list`. **Target**: 100%.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-20-linkedin-import.sh` | `jobs.linkedin.import` |

## US-21: Catch interview emails in Outlook, Fastmail, and Proton

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** careerbrew to read mail metadata from my accounts, **so that** interview and recruiter emails never slip.
- **Acceptance criteria**:
  - Given Outlook (Graph), Fastmail (JMAP), and Proton (Bridge IMAP) accounts, when I run `mail scan`, then messages are classified as LinkedIn alerts, applications, interview emails, or other, from metadata only.
  - Given an account that cannot connect, when I scan, then the error is reported and the other accounts still work.
  - Given a corporate-role machine, when I scan, then personal accounts are skipped and job mail is not processed.
- **Metric**: `interview_followup` in `careerbrew review` (interview emails followed by a logged stage within 2 days). **Target**: >= 90%.
- **Skill**: `skills/careerbrew-inbox-triage`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-21-mail.sh` | `mail.fetch`, `mail.error` |

# Job helper mode

## US-07: Collect evidence of my work

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** my commits, PRs, and tool activity in one deduplicated store, plus notes, **so that** nothing I do is forgotten.
- **Acceptance criteria**:
  - Given git sources, when I run `work collect` twice, then the second run adds nothing.
  - Given a broken source, when I collect, then it is counted with its reason and the others still collect.
- **Metric**: evidence_freshness_days. **Target**: <= 7.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-07-evidence.sh` | `work.collect`, `work.collect.error` |

## US-08: Back up my data encrypted and restore it

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** an encrypted, verified backup, **so that** a lost laptop does not lose my case.
- **Acceptance criteria**:
  - Given my data, when I run `backup create`, then each target gets an encrypted file that verifies.
  - Given a changed backup file, when I verify it, then verification fails.
  - Given `backup_icloud` set to true, when I run `backup create`, then an encrypted copy also lands in iCloud Drive/careerbrew and restores (off by default; refused on a corporate machine).
- **Metric**: share of `backup.create` runs that verify. **Target**: 100%.
- **Skill**: `skills/careerbrew-weekly-review`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-08-backup.sh` | `backup.create`, `backup.restore` |

## US-09: Write my promotion doc and see what is missing

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** the best promotion doc my evidence supports, plus gaps by priority, **so that** I file the strongest case and know what to do next.
- **Acceptance criteria**:
  - Given evidence and a ladder, when I run `work promo`, then a doc with evidence per competency and a priority-sorted gap list is written.
  - Given an unknown level, when I run it, then I get a clean error.
- **Metric**: promo_coverage. **Target**: >= 0.8.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-09-promo.sh` | `work.promo` |

## US-10: Rank my tasks

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** my tasks ranked by deadline, impact, size, and promotion gaps, **so that** I work on what matters.
- **Acceptance criteria**:
  - Given tasks, when I run `work plan --hours N`, then the plan fills N hours with the best-ranked tasks that fit.
  - Given a finished task, when I run `work done <id> <hours>`, then the actual hours feed calibration.
- **Metric**: estimate_ratio (actual / planned). **Target**: <= 1.3.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-10-plan.sh` | `work.plan` |

## US-11: Know when to pivot

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** a user, **I want** every outcome metric labeled KEEP, WATCH, or PIVOT with the pivot action, **so that** careerbrew learns from real data and changes course.
- **Acceptance criteria**:
  - Given a metric past its pivot threshold, when I run `review`, then it shows PIVOT and the action.
  - Given no data, when I run `review`, then it shows no-data, never zero.
- **Metric**: share of metrics in KEEP. **Target**: >= 75%.
- **Skill**: `skills/careerbrew-weekly-review`
- **Web views**: Review

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-11-review.sh` | `review.run` |

## US-14: Connect Slack, Google Docs, Jira, GitHub, Zoom, and Outlook securely

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** my work tools connected with tokens in the Keychain or 1Password, **so that** evidence collects itself without secrets on disk.
- **Acceptance criteria**:
  - Given six connectors with secret references, when I run `work collect`, then each adds tagged evidence.
  - Given an Outlook connector and no `calendars.yaml`, when I plan the day, then its Microsoft Graph calendar events count as meetings.
  - Given a literal token in config or a piped token, when I connect, then careerbrew refuses.
- **Metric**: share of `connector.run` events with `ok:false` per week. **Target**: 0.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: Setup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-14-connectors.sh` | `connector.run`, `secret.missing` |

## US-16: Get a LinkedIn block of my results

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** a paste-ready LinkedIn Experience description of my results, **so that** my profile shows impact without anything confidential.
- **Acceptance criteria**:
  - Given evidence, when I run `work linkedin --show`, then impact-first bullets under 2,000 characters appear, scrubbed of terms, ids, links, and paths, plus a list of suspect words to review.
  - Given a policy with `public_metrics: false`, when I run it, then numbers are removed.
- **Metric**: runs blocked by the final leak check. **Target**: 0.
- **Skill**: `skills/careerbrew-linkedin-profile`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-16-linkedin.sh` | `work.linkedin` |

## US-23: A light background watcher fills my results-of-the-day list

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** a toggle-able watcher that checks my work tools and calendar through the day, **so that** my results list fills itself and costs almost nothing.
- **Acceptance criteria**:
  - Given `day watch on`, when work happens, then new evidence, finished meetings, and interview emails are appended to today's list, once each, and mail and calendars are read at most every 15 minutes.
  - Given `day watch pause`, when work happens, then nothing is appended until `day watch resume`, which continues from the last checkpoint.
  - Given a killed watcher, when it starts again, then it resumes without duplicates; each cycle stays under 500 ms CPU and 120 MB memory.
  - Given results with UTC, explicit-offset, or local timestamps, when I run `day results --show`, then the times use my configured time zone and rows follow chronological order; malformed or missing times stay listed with a blank time (issue #334).
- **Metric**: `watcher_cpu_ms` in `careerbrew review` (p95 CPU per cycle, last 50 cycles). **Target**: < 500.
- **Skill**: `skills/careerbrew-day-watch`
- **Web views**: Setup, Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-23-day-watch.sh` | `day.cycle`, `day.watch.start` |

## US-24: Know what to work on next

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** careerbrew to suggest the next task from my calendar, plan, and priorities, **so that** every free slot goes to the right work.
- **Acceptance criteria**:
  - Given a meeting within 10 minutes, when I ask `day next`, then it says to prepare for the meeting.
  - Given a free slot, when I ask, then it names the best-ranked task that fits the slot, with the reason.
  - Given no earlier recommendation fits and an open ask fits the time left, when I open Today, then Next step shows the oldest eligible ask with its source link. The CLI gives the action without revealing the source link (#374).
  - Given the end of work hours, when I ask, then it says to review the day.
  - Given the watcher is on, when the first cycle of a workday runs, then it builds the plan and notifies "Today's plan is ready" with counts only.
- **Metric**: `suggestion_follow_rate` in `careerbrew review` (suggested tasks done the same day). **Target**: >= 60%.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-24-day-next.sh` | `day.plan` |

## US-25: Plan my day from facts

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** a time-blocked plan from my meetings, free time, deadlines, promotion gaps, and how long tasks really take, **so that** my plan survives the day.
- **Acceptance criteria**:
  - Given Outlook or ICS calendars, when I run `day plan`, then tasks are placed in free blocks only, inflated by my real estimate ratio, with yesterday's unfinished tasks first.
  - Given a task too big for today, when I plan, then it is listed as not today.
  - Given a literal private ICS link in config, when I plan, then it is refused.
- **Metric**: `plan_done_rate` in `careerbrew review` (last workday's planned tasks done). **Target**: >= 70%.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-25-day-plan.sh` | `day.plan`, `calendar.error` |

## US-28: Import my company's real promotion criteria

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** my company's actual promotion criteria as a ladder, **so that** my promotion doc is graded against the real bar, not a generic one.
- **Acceptance criteria**:
  - Given a ladder YAML from my company's criteria, when I run `careerbrew ladder import <file>`, then it validates and appears in `ladder list`, and `work gaps --ladder <id>` uses it.
  - Given a ladder missing an expectation for a level, when I import it, then careerbrew refuses and names the competency.
- **Metric**: share of promotion docs graded against a company ladder (not the generic one). **Target**: 100% at companies with published criteria.
- **Skill**: `skills/careerbrew-promo-research`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-28-ladder-import.sh` | `cli.run`, `cli.error` |

# Platform (v3)

## US-29: A second person installs careerbrew on personal and corporate machines

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a colleague at the same company (another team), **I want** one install command for my personal and my corporate machine with our shared company plugin plus my team plugin, **so that** I get the same company rules without sharing any data with my colleague.
- **Acceptance criteria**:
  - Given a clean clone, when I run `scripts/install.sh --role corporate --plugin <company> --plugin <team>`, then both plugins install, the team ladder becomes my default, and the strictest policy applies.
  - Given a team plugin that `extends` the company plugin, when the company plugin is missing, then install refuses, and the company plugin cannot be removed while the team plugin needs it.
  - Given two people, when both use careerbrew, then neither data root contains the other's data.
  - Given Apple Silicon and an x86_64 `.venv`, or a CLI that does not start, when I run install, then it rebuilds an arm64 venv or exits non-zero with the fix, never a silent exit 0.
- **Metric**: failed `plugin.add` events during installs. **Target**: 0.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-29-second-person.sh` | `plugin.add` |

## US-30: Update careerbrew's code while it runs

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user (or an agent improving careerbrew), **I want** code updates to take effect while the watcher runs, **so that** improvements land with no lost state and almost no interruption.
- **Acceptance criteria**:
  - Given a running watcher, when its code changes, then between cycles it compiles the new code, re-executes in place (same pid), and keeps its state, pause, lock, and results.
  - Given a broken update, when the check fails, then the watcher keeps running the previous code and reports the failure.
  - Given `careerbrew day watch reload`, when the watcher reaches a safe point, then it loads the current code.
- **Metric**: `watcher_cpu_ms` in `careerbrew review` (reloads must not raise the cost per cycle). **Target**: < 500.
- **Skill**: `skills/careerbrew-day-watch`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-30-hot-reload.sh` | `day.watch.reload` |

## US-31: Pivot when the company or its promotion process changes

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** an employee, **I want** careerbrew to notice when the promotion ladder, a company plugin, or a company's hiring process changes, **so that** my promotion case, plans, and strategy follow the new rules.
- **Acceptance criteria**:
  - Given an updated ladder, when `careerbrew changes` runs (or the watcher's hourly check), then the change is recorded, the promotion case is re-assessed, and today's plan is rebuilt.
  - Given a changed company loop or source page, when changes run, then the job strategy is rebuilt and that company's prep is marked stale.
  - Given a stale mark, when I run the prep check for that company, then the first gap is `company:changed`; answering it yes clears the mark.
- **Metric**: `promo_coverage` in `careerbrew review` (always graded against the current ladder). **Target**: >= 0.8.
- **Skill**: `skills/careerbrew-weekly-review`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-31-changes.sh` | `changes.pivot` |

## US-32: Keep company and hiring information fresh, including AI policies

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a candidate, **I want** every company fact to carry the date it was checked, a way to re-check it with a browser, and each company's rules on AI tools in interviews, **so that** I prepare with current information.
- **Acceptance criteria**:
  - Given a public page, when I run `careerbrew research page <url>`, then its text hash and fetch time are stored and a later change is flagged.
  - Given sources older than the limit, when I run `careerbrew research stale`, then they are listed, pipeline companies first; the gate fails on any source without a checked date.
  - Given a company guide, when I run `careerbrew interview <company> --show`, then its AI policy (allowed, partial, no) shows with the date and source.
- **Metric**: stale sources in `scripts/check-sources.py` output. **Target**: 0.
- **Skill**: `skills/careerbrew-browser-research`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-32-research.sh` | `research.page` |

## US-33: See what interview loops share

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** the patterns shared across all researched companies (steps, order, AI rules), **so that** I prepare for what nearly every loop has before company specifics.
- **Acceptance criteria**:
  - Given the sourced companies, when I run `careerbrew interview-patterns`, then step frequency, the typical order, common transitions, AI-policy counts, and shared AI patterns appear.
  - Given an unknown company, when I ask for its loop, then the default loop is built from those patterns.
- **Metric**: `loops_sourced` in `careerbrew review`. **Target**: 100%.
- **Skill**: `skills/careerbrew-interview-coach`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-33-patterns.sh` | `cli.run` |

## US-34: Every skill starts from best practices and improves itself

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who works through an agent, **I want** each careerbrew skill to load the house rules first and to record a lesson at the end, **so that** every action follows the same rules and the skills get better with use.
- **Acceptance criteria**:
  - Given any skill, when `evals/run.sh` runs, then a skill that does not load `best-practices` before its first step, or does not end with an Iterate section and a LESSONS.md, fails.
  - Given the browser-research skill, when it runs, then it also loads the playwright-best-practices and agent-browser skills and records site lessons.
- **Metric**: skills failing the best-practices or Iterate checks in evals. **Target**: 0.
- **Skill**: `skills/careerbrew-best-practices`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-34-skill-practice.sh` | `check.step` |

## US-66: Grade the assistant's answers with a local judge I have checked against my own labels

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** the owner who changes skills and prompts, **I want** a local model to answer and grade every open-quality eval case, and to know how often that judge agrees with me, **so that** I trust its verdicts only after it matches my own judgment, and no eval data leaves my machine.
- **Acceptance criteria**:
  - Given the `input:`/`expect:` cases and the `local` route in `config/models.yaml`, when I run `EVALS_LLM=1 evals/run.sh`, then every case is answered with its skill as the prompt and graded one criterion per call (PASS, FAIL, or UNKNOWN with evidence), and a rerun with unchanged inputs calls no model.
  - Given fewer than 20 labels, or under 85% agreement, when a case is judged a failure, then it is listed as `[judge not trusted yet]` and the run still passes; once the judge agrees on >= 85% of >= 20 labels, a judged regression failure fails the run.
  - Given a case, when I run `evals/run.sh show` and then `evals/run.sh label <case> pass|fail`, then `evals/labels.jsonl` keeps my label with the answer I read and hashes of the answer and criteria, and `evals/run.sh agreement` reports agree/total, each disagreement, and KEEP, CALIBRATE, PIVOT, or COLLECT.
  - Given an edited labeled answer, an unknown case, or a judge that does not answer (no server, or the model is not installed), when the evals run, then they fail and name the fix (`ollama serve`, `ollama pull <model>`, or `evals/run.sh show <case>`).
  - Given a cache or label path inside the data root, then evals refuse it before reading or writing and name the external-path repair; synthetic CLI cases restore the caller environment before labels are checked (#423). Case and skill sources must stay in their repo folders and outside the data root.
- **Metric**: judge agreement with the owner's labels (`evals/run.sh agreement`). **Target**: >= 85% on >= 20 labels; pivot to code graders below 70%.
- **Skill**: `skills/careerbrew-best-practices`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-66-llm-evals.sh` | none (a dev tool: it writes no app events) |

# Web app

## US-35: Use careerbrew as a web app with several views

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who plans ahead and switches between tasks, **I want** a local web app with Today, Week, Job hunt, Interviews, Work, Review, and Setup views, **so that** I see everything careerbrew knows without remembering commands.
- **Acceptance criteria**:
  - Given `careerbrew serve`, when I open the printed URL, then the app loads with my token and every view opens from the navigation with real data.
  - Given an interview company link, when I open it, then its loop, AI policy, tips, traps, and sources show.
  - Given a view that is blocked on this machine, when I open it, then a clear message explains why.
  - Given the sidebar, when I open the app, then it shows three groups: Plan (Dashboard, Teach me, Today, Goals, Week, Work), Job search, and System (Review, Actions, Setup), and Actions says under its title that it runs any careerbrew command from the browser.
  - Given a screen 375, 768, or 1280 px wide, when I open each view, then nothing scrolls sideways: not the page and not any part of it (tables stack into one block per row, long text wraps, the top bar wraps).
  - Given any view, when it shows two or more cards, then one spacing token (`--space-4`, 16 px) separates every stacked card, grid row, and column, so no two cards touch (issue #142; test: `web/e2e/blocks.spec.ts`, run by `tests/e2e/us-58-action-feedback.sh`).
- **Metric**: browser console errors during the e2e walk through all views. **Target**: 0.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: all

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-35-web-app.sh` | `web.api` |

## US-36: Plan ahead in the web app

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an employee, **I want** a Week view built from my calendar and ranked tasks, where I can add tasks, **so that** I can plan the coming days, not only today.
- **Acceptance criteria**:
  - Given calendars and tasks, when I open Week, then each day shows its meetings, free minutes, and scheduled tasks, for 5, 7, or 14 days.
  - Given a new task added in Week, when the view refreshes, then the task appears in a free slot, and future days are planned without writing plan files.
  - Given a task bigger than the focus time of any one day, when I open Week, then it is listed as too big for one day with its hours and the most focus time a day has (split it), and "No room" names only the tasks that wait for room (tests: `test_plan_week_says_a_task_is_too_big_for_one_day_not_no_room`, `web/e2e/app.spec.ts` "Week plans ahead").
- **Metric**: `plan_done_rate` in `careerbrew review`. **Target**: >= 70%.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: Week

| Case | Test | Log event |
|---|---|---|
| happy | `tests/e2e/us-36-plan-ahead.sh` | `day.plan` |

## US-37: The web app's API stays local and private

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user with private and business data, **I want** the web app's API to answer only my own browser on this machine, **so that** no other site or device can read my data.
- **Acceptance criteria**:
  - Given a request without the token (it lasts across restarts, US-71), when it reaches the API, then it gets 401; a foreign Host header gets 403.
  - Given a corporate-role machine, when the app asks for job-hunt views, then the API answers 403 and the app explains it.
  - Given a corporate-role machine, when the app opens, then the sidebar shows no blocked view and no group whose every view is blocked (Job search), not even while the role loads, and the app never calls their endpoints.
  - Given any response, when the browser receives it, then it is `Cache-Control: no-store` and cannot be framed by another site.
  - Given evidence, the inbox, or a day log written by any process, when the app polls `/api/changes`, then the version moves and the open Today, Week, and Dashboard views refetch without a reload (polling pauses while the tab is hidden).
  - Given `careerbrew open`, when I run it, then my default browser opens the running app with its token, and the token never prints to the terminal.
  - Given a careerbrew server on the web app's port that careerbrew does not track (a crash, a test run, or another data folder left it), when I run `careerbrew open` (or `careerbrew`, or click a notification), then it stops that server, starts a tracked one, and opens a link that works (never "missing or wrong token"); given two starts at once, then one server runs and both get its link; given a program that is not a careerbrew server on the port, then it is never stopped and the error names its pid and the fix (tests: `tests/unit/test_issue_118.py`).
- **Metric**: API guard failures in `tests/unit/test_server.py`. **Target**: 0.
- **Skill**: `skills/careerbrew-privacy-guard`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-37-web-api.sh` | `web.api` |
| untracked server on the port | `tests/e2e/us-37-web-api.sh` | `web.untracked` |


## US-38: Merge my Notion career workspace into careerbrew

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** someone who kept career notes in Notion, **I want** my jobs board, flashcards, skills, STAR stories, job search plan, guides, and past-job notes inside careerbrew, **so that** one app holds everything and plans with my real history.
- **Acceptance criteria**:
  - Given a Notion bundle with a manifest, when I run `careerbrew import notion`, then every kind lands in its feature (Library, Jobs board, application log, practice deck, profile skills, story bank, work evidence) and `--check` prints N/N.
  - Given the same bundle again, when I re-import, then nothing is duplicated and my practice history is kept; given a manifest whose counts do not match the files, when I import, then it refuses and names the kind.
  - Given a corporate-role machine, when I import, then it refuses: interview prep stays personal.
- **Metric**: items present / items in the bundle (`careerbrew import notion --check`). **Target**: 100%.
- **Skill**: `skills/careerbrew-notion-import`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-38-notion-import.sh` | `notion.import` |

## US-39: Practise interview knowledge with flashcards

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** flashcards that make me recall before I see the answer and come back just before I forget, **so that** I keep interview knowledge with the least time.
- **Acceptance criteria**:
  - Given due and new cards, when I open Practice or run `careerbrew cards review`, then due cards come first, new cards are mixed by topic and capped per day, and the answer stays hidden until I ask.
  - Given my grade, when I press 1–4, then FSRS schedules the next review, and a card failed 8 times is suspended as a leech until I unsuspend it.
  - Given reviews over 30 days, when I open stats, then I see due, new, retention, streak, and per-topic counts.
- **Metric**: `retention` in `careerbrew review`. **Target**: >= 0.8.
- **Skill**: `skills/careerbrew-interview-drill`
- **Web views**: Practice

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-39-flashcards.sh` | `cards.review` |

## US-40: Get a fresh corporate machine ready and build its company plugin

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** an employee on a new corporate laptop, **I want** one runbook and one skill that take me from no code to full readiness, **so that** an agent can finish the setup and I only type tokens.
- **Acceptance criteria**:
  - Given a git bundle, when I follow `docs/corporate-machine.md`, then the clone blocks pushes and the install sets the corporate role.
  - Given a fresh install, when I run `careerbrew machine ready`, then each failed check is a blocker on its run page and names its fix.
  - Given a filled plugin (real ladder, terms, target level, policy), when I run `plugin refresh` and `machine ready`, then its summary line says `corporate readiness: N/N`, never shows the company name, and job-hunt commands stay blocked.
- **Metric**: readiness checks passed on the corporate machine (`careerbrew machine ready`). **Target**: N/N.
- **Skill**: `skills/careerbrew-corporate-plugin`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-40-corporate-rehearsal.sh` | `plugin.add` |

## US-41: See the most efficient work action for every horizon, with why

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer at my current job, **I want** one dashboard that shows the best thing to do right now, today, this week, this month, this quarter, this year, and at this company, **so that** I spend time where it pays most and can check the reason.
- **Acceptance criteria**:
  - Given ranked tasks, a calendar, and a ladder, when I open the Dashboard (or run `careerbrew work focus`), then each of the 7 horizons shows its top action and a why with the numbers behind the rank.
  - Given no tasks, when I open it, then empty horizons name the action that creates their data, and nothing is invented.
  - Given ranked tasks with hours, when I open the Dashboard, then a Tomorrow card lists the next workday's proposed tasks with time, title, and project, and links to Today's plan.
  - Given tasks that today's plan holds, when I open the Dashboard, then Tomorrow does not repeat them: it is Week's next workday planned from now, so late in the day the unfinished tasks move to it (tests: `test_tomorrow_does_not_repeat_today_and_matches_the_week_plan`, `test_tomorrow_takes_what_today_has_no_time_left_for`, `web/e2e/app.spec.ts` "Dashboard shows the best action").
  - Given a company plugin ladder, when I open it, then "This company" names the company ladder's highest-weight competencies.
- **Metric**: horizons with an action and a why on the seeded data (`npx playwright test -g Dashboard`). **Target**: 7/7.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Dashboard

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-41-focus-dashboard.sh` | `work.focus` |

## US-42: Connect a work tool by following only what careerbrew prints

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** someone setting up careerbrew at a job, **I want** each connect step to say where to get the token, what it looks like, and which fields it needs, then to test it, **so that** I never save a token that silently does nothing.
- **Acceptance criteria**:
  - Given `careerbrew connect jira`, when a field such as the email is missing, then it asks for it (or names the flag when not in a terminal).
  - Given a token that fails `/rest/api/3/myself`, when I connect, then it says FAIL with the fix and saves nothing.
  - Given a scoped Atlassian token, when the site URL refuses it, then careerbrew finds the cloud id and saves the `api.atlassian.com` gateway as the base URL.
  - Given connectors that are not working, when I open the Setup view, then each one lists the steps that finish it.
- **Metric**: connectors saved that fail their first collect. **Target**: 0.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: Setup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-42-connect.sh` | `cli.run` |

## US-43: Keep my data encrypted on disk

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user whose data folder holds 1:1 notes, salaries, and interview history, **I want** every data file encrypted on disk, **so that** a copied folder, a sync tool, or another program reading the disk sees only ciphertext.
- **Acceptance criteria**:
  - Given any careerbrew command that writes data, when I look at the file on disk, then it is ciphertext and careerbrew still reads it.
  - Given older plaintext files, when I run `careerbrew data encrypt`, then `careerbrew data status` shows every file encrypted.
  - Given the wrong key, when careerbrew reads a file, then it stops with an error that names the key, never garbage.
  - Given an encrypted data file, when I run `careerbrew data show <file>`, then it prints the file decrypted (sensitive terms redacted) and no file changes; given an agent, when it runs `data show <file> --keys`, then it gets key names and value types only, and the text of any file outside the top of the data root and `jobs/`, `tutor/`, `prep/`, `linkedin/` (in any letter case) is refused to a pipe (exit 3) unless the plugin policy allows agents to read business data; for such a file, `--keys` never prints a name that keys a map, such as a project name (issue #103). A root-relative path and the same path with one leading `private/` behave alike, including business refusal and structure-only output; path traversal and symlinks outside the data root remain refused (#366).
  - Given a profile and owner-supplied name or employer, when the owner uses `data set profile.yaml`, then the selected fields change without an editor, the result hides their values, and the file remains encrypted when encryption is on (#412).
- **Metric**: plaintext files under the data root outside `logs/` and `out/` (`careerbrew data status`). **Target**: 0.
- **Skill**: `skills/careerbrew-privacy-guard`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-43-encryption-at-rest.sh` | `data.encrypt` |
| read-only show and `--keys` | `tests/e2e/us-43-data-show.sh` | `data.show` |

## US-44: Learn from a public study deck that keeps improving

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate (and anyone else preparing), **I want** every flashcard to come with a detailed, easy explanation in a public study deck, **so that** I understand the answer instead of memorising it, and others can learn from it too.
- **Acceptance criteria**:
  - Given my private cards, when I run `careerbrew cards explain-queue`, then it lists the questions no public card explains yet, and behavioural answers never appear in it.
  - Given generated cards, when I run `careerbrew cards publish <file>`, then cards that meet the bar are merged into `knowledge/cards/`, personal questions get a private, encrypted explanation instead of a public card, and a card below the bar or with personal data is refused.
  - Given a better prompt or a card learners keep failing, when I run `explain-queue`, then those cards come back for a rewrite.
  - Given the public deck, when I open Library or reveal a card in Practice, then I can read its explanation and report a problem, which sends the card back for a rewrite.
- **Metric**: share of the owner's non-personal cards with a public explanation (`careerbrew cards explain-queue` count of new cards). **Target**: 100%.
- **Skill**: `skills/careerbrew-card-explainer`
- **Web views**: Library

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-44-public-study-deck.sh` | `cards.publish` |

# Teacher mode

## US-45: Learn one step at a time in teacher mode

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a learner with little time, **I want** the app to show me one step at a time toward my upcoming goal (a lesson to confirm, an exercise, cards to review or validate, a resume to send, an agent action to approve), **so that** I never have to decide what to do next.
- **Acceptance criteria**:
  - Given an upcoming goal, when I open Teach me, then I see one step with the reason it is next, and finishing it shows the teacher's short message and the next step.
  - Given no upcoming goal, when I open Teach me, then I see only a "Set a goal first" step, never a lesson.
  - Given an agent's approval request, when I open teacher mode, then it comes first, and the agent can act only after I approve it.
  - Given my written feedback or signals the teacher sees (slow steps, stuck exercises, repeated mistakes), when I run `careerbrew teach improvements`, then each becomes a proposal with a fix.
- **Metric**: `teacher_steps_done` per day (event `teach.step`). **Target**: >= 8.
- **Skill**: `skills/careerbrew-teacher`
- **Web views**: Teach me

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-45-teacher.sh` | `teach.feedback` |

## US-46: Practise coding exercises in the right order with time and quality measured

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate, **I want** LeetCode-style exercises in the classic pattern order, run locally and graded on correctness, complexity, and readability, with my solve time kept, **so that** I build skill in the order interviews test it.
- **Acceptance criteria**:
  - Given the curriculum, when I list it, then 150 exercises run from arrays and hashing to bit manipulation.
  - Given a submission, when it runs, then I see tests passed, whether it meets the expected complexity, readability findings, and a score; time is stored.
  - Given a correct solution on a busy machine, when the complexity check runs, then it still passes: each run is timed in CPU time (time spent waiting for a CPU does not count as growth, and does not cut the run off as too slow), growth is fitted over 3 sizes from the fastest of 5 runs each, and only a wrong class (for example quadratic for linear, or cubic for quadratic) fails.
  - Given problems I solved elsewhere, when I sync a public LeetCode profile or list slugs, then those exercises are skipped.
- **Metric**: `exercise_pass_rate` (first-try passes over attempts, event `exercise.submit`). **Target**: rising week over week.
- **Skill**: `skills/careerbrew-teacher`
- **Web views**: Teach me

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-46-exercises.sh` | `exercise.submit` |

## US-47: Set my own goals and have the schedule follow them

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** the owner, **I want** to set my long-term goals and my upcoming goal myself, and have today's schedule change the moment I change the upcoming goal, **so that** my time always serves the next thing that matters.
- **Acceptance criteria**:
  - Given an agent without a terminal, when it tries to set a goal, then it is refused.
  - Given a new upcoming goal, when I save it, then today's plan starts with its study blocks in the same response.
  - Given a different goal, when I save it, then the old goal's blocks are gone.
  - Given several upcoming goals, when I change any of them, then the nearest goal still ahead drives the schedule, and the next takes over when its date passes.
- **Metric**: `goal_replan_seconds`. **Target**: < 1 s.
- **Skill**: `skills/careerbrew-teacher`
- **Web views**: Goals

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-47-goals.sh` | `goals.update` |

## US-48: Choose the jobs I want

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** the owner, **I want** careerbrew to suggest jobs but put a job in my plan only when I choose it and its tier, **so that** my season is spent on the jobs I want.
- **Acceptance criteria**:
  - Given scanned postings, when the strategy runs, then tiers are empty and suggestions are listed until I choose.
  - Given my choice, when the strategy runs, then the job is in the tier I chose, and I can take it back.
- **Metric**: chosen jobs with a sourced interview loop (`loops_sourced`). **Target**: >= 1.0.
- **Skill**: `skills/careerbrew-job-hunt`
- **Web views**: Job hunt

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-48-job-choice.sh` | `jobs.choose` |

## US-49: Never make the same mistake twice

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a learner, **I want** every mistake named, explained, and turned into a prevention card, and a repeat called out, **so that** mistakes stop repeating.
- **Acceptance criteria**:
  - Given a failed attempt, when it is graded, then the mistake has a kind and a prevention tip, and a card joins the `mistakes` deck.
  - Given the same mistake again in the same pattern, when it is graded, then it is marked as a repeat and becomes the teacher's next step.
- **Metric**: `mistake_repeat_rate`. **Target**: < 15%.
- **Skill**: `skills/careerbrew-teacher`
- **Web views**: Teach me

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-49-mistakes.sh` | `exercise.submit` |

## US-50: Know which project sources are stale before I work from them

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer on a long project whose state lives in a PRD, a tracker epic, repos, chats, and my agent's notes, **I want** each source to carry when I last read it, **so that** neither I nor my agent works from weeks-old state without knowing.
- **Acceptance criteria**:
  - Given a project, when I add sources and run `work project check <name> [<ref>]`, then `checked_at` (UTC) is stamped on one source or all of them, with an optional one-line status.
  - Given `--stale-after 7d`, when a source was never checked or is older than 7 days, then `show`, `list`, `checkup`, `day next`, and `work focus` (and the Dashboard) list it first.
  - Given an unknown project or a duplicate source, when I run a command, then it refuses and names the fix.
  - Given the record, when it is on disk, then it is encrypted at rest and holds pointers and dates only.
  - Given `--memory` naming an agent memory index (`MEMORY.md`), when I add the project, then it refuses and suggests `<project>-research-log.md`; a folder gets that log; checkup flags old records that point at an index.
  - Given a company plugin with `agent_reads`, when checkup runs, then stale sources of those kinds are labeled "agent reads this" instead of failing; `agent_reads: [url]` covers `url` sources, and careerbrew never fetches a source of any kind the agent reads (test: `test_issue_101.py`). `work project show` gives such a stale source the agent step (read it with the agent's tools, then `careerbrew work project check <name> <ref> --status "<one line>"`), never a connector to set up (test: `test_issue_184.py`).
  - Given a source careerbrew cannot read (a link that opens a sign-in page: Google Docs, Sheets, or Drive, Outlook or Microsoft 365, Slack), or one I mark with `work project reference <name> <ref>`, when its limit passes, then it is a reference: never read, never stale, so never in Housekeeping, `checkup`, `day next`, or `work focus`; `show` says why, and `--off` tracks it again (test: `test_issue_70.py`).
  - Given a `url` source on a host outside the plugin policy `allowed_hosts` (for example a company web app behind single sign-on) and no `agent_reads: [url]`, when checkup runs, then it is a reference read by you, not a failure; `show` names the host and the policy (test: `test_issue_101.py`).
- **Metric**: sources older than their limit when an agent starts work (`careerbrew checkup` "project sources fresh"). **Target**: 0.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Dashboard

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-50-work-projects.sh` | `work.project` |

## US-51: Hand anything to a project, and let checkup research what changed

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer on a long project, **I want** to hand careerbrew a link, a file, a task, or a channel and have it filed under the right project with a dated summary, and **I want** checkup to read what changed in stale sources, **so that** my project notes stay current without me copying anything.
- **Acceptance criteria**:
  - Given a link, file, task id, or `#channel`, when I run `work project absorb [<name>] <it>`, then careerbrew detects the kind, reads it through the matching connector, adds it as a source with `checked_at`, and appends key lines to the project memory.
  - Given a GitHub PR or issue link, absorb keeps one `url` source and writes that item's title and state, never a repository-wide source. Given a Jira browse link, repository tree link, or Slack channel id, source add normalizes the ref, refuses canonical duplicates, and refresh/sync repair old raw refs while keeping explicit reference choices (issues #338, #309).
  - Given no project name, when one project matches best (same tracker key prefix, repo owner, host, or channel), then it is picked; on a tie it refuses and names the candidates.
  - Given stale sources, when I run `careerbrew checkup`, then it reads what changed since `checked_at`, writes dated notes, moves `checked_at`, and proposes related sources (child issues); a source it cannot read stays stale with the reason, unless it is a reference (US-50), which it never reads.
  - Given business reading is off (no terminal and no plugin `agent_may_read_business`), when I absorb, then only the ref and date are stored and careerbrew says so.
- **Metric**: stale sources left after a checkup that could have been read. **Target**: 0.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-51-project-research.sh` | `work.project` |

## US-52: One command keeps my work day logged, planned, and explained

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer, **I want** one command (`careerbrew`, or `/careerbrew` in my agent) to start a background watcher that keeps a daily log of what I did and what I plan for tomorrow, proposes goal and task updates from new context, and asks me questions when it lacks context, **so that** I always see the latest state, have a history, and can send my PM an update in seconds.
- **Acceptance criteria**:
  - Given no command, when I run `careerbrew`, then it starts the watcher and the web app with sane defaults, refreshes, and prints today's counts, proposals, and questions.
  - Given a watcher cycle that sees new work, when it ends, then today's log, proposals, and questions are rebuilt; every day's log is kept.
  - Given an overdue task, a task due by the next workday, and a stale project source, when the log builds, then they appear under Blockers, Risks, and Housekeeping; the PM update lists Blockers only (test: `test_issue_60.py`).
  - Given a proposal, when I accept it, then it applies; goal proposals apply only when I approve them in the web app.
  - Given a proposal, when I see it (Today's own "careerbrew suggests" section or `careerbrew day inbox`), then it says what it is, why careerbrew suggests it (the evidence), and exactly what Accept and Dismiss change (tests: `test_every_suggestion_says_what_why_and_what_accept_and_dismiss_change`, `web/e2e/app.spec.ts`).
  - Given a task an agent added (with `--why`), when the plan is built, then it is not planned until I accept it, and the log shows "added by agent: <why>".
  - Given a task an agent just added, when I open the inbox or Today, then its suggestion is there at once, with no watcher cycle and no connector call (test: `test_agent_task_shows_in_the_inbox_at_once`).
  - Given a task an agent just proposed, when the CLI replies, then it names the stored inbox id and exact owner accept command, without accepting the task or printing its title or reason (tests: `test_issue_371.py`, US-74).
  - Given an installed plugin, when it runs `careerbrew day suggest` with its own what, why, and on-accept text, then the suggestion shows at once with that text plus what careerbrew really changes; Accept adds its `--task` (if any), the same ref never shows twice, a decision sticks, the plugin reads decisions with `day suggestions`, and an unknown plugin is refused (tests: `test_issue_71.py`, `tests/e2e/us-52-daily-log.sh`).
  - Given a change that arrives as a PR and its commits, when the log is built, then it is listed once (the PR line).
  - Given a question, when I answer it, then the answer is applied (impact note, project link, competencies, notes path); a bad answer is refused with the fix. A question about a task's competency lists the ladder's ids to answer with (test: `test_a_competency_question_lists_the_ids_to_answer_with`).
  - Given tasks that the rest of today's plan holds, when the log builds, then the plan for the next workday does not repeat them, and the PM update still names them as next (tests: `test_plan_for_tomorrow_leaves_out_what_today_still_holds`, `test_the_pm_update_names_later_today_work_as_next`).
  - Given the Today view, when I open it, then I see done today, the plan for tomorrow, suggestions, questions, and a PM draft with Copy and Send (a mail draft I send myself).
  - Given a Plan or Done row in Today, when I click it, then it opens its details: source link, project, why it was planned or how it was found, related tickets and PRs, and its history (tests: `test_plan_and_done_rows_carry_details`, `web/e2e/app.spec.ts`).
  - Given any setting, when I run `careerbrew config set <key> <value>`, then it is validated and stored; `careerbrew config` shows every default and its source.
- **Metric**: workdays with a kept log while the watcher runs. **Target**: every workday.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-52-daily-log.sh` | `day.log`, `day.inbox`, `day.inbox.suggest` |

## US-53: Run every action from the CLI, an agent, or the web app, and send the PM update to Slack

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a careerbrew user, **I want** every action to work from the terminal, from my agent, and from the web app, and my PM update to go to Slack by default, **so that** I never need a specific tool to get something done and my PM gets the update where they read it.
- **Acceptance criteria**:
  - Given any CLI command, when I open the web app's Actions view or run `careerbrew actions --json`, then it is listed with its arguments and runs from there; actions that need a terminal say why and give the command.
  - Given an action that changes or sends something hard to undo, when I run it from the web app, then it asks me to confirm first.
  - Given any CLI command, when the gate runs, then an e2e story runs it (`tests/unit/test_every_action_has_an_e2e.py`), so a new command cannot ship untested.
  - Given owner-supplied profile values, when I run `data.set` from Actions, then it updates the encrypted profile and hides the values in the result (#412).
  - Given `pm_slack` and a Slack connector with `chat:write`, when I press Send (or `day send --yes`), then the update is posted to that channel or person; without them, Send says exactly what to set. When a destination is set without a Slack connector, the web Send button is disabled and shows `careerbrew connect slack` (`web/e2e/inputs.spec.ts`). `pm_send_via email` opens a mail draft.
  - Given equivalent synthetic Claude and native feed replies, when the CLI ingests each, then facts deduplicate and no payload title prints.
- **Metric**: CLI commands reachable from all three paths (`test_action_parity.py`). **Target**: every command, or a written terminal-only reason.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Actions, Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-53-actions-and-slack.sh` | `cli.run`, `day.pm.send` |

## US-54: Opt in to share weekly aggregate numbers with my team

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user whose team lead wants to see what careerbrew gains people, **I want** to opt in to a weekly file of numbers only, **so that** my team can measure value without any of my content leaving the machine.
- **Acceptance criteria**:
  - Given the default settings, when a week passes, then no file is written.
  - Given `metrics_share: true`, when the watcher runs in a new week (or I run `careerbrew metrics weekly`), then `weekly-<YYYY-Www>.json` holds the tool, version, week, a random machine id, and numbers only; text never appears.
  - Given a company plugin with `policy.metrics_share: false`, when I opt in, then careerbrew refuses.
- **Metric**: canary strings found in any weekly file (`test_metrics_share.py`). **Target**: 0.
- **Skill**: `skills/careerbrew-weekly-review`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-54-metrics-share.sh` | `metrics.weekly` |

## US-55: A coworker sets up careerbrew with our company plugin in one command

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a coworker of the plugin's owner, **I want** one command on my own corporate machine that uses the shared company plugin and generates my own settings, **so that** I get the company rules without the owner's email, logins, or repos.
- **Acceptance criteria**:
  - Given the company plugin, when I run `careerbrew machine new-user --plugin <it>`, then my role, profile, repos (as evidence sources), and connectors are set from my `git config`, and it ends at `corporate readiness: N/N`.
  - Given a plugin that holds one person's email or login, when anyone runs `machine ready`, then it fails and names the fix: `careerbrew machine split-plugin-evidence`.
  - Given an older plugin whose evidence file lists the owner's hand-picked repos (with author emails) and tuned connectors (a gateway `base_url`, a custom `jql`), when the owner runs `careerbrew machine split-plugin-evidence` (`--dry-run` first to see the counts), then those entries move to their private `work/sources.yaml` and `work/connectors.yaml` as they are, shared entries stay, the plugin is reinstalled from its folder (`plugin verify` stays ok), and it prints the evidence counts before and after; a rerun moves nothing.
  - Given a plugin installed from a git URL (no folder on this machine to edit), when the owner runs `split-plugin-evidence`, then nothing moves and it names the fix (`plugin add <its folder>`).
  - Given no plugin, when I run `new-user`, then it says to install the plugin first.
- **Metric**: owner values found in a coworker's generated settings (`us-55-second-user.sh`), and per-person entries left in a migrated plugin (`us-55-split-plugin-evidence.sh`). **Target**: 0 and 0, with the same number of evidence sources before and after.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-55-second-user.sh` | `plugin.add` |
| migrate an older plugin, and error | `tests/e2e/us-55-split-plugin-evidence.sh` | `machine.split_evidence` |

## US-56: Explain a tracked project in layers, with a citation on every claim

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer joining or returning to a project, **I want** a five-minute explanation built from the project's own sources, **so that** I can trust each claim and know where to read next.
- **Acceptance criteria**:
  - Given a project record, when I run `work project explain <name> [--short]`, then an outline with the fixed headings and the project's sources as the reading list is written to `private/out/`.
  - Given a filled doc, when I run `work project explain <name> --check <file>`, then it passes only with the headings in order, an inline citation on every bullet and table row, typed and dated sources, no leads-only claims, a Deploys as column, a decided-or-proposed Why table, 3 flat one-minute bullets, and acronyms defined at first use.
  - Given a project I ran, when I run `work project explain <name> --deep [--check <file>]`, then the outline adds a "How it runs" section after the codemap, and the checker needs a cited bullet with the command in backticks for each step.
  - Given a project record, when I run `work project prd <name> [--slot N]` and then `--check <file>`, then the PRD passes only with Problem, Requirements, Decisions, an optional Pushback, Open questions, and Sources in order, a plan slot and every estimate in dev weeks, decided-or-proposed decisions with Who and When, a Pushback with the gap in dev weeks when the estimates add up to more than the slot, a Who on each open question, and a citation on every bullet and row.
  - Given an unknown project or an unfilled outline, when I run it, then it names the fix or the problems and exits 1.
- **Metric**: checker problems in a shipped doc or PRD (`us-56-project-explain.sh`). **Target**: 0.
- **Skill**: `skills/careerbrew-project-explainer` (cases: `evals/cases/023-project-explainer.yaml`)
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-56-project-explain.sh` | `work.explain`, `work.prd` |

## US-57: Practice interview answers by voice

- **Host contract**: dual-host-shared-memory
- **Mode**: job search
- **As** a candidate who will answer out loud in the interview, **I want** to hear the question, answer by speaking, and get my answer graded by the rubric, **so that** I practice the way the interview really works without my voice leaving this machine.
- **Acceptance criteria**:
  - Given a local speech-to-text engine (`stt_command`, or whisper.cpp with `stt_model`), when I run `drill speak <item> --audio <file>` or press Record answer in the Interviews view, then the answer is transcribed on this machine, graded against the rubric, and recorded as a drill attempt; the audio and the transcript are not stored.
  - Given no engine or no microphone, when I try to answer by voice, then the error names the setup fix and I can type the answer instead (`--text`, or the typed box), graded the same way.
  - Given the web app, when I press Read question aloud, then the browser speaks the question with the device's own voice.
- **Metric**: rubric criteria the grader finds in the synthetic spoken answer (`us-57-voice-practice.sh`). **Target**: 3 of 3.
- **Skill**: `skills/careerbrew-interview-drill`
- **Web views**: Interviews

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-57-voice-practice.sh` | `tutor.voice` |

## US-58: See that every action is running, and how to fix it when it fails

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who clicks a button and waits, **I want** every form, button, and inline answer in the web app to show that it is working and, when it fails, a clear error that names the fix, **so that** I never wonder whether a click did anything.
- **Acceptance criteria**:
  - Given any action in the web app (inbox answer, accept or dismiss, PM draft and send, tasks, goals, job choices, card grades, voice grading, feedback, reports, settings, the watcher, the Actions view), when it runs, then a "working" note shows and its button is disabled until it ends.
  - Given the action fails, when the server answers with an error, then an alert shows the message and a "Fix:" step (start the server, reopen the link, check the input, or check the server log), and nothing I typed is lost.
  - Given a view whose data fails to load, when I open it, then the same alert with its fix shows instead of a blank page.
  - Given any view, when I open it, then within 100 ms every block that waits for data shows a visible "Loading <what>…" label, and once it takes 3 s or more, the seconds it has waited (issue #142).
  - Given a block whose data fails to load, when the server answers with an error, then that block alone shows the error with its fix command and a Retry button, the other blocks stay, and Retry loads it again; a block that already shows data keeps it, marked "not refreshed" (test: `web/e2e/blocks.spec.ts` "one block waits and fails alone" on Dashboard and Today, `web/src/blocks.test.tsx`).
  - Given the Dashboard or Today, when I open it, then its blocks are fetched at once from their own endpoints (each Dashboard horizon, Tomorrow, the stale sources; Today's plan, open asks, day log, and inbox), and each one shows as soon as its data arrives (test: `web/e2e/blocks.spec.ts` "one block waits and fails alone": the other blocks show their data while one read is held).
- **Metric**: views whose action shows both the loading and the error state in the Playwright suite (`web/e2e/feedback.spec.ts`), and views whose every block shows a loading label within 100 ms, an error with Retry, and gaps between cards (`web/e2e/blocks.spec.ts`). **Target**: 12 of 12 (Dashboard, Today, Week, Goals, Teach me, Job hunt, Interviews, Practice, Library, Actions, Setup, Reports), and 16 of 16 views.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: Today, Dashboard, Week

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-58-action-feedback.sh` | `web.api` |
| every block of every view (issue #142) | `web/e2e/blocks.spec.ts` (run by `tests/e2e/us-58-action-feedback.sh`), `web/src/blocks.test.tsx` | `web.api` |

## US-59: Send my manager a five-line update ranked by my project priorities

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer who ships many PRs and commits a day, **I want** the PM update to be plain lines, one per project in the order I rank my projects, each naming its results, and the same ranking to drive my plan and Next step, **so that** my manager reads it in seconds and my day follows what matters most.
- **Acceptance criteria**:
  - Given a day with 40+ PRs, commits, tickets, and meetings, when the log builds, then the PM draft has one line per project, each starts with its project, names its done work by plain titles (up to 8, then "and N more"), and keeps the counts ("15 PRs in 3 repos (agentbrew, dotfiles, minsky)"); no project is folded into an "Other" line any more (issue #214; tests: `test_issue_68.py`, `test_every_project_gets_its_own_line_with_no_cap`, `test_a_project_line_names_each_result_not_only_a_count`).
  - Given the draft or my saved edit, when a line has a time estimate (`ETA`, `about 2 days left`, `~4h`), a percent or "3x" claim with no link and no source title that names it, or a `next:` item done today, then the PM view and `day send` list that line with its fix, for me only, and the save still goes through; the generated draft passes every check, and each line's "why it matters" note starts "Not sure:" when no result checks it (issue #226; tests: `test_issue_226.py`, `web/e2e/pm-copy.spec.ts`).
  - Given a meeting I organized that is not a series occurrence or a standup, my own channel post that announces a launch, and my reply in a group thread that did not ask me, when the draft builds, then the launch leads its project's line with its link, then "led <meeting>", and the check-in is named with its link; a series I organize and other people's meetings are counted; small CI, lint, or config changes are named last and "review PRs" is the last next item (issue #239; tests: `test_issue_239.py`, `us-59-pm-update.sh`).
  - Given the draft, when I read it, then it has no task ids, ticket keys, or PR numbers, no commit prefixes, and only numbers that can be checked: counts of rows with a source, due dates, and numbers in source titles.
  - Given an edit or a `day send --text` with a PR number (`#12`) or a ticket key outside a link, when I save or send it, then it is refused and the error names each such line (an agent sees only the count); a markdown link, a Slack `<url|label>` link, or a plain URL passes, and UTF-8 or C# stays a word (issue #227; tests: `test_issue_227.py`, `us-59-pm-update.sh`, `web/e2e/pm-copy.spec.ts`).
  - Given a title with a word shaped like a ticket key (`fix: decode UTF-8 file names`, `Parse ISO-8601 dates`), when the draft builds, then the word stays and gets no line of its own: ticket keys are keys of a ticket project that a `project_priority` pattern names (an upper-case `PROJ-*` or `PROJ-12`, and a bare upper-case `PROJ` next to one) or that a tracker link names (`.../browse/OPS-3`); with no named ticket project (a repo glob such as `api-*`, a project `web-2`, or a lone `SRE` names none), any `ABC-123` is a key except a standard such as UTF-8 or ISO-8601 (tests: `test_utf_8_and_iso_8601_stay_in_the_pm_update`, `test_ticket_keys_come_from_named_ticket_projects_or_tracker_links`, `test_a_repo_glob_or_a_lone_bare_name_keeps_the_generic_strip`, `test_a_bare_upper_case_name_is_a_ticket_project_next_to_a_key_shaped_one`).
  - Given `careerbrew work priority set <project> --rank <n> --match <glob>` (or `config set project_priority "name=glob|glob, name"`), when I change the ranking, then the PM update, the plan, and Next step follow it; `work priority show` lists each project's patterns and what they match.
  - Given two equal tasks in different projects, when the planner ranks them, then the higher project's task comes first and its why says "project <name> (priority r of n)".
  - Given a bad name, a duplicate project, or an empty pattern, when I set the ranking, then it is refused and the error names the fix.
  - Given the web app, when I click Copy update at the top of Today or Dashboard, then one click copies the current PM draft and shows a toast. With `pm_send_via slack`, supported links keep their labels in the rich clipboard payload. The plain fallback keeps one delimited URL per link. With `email`, the subject line comes first. Copy under the draft uses the same payload (test: `web/e2e/pm-copy.spec.ts`).
  - Given an update with `•` and `◦` bullets, indentation, and markdown links, when I copy it, then the bullets and indentation stay and each link becomes its label and its plain URL (`the doc (https://...)`), so Slack links it; an edit I have not saved yet is what gets copied.
  - Given a browser that refuses the clipboard, when I click Copy update, then the text shows selected in a box, so Cmd+C (Ctrl+C) copies it.
  - Given the update, when I look next to Copy update, then it says whether it is the generated draft or my edit, and when it last changed: my last saved edit, or the last change of the kept day log ("updated 15:45") (test: `test_issue_145.py`).
  - Given no-action status text or source-proven draft work presented as complete, `day send` names `no-action` or `draft-state` checks, including explicit text overrides; restricted output hides the line. Generated drafts drop no-action checks and preserve `In draft`. Each changed shown-to-saved line yields a private candidate rule, while automatic style lessons retain their repeat threshold (issue #336, `tests/unit/test_issue_336.py`).
- **Metric**: lines in the PM draft on the 46-row fixture day (`test_issue_68.py`, `us-59-pm-update.sh`), and clicks from opening Today to a Slack-ready update on the clipboard (`web/e2e/pm-copy.spec.ts`). **Target**: one line per project, with 0 ids; 1 click, with no subject line for Slack.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-59-pm-update.sh` | `work.priority`, `config.set` |
| web app: Copy update at the top of Today and Dashboard | `web/e2e/pm-copy.spec.ts` (run by `tests/e2e/us-59-pm-update.sh`) | `web.api`, `config.set` |

## US-60: Import a team's goal list from a sheet, with parents and links between projects

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose team keeps a goal list or a quarter's cutline of dozens of related projects in a sheet, **I want** to import it in one command, with each project's parent and links, and see the tree in my project views, **so that** I do not re-type the list or keep the relations in a plugin outside core.
- **Acceptance criteria**:
  - Given a CSV or JSON file (a CSV download is the recommended way), or a Google Sheets link (only when the sheet's owner and your company policy allow it), with columns name, title, group, parent, links, and trackers, when I run `work project import <file|link>`, then each row adds or updates a project in one write, trackers become `tracker` sources, and unknown columns are named and ignored.
  - Given a row with an unknown parent or link, a parent loop, a duplicate name, or no name, when I import, then nothing is written and the error names each row and its fix; `--dry-run` checks every row and writes nothing.
  - Given a re-import, when a known project's cell is empty, then the stored value stays, and a source's `checked_at` is never reset.
  - Given a private sheet or another site, when I import the link, then it refuses and names the fix (download it as CSV, which is recommended; use a shared link only when the sheet's owner and your company policy allow it); careerbrew never signs in to Google, and the plugin host policy applies.
  - Given projects with a parent or links, when I run `work project show` or `list`, or open the Work view, then the group, parent, children, links, and linked-from projects show, and the Work view nests children under their parents; a title prints to an agent only when business reading is on.
  - Given `work project add --parent <p> --link <l>` or `work project set <name> --parent <p> --link <l> --unlink <l>`, when the parent or link names no project or makes a loop, then it is refused and the record is unchanged.
  - Given an imported tree with no notes paths, when the inbox asks where notes go, then it asks once per root project (2 for the fixture's 6 rows) and names how many projects the answer covers; a child with no notes path writes its notes to the nearest parent's, headed with its own name, and a child's own path wins (test: `test_import_memory_questions.py`).
- **Metric**: rows of the fixture goal list that land with their parent, links, and trackers from one command (`us-60-project-import.sh`). **Target**: 6 of 6, and 0 rows written from a sheet with one bad row.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-60-project-import.sh` | `work.project` |

## US-61: See every work tool's asks in one local feed, with personal events kept out

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose day is mostly Slack asks, mail, meetings, and pages, **I want** one local store that reads Slack, PagerDuty, and Outlook (and my plugins' events), triages them by rule, and keeps personal events out of my work outputs, **so that** I see what asks me without opening each tool, my handled pages count as evidence, and nothing private reaches my manager.
- **Acceptance criteria**:
  - Given `careerbrew connect slack | pagerduty | outlook`, when I run `work feed pull` (or the watcher runs, at most every 15 minutes), then each event is stored once by id: its original encrypted in `work/feed/originals.jsonl` for `feed_keep_days` days, its analysis (facts, triage, personal flag, deciding rule) apart in `work/feed/events.jsonl`, and the analysis holds no message text (test: `test_pull_keeps_originals_encrypted_and_the_analysis_apart`).
  - Given the built-in rules, when events arrive, then each gets act (a DM, a mention, a page assigned to me, important or flagged mail), fyi, noise (bots, no-reply senders), or mine (my own messages, sent mail, organized meetings, pages I acknowledged or resolved); an ask stays open until my later reply or acknowledge in the same thread; `work feed show` counts them and `--show --open` lists open asks to my terminal only.
  - Given my own rules in `work/feed-rules.yaml`, when I run `work feed retriage`, then my rules run before the built-in ones on every kept original; a bad rule is refused and names the fix (`work feed rules` shows the order and an example).
  - Given a private calendar entry (Outlook sensitivity private or personal, ICS `CLASS:PRIVATE`), a rule that marks an event personal, or `work feed mark <id> personal`, when the day log and the PM update build, then that event is not in them, not counted, and not in evidence; my mark wins and sticks on the next collect (tests: `test_personal_never_reaches_the_pm_update`, `test_marking_an_event_personal_drops_its_evidence_and_its_pm_line`).
  - Given a page I acknowledged or resolved, when evidence is collected, then it becomes ownership evidence ("Resolved incident: ...") and the PM update counts it as an incident.
  - Given `feed_tool_sources` maps an agent's MCP tools to feed sources, when I run `work feed transcript <session.jsonl>` at a cycle's end, then each event in a mapped tool reply is stored once (a large reply is read from its saved file inside the transcript's folder), a rerun stores nothing twice, the output has counts only, and a mapped source with nothing stored this cycle prints a `gap` line and exits 1 (tests: `test_issue_234.py`).
  - Given a live-feed cycle, when I run `work feed cycle [--require chat,jira,mail]` at its end, then it names each required source (the union of configured tool sources, required live sources, and additive `--require`) with missing or invalid completion coverage as `not stored` and exits 1, and a new cycle starts; an event stored before still counts as stored, and so does a raw search reply with no items (tests: `test_issue_254.py`).
  - Given a tool reply saved exactly as returned, an object with a `tickets` list or a `messages` list, when I run `work feed add <file> [--source jira]` or `work feed transcript`, then each item is one event (a ticket's id is key + updated + status; a message's is its id), an item with no id, time, or title is skipped and counted (tests: `test_issue_254.py`).
  - Given an installed plugin, when it runs `work feed add <file.jsonl> --plugin <name>`, then its events go into the same store with `origin: plugin:<name>`; one bad line writes nothing and names the line; an unknown plugin is refused; `plugin remove --purge-data` deletes its events and originals.
  - Given current declared tool sources and required live sources, direct batch completion checks their union; `--require` can only add sources. Quiet validated replies count, old batches and unknown sources cannot certify coverage, and own eligible feed actions reach their local-day results without a collector (#406, `test_issue_406.py`).
- **Metric**: personal events counted in the PM update on the fixture day (`us-61-work-feed.sh`). **Target**: 0, with 16 of 16 fixture events triaged as the rules say.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-61-work-feed.sh` | `work.feed.pull`, `work.feed.mark`, `work.feed.transcript`, `work.feed.cycle` |

## US-62: Hear about important new events, with what to do next, and act on each one

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose asks and pages arrive in many tools, **I want** opt-in notifications only for important new events, each saying what happened, why it matters, and the next step, that I can act on from the pop-up or the web app, **so that** I answer pages, DMs, review requests, and suggestions in time without being interrupted by noise.
- **Acceptance criteria**:
  - Given `notify_events` is false (the default), when the watcher runs, then nothing notifies and nothing is stored; given it is true, when a page on me (5), a DM or review request (4), a mention, important mail, or a suggestion (3) arrives, then only those at or above `notify_min_priority` become notifications (+1 for my rank-1 project), never a personal event, a bot, or an event older than a day (tests: `test_priority_by_kind_with_a_bonus_for_the_rank_1_project`, `test_threshold_keeps_lower_priorities_out`).
  - Given several important events at once, when careerbrew checks, then one pop-up counts them ("2 new for you: 1 page, 1 direct message") and names the most urgent with its next step; given the same events again, or a repeat in the same thread while it is unread, when it checks, then no second pop-up shows; events within `notify_batch_minutes` of the last pop-up wait for the next one.
  - Given `notify_quiet_hours` (default 19:00-08:00), when an event arrives in them, then no pop-up shows and one "While you were away" pop-up follows them; given `notify_daily_cap` pop-ups today, when another arrives, then it waits in the web app's Notifications view and never pops up later (tests use the pinned clock `CAREERBREW_NOW`).
  - Given alerter, when I click Accept or Dismiss on a suggestion's pop-up, then the suggestion is decided in the inbox; given I decided it elsewhere since the pop-up showed, when I click Accept or Dismiss (or run `notify act`), then nothing changes, it says so, and the click is not counted as acted on (test: `test_a_late_accept_or_dismiss_on_a_suggestion_decided_elsewhere_changes_nothing`); Snooze hides it for `notify_snooze_minutes`; a click on the text opens `#/notifications/<id>`. Given terminal-notifier, when I click, then `careerbrew notify open <id>` opens that notification through the same flow as `careerbrew open`. Given neither, when a pop-up shows, then it is the plain osascript notification. `scripts/doctor.sh` says which one this Mac uses.
  - Given any notification, when I open the web app's Notifications view, then it shows the title (local only), why, next, and priority, with Open, Accept or Dismiss (suggestions), and Snooze; a pop-up shows no title, name, project or plugin name, or token (it says "your rank-1 project" and "a plugin"; the web app names them) (test: `test_a_popup_never_names_the_rank_1_project_or_the_plugin`).
- **Metric**: pop-ups per burst of important events, and pop-ups for repeats, on the fixture day (`us-62-notifications.sh`). **Target**: 1 per burst and 0 for repeats, with 0 during quiet hours.
- **Skill**: `skills/careerbrew-day-watch`
- **Web views**: Notifications

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-62-notifications.sh` | `notify.check`, `notify.act` |
| web app list and actions | `web/e2e/notifications.spec.ts` (run by `tests/e2e/us-62-notifications.sh`) | `notify.act` |

## US-63: See my open asks on Today, and keep a personal one out

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose asks arrive in Slack, mail, and pages, **I want** Today to list the work feed's open asks with a link to each one in its tool and a Mark personal button, **so that** I answer them where I plan the day instead of checking four tools.
- **Acceptance criteria**:
  - Given open asks in the work feed (triage act, no later reply or acknowledge of mine in the same thread), when I open Today, then an "Open asks" section lists each one newest first (issue #275) with its source, its title linked to the event in its tool, and how long it has waited; the wait counts work hours only (`work_start` to `work_end` on `work_days`, local time), so nights and weekends do not count; an ask waiting over `feed_ask_overdue_hours` work hours (default 8) is marked "over 8 work hours"; answered and personal asks are not listed (test: `test_today_lists_open_asks_newest_first_with_link_and_wait`).
  - Given an ask with a qualifying reply or Done on the current local day, Today shows it once in Closed today. Undo removes a manual closure unless a qualifying reply still closes the ask. This count stays the same when I page through open asks (tests: `test_issue_377_closed_today.py`, `web/e2e/feed.spec.ts`).
  - Given more open asks than one answer holds (50), when I open Today, then the 50 newest are listed, the section says "Showing 50 of N open asks, newest first.", and Show all N loads the rest (one answer holds up to 1,000, the answer's `limit`: past it the button reads "Show 1000", and the oldest ones are named as only in the terminal, with `careerbrew work feed show --show --open --top N`, which lists them; test: `test_past_the_cap_the_terminal_hint_lists_the_oldest_asks_too`); with 60 open asks, the 10 newest are listed (test: `test_with_60_open_asks_the_10_newest_are_listed_and_the_rest_page_in`; `web/e2e/feed.spec.ts`, issue #275).
  - Given a live refresh while I choose Show all, the click still expands the list and an older response cannot replace the expanded result (tests: `web/e2e/open-asks-refresh-click.spec.ts`, `web/src/open-asks-response-order.test.tsx`).
  - Given an ask, when I click Mark personal, then the app runs the `work.feed.mark` action (the same handler as `careerbrew work feed mark <id> personal`), the ask leaves the list, it stays out of evidence, the day log, and the PM update, and Undo marks it work again (test: `test_mark_personal_from_today_runs_the_cli_action`).
  - Given an event link that is not http or https (a plugin's `javascript:` link), when the API sends the ask, then the link is dropped and the title shows as plain text.
  - Given no feed events yet, when I open Today, then the section says how to connect Slack, PagerDuty, or Outlook and pull; given every ask answered, then it says so.
  - Given `careerbrew work feed show`, when I run it, then it prints the open asks and how many wait over `feed_ask_overdue_hours` ("open asks=2 (1 waiting over 8 work hours)"), the same count Today marks; an ask that came on Sunday is not overdue on Monday morning (test: `test_an_ask_is_overdue_after_8_work_hours_not_8_clock_hours`).
- **Metric**: open asks waiting over 8 work hours at day end (`careerbrew work feed show`), over 2 weeks. **Target**: half the first week's count.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-63-today-open-asks.sh` | `web.api`, `work.feed.mark` |

## US-64: Open a view again without waiting

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user whose work feed is busy, **I want** a view I open again to show at once, **so that** I switch between Today, Week, Work, and the day log without waiting seconds each time.
- **Acceptance criteria**:
  - Given no data file, setting, or clock input changed, when I open Today, Week, Dashboard, Work, the day log, Job hunt, or Actions again, then the server answers from its memory (`X-Careerbrew-Cache: hit`) without rebuilding the view, in well under 100 ms (test: `test_a_cached_repeat_of_the_day_log_answers_from_memory`).
  - Given a write by any process (the watcher, the CLI, a plugin), when the page polls `/api/changes`, then the version moves and the refetch shows the new data; a POST from the app clears every cached view (test: `test_new_data_shows_within_one_poll`).
  - Given I only read views, when the page polls `/api/changes`, then the version does not move: a read never looks like new data, so live views do not refetch for nothing (test: `test_reading_a_view_does_not_move_the_change_version`).
  - Given a watcher cycle that finds nothing new (`careerbrew day watch once`, also with every throttled source due), when the page polls `/api/changes`, then the version does not move and every cached view still answers from memory; a cycle that adds a result or a notification still moves it (test: `test_an_idle_cycle_does_not_move_the_change_version`, `test_a_cycle_that_adds_a_result_still_moves_it`).
  - Given a view that counts minutes (Today's next step, how long an ask waited, and Week, the day log, and Tomorrow, which plan from the free time left today), when the minute changes, then it is rebuilt; the other cached views are rebuilt on a new day, and every one at most `web_cache_seconds` (default 300) after it was built, so calendar feeds read over the network show up; `0` turns the cache off.
  - Given `careerbrew config set web_cache_seconds 99999`, when I run it, then it is refused with the range 0 to 3600.
  - Given `careerbrew serve` just started, or a watcher cycle, the CLI, or a plugin wrote new data, when I first open the day log, Today, open asks, Week, Work, or any Dashboard block, then the server already built it and answers from memory (issue #142; test: `test_the_warmer_warms_at_start_and_after_new_data`; on the seed: `scripts/api-timing.py --check`).
  - Given a view I opened before in this tab, when I switch back to it or reload the page, then its last data shows at once with "Updated hh:mm · refreshing…" before the server answers, and the fresh data replaces it; when `/api/changes` moved, the note says new data is coming (test: `web/e2e/blocks.spec.ts` "a view switch after /api/changes moved", `web/src/blocks.test.tsx` "says new data is coming").
  - Given a form in a block that shows its last data (a goal in Goals, an answer in a Teach me step, the PM update), when I type and the fresh data or a live refresh replaces it, then what I typed stays, and a Goals save still keeps the goals added elsewhere in the meantime (tests: `web/e2e/inputs.spec.ts` "Goals: a title typed while the view shows its last answer…", `web/src/teach.test.tsx`, `web/src/daylog.test.tsx` "keeps unsaved typing in the PM box").
  - Given the tab's copy of that data, when the browser keeps it on disk, then it is unreadable without the running server's key (AES-GCM, `/api/tab-key`), names no endpoint, holds no token, and a restarted server drops it; `careerbrew config set web_tab_cache false` keeps it in memory only (test: `test_the_tab_key_is_per_server_and_off_with_the_setting`, `blocks.test.tsx`).
- **Metric**: time to show a view again: the second `/api/daylog` request on the e2e seed (`uv run python scripts/api-timing.py`), and `ms` of `web.api` events with `cached: true` on your own machine; warm views answering their first open from memory, at start and after new data (`scripts/api-timing.py`). **Target**: under 100 ms; 14 of 14 warm views, both times.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-64-fast-views.sh` | `web.api`, `web.warm` |
| a reload shows the last data (issue #142) | `web/e2e/blocks.spec.ts` (run by `tests/e2e/us-58-action-feedback.sh`) | `web.api` |
| typed text survives the fresh data | `web/e2e/inputs.spec.ts` (run by `tests/e2e/us-72-every-control.sh`) | `web.api` |

## US-67: Every long run ends with one page of what was done, what blocks me, and what to do next

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who starts long runs from a terminal or an agent, **I want** each one to end with one HTML page (Done with evidence, Blockers with the fix, Next with the exact command) and my agent to ask me which next action to take, **so that** I read one scannable page instead of a wall of text and never lose a next step.
- **Acceptance criteria**:
  - Given `scripts/checkup.sh`, `careerbrew` with no command, `review`, `work collect`, `day plan`, `checkup`, `machine ready`, `machine new-user`, or `plugin new`, when it finishes, then it writes a self-contained page (inline CSS, light and dark, no network, no script) under `private/out/reports/<run>-<date>-<time>.html`, copies it to `latest.html`, and prints only the page path and one summary line (its key counts first) instead of its step text; `scripts/checkup.sh` folds each section's text into the page (test: `test_each_long_run_prints_only_its_page_path_and_one_summary_line`).
  - Given the repo checkup and open work found by the wrap-up skill, when the page is written, then each item is a Next step with its command and the open work is a blocker, never "No open work" (test: `test_open_work_from_the_wrap_up_skill_is_a_blocker_with_next_steps`).
  - Given a run page, when I run `careerbrew report latest`, then its Done, Blockers, and Next items print as text, without the page-only detail.
  - Given an agent, when it runs `careerbrew report latest --json`, then it gets counts, ids, statuses, and next-action commands only: a task title or a question's text stays on the page (test: `test_summary_is_content_free_and_detail_stays_on_the_page`).
  - Given the web app, when I open Reports (or run `careerbrew open --report latest`), then the newest page shows in a sandboxed frame behind the app's token, with every kept page listed.
  - Given `careerbrew config set run_pages false`, when a long run finishes, then it prints its step text, as before.
- **Metric**: lines `scripts/checkup.sh` prints with pages on, on the stub repo (`test_checkup_sh_writes_its_page_and_prints_the_path_and_one_summary_line`). **Target**: 2 (was 34), with 9 of 9 long runs writing a page and printing only two lines.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: Reports

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-67-run-pages.sh` | `report.write` |

## US-68: Keep a project and its memory file in sync both ways

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose agent keeps notes per project, **I want** careerbrew to update a project when its memory file changes, and to write what Slack, Jira, PagerDuty, and mail say about the project into that file, **so that** neither my project list nor my agent's notes go stale, and I never copy links between them by hand.
- **Acceptance criteria**:
  - Given a project with a memory file, when the file's content changes (by hash, not mtime), then the watcher's next cycle (or `work project sync`) adds its links as sources of their kind (the same reader as absorb), `trackers:` and `channels:` lines and ticket keys with a tracked prefix as sources, `links:` lines as project links, and the last `status:` line as the memory source's status; a content-free `work.memory.in` event is logged (test: `test_a_memory_edit_updates_its_project`, `test_a_change_is_found_by_content_hash_not_by_mtime`).
  - Given work-feed events and evidence from the last 7 days, when careerbrew collects (the watcher, `work collect`, `work feed pull` or `add`), then each item that matches a project (its tracker keys, its repo, channel, doc, or url sources, or its `project_priority` patterns) goes into the project's memory file as one dated line with its link, and collecting again adds nothing (test: `test_matching_items_become_dated_notes_with_their_link_once`).
  - Given a child project with no memory file, when careerbrew collects, then each item that matches it goes into its nearest parent's file once, under a `work tools` header that ends `(project <child>)`; an item the parent matches too is the parent's untagged note; a child's own file wins; a personal event never goes there, and a note whose event turns personal later leaves the parent's file (tests: `test_a_child_without_notes_gets_its_notes_in_the_parents_file_headed_with_its_name`, `test_a_childs_note_leaves_the_parents_file_when_its_event_turns_personal`, `test_an_item_of_both_the_parent_and_the_child_is_the_parents_note`, `test_a_childs_own_memory_file_wins`).
  - Given a personal event (a private entry, a rule, my mark), when notes are written, then it never goes into a memory file, and marking an event personal later removes its note, keeps the file's mode, and keeps an agent's append made meanwhile (test: `test_personal_events_never_reach_memory`, `test_an_event_marked_personal_later_leaves_memory`, `test_removing_a_note_keeps_the_files_mode`, `test_an_agent_append_during_a_removal_is_kept`, `test_a_file_rewritten_during_a_removal_is_left_alone_and_the_removal_retried`).
  - Given careerbrew just wrote notes (or absorb or checkup research did), when the watcher reads the file again, then it sees no change: it records the hash it wrote, never parses its own lines (also lines written while the sync was off, and in a file from before the sync only the lines in the shape it writes), still reads an edit made just before its write, and checkup research never copies the file into itself (test: `test_the_watcher_ignores_its_own_writes`, `test_an_edit_made_just_before_a_careerbrew_write_is_not_lost`, `test_checkup_research_never_copies_a_memory_file_into_itself`, `test_a_careerbrew_write_made_while_the_sync_is_off_is_never_read_back`, `test_a_file_from_before_the_sync_reads_the_owners_bullets_under_careerbrews_old_sections`).
  - Given business reading is off (no terminal, no plugin policy `agent_may_read_business`), when careerbrew collects, then no notes are written and `work project sync` says why; `config set project_memory_sync false` turns both directions off.
- **Metric**: matching fixture items written to the memory file after one watcher cycle, and duplicates after collecting again (`us-68-project-memory-sync.sh`). **Target**: 3 of 3, 0 duplicates, 0 personal events, 0 re-reads of careerbrew's own writes; for a child project with no file, 2 of 2 in its parent's file under its name, 0 duplicates.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-68-project-memory-sync.sh` | `work.memory.in`, `work.memory.out`, `work.memory.forget` |

## US-65: Get one standup page: what I did on each item I report on since the last standup

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer who reports on a team board at standup, **I want** one page that says, for each item I report on, what I did on it since the last standup, **so that** I give my update from evidence in a minute, and my plugin does not keep its own script for it.
- **Acceptance criteria**:
  - Given `standup_days`, `standup_time`, `standup_tz`, and `standup_minutes` (defaults: Monday to Friday, 10:00, the `timezone` setting, 15 minutes), when I run `work standup`, then the window starts at the last standup that has ended (with `standup_window since-last`; the default is below): on Monday it is the last standup day of the week before, and during the standup it is the one before (tests: `test_monday_counts_from_the_last_standup_day_of_the_week_before`, `test_a_run_during_the_standup_counts_from_the_one_before`).
  - Given `standup_window` (default `previous-day`, issue #200), when I run `work standup` or open the Standup view after today's standup, then the window still starts at the previous standup day's standup, never today's own, and both name the day they compare against ("compares against the Thu Oct 15 standup"); `since-last` keeps the last ended standup, and `config set standup_window` rejects any other value (tests: `test_a_monday_run_after_the_standup_starts_at_thursdays_standup`, `test_since_last_keeps_the_old_window`, `test_standup_window_is_a_validated_setting`).
  - Given a plugin or an agent passes rows with `work standup --rows <file|->` (title, status, owner, keys, project, repos, aliases; optional groups and `ticket_projects`), when the report builds, then rows I own (owner `me`, an owner in `standup_me`, or `mine: true`) are marked mine, and a row shows only when it has an update in the window (US-95: a row with no update is hidden, mine too); a bad row is refused, names the row and the fix, and writes nothing; the last good rows are kept encrypted for the next run and the web app.
  - Given my evidence and my own work-feed events in the window (never a personal one, chat, or mail), when they are matched, then the first hit wins, level by level: project, ticket key, repo alias, then whole-word alias; a group with `never_rows: true` (tooling repos) takes its work before any row; other groups take what no row matched; the rest is Other work.
  - Given the same PR as a PR, its squash commit, and a feed event, when the report builds, then it is one line that keeps the repo and the link from any copy (test: `test_one_line_per_action_when_a_pr_shows_up_as_a_pr_a_commit_and_a_feed_event`); given two actions that share only a title (two commits "fix: lint", two notes, two feed events on two tickets, or two links), when the report builds, then they stay two lines and each keeps its own row (test: `test_two_feed_events_with_one_title_on_two_tickets_stay_two_lines_on_their_rows`).
  - Given an evidence time with no offset, a title with `UTF-8`, or a feed event whose project title is null, when the report builds, then the time is read in the standup zone and kept, `UTF-8` is never a ticket key or dropped from a talking point (keys come only from configured projects), and a null project matches nothing (tests: `test_evidence_times_without_an_offset_use_the_configured_zone_not_dropped`, `test_ticket_keys_are_read_only_for_configured_projects`, `test_feed_project_titles_can_be_null`).
  - Given the report, when I open `out/standup.html` or the web app's Standup view, then each row shows a talking point first, then its status and owner, then the evidence list (time, kinds, linked title, repo, keys, and why it matched); the CLI prints only counts and the page path to an agent, and titles only with `--show` to my terminal.
  - Given an agent passes `work standup --talk <file|->` (`{row title: string or [bullet, ...]}`, 1 to 5 bullets of at most 200 characters each) or `--extra <file|->` (`[{title, at, kind, url, row}]`, actions from Slack, meetings, or mail), when the report builds in the same standup window, then a talking point replaces the row's generated line and says "written by an agent", and an extra action joins the row's evidence ("via an agent") and counts as new work, so a row with no other work shows that work; a bad input names the fix and writes nothing; in a new standup window both are gone (issue #229; tests: `test_a_talking_point_from_an_agent_replaces_the_generated_line`, `test_an_extra_action_turns_a_no_new_work_row_into_a_row_with_work`, `test_a_new_standup_window_clears_both`).
  - Given a row with three speaking bullets, the web view, HTML page, and Copy standup keep all three; a string remains a paragraph, and invalid counts or bullets over 200 characters are refused (#307; `test_issue_307.py`).
  - Given the Standup view, its window appears once as a subheading, Copy sits beside the title, Items you report on (or a callout) comes first, and no file path appears (#306; `standup.spec.ts`).
  - Given Today or Standup, the same source notice separates acquisition age, actual adapter running state, saved-row facts supplied age, and per-row proof age. Visible views expire without a data write and refresh after outside intake; rendering and new activity never renew saved-row review (#406, `live-source-health.spec.ts`).
- **Metric**: standup lines for one PR seen as a PR, a commit, and a feed event, and fixture rows reported correctly (`us-65-standup-report.sh`). **Target**: 1 line per action, and 4 of 4 fixture rows shown or hidden as the update rule says.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Standup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-65-standup-report.sh` | `work.standup`, `work.standup.rows`, `work.standup.notes` |
| web app view | `web/e2e/standup.spec.ts` (run by `tests/e2e/us-35-web-app.sh`) | `web.api` |

## US-69: Push one work project forward in one pass

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose project moves through PRs, tickets, review comments, and chat threads, **I want** my agent to push one project forward in one pass (collect what changed, act on it, record it, and name the next blocker), reading only the sources that changed since I last read them, **so that** no review ask is missed, even one that arrived in chat, and every pass ends with one clear next action.
- **Acceptance criteria**:
  - Given a project whose sources were checked at different times, when I run `work project check <name> --since`, then each source prints as changed (with a count of changes), unchanged, never read, unknown (with the reason), or reference (skipped), it says how many to read, and no `checked_at` moves (test: `test_since_names_changed_unchanged_never_read_and_references_and_stamps_nothing`).
  - Given business reading is off (no terminal and no plugin `agent_may_read_business`), when I run it, then local files are still compared by their modified time and every remote source is unknown with that reason; a url is unknown without a fetch, since a page has no change date.
  - Given tools that write times with different offsets (Jira `-0400`, GitHub `Z`), when changes are compared, then times are compared, not strings, and a date it cannot parse counts as a change (test: `test_after_compares_times_not_strings`).
  - Given a chat or tracker source, whose search filters by whole day in the searcher's own time zone (Slack's `after:` also leaves out the named day), when changes are counted, then the search starts days early and only a message or child updated after the check counts: a reply later on the day of the check is a change, and a child updated before the stamp is not (tests: `test_a_chat_reply_after_the_check_is_a_change_whatever_the_day_or_time_zone`, `test_a_tracker_counts_its_own_update_and_each_child_updated_after_the_check`).
  - Given an unknown project, an unknown ref, or `--since` with `--status`, when I run it, then it refuses, stamps nothing, and names the fix.
  - Given the `project-push` skill, when the agent runs it, then it reads the replies where each PR or ticket was posted, verifies each review claim before a fix, drafts every reply and posts none without a yes, asks before any change in another repo, stamps only the sources it read (never a reference), rewrites the memory note's `## State` block, and ends with the next blocker, its owner, and one action (cases: `evals/cases/022-project-push.yaml`).
- **Metric**: sources reported unchanged that did change, on the fixture project (`us-69-project-push.sh`). **Target**: 0, with every unchanged source skipped (1 of 4 on the fixture).
- **Skill**: `skills/careerbrew-project-push`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-69-project-push.sh` | `work.project` |

## US-70: Open a task's file, ticket, or channel from its row

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose agent adds tasks such as "tell the repo owner about a file", **I want** each task row to show the task's links and the agent's own why, **so that** I open the file, the ticket, or the channel to post in with one click instead of searching for each one.
- **Acceptance criteria**:
  - Given a task added with `work task --link <url>` (repeat it for more; `LABEL=URL` names a link), when I open Dashboard, Today (its plan and Next step), or Week, then every row of that task shows each link as an anchor that opens in a new tab (`target=_blank`, `rel="noreferrer noopener"`); a link with no label shows its host and last path part (tests: `test_today_week_next_step_and_dashboard_rows_carry_links_and_the_tasks_own_why`, `web/e2e/task-links.spec.ts`).
  - Given a task with its own `--why`, when its row shows, then that text shows as Context next to the computed score line, and each http(s) URL in it is a link; the rest stays text, HTML included (tests: `web/src/links.test.tsx`).
  - Given a `--link` value that is not a full `http(s)://` URL (a path, `#channel`, `javascript:`, `data:`, `file:`), when I add the task, then it is kept, the CLI warns and names the fix, and the web app shows it as text, never as a link (tests: `test_link_repeats_keeps_labels_and_warns_on_a_value_that_is_not_a_full_url`, `test_a_link_is_url_or_label_equals_url_and_only_http_is_web`).
  - Given a task saved before this change with one `link`, when careerbrew reads it, then it reads as `links`, every view shows it, and the next save stores the new shape; the day log's source link is the first http(s) link only (tests: `test_an_old_task_with_one_link_reads_as_links_and_a_save_keeps_the_new_shape`, `test_the_day_log_shows_the_first_web_link_and_never_a_script_link`).
- **Metric**: seeded task links shown as anchors that open in a new tab on Dashboard, Today, and Week, and anchors with a `javascript:`, `data:`, or `file:` address (`us-70-task-links.sh`); on your own machine, `web_links` / `links` of `work.task` events. **Target**: 3 of 3 on every view, and 0 script anchors.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Dashboard, Week

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-70-task-links.sh` | `work.task` |
| web app view | `web/e2e/task-links.spec.ts` (run by `tests/e2e/us-70-task-links.sh`) | `web.api` |

## US-71: Stay signed in to the web app across restarts

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who keeps the web app open all day while careerbrew updates and restarts, **I want** open tabs and bookmarks of `http://127.0.0.1:<port>/` to keep working after a restart, **so that** a restart never looks like a broken app, and other sites still cannot read my data.
- **Acceptance criteria**:
  - Given the web app runs and a tab is open, when the server ends (an update, a crash, a reboot) and the next `careerbrew open` or `careerbrew` starts it again, then the start reuses the token from `day/web-token.json` (encrypted at rest), and a reload of the old tab shows its data (tests: `test_a_restart_keeps_the_token_so_an_open_tab_keeps_working`, `web/e2e/signin.spec.ts`).
  - Given a tab that sent the token, when it signs in, then it gets a host-only cookie for 127.0.0.1 (HttpOnly, SameSite=Strict, Path=/, Max-Age until the token expires); given a new tab or a bookmark with no link, when it opens, then it gets the token back from `GET /api/session`, which answers only careerbrew's own pages (`Sec-Fetch-Site: same-origin`); every other API route still needs the token header (tests: `test_a_tab_with_the_token_gets_an_httponly_strict_cookie_for_127_0_0_1`, `test_every_other_api_route_still_needs_the_token_header`).
  - Given another web page (another site, or another port on 127.0.0.1), when it requests `/api/session`, then the browser sends no cookie from another site, and the server refuses both with 403 and `Cross-Origin-Resource-Policy: same-origin` (tests: `test_the_cookie_restores_the_token_for_this_apps_own_pages_only`, `web/e2e/signin.spec.ts`).
  - Given `careerbrew web rotate-token --yes` (or Run with confirm in the web app's Actions view), when it runs, then the old token, tab, and cookie stop working at once without a restart, the open tab shows the sign-in page within one live-refresh poll, and no token prints; without `--yes` it refuses (tests: `test_rotate_token_signs_out_the_old_token_at_once`, `test_rotate_token_runs_from_the_web_app_only_when_confirmed`).
  - Given a token older than `web_token_max_days` (default 30, 1 to 365), when a tab uses it, then the API answers 401 with the fix, and the next `careerbrew open` makes a new one (test: `test_the_token_expires_after_web_token_max_days`).
  - Given a tab that cannot sign in, when it opens, then the sign-in page names the one command (`careerbrew open`), and its button opens a signed-in tab in the default browser (the server opens the one-time link; the reply never holds the token), after which the tab signs in again by itself (test: `test_the_sign_in_page_button_opens_a_signed_in_tab_without_returning_the_token`).
  - Given any of these flows, when the logs, `day/web.json`, `careerbrew serve` without a terminal, and `rotate-token` are read, then none of them holds the token (tests: `test_web_token_events_are_content_free`, `test_serve_prints_the_token_only_to_a_terminal`).
- **Metric**: sign-in pages shown after one restart, for the old tab and a bookmark (`us-71-web-sign-in.sh`). **Target**: 0 of 2, with 2 of 2 cross-site attempts refused and 0 tokens in the logs.
- **Skill**: `skills/careerbrew-privacy-guard`
- **Web views**: Sign-in page

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-71-web-sign-in.sh` | `web.token`, `web.session` |

## US-72: Every control in the web app does what it says

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who clicks, types, and picks in the web app, **I want** every input, select, textarea, checkbox, button, and form in every view to work, each checked by a browser test of its real effect, **so that** no control quietly does nothing, and a new control cannot ship without a test.
- **Acceptance criteria**:
  - Given every view on a personal and a corporate machine, with each folded part open and each state the seed lacks shown (each teacher step, an edited PM draft, notifications off, the watcher on or paused, a refused clipboard behind Copy update, a block whose read failed, and the sign-in page of a signed-out tab), when the control inventory (`web/e2e/controls.spec.ts`) lists each control by its accessible name, then each one has an entry in `web/e2e/controls.json` naming the Playwright test that exercises it, that test exists, and a control with no name, no entry, or a missing test fails the run with the fix.
  - Given a control, when its mapped test clicks, types, or picks, then the test checks the real effect: the server's state read back through the API, the view after the server answered, or an error that names the fix.
  - Given a button that has nothing to act on yet (Add task with no title, Open loop with no company, a long-term goal's Add with nothing typed), when I look at it, then it is disabled instead of doing nothing on a click.
  - Given the Actions filter, when I type a name in capitals, then the action still matches.
  - Given the Goals view showed its last answer from the page's cache, when the fresh answer has goals added elsewhere (the CLI, an agent), then the view shows them, and a save from the view keeps them.
  - Given `scripts/check.sh`, when its e2e step runs, then it runs the whole Playwright suite (no `-g` filter) on its own port, so a new browser test runs in the gate the moment it exists.
- **Metric**: controls in the inventory with no passing test (`controls.spec.ts`, run by `us-72-every-control.sh`). **Target**: 0, with every Playwright test run by the gate.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: Week, Interviews

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-72-every-control.sh` | `web.api` |

## US-73: Find the block I act on first at the top of every page, and page through long lists

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** an owner with a busy work feed (100+ suggestions, many asks, a long day log), **I want** each web view to start with the block I act on first and to show long lists one page at a time, **so that** the PM update, the next step, and the asks that need me stay above the fold on any day.
- **Acceptance criteria**:
  - Given any of the 16 views, when it opens, then its blocks show top to bottom in the order docs/web.md lists ("Block order and paging"); on Today that is Copy update, the PM update draft, Next step, open asks, questions, suggestions, the plan, the day log, earlier days (tests: `web/e2e/order-paging.spec.ts` per view, `web/src/order.test.ts` for docs and code).
  - Given a list of live data longer than its page (suggestions, questions, open asks, results, the day log's lists, stale sources, notifications, standup rows, Work's tasks, projects, and evidence, Job hunt's suggestions, the jobs board, postings, run pages), when it shows, then it shows one page, "Showing 1–10 of N", and Previous and Next, most important rows first (by priority where a row has one, then by time); a list that fits shows neither.
  - Given a page or a filter I chose, when I reload, then the view keeps it: the hash holds it after `?` (`#/today?suggestions=2&suggestions-filter=goal`), and a link to a view starts on page 1.
  - Given a hand-edited link with a page past the end or text that is not a bare token, when it opens, then the list shows its last page, and the app never writes typed text or a non-token value into the URL (error case).
  - Given a folded block (Notifications' Closed, Today's Housekeeping), when it is folded, then its header shows the count ("Closed (12)"); a notification link opens the fold and the page that hold it.
- **Metric**: views whose first block and block order match docs/web.md, and long lists that show one page with "Showing" and keep their place across a reload (`us-73-page-order-and-paging.sh`). **Target**: 16 of 16 views; 0 lists that grow the page past one page of rows.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: Today, Notifications

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-73-page-order-and-paging.sh` | `web.api` |

## US-74: Understand every suggestion in my inbox, decide alike ones at once, and drop the ones already done

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a new owner with a long suggestions inbox (100+ rows: trackers research found, agent tasks, tickets assigned to me), **I want** the inbox to say what a suggestion is, where each row came from, and what Accept and Dismiss will do, to fold repeats into one row, and to drop asks already done elsewhere, **so that** I can decide in minutes instead of guessing or ignoring the list.
- **Acceptance criteria**:
  - Given the Today view, when the suggestions block shows, then one paragraph at its top says what a suggestion is, who adds them (careerbrew, a plugin, an agent), and that nothing changes until Accept; and every block of every view has a "What is this?" line (tests: `web/e2e/inbox.spec.ts`, `web/src/order.test.ts`).
  - Given any suggestion, when it shows, then it has its whole title (wrapped, never cut), "Why this is suggested" (a research row names the parent ticket its project follows, never the title again), its source, at least one link (the ticket, the parent, the task's own links, or the view Accept lands it in), and "Accept will …" and "Dismiss will …" before the click (tests: `tests/unit/test_issue_147.py`, `web/e2e/inbox.spec.ts`).
  - Given 3 or more alike suggestions (`inbox_group_min`: one tracker's children for one project, one agent's tasks for one reason, one plugin's suggestions with one Accept), when the inbox shows, then they are one group ("Add 3 trackers to project <name>") with Accept all, Review each, and Dismiss all; Accept all and Dismiss all ask to confirm first, Cancel changes nothing, and Yes decides every row the confirm counted, while a row that joined the group since stays open (the page sends the ids it showed; the CLI needs `--yes`, without it exit 2 and nothing changes, and a `--count` other than the group's size exits 1 and changes nothing: error cases).
  - Given the suggestions, when I filter by kind, project, or priority (the rank of the row's project in `project_priority`) or sort them, then the list shows those rows one page at a time and the URL hash keeps the choice across a reload.
  - Given restricted CLI output, agent-task rows show canonical ASCII task ids beside their inbox ids without titles, reasons, links or arbitrary legacy task references. An explicit `day inbox --group` shows every member, including groups over 100; the normal list budget stays unchanged (test: `test_issue_371.py`).
  - Given an ask whose PR got merged, a review I gave, or CI that went green, in data careerbrew already reads (evidence and the work feed), when the next cycle runs, then a sure match leaves the list for Done elsewhere (kept, never deleted, with Bring back that keeps it for good) and an unsure one stays with "Maybe done already" (`inbox_expire_done`).
  - Given a new user, when Dashboard opens, then Getting started lists the first steps, each ticked from data careerbrew has, with a link and a command, until every step is done or I press Hide checklist (`careerbrew checklist` prints the same list).
- **Metric**: suggestions that show a source, a link, and what Accept and Dismiss will do; rows a 36-tracker research inbox shows; asks done elsewhere still open after a cycle (`us-74-inbox-for-new-users.sh`). **Target**: every row; 1 group row instead of 36; 0 sure matches left open, none deleted.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today, Dashboard

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-74-inbox-for-new-users.sh` | `day.inbox.group`, `day.inbox.expire` |

## US-75: Catch a broken screen before it ships: every story's screens are approved pictures

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who reads careerbrew in the web app all day, **I want** each story's key screens compared with approved pictures at desktop and phone width, **so that** a wrong date, a list that reorders, an empty block, or text cut off at the edge is caught before it reaches me.
- **Acceptance criteria**:
  - Given a story with web views (its **Web views** line), when `scripts/check-stories.py` runs, then it fails unless `web/visual/us-NN-*.spec.ts` exists and each picture it names is approved at desktop (1280 × 800) and phone (390 × 844) width; given **Web views**: none on a story that names the web app or a view, then it fails unless the line says why, `none (...)` (tests: `tests/unit/test_check_stories.py`).
  - Given the visual tests, when the gate runs them (`tests/e2e/us-75-visual-stories.sh`), then every screen matches its picture pixel for pixel on the synthetic seed, with the server and the browser clock pinned at Monday 2026-10-19 09:00 in Etc/GMT+4, system fonts, no animations, and no caret; the three values that change on every run (a CPU measurement, a file's write time, the temporary data folder) are pinned in the page, so no part of a picture is masked.
  - Given a screen that differs from its picture, or a picture that is missing, when the visual tests run, then the run fails, names the picture, and writes nothing; given a screen changed on purpose, when I run `npm run visual:update` in `web/`, then the changed pictures are redrawn, and I open each one before I commit it (docs/web.md, "Visual tests").
  - Given the server's own answers, when one lists items from a set (a company's sources), then its order is the same on every start, so a picture never changes on its own (test: `test_a_company_page_lists_its_sources_in_one_order_every_start`); given a pinned clock, when Review, the Work view, a task's history, Teach me's today, cards and drills, approvals, goals history, the job cooldown, mistakes, prep runs, or any other stored stamp reads the time, then it reads the pinned one (tests: `tests/unit/test_pinned_clock.py`); given a new `datetime.now()` or `date.today()` in `src/careerbrew` outside the allow list of real elapsed-time reads (the watcher's throttles, the step timer, event and backup stamps, each with its reason), when the unit tests run, then they fail and name the file and line (test: `tests/unit/test_clock_reads.py`).
- **Metric**: stories with web views whose pictures are missing or differ (`check-stories.py`, `us-75-visual-stories.sh`), and pictures that change between two runs with the same inputs. **Target**: 0 of 49, and 0.
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: none (it checks the pictures of the other stories)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-75-visual-stories.sh` | `web.api` |

## US-76: Read and tap every view comfortably on a phone and a desktop

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a user who opens careerbrew on a laptop and on a phone, in light or dark mode, **I want** every view to be easy to read and to tap, with clear headings and friendly dates, **so that** I see what needs me at a glance and never squint at small text or miss a small button.
- **Acceptance criteria**:
  - Given every view, a company page, and an action form, on a 375 px phone and a 1280 px desktop, in light and dark, with reduced motion and every folded part open, when the comfort spec (`web/e2e/comfort.spec.ts`) checks the loaded page, then it finds no serious or critical axe problem, no text under 14 px, no sideways scroll, and on the phone no touch target under 44 px.
  - Given a phone, when I open the app, then one top bar shows the brand and a Menu button; the button opens the grouped views, and opening a view (also the one I am on) closes it.
  - Given the everyday jobs on a phone, when I do them, then opening Today from another view takes 2 taps, copying the PM update takes 1, and showing a flashcard's answer takes 1.
  - Given any app text that names a day (a heading, a why, a history row, a due date, a run page's title), when it shows, then it reads "Mon Oct 19" or "Monday, Oct 19" (another year's with its year), never an ISO date; a value that is not a day stays as it is, and code, typed values, and imported notes keep theirs.
  - Given manually created comfort-test contexts, when a test succeeds, its body fails, or opening a page fails, then its test-scoped fixture closes only its own contexts; the final browser-only probe finds zero residual contexts. This resource check does not establish the cause of an unrelated stalled page.
- **Metric**: comfort budget problems over 18 pages and 4 modes (`comfort.spec.ts`). **Target**: 0 (baseline on 2026-10-22: 3 axe, 212 kinds of small text, 1,115 small phone targets).
- **Skill**: `skills/careerbrew-start-session`
- **Web views**: none (it measures every view itself, 18 pages in 4 modes: `web/e2e/comfort.spec.ts`)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-76-comfort-budget.sh` | `web.api` |

## US-77: Watch every active project from one list

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose daily agent routine watches several work projects, each with its own threads, tickets, dashboards, and status doc, **I want** one section per active project and a machine-readable list of the sources to watch, **so that** the routine loops over my real projects without a hard-coded list and I see each project's latest change at a glance.
- **Acceptance criteria**:
  - Given projects with sources, when I run `work project list --active --json`, then each active project prints once with its group, parent, last check, stale count, latest update, and each source's kind, ref, last check, stale and reference flags, and reader; titles, statuses, and update text print only when business reading is on (test: `test_without_business_reading_the_list_holds_no_titles_statuses_or_update_text`).
  - Given `work project archive <name>`, when I list projects (`list` and `list --json` show the active ones; `--all` adds archived ones), then it is gone, its sources are never stale in checkup, `day next`, or `work focus`, the watcher's memory sync skips it, and `unarchive` watches it again; adding a project adds it to the list (tests: `test_archiving_a_project_changes_what_the_active_json_list_returns`, `test_the_watcher_skips_an_archived_projects_memory_file`).
  - Given `work project remove <name>`, when I run it without `--yes`, then nothing is removed and it names `--yes` and archive; with `--yes` the record is deleted and its memory file stays; it is refused while another project names it as a parent or link (tests: `test_remove_needs_yes_and_keeps_the_memory_file`, `test_remove_is_refused_while_another_project_names_it`).
  - Given `work project update <name> <company|project|team|other> "<one line>"`, when it is noted, then it is the project's latest change in the JSON list and the Work view; the last 20 are kept, and a bad kind, an empty line, or two lines is refused with the fix.
  - Given the Work view, when it opens, then Watch list shows one section per active project with its sources (stale ones first, 5 then "Show N more"), when one was last checked, and the latest change, and archived projects carry an "archived" tag in Projects (test: `web/src/views.test.tsx`).
- **Metric**: projects an agent's routine must hard-code to watch. **Target**: 0.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-77-project-watch.sh` | `work.project` |

## US-78: Keep my teammates' work in the feed, with every link read

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose projects move in my teammates' threads as much as in mine, **I want** careerbrew to know my team and keep their messages, and messages about them, as worth a look, with every link in a kept message read, **so that** decisions, next steps, and shared docs that shape my projects are not lost as chatter. The teammate feed reads coworkers' messages, so it needs company policy approval before use.
- **Acceptance criteria**:
  - Given `work team add <name> [--chat <id>] [--email <addr>] [--login <login>]`, when I add, fill in, or `remove` a teammate, then the roster (`work/team.yaml`, encrypted) changes, every kept event is triaged again, and `work team list` prints names only to my terminal (agents see which ids are set, never names) (test: `test_an_agent_sees_no_teammate_names_or_links`).
  - Given an event whose actor is a teammate (chat id, login, email, or name), or whose text names one (a whole-word name, `<@chat-id>`, or `@login`), when it is triaged, then its analysis carries `team` (the names) and it is `fyi` by rule `teammate` where another rule would make it noise; an owner rule that names `team` in its `when` wins (tests: `test_a_teammates_event_is_fyi_not_noise_and_names_the_teammate`, `test_an_owner_rule_that_names_team_wins_and_team_is_a_rule_key`).
  - Given a kept event (not noise, not personal) with links in its text (Slack's `<url|label>` or bare URLs), when I run `work feed links --pending`, then each link not yet researched is listed (to my terminal); `work feed research <id> <file>` stores a one-line summary per link, or `opened: false` with the reason, and is refused when a link is missing; research survives a retriage (test: `test_research_is_refused_unless_it_covers_every_link`).
  - Given a Slack connector and teammates with chat ids, when the feed pulls, then it also searches each teammate's messages and mentions of them, and reads every reply of each thread it found as its own event whose `thread` is the parent's id; without the history scope the replies are skipped, the pull still works, and a `work.feed.replies` event names the error (tests: `test_each_reply_of_a_thread_is_its_own_event_under_the_parent`, `test_without_the_history_scope_replies_are_skipped_and_the_pull_still_works`).
  - Given kept teammate events, when I run `work team feed`, then each lists with the project it matches (or none) and its link research, titles only to my terminal; a teammate's note in a project's memory file names them (`<date> slack: Sam Lee: <title> <link>`) (tests: `test_team_feed_groups_teammate_events_by_project_and_shows_research`, `test_a_teammates_note_line_names_them`).
- **Metric**: links in kept teammate events with no research entry (`work feed links --pending`). **Target**: 0 after an agent's daily pass.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none (CLI and agent actions; teammate events reach a project's memory file)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-78-team-feed.sh` | `work.team`, `work.feed.research`, `work.feed.replies` |

## US-79: Share how the careerbrew repo is doing at one stable link

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** the owner who runs careerbrew on more than one machine, **I want** each machine's repo checkup to refresh one claude.ai Dashboard of gate health (step failure rates and times, e2e stories passing, the last checkup), built the same way on every machine, from an export that cannot hold my data, **so that** I see flaky steps at a glance and can share the page without leaking anything.
- **Acceptance criteria**:
  - Given the gate's `check.step` events, when I run `careerbrew report public`, then it prints one summary line (gate runs, failed step runs, e2e passing, checkup status, open PRs) and the steps that fail most; `--json` prints the whole export and `--out <dir>` writes `summary.json`, `steps.json`, and `daily.json` as `{rows: [...]}` (test: `test_cli_prints_json_and_writes_the_dashboard_row_files`).
  - Given events that name a step `scripts/check.sh` does not define, e2e story names, or a checkup whose labels hold business text, when the export is built, then none of that text is in it: only fixed keys, numbers, gate step names, days, times, and statuses (tests: `test_the_export_counts_gate_steps_and_holds_no_text`, `test_the_checkup_gives_its_status_and_counts_never_its_words`, `test_the_allowlist_refuses_anything_it_does_not_know`).
  - Given one of my sensitive terms in the export, when it is built, then it is refused with exit 1 and nothing is written (test: `test_a_sensitive_term_refuses_the_whole_export`).
  - Given `/careerbrew-checkup` on any machine, when it finishes, then it refreshes the Dashboard at `checkup_artifact_url`, or creates one from `config/repo-health-dashboard/` and saves its link with `careerbrew config set`; the page reads only columns the export writes (test: `test_the_tracked_dashboard_reads_only_what_the_export_writes`).
- **Metric**: the e2e gate step's failure rate over 28 days (`careerbrew report public --json`, `steps[e2e].failure_rate`). **Target**: 5% or less within 4 weeks (was 14.9% on 2026-10-25).
- **Skill**: `skills/careerbrew-privacy-guard` (what may be published; the steps are in `commands/careerbrew-checkup.md`)
- **Web views**: none (a claude.ai Dashboard, not the local web app)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-79-repo-health.sh` | `report.public` |

## US-80: Learn a project so it sticks, and see that it stuck

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer learning a new repo, service, or design (on a personal or a corporate machine), **I want** careerbrew to keep what I studied, what I got right, and what is due next across days, **so that** I remember the project a week later instead of only during the session.
- **Acceptance criteria**:
  - Given a local repo and a fetched doc, when I run `learn start <project> --repo PATH --doc LABEL:FILE --consume`, then the repo is recorded by path and commit, the doc is stored encrypted under `private/learn/`, my plaintext copy is deleted, and nothing is written inside the repo; a second run names the sources that changed (tests: `test_start_reads_repos_in_place_and_stores_fetched_text_encrypted`, `test_freshness_names_the_sources_that_changed`).
  - Given a concepts file with a missing citation, a prerequisite cycle, more than 30% pure-recall questions, or an untagged wrong option, when I run `learn concepts <project> FILE`, then it names each problem and imports nothing (test: `test_the_concept_check_blocks_bad_cites_cycles_recall_heavy_decks_and_untagged_options`).
  - Given imported concepts, when I run `learn next`, then a new section starts with an unscored pretest and a concept waits for its prerequisites; when I record `learn answer`, then every taught concept gets a spaced review, mastery needs two confident correct answers with one at Bloom `apply` or higher, and a mastered concept gets retests at +1 and +7 days (tests: `test_next_starts_each_section_with_a_pretest_then_teaches_prerequisites_first`, `test_mastery_needs_two_confident_correct_answers_one_at_apply`, `test_mastery_schedules_retests_at_one_and_seven_days`).
  - Given a corporate machine, when I run any `learn` command, then it works, and learn cards never enter the interview deck (tests: `test_learn_runs_on_a_corporate_machine`, `test_learn_cards_stay_out_of_the_interview_deck`).
  - Given an unknown project or concept, when I run `learn status` or `learn answer`, then it names the command that fixes it and exits 1.
- **Metric**: the share of taught concepts with a spaced review scheduled (`learn status --json`, `scheduled_share`, `us-80-learn-project.sh`). **Target**: 100% (was 0%: the agentbrew skill had no scheduler).
- **Skill**: `skills/careerbrew-learn-project`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-80-learn-project.sh` | `learn.answer` |

## US-81: Sign in to Outlook once and stay connected for weeks

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose day plan reads Outlook mail and meetings, **I want** to sign in once with a device code and have careerbrew refresh the token by itself, **so that** the connector keeps working for weeks instead of going quiet an hour after I paste a token.
- **Acceptance criteria**:
  - Given an app registration's client ID (the `outlook_client_id` setting or a company plugin's `connector_defaults.outlook.client_id`), when I run `careerbrew connect outlook`, then it shows a link and a code, waits for my sign-in, and keeps only the refresh token, in the Keychain; no token reaches a file, stdout, or the event log (test: `test_connect_signs_in_with_a_device_code_and_keeps_only_the_refresh_token`).
  - Given no client ID, when I connect, then it names the setting, and says a manual token must be one your admin issued or allows, and that it expires after about an hour (test: `test_connect_without_a_client_id_names_the_setting_and_the_manual_token`).
  - Given a signed-in connector, when careerbrew reads the calendar, mail, or feed, then it refreshes the access token first, once per hour per process, and keeps the new refresh token (test: `test_each_read_refreshes_the_token_once_and_keeps_the_rotated_one`).
  - Given Microsoft refuses the refresh token, when careerbrew reads, then Today, the day plan, the work feed, and Setup show one step, "Reconnect Outlook", not empty data; a new sign-in clears it (tests: `test_an_expired_sign_in_shows_one_reconnect_step_not_empty_data`, `test_a_new_sign_in_clears_the_reconnect_step`).
- **Metric**: `connector.reconnect` events per signed-in connector per 30 days. **Target**: at most 1.
- **Skill**: `skills/careerbrew-corporate-setup`
- **Web views**: none (the reconnect step is text in Setup's existing connector steps and Today's existing calendar note)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-81-outlook-sign-in.sh` | `connector.reconnect`, `connector.run` |

## US-82: See my ranked projects at a glance and change the ranking where I see it

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer with many projects whose priorities change day to day, **I want** the projects I ranked with `work priority set` at the top of Today and above Projects, with their open tasks and today's items, **so that** I see what is active now without the terminal and change the ranking in one tap.
- **Acceptance criteria**:
  - Given a ranking, when I open Today or Work, then Active now lists the ranked projects in rank order, each with its open tasks and today's items, the same numbers as `careerbrew work priority show` (tests: `test_the_active_now_numbers_match_work_priority_show`, `web/e2e/active-now.spec.ts`).
  - Given Active now, when I tap Up, Down, or Remove on a project, then the web app runs the same `work priority set` and `remove` actions as the CLI, the ranking changes, and the PM update, the plan, and Next step follow it; Put back sets a removed project again at its old rank with its old patterns (tests: `test_move_and_remove_from_the_web_run_work_priority_set_and_remove`, `web/src/views.test.tsx`).
  - Given the Projects list on Work, when it opens, then ranked projects come first in rank order with a `#rank` badge, and every other project stays listed in its usual order (test: `test_the_projects_list_puts_ranked_projects_first_with_their_rank`).
  - Given no ranking, when Active now shows, then it says how to rank a project and the Projects list keeps its order (test: `test_with_no_ranking_the_view_is_empty_and_the_projects_keep_their_order`); given an unknown project, `work priority remove` names the projects it has.
- **Metric**: taps from opening Today to moving a project up one rank. **Target**: 1.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Today, Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-82-active-now.sh` | `work.priority` |

## US-83: Learn a repo by redoing its real changes and walking its code

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer learning a repo, **I want** to redo one of its real small changes, graded by that change's own tests, and to walk a code tour that stops explaining what I already know, **so that** I prove I can work in the code, not only talk about it.
- **Acceptance criteria**:
  - Given a staged repo with squash-merged PRs, when I run `learn replay list <project>`, then I see the newest small PRs (12 files and 500 lines at most) with their test files (test: `test_candidates_are_small_squash_merged_prs_with_their_tests`).
  - Given a PR with tests, when I run `learn replay start <project> <pr>`, then I get a clone in my data folder at the PR's parent with only its tests; `learn replay check` fails until I make the change and passes after; the learned repo and its `.git` are unchanged (test: `test_a_replay_task_fails_until_the_change_is_made_and_writes_nothing_in_the_repo`).
  - Given a PR without tests, when I start it, then it says nothing can grade it and exits 1 (test: `test_a_pr_without_tests_cannot_be_a_task`).
  - Given a tour with a missing file, a line past the end, or an unknown concept, when I run `learn tour add`, then it names each problem and stores nothing; when I export a valid one, then mastered steps ask me to predict, and step 1 says when no maintainer approved it or when a toured file changed since approval (tests: `test_a_tour_is_checked_against_the_staged_repo`, `test_a_tour_fades_mastered_steps_and_flags_review`).
- **Metric**: the share of the last 50 small merged PRs that make a runnable task (`learn replay measure --repo PATH`). **Target**: 70% on careerbrew (was 56% on 2026-10-26: 28 of 50; 17 changed no test, so 66% is this repo's ceiling).
- **Skill**: `skills/careerbrew-learn-project`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-83-learn-replay-tour.sh` | `learn.replay` |

## US-84: Every decision I make changes a later one

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner who accepts, dismisses, answers, and skips what careerbrew offers, **I want** each of those decisions to change what it offers next, **so that** I teach it once instead of saying no to the same kind of thing every day (issue #185).
- **Acceptance criteria**:
  - Given proposals from one plugin or kind that I keep dismissing, when the inbox lists open proposals, then they rank below ones I accept or have not judged; with no decisions the order is unchanged (test: `test_a_plugin_the_owner_keeps_dismissing_ranks_below_one_with_no_decisions`).
  - Given 3 questions of one kind about one source that I skipped in a row (a project, a tool such as GitHub, a tracker or repo, who added a task), when new questions are picked, then no new question of that kind is asked about that source; an answer ends the run, and other sources are still asked (tests: `test_a_question_kind_skipped_three_times_for_one_project_is_not_asked_again`, `test_skipping_one_tools_impact_questions_mutes_only_that_tool`, `test_an_answer_ends_a_run_of_skips`).
  - Given more questions than `questions_per_day`, when the budget fills, then the kinds I answer come first (test: `test_the_question_budget_goes_first_to_the_kinds_the_owner_answers`).
  - Given a plugin's suggestions I decided, when it reads them back (`day suggestions`), then each row carries the weight its next suggestions get (test: `test_a_plugin_reads_back_the_weight_its_next_suggestions_get`).
  - Given my decisions, when I run `careerbrew signals [--json]`, then each signal says what it changes now (test: `test_signals_say_what_each_decision_changed`).
  - Given PM updates I edited and saved, when the next draft is written, then a line I cut on 2 days is left out, a label I renamed twice is written my way, a part I cut twice is dropped, and unranked lines follow my order; one edit is not a rule, and the raw draft stays as it was (tests: `tests/unit/test_issue_185_pm.py`).
  - Given the PM update I sent (recorded with `careerbrew day pm-sent <file>`, or my own message in the `pm_slack` channel that day), when careerbrew learns, then it learns from the sent text like a save, the sent text wins over a web save of that day, the same text is learned once, a kind of line I added on 2 days shows in `signals`, and the text is never printed; with no shown draft it says so (tests: `tests/unit/test_issue_238.py`).
  - Given a sent Slack update is edited, when a watcher cycle adds no result rows, then its original sent day learns the latest text without duplicating that day's history or rewriting an unchanged repeat. A sent DM requires exact recipient metadata. App sends follow only their saved response identity; a manual sent record remains authoritative (tests: `tests/unit/test_pm_sent_reconciliation.py`).
  - Given my goals and Teach me choices, when the next step is picked, then new cards on the goal's topics come first, a long-term goal that names a competency raises its gap, a card I drop leaves review, an exercise I saw the solution for comes back after 3 days, and a step kind I rated low twice moves later (tests: `tests/unit/test_issue_185_teach.py`).
  - Given the smaller inputs (a task I regrade, tasks I finish, notifications I snooze, notes on approvals, tags on job ratings), when careerbrew next plans, notifies, or ranks, then each one changes it as `docs/feedback-signals.md` says (tests: `tests/unit/test_issue_185_gaps.py`).
  - Given an open ask, when I mark it not an ask or done (Today's buttons or `work feed mark <id> not-ask|done`), then it leaves my open asks, stays closed after the next pull, and Undo opens it; 3 not-ask marks or dismissed ask notifications from one channel or sender, or 3 personal marks, propose a feed rule I can accept (tests: `tests/unit/test_issue_185_feed.py`, `web/e2e/feed.spec.ts`).
- **Metric**: inbox items of a kind I dismissed 3 or more times in a row that still reach the top of my list. **Target**: 0.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: Today

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-84-decision-signals.sh` | `day.inbox` |

## US-85: The first session each morning starts from what yesterday's session left

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agents help through the work day, **I want** a short next-day handoff kept per day and shown first the next morning, **so that** a new session does not re-read every thread, repeat asks I already sent, or forget promises I made (issue #210).
- **Acceptance criteria**:
  - Given items added with `careerbrew day handoff add <section> <text> [--link]`, when the next day starts (`careerbrew` or `day handoff show`), then the latest earlier handoff shows first, grouped by section, with "verify each one live before acting" (tests: `test_the_next_day_shows_the_latest_earlier_handoff`, `test_start_shows_the_handoff_first`).
  - Given an item, when it is done (`day handoff done <id>`), then it leaves the handoff; adding the same item twice keeps it once; every change rewrites the whole file (tests: `test_done_drops_an_item_and_a_repeat_is_kept_once`).
  - Given an unknown section, empty text, or text over 200 characters, when I add it, then it is refused and names the story (test: `test_bad_items_are_refused`).
  - Given an agent (stdout not a terminal) with no plugin policy that allows business text, when it reads the handoff, then it sees ids, sections, and links, not item text (test: `test_an_agent_sees_ids_not_text`).
- **Metric**: mornings whose first session shows the earlier handoff before any other step. **Target**: every morning that has one.
- **Skill**: `skills/careerbrew-day-planner`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-85-handoff.sh` | `cli.command` |

## US-88: A PM update is graded against the next ladder level

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner who wants a promotion, **I want** each daily or PM update graded against my next ladder level, **so that** ladder gaps show while the work happens, not when the promo doc is due (issue #216).
- **Acceptance criteria**:
  - Given any configured ladder, when I run `careerbrew ladder levels --level <id> --json`, then it prints each dimension's id, name, description, and looks-like / doesn't-look-like examples (tests: `test_levels_lists_each_dimension_with_its_description_and_examples`, `test_cli_levels_json`).
  - Given today's PM update, when I run `careerbrew day pm-grade`, then each line is tagged with its dimensions and I see coverage strong / some / none, plus one hint per weak dimension that quotes only my own lines or names the work that would create a true fact (test: `test_each_line_is_tagged_and_coverage_is_strong_some_or_none`).
  - Given `--record`, when it runs twice, then each tagged line counts once as promo evidence (test: `test_recorded_lines_count_in_promo_once`).
  - Given a task with no competencies, when tasks are ranked for the plan, Week, or Next step, then the ladder reads its title and a task that closes a ladder gap ranks higher (test: `test_an_untagged_task_gets_the_ladder_bonus_from_its_title`).
  - Given the PM report, then it carries the grading for the owner's view, and its body stays free of tags (test: `test_the_pm_report_carries_the_owner_only_grading_but_not_in_its_body`).
  - Given an agent caller, then it sees coverage counts but no line text; the sent update never holds tags (test: `test_agents_see_coverage_but_no_line_text`).
  - Given an unknown ladder or level, then it is refused and names the story (tests: `test_an_unknown_level_is_an_error`, `test_cli_levels_error_names_the_story`).
- **Metric**: ladder dimensions with coverage "none" in the week's updates. **Target**: falls week over week.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-88-ladder-grade.sh` | `work.ladder_grade` |

## US-86: See how much time careerbrew saves me each week

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** a programmer who wants proof the assistant pays off, **I want** the minutes it saves me per week, from my own runs, **so that** I keep using what saves time and drop what does not.
- **Acceptance criteria**:
  - Given a run of an action that replaces manual work (PM update send, standup, day log, day plan, evidence collect, promotion doc, job scan, inbox accept), when it finishes, then `cli.run` logs its `saves` id and `seconds`, and nothing else about it.
  - Given a PM update, a standup, or a suggestion open in the web app, when I press Copy update, Copy, Send, Copy standup, or Accept, then `web.timing` logs the action id, the step, and my seconds from the open (visible tab time only), and nothing else; `careerbrew metrics time-saved` counts that run with my time, labeled `web`, and a CLI run keeps its run time, labeled `cli` (tests: `tests/unit/test_time_saved.py`, `web/e2e/time-saved.spec.ts`).
  - Given timed runs, when I open Dashboard, then Time saved shows this week's minutes saved and the last 4 weeks, with how many runs used my own time.
  - Given no baseline of mine, when I run `careerbrew metrics time-saved`, then each action uses its default estimate, labeled `estimate, <source>`; an estimate is never shown as measured.
  - Given `careerbrew time baseline <action> <minutes>`, when I run `careerbrew metrics time-saved`, then that action's baseline is labeled `measured`, and the minutes saved per local week are the baseline minus each run's time.
  - Given an unknown action, when I record a baseline, then careerbrew refuses it.
  - Given real runs on this machine, when I run `careerbrew metrics field`, then it writes this machine's totals (runs, success rate, errors, run time, time saved) to a local file under a salted-hash label; the host name never appears, and nothing is sent.
- **Metric**: measured baselines out of 8, PM update and standup runs with my own time (`time` `web`), and minutes saved per week (`careerbrew metrics time-saved --json`). **Target**: 8/8 measured; every PM update and standup run from the web app timed; minutes saved above 0 every week of real use.
- **Skill**: `skills/careerbrew-weekly-review`
- **Web views**: Dashboard, Standup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-86-time-saved.sh` | `cli.run` |

## US-87: Each task carries its expected and measured impact

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner who writes a daily update and a promotion doc, **I want** to record a task's expected impact when I start it and its measured result with evidence when I deliver it, **so that** impact is not rebuilt later from memory (issue #214).
- **Acceptance criteria**:
  - Given a task, when I run `careerbrew work impact <task> --expect ... --who ... --metric ...`, then the task keeps the hypothesis, who benefits, and the metric (test: `test_expected_then_measured_impact_is_kept_on_the_task`).
  - Given a delivered task, when I add `--result ... --evidence <link>` (or `--not-sure`), then the task keeps the result and links, a result that cannot be checked starts with "Not sure:", and promo evidence gets a note with the result and links (tests: `test_expected_then_measured_impact_is_kept_on_the_task`, `test_a_result_that_cannot_be_checked_is_marked_not_sure`).
  - Given a result recorded today, when the daily update is drafted, then its line says `impact: <result>` (test: `test_the_daily_update_names_todays_measured_impact`).
  - Given a shipped PR whose link is a task's evidence, when open questions are picked, then no "What was the impact of" question is asked about it (test: `test_an_impact_question_is_answered_from_the_record`).
  - Given a PM update line whose tasks have an expected impact, a checked result, or a ranked project, when the draft is shown (the Today view, `day send`), then a "why it matters" sentence for the owner sits beside it: who benefits, the expected change, the checked result with its evidence count (or the metric), and the rank; a line with none of these has no note (tests: `test_a_line_gets_a_why_note_from_the_tasks_expected_and_measured_impact`, `test_no_note_when_nothing_is_known`, `web/e2e/pm-copy.spec.ts`).
  - Given a saved PM edit, Today shows one impact note for each displayed deliverable in order. Reset restores the generated notes (test: `web/e2e/inputs.spec.ts`, issue #372).
  - Given a note, when the update is copied, posted to Slack, or opened as a mail draft, then the note is not in the text; an agent sees only how many notes there are (tests: `test_the_why_note_never_reaches_the_sent_or_copied_text`, `test_the_send_preview_shows_the_notes_apart_from_the_text`, `web/src/daylog.test.tsx`).
  - Given nothing to record or an unknown task, when I run it, then it is refused and names the story (test: `test_empty_or_unknown_impact_is_refused`).
- **Metric**: delivered tasks with a measured result. **Target**: every delivered task with expected impact.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none (the notes show in Today's PM update block, whose pictures US-59 owns)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-87-impact.sh` | `work.task` |

## US-89: Show the owner how careerbrew does in real use, with no content

- **Host contract**: dual-host-shared-memory
- **Mode**: platform
- **As** the owner, **I want** anonymous usage counts from real runs (command, ok or error, duration, counts) on PostHog EU, **so that** the README's headline numbers come from the field, not from tests.
- **Acceptance criteria**:
  - Given events in the log with business text, paths, or names in fields, when telemetry sends, then only the event name, time, a random install id, allowlisted ids, and numbers leave (tests: `test_only_allowlisted_fields_leave_the_machine`, `test_the_host_and_user_names_never_leave`).
  - Given no key, `DO_NOT_TRACK=1`, `careerbrew telemetry off`, or a plugin policy `telemetry: false`, when a send runs, then nothing is sent (tests: `test_no_key_sends_nothing`, `test_off_switches_send_nothing`, `test_a_plugin_policy_forbids_telemetry`).
  - Given a plugin host policy (`allowed_hosts`) that does not list the telemetry host, or a telemetry address that redirects to an unlisted host, when a send runs, then nothing is sent there (tests: `test_a_host_policy_without_the_telemetry_host_sends_nothing`, `test_a_telemetry_redirect_to_a_host_outside_the_policy_sends_nothing_there`).
  - Given a sent event, when the next send runs, then it is not sent again; a half-written line waits (test: `test_nothing_is_sent_twice_and_a_partial_line_waits`).
  - Given a dead or failing endpoint, when the watcher or `telemetry send` runs, then no other command fails and the events stay queued (tests: `test_a_failed_send_never_raises_and_keeps_the_events`, `test_an_unreachable_endpoint_never_fails_the_watcher`).
  - Given telemetry is on, when `careerbrew` starts the first time, then it says so in one line, and the watcher sends at most once a day after that (test: `test_the_watcher_sends_once_a_day_and_only_after_the_notice`).
  - Given `careerbrew telemetry send --dry-run`, then it prints the exact JSON with the key as a placeholder and sends nothing (test: `test_dry_run_prints_the_exact_body_without_the_key_and_sends_nothing`).
- **Metric**: requests with a canary, path, host, or user name in the body. **Target**: 0.
- **Skill**: `skills/careerbrew-privacy-guard`
- **Web views**: none (Actions lists `telemetry status`, `on`, `off`, and `send`)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-89-field-telemetry.sh` | `telemetry.send` |

## US-90: A review ask names enough people for the required approvals

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer who asks for reviews, **I want** each review ask to name at least as many people as the base branch requires approvals (never fewer than two), each with what to review, **so that** a PR that needs two approvals does not stall on one reviewer or a group handle (issue #270).
- **Acceptance criteria**:
  - Given a PR whose base branch requires two approvals (branch protection or a ruleset, the higher wins), when I run `careerbrew work review-ask <PR link>`, then the draft names two people, code owners of the changed paths first, and says which paths each one reviews (test: `test_a_repo_that_needs_two_approvals_gets_a_draft_with_two_named_people`).
  - Given a draft that names one person plus a group handle such as `@org/team`, when I run it with `--check <file>`, then it is refused and names the story; group handles and the PR's author never count (test: `test_a_draft_with_one_person_plus_a_group_handle_is_refused`).
  - Given a branch that requires no approvals, or rules this token cannot read, when the draft is built, then it still names two people (tests: `test_never_fewer_than_two_people_even_when_no_approval_is_required`, `test_recent_reviewers_fill_in_and_a_short_draft_says_how_many_are_missing`).
  - Given too few code owners, when the draft is built, then recent reviewers of the same folders fill in, and if people are still missing it is refused with how many more to add with `--reviewer` (test: `test_recent_reviewers_fill_in_and_a_short_draft_says_how_many_are_missing`).
  - Given an agent runs it, when the draft is built, then it sees counts only, never names or paths (test: `test_the_cli_prints_counts_to_an_agent_never_names`).
- **Metric**: review asks that name fewer people than the required approvals. **Target**: 0.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-90-review-ask.sh` | `work.review_ask` |

## US-91: See what my background agents do while they work

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose background agents review, fix, and refresh things for 20 to 30 minutes at a time, **I want** each agent to report every step as it works, and Work to show each agent's run by project with its last event and link, **so that** the web app is the current state of my day without waiting for a cycle to end or reading a terminal (issue #272).
- **Acceptance criteria**:
  - Given an agent at work, when it runs `careerbrew work agent event <agent> "<what it did>" [--status running|done|failed] [--project <name>] [--url <link>] [--run <id>]`, then one row is stored under `work/` (encrypted), `/api/changes` moves at once, and the event log gets the status only (tests: `test_one_event_moves_the_change_version_at_once_so_live_views_refetch`, `test_events_are_encrypted_at_rest_and_the_event_log_gets_the_status_only`).
  - Given an open Work view, when an agent reports a step, then Agent activity shows it within 10 s with no reload, in its project's group, with the event's title as a link, and `--status done` ends the run (tests: `web/e2e/agents.spec.ts`, `test_the_view_groups_runs_by_project_each_with_its_last_event_and_link`, `web/src/agents.test.tsx`).
  - Given a run whose last event is `running` and older than `agent_quiet_minutes` (default 60), when Work shows it, then it is `quiet`, never running forever; a `running` event after `done` starts the run again; one agent's parallel jobs keep apart by `--run` (tests: `test_a_running_run_that_goes_silent_shows_as_quiet_and_a_new_running_event_restarts_a_finished_run`, `test_parallel_runs_of_one_agent_keep_apart_by_run_id`).
  - Given a bad agent name, status, empty or over-200-character title, or a link that is not http(s), when an agent reports it, then it is refused with the fix, names the story, and writes nothing (tests: `test_bad_input_is_refused_names_the_fix_and_writes_nothing`, `test_a_bad_event_from_the_cli_names_the_story`).
  - Given an agent runs `work agent show`, when it reads the runs, then it sees counts, states, agents, and projects, never titles or links (test: `test_the_cli_reports_one_step_and_prints_titles_only_where_business_data_may_show`); rows older than `agent_keep_days` (default 7) are never shown and are dropped at the next write (test: `test_rows_older_than_agent_keep_days_are_dropped_and_never_shown`).
- **Metric**: seconds from an agent's `work agent event` to the row on an open Work view. **Target**: under 10.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-91-agent-activity.sh` | `work.agent.event` |

## US-94: Run my work day from one plan, with a row per step and one page

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** careerbrew to give the day's ordered steps (its own, mine, and each company plugin's), keep one status row per step, and write one page for the day, **so that** every work-day feature lives in careerbrew, the tool that owns my work record (issue #257).
- **Acceptance criteria**:
  - Given the `work_steps` setting and a plugin manifest with `work_steps:`, when I run `careerbrew day run --json`, then the plan lists careerbrew's steps, then mine, then the plugin's, each with where it came from, and a waiting row per step; a name declared earlier keeps its place (test: `test_the_plan_is_careerbrew_then_settings_then_plugins_first_declaration_wins`).
  - Given a step of today's plan, when `careerbrew day update <step> --status done --summary <line>` runs, then that row changes and the others stay; parallel updates never lose a row (tests: `test_update_writes_one_row_and_refuses_bad_input`, `test_parallel_updates_never_lose_a_row`).
  - Given a step not in the plan, a bad status, a non-http link, or a summary over 200 characters, when I update it, then it is refused and names the story (tests: `test_update_writes_one_row_and_refuses_bad_input`, `test_errors_name_the_story`).
  - Given rows with summaries and asks, when `careerbrew day render` runs, then the page shows done rows, blocked rows as blockers, and asks and waiting steps as next actions, with the row text on the page only, never in the agent summary or `day run --json` for an agent (tests: `test_the_page_puts_row_text_in_detail_only`, `test_day_run_json_hides_row_text_from_an_agent`).
- **Metric**: work days whose `/work` run ends with a rendered page. **Target**: every work day `/work` runs.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day run`, `day update`, and `day render`; the Work day view is issue #273)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-94-work-day.sh` | `workday.update` |

## US-96: My work day runs safe steps together and never lets two steps share a browser, a draft, or a checkout

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** each step to declare its order (`after`, `longRunning`), what it touches (`scope`), and the named resources it needs alone (`uses`), and the plan to put steps in stages from them, **so that** safe steps run at once and two steps never work on one sign-in browser, one Slack draft, one file, or one checkout at the same time (issue #258).
- **Acceptance criteria**:
  - Given an owner-supplied work-step JSON mapping, the saved plan retains its scope, resources and dependencies while plugin steps stay bound to their own sources (test: `test_owner_mapping_resolves_and_orders_external_step`).
  - Given two steps that share a `uses` entry, when the plan is built, then they land in different stages, and the later one's `why` names the shared entry; steps whose scopes overlap split the same way (tests: `test_two_steps_that_share_a_uses_entry_land_in_different_stages`, `test_overlapping_scopes_split_and_separate_ones_share`).
  - Given an independent `longRunning` step with declared resources, it starts after careerbrew and its own dependencies without waiting for unrelated checkups; finite work sharing its resources runs first, with the reason in `ready_when` and `why` (tests: `test_an_independent_long_running_step_starts_early_when_declared_first`, `test_long_loop_waits_for_finite_work_with_the_same_resource_and_explains_it`).
  - Given unrelated running work, `day check --json` lists a successor as ready once its own dependencies are done or blocked; failed dependencies keep it waiting. A ready batch never shares a named resource, overlapping scope, or unknown scope with running work (issue #308, `tests/unit/test_issue_308.py`).
  - Given an `operatorTurn` quick dispatcher, `day run --json` marks `operator_turn`, lists it in `start_of_day.operator_turn`, and defaults its timeout to 3 minutes. The skill runs it inline in the owner's initial turn before background waits; invalid booleans, long-running dispatchers, and background prerequisites are refused with repair guidance (issue #310, `tests/unit/test_issue_310.py`).
  - Given a step with `after`, when the plan is built, then it runs in a later stage than each step it names; a loop, or a step after a long-running one, is refused, and `day run` and `day update` name the story instead of writing a row (tests: `test_after_orders_stages_even_against_declared_order`, `test_impossible_orders_are_refused`, `test_day_update_names_the_story_when_the_steps_cannot_be_staged`).
  - Given a step that declares neither `scope` nor `uses` (every `work_steps` setting entry), when the plan is built, then it runs alone in its declared place, as in US-94 (tests: `test_a_step_that_declares_nothing_runs_alone_in_its_place`, `test_undeclared_steps_keep_the_us94_order_one_stage_each`).
  - Given any plan, when I run `careerbrew day run --json`, then each step shows its `stage` and `why`, and `stages` lists the steps of each stage (test: `test_the_plan_json_shows_each_steps_stage_and_why`).
  - Given a step that handled a checkout or PR (`careerbrew day update <step> --handled checkout:<path>`), when a later step reads the plan or claims the same item, then `handled` names the owner and a second claim is refused (test: `test_one_step_owns_a_checkout_or_pr_per_day`).
  - Given a project source another step checked after the stale list was read, when a refresh reaches it, then it is skipped, not read again (test: `test_refresh_skips_a_source_another_step_checked_since_the_list_was_read`).
- **Metric**: stages in the plan for a day of declared steps, against one stage per step. **Target**: the 6-step sample day runs in 4 stages, and no stage holds two steps that share a `uses` entry.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day run` and `day update`, which takes `--handled`)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-96-work-day-stages.sh` | `workday.update` |
## US-100: My work-day dashboard tells the truth: done is proven, a hung step is retried once, and blocked differs from failed

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** a step's `verify` command to prove it is done, a step past its timeout to get one retry and then fail with a class, and blocked rows to carry their question while failed rows carry what broke, **so that** the day's page never shows a step done on its own word, a hung step never holds the day, and I know which rows wait on me (issue #259).
- **Acceptance criteria**:
  - Given a step that declares `verify:`, when the runner marks it done, then the row is done only when the command exits 0; otherwise it is failed with class `tool`, and the command's output is never printed or stored (tests: `test_a_step_whose_verify_exits_non_zero_is_failed`, `test_a_step_whose_verify_passes_is_done`, `test_a_verify_that_hangs_fails_with_a_timeout`).
  - Given a running step past its `timeout:` (or the `work_step_timeout` setting), when `careerbrew day check` runs, then the row goes back to waiting for one retry, and after the retry also times out it is failed with class `tool` and a next action (tests: `test_a_step_past_its_timeout_is_retried_once_then_failed`, `test_the_default_timeout_is_a_setting`).
  - Given a runner that crashed mid-step, when the next pass reads the plan, then the step's running row, written at step start, shows `overdue` (test: `test_the_default_timeout_is_a_setting`).
  - Given an update to blocked without `--needs-you`, or to failed without `--error-class`, when it runs, then it is refused and writes nothing; each accepted row carries a next action (tests: `test_a_blocked_row_without_a_question_record_is_refused`, `test_a_failed_row_needs_an_error_class`).
  - Given a day with a blocked and a failed row, when the page is rendered, then it counts them apart and names the failed row's class (test: `test_the_page_tells_blocked_from_failed`).
- **Metric**: rows marked done whose verify fails, and running rows past their timeout that no pass flags. **Target**: 0 of each in the US-100 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day update`, which takes `--error-class`, and `day check`)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-100-work-day-truth.sh` | `workday.verify` |

## US-102: My work day starts on fresh checkouts, with the shell traps named and one sign-in ask

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** one start-of-day step that fast-forwards every checkout the steps read, names this machine's shell traps with their safe forms, and probes sign-in once, **so that** no step runs on a stale checkout, steps stop hitting the same shell traps, and I get one sign-in question early instead of a stalled step (issue #260).
- **Acceptance criteria**:
  - Given `careerbrew day run --json`, then the plan starts with `start_of_day` (the `careerbrew day start` command, its rule, the checkouts it refreshes, the watch list, the shell traps reference, the sign-in state, and today's questions), and each step lists its resolved `source` path and commit (tests: `test_the_plan_json_starts_with_the_refresh_and_preflight`, `test_the_plan_lists_each_steps_source_and_commit_and_watches_one_checkout_per_remote`).
  - Given a checkout behind its upstream, when `careerbrew day start` runs, then it is fast-forwarded before any step starts; a checkout with local changes, local commits, a diverged branch, no upstream, or a detached HEAD is reported and never touched (tests: `test_a_behind_checkout_is_fast_forwarded_before_stage_1`, `test_local_work_and_a_diverged_branch_are_reported_and_never_touched`).
  - Given two folders with one remote (a dev checkout and a managed clone a step's skill links into), then both are refreshed, the clone is a `duplicate_of` the dev checkout, and only the dev checkout is watched; the remote URL is never shown (test: `test_the_plan_lists_each_steps_source_and_commit_and_watches_one_checkout_per_remote`).
  - Given stale source checkouts, when `careerbrew day start --update-only --json` runs before the plan, then clean checkouts fast-forward, dependencies and the launcher are repaired, installed plugins and configured agent commands refresh, and changed instruction paths tell the agent what to re-read; dirty or diverged sources are preserved and reported (issue #330; tests: `tests/unit/test_issue_330.py`).
  - Given an old installed /work, a broken path outside a code block, a legacy Agentfile step absent from the plan, or a missing planned step file, then the start preflight names the failed check and its safe repair without removing owner edits (issue #305; tests: `tests/unit/test_issue_305.py`).
  - Given the preflight, then nonblocking shell, sed, available `python3`, and push-guard advice is a warning; missing `python3` and failed MCP or workflow checks remain failures. It names the shell, the sed flavor, the bash version, `python3`, and the push guard, each with its safe form, and every step loads one shared traps reference (`skills/careerbrew-work-day/shell-traps.md`); a step `verify` that starts with bare `python3` is refused (tests: `test_the_preflight_names_each_trap_and_its_safe_form`, `test_a_verify_with_a_bare_python_is_refused`).
  - Given steps that use a `browser:<name>` and a `work_signin_probe` that fails, when the day starts, then the day has one question record, operator-turn browser dispatchers stay blocked until sign-in succeeds, other browser steps (and steps `after` them) run after independent work, and the page asks it first; once the probe passes, the plan is as declared (tests: `test_a_failed_signin_probe_yields_one_question_and_moves_signin_steps_later`, `test_no_probe_and_no_signin_steps_ask_nothing`).
  - Given recorded session cron jobs, then the preflight warns when the host reports an active goal hook, and reports the hook state as unknown when the host supplies no fact (#339).
  - Given an empty launchd GitHub Authorization header in Claude, `day start --fix` refuses without explicit `--yes`; an approved repair preserves a symlink target, permissions and other settings, stays idempotent, and is reachable through the same confirmed Actions handler (`test_mcp_fix.py`, US-102 rehearsal). Withdrawing confirmation in the generated form leaves the synthetic settings unchanged; an approved browser request writes once and its repeat changes nothing (`web/e2e/mcp-repair.spec.ts`). Live authentication is verified separately after host reconnect.
- **Metric**: checkouts still behind their upstream when stage 1 starts, and sign-in questions per day. **Target**: 0 behind (checkouts with local work aside) and at most 1 sign-in question in the US-102 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: Actions
- **Web controls**: The `day.start` repair form is in Actions. Setup also lists the existing `work_checkouts` and `work_signin_probe` settings.

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-102-work-day-start.sh` | `workday.start` |

## US-104: Every work-day question is a record, rechecked before the one batched ask

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** each question a step raises saved as a record with its options, the agent's pick, and a recheck, and one batched ask that rechecks every item first, drops the closed ones, and shows each one's age, **so that** no step stops to ask me mid-run and I am never asked about a draft, a PR, or a sign-in that is already done (issue #261).
- **Acceptance criteria**:
  - Given a step's question, when the agent runs `careerbrew day question add <step> "<ask>" --option ... --pick ... --recheck ...`, then a record `{id, step, text, options, pick, createdAt, recheck}` is kept encrypted in the day's status file; the same ask twice is one record that keeps its first age; a pick that is not an option, a recheck a shell could not split, and a step outside today's plan are refused (tests: `test_a_question_is_a_record_stored_encrypted_with_the_days_work_data`, `test_the_same_question_twice_is_one_record_and_keeps_its_first_age`, `test_bad_records_are_refused_with_the_fix`).
  - Given open records, when `careerbrew day question recheck` runs, then each command recheck runs without a shell and its output is never read (exit 0 keeps the item, any other exit drops it), a link is left to the agent and never fetched, and the ask lists the rest with their age and the dropped ones with theirs (tests: `test_the_ask_drops_a_record_whose_recheck_says_closed_and_shows_each_age`, `test_a_recheck_command_runs_without_a_shell_and_its_output_is_never_read`).
  - Given a question from a plugin's step, then its recheck command runs only when the plugin was added with `--allow-exec`; otherwise the item stays in the ask as `unknown`, with that fix (test: `test_a_plugin_recheck_runs_only_when_the_plugin_may_run_commands`).
  - Given a blocked row's `--needs-you` and a failed sign-in probe, then each is a record in the same store, and clearing the row's ask or a passing probe closes it; every writer of the day file keeps the keys it does not own (tests: `test_a_blocked_row_is_a_question_record_and_clearing_it_closes_it`, `test_the_signin_question_lives_in_the_question_store`, `test_every_writer_of_the_day_file_keeps_the_keys_it_does_not_own`).
  - Given an open record at the end of the day, then the next morning's list and recheck show it, and it closes in its own day's file; the handoff points at the open questions (test: `test_an_open_question_carries_to_the_next_morning_and_closes_in_its_own_day`).
  - Given the day plan and the work-day skill, then both say a step never asks the user itself and name `day question add` and `day question recheck`; the page lists each saved question with its age, its text in the detail only; question text shows only when business data may show (tests: `test_the_step_contract_says_return_questions_never_ask`, `test_the_page_lists_each_open_question_with_its_age`, `test_text_shows_only_when_business_data_may_show`).
- **Metric**: closed items asked in the batched ask, and items asked without an age. **Target**: 0 and 0 in the US-104 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day question add`, `list`, `recheck`, and `close`; `recheck` asks to confirm)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-104-question-records.sh` | `workday.recheck` |

## US-105: My work day shows how long each step took, flags a slow one, and keeps a record across days

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs the work day, **I want** each step's row to keep when it ended and how long it took, a step slower than its usual time flagged, and a record across days, **so that** I see that a live cycle took 18 minutes, and can answer "how did the work day go this week?" and "which step fails most?" (issue #263).
- **Acceptance criteria**:
  - Given a step marked running and later done, blocked, or failed, then its row has `ended` and `seconds` (from the `started` stamp of #259); a new start clears them, a summary-only update keeps them, and a step that never ran has an end but no duration (tests: `test_a_finished_step_records_its_end_and_duration`, `test_a_summary_update_after_done_keeps_the_duration`, `test_a_step_that_never_ran_has_an_end_but_no_duration`, `test_a_timed_out_step_is_recorded_as_failed`).
  - Given finished steps on several days, then `day/work-history.json` (encrypted) keeps each step's final status and duration per day, no row text, and only the last 120 days; a history file nobody can read never breaks the day (tests: `test_the_history_keeps_duration_and_final_status_per_step_per_day`, `test_the_history_keeps_a_bounded_number_of_days`, `test_a_corrupt_history_never_breaks_the_work_day`).
  - Given a step whose done runs on 3 or more earlier days have a median, when it takes more than `work_slow_percent` (200) percent of it, then its row is `slow`; with fewer earlier runs, no flag (tests: `test_a_slow_step_is_flagged_against_its_median`, `test_no_flag_before_enough_history`).
  - Given a finished day, when `careerbrew day render` runs, then `careerbrew report latest` lists it with each step's duration, the slow count, and the total time, and no row text (tests: `test_a_finished_day_is_listed_by_report_latest`, `test_the_page_without_history_still_renders`).
  - Given `careerbrew day history [--days N] [--json]`, then it lists each step's runs, done, blocked, failed, and median, the step that fails most first, and each day's totals; an agent sees a plugin's step as `plugin:<name> step <n>` (tests: `test_day_history_answers_how_the_week_went`, `test_day_history_hides_plugin_step_names_from_agents`).
- **Metric**: ended rows without a duration although they started, and slow steps the page does not flag. **Target**: 0 of each in the US-105 sample week.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day history`; Setup lists `work_slow_percent`; the Reports view shows the work-day page)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-105-work-day-timing.sh` | `workday.update` |

## US-93: A prep project is done only when I am ready for the event

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer preparing for an event (an interview to run or take, a meeting, a demo, a talk), **I want** a prep project to count as done only after I tried the tool, read every source link, wrote my own questions, and passed an agent quiz, **so that** "done" means I am ready, not that a prep pack exists.
- **Acceptance criteria**:
  - Given `work project add <name> --kind prep [--event-on YYYY-MM-DD]`, when it is added, then it has four open gates (tool, links, questions, quiz), each with the command that closes it; a plain project has none, and a bad kind or day, or an event day on a plain project, is refused with the fix (tests: `test_a_prep_project_starts_with_four_open_gates_and_a_plain_project_has_none`, `test_kind_and_event_day_are_checked_and_set_can_change_them`).
  - Given a prep project, when I run `work project gate <name> tool|questions`, `work project gate <name> links <ref>...` for each source I read, and `work project quiz <name> --asked N --missed K`, then each gate passes on its own fact: links only when I marked every source but the memory file read (a `check` stamp, which the watcher also sets, does not count), quiz only when the latest quiz had no miss; the quiz is never marked by hand, an unknown link is refused, and `--undo` opens a gate again (tests: `test_the_project_is_ready_only_when_all_four_gates_pass`, `test_a_check_stamp_is_not_the_owner_reading_a_link`, `test_links_name_each_ref_and_an_unknown_ref_is_refused`, `test_gates_are_refused_on_a_plain_project_and_the_quiz_is_not_marked_by_hand`).
  - Given a quiz with misses, when I store the next quiz, then it must ask at least that many questions (each miss again), the gate stays open until a quiz comes back clean, and only counts are stored: time, asked, missed, never a question or an answer (tests: `test_misses_keep_the_quiz_gate_open_until_a_re_quiz_of_every_miss_is_clean`, `test_the_quiz_result_is_content_free`).
  - Given open gates, when I run `work project archive <name>`, then it is refused and names the open gates and the next fix; `--force` archives it when the event was cancelled (tests: `test_the_project_is_ready_only_when_all_four_gates_pass`, `test_a_cancelled_event_can_still_be_archived_with_force`).
  - Given the event's day or the workday before (Friday for a Monday event), when the day or week is planned, then the plan starts with the open gates (before study blocks and tasks), they name their project but no link, `day next` points at the first open one that fits the free time, and they stay out of the PM update (tests: `test_the_event_day_plan_starts_with_the_open_gates_and_other_days_do_not`, `test_the_next_step_is_an_open_gate_before_any_task`, `test_a_monday_event_gets_its_gates_on_the_workday_before`, `test_the_next_step_skips_a_gate_that_does_not_fit_before_the_next_meeting`, `test_gate_blocks_stay_out_of_the_pm_update`).
  - Given the Work view, when a watched project is a prep project, then its card shows "Ready" or "Not ready", the gates passed, the event day, and each open gate with its command (test: `web/src/views.test.tsx`).
- **Metric**: prep projects archived with an open gate and no `--force`. **Target**: 0.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Work

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-93-prep-readiness.sh` | `work.prep` |

## US-95: My standup carries only real updates, each backed by a check made right before it

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer who gives a standup update from a board, **I want** rows with nothing new left out, and each row I keep backed by a live check made right before the meeting, **so that** I never repeat a stale or cached claim as news.
- **Acceptance criteria**:
  - Given rows passed with `work standup --rows`, when the report builds, then a row shows only when it has an update in the window (my work matched it, an agent passed an action or a talking point for it, or its live proof passed); a row with no update is hidden, mine too and one done before the window, and the page, the view, and the command say how many in one line ("2 rows with no update hidden") (issue #271; tests: `test_a_row_with_no_update_is_hidden_yours_too_and_one_done_before_the_window`, `test_an_agent_talking_point_or_extra_action_is_an_update`).
  - Given an agent passes `work standup --proof <file|->` (`{row title: {verdict, proofs: [{source, url, checked_at, quote}], reason}}`), when the report builds, then a row shows only when its verdict is `pass`, it has at least one proof, every proof has a source, an http(s) URL, and a quote, and every `checked_at` is after the window start, no older than `standup_proof_minutes` (default 60) before the build, and not in the future; the gate runs at every build, so a proof goes stale with time; a bad file names the fix and writes nothing (tests: `test_with_proofs_only_rows_that_pass_a_fresh_check_show_with_each_proof_link`, `test_a_row_that_fails_the_check_stays_off_the_page_and_says_why`, `test_a_proof_goes_stale_as_time_passes_and_the_limit_is_a_setting`, `test_a_bad_proof_file_is_refused_names_the_fix_and_writes_nothing`).
  - Given proofs, when I open the page or the Standup view, then each proof link is listed under its row ("Checked <when> in <source>: <quote>"); a failed row and its reason never reach the page or the view: the command prints `failed row N: <reasons>`, and the row's title and the agent's reason only with `--show` to my terminal; the proofs are kept encrypted for the current standup window only (tests: `test_cli_prints_failed_rows_with_reasons_but_titles_only_with_show`, `test_the_web_view_never_carries_failed_rows`, `test_proofs_are_kept_encrypted_for_this_window_only`).
  - Given a plugin that publishes on a schedule, when no row passed its check, then `--rows []` clears the rows and the page and the view say "No row passed the proof check"; the report window line says when the rows were last passed ("updated 09:40", with the day when it was not today) (tests: `test_an_empty_row_list_clears_the_rows_and_the_page_says_no_row_passed`, `test_the_report_says_when_the_rows_were_last_passed`).
  - Given the work-companion skill builds a standup, when it has talking points, then before the build it collects live proofs in this run (GitHub, chat, the tracker) and a fresh-context subagent grades each row claim by claim against those proofs only, then it passes them with `--proof` (eval: `work-companion-standup-judges-rows-against-live-proofs`).
- **Metric**: rows on the page with no update, a failed check, or a stale proof, in the US-95 fixture (`us-95-standup-proofs.sh`). **Target**: 0, with 1 of 1 freshly proved rows shown with its proof link.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Standup

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-95-standup-proofs.sh` | `work.standup`, `work.standup.notes` |
| web app view | `web/e2e/standup.spec.ts` (run by `tests/e2e/us-35-web-app.sh`) | `web.api` |

## US-97: Run the live work feed in core, with my company's sources plugged in

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose agent watches chat, tickets, the code host, mail, calendar, and paging all day, **I want** the live-feed cycle (pull, store, triage, a verdict per event, actions, mark) to live in careerbrew with source adapters, **so that** a company plugin holds only its hosts, channels, and roster, and a source with missing, invalid or stale acquisition fails the cycle instead of reading as quiet.
- **Acceptance criteria**:
  - Given a source config (`work live start --sources <file>`, no plugin), when I start the feed, then it lists each source with its kind, required flag, adapter, and cursor; a bad source (kind, name, missing inbox or connector, unknown key) is refused with the fix and saves nothing; pull hosts and channels show only where business data may show (tests: `test_a_bad_source_names_the_fix_and_saves_nothing`, `test_the_fixture_runs_without_a_plugin`).
  - Given a running feed, when a cycle runs, then it stores each fresh raw reply as returned, triages it by the feed's rules, gives each stored event one verdict (answer, done, read, skip, evidence, log, personal), and lists next actions; only my own non-personal evidence reaches the PM draft (tests: `test_each_stored_event_gets_one_verdict_by_the_rules`, `test_the_pm_rule_needs_your_own_non_personal_evidence`).
  - Given a required source with no healthy acquisition (none saved, invalid, beyond its inclusive limit, or an adapter error), when the cycle ends, then `not_stored` names it, the cycle fails (exit 1), cursors move only for sources that stored, and an empty supported reply counts as a quiet source. An unchanged validated reply within its acquisition limit passes without a new pull/store, cursor movement, followup or age renewal (tests: `test_408_02_fresh_unchanged_input_has_no_false_failure`, `test_408_03_exact_deadline_is_inclusive_without_renewal`, `test_408_04_expired_validation_cannot_be_renewed_by_poll`, `test_a_cycle_that_stores_only_chat_returns_not_stored_with_the_other_required_source`, `test_a_stale_inbox_is_not_a_fresh_pull`, `test_every_required_source_stored_passes_and_a_quiet_reply_counts_as_stored`, `test_cursors_move_only_for_sources_that_stored`, `test_a_broken_inbox_fails_only_its_source_and_names_the_fix`, `test_a_connector_source_that_is_not_connected_fails_its_source`).
  - Given a cycle already running, when a second one starts, then it fails at once; given a stopped feed, then a cycle is refused and the cursors are kept (tests: `test_one_cycle_at_a_time`, `test_stop_refuses_cycles_and_keeps_cursors`).
  - Given a plugin with `live: {sources, team}`, when it is installed, then its sources join (the owner's source of the same name wins) and its roster marks teammates' events, read only; the manifest refuses a live file outside the plugin (tests: `test_a_plugin_supplies_only_its_sources_and_roster`, `test_the_manifest_checks_live_files`).
  - Given any cycle, then the cycle record, the state, and the event log hold ids and counts only, never message text (test: `test_the_cycle_record_holds_no_message_text`).
  - Given complete current direct coverage, status reports healthy direct mode with adapters stopped. A later running adapter required failure overrides old direct success; only newer complete validation restores health. Missing, ambiguous or reversed clocks cannot certify health. `/api/feed/health` reads state only and returns fixed repair commands (#406, `test_issue_406.py`).
- **Metric**: fresh unchanged cycle/loop false failures and stale/invalid required accepts. **Target**: 0 for each.
- **Skill**: `skills/careerbrew-live-feed`
- **Web views**: none

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-97-live-feed.sh` | `work.live.cycle` |

## US-98: A reply draft fits the thread's newest state and is saved safely

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent drafts my chat replies, **I want** every draft judged on the thread's newest message and saved through one helper that reads it back, **so that** I never send a change request after a release started, a text reply where an emoji fits, or a draft the chat tool silently kept from before (issue #269).
- **Acceptance criteria**:
  - Given a thread whose newest message says a deploy or release started, when `careerbrew draft fit --thread <file> --draft <file>` runs, then the result is an approve or ack draft, never a change request; a draft that already approves is kept; with `--major "<one line>"` it is a stop with that one-line reason; a planned or asked-about deploy, or one rolled back since, is not a started one (tests: `test_a_thread_whose_newest_message_says_a_deploy_started_yields_an_ack_not_a_change_request`, `test_an_approving_draft_is_kept_after_a_deploy_started`, `test_a_major_issue_stops_the_release_in_one_line_and_a_long_reason_is_refused`, `test_a_planned_or_asked_about_deploy_is_not_a_started_one`, `test_a_rollback_after_the_start_ends_it_and_a_new_start_counts_again`).
  - Given a newest message that is a small acknowledgement (a one-word thanks, "ok", "done"), when the draft is fit, then it is an emoji suggestion, not a text draft (tests: `test_a_one_word_thanks_yields_an_emoji_suggestion`, `test_a_small_ack_draft_becomes_an_emoji_and_a_long_message_is_not_small`).
  - Given a draft written against an older message (`--seen <ts>`), when it is fit, then the result counts the newer messages and judges the newest state (test: `test_the_newest_message_decides_and_an_older_draft_is_stale`).
  - Given a draft, when `careerbrew draft replace <target> --file <file>` runs, then it clears the old draft, checks it is gone, saves, reads it back, and fails (exit 1) on a mismatch; `careerbrew draft check --sent <file> --read-back <file>` does the same comparison for a draft saved in a chat tool (tests: `test_a_read_back_mismatch_fails_the_save`, `test_an_old_draft_that_survives_clearing_fails_before_saving`, `test_cli_check_fails_on_a_mismatch_and_errors_name_the_story`).
  - Given two steps that write one target, when they save at once, then one waits and a second writer is refused after the wait with the `uses: slack-draft:<target>` fix; the work-day runner keeps such steps in different stages (issue #258) (tests: `test_a_second_writer_of_one_target_waits_then_is_refused_with_the_uses_fix`, `test_parallel_replaces_of_one_target_never_interleave`).
  - Given thread and draft text, when any draft command runs, then neither reaches stdout or the event log, and the saved draft is encrypted at rest (test: `test_cli_fit_and_replace_print_no_thread_or_draft_text`).
  - Given an access ask (a missing permission, policy, role, or group) whose topic an open thread in the owning channel already asks, when `careerbrew draft route --ask <file> --directory <file>` runs, then it links that thread instead of opening a new one; and when `careerbrew draft judge` checks a draft that mentions a non-member, sits in a channel that does not own the topic, or opens a duplicate thread, then it exits 1 with each reason (issue #314) (tests: `test_an_access_ask_with_an_existing_thread_links_it_instead_of_opening_a_new_one`, `test_a_draft_that_mentions_a_non_member_fails_the_judge_step`).
- **Metric**: drafts that ask for changes after a release started, or that fail a read-back and still get sent. **Target**: 0.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `draft fit`, `draft replace`, `draft check`, `draft route`, and `draft judge` as terminal-only: they read files on your machine)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-98-reply-drafts.sh` | `draft.replace` |

## US-99: Promises I make in my messages are tracked until I keep them

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer who makes commitments in messages all day ("I'll look into it", "I'll send it tomorrow"), **I want** each promise in my own sent messages found and tracked as a project with its link, who I made it to, and its due day, worked by the work-day cycle, and shown until I close it, **so that** no promise gets lost (issue #268).
- **Acceptance criteria**:
  - Given my own sent message "I'll look into the timeout" in the work feed, when it is stored, then a project of kind `promise` is tracked with the promised action, the message link (a reference: never read, never stale), who it was made to (the last other person in the thread, else the channel), and a due day when one is stated ("tomorrow", "by Friday", "next week", a date); a promise on a known project is a task under it; the feed's analysis still holds no message text (tests: `test_a_sent_message_i_ll_look_into_the_timeout_creates_a_tracked_promise_with_its_link`, `test_a_first_person_future_action_is_a_promise_with_its_due_day`, `test_a_promise_on_a_known_project_is_a_task_under_it`).
  - Given someone else's message, a personal one, or text that only sounds like a promise ("let me know", "I'll be out", "I can review it" with no deadline, a question), when it is stored, then nothing is tracked (tests: `test_other_text_is_not_a_promise`, `test_only_my_own_non_personal_sent_messages_hold_promises`).
  - Given an open promise, when the handoff (`day handoff show`) or Dashboard's Commitments block is shown, then it is listed, overdue first, with its link; when I close it (`work promise done`, `dropped`, `day handoff done <name>`, or Done and Not a promise in the web app), then it leaves both at once and the next pull never tracks it again (tests: `test_closing_a_promise_removes_it_from_commitments_and_the_handoff`, `test_a_promise_is_tracked_once_and_a_closed_one_never_comes_back`, `test_handoff_done_on_a_promise_closes_it`, `web/e2e/promises.spec.ts`).
  - Given open promises, when the work-day cycle runs (`/careerbrew`), then `work promise list --json` names each one's next step (research, draft the follow-up, report status), an agent per promise works it in parallel, and `work promise step` records each step (test: `test_the_cycle_steps_research_draft_status_and_an_overdue_promise_comes_first`).
  - Given an agent without the business-read policy, when it lists promises or the handoff, then it sees names, due days, links, and next steps, never the promised action or the person (test: `test_an_agent_sees_names_and_dates_but_no_text`).
- **Metric**: open promises past their due day. **Target**: 0 on a typical work day.
- **Skill**: `skills/careerbrew-work-companion`
- **Web views**: Dashboard

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-99-promises.sh` | `work.promise` |

## US-101: My live feed listens all day and does the follow-up work as drafts

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an engineer whose agent runs the live feed from the start of my day until wrap-up, **I want** every cycle to do the follow-up work for each new event (a reply draft, research for each link, review notes, the project update, one line per goal), as drafts only, **so that** I read and send instead of reading every message, opening every link, and writing every reply myself (issue #266).
- **Acceptance criteria**:
  - Given a cycle with a direct ask, when it runs, then a reply draft is saved at the ask's target through the safe save, judged on the thread's newest state (an emoji suggestion for a small ack, no text draft); a target that already holds a draft keeps it; each new event is followed up once (tests: `test_a_cycle_with_a_direct_ask_stores_a_reply_draft`, `test_a_small_ack_gets_an_emoji_suggestion_not_a_draft`, `test_a_new_event_is_followed_up_once_and_an_existing_draft_is_kept`).
  - Given a cycle with a link, when it runs, then each link of a kept event has a one-line summary or `opened: false` with the reason; links open only with `live_open_links` on (default off: the reason says so and the link stays pending for the agent); never a private or local address, a project reference, or more than 1 MB; with reading off the reason says so (tests: `test_a_cycle_with_a_link_stores_research_or_a_reason`, `test_with_reading_off_a_link_gets_a_reason_and_stays_for_the_agent`, `test_with_live_open_links_off_nothing_is_fetched`, `test_with_live_open_links_on_a_private_or_loopback_link_is_skipped`, `test_a_large_page_is_cut_at_the_size_cap`, `test_a_link_marked_as_a_project_reference_is_not_opened`, `test_an_http_protocol_error_while_reading_a_link_is_a_reason`).
  - Given a review asked of me, when the cycle runs, then review notes are drafted with the link and a checklist, never posted, and an ask already answered gets none; given a project whose source matches a link, then the event is attached to it and the change recorded, one update per project per cycle (tests: `test_a_review_asked_of_you_gets_review_notes_drafted`, `test_an_answered_ask_gets_no_review_notes`, `test_an_event_is_attached_to_its_project_and_the_change_recorded`, `test_one_project_update_per_project_per_cycle`).
  - Given my goals for today, when the cycle reports, then it has one update line per goal: what moved, other updates, and what waits on me or on whom; a past goal has no line (tests: `test_the_cycle_report_has_one_update_line_per_goal`, `test_a_past_goal_gets_no_line_and_no_goals_means_no_lines`).
  - Given any cycle, then each new event shows its class (needs you, worth knowing, noise, personal) and the action taken; nothing is sent; one bad event fails alone and the cycle is still marked (tests: `test_a_poison_event_between_two_good_ones_fails_alone_and_a_repull_adds_nothing`, `test_an_error_in_follow_up_work_never_stops_the_cycle_mark`); the owner's draft is checked under the draft's lock (test: `test_replace_if_empty_keeps_the_owners_draft_under_the_lock`); stdout to an agent and the cycle record hold no message text, and `draft show` prints a draft's text only where business data may show (tests: `test_each_new_event_gets_a_class_and_the_action_taken`, `test_the_cycle_never_sends_and_keeps_text_out_of_the_record_and_stdout`, `test_draft_show_prints_the_text_only_where_business_data_may_show`).
- **Metric**: open asks after a cycle with no reply draft, and links in kept events with neither research nor a reason. **Target**: 0.
- **Skill**: `skills/careerbrew-live-feed`
- **Web views**: none (Actions lists `draft show`)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-101-live-listening.sh` | `work.live.listen` |

## US-103: One work-day watch loop runs every watch on one measured clock, and none outlives the day

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent watches my work day, **I want** one watch loop that runs the live feed and each step's cycle under one lock, at an interval measured from real cycles, with every loop recorded, **so that** cycles never pile up or overlap, I never re-arm a timer by hand, and nothing keeps running after end of day (issue #264).
- **Acceptance criteria**:
  - Given a pass running, when another `careerbrew day loop pass` starts, then it exits at once with `skipped: previous cycle running`, runs no step, and is not measured (tests: `test_a_pass_that_starts_while_one_runs_is_skipped`, `test_a_skipped_pass_is_not_measured`).
  - Given measured passes, when the next pass is armed, then the interval is 1.2 times the median of the last 10 passes (60 s to 1 h; 300 s before the first), and the pass names its next re-arm time; `day loop run` waits that interval between passes and stops within 5 s of end of day (tests: `test_the_interval_is_about_1_2_times_the_median_cycle_time`, `test_each_pass_measures_itself_and_names_the_next_re_arm_time`, `test_the_run_loop_re_arms_itself_and_ends_with_the_day`, `test_a_pass_after_end_of_day_restarts_the_loop_but_the_run_loop_does_not`).
  - Given steps with `cycle:` and `watch:` inputs (`repo:`, `feed:`, `file:`), when a pass runs, then it runs `work live cycle` when the feed runs, and each step's cycle only when an input changed (every pass without inputs), with `CAREERBREW_WATCH_PHASE=skip` and its output discarded; a plugin's cycle runs only after `--allow-exec` (tests: `test_a_step_with_watch_inputs_runs_only_when_an_input_changed`, `test_a_feed_input_follows_the_live_feed_cursor`, `test_the_pass_runs_the_live_cycle_and_reports_a_running_one_as_skipped`, `test_a_cycle_runs_with_the_steps_own_watch_phase_off_and_its_output_goes_nowhere`, `test_a_plugin_cycle_runs_only_after_allow_exec`).
  - Given recorded loops (the watch loop, a running live feed, and a running day watcher by themselves, a cron, monitor, agent, or session by `day loop add`), when `day loop end` runs, then careerbrew stops its own, names each other one with its stop command (use `careerbrew day loop stop --id <id>` after stopping a tool-owned loop), and exits 0 only when none is left; a loop left from an earlier day shows as stale, and careerbrew's own is replaced by today's record (tests: `test_end_of_day_stops_each_loop_and_the_list_is_empty`, `test_a_loop_left_from_an_earlier_day_shows_as_stale`, `test_the_day_watcher_is_recorded_and_stopped_at_end_of_day`, `test_careerbrews_own_loop_from_an_earlier_day_is_replaced_not_stale`).
  - Given a pass with no declared coverage or a required acquisition gap, text and JSON exit 1 while retaining step outcomes and next re-arm time. A stopped adapter creates no running live-feed loop; valid direct coverage can pass without starting adapters (#406, `test_issue_406.py`).
- **Metric**: overlapping cycles, and recorded loops still running after `day loop end` exits 0. **Target**: 0 of each in the US-103 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day loop` as terminal-only: `run` keeps going until end of day)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-103-work-day-watch.sh` | `workday.loop` |

## US-106: My work day ends by clock, with careerbrew's PM draft and a handoff the next morning rechecks

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs my work day, **I want** the day to switch to wrap-up shortly before `work_end` and to end by itself at `work_end`, with careerbrew's own PM draft as the end-of-day output and tomorrow's handoff written from what is still open, **so that** I never have to type "end of day", no agent writes a second Today/Tomorrow report, and the next morning starts from what is really still open (issue #265).
- **Acceptance criteria**:
  - Given `work_end` and `work_wrap_minutes` (default 30), when the first watch pass at or after `work_end` minus the lead time runs, then the day switches to wrap-up once: the loops keep running, tomorrow's handoff is written, and one reminder pops up through the notifier (only with `notify_events` on; never a second one that day); at `work_end` the first pass ends the day by itself (tests: `test_wrap_up_starts_at_work_end_minus_the_lead_time`, `test_the_lead_time_is_a_setting`, `test_wrap_up_reminds_once_through_the_notifier`, `test_no_pop_up_when_notifications_are_off`, `test_a_pass_in_wrap_up_keeps_the_cycles_running`, `test_a_pass_at_work_end_ends_the_day_and_the_run_loop_stops`, `test_plan_names_the_wrap_up_time_and_phase`).
  - Given the day already ended, or two watch passes at `work_end`, when end of day runs again, then nothing changes: it claims the day's `end` record under the day file's lock first, and the clock check runs inside the pass lock. A Mac asleep through midnight skips that day's end: the clock ends a day only on a pass that day (tests: `test_a_second_end_of_day_changes_nothing_and_returns_the_first`, `test_the_clock_check_runs_inside_the_pass_lock`, `test_two_writers_of_one_handoff_keep_both_items`).
  - Given a work day, when `careerbrew day end` runs (or the clock ends the day), then its output is careerbrew's PM draft (the owner's edit wins) with the Dashboard link, it writes no report of its own, it stops careerbrew's loops, and it keeps every key of the day file it does not own (tests: `test_end_of_day_output_is_the_pm_draft_not_a_new_report`, `test_the_pm_draft_the_owner_edited_is_the_one_shown`, `test_day_end_cli_prints_counts_not_the_draft_for_an_agent`, `test_end_of_day_stops_the_loops`, `test_wrap_up_keeps_the_rows_and_start_record_of_the_day_file`).
  - Given open PRs (a row's link, a step's handled `pr:` item) and open question records, when the handoff is written, then each is an item with its recheck (a PR's fails once it merged or closed); the next `careerbrew day start` reruns each recheck, shows each item's result, closes the done ones, and drops them from the handoff (tests: `test_end_of_day_writes_open_prs_and_open_questions_into_the_handoff`, `test_ending_twice_keeps_one_item_each_and_drops_a_question_closed_since`, `test_next_morning_shows_each_handoff_item_with_its_recheck_result`, `test_day_start_records_and_prints_the_handoff_recheck`, `test_morning_with_no_handoff_is_empty`, `test_a_pr_recheck_closes_only_on_a_merged_or_closed_answer`, `test_a_second_day_start_does_not_fall_back_to_an_older_handoff`); a PR's recheck closes it only on a MERGED or CLOSED answer, so a missing or offline `gh` keeps it open.
  - Given an agent turn after a stored loop or wrap-up is more than 5 minutes late, then `day loop catch-up` claims and runs that due event once, including concurrent turns (#339).
  - Given `day wrap --at HH:MM` or `--hold`, then today’s automatic wrap-up and end move together within the same day or stay held; a move crossing midnight is rejected with a hold-and-explicit-end repair. `--at` resumes a hold. An explicit owner `day end` ends now even while held (#339). `day check` shows when the automatic end is paused (#397).
  - Given active project sessions at wrap-up, then each gets a graceful request; an old checkpoint cannot archive the session, and a handoff written after its request can. Repeated end calls still report pending sessions (#339).
- **Metric**: end-of-day report files besides the PM draft and the day's page, reminders per day, and handoff items without a recheck result the next morning. **Target**: 0 extra files, exactly 1 reminder, 0 items without a result in the US-106 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: none (Actions lists `day end`; the PM draft it reads is edited and sent on the Dashboard, US-35)

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-106-end-of-day.sh` | `workday.end`, `workday.morning` |

## US-107: Each pressing project gets one long-lived session that talks through careerbrew

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** an owner whose agent runs my work day, **I want** the main session to run only the work process and each pressing project to get one session, reused all day, that writes its steps, project updates, questions, and claims to careerbrew and ends with its project's handoff, **so that** the main session's context stays small, two sessions never write the same draft, and I get one dashboard and one ask, not one per session (issue #274).
- **Acceptance criteria**:
  - Given standing rules on a project, the session contract instructs each pass to read the current snapshot and block that project's pass if policy refuses; restricted output hides their text (#438; `test_issue_438.py`, `us-107-project-sessions.sh`, `026-project-sessions.yaml`). Agent compliance is evaluated separately.
  - Given a project, when the agent runs `careerbrew work session open <project> --id <id>`, then one session is recorded (and recorded as a `session` loop that `day loop end` checks); later that day `open <project>` returns the same session, a second id for the project and a session past `work_session_cap` are refused with the fix, and a session from an earlier day is stale, never reused (tests: `test_a_project_gets_one_session_reused_all_day`, `test_a_session_past_the_cap_is_refused_with_the_fix`, `test_opening_records_a_session_loop_that_day_loop_end_checks`, `test_a_session_from_an_earlier_day_is_stale_and_not_reused`).
  - Given a project session with a handoff yesterday, when a new session opens today, its brief includes that handoff only where business output is allowed; a newer session without a handoff does not revive an older brief (tests: `test_issue_432.py`, `us-107-project-sessions.sh`). This does not schedule sessions or certify completion.
  - Given a live session, when it reports with `work agent event --session <id>`, then the run is the session and the project the session's, Work's Agent activity groups it under the project, and `careerbrew work session list` shows each session's state, last event, claims, open questions, and handoff (tests: `test_a_sessions_steps_are_agent_events_under_its_project`, `test_the_list_is_the_main_sessions_one_view`).
  - Given a session's claim, when another session runs `work claim take` or `draft replace` on the claimed target (or any writer with no session, the live cycle too), then it is refused with exit 1 and skips the target; the holder renews it, and a claim ends on release, when its session ends, or `work_claim_minutes` after it was taken (tests: `test_a_claimed_draft_is_skipped_by_another_session`, `test_the_live_cycle_skips_a_claimed_target`, `test_a_claim_expires_and_ends_with_its_session`, `test_bad_claims_are_refused_with_the_fix`).
  - Given questions from several sessions, when the main session runs `careerbrew day question recheck`, then every session's open asks are in the one batched ask, closed ones are dropped, and the day's page lists each with its age; a session's ask keeps the day file's other keys (tests: `test_session_questions_land_in_the_one_batched_ask`, `test_a_session_ask_keeps_the_day_files_other_keys`, `test_the_work_day_page_lists_a_sessions_question`).
  - Given the end of the day, when `careerbrew work session end --all` runs, then each session that wrote its handoff (`work session handoff`, one "Project sessions" item per project) is archived and its claims released, and one without a handoff stays open with exit 1 (tests: `test_end_needs_a_handoff_then_archives_and_releases_claims`, `test_one_handoff_item_per_project`).
  - Given three future bullets in a sent PM update under Tomorrow, a named Tomorrow heading, or Next week, then three idempotent promises are due next workday, `pm_promises` marks their project session coverage (including unmatched promises), a missing sent update is explicit, and the next draft names each recorded outcome; restricted output keeps their text private (#331; `test_issue_331.py`, `us-107-project-sessions.sh`).
  - Given a project brief that names only planning, the emitted session contract still requires full ticket scope, ordered delivery, verification, batched approvals, and itemized completion before the next task (#333; `test_issue_333.py`, `us-107-project-sessions.sh`).
  - Given a project quiet until a future date, new session opens are refused, new owner questions wait, matched notifications are held, and existing questions stay open. On the date it resumes from its prior handoff; `none` clears the date (tests: `test_issue_409.py`, `us-107-project-sessions.sh`).
  - Given the day plan, the work-day skill, and `/work`, then they say the main session runs only the work process and name the session commands; project names, handoffs, and claimed resources show only when business data may show (tests: `test_the_contract_says_main_runs_only_the_process_and_sessions_write_through_the_store`, `test_projects_handoffs_and_claims_show_only_when_business_may_show`).
- **Metric**: writes to a claimed draft by another session, live sessions over `work_session_cap`, and sessions archived without a handoff. **Target**: 0, 0, and 0 in the US-107 sample day.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: Actions

| Case | Test | Log event |
|---|---|---|
| happy and error | `tests/e2e/us-107-project-sessions.sh` | `work.session` |

## US-110: Review outcomes of sent PM promises

- **Host contract**: dual-host-shared-memory
- **Mode**: job helper
- **As** the owner, **I want** to review proposed outcomes for promises in sent PM updates, **so that** a moved promise keeps its identity and my decision controls its status.
- **Acceptance criteria**:
  - Given a repeated Tomorrow item and an exact sent receipt, when outcomes are reviewed, then show a moved proposal without creating another promise. Preserve ambiguous items and refuse missing or stale receipt bindings.
  - Given a current proposal, when the owner confirms it, then accept it. Reject stale, repeated, and unconfirmed decisions.
  - Given a creation-day window, when statistics are requested, then report made, kept on time, kept late, moved, dropped, not mentioned, and median days to keep.
- **Metric**: duplicate promise IDs, stale decisions accepted, and missing statistics fields. **Target**: 0 of each in the synthetic US-110 cases.
- **Skill**: `skills/careerbrew-work-day`
- **Web views**: Actions

| Case | Test | Log event |
|---|---|---|
| owner decision and refusals | `tests/e2e/us-110-pm-promise-outcomes.sh` | `work.promise` |
