# Hosted checks (CI)

Every pull request and every push to `main` runs the gate on clean GitHub
runners ([`.github/workflows/ci.yml`](../.github/workflows/ci.yml)).
**The hosted checks are the merge gate.** A pull request merges when
they are green. Nobody waits for a full gate on their own machine.

## What runs

| Check | Runner | Commands |
|---|---|---|
| `gate (ubuntu-24.04)` | `ubuntu-24.04` | green when all four parts of the Linux gate passed (see "Parallel parts") |
| `gate (macos-15)` | `macos-15` | the same four parts on macOS |
| `public-readiness` | `ubuntu-24.04` | `scripts/public-readiness.sh --released` ([workflow](../.github/workflows/public-readiness.yml)): fails on a path the public tree leaves out, a secret-shaped value, or a link to a left-out path; prints counts only |

## Parallel parts

Each OS runs the gate as four parts at the same time
([`gate-part.yml`](../.github/workflows/gate-part.yml), called by `ci.yml`).
Each part runs `scripts/setup.sh`, then `scripts/check.sh` with
`CHECK_STEPS`, the list of gate steps it runs:

| Part | `CHECK_STEPS` | Usual time (macOS) |
|---|---|---|
| `checks` | `docs` to `tasks`, `format`, `lint`, `types`, `unit`, `registry`, `public-cards`, `card-code`, the four `web-*` steps | about 10 min |
| `native` | `exercises` and the four Rust steps | about 10 min |
| `e2e-1` | `e2e` with `E2E_SHARD=1/2` | about 13 min |
| `e2e-2` | `e2e` with `E2E_SHARD=2/2` | about 9 min |

- `CHECK_STEPS="<step> ..."` runs only those steps of `scripts/check.sh`. A
  name that does not run (a typo, or a step whose tool is missing) fails it.
- `E2E_SHARD=i/n` runs one share of the story files. A story's line
  `# e2e: weight: <seconds>` gives its usual time on a hosted runner (no line
  means 10). The heaviest story goes first, each to the share with the least
  weight so far. US-72 (the whole browser suite, about 12 minutes) is the
  longest part, so it has a share almost to itself.
- `tests/unit/test_gate_parts.py` fails when the parts do not run every gate
  step exactly once, when a share is missing, or when the two OSes run
  different parts. Add a new gate step to one part in `ci.yml` (both lists).
- The required checks `gate (ubuntu-24.04)` and `gate (macos-15)` are small
  jobs that wait for their OS's parts (`if: always()`), so the branch rule on
  `main` did not change. A part that fails, is cancelled, or is skipped makes
  its OS's check red.
- When a story becomes much slower or faster, change its weight line to the
  time in the log line `e2e: ok <file> (<s> s)`.

Each part installs the tools a new machine lacks: uv, Node 24, the Rust version
that `rust-toolchain.toml` pins, gitleaks (a pinned release with a checked
SHA-256), and Playwright's Chromium. Then it runs the same two commands a
contributor runs.

The uv step pins the exact Python version, because the frozen HTTP reference
of the Rust capture tests holds it in a `Server` header
(`tests/unit/test_hosted_ci.py` keeps the two equal). The uv cache is off, so
uv uses its default cache path, which a capture test needs: it runs uv with a
cleared environment.

Each job also makes a local `main` branch (a pull request checkout is detached,
and one story bundles `main`), puts the pinned Rust tools first on `PATH` (not
the rustup proxies), and sets an empty telemetry key, so a runner sends no
usage data.

Every Playwright config sets `captureGitInfo: { commit: false, diff: false }`.
Without it, in a pull request on GitHub Actions, Playwright runs
`git fetch origin <base> --depth=1`. That makes the checkout shallow, and the
corporate rehearsal (US-40) then bundles a `main` that has no parents and
cannot be cloned.

The runners set `UNIT_JOBS=2` and `E2E_JOBS=2`: a standard runner has few CPUs.

## Work with the hosted checks

1. While you work, run lint, format and type checks only. Tests run on
   hosted CI only (see "Tests run on hosted CI only" below).
2. Push when the change is reviewable. The pre-push hook takes seconds: it
   checks the push context, runs the leak scan, and runs gitleaks on every
   commit the push adds. It runs no test.
3. Open the pull request. Wait with `gh run watch <run-id> --interval 30` in
   the background and do other work.
4. Merge when `gh pr checks <pr>` shows all three checks green.

