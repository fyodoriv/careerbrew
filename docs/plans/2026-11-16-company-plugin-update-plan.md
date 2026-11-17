# Company plugin update plan: adopt the 2026-11 core

**Task**: `company-plugin-adopt-core-2026-11` in `TASKS.md` (P0).
**Who runs it**: a person or agent on a company machine, in the company
plugin repo (`careerbrew-<org-overlay>`). The personal Mac never touches that
repo. Nothing company-specific ever comes back into this public repo.
**Contract**: [plugins.md](../plugins.md) (manifest and policy),
[corporate-machine.md](../corporate-machine.md) (setup and `machine ready`),
[ci.md](../ci.md) (hosted checks), [rust-migration.md](../rust-migration.md).
**Base commit read**: `main` at `480acf34` (2026-11-16).

This file is in `docs/plans/`, which the public release tree leaves out
(`config/public-release-exclude.txt`). Do not link to it from a public file.

## Why

Between 2026-11-10 and 2026-11-16 the core changed in five ways. A plugin that
does nothing keeps working, but:

| Area | What changed in the core | What a plugin misses if it does nothing |
|---|---|---|
| (a) Public repo | The core is public. Its leak gates fail closed. | Nothing breaks. A company term pasted into a core issue or PR leaks. |
| (b) Tests | Tests run on hosted CI only. A PreToolUse hook `hooks/block-local-tests.sh` is in flight. | Agents on the company Mac run heavy local suites, or get blocked with no plugin-side CI. |
| (c) Telemetry | `policy.telemetry: false` is the only plugin switch. The company log sink is a proposal. | Nothing, if the plugin forbids telemetry. A plugin that adds `telemetry_sink` now gets no effect. |
| (d) Rust | No Rust runs in production. `rust-07` (live policy port) is planned. | Nothing today. When `rust-07` lands, a manifest that only Python accepts may fail. |
| (e) CI cache | No gate cache between hosted runs (measured: under 2% saving). A follow-up measurement is in flight. | Nothing. Do not add an `actions/cache` step on the plugin's word alone. |

## Verified manifest and policy fields

Read from the code at `480acf34`. Only these fields exist. Any other key in
`careerbrew-plugin.yaml` is ignored (or refused, for `provides` and `live`).

### Top-level manifest keys

| Key | Where it is read | What it does |
|---|---|---|
| `name` | `src/careerbrew/plugins.py:210` | must match `^[a-z][a-z0-9-]{1,39}$` |
| `version` | `src/careerbrew/plugins.py:213` | required |
| `requires` | `src/careerbrew/plugins.py:215` | minimum careerbrew version |
| `provides` | `src/careerbrew/plugins.py:218`, kinds at `:43` | `ladders`, `job_sources`, `rules`, `prompts`, `skills`, `evidence_sources`, `knowledge`, `rubrics`, `commands`; other kinds refused |
| `policy` | `src/careerbrew/plugins.py:232` | must be a mapping; keys below |
| `connector_defaults` | `src/careerbrew/plugins.py:235` | kind to field map |
| `agent_reads` | `src/careerbrew/plugins.py:238` | list of connector kinds |
| `live` | `src/careerbrew/plugins.py:241` | only `sources` and `team`, files inside the plugin |
| `work_steps` | `src/careerbrew/plugins.py:251` | validated by `workday.steps` |
| `extends` | `src/careerbrew/plugins.py:282` | base plugin name |
| `settings_defaults` | `src/careerbrew/plugins.py:298` | setting defaults |
| `sensitive_terms` | `src/careerbrew/plugins.py:437` | terms file blocked from every exit |
| `detect` | `src/careerbrew/plugins.py:521` | `env:` signals |
| `checkup` | `src/careerbrew/checkup.py:152` | extra checkup steps, only with `--allow-exec` |

### Policy keys

