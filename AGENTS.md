# AGENTS.md — careerbrew guide for agents

Proposal identity (#371): use the inbox id returned by `work task` for an owner
decision. Restricted output may show only canonical ASCII task references.
Explicit group review lists every member; retain the ordinary list budget.
The offer receipt has invocation scope. A returned id never grants acceptance
or permission to read business text.

Hook setup (#327): retain a configured Git hook path. Name the repository entry
point separately from verified delegation. The pre-commit runner uses
`lefthook run --no-auto-install pre-commit`; keep its configured checks and
failure status. Native qualification uses owned synthetic hooks, never owner
hooks or configuration.

## Start here

careerbrew is a personal AI assistant. A private, local-first career assistant for programmers: it finds and wins the best next job, and supports you at your current job with evidence, promotion docs, priorities, and coaching.
The canonical branch is `main`.

careerbrew bug or idea? Offer the user a [safe report](CONTRIBUTING.md#report-problems-and-ideas); never include `private/` data.

Read this file first. Then open only the doc that the routing table names.
`CLAUDE.md` is a symlink to this file. The docs map is [`docs/README.md`](docs/README.md).
Before you propose a feature, read [`docs/VISION.md`](docs/VISION.md): its
non-goals and kill criteria decide every trade-off.

## What careerbrew owns

- careerbrew owns the user's work record and everything built from it: the day log, the PM update, a standup report, evidence, promotion docs, the work feed with its web views, and the work-day runner (`/work`, `src/careerbrew/workday/`).
- agentbrew only deploys careerbrew's commands; it never owns them. The source is `commands/` here, declared in `Agentfile.yaml`.
- Litmus test: if it reads or writes the user's career data, it is careerbrew.
- So a new career workflow is a careerbrew CLI action, skill, or command in this repo (or in a company plugin, [`docs/plugins.md`](docs/plugins.md)). Never write it into agentbrew config or by hand into an agent config folder such as `~/.claude/commands/`.

## Commands

Run every command from the repo root.

| Task | Command |
|---|---|
| Start everything (watcher, web app, refresh) | `uv run careerbrew` (agents: `/careerbrew`) |
| Capture the bounded synthetic HTTP handler contract (local receipt) | `cargo xtask migration capture-http --python "$PWD/.venv/bin/python" --out "$PWD/.metrics/http-contract.json"` |
| Capture the bounded synthetic DATA API contract (developer receipt) | `cargo xtask migration capture-data --python "$PWD/.venv/bin/python" --out "$PWD/.metrics/data-contract.json"`; use a new local output, not a user data route |
| List every action (CLI, agents, web app) | `uv run careerbrew actions [--json]` |
| Show or change a setting | `uv run careerbrew config` · `uv run careerbrew config set <key> <value>` |
| One-line setup (idempotent) | `scripts/setup.sh` |
| Health check | `scripts/doctor.sh` |
| Checkup (whole repo: when it last ran here and the commits since, open PRs and branches, health, cached gate, broken stories, speed, P0s, monthly audit) | `scripts/checkup.sh` (ends with a run page and one summary line; a copy stays at `.metrics/checkup.html`); agents: `/careerbrew-checkup`, which ends by filing each problem found (duplicate search first, drafts checked with `careerbrew privacy scan --file`) |
| What a long run did, its blockers, and next commands (content-free, for agents) | `uv run careerbrew report latest --json` (open the page: `uv run careerbrew open --report latest`) |
| Install | `uv sync` |
| One test | `uv run pytest tests/test_file.py::test_name -q` |
| Select Python tests while iterating | `uv run python scripts/tests_affected.py [<ref>]` lists modules. Add `--run` in a trusted checkout and Python environment. The hosted checks run the full gate for the pull request. |
| Several related test files | `heavy-run uv run pytest -q -n 8 <files>` |
| Lint and format | `uv run ruff check --fix . && uv run ruff format .` |
| Type check | `uv run pyright` |
| Fast gate (each commit; unit tests run in parallel with pytest-xdist, `UNIT_JOBS=0` runs them one by one) | `scripts/check.sh fast` |
| Full gate with e2e (optional on your machine: the hosted checks run it for every pull request, [`docs/ci.md`](docs/ci.md); eligible steps cache by their inputs; leak scan, format, lint, `types`, unit, exercises, `web-types`, e2e and Rust steps do not reuse cache entries) | `scripts/check.sh` (`NO_CACHE=1` for a full rerun) |
| A gate step's inputs (git pathspecs, like nx target inputs; empty means every file) | `scripts/check.sh inputs <step>...` (change them in `inputs_of`; `tests/unit/test_gate_inputs.py` must pass) |
| E2E tests only (in parallel; a failing file's log prints at the end; `E2E_JOBS=1` runs them one by one) | `scripts/e2e.sh` |
| LLM-judged evals and owner labels | `EVALS_LLM=1 evals/run.sh` · `evals/run.sh show` · `evals/run.sh label <case> pass\|fail` · `evals/run.sh agreement` |
| Capture the current Python action catalog in two synthetic homes | `cargo xtask migration capture-actions --python "$PWD/.venv/bin/python" --out "$PWD/.metrics/action-contract.json"` after `uv sync --locked --offline`; output must be a new local file; receipts include local paths and observe retained owned changes only |
| Fixed Linux observer preparation | `cargo xtask migration observer-probe --image <frozen-image-id>`; follow the fixed observer protocol, retain receipts locally, and obtain release before actual execution |
| Speed report | `scripts/metrics.sh` |
| README Proof numbers (after a green gate; writes `docs/proof/metrics.json`, then the README section) | `scripts/metrics.sh proof` |
| Capability coverage (runs each distinct story once per report; cross-run reuse is unqualified) | `scripts/metrics.sh capability-coverage` |
| One local full gate for up to 4 ready PRs, then merge them in order (optional since the hosted checks; exit 0 merged, a PR that conflicts left out and named; 1 failed and nothing merged, 2 refused, 3 stopped after a merge; `--dry-run` merges nothing) | `scripts/batch-gate.sh <pr>...` (`/careerbrew-grind` step 3, "Batch the full gate") |
| Hosted checks of a pull request (the gate on clean GitHub runners; rules in [`docs/ci.md`](docs/ci.md)) | `gh pr checks <pr>`; a failed run: `gh run view <run-id> --log-failed` |
| Open work to continue first (PRs, branches, unpushed changes) | `scripts/open-work.sh` |
| Public-readiness baseline for the open-source release (one line per exposure surface S1 to S22: id, kind, status, count; reads a git ref, never prints matched text; exit 0 only when no surface is `fail` or `error`) | `scripts/public-readiness.sh [--tree <git-ref>]`; owner mode on the owner's Mac: `--release` (adds the private pattern, the privacy scan, the guard, plugin-name and owner-place checks, and the review signatures in `docs/proof/public-readiness.md`); `--tree-hash` prints the hash a signature must carry; `--list-binaries` prints the path and hash of each non-text file for the S7 inventory `docs/proof/public-readiness-binaries.tsv`; `--first-run` makes S17 run setup, doctor and the fast gate in a scratch copy that holds one commit, like a clone (slow; a failed S17 names each failing command by its place and script, for example `failed=4:check.sh`); `--allow-home-override` lets S16 and S20 read the data folder that `CAREERBREW_HOME` names (without it they are `unmeasured` when it is set) |
| The public release tree (the ref without the paths in `config/public-release-exclude.txt`; every readiness check and the signature hash read this tree, and the summary line says `excluded=<files>`) | `scripts/public-readiness.sh --write-release-tree <new-dir> [--tree <git-ref>]` writes it; `--release-tree-id` prints its git tree id for `git commit-tree`; `--list-dangling` prints each file that points to a left-out path (surface S21), with its count of paths; add `--released` to every run on the release commit itself |
| Work the whole queue, then keep watching it (agents) | `/careerbrew-grind`: checkup and its HTML page, open work, every open issue by the owner and unblocked task, a batch report, then the agentbrew `watch-issues` skill (polls every 5 minutes) |
| Web app (TypeScript) | `uv run careerbrew serve`; dev: `cd web && npm run dev`; checks: `npm run typecheck && npm run lint && npm test && npx playwright test` |
| Visual tests (approved pictures of each story's screens) | `cd web && npm run build && npm run visual`; after a change on purpose: `npm run visual:update`, then open every changed picture ([`docs/web.md`](docs/web.md), "Visual tests") |

## Privacy (read before touching data)

- Personal and business data live in `private/` (allowlist `.gitignore`). The few plaintext files outside it are in the "Where data lives" table of [`docs/privacy.md`](docs/privacy.md); read it first.
- Learn about the user's work data from CLI output (aggregates, ids, paths). Do not open `private/work/` or `private/out/promo-*` unless the plugin policy sets `agent_may_read_business: true`.
- Data records are encrypted at rest by default (the plaintext exceptions are in the "Where data lives" table of [`docs/privacy.md`](docs/privacy.md)). Read and write them with `store.read_text` / `store.write_text` (never `Path.read_text`); people edit them with `careerbrew data edit <file>`. To check a file without changing it, run `careerbrew data show <file> --keys` (key names and types, never a value).
- Tests use synthetic canaries only (`tests/unit/conftest.py`). `scripts/leak-check.sh` runs in the gate and the git hooks.
- On a corporate-role machine, job-hunt commands are blocked by design ([`docs/integrations.md`](docs/integrations.md)).

## Where to change what

| I want to… | Change | Update this doc too |
|---|---|---|
| Add a feature | the code, a story in `docs/user-stories.md`, and its tests in `tests/e2e/` | `README.md` if users see it |
| Add a career workflow (a report, an update, a standup) | a CLI action, a skill in `skills/`, or a command in `commands/` here; never agentbrew config (see `## What careerbrew owns`) | `README.md` if users see it |
| Add a log event or metric | the code | `docs/observability.md` event or metric table |
| Add a dependency | the manifest, plus a check in `scripts/doctor.sh` | `README.md` if setup changes |
| Record a decision that is hard to undo | a new `docs/decisions/NNNN-*.md` | none |
| Plan a change bigger than one PR | `docs/plans/YYYY-MM-DD-<topic>.md` | none |
| Record what a shipped change taught us | the pull request body, under Predicted / Observed / Lesson (no new file in the learnings folder: the public tree leaves it out) | none |
| Add or close a task | `TASKS.md` | none |
| Add an assistant feature | `skills/<feature>/SKILL.md` and a case in `evals/cases/` | `README.md` |
| Change assistant behavior | `prompts/` or the skill, then run the evals | `evals/rubric.md` if the bar moves |
| Change a model or provider | `config/models.yaml` | `docs/privacy.md` if data leaves the machine |
| Add reference material | `knowledge/<topic>.md` | `knowledge/README.md` |
| Add a work-tool connector | `src/careerbrew/connectors.py`, a fixture in `tests/fixtures/http/`, a unit test | `docs/integrations.md` |
| Add a work feed source or a built-in triage rule | `connectors.FEED_READERS` or `BUILTIN_RULES` in `src/careerbrew/work/feed.py`, fixtures in `tests/fixtures/http/feed/`, a case in `tests/unit/test_issue_72.py`; an agent's MCP tool source is a `feed_tool_sources` setting, read by `src/careerbrew/work/transcript.py` (tests: `tests/unit/test_issue_234.py`, fixtures in `tests/fixtures/transcripts/`); a raw tool reply shape or the live-feed cycle record is `feed.raw_events` / `feed.mark_cycle` (tests: `tests/unit/test_issue_254.py`) | `docs/integrations.md` (Work feed) |
| Change the live feed (cycle, source adapters, verdicts, PM draft rule, a plugin's sources and roster) | `src/careerbrew/work/live.py`, `skills/careerbrew-live-feed/`, `tests/unit/test_issue_267.py`, fixtures in `tests/fixtures/live/`, `tests/e2e/us-97-live-feed.sh` | `docs/integrations.md` (Work feed), `docs/plugins.md` (Live feed sources and roster) |
| Change how background agents report progress or what Agent activity shows | `src/careerbrew/work/agents.py`, `cmd_work_agent` in `src/careerbrew/cli.py`, `AgentActivity` in `web/src/agents.tsx`, `tests/unit/test_issue_272.py`, `web/e2e/agents.spec.ts`, `tests/e2e/us-91-agent-activity.sh` | `docs/integrations.md` (Agent activity), US-91 in `docs/user-stories.md` |
| Change plugin manifest or policy fields | `src/careerbrew/plugins.py` and its tests | `docs/plugins.md` |
| Change a data exit (logs, models, backups, sync, stdout) | the module and a canary test | `docs/privacy.md` exits table |
| Add a company interview loop or a strategy rule | `knowledge/research/interview-research.yaml`, then `scripts/import-research.py` | `knowledge/README.md`; `scripts/check-sources.py` must pass |
| Add a job platform | `src/careerbrew/jobs/platforms.py`, a fixture in `tests/fixtures/http/` | `docs/integrations.md` (terms column) |
| Add or change a web view or a control in it | `web/src/views.tsx`, `web/src/api.ts`, an endpoint in `src/careerbrew/server.py`, a Playwright test in `web/e2e/` that checks each control's real effect, and each control's entry in `web/e2e/controls.json` (`controls.spec.ts` fails on a control with no entry; the gate runs the whole suite in `tests/e2e/us-72-every-control.sh`), and the story's visual test in `web/visual/` with its pictures (`npm run visual:update`, then open each one) | `docs/web.md`; the story's **Web views** line |
| Set up a corporate machine or build a company plugin | follow `docs/corporate-machine.md` with `/careerbrew-corporate` (skills `careerbrew-corporate-setup` and `careerbrew-corporate-plugin`); the plugin lives outside this repo | `docs/plugins.md` if the plugin shape changes; `tests/e2e/us-40-corporate-rehearsal.sh` if a step changes |
| Change how a project and its memory file sync (memory edit → project, work items → notes) | `src/careerbrew/work/memory_sync.py` and `tests/unit/test_memory_sync.py` | `docs/integrations.md` (Project memory sync), `docs/privacy.md` exits table |
| Change a prep project's readiness gate (tool, links, questions, quiz) or where its open gates show | `readiness`, `gate`, `quiz`, `gate_blocks` in `src/careerbrew/work/projects.py`, the plan in `src/careerbrew/day/planner.py`, `PrepGates` in `web/src/views.tsx`, `tests/unit/test_issue_276.py`, `tests/e2e/us-93-prep-readiness.sh` | `README.md`; US-93 in `docs/user-stories.md` |
| Change how promises the owner makes are found, tracked, or closed (`work promise`, Dashboard's Commitments) | `src/careerbrew/work/promises.py` (detection rules, the handoff's live items), its writers in `projects.py` (`add_promise`, `close_promise`, `promise_step`, under `@_writes`), `Commitments` in `web/src/views.tsx`, `tests/unit/test_issue_268.py`, `tests/e2e/us-99-promises.sh`, `web/e2e/promises.spec.ts` | `README.md`; US-99 in `docs/user-stories.md`; `docs/privacy.md` exits table |
| Change how a project is learned (`careerbrew learn`, the careerbrew-learn-project skill) | `src/careerbrew/learn/` (`project.py`, `replay.py`, `tour.py`), `skills/careerbrew-learn-project/`, `tests/unit/test_learn*.py`, `tests/e2e/us-80-learn-project.sh` | `docs/learn.md`; `docs/privacy.md` if what is stored or printed changes |
| Change the standup report, which rows it shows, or its proof gate | `src/careerbrew/work/standup.py`, `tests/unit/test_issue_110.py`, `test_issue_229.py`, `test_issue_271.py`, `tests/e2e/us-65-standup-report.sh`, `us-95-standup-proofs.sh`, the judge step in `skills/careerbrew-work-companion/SKILL.md` | `docs/plugins.md` (Standup rows), US-65 and US-95 in `docs/user-stories.md` |
| Change the project explainer, its deep mode, or the PRD rules | `skills/careerbrew-project-explainer/`, `src/careerbrew/work/explain.py`, `src/careerbrew/work/prd.py`, `tests/unit/test_issue_223.py`, `evals/cases/023-project-explainer.yaml` | `README.md`; US-56 in `docs/user-stories.md` |
| Change how a review ask picks reviewers or reads required approvals | `src/careerbrew/work/review_ask.py`, fixtures `repos_example_shop_*` and `repos_example_tools_*` in `tests/fixtures/http/api.github.com/`, `tests/unit/test_issue_270.py`, `tests/e2e/us-90-review-ask.sh` | `docs/integrations.md` (Review asks), `docs/privacy.md` exits table |
| Import from Notion or change flashcards | `src/careerbrew/notion_import.py`, `src/careerbrew/tutor/cards.py`, the `careerbrew-notion-import` skill | `docs/practice.md`, `docs/integrations.md` |
| Change what notifies, when, or how | `src/careerbrew/notify.py`, the `notify_*` rows in `src/careerbrew/config.py`, `tests/unit/test_issue_74.py` (the fake notifier: `tests/fixtures/notifier/`) | `docs/integrations.md` Notifications; `docs/observability.md` events; `docs/privacy.md` exits |
| Add or change a user story | `docs/user-stories.md` (mode, criteria, metric, skill, web views, e2e), and for web views a visual test in `web/visual/` | `scripts/check-stories.py` must pass |
| Change what the public repo health export or its Dashboard holds | `src/careerbrew/repo_health.py` (`validate` is the allowlist), `config/repo-health-dashboard/` (the page and datasets every machine builds), `tests/unit/test_repo_health.py` | `docs/privacy.md` (Repo health Dashboard), `commands/careerbrew-checkup.md` |
| Change what a live cycle does for each new event (reply drafts, link research, review notes, project updates, goal lines) | `src/careerbrew/work/listen.py`, `live.cycle`, `skills/careerbrew-live-feed/`, `tests/unit/test_issue_266.py`, `tests/e2e/us-101-live-listening.sh` | `docs/integrations.md` (Proactive listening); US-101 |
| Change the work-day runner (`/work`: its plan, step sources, status rows, or page) | `src/careerbrew/workday/` (`steps.py`, `stages.py`, `plan.py`, `status.py`, `start.py` for `day start`: refresh, preflight, sign-in probe; `questions.py` for question records and the recheck; `timing.py` for step durations, the slow flag, and `day history`), `runs.work_day`, `skills/careerbrew-work-day/` (with `shell-traps.md`), `commands/work.md`, `tests/unit/test_workday.py`, `tests/unit/test_issue_258.py` (step fields and stages), `tests/unit/test_issue_260.py` (start of day; temp git repos only), `tests/e2e/us-94-work-day.sh`, `tests/e2e/us-96-work-day-stages.sh`, `tests/e2e/us-102-work-day-start.sh`, `tests/unit/test_issue_261.py` and `tests/e2e/us-104-question-records.sh` (question records), `tests/unit/test_issue_263.py` and `tests/e2e/us-105-work-day-timing.sh` (timing) | `README.md`; `docs/plugins.md` ("Work steps" fields table) if `work_steps` or a step field changes; US-94, US-96, US-102, US-104, US-105 |
| Change project sessions (one session per project on `/work`, claims, the session contract) | `src/careerbrew/work/sessions.py`, the claim check in `work/drafts.py`, `cmd_work_session` / `cmd_work_claim` in `cli.py`, `skills/careerbrew-work-day/SKILL.md` (Project sessions), `commands/work.md`, `tests/unit/test_issue_274.py`, `tests/e2e/us-107-project-sessions.sh`, `evals/cases/026-project-sessions.yaml` | `docs/integrations.md` (Project sessions), `docs/privacy.md` exits table; US-107 |
| Change reply-draft rules (fit the thread's newest state) or the safe draft save (clear, save, read back, per-target lock) | `src/careerbrew/work/drafts.py`, `cli.cmd_draft_*`, `tests/unit/test_issue_269.py`, `tests/e2e/us-98-reply-drafts.sh` | `skills/careerbrew-work-day/SKILL.md` (Reply drafts); US-98 in `docs/user-stories.md` |
| Change a work step's proof (`verify`), timeout and retry, or the blocked-versus-failed rule | `src/careerbrew/workday/gate.py`, the truth fields in `status.py`, `cmd_day_update` and `cmd_day_check` in `cli.py`, `tests/unit/test_issue_259.py`, `tests/e2e/us-100-work-day-truth.sh` | `docs/plugins.md` (Work steps); US-100 |
| Change how the work day ends (wrap-up by clock, `day end`, the end-of-day handoff and its morning recheck) | `src/careerbrew/workday/end.py`, `src/careerbrew/day/handoff.py` (`replace_owned`), `notify.remind`, the `work_wrap_minutes` row in `src/careerbrew/config.py`, `tests/unit/test_issue_265.py`, `tests/e2e/us-106-end-of-day.sh` | `README.md`; `docs/privacy.md` (Wrap-up reminder); US-106 |
| Change the work-day watch loop (`day loop`: its lock, interval, step `cycle`/`watch` fields, or loop records) | `src/careerbrew/workday/watch.py`, `tests/unit/test_issue_264.py`, `tests/e2e/us-103-work-day-watch.sh` | `docs/plugins.md` (Work steps); US-103 |
| Make a long run end with a run page | a builder in `src/careerbrew/runs.py` (business text only in `detail`), `report.finish(...)` at the end of the command, a test in `tests/unit/test_run_pages.py` | the run lists in `docs/privacy.md` (Run pages) and `README.md` |

## Corporate machine

- Entry point: `/careerbrew-corporate`, or the skills `careerbrew-corporate-setup` (role, tokens, pairing) and `careerbrew-corporate-plugin` (build, fill, refresh the plugin).
- Done means `uv run careerbrew machine ready` says `corporate readiness: N/N` in its summary line. Report the before and after counts; the failed checks and their fixes are in `uv run careerbrew report latest`.
- The user types every token. Give them the `careerbrew secret set` lines; never ask for a token in chat.
- Never push from the corporate machine. Company content lives only in `careerbrew-<org>`, never here.
- A runbook change must keep `tests/e2e/us-40-corporate-rehearsal.sh` green. It must use synthetic names made at runtime: a fixed name in a tracked file fails the repo-term check.

## Rules

1. A change is done when the hosted checks of its pull request are green: `gate (ubuntu-24.04)`, `gate (macos-15)` and `public-readiness` ([`docs/ci.md`](docs/ci.md)). Show the `gh pr checks <pr>` output. Never wait for a local full gate: run `scripts/check.sh fast` or single tests while you work, and let the runners run the full gate.
2. A bug fix starts with a failing test that reproduces it.
3. Each change updates its docs in the same commit (see `## Docs upkeep`).
4. Touch only what the task needs. Note other problems as tasks in `TASKS.md`.
5. Never commit secrets. Real values go in `.env` (ignored); names go in `.env.example`.
6. Prefer a maintained library or tool to new code. Wrap it behind a small adapter.
7. Every user story has an e2e test per case in `tests/e2e/`, named after its id. Every CLI action runs in one too (`cb <command>` in a story; a browser spec does not count): `test_every_action_has_an_e2e.py` fails on a new command until a story runs it. A story that runs `open` or `notify open` calls `fake_browser` and stops the server with `web_stop` (`tests/e2e/lib.sh`).
8. Log content-free JSONL events ([`docs/observability.md`](docs/observability.md)). Errors name the story they break; a broken story is fixed first.
9. A rule a machine can check becomes a step in `scripts/check.sh`, not a sentence here.
10. Key costly outputs by a hash of their inputs, so nothing is computed twice.
11. Personal data lives in `private/`; the few plaintext files outside it are in the "Where data lives" table of [`docs/privacy.md`](docs/privacy.md). Tests and evals use synthetic data.
12. Models are named only in `config/models.yaml`. Use the local route first.
13. Each feature is a skill in `skills/<feature>/SKILL.md` with at least one case in `evals/cases/`.
14. A prompt, skill, or model change runs the regression evals (`CHECK_EVALS=1 scripts/check.sh`).
15. Every action runs three ways: CLI, agents, web app. A new CLI command is an action automatically (`actions.py` reads the parser); if it cannot run in a browser, add it to `actions.TERMINAL_ONLY` with the reason, and if it changes or sends something hard to undo, to `actions.CONFIRM`. `test_action_parity.py` enforces it.

## Gotchas

The synthetic storage signal fixture may read back SA_NOCLDWAIT along with
SIG_IGN on macOS. Verify the requested ignored handler and fail closed on either
incompatible condition; a start-attempt marker needs stderr before process exit
(`incompatible_sigchld_refuses_before_worker_start`).

Exercise checks rerun validation and ignore old saved passes, including when
`NO_CACHE=1`; neither gate layer binds all runtime inputs. The separate
in-process YAML parse cache still uses path, size and mtime. Removing the
exercise cache shifted 21 Rust activation anchors; keep their source sites and
the deferred-boundary expectation aligned (`activation_manifest.rs`).

Capability coverage reruns each distinct story per report. The saved e2e key
does not bind external runtime inputs; only duplicate rows share a result
within one invocation (`test_e2e_results.py`).

A Rust migration verdict rejects an empty expected check set. Nonempty labels
alone do not prove execution; the future collector must bind that evidence.

Browser fixture EPERM diagnostics record worker Node version and executable-path
file metadata without the path. The metadata is observed after startup and does
not prove the file was not replaced; an unavailable identity preserves EPERM.

The synthetic Rust child-wait fixture must use unbuffered pipe handles for its one-byte readiness signal; a buffered stdout can deadlock the handshake. Its observed input receipt is not build provenance, and a failed outer owner never proves the worker was reaped.

Python’s non-validating vault decoder can accept trailing bytes after completed padding. Keep the finite synthetic differential matrix and Fernet authentication when changing the Rust candidate; the candidate is not production storage.

The first successful browser-page change poll invalidates live views once. It covers writes between an initial view read and the first version response; ignore a response after the poller stops (`live.test.tsx`).

Generated agent metadata uses one exclusive `.tmp` sibling per invocation; keep the descriptor until owned cleanup and exclude in-flight files from bulk vault scans (`test_issue_423_metadata_temp.py`). The path checks do not prevent concurrent replacement by another process under the same account, and this does not serialize deployer reads.

Update-only checkout discovery shares a lookup only within that invocation and drops it before refresh. Never reuse discovery results across refreshes or later runs (`test_issue_426.py`).

The Rust inventory gate checks source assignments against the staged Git index; stage new source paths and their assignments together.

Rust capture tests read the staged Git tree, HEAD, and external runtime files.
Stage intended source changes before the gate so its source-copy checks can
compare the staged and working trees. The Rust checks always run until a complete runtime binding is qualified; old cached passes are not used. Do not edit while a gate runs.

Git can invoke pre-push with no ref records for an up-to-date push. The context check still runs the strict scan; an empty stream does not prove delivery or authorize an exception. Source revision expressions are opaque; their full object IDs remain separate.

Privacy scans read staged blobs by object ID and label staged versus working
findings. Visual snapshots use a narrow validated PNG policy; pixel text is not
OCR-scanned. The index recheck does not prove pushed commit history.

Evaluator HTTP responses must include `finish_reason: stop` before their text
can be graded or cached. Older text-only cache entries lack that evidence.

`draft replace` reads back careerbrew's local draft store. It does not save or
verify a draft in a chat tool; a chat-tool save needs its own read-back.

Project-session instructions also reach agents through `sessions.CONTRACT` in
the work-day plan. Keep that contract, the skill, and `/work` aligned; check the
plan field when changing session behavior.

Project source normalization lives in `projects._source_ref`; refresh and memory
sync repair stored refs through `normalize_sources`. Keep GitHub item URLs scoped
to the item. Live-link research uses the same item-reader predicate; preserve
exact reference-only conversation URLs during source migration. Never infer a missing item URL from an existing bare repo source.

Work-day plan checkout discovery reuses Git top-level results by resolved
directory during one invocation, including the restricted JSON labels pass.
Separate invocations and checkout refreshes query Git again.

Work-day stages are an earliest-start view. Dispatch from `day check --json`
readiness after each completion. Keep unknown-scope serial barriers and finite
resource prerequisites for long-running loops in `ready_when` and `why`.

PM draft facts must survive GitHub/gh collection, evidence, watcher rows, results,
and log details. Version same-id rows when draft/state facts change. A closed PR
without explicit merge evidence must not get a delivery tag.

Keep open asks at the end of the Next step priority order. Use the feed's open
queue and keep titles and links out of restricted CLI output and watcher suggestions.

A trap that cost time gets a dated row here, or becomes a check.

| Date | Symptom | Fix |
|---|---|---|
| 2026-11-13 | US-40 failed only in a hosted pull-request run: `Could not read <parent of main>` when it cloned its bundle of `main`; the same gate passed on a push | Playwright 1.59 records a diff on GitHub Actions: in a pull request it runs `git fetch origin <base> --depth=1`, which makes the checkout shallow. Every Playwright config sets `captureGitInfo: { commit: false, diff: false }` (test: `test_hosted_ci.py`). When a failure shows only in one CI event type, compare what tools do with `GITHUB_EVENT_PATH` |
| 2026-11-13 | every merge waited for the local full gate, which queues behind other sessions on one machine (a pre-push fast gate alone took 12 to 16 minutes under load) | the hosted checks are the merge gate now ([`docs/ci.md`](docs/ci.md)); the pre-push hook runs only the push-context check, the leak scan and gitleaks. A test that passes on your Mac can fail on a clean runner: read `gh run view <run-id> --log-failed` first (the first hosted run found 13 such tests: a resolver lookup at bind, `DO_NOT_TRACK`, nice 19 on Linux, folder order) |
| 2026-11-10 | a public file linked to a journal file, or a build read a file in `docs/plans/`, and the release tree did not hold it | the release tree leaves out `docs/learnings/` and `docs/plans/` (`config/public-release-exclude.txt`). Do not link to a journal file from a file that stays public. A data file that code or a test reads does not belong in a journal folder; until it moves, it needs a `!` line in the list. The list must work, or the readiness script gives `error`: no missing or empty list, no line that matches no file, at least one file left out and one file left, no path that differs from a line only by letter case, no submodule. Surface S21 counts the files that still point to a left-out path. Prove a change with `scripts/public-readiness.sh --write-release-tree <new-dir>`, then run the gate inside that folder with `CAREERBREW_BIN_DIR` set to a scratch folder (`test_public_readiness.py`) |
| 2026-11-08 | combining rules and promise references added five source inputs beyond the prior census | bind both deltas and all three successor pairs, and update the current source-copy count without changing historical pins (`2026-11-08-rules-promise-combined-reference.md`) |
| 2026-11-08 | generated confirmed forms showed a second `yes` checkbox although confirmation supplies `--yes` | hide only the boolean `--yes` field for confirmed forms and keep `YES_FROM_CONFIRM` equal to those actions; preserve other `yes` arguments (`test_action_parity.py`) |
| 2026-11-08 | new action commands changed both the CLI catalogue and two HTTP responses while the v1 references stayed frozen | use the pinned expected-side rules-v2 fixtures, preserve v1 bytes and provenance, bind the four added source inputs, and check the independent delta plus the current source census (`test_rules_reference_successor.py`) |
| 2026-11-08 | three new actions changed the count in six unrelated Actions form pictures | update and inspect the US-53 desktop and phone pictures when the action count changes (`us-53-actions-and-slack.spec.ts`) |
| 2026-11-08 | sent-PM changes moved Rust activation anchors and introduced new receipt and lock boundaries | relocate the existing anchors against their exact call sites; keep the manifests partial and track the new boundaries separately (`2026-11-08-promise-activation-boundaries.md`) |
| 2026-11-08 | a new HTTP-only control was unused in the separate DATA test binary and broke Rust lint | allow that one shared helper to be unused there; do not present the HTTP control as DATA coverage (`http_controls.rs`) |
| 2026-11-08 | reading the day log created the feed lock and made the next cache lookup miss | exclude only the empty `work/feed/.lock` from file versions; keep nonempty files as inputs (`test_api_cache.py`) |
| 2026-11-08 | a corrected sent update could create a second promise while an earlier decision still needed review | defer the matching day-plan item; only a confirmed, uniquely bound choice may create or retain a separate promise, after rechecking every matching record under the project lock (`test_issue_389_resolution.py`) |
| 2026-11-08 | an older sent-update replay could replace a newer proposal, or a feed edit could race an owner decision | retain the newest observation and hold sent-day, feed, then project locks through acceptance; recheck the exact retained receipt revision (`test_issue_389_outcomes.py`) |
| 2026-11-08 | the promise decision form asked for `yes` twice | hide only its boolean `--yes` field; the confirmed Actions run supplies the CLI flag (`us-110-pm-promise-outcomes.spec.ts`) |
| 2026-11-08 | source-mode fixture checks passed under the queue's `077` mask but failed under `022` | make the owned synthetic fixture root `700` explicitly; retain the production private-root check (`source_copy_construction_structural_modes`) |
| 2026-11-08 | a synthetic web type checker reused a stale pass after an external input changed | always rerun `web-types` until its runtime inputs are qualified (`test_web_type_cache_probe.py`) |
| 2026-11-08 | a synthetic end-to-end runner changed behavior while the full gate reused its cached pass | always rerun end-to-end tests until their runtime inputs are qualified (`test_e2e_cache_runtime.py`) |
| 2026-11-08 | synthetic JavaScript and Python type checks shared a cache entry, allowing one to skip the other | require fresh type checks until their identities and effective inputs are qualified (`test_types_command_collision_probe.py`) |
| 2026-11-08 | a changed synthetic type checker reused a cached success without a source change | rerun each available root type checker until its effective inputs are qualified; `web-types` is separate (`test_type_cache_runtime.py`) |
| 2026-11-08 | the independent DATA CLI expectation was built only after capture started | check its literal revision and digest against the same-source production receipt before the ordinary capture; retain historical pins and distinguish diagnostic launch paths (`data_expectation_preflight`) |
| 2026-11-08 | a standup page route was still fetching when its browser context closed | wait for page-owned route handlers before the existing page-error check; keep handler errors visible (`controls-route-lifecycle.spec.ts`, `route-owner-probe`) |
| 2026-11-08 | the control inventory removed a visit route and began navigation before its in-flight response completed | wait for page-route handlers before leaving each visit; keep corporate routes at browser-context scope (`controls-route-lifecycle.spec.ts`) |
| 2026-11-08 | a real-clock source-health fixture stamped an incident before midnight but Today refreshed after midnight | choose a fixture-only fixed-offset zone whose setup hour is noon; keep real expiry and verify all UTC hours (`test_live_health_seed.py`) |
| 2026-11-08 | a repeated synthetic Tomorrow promise created a second identity and outcomes actions were absent | preserve the baseline and verify the registered histories before changing sent-promise reconciliation (`test_issue_389_baseline.py`) |
| 2026-11-07 | ended or ambiguous prior sessions could be mistaken for completed work | classify only verified identities and calendar dates; surface uncertainty and never infer completion (`test_prior_session_selection.py`) |
| 2026-11-08 | a browser test count alone could not prove which discovered tests ran | compare Playwright discovery and execution identities and reject duplicates or non-passes (`browser-test-receipts.py`) |
| 2026-11-08 | the extra JSON discovery was mistaken for a replacement for the spec-count check | permit only US-72 receipt discovery while requiring `pw_total` and the identity comparison (`test_e2e_lib.py`) |
| 2026-11-07 | a synthetic changed formatter reused a cached pass without a source change | always rerun format until all effective runtime inputs are qualified (`test_format_cache_runtime.py`) |
| 2026-11-07 | a synthetic changed lint runner reused a cached pass without a source change | always rerun lint until all effective runtime inputs are qualified (`test_lint_cache_runtime.py`) |
| 2026-11-07 | the end-of-day no-extra-report test counted synchronization locks as reports | allow the session-store lock and only an empty regular `work/feed/.lock`; keep negative checks for a nonempty feed lock and unexpected report files (`test_issue_265.py`) |
| 2026-11-07 | a controlled live refresh disabled Show all between pointer-down and pointer-up, losing a read-only expansion | keep the expansion available during refresh and verify both response orders (`open-asks-refresh-click.spec.ts`) |
| 2026-11-06 | doctor reported a readiness refusal but still called PATH Cargo, or checked one installation and called another | gate metadata on this invocation's successful readiness flag and use its verified direct Cargo with command-local auto-install disabled; keep the Serde validator and exact argv (`test_rust_readiness.py`) |
| 2026-11-05 | Cargo presence alone hides a wrong compiler or missing native components, and a Rustup proxy may install during setup | setup and doctor share `scripts/rust-readiness.sh`; resolve installed direct tools before fetch, disable Rustup auto-install during local discovery, and refuse compiler overrides (`test_rust_readiness.py`) |
| 2026-11-06 | strict eval key checks rejected valid YAML merge overrides | check explicit mapping keys before SafeLoader flattens merges; keep unsafe-tag and nested-duplicate controls (`test_eval_case_loading.py`) |
| 2026-11-06 | a missing eval document separator silently overwrote a complete case | use the private strict case loader and separate cases with `---`; preserve effective records when repairing boundaries (`test_eval_case_loading.py`) |
| 2026-11-07 | a synthetic changed test runner reused a unit-cache pass with the same repository key | always rerun the unit step until all effective runtime inputs are qualified (`test_unit_cache_runtime.py`) |
| 2026-11-07 | a project quiet period held unrelated asks, lost snoozes, or launched a stale reservation | keep held and snoozed state per ask, omit empty state on idle passes, recheck at launch under the project lock, and surface unconfirmed launches without blind retries (`test_issue_409.py`, `test_watch_idle.py`) |
| 2026-11-07 | profile update feedback tried to redact through a malformed or locked profile and failed | print only fixed content-free messages and allowed field names; never echo arguments or YAML errors (`test_issue_412.py`) |
| 2026-11-07 | a failed Git hook-path lookup looked like an absent setting and ran the installer | install only on Git's missing-value status; stop on other errors (`test_issue_327.py`) |
| 2026-11-07 | a synthetic daily-log selection exited 141 when its reader stopped before a large producer | consume the whole inbox while selecting only the first matching ID; keep producer failures visible (`test_e2e_us52_pipes.py`) |
| 2026-11-06 | the source-copy native controls compared modified files with an older index and reported a frozen binding mismatch | stage the reviewed source changes before running the gate; retain the original failed log and keep the byte and mode checks strict (`source_copy_metadata_probe_once`) |
| 2026-11-06 | eval CLI cases left the last synthetic data root active before labels were read | restore the caller environment after each case, and reject data-root cache and label paths before reading or writing (`test_issue_423_eval_output.py`) |
| 2026-11-06 | a story failed with an empty log and the runner discarded its status | report the child status beside the failed log; an absent receipt stays unavailable, and a number does not prove signal provenance (`test_e2e_runner.py`) |
| 2026-11-06 | a PM draft reload test passed its empty notice check before fresh data arrived, or reloaded while the save was still in flight | wait for the matching save response, then fresh Day log data and the exact saved text; retain the notice assertion (`pm-copy.spec.ts`) |
| 2026-11-06 | setup story reported a missing health line that its output contained | avoid `echo | grep -q` under `pipefail`; check the captured output with a here-string and keep the missing-line control (`test_setup_story_health_check.py`) |
| 2026-11-06 | a source-health expiry story sampled its baseline while initial Day log requests could still write | wait for both Today loaders' fresh markers and a shared check that both pages' observed requests have finished before the baseline; retain the expiry and later-write assertions (`health-setup.spec.ts`, `live-source-health.spec.ts`) |
| 2026-11-06 | bulk encryption wrapped existing backup and sync bundles, and keys could be placed inside the data folder | refuse key and bundle destinations inside the data root; check for existing raw Fernet bundles before changing files or the encryption marker (`test_issue_423_keys.py`, `test_issue_423_bundles.py`) |
| 2026-11-05 | independent Git fixtures produce different commit fields, and a nested-root comparison misses an engine-source path | fix both author and committer dates; normalize only exact owned source/fixture root prefixes, preserving commit and status values (`test_issue_385.py`) |
| 2026-11-06 | a new reader default changed 16 DATA argument digests; an invalid synthetic target then masked the intended target comparison | use the committed current-only binding revision and an admitted alternate fixture target; derive expected metadata from immutable inputs, retain actual observations and historical references, and qualify native controls separately (`2026-11-06-data-current-bindings.md`) |
| 2026-11-06 | an end-of-day clock story stopped on missing feed coverage before its wrap-up assertions | declare quiet synthetic direct coverage with a legal TTL covering the fixture clock span; preserve no-source failures and every original clock assertion (`us-106-end-of-day.sh`) |
| 2026-11-06 | adding a Python module made the source-copy metadata probe fail before copying | census current Python sources plus the ten fixed inputs for the current probe count; preserve historical reference provenance counts and hashes (`source_copy_metadata_probe_once`) |
| 2026-11-06 | stored direct replies did not certify configured coverage or reach Today; stopped adapters looked like a successful pass | complete one declared batch under the live-cycle lock after releasing the feed lock; use the read-only health result, preserve physical stored counts and source-specific gaps, and keep row review and proof ages separate (`test_issue_406.py`) |
| 2026-11-06 | a changed action help string masked provenance negatives behind the frozen catalogue comparison | update only the source-derived help field in CLI and HTTP references and their hashes; preserve all negative controls and HTTP lengths when the byte length is unchanged |
| 2026-11-05 | unchanged required inbox replies failed each loop; accepting cached good replies could hide malformed current bytes | validate current bytes before reuse, bind receipt to source/config/content/file identity, and expire acquisition without renewing on poll; stamp synthetic file mtimes to the test clock; sample actual payload validation, then recheck all health after postprocessing to catch in-cycle refresh/expiry (`test_issue_408.py`) |
| 2026-11-06 | diagnostic source movement left activation-table declarations stale | refresh recorded source-line cells and duplicated expected-line data after a full census; retain partial graph scope, source and checker logic (`activation_manifest`) |
| 2026-11-05 | scaling validation lost process failure details and reported growth n^None | carry owner-generated diagnostic metadata through `run` into `validate`; never infer CPU exhaustion from SIGKILL or echo child text in the new summary (`test_exercise_diagnostics.py`) |
| 2026-11-05 | the install-test terminal helper retained its parent-owned master after EOF or EIO | close only that master in finally before the existing wait; preserve foreign descriptors, output and status (`test_terminal_master_fd_closes_*`) |
| 2026-11-05 | two comfort phone tests left manual browser contexts polling after they passed | use a lazy test-scoped screen fixture with finally cleanup for only its own contexts, including newPage failure; the last browser-only probe must find zero (`comfort-screen.test.ts`, `comfort.spec.ts`) |
| 2026-11-05 | a direct Serde derive dependency could be absent or mismatched while doctor only checked Cargo presence | validate locked offline no-dependency metadata without building; keep malformed metadata and Cargo failure actionable (`test_doctor_rust_deps.py`) |
| 2026-11-05 | a short strace follow flag also changes the output-separation setting | use independent long follow and output options; keep actual PID filenames in both runs and omit only follow for the negative control |
| 2026-11-05 | fixed -yy traces decorate AT_FDCWD with the known working directory while small fixtures used a bare token | support only the exact declared /work decoration at required marker operations; replay full byte-pinned traces offline before another container and retain wrong/malformed annotations as refusals |
| 2026-11-05 | Docker cp exited zero but exported no files from the running tmpfs | use the pinned bounded exec exporter; retain safe peers from valid incomplete manifests before cleanup and keep transport success separate from kernel-event qualification |
| 2026-11-05 | Docker typed-container absence differs from generic object-inspect errors | require exit 1, a parsed empty JSON array and the entire typed-container error for the exact owned ID or name; bind the CLI and daemon and reject unknown protocol forms |
| 2026-11-05 | an idle watcher expected the busy status format and never submitted its run | read the installed queue protocol; match its one free-status line with no queued entries, and keep the owning caller alive while a queued job waits |
| 2026-11-05 | an arbitrary missing-child parser error hid unsafe parent-only traces | credit only typed expected child incompleteness after parent execution, causal wait-zero and effect validation; malformed or unsafe traces fail qualification |
| 2026-11-05 | a stopped container cannot retain its tmpfs trace for later copy | keep the fixed observer supervisor alive until bounded copy, then require actual tracer/container waits and exact owned-resource absence; pass remaining cleanup time to the shared process owner |
| 2026-11-05 | static strace fixtures assumed plain descriptor paths and IP text under `-xx` | bind the installed help and upstream encoder before freezing parser fixtures; require exact hex strings and reject truncated or unsupported records |
| 2026-11-05 | non-creating PM audit reads moved source declarations and call text beyond activation metadata anchors | refresh only reviewed source locations and exact call text; preserve owners, edges and validator, then run the activation controls (`activation_manifest`) |
| 2026-11-05 | repeated synthetic PM bodies made a matching-body-elsewhere flag look like proof of a misdated record | preserve original fixtures and coverage; register distinct-body controls before execution and report the flag as an observation only (`test_pm_sent_coverage.py`) |
| 2026-11-05 | an outside-script Node version probe differed from the gate and scratch launcher after shell startup | pin the inherited startup file and observe the actual script, scratch-HOME and worker tool routes; preserve the original CHECK and launcher instead of silently switching runtimes |
| 2026-11-05 | a browser-fixture group-liveness check rethrew EPERM without the process-snapshot rejection reason | retain bounded content-free classifications and documented state flags before rethrowing the same error; unknown state does not prove an E suffix, and a standalone pass does not explain the earlier failure (`test_mcp_browser_fixture.py`) |
| 2026-11-05 | the HTTP capture saw a readiness file before its JSON write finished | close an exclusively created sibling temp file before renaming it to `ready.json`; keep published malformed JSON strict and the paused-writer control (`xtask/tests/capture_http.rs`) |
| 2026-11-05 | a copied environment had the right `careerbrew.pth`, but pytest imported another checkout | regenerate console-script entrypoints in the new checkout with `uv sync --locked --offline --reinstall`; check the pytest shebang and actual imported module paths before running the gate |
| 2026-11-05 | DATA controls passed through a test-only include while the ordinary executable still omitted the module | test the compiled default executable; retain meaningful command attestation and typed owner evidence, and compile genuine control-only helpers only for tests |
| 2026-11-05 | a copied HTTP source fixture used `/var` while loader evidence reported `/private/var` | canonicalize only the supplied capture root once; keep actual loader paths and source hashes strict, with wrong-root and changed-source rejection controls |
| 2026-11-04 | a Rust cache fixture reaped its Git commit but still found the process group | disable automatic Git maintenance for that synthetic commit. Keep the process-group cleanup check (`test_rust_cache_primary`) |
| 2026-11-05 | a retained proof checkout has package metadata but missing package source; CLI import fails at `click.command`, then pytest cannot import `pluggy.HookimplMarker` | restore that checkout's dependencies with `uv sync --locked --offline --reinstall`, verify the CLI and pytest imports, then replay the gate; preserve the failed logs and leave the lockfile unchanged |
| 2026-11-05 | sent learners for different days, or a sent learner and web save, can replace each other's shared PM history | hold the data-root history lock across read, merge and replacement; keep sent-day then history lock order and exclude only the exact empty history lock from the business version (`test_pm_history_concurrency.py`, `test_pm_sent_cache.py`) |
| 2026-11-02 | a second Playwright server changed the config shape and a constant test title hid a case from story checks | keep the ordinary server first, check its own readiness line, and write literal test titles for the coverage scanner |
| 2026-11-02 | Python format, lint and unit success hid the corresponding native checks in a mixed-manifest fixture | keep Rust checks distinct and fresh, rerun Python checks, and retain the native failure oracles (`test_gate_cache.py`) |
| 2026-11-02 | copying a documented `private/` path into `data show` looked inside a second private folder | remove one exact leading `private/` only at the show boundary; keep the shared resolver, containment and business policy unchanged (`test_issue_366.py`) |
| 2026-11-02 | a missing optional job-source credential aborts after free sources fetched but before their postings save | catch only `MissingSecretError` at the source fetch; keep warnings in the CLI invocation, preserve all-missing snapshots and propagate policy/provider errors (`test_issue_326.py`) |
| 2026-11-02 | the first sent-source read creates a control lock and the next unchanged API request misses its cache | keep canonical empty sent-day locks out of the business version; preserve sent text, receipt, nonempty, near-name and other-directory inputs (`test_pm_sent_cache.py`) |
| 2026-11-02 | scratch gate tests use another tool after a shell startup file changes PATH | a test child that relies on PATH stubs omits `BASH_ENV` and `ENV`; keep the host startup configuration unchanged (`test_inherited_shell_startup_keeps_fixture_tools`) |
| 2026-11-01 | a temporary Playwright proof spec outside `web/` failed before launch with `exports is not defined in ES module scope` when it imported the app's ESM helpers | give the temporary proof folder its own `package.json` with `"type": "module"`, matching `web/package.json`; keep its test data and output outside owner data |
| 2026-10-18 | `git init` and `uv` fail with "Operation not permitted" in the agent sandbox | run git init and uv commands outside the sandbox (they write `.git/config` and `~/.cache/uv`) |
| 2026-10-18 | a tampered backup with appended bytes still verified | Fernet's decoder ignores trailing junk; `backup.verify` decodes base64 strictly first (test: `test_backup_with_appended_bytes_fails_verification`) |
| 2026-10-18 | the gate passed before the first commit, then the privacy e2e failed | `scripts/leak-check.sh` scans tracked files only; with nothing tracked it scans 0 files. Always re-run `scripts/check.sh` after the first commit |
| 2026-10-18 | intermittent `FileNotFoundError` on `watch.json.tmp`, and a pause could be lost | two processes shared one temp name and raced read-modify-write; `store` now uses unique temp files and watcher state goes through the locked `update_state` (test: `test_state_concurrency.py`) |
| 2026-10-19 | `NaN` and `Infinity` hours passed a `<= 0` check and crashed planning | `float("nan") <= 0` is False; validate with `priorities.hours` (finite, 0.05–200) and read stored estimates with `priorities.estimate` (test: `test_regrade.py`) |
| 2026-10-19 | a hand-added source vanished after `import-research.py` | `knowledge/sources.yaml`, `knowledge/companies/` and the optional batch output next to them (`sources-extra.yaml`, `companies-extra/`) are generated; add facts to `knowledge/research/*.yaml` only |
| 2026-10-19 | a grader's probes wrote test data into the real `private/` | `$(mktemp -d)` failed in the sandbox and left `CAREERBREW_HOME` empty; an empty value is now an error (test: `test_empty_careerbrew_home_is_an_error_not_the_real_data`) |
| 2026-10-20 | the gate failed at `cards public-check` when the machine role was `corporate` | a gate step that checks repo content must not depend on the machine role; list it in `cli.REPO_CHECKS` (test: `test_gate_corporate.py`) |
| 2026-10-20 | after the clone moved, `check.sh` failed with `Failed to spawn: pytest` while doctor passed | `.venv/bin/*` keep absolute shebangs to the old folder; `uv sync --reinstall` (setup.sh does it; doctor checks it) |
| 2026-10-21 | a push from a worktree set `core.bare=true` on the main checkout and added junk commits | the pre-push hook exports `GIT_DIR`; `scripts/check.sh` now unsets `git rev-parse --local-env-vars` first |
| 2026-10-21 | Today's day log stayed on "Loading…" in Playwright after a goal was set | `exercises.load` re-parsed every YAML file per call; it now caches by path, size, and mtime (test: `test_exercises_cache.py`) |
| 2026-10-21 | the teacher's "Steps today" showed 0 every local evening | rows are stamped in UTC but were compared with the local date; compare by local day (test: `test_today_counts_steps_by_local_day_not_utc_day`) |
| 2026-10-22 | a story script's Playwright test saw the story's own projects, not the web seed's | `lib.sh` exports `CAREERBREW_HOME`, and `web-seed-and-serve.sh` keeps a set one; run Playwright from a story with `env -u CAREERBREW_HOME -u CAREERBREW_NOW` (see `us-60-project-import.sh`). `us-39` hit it too: its CLI review made "Reviewed today" 3, and the test passed only when it polled before the last grade landed (check: `test_e2e_lib.py`) |
| 2026-10-22 | every `git push` took 3–4 minutes right after a green `scripts/check.sh` | the pre-push hook (dotfiles `core.hooksPath` → `hooks/pre-push` → lefthook) runs `scripts/check.sh fast`; its cache uses file content and adds `HEAD` for `secrets`. `leaks` always runs because private terms can change outside the tracked tree. The key copies the index with `cp -p` (a fresh index mtime hides same-second edits from git) (test: `test_gate_cache.py`). To time the hook, push a branch with changed files: lefthook skips pre-push commands when the pushed range has no files |
| 2026-10-22 | `us-53` failed "missing" on text that was in the output (the actions list, about 4,000 lines) | `printf \| grep -q` under `pipefail`: grep exits on the match, printf gets SIGPIPE, and the pipe fails; `refute` could also miss a leak. `expect` and `refute` in `tests/e2e/lib.sh` use a here-string (test: `test_e2e_lib.py`). Do not pipe a large output into `grep -q` in a story |
| 2026-10-22 | a new reader got another test's answer for the same endpoint | HTTP fixtures are keyed by host and path only (the query string is ignored), so two scenarios on one endpoint need separate folders: the work feed's live in `tests/fixtures/http/feed/` |
| 2026-10-22 | a test that turns `notify_events` on could pop up a real notification: terminal-notifier may be installed and `osascript` is in `/usr/bin` | put the fake notifier first on PATH and set `notify_backend` to it (`fake` fixture in `test_issue_74.py`, `us-62-notifications.sh`); a named backend that is missing shows nothing instead of falling back |
| 2026-10-22 | the `exercises` gate step failed with `unique-paths ... growth n^2.54` (limit 2.5) at load ~50, or with `copy-list-with-random-pointer ... growth n^None` at load ~150, and a rerun passed | the scaling check timed wall time, so a long run's wait for a CPU read as growth, and its 15 s wall limit cut off a run with under 3 s of CPU. It times CPU time (`time.process_time`) now, and the scaling run's wall limit (`SCALE_WALL_SECONDS`, 120 s) only stops a hang; its own CPU limit (`SCALE_CPU_SECONDS`, 20 s since 2026-10-25; the test run keeps `CPU_SECONDS`, 6 s) stops a slow one (tests in `test_exercise_complexity.py`). A check that judges speed in the gate must not use wall time |
| 2026-10-25 | the `exercises` gate step failed with `word-search-ii ... growth n^None` at load 40 to 130, 3 of 3 runs alone too | not load: the scaling run's 15 timed runs share one process, and the reference solution needs 7.3 s of CPU in all, past the 6 s one-run limit (`CPU_SECONDS`). The scaling run has its own budget, `SCALE_CPU_SECONDS` (20 s) (test: `test_the_scaling_runs_together_may_use_more_cpu_than_one_run`). Before you call a gate step a load flake, run it alone and measure its CPU time against its limit |
| 2026-10-29 | `test_two_sizes_become_three_and_each_size_runs_several_times` failed in every full gate (`assert 2 == 3`, scale `too slow`), on main too | the copy-list-with-random-pointer reference needs 13.7 s of CPU for its scaling run at load 4, and the gate's 8 workers pushed it past `SCALE_CPU_SECONDS` (20 s). The limit is 60 s now; the slope check, not the CPU limit, catches a wrong complexity class. It then failed 1 of 4 gates on the wall limit (`time limit: the code ran longer than 120 s`, read from the result's `reason`), so `SCALE_WALL_SECONDS` is 600 s: the CPU limit stops a loop, the wall limit only a run that hangs without CPU. Measure a scaling run's CPU with `exercises._child` directly before you call it a flake |
| 2026-10-25 | six server tests failed "the web app did not start on port N: another program may use it" in a gate at load ~125, and passed alone | a cold `careerbrew serve` took 1.5 to 2.4 s alone at load 84, and the start waited only 50 tries of 0.1 s (about 5 s); `start_background` now waits up to `START_WAIT_SECONDS` (20 s) while the new process lives, and a process that cannot bind its port still fails at once (test: `test_slow_start.py`) |
| 2026-10-25 | all 18 browser stories failed "Timed out waiting 60000ms from config.webServer" in a gate at load ~250 | `tests/e2e/web-seed-and-serve.sh` made ~30 CLI processes before it served, 50 s alone at load ~100; `web/playwright.config.ts` waits up to 180 s now (readiness is still the server's own address line), and the seed runs every step in one process (`tests/e2e/web_seed.py`, ~1 s at load 5). A wall-clock limit in a test is a hang guard: size it for a busy machine, at 3x or more the time alone |
| 2026-10-22 | after a rebase, `web/e2e/feed.spec.ts` saw 4 open asks instead of 2 | every Playwright spec shares one seed (`tests/e2e/web_seed.py`, run by `web-seed-and-serve.sh`): a feed event another story seeds (#74's page and DM) is an open ask too. Name the seeded rows a spec expects, and add a fixture after `notify check` when it must stay out of the notifications |
| 2026-10-22 | `careerbrew open` said "another program may use it" while the program on 8765 was an untracked careerbrew server | `start_background` now stops a careerbrew server on its port that `day/web.json` does not track; a test that calls it must use a free OS port or stub `server.listening_pids`, or it stops another worktree's server (test: `test_issue_118.py`) |
| 2026-10-22 | an LLM eval answer from a local qwen3 model did not finish in 300 s | Ollama's OpenAI endpoint lets a thinking model think first; the judge sends `reasoning_effort: none` (Ollama routes) and `max_tokens` (test: `test_run_answers_and_judges_one_criterion_per_call_on_the_local_route_then_caches`) |
| 2026-10-22 | the open Today and Day log views refetched every 5 s with no new data, and a version-keyed cache never hit | reading `/api/today` and `/api/daylog` rewrote `day/plan-<day>.json` and `day/log-<day>.md` (a new ciphertext and mtime each time), which moved `/api/changes`. A file a read rebuilds is written with `store.write_text_if_changed` / `write_json_if_changed` (test: `test_reading_a_view_does_not_move_the_change_version`). Time the API with `scripts/api-timing.py`, which also checks the version |
| 2026-10-22 | `/api/changes` moved every 300 s with no new data, so live views refetched and cached views rebuilt | each watcher cycle rewrote `day/cursors.json`, `day/watch.json`, `day/notifications.json`, and (with project memory sync on) `work/memory-sync.json`. A file a cycle writes is either written only on change (`store.write_json_if_changed`) or, when it holds only bookkeeping that no cached or live view reads, listed in `server.CHANGE_SKIP` (tests: `test_watch_idle.py`, which also fails when a cached or live view reads a skipped file) |
| 2026-10-22 | an agent's `status:` line appended at the end of a project memory file was never read | the end of the file sat under careerbrew's last `##` notes header; `memory_sync` records digests of the lines careerbrew wrote; only a file with no record falls back to its headers plus the exact shape each writer produces (test: `test_an_owner_line_appended_right_after_careerbrews_notes_is_read`) |
| 2026-10-22 | removing a personal note made a `0600` agent notes file `0644`, and could drop an agent's append | a temp file plus `replace` takes the umask's mode and the text read earlier; `memory_sync._rewrite` copies the mode, reads the file again right before the swap, carries over an append, and skips any other change (tests: `test_removing_a_note_keeps_the_files_mode`, `test_an_agent_append_during_a_removal_is_kept`) |
| 2026-10-22 | a standup talking point said "decode file names" for "decode UTF-8 file names" | any `[A-Z]+-\d+` regex reads `UTF-8` and `ISO-8601` as ticket keys (`project_priority.TICKET` is one); read keys only for configured projects: `standup.key_pattern` for standup rows, `project_priority.ticket_pattern()` everywhere else, never a new key regex (tests: `test_ticket_keys_are_read_only_for_configured_projects`, `test_a_utf_8_commit_is_other_work_with_no_ticket`, `test_utf_8_and_iso_8601_stay_in_the_pm_update`) |
| 2026-10-22 | e2e stories run in parallel now: `us-01` failed with tar "File removed before we read it" (3 of 3 beside churning `web/test-results`), and `us-35` missed a 5 s Playwright wait beside the other browser stories (4 of 5 runs) and also alone at load ~85 (1 of 5) | a story that copies the repo copies only what a fresh clone has (`us-01` excludes `test-results` and `__pycache__`); a Playwright story calls `browser_story` from `lib.sh` for its own port and output folder (`scripts/e2e.sh` fails without it). A wait that fails only under load is CPU work behind it: `us-35` hit two 1 s endpoints (the actions catalog built the CLI parser 133 times; the deck used pure-Python YAML), fixed in code (tests: `test_action_parity.py`, `test_public_cards.py`). Time the first call of each endpoint on a fresh server first; a `# e2e: serial: <reason>` line (runs alone after the batch, `test_e2e_runner.py`) is the last resort |
| 2026-10-22 | browser tests failed on `Failed to load resource: net::ERR_CONNECTION_RESET` once the Dashboard asked for its 9 blocks at once | the stdlib server's listen backlog is 5, and a burst overflowed it on a busy machine; `server.Server` sets `request_queue_size = 128` (test: `test_a_burst_of_block_requests_is_never_reset`, 78 of 200 resets before) |
| 2026-10-22 | a Playwright test waited for a view's loading skeleton and never saw it | the page caches each view's last answer (issue #142): a view the page opened before shows its data at once, from memory or the tab's `sessionStorage`. To see a cold load, open another view first in a new page, or remove the `careerbrew-cache*` keys and reload (`web/e2e/feedback.spec.ts` Dashboard) |
| 2026-10-22 | `signin.spec.ts` found no sign-in cookie right after Today's "Next step" heading showed (1 of 2 gate runs) | since issue #142 a block's heading shows before its data and before the page's own requests answer; a heading is not "the page is done". Wait for the thing itself: `expect.poll` the cookie, or wait for the block's data text |
| 2026-10-22 | parallel `playwright test --list` runs on a cold cache read 2 to 25 tests too few, with exit 0 and no error | Playwright 1.59 writes compile-cache entries in place, and a parallel run can read one empty; `browser_story` gives each story its own `PWTEST_CACHE_DIR`, and `pw_total` names a spec the list left out (test: `test_e2e_lib.py`; docs/web.md) |
| 2026-10-25 | the pre-push gate failed every Sunday evening on `test_metrics_share`: `evidence_items` was 0 | the weekly file picked its week by the UTC date (already Monday) but read notes by their local date (Sunday); `metrics_share` now uses the local date for both. A week or day window takes `calendar.now()` and converts each stamp to `calendar.local_tz()` first (test: `test_a_sunday_evening_note_counts_in_the_local_week_not_the_utc_week`) |
| 2026-10-26 | the gate hung for 2 hours at 99% of the unit tests, and a repeat `/api/daylog` read missed its cache | a test's `while kind != "exercise"` loop never ended once the exercise waited 3 days (cap such loops; `test_teacher.py`); the day log view wrote a new `day/pm-shown-<day>.json`, which moved the change version: bookkeeping only a save reads goes in `server.CHANGE_SKIP_PREFIXES` (test: `test_watch_idle.py`). Find a unit hang with one `timeout 180 pytest <file>` per file |
| 2026-10-27 | the checkup's gate failed `leaks` on main on a file no one had changed: an author name in a public citation | a private term had been added since; the scan prints only the term id. Find the match without opening the term list: scan a scratch copy one line, then one word, at a time with `careerbrew privacy scan --file`, and reword it |
| 2026-10-27 | in a fresh worktree the gate failed `unit` with `ERR_MODULE_NOT_FOUND` from `test_e2e_lib.py` | the worktree had no `web/node_modules`; run `scripts/setup.sh` in a new worktree first. The gate now stops at once with that fix line (test: `test_issue_221.py`) |
| 2026-10-27 | two agent sessions that updated different projects at once lost one update in `work/projects.yaml` (issue #230) | every load-change-save of the file runs under its `.projects.yaml.lock` (`projects.locked()`, the `@_writes` decorator on each writer, and `project_import.run`), so project writes are safe from parallel sessions. A new writer of `projects.yaml` takes `@_writes` (test: `test_issue_230.py`) |
| 2026-10-27 | after the Hallmark restyle moved spacing onto a 4 px scale, the comfort budget failed on phone: a small-text link's tap area was 43.6 px | the 44 px came from 19.6 px of text plus 2 x 13 px padding; the scale's 12 px fell short. The tap-area rule in `web/src/styles.css` now sets `min-height: 44px` itself. Run `tests/e2e/us-76-comfort-budget.sh` after any spacing or font-size change, and audit with `/hallmark audit web/` (`docs/design/`) |
| 2026-10-28 | three grind workers sat idle for about 3 hours with commits but no PR, while the check-in said "progressing" | each ended its turn to wait for a background gate's completion notice; the gate shells were killed, so no notice came. Workers now wait for the gate's `rc=` line in the foreground, and the lead watches them with agentbrew run-goal `scripts/stalled-agents.py` (`commands/careerbrew-grind.md`). Judge a worker by its last transcript write, never by the `heavy-run` lock holder |
| 2026-10-28 | the 5 watcher-start tests (`test_day.py`, `test_hot_reload.py`) timed out on `_wait(watch.running)` in the full unit run, and passed alone | the run was at nice 20: a zsh background job adds 5 and `heavy-run` adds 15, and at nice 20 macOS refuses even `os.nice(0)`, so every watcher child died with `PermissionError` (its `logs/watcher.log` under `--basetemp` showed it). Read the nice level with `os.getpriority(os.PRIO_PROCESS, 0)`, never `os.nice(0)` (test: `test_issue_280.py`). When a spawned helper "never starts", read its log before you call it slow |
| 2026-10-28 | a gate-cache test saw no key change after editing `TASKS.md` in a scratch repo | a global gitignore (`core.excludesFile`) can ignore `TASKS.md`, and `scripts/input-key.sh` hashes untracked files with `--exclude-standard`, so an untracked `TASKS.md` is invisible to the key. In the repo it is tracked, so edits count; in a test repo, `git add -f` it first (`test_a_notes_only_change_skips_the_steps_whose_inputs_leave_it_out`) |
| 2026-10-18 | `sed -i ''` fails on this Mac (GNU sed on PATH) | edit files with Python or the Edit tool, not `sed -i` |
| 2026-10-22 | every merge to main made every open PR conflict on GitHub, though `.gitattributes` set `merge=union` on the learnings log | GitHub ignores merge drivers when it decides whether a PR can merge, so a file every PR appends to always conflicts. Each learning is its own file in `docs/learnings/` now; the gate's `learnings` step fails on a row written into `docs/learnings.md` (test: `test_two_prs_that_each_add_a_learning_merge_without_a_conflict`) |
| 2026-10-22 | a visual picture differed on every server start | the API built a list from a Python set, whose order changes with each process's hash seed; sort it (test: `test_a_company_page_lists_its_sources_in_one_order_every_start`) |
| 2026-10-22 | under the pinned clock, Review showed today's plan as the last workday's and evidence days old | a default of `datetime.now()` or `date.today()` reads the wall clock; use `calendar.now()`, or `calendar.now_utc()` for a stamp stored in UTC (tests: `tests/unit/test_pinned_clock.py`). Now a check: `tests/unit/test_clock_reads.py` fails on a new wall-clock read in `src/careerbrew`; a real timer goes in its `ALLOWED` list with a reason |
| 2026-10-22 | a second `page.route` patch of one URL in a visual test did nothing | `route.fetch()` skips the other route handlers; `patch()` in `web/visual/helpers.ts` runs every patch of a URL in one handler |
| 2026-10-22 | Playwright tests failed on "Blocked script execution in 'about:srcdoc'" once the Reports view showed a run page in a sandboxed frame | Playwright's trace recorder injects a script into every frame and Chrome blocks it there; the specs ignore that one message (`TRACE_IN_SANDBOX` in `web/playwright.config.ts`); the run page itself has no script (`test_run_pages.py`) |
| 2026-10-22 | a new Playwright spec passed alone (its story script runs it alone) but failed in the full `npx playwright test` | all specs share one seeded server (`workers: 1`, files in name order), and an earlier spec leaves state behind (`app.spec.ts` saves its own PM edit). Reset what a spec reads in its `beforeEach` (`pm-copy.spec.ts` posts an empty `/api/pm/draft`), and run the full suite before you say a spec is done |
| 2026-10-22 | an e2e or unit test that greps a long run's step lines (`FAIL  ...`, `pass  ...`, the plan lines) finds only two lines | with `run_pages` on (the default) a long run prints only its page path and one summary line; assert the summary line, or read the page as text with `careerbrew report latest` (`done     ...`, `blocker  ...`, `next N. ...`) |
| 2026-10-22 | the repo checkup page said "No open work" while the wrap-up skill listed open work | the skill prints indented `  <kind>     <text>` lines, not `- ` lines; `runs.SKILL_ITEM` parses them (test: `test_open_work_from_the_wrap_up_skill_is_a_blocker_with_next_steps`) |
| 2026-10-22 | a test that runs `scripts/checkup.sh` would run the whole gate (and the e2e suite inside it) | run a copy of the script in a temp repo with stub `scripts/*.sh` and a `.venv` symlink (`test_run_pages.py::_stub_repo`, `us-67-run-pages.sh`) |
| 2026-10-22 | a block's new "What is this?" line broke `getByText(/^Notifications are off/)` | Playwright matches an element's whole text: a line put before a callout's own text changes how that text starts. Put the line after the text (`About` in `web/src/about.tsx`) |
| 2026-10-19 | every Playwright test fails with "Executable doesn't exist ... chromium_headless_shell-1217" in a cloud sandbox | the sandbox ships one Chromium at `/opt/pw-browsers/chromium`. Make a scratch dir with `chromium_headless_shell-1217/chrome-headless-shell-linux64/chrome-headless-shell` symlinked to it (plus an empty `INSTALLATION_COMPLETE`) and run with `PLAYWRIGHT_BROWSERS_PATH=<dir>`; that also covers the story scripts. Do not run `playwright install` |
| 2026-10-22 | a browser story's Library check found no study cards, though the same spec passed in a direct `npx playwright test` | `tests/e2e/lib.sh` points `CAREERBREW_PUBLIC_CARDS` at an empty folder; a story whose browser test needs the public deck runs Playwright with `env -u CAREERBREW_PUBLIC_CARDS` (US-35, US-58, US-72, US-73) |
| 2026-10-22 | a browser test of a cross-site request to the API saw no request at all, then no `Cookie` or `Sec-Fetch-Site` header on it | a page careerbrew serves carries its CSP (`default-src 'self'`), which blocks the fetch before it leaves: serve the foreign page from a plain server in the test. A response that `Cross-Origin-Resource-Policy` blocks shows Playwright only the provisional request headers: assert what arrived from the server's content-free event (`web.session` `cookie`, `code`) (test: `web/e2e/signin.spec.ts`) |
| 2026-10-22 | Playwright tests passed only on the seed's default day: on a later `CAREERBREW_NOW` a fixed goal date was in the past, and on a weekend the Today test found no "Team standup" | derive dates from `NOW` in `web/playwright.config.ts`; `npm run lint` fails on a fixed date in `web/e2e/`. Lint cannot see a weekday rule: the calendar fixture's standup repeats Monday to Friday, so a workday-only check asks `isWorkday(TODAY)` first (US-35 runs Today on a Saturday) |
| 2026-10-22 | `npx playwright test` in a fresh worktree times out "waiting 60000ms from config.webServer" | the server shows no page until the web app is built: run `npm run build` in `web/` first (the gate builds it before e2e) |
| 2026-10-22 | the open asks browser test failed a count after Mark personal (4 asks in a gate; 2 for 3 when reproduced) | two Playwright runs on one port shared one server: the run whose server lost the port passed the old `url` check on the other run's server and tested that run's data (7 of 10 paired runs at 1d2e642). The config now waits for its own server's address line (`wait.stdout`; test: `test_playwright_server.py`); still give each checkout its own `CAREERBREW_E2E_PORT` |
| 2026-10-22 | a Playwright test passed alone but failed in the whole suite: an earlier file had approved the seed's only goal suggestion | the whole suite shares one seeded server, files in name order; a test sets up its own state (an agent task, an approval) or patches a GET answer, and never needs a seed item another file acts on |
| 2026-10-22 | Setup's "Turn on" in a test started a real watcher whose first cycle, at the pinned 09:00, would add notifications the later files do not expect | the watcher test moves `work_start`/`work_end` past the pinned clock for its run, then unsets them; `getByLabel(/^.../)` missed a label whose text starts with a space, so match the checkbox with `getByRole("checkbox", { name })` |
| 2026-10-22 | the Goals form kept the lists of the page's cached answer after the fresh one arrived, so a save dropped goals the CLI or an agent had added; the first fix, a key on `q.cached`, remounted the form and dropped what the owner had typed (e2e "confirm a lesson": "give the goal a title") | a form inside a cached block never keys on freshness or on a value each read changes (a step's `started`, a regenerated PM draft): it takes each new answer's server lists during render and keeps the draft (`GoalsForm`, `PmReport`, `stepId` in `teach.tsx`; tests: `web/src/teach.test.tsx`, `inputs.spec.ts` "Goals: a title typed…" and "Goals: goals added elsewhere…"). A test that patches a GET answer waits for the patched response, not for a heading: the cache may show the same heading first (`showStep` in `inputs.spec.ts`); the control inventory waits for no "refreshing…" note before it lists controls |
| 2026-10-22 | the comfort budget passed, yet a blind judge saw "goal:promotion" spill over the Week columns and "0.571" break over two lines in Review | the sideways check sees only boxes that scroll; text that overflows a `visible` box (an inline-block id, an inline code) or a number cell that wraps is the spill check in `web/e2e/comfort.spec.ts`. It needs the crowded state (a goal set, cards graded), so `us-76-comfort-budget.sh` runs three `app.spec.ts` tests first, as the gate does |
| 2026-10-23 | an approved picture kept a sticky phone bar's faint edge after the bar became static, and `npm run visual` still passed | `toHaveScreenshot` counts only pixels past its colour threshold (0.2), so a faint grey edge reads as equal and `--update-snapshots=changed` never rewrites it: after a layout change, redraw with `npx playwright test -c playwright.visual.config.ts --update-snapshots=all` and read what changed (25 stale pictures found this way) |
| 2026-10-23 | the checkup was always blocked by "55 open work item(s)", 50 of them branches of merged PRs | GitHub squash-merges, so a branch's commits never reach `main`; `scripts/open-work.sh` skips a branch whose tip is a PR's head merged into `main` (gh), whose whole diff is one commit on `main` (patch id) that no later commit reverts, or that merges with no change. `git merge-tree` alone missed 49 of 50: `main` had changed the same lines since (test: `test_a_squash_merged_branch_is_not_open_work`) |
| 2026-10-28 | the checkup was blocked by 5 branches whose work had landed as other PRs (split into #190 to #192, or reworded since), and `open-work.sh` printed `? unpushed commit(s)` | no patch id, merged head, or merge matches such a branch; `scripts/open-work.sh` tags it `likely landed (N% of its added lines are on main)` when `main` gained 90% or more of them since the fork (`OPEN_WORK_LANDED_PCT`; a move or reindent gains nothing, and a branch with unpushed commits is never tagged), keeps it listed, and prints a real count, with `never pushed` for a branch with no upstream (test: `test_a_branch_that_landed_through_other_prs_is_likely_landed_and_counts_are_numbers`) |
| 2026-10-29 | a never-pushed branch whose work landed reworded (`fix/183-184-186`, 62% of its lines on main) blocked every checkup, and only a forced branch delete, which agent hooks block, cleared it | a share cannot tell reworded work from new work. Its `open-work.sh` line names a retire command, `git tag archive/<branch> <branch>`, and the checkup page offers it; a branch whose tip matches its `archive/` tag is hidden (a `(retired: ...)` line), and a new commit lists it again. Run it only after you check the work landed; `git tag -d archive/<branch>` undoes it (tests: `test_a_never_pushed_branch_can_be_retired_with_an_archive_tag_and_shows_again_on_a_new_commit`, `test_a_never_pushed_branch_offers_a_retire_command_and_a_retired_one_does_not_block`) |
| 2026-10-29 | `test_open_work_lists_names_and_counts_only` ran `open-work.sh` on the real repo and timed out at 300 s in the gate (131 local refs; 69 s alone), blocking a TASKS.md-only push | a test that reads machine state grows with it; the test now builds a scratch repo with one feature ref (5 s) (test: `test_open_work_lists_names_and_counts_only`) |
| 2026-10-22 | US-75 failed in every checkout but the one that drew the pictures: Plugins was 492x139, its picture 492x140 | the seed's `stt_command` holds the checkout's path, and Settings on Setup shows it above Plugins. A longer path wraps one more line (20.25 px, not a whole pixel), so Plugins moved by part of a pixel and the element picture rounded to another height. The visual fixture pins the value, and `shot` fails when a page shows the checkout or the seed's temp folder (`machinePaths` in `web/visual/helpers.ts`). To prove a picture is stable, run US-75 from two worktrees with paths of different length |
| 2026-11-01 | a green gate missed scalar MCP entries and malformed header maps; preflight could report false health or raise outside its host boundary | strict preflight validates enabled entries and string header maps, skips exact `enabled: false` first, and keeps URL checks inside the sanitized host diagnostic boundary; permissive discovery stays unchanged (`test_issue_312.py`) |
| 2026-11-01 | the start prompt named Codex, but the doctor had no agent CLI or credential-state check | the existing doctor entrypoint probes configured clients only, bounds native status queries to two seconds, and prints enums without raw account output; preserve shell-only use and opt-outs, and never treat credentials as service access (`test_runtime_doctor_reports_safe_native_status`) |
| 2026-11-01 | `/careerbrew-checkup` exceeded agentbrew's 8,000 command limit after adding host guidance | the lint's chars label measures UTF-8 bytes; keep the source within 8,000 bytes and preserve workflow/privacy/memory clauses (`test_checkup_command.py`), then sync instead of editing deployed copies |

## Delivery

1. Branch `type/short-description` from `main`.
2. Write the task's Hypothesis, Measurement, Success, and Pivot before the code.
3. Make the change with its tests and docs. Run the tests you changed, or `scripts/check.sh fast`, while you work. Push as soon as the change is reviewable: a push runs only the leak and secret scans (seconds), and the hosted checks run the full gate on clean runners ([`docs/ci.md`](docs/ci.md)). A local full gate (`scripts/check.sh`, or `scripts/batch-gate.sh` for up to 4 PRs) is optional; use it only when the hosted checks are unavailable.
4. Commit with a conventional header (`feat:`, `fix:`, `docs:`, `chore:`).
5. Open a PR with `## Summary`, `## Details`, `## Docs`, and Predicted / Observed / Lesson. The lesson goes in the PR body under Predicted / Observed / Lesson; add no file to the learnings folder under `docs/` (the public tree leaves that folder out, and the `public-readiness` check fails a pull request that adds one). Merge when the hosted checks are green (`gh pr checks <pr>`); read a red check with `gh run view <run-id> --log-failed` before any fix.

## Task Management

Tasks live in `TASKS.md` ([tasks.md format](https://github.com/tasksmd/tasks.md)).
Before you pick a task, run `scripts/open-work.sh` (also part of `scripts/checkup.sh`). Continue any open PR or unmerged branch first: get it green and mergeable, or wrap it up as a draft PR that lists what is pending, with its task in `TASKS.md`. Only then pick the highest-priority unblocked task, claim it, and remove its block when it is done.

## Docs upkeep

- A behavior change updates its doc in the same PR. The routing table names the doc.
- A gotcha that cost time goes into `## Gotchas`, or becomes a test when a machine can check it.
- A new top-level `docs/*.md` file gets a row in [`docs/README.md`](docs/README.md).

## Agent host portability

Read [docs/agent-hosts.md](docs/agent-hosts.md) before changing agent workflows. Every story declares `dual-host-shared-memory`; commands and skills carry the matching marker. The story gate rejects omissions. Preserve the parser-generated action registry and Careerbrew data ownership. Plugin command source paths belong to the private fragment; agentbrew alone renders host outputs. Native transcripts use the normalized adapter, never Claude-only liveness inference.

Test deployment helpers use a test-owned bin folder and scratch client HOME. Keep
e2e CLI HOME scoped to the invocation so the browser runner retains its installed
binary cache. Do not read owner agent metadata in synthetic stories.
The Python seeder/server child uses the same synthetic client HOME and explicit
agent-config paths. Keep the browser parent's HOME unchanged. Setup stories must
also pin CAREERBREW_BIN_DIR and prepend its child PATH: HOME alone does not stop
an inherited bin override from replacing the owner's launcher. Do not pre-seed
agent installation state to make a fresh setup pass. Print captured command
failures before returning their exact child exit status; set-e alone loses output.

Declare published workflow sources in Agentfile commands/skillDirectories. The
story checker and doctor share its own-checkout inventory; keep explicit source
roles, exact-file selection and current-path ownership checks when extending it.
Runtime work steps use that same invocation-owned inventory before host mirrors.
Recheck an owned path before reading its body. Keep explicit roots and plugin
origins separate; never widen a selected file or guess a native wrapper name.
Codex prompts are a legacy resource fallback, not a native GUI command route.
Nested legacy names use selected-directory resource scope. Keep this fallback
separate from direct-only publication; exact-file declarations never widen.
The raw resolver rejects empty/absolute names and dot path components before
joining paths. Do not rely only on upstream normalization at this read boundary.
Ordinary dotted names and internal empty colon segments remain compatible.
Implicit nested resources also stay inside the current resolved selected
directory at the body read. Preserve explicitly selected aliases; do not apply
the implicit directory restriction to canonical entries or old host roots.

Signal-only watcher tests pin off-hours and use the default interval. An explicit
interval forces work cycles and measures unrelated source latency. Wait for the
child's started_at marker before signaling; a held lock alone is not readiness.

Published skills load careerbrew-best-practices first. Put their host contract
before the final Iterate section; appending another section breaks that rule.

Visual fixtures must pin measurements before persisted report generation, not
only in a browser API patch. `CAREERBREW_VISUAL_SEED=1` pins synthetic watcher CPU
to 120 ms; the functional browser config never sets it. Keep real CPU checks and
their thresholds unchanged. Review each changed golden against its actual old
image; do not infer faint text from a difference image.

Normal web unit tests bound their file-worker pool to one or two available CPUs
in vite.config.ts. Keep waits and assertions unchanged when calibrating load.
Use `UNIT_JOBS=2 E2E_JOBS=2 CHECK_EVALS=1 heavy-run scripts/check.sh` for the full gate on
this Mac; that invocation does not change the E2E runner default.

Sent PM learning must run even when a watcher cycle adds no result rows. Slack
edits already replace encrypted feed originals; use their created local day.
Only exact app-send receipts may supersede saved sent text. Manual `day pm-sent`
clears that receipt. Keep recipient checks on existing raw metadata and keep
the two-distinct-day automatic lesson threshold.


Sent PM text and its receipt share the canonical sent-day lock at
`day/.pm-sent-<day>.lock`. Public save, read and learn calls acquire it;
already-locked callers use `_save_sent`, `_pm_sent_text` or `_learn_sent`.
Never open another flock handle while holding that lock. A preview may create
the empty control lock file, but must not rewrite source, receipt or promise
metadata. When integrating PM331, its locked promise reconcile caller must use
`_pm_sent_text`; preserve sent-day then project-file lock ordering.

Late work-day jobs (#339): each active /work turn runs `careerbrew day loop catch-up --json`. Goal-hook activity is a host-supplied invocation boolean; never infer it from installed hooks. Move or hold today’s wrap-up with `day wrap`; explicit owner `day end` overrides the hold. Project sessions need a handoff newer than their graceful wrap request before archival. Tests pin clocks and use synthetic sessions.

Work-day freshness (#330, #305): run `careerbrew day start --update-only --json` before building a plan. Re-read changed workflow sources after an update. Preflight diagnoses stale installed commands and lost steps; preserve owner edits before source sync. Tests use scratch git remotes and fake repair commands, never the owner launcher or checkouts.

The optional `day start --fix --yes` repair changes Claude connector settings. Refuse agentbrew-registered settings and change their source instead. Ask the owner before using it on owner-managed real settings; a failed preflight is not approval. Tests must pin `CAREERBREW_CLAUDE_CONFIG` to a scratch symlink and use a fake gh command. Preserve the symlink target and mode, never inspect a live token, and prove live authentication through the intended client after reconnecting.
