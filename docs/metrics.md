# Metrics

Two kinds: **outcome metrics** about you (computed by `careerbrew review` from `private/`), and the **build metric** about this repo (capability coverage).

## Outcome metrics

Defined, with success and pivot thresholds and literature anchors, in [`config/goals.yaml`](../config/goals.yaml). Override per user in `private/goals.yaml`. A missing value is `no-data`, never 0.

## README proof

`scripts/metrics.sh proof` measures the README's claims and writes `docs/proof/metrics.json` (value, unit, date, data kind, command, and a history per metric). Then `scripts/proof.py render` rewrites the README Proof section from it, and `tests/unit/test_proof.py` fails when the two differ. Run it after a green `scripts/check.sh`, so the e2e results are fresh. Counts and rates only: never anything from `private/`. Real-use (field) totals are left out unless you set `PROOF_FIELD=1`, which is for your own copy: the committed file holds no machine's real use.

## Capability coverage (build metric)

The share of goal capabilities with a passing e2e test. Measured by `scripts/metrics.sh capability-coverage`. Each report runs every distinct story once; duplicate capability rows share that invocation’s result. Saved passes are not reused across reports because external runtime inputs are not bound ([observability](observability.md#checkup-and-caching)). Baseline 0/16 on 2026-10-18 (14 at goal start; 2 added when the owner asked for work-tool integrations and corporate/personal machines). Success is 17/17 (the owner later added the LinkedIn block). The v2 goal (2026-10-18) added rows 18–30: baseline 17/30, success 30/30. The v3 + web goal added rows 31–41: baseline 30/41 (Notion was dropped by the owner), success 41/41. The Notion + flashcards goal (2026-10-19) added rows 42–45: baseline 41/45, success 45/45.

| # | Capability | E2E test |
|---|---|---|
| 1 | Scan public job boards (Greenhouse, Lever, Ashby) | `tests/e2e/us-02-jobs-scan-filter.sh` |
| 2 | Filter by personal criteria | `tests/e2e/us-02-jobs-scan-filter.sh` |
| 3 | Propose a Want score per posting | `tests/e2e/us-03-want-rate-learn.sh` |
| 4 | Rate postings and refit Want weights | `tests/e2e/us-03-want-rate-learn.sh` |
| 5 | Season strategy: learn → practice → target → dream | `tests/e2e/us-04-strategy.sh` |
| 6 | Drills with FSRS and binary rubrics | `tests/e2e/us-05-drill.sh` |
| 7 | Progress charts from real attempts | `tests/e2e/us-06-progress-chart.sh` |
| 8 | Evidence collection with dedup and cache | `tests/e2e/us-07-evidence.sh` |
| 9 | Encrypted, verified backups | `tests/e2e/us-08-backup.sh` |
| 10 | Promotion doc + gaps by priority | `tests/e2e/us-09-promo.sh` |
| 11 | Task priorities and daily plan | `tests/e2e/us-10-plan.sh` |
| 12 | Outcome review with pivots | `tests/e2e/us-11-review.sh` |
| 13 | Corporate plugin lifecycle | `tests/e2e/us-12-plugin.sh` |
| 14 | Leak prevention on every exit | `tests/e2e/us-13-privacy.sh` |
| 15 | Slack, Google Docs, Jira, GitHub, Zoom, Outlook connectors with secret references and policy | `tests/e2e/us-14-connectors.sh` |
| 16 | Corporate and personal machines with policy-filtered encrypted sync | `tests/e2e/us-15-machines-sync.sh` |
| 17 | Public-safe LinkedIn Experience block from work results | `tests/e2e/us-16-linkedin.sh` |
| 18 | Detailed user stories for both modes, linted | `tests/e2e/us-27-stories-skills.sh` |
| 19 | Every strategy rule and company fact cites a source | `tests/e2e/us-26-sources.sh` |
| 20 | Every interview step described, for every seeded company loop | `tests/e2e/us-17-interview.sh` |
| 21 | Tips and traps for every interview step | `tests/e2e/us-17-interview.sh` |
| 22 | Interactive preparation check | `tests/e2e/us-18-prep.sh` |
| 23 | Remote and nearby job platforms | `tests/e2e/us-19-job-platforms.sh` |
| 24 | LinkedIn export and job-alert integration | `tests/e2e/us-20-linkedin-import.sh` |
| 25 | Outlook, Fastmail, and Proton mail integration | `tests/e2e/us-21-mail.sh` |
| 26 | A skill with evals for every user story | `tests/e2e/us-27-stories-skills.sh` |
| 27 | One-command plugin for a new job | `tests/e2e/us-22-plugin-new.sh` |
| 28 | Toggle-able, pausable, light day watcher | `tests/e2e/us-23-day-watch.sh` |
| 29 | Proactive next-task suggestion | `tests/e2e/us-24-day-next.sh` |
| 30 | Daily plan from calendar facts | `tests/e2e/us-25-day-plan.sh` |
| 31 | Code updates while the watcher runs (hot reload, stable pause/resume) | `tests/e2e/us-30-hot-reload.sh` |
| 32 | Pivot on ladder, plugin, and company changes | `tests/e2e/us-31-changes.sh` |
| 33 | Browser research skill and static page fetch with timestamps | `tests/e2e/us-32-research.sh` |
| 34 | Freshness dates and AI-in-interview policies | `tests/e2e/us-32-research.sh` |
| 35 | Shared interview patterns and a derived default loop | `tests/e2e/us-33-patterns.sh` |
| 36 | Second person on personal and corporate machines, team plugins extend the company plugin | `tests/e2e/us-29-second-person.sh` |
| 37 | Every skill loads best-practices first and iterates at the end | `tests/e2e/us-34-skill-practice.sh` |
| 38 | Web app with seven views and navigation (TypeScript, React + Vite) | `tests/e2e/us-35-web-app.sh` |
| 39 | Plan ahead: Week view with tasks added from the app | `tests/e2e/us-36-plan-ahead.sh` |
| 40 | Local-only, token-guarded web API | `tests/e2e/us-37-web-api.sh` |
| 41 | Web quality gate: tsc, lint, vitest, Playwright, build | `tests/e2e/us-35-web-app.sh` |
| 42 | Notion career workspace imported into its features, idempotent, with a count check | `tests/e2e/us-38-notion-import.sh` |
| 43 | Library view of imported pages and the Notion jobs board | `tests/e2e/us-38-notion-import.sh` |
| 44 | Flashcards: FSRS, recall before reveal, interleaving, daily new cap, leeches, stats | `tests/e2e/us-39-flashcards.sh` |
| 45 | Web Practice view with keyboard grading | `tests/e2e/us-39-flashcards.sh` |