| Key | Where it is read | Merge rule |
|---|---|---|
| `allowed_routes` | `src/careerbrew/plugins.py:479`, `:493` | intersection |
| `allowed_connectors` | `src/careerbrew/plugins.py:480`, `:493`; `src/careerbrew/machine.py:354` | intersection |
| `allowed_hosts` | `src/careerbrew/plugins.py:481`, `:493` | intersection |
| `secret_backends` | `src/careerbrew/plugins.py:482`, `:493` | intersection |
| `require_encrypted_backup` | `src/careerbrew/plugins.py:483` | any true wins |
| `backup_targets` | `src/careerbrew/plugins.py:484` | intersection |
| `export_to_personal` | `src/careerbrew/plugins.py:485`, `:498` | lowest of `none`, `aggregates`, `redacted`, `full` |
| `agent_may_read_business` | `src/careerbrew/plugins.py:486` | true only if one says true and none says false |
| `public_metrics` | `src/careerbrew/plugins.py:487` | any false wins |
| `telemetry` | `src/careerbrew/telemetry.py:339` | any `false` blocks every send |
| `metrics_share` | `src/careerbrew/metrics_share.py:39` | any `false` forbids the weekly file |

**Not a field**: `policy.telemetry_sink`. It exists only as the P0 task
`telemetry-plugin-sink` in `TASKS.md`. `src/careerbrew/telemetry.py:255`
says so in a comment. Do not add it to a plugin yet.

## Before you start

1. A core commit at or after `480acf34`: `git -C <careerbrew-checkout> merge-base --is-ancestor 480acf34 HEAD` exits 0.
2. The baseline count: run `uv run careerbrew machine ready` and write down the `corporate readiness: N/N` line. This is the "before" number.
3. `uv run careerbrew plugin verify` prints each plugin as ok. If not, stop: the installed tree differs from the lock.

## Step (a): public repo and leak gate

Goal: plugin content stays out of the base repo; the plugin repo checks its own
private pattern.

1. Confirm the plugin repo is private on the company host: open its settings page on `github.example.com` and check "Private".
2. Add a private-pattern file to the plugin repo, for example `config/private-pattern.txt` (company names, codenames, internal hosts). It lives only in the plugin repo.
3. Add a check script `scripts/leak-check.sh` to the plugin repo. It reads that file and greps every file that may leave the company (an issue draft, a report to the core). Print only a count, never the match.
4. Never copy a plugin file, a host, or a term into a core issue, PR, or test. Use `<org-overlay>`, `*.example.com`, `PROJ-123`.
5. Before you file a core issue from the company Mac, run `careerbrew privacy scan --file <draft>`. It uses the plugin's `sensitive_terms`.

**Verify**: `bash scripts/leak-check.sh <draft>` prints `0`; `careerbrew privacy scan --file <draft>` passes; `uv run careerbrew machine ready` still shows the baseline N/N.

**Rollback**: remove the script and the pattern file in one revert commit. No core change is involved.

## Step (b): hosted CI only and the block-local-tests hook

Facts on the core:

- The merge gate is the hosted checks: `gate (ubuntu-24.04)`, `gate (macos-15)`, `public-readiness` ([ci.md](../ci.md)).
- A PreToolUse hook `hooks/block-local-tests.sh` is in flight in a PR. It is not on `main` at `480acf34` (`ls hooks/` shows only `lefthook`, `pre-commit`, `pre-push`). When it lands, it blocks an agent's local test run. The opt-out is `CAREERBREW_ALLOW_LOCAL_TESTS=1`.

Steps for the plugin:

1. Add a hosted CI workflow to the plugin repo on the company's CI runners. It should:
   - check out the plugin and a pinned core commit;
   - run `uv run careerbrew plugin add <plugin-folder>` in a scratch `CAREERBREW_HOME` (manifest validation is the test);
   - run `uv run careerbrew plugin verify`;
   - run the step (a) leak check;
   - pin every action by full commit SHA, not by `@v4`.
2. Add a weekly job that does the same against the newest core `main`, so a core change that breaks the plugin shows early.
3. On the company Mac, do not run the core suite locally. When the hook lands, leave `CAREERBREW_ALLOW_LOCAL_TESTS` unset. Set it to `1` for one command only when hosted CI is down, and say so in the PR.
4. Do not run the core e2e suite in plugin CI. It needs no plugin and costs about 30 minutes.