The lesson of a change goes in the pull request body (Predicted / Observed /
Lesson). Do not add a file to the learnings folder under `docs/`: the public
tree leaves that folder out, so `public-readiness` fails that pull request.

## Tests run on hosted CI only

The `hooks/block-local-tests.sh` hook (Claude Code PreToolUse on Bash,
declared in `Agentfile.yaml`) blocks a local test run in a careerbrew
checkout: pytest, cargo test or nextest, `scripts/check.sh`,
`scripts/e2e.sh`, `scripts/batch-gate.sh`, `tests/e2e/*.sh`,
`evals/run.sh`, and the web tests (`npm test`, `npm run visual*`, vitest,
`playwright test`), also through heavy-run, xargs, npx, bash -c or eval.
Lint, format, type checks and builds stay allowed. Read the result with
`gh pr checks <pr> --watch` and `gh run view <run-id> --log-failed`.

When the hosted checks are unavailable, export
`CAREERBREW_ALLOW_LOCAL_TESTS=1` before the agent starts. Setting it inside a
command does not work: the hook reads its own environment. The hook's own
test is `bash hooks/block-local-tests.test.sh`
(`tests/unit/test_block_local_tests_hook.py` runs it in the gate).

agentbrew keys a hook by its event, matcher and agents. A later layered
Agentfile (for example a company overlay) that declares its own PreToolUse /
Bash hook with the same key replaces this entry, so keep the guard in that
file too. Static matching also has limits: it cannot see indirection such as
`x=pytest; $x`, base64 text piped into `sh`, or a shell alias.

The checkup (`scripts/checkup.sh`) runs no gate either. It reads the hosted
result for the commit `main` points at now with `scripts/hosted-gate.sh`: the
push runs of both merge workflows, `ci.yml` and `public-readiness.yml`.
- Both green: it passes.
- A red run: it names `gh run view <id> --log-failed`.
- A run still going, or no run yet: it is pending and names `gh run watch <id>`.
- No `gh`, signed out, or offline: unavailable.

Only the first one is green. A green run for an older commit never counts.
`CHECKUP_LOCAL_GATE=1 scripts/checkup.sh` runs the full gate on this machine
instead.

## Safety rules

Keep every rule when you change a workflow.

- Use only the standard runners `ubuntu-24.04` and `macos-15`. Never a larger runner.
- Use no repository secret.
- Never use the `pull_request_target` trigger. A fork pull request must get a read-only token.
- Keep `permissions: contents: read`.
- Upload no artifact. A log or a trace would be a public download. One
  exception: `migration-measure.yml` uploads the four content-free migration
  records of each step, and nothing else (see "Batched migration measurement";
  `tests/unit/test_migration_records.py` fails when another workflow uploads one).
- Never copy `private/`, a real `.env`, or the owner's private-pattern file to a runner.
- Pin every action to a full 40-character commit SHA, with the version in a trailing comment.
- Install a tool from a release only with a pinned version and a checked SHA-256. Never `curl | sh`.

## What the hosted gate cannot check

Two checks stay local.

**The darwin pictures (US-75).** The `*-darwin.png` pictures in
`web/visual/__screenshots__` are drawn on a developer Mac and compared with no
pixel of slack. A hosted macOS runner draws each screen 50 to 160 pixels off
(run 43493131555: 176 of 196 visual tests failed that way). So the macOS job
checks that every story has its pictures and that Playwright lists every
visual test, but compares none. After a change to a screen, run
`cd web && npm run build && npm run visual` on your Mac. The Linux job
compares its own pictures: see "Approved pictures on the Linux runner".

**The private pattern.** The owner's private-pattern file lives only on the owner's machine. So the
private-pattern leak check runs only in the local git hooks. The hosted gate
still runs the repo's own privacy scan (`scripts/leak-check.sh`) and gitleaks.

## Approved pictures on the Linux runner

`gate (ubuntu-24.04)` compares every screen of the visual tests (US-75) with
its `*-linux.png` picture, with no pixel of slack (`maxDiffPixels: 0`). The
workflow sets `CAREERBREW_VISUAL_PICTURES=hosted` for that. The pictures are
drawn on the same runner image, so the same fonts draw and compare.

- **Draw or redraw them**: the manual workflow
  [`visual-pictures.yml`](../.github/workflows/visual-pictures.yml), then
  `scripts/visual-pictures.sh fetch <run-id>`. Steps: [`docs/web.md`](web.md),
  "Linux pictures". The workflow uploads no artifact: it prints each new or
  changed picture in its log as base64 with a SHA-256. The pictures show
  synthetic test data only. All 204 pictures make a log of about 20 MB.
