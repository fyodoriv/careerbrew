# Vision

Keep this to one screen. A PR is accepted or rejected by reading only this page.

## Problem

A programmer gets about one interview loop per company per season. Most people spend it unprepared, on the wrong companies, in the wrong order. At the job they already have, most of their work goes unrecorded. So the promotion case is written from memory, against criteria they never read. Existing tools cover one slice each (job scoring, brag docs, flashcards). None of them keep a measured, private model of you that drives both decisions.

## User

One programmer (the owner) who runs careerbrew through a coding agent and the CLI. It is used both while job hunting and while employed.

## Scope

- **Job hunt mode**: find, rank, and prepare for the next job.
- **Work mode**: the work record (day log, PM update, standup, evidence, promotion docs, work feed) and the work-day runner (`/work`: the day's ordered steps, a status row per step, one page for the day, end of day). All work-day features live in careerbrew because they are career work; agentbrew only deploys careerbrew's commands.

## Success

- **Metric**: the share of outcome metrics in KEEP state for the active mode in `careerbrew review` (job hunt: want_agreement, readiness, retention, practice_pass_rate; work: promo_coverage, gap_closure, estimate_ratio, evidence_freshness_days).
- **Target**: at least 75% KEEP after 8 weeks of use in a mode.
- **Measured by**: `uv run careerbrew review` (snapshots in `private/review.jsonl`).
- **Build metric**: capability coverage (14 goal capabilities with a passing e2e test): `scripts/metrics.sh capability-coverage`.

## Non-goals

- Never applies, sends messages, or posts on your behalf. It drafts; you send.
- Never scrapes logged-in sites (LinkedIn, internal wikis). Use public ATS APIs or a plugin you control.
- Never stores company content in this repo. That goes in a corporate plugin.
- No hosted service; a local web app only. CLI, agent skills, local HTML reports, and the local web app.

## Kill criteria

- Fewer than 3 sessions a week for 4 weeks in a row, after 2026-12-31 (from `private/logs/events.jsonl`).
- The success metric stays under 50% KEEP after 12 weeks in a mode.
- An upstream tool covers 80% of both modes. Then contribute to it instead.

## Prior art

| Tool | Overlap | Verdict |
|---|---|---|
| [santifer/career-ops](https://github.com/santifer/career-ops) | Job scoring on weighted dimensions inside Claude Code, tailored CVs, a tracker | Borrow the weighted-dimension idea. Build the rest: strategy tiers with one-shot gates, the tutor, work mode, plugins, and privacy |
| brag-doc skills (pm-claude-skills `brag-doc`, awesome-copilot `brag-sheet`), [brag-ai](https://pypi.org/project/brag-ai/) | Evidence capture for reviews | Build: careerbrew maps evidence to a ladder and ranks the gaps |
| [FSRS / py-fsrs](https://github.com/open-spaced-repetition/py-fsrs) | Spaced-repetition scheduling | Use (dependency) |
| agentbrew team overlays | The overlay shape for company content | Use the shape. Extend with several plugins at once, hash verification, policies, and a data purge |