**Verify**: the plugin PR's hosted checks are green (company CI page); `uv run careerbrew plugin verify` prints ok; after the hook lands, `rg -n CAREERBREW_ALLOW_LOCAL_TESTS hooks/block-local-tests.sh` in the core finds the opt-out.

**Rollback**: delete the workflow file. The hook is a core change: unset nothing, it has no plugin side.

## Step (c): telemetry

Today a plugin has one telemetry switch: `policy.telemetry: false`
(`src/careerbrew/telemetry.py:339`). It blocks every PostHog send on the
machine. `metrics_share: false` is a separate switch for the weekly metrics
file. A corporate machine starts at consent `no`.

1. Decide with the company privacy contact: may anonymous usage counts go to the core's PostHog project? If no, or if unsure, set `policy.telemetry: false`.
2. Keep `metrics_share: false` and `public_metrics: false` on a corporate plugin unless the company says otherwise.
3. Do not add `policy.telemetry_sink`. It is not a field yet. When task `telemetry-plugin-sink` lands, follow task `telemetry-plugin-sink-company` (its own steps: written approval, token in an allowed secret backend, `consent: ask` unless approved in writing).
4. Run `careerbrew plugin refresh`.

**Verify**: `uv run careerbrew telemetry status` names the plugin block ("a plugin policy forbids telemetry on this machine") when `telemetry: false`; `uv run careerbrew telemetry send --dry-run` sends nothing; `uv run careerbrew machine ready` shows the baseline N/N.

**Rollback**: remove the `telemetry` line and run `careerbrew plugin refresh`. Consent stays `no` on a corporate machine, so nothing sends until the user says yes.

## Step (d): Rust migration

Facts: no CLI command, HTTP route, or watcher calls Rust
([rust-migration.md](../rust-migration.md#current-state)). The app runs on
Python. `rust-07` ports the live plugin and machine policy checks; it is
blocked behind `rust-00` to `rust-05`.

1. Today: no change. Do not install a Rust toolchain for the plugin.
2. Make the plugin safe for `rust-07` now: use only the fields in the tables above. Remove any unknown key. A stricter native validator may refuse what Python ignores.
3. Keep every `provides` path relative, inside the plugin, with no symlink (`src/careerbrew/plugins.py:220` to `:230`).
4. When `rust-07` lands, its PR states the parity result. Rerun the step (b) plugin CI against that core commit before you update the company Mac.

**Verify**: `uv run careerbrew plugin add <plugin-folder>` in a scratch `CAREERBREW_HOME` succeeds; `rg -n 'rust-07' docs/rust-migration.md` in the core shows its state; `uv run careerbrew machine ready` shows the baseline N/N.

**Rollback**: pin the company Mac to the last core commit before `rust-07` (`git checkout <commit>` then `uv sync --locked`) until the plugin is fixed. The core keeps the Python handler as its own rollback.

## Step (e): CI cache decisions

Facts: [ci.md](../ci.md#no-gate-cache-between-hosted-runs) measured that a
hosted gate cache saves under 2%, so the core has none. A follow-up PR that
measures more cache options is in flight.

1. Plugin CI is small (step b). Add no cache step until a run takes over 5 minutes.
2. If you add one, key it on every input: the files, the runner image, the tool versions. Never cache `private/`, a secret, or the private-pattern file.
3. Use no `pull_request_target` and no cache saved from fork code.
4. When the core's cache PR merges, read its result in [ci.md](../ci.md). Change plugin CI only if that result applies to plugin-sized jobs.

**Verify**: the plugin workflow file has no `actions/cache` step, or its key names the image and tool versions; plugin CI stays green.

**Rollback**: delete the cache step.

## Done when

- `uv run careerbrew machine ready` prints `corporate readiness: N/N` with the same or higher N than the baseline.
- `uv run careerbrew plugin verify` prints ok for every plugin.
- Plugin CI is green on its pinned core and on the weekly newest-core run.
- The step (a) leak check prints `0` on every report sent back to the core.

## Report back to the core

Only two generic lines, after `careerbrew privacy scan --file <draft>` passes
and company policy allows it: "readiness before N/N, after N/N" and "plugin CI
on newest core: green or red". Never a company name, host, or file.