- **Noise floor of a migration step**: the manual workflow
  [`noise-floor.yml`](../.github/workflows/noise-floor.yml) baselines one
  step twice on each hosted OS and prints the proposed floors to the log:
  `gh workflow run noise-floor.yml -f step=rust-03`.
- **What drew the pictures** (the `visual-environment` lines of run
  43605737771, 2026-11-14): image `ubuntu24 20261111.127.1`, Playwright
  1.59.1, Liberation Sans (font version 137625) for text, DejaVu Sans Mono
  (155320) for code.
- **When every Linux picture fails at once**: the image got another font, or
  Playwright another Chromium. Start the drawing run with `-f update=all`,
  compare its `visual-environment` lines with the ones above, read the
  pictures, commit them, and update the lines above.

Measured on 2026-11-14 (task `hosted-visual-pictures`): three runs on three
runner machines drew the same pixels for all 196 visual tests (runs
43605737771, 43606212585 and 43606558768). A picture with 144 pixels painted
black on purpose failed a compare-only run (43606807817: `1 failed`, `105
pixels ... are different`). A drawing
run takes about 3 to 5 minutes.

## Batched migration measurement

The owner decided on 2026-11-27 that the 30-run `baseline` and the paired
`measure` of a Rust migration step no longer block its merge
([`docs/rust-migration.md`](rust-migration.md), "Measurement protocol for
every step"). The manual workflow
[`migration-measure.yml`](../.github/workflows/migration-measure.yml) runs
them for many steps at once.

- **Start a run**: `gh workflow run migration-measure.yml --ref <branch> -f steps="rust-14 rust-15"`.
  Inputs: `steps` (empty means every step with a registered candidate in
  `xtask/src/measure.rs`), `pairs` (default 30; fewer adds the test-only
  `--test-allow-few-pairs` flag) and `macos` (default off; on adds a
  `macos-15` job per step).
- **What a job does**: one job per step and OS. It builds xtask and every bin
  of the package that holds the step's candidate (some verifies need a helper
  next to it, such as rust-22's `exercise-fixture-runner`), with the profile
  the candidate's registry path names, then runs
  `scripts/migration-records.sh run`: `baseline`, `verify`, `measure --pairs N`
  and `decide`, each into a new file under the runner's temp folder, in fresh
  synthetic homes. A failed verify or a missing record fails the job; a
  `keep-python` verdict does not. The step sets `pipefail`, because it pipes
  the script into `tee` and would otherwise pass with records missing (run
  46069607808 did for rust-22).
- **The records**: each job uploads `baseline.json`, `verify.json`,
  `measure.json` and `decide.json` as the artifact `migration-<step>` (on macOS
  `migration-<step>-macos-15`), kept 30 days. They are content-free.
- **Fetch them**: `scripts/migration-records.sh fetch <run-id>` checks that the
  run is a `workflow_dispatch` run of this workflow in this repository,
  downloads each artifact into `.metrics/migration/<step>/` (macOS:
  `.metrics/migration/<step>/macos-15/`), refuses any other file or a record of
  another step, and prints one line per step:
  `rust-14 verify=pass decide=keep-python reasons=...`.
- **The step list** comes from the registry at run time
  (`scripts/migration-records.sh steps`), so a new step joins with no workflow
  edit. `tests/unit/test_migration_records.py` checks that the list equals the
  steps with a candidate in `xtask/src/baseline.rs`.

## Read a failed run

1. Find the run: `gh pr checks <pr>` or `gh run list --workflow ci.yml --limit 5`.
2. Read the failing step first: `gh run view <run-id> --log-failed`.
3. Reproduce the step on your machine before you change code.
4. A macOS job that was cancelled at its time limit with no log got no runner.
   Read `gh api repos/{owner}/{repo}/check-runs/<job-id>/annotations`, then
   `gh run rerun <run-id> --failed`.

Wait for a run with `gh run watch <run-id> --interval 30`.

## Cost rule

This project never spends money. Standard runners are free for a public
repository. If the repository becomes private, turn the workflow off first:
a macOS minute on a private repository is billed as 10.

## Measure it

- Result time of the last runs: `gh run list --workflow ci.yml --limit 10 --json conclusion,createdAt,updatedAt`. Target: under 20 minutes (before the parallel parts: a median of 39 minutes for a push to `main`, 2026-11-14).
- Open-to-merge time: `gh pr list --state merged --limit 10 --json number,createdAt,mergedAt`.
  Measured on 2026-11-14: the two pull requests opened after the hosted checks became the
  merge gate (#73, #74) merged 39 and 47 minutes after they were opened, which is one hosted
  run plus the merge. A before-and-after median is not possible. Five pull requests merged
  earlier in the public history: two (#66, #68) were opened after their local full gate had
  passed, so their open-to-merge time (under 10 seconds) leaves out the wait; three (#69 to
  #71) built the hosted gate itself and stayed open for hours of that work. The wait that the
  hosted checks removed is the local one: local full-gate runs for one merge fell from 1 to 0,
  and a push fell from 12 to 16 minutes under load (`AGENTS.md`, Gotchas) to under 10 seconds.
- Runner labels of one run: `gh run view <run-id> --json jobs -q '.jobs[].labels'`. Only `ubuntu-24.04` and `macos-15` may appear.
- Owner step, once after the first week: read the billing page and confirm 0 paid minutes. An agent cannot see that page.

## Dependency caches

Measured on 2026-11-16 with `workflow_dispatch` runs of `ci.yml`: a warm arm
(caches on) and a cold arm (caches off). Each number is the mean seconds per
run, ± one standard deviation, summed over the 4 parts of one OS. A cache's
total is its restore step, its Post (save) step, and the install it speeds up.
`scripts/setup.sh` runs both `uv sync` and `npm ci`, so the uv and npm caches
are judged together.

Rule: keep a cache only if warm saves at least 10% of its steps' time and the
gap is larger than one pooled standard deviation.

| Cache | OS | Warm mean ± sd | Cold mean ± sd | n (warm + cold) | Decision |
|---|---|---|---|---|---|
| uv + npm (setup-uv, setup-node, `scripts/setup.sh`) | ubuntu-24.04 | 82.5 ± 5.7 s | 77.6 ± 5.8 s | 4 + 5 | drop: no measurable gain |
| uv + npm (setup-uv, setup-node, `scripts/setup.sh`) | macos-15 | 116.2 ± 16.5 s | 109.4 ± 4.3 s | 5 + 5 | drop: no measurable gain |
| Playwright browsers (cache step + `playwright install`) | ubuntu-24.04 | 97.8 ± 19.4 s | 112.6 ± 8.2 s | 4 + 5 | keep: saves 13%, gap 14.8 s vs pooled sd 14.1 s (close to the line) |
| Playwright browsers (cache step + `playwright install`) | macos-15 | 30.8 ± 5.4 s | 39.6 ± 2.1 s | 5 + 5 | keep: saves 22% |
| Rust build, Swatinem/rust-cache trial (cache step + native gate step) | ubuntu-24.04 | 509.5 ± 23.8 s | 429.4 ± 38.4 s | 4 + 5 (insufficient; decision unaffected) | not added: no measurable gain |
| Rust build, Swatinem/rust-cache trial (cache step + native gate step) | macos-15 | 740.2 ± 67.6 s | 718.2 ± 21.4 s | 5 + 5 | not added: no measurable gain |

Run ids: warm 43992340679, 43992348423, 43992355392, 43992362019, 43992368437
(primed by 43989268939); cold 44011221761, 44011224538, 44011227846,
44011231568, 44011235377.

Limits of this measurement:

- The ubuntu uv + npm and Playwright warm rows leave out run 43992355392: its
  e2e-2 part failed at gitleaks and skipped those steps, so its sums were partial.
- The ubuntu Rust warm row leaves out run 43992362019: its native gate failed
  early at the Python `exercises` step, not at a Rust step.
- Some other warm runs failed on steps that were not timed. Their timed steps count.
- Whether each warm restore was a hit or a miss was not recorded.
- The arms were not interleaved: all warm runs started first, the cold runs
  about an hour later, so runner load may differ.

Re-measure:

1. On a branch, restore what a cache needs:
   - Playwright: nothing; `-f cold=true` skips its cache step.
   - uv: `enable-cache: true` on setup-uv, and the guarded step that links
     `$UV_CACHE_DIR` to `~/.cache/uv` (it exits early when `UV_CACHE_DIR` is empty).
   - npm: `cache: npm` and `cache-dependency-path: web/package-lock.json` on setup-node.
   - Rust: the trial step "Cache Rust build (trial)" in `gate-part.yml` at
     commit f3441ceff5fd, the head of warm run 43992340679
     (`Swatinem/rust-cache@d1527c8f67ef9af357290475d4af4b0fab8f12e0`, v2.9.3).
2. Run one warm priming run and discard it.
3. Run 5 warm and 5 cold, interleaved: `gh workflow run ci.yml --ref <branch>`
   and `gh workflow run ci.yml --ref <branch> -f cold=true`.
4. List them: `gh run list --workflow ci.yml --event workflow_dispatch --branch <branch>`.
5. Read each step's times with `gh run view <run-id> --json jobs`. Log each
   restore's cache-hit output (`gh run view <run-id> --log`). Drop a run whose
   timed steps were skipped.

## No gate cache between hosted runs

`scripts/check.sh` skips a step whose inputs already passed, and keeps that
record in `.metrics/cache/`. A runner starts with that folder empty. The
workflow does not save and restore it with `actions/cache`. This was measured
on 2026-11-14 and stopped before any code:

- Only 11 steps may reuse a pass: `docs`, `learnings`, `sources`, `stories`,
  `secrets`, `tasks`, `public-cards`, `card-code`, `web-lint`, `web-unit` and
  `web-build`. The other steps always run by design, because their inputs are
  not fully keyed (the list is at the top of `scripts/check.sh`; the reasons
  are in `AGENTS.md`, "Gotchas").
- In run 43571680438 (a push to `main`), those 11 steps took at most 29 seconds
  in each job. The times come from the log timestamps of the gate step:

| Part of the gate step | `ubuntu-24.04` | `macos-15` | May reuse a pass |
|---|---|---|---|
| `docs` to `tasks`, with `leaks`, `format`, `lint` | 4 s | 7 s | 6 of these 9 steps |
| `types` | 10 s | 13 s | no |
| `unit` | 357 s | 518 s | no |
| `public-cards`, `card-code` | 2 s | 2 s | yes |
| `exercises` | 204 s | 179 s | no |
| Rust steps | 350 s | 400 s | no |
| `web-types`, `web-lint`, `web-unit` | 18 s | 15 s | 2 of these 3 steps |
| `web-build` | 5 s | 5 s | yes |
| `e2e` | 824 s | 820 s | no |
| Whole job | 1824 s | 2011 s | at most 29 s (1.6% and 1.4%) |

- The task asked for a rerun at least 30% shorter. A full hit on every step
  that may hit saves under 2%, and the restore and save steps cost seconds
  too. So the cache step was not added.

Rules for a later try:

- A cached pass needs the same inputs. A step's key must cover every input:
  its files, the runner image and the tool versions. A pass on one image is not
  a pass on another. A step with unknown inputs always runs.
- Do not move a step off the always-run list to make a run faster. First
  qualify its runtime inputs, with a test that fails on a stale pass.
- GitHub scopes a cache entry by branch. A pull request reads its own entries
  and the entries of `main`. It writes only to its own merge ref, so a fork
  cannot change what `main` restores. Keep it so: no `pull_request_target`, and
  no workflow that saves a cache from fork code with a wider token.
- Never cache `private/`, a secret, or the owner's private-pattern file.
- `unit`, `exercises`, the Rust steps and `e2e` are 95% of the job. A faster
  hosted run must come from one of them.

## Branch rule on `main`

`main` requires the three checks. The owner approved this rule on 2026-11-13.
This call sets it (an administrator of the repository runs it):

```bash
gh api -X PUT repos/{owner}/{repo}/branches/main/protection --input - <<'JSON'
{
  "required_status_checks": {
    "strict": false,
    "checks": [
      {"context": "gate (ubuntu-24.04)"},
      {"context": "gate (macos-15)"},
      {"context": "public-readiness"}
    ]
  },
  "enforce_admins": false,
  "required_pull_request_reviews": null,
  "restrictions": null,
  "allow_force_pushes": false,
  "allow_deletions": false
}
JSON
```

- Read it: `gh api repos/{owner}/{repo}/branches/main/protection --jq '.required_status_checks.checks[].context'`.
- Remove it: `gh api -X DELETE repos/{owner}/{repo}/branches/main/protection`.
- `enforce_admins` is `false`, so the owner can still merge when GitHub itself
  is down. Agents never use that path: a red check is read and fixed.
- If a check name changes in a workflow, change it here in the same pull request.

## Action updates

`.github/dependabot.yml` opens a weekly pull request when a pinned action has
a newer release. Review the release notes, then merge on green checks.

## Change an action version by hand

Get the commit of a release tag, then replace the SHA and the comment:

```bash
gh api repos/<owner>/<action>/commits/<tag> --jq .sha
```
