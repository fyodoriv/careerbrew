# Rust migration plan

This page is the complete plan to move careerbrew's code outside the UI from
Python and shell to Rust. It is the one public source for the Rust tasks
`rust-00` to `rust-28` in [`TASKS.md`](../TASKS.md). A worker with only this
repository can do any step from this page and the task block.

- **Status:** in progress. One Rust command runs by default: `careerbrew
  actions` routes to `careerbrew-native` since `rust-03` activated
  (2026-11-19, [rust-03 verdict](#rust-03-verdict)). No other gain is claimed. See [Current state](#current-state) and
  [State of rust-00](#state-of-rust-00).
- **Next task:** `rust-14` (live listening, review asks and notifications).
  `rust-00` through `rust-13` have their recorded preparation or component
  results; only the `rust-03` action catalog is active in production. Follow-ups
  and later activation closure remain tasks in `TASKS.md`.
- **How to read it:** the [goal](#goal-and-vision) and [decision](#decision-and-rejected-alternatives),
  then the [measurement protocol](#measurement-protocol-for-every-step), then
  [build order versus production activation](#build-order-versus-production-activation),
  then your step in the [work breakdown](#work-breakdown).

## Contents

1. [Goal and vision](#goal-and-vision)
2. [Decision and rejected alternatives](#decision-and-rejected-alternatives)
3. [Evidence the plan starts from](#evidence-the-plan-starts-from)
4. [Scope](#scope)
5. [Boundary contracts](#boundary-contracts)
6. [Measurement protocol for every step](#measurement-protocol-for-every-step)
7. [Build order versus production activation](#build-order-versus-production-activation)
8. [Work breakdown](#work-breakdown) (`rust-00` to `rust-28`)
9. [Rollout, rollback, security and regression coverage](#rollout-rollback-security-and-regression-coverage)
10. [Source-map refresh rule](#source-map-refresh-rule)
11. [Current state](#current-state)
12. [State of rust-00](#state-of-rust-00)
13. [Every module has an owner step](#every-module-has-an-owner-step)
14. [Keep this page true](#keep-this-page-true)

## Goal and vision

- Replace careerbrew's first-party executable code outside the UI with Rust.
- Do it in small, measured steps.
- Keep these working the whole time: the private work record, the job-hunt
  tools, the teaching features, the CLI, the agents and the browser UI.
- The owner asked for this migration, and for one task per step.
- The product targets stay the work-mode and job-hunt outcome targets in
  [VISION](VISION.md). The language is not an outcome metric.
- This page plans the work. It does not claim a Rust port or a speed gain.

## Decision and rejected alternatives

**Decision:** replace one capability at a time, behind small adapters.

1. Build the measurement tools first (`rust-00`).
2. Prove the maintained Rust libraries next (`rust-01`).
3. Port one read-only CLI pilot (`rust-03`).
4. Port each complete writer transaction as one unit. Never port half a transaction.
5. Keep the current Python code as a separate **test oracle** until the last step.
6. Never run two production writers side by side ("shadow mode").

**Rejected alternatives:**

| Alternative | Why it is rejected |
| --- | --- |
| One big rewrite | It removes the working comparison target before parity is proved. |
| A Rust extension inside Python only | Python stays the app runtime. That does not meet the requested final state. |
| A new hosted service | It breaks the local-first vision. |
| Custom code for cryptography, recurrence rules or spaced repetition | Reuse maintained crates behind adapters instead. |
| One crate per old Python file | Too many crates. Use a small Cargo workspace. |

**Workspace shape:** a small Cargo workspace with a library, the CLI and server
entry points, and an `xtask` package for developer commands. See the
[Cargo workspace guide](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html)
and the [Cargo test reference](https://doc.rust-lang.org/cargo/commands/cargo-test.html).
`rust-01` picks the exact crates. `rust-28` picks the supported release
targets. Both decide from compatibility evidence and the repository's real
support contract.

## Evidence the plan starts from

| Claim | Source of truth | Status |
| --- | --- | --- |
| The app entry point is Python 3.12 or later | `pyproject.toml`, `src/careerbrew/cli.py` (the `careerbrew` console script) | fact |
| The runtime uses cryptography, FSRS, YAML and calendar recurrence libraries | `pyproject.toml` | fact |
| The UI is React and TypeScript, with browser tests | `web/package.json`, `web/src/`, `web/e2e/`, `web/visual/` | fact; the built UI calls the local HTTP server |
| File transactions use advisory locks and encrypted storage | `src/careerbrew/store.py`, `src/careerbrew/vault.py` | fact; the key is kept apart from the data root |
| Voice calls `whisper-cli` and `ffmpeg` | `src/careerbrew/tutor/voice.py` | fact; bounded external processes |
| The API timing tool reports timings, with no hard latency limits | `scripts/api-timing.py` | fact; cold and warm API calls, and the change-version probe |
| Exercise checks run Python solutions and scale generators | `src/careerbrew/tutor/exercises.py` (`validate`, `run`, `_child`); `knowledge/exercises/*.yaml` | fact; the normal `exercises` gate step |
| Card-code checks use `compile()` and an embedded Node and esbuild call | `scripts/check-card-code.py` | fact; the normal card-code gate step |
| `cargo xtask migration measure --pairs` runs paired cold and warm trials | [Current state](#current-state) | built; `rust-02` has the first registered candidate, with four vector sets (clock, hash, paths, settings) |
| No before and after Rust speed or memory result exists | each step records its own baseline | verify per step |

Pending changes and new modules count from their final landed versions. An old
snapshot must never replace a newer correctness fix.

## Scope

**Ported to Rust (in scope):**

- runtime modules, the CLI, and the local HTTP server;
- encryption and storage adapters;
- connectors, watchers, and the work-day runner;
- jobs, the teacher, and the exercise runner;
- model routes and voice adapters;
- report builders;
- developer and operations scripts;
- test helpers, backend tests, and evals;
- installation, packaging, and release checks;
- executable snippets embedded in other files (shell in docs and hooks, Node
  and shell helpers inside backend scripts).

**Kept as they are:**

- **UI in TypeScript:** React views, CSS, the Vite configuration, browser tests,
  visual tests, and UI-only tooling.
- **Report and dashboard HTML and CSS** stay UI. Their backend builders port.
- **Data stays data:** declarative skills, prompts, configuration, fixtures and
  docs. Executable examples in them change to the native commands.

**Exercise rules:**

- Canonical exercise reference solutions, scale generators and verification
  fixtures are implementation code. They port in `rust-22`.
- Python examples may stay as language-specific teaching data. An optional
  Python interpreter runs them only when the learner explicitly practises Python.
- At the end, normal app use, tests and maintenance must not need Python.
  `rust-27` checks the syntax of retained Python examples with a native parser.

**No silent exceptions:** no other Python or shell code is exempt. A minimal
first-run OS bootstrap may stay only with an explicit inventory reason. It may
only get and start the native tool.

**The source map:** the [code inventory](plans/2026-11-01-rust-code-inventory.csv)
assigns each tracked executable file to a task or to the UI exception.
`rust-00` extends it to executable bits, shebangs, code blocks, package hooks
and generated entry points. The gate rejects new unassigned code during the
whole migration. The [owner map](#every-module-has-an-owner-step) below adds
the data files that each step must keep valid.

## Boundary contracts

Each port must keep these contracts byte for byte or meaning for meaning.

- **CLI and action registry.** One registry owns names, arguments, defaults,
  help, exit codes, stdout, stderr and the discovery JSON. The UI and agents
  read that registry. Temporary adapters only route a capability. They never
  add a second action list. Product screens never show a backend choice.
- **HTTP.** Keep method, path, status, schemas, errors, cookies, authorization,
  allowed origins, cache keys and change-feed meaning. Browser-page caches keep
  their current lifecycle and reset rules. The product UI keeps its behaviour
  and look. Backend test launchers may change.
- **Storage.** The data-root transaction owns the lock and the write. Keep the
  current paths, byte and schema meanings, encryption envelope, key lifecycle,
  file modes, atomic replacement, cursors and version receipts. Check Fernet
  against its [specification](https://github.com/fernet/spec/blob/master/Spec.md).
  Crash, retry and mixed-engine tests must prove that both engines work on one
  data root. Do not infer it from both libraries supporting locks. One engine
  owns each mutation.
- **Source identity.** Keep exact URLs, configured project identities, original
  timestamps, message edits and sent-message receipts. Never widen an item to
  repository or channel scope. Never guess a recipient. Never generate owner
  labels automatically.
- **Policy.** Role and plugin policy decide reads, data exits and approvals
  before anything runs. Use synthetic fixtures only. Proving a port needs no
  live send and no private-data migration. Logs and results hold no business
  text and no secrets.
- **Processes.** Name the owner and lifetime of each process, server instance,
  request, data-root lock, browser page and watcher loop. Keep bounded retries,
  cancellation, cleanup and subprocess limits. Never start a Python fallback
  after a Rust mutation may have happened.

## Measurement protocol for every step

`rust-00` creates the commands below and tests them, with the `cargo xtask`
alias. Until it lands they are **proposed** commands, and every dependent task
stays blocked.

1. **Pre-register.** Before any port code, commit the hypothesis, the exact
   suite and fixtures, the numeric keep and pivot thresholds, the normalization
   rules and the source revision.
2. **Baseline.** Run `heavy-run --exclusive cargo xtask migration baseline --step rust-NN`
   just before the step. Store a content-free JSON record: baseline B, source
   revision, toolchains, machine, fixture hashes, cache state and raw samples.
   A changed relevant source hash makes the baseline invalid.
3. **Differential verification.** Run `heavy-run cargo xtask migration verify --step rust-NN`.
   It runs the CLI, HTTP, data and side-effect comparisons against the Python
   behaviour, the native unit and integration tests, and the step's user
   stories. The native run has Python denied. The Python oracle runs on its
   own. Only the explicit Python exercise case may opt in to Python.
4. **Paired resource runs.** Run `heavy-run --exclusive cargo xtask migration measure --step rust-NN --pairs 30`.
   - At least 30 fresh pairs for each cache regime (cold and warm).
   - Alternate or randomize which side runs first.
   - Keep cold and warm cases and their caches apart.
   - Collect process-tree CPU time, peak RSS, and wall-clock p50 and p95.
   - Count only real executions. A cached gate result is never a sample.
   - Timing runs take exclusive machine slots.
5. **Decide.** Run `cargo xtask migration decide --step rust-NN`. All three
   records must have the same full source revision, `evidence_sources_sha256`
   and `fixtures_sha256`; verify and measure must also have the same
   `candidate_sha256`. A missing, malformed or unequal identity is refused
   before a verdict is written. The common evidence hash covers the capture
   sources, `baseline.rs`, `resources.rs`, the step's sources and its pinned
   environment. Each producer's separate `sources_sha256` keeps its own wider
   or different harness coverage; those producer-specific values are not
   expected to match. The shared fixture hash covers the HTTP fixture tree and
   each exact path in `baseline::Step::fixtures`: today the four pure-vector
   JSON files for `rust-02` and the DATA corpus for `rust-05`. It never reads
   an unregistered fixture path.
6. **Close the step.** Update docs, the per-step coverage and the rollback
   evidence in the same pull request. Run the fast gate per commit. The hosted
   checks run the full gate before the step counts as done
   ([docs/ci.md](ci.md)). Keep the Python oracle and the original assertions
   until native equivalents pass.

### The limits (the decide step)

A step **keeps** only when all of these hold:

- 100% contract agreement;
- 0 privacy, authorization, role or side-effect violations;
- 0 missing writes and 0 duplicate writes;
- CPU-time p95 at most **1.10 x B**, and peak RSS at most **1.10 x B**, for
  each measurable baseline case.

**Any** contract disagreement, privacy violation, or missing or duplicate write
**blocks rollout**. Any failure holds the step. Then pivot the adapter or the
design, register the new approach, and measure again. Keep the last verified
handler meanwhile.

Rules of the decision (already built in `xtask/src/baseline_verdict.rs`, with
synthetic data only):

- Each decision covers one cache state. `migration decide` judges each
  measure regime, cold and warm, against that regime's own paired oracle
  samples. The warm baseline record feeds only the protocol checks.
- It needs at least 30 unique pairs. Neither side may come from a cache.
  29 pairs, a duplicate pair ID or a cached pair holds the step.
- Both sides are compared with frozen bytes, exit status, a nonempty set of
  exact executed check IDs, and the ordered writes. An empty expected check
  suite is an invalid oracle.
- Paths, stdout, stderr and written bytes are scanned for synthetic canaries.
  A canary holds the step, even when the oracle has it too.
- The verdict does not normalize bytes. A fixture may apply only its own
  pre-registered normalization before the comparison.
- CPU uses the nearest-rank p95 of samples in nanoseconds. Memory uses the
  maximum of the process-tree peak samples, in bytes. Exactly 110% keeps.
- **Noise floor:** at and above the registered floor of a baseline case, use
  the 110% ratio. Below it, use a pre-registered absolute ceiling, never a
  division. `rust-00` sets the real floors. The synthetic controls use a 100 ns
  CPU floor and a 1,000 byte RSS floor; those numbers are controls only.
- Wall time is reported, not judged. CPU time is the performance gate.

The eight registered verdict controls are: 30 unchanged pairs keep (also with
the check order reversed); changed stdout plus an unexpected write hold as
contract drift; a canary in both oracle and output holds; missing or duplicate
check IDs hold; a cached pair, a duplicate pair ID or 29 pairs hold; CPU p95 or
peak RSS over 110% holds (exactly 110% keeps); a below-floor baseline uses the
absolute ceiling; an empty check suite holds as an invalid oracle.

### The paired trial planner

`xtask/src/trial_plan.rs` fixes the run order. It does not start commands or
record results. Its six controls:

1. It refuses 0 and 29 pairs for either regime.
2. For 30 pairs it gives the unique IDs 1 to 30.
3. Cold pairs alternate baseline-first and candidate-first, with both sides in each pair.
4. For 31 pairs, the baseline-first and candidate-first counts differ by one in each regime.
5. Every row carries its cache regime. For one ID, cold and warm put different sides first.
6. Two independent iterators give the same plan, even when read together.

Pair IDs are scoped to one benchmark case and one cache regime. The capture
must set that scope and bind each observation to its execution.

### What the limits are not

- The numeric limits are registered targets, not measured results.
- New behaviour is not needed for parity.
- Never claim a blanket speedup.
- `rust-00` must prove that a planted slow path fails the decision.
- [Criterion](https://docs.rs/criterion/latest/criterion/) is a reuse
  candidate. Its default wall-time measurement alone does not meet the CPU-time
  rule.
- `rust-00` and `rust-01` are preparation experiments. Harness and dependency
  controls replace product speed claims there.
- A step can ship as several small pull requests. **Each** pull request records
  and measures its own slice.
- Task thresholds in the work breakdown add to these common rules. They never
  replace them.

Sources: Basili, Caldiera and Rombach 1994 (Goal Question Metric); Munafò et
al. 2017 (pre-registration); Kalibera and Jones 2013, *Rigorous Benchmarking in
Reasonable Time* (repeated, paired measurements).

## Build order versus production activation

**The rule:** the task order is the order to **build and measure** native
components. It is **not** permission to switch a whole Python module after one
unit test.

- A Rust component tested with synthetic typed providers, before its whole call
  closure is native, is **component parity only**. Label it so.
- Component parity only **never switches production dispatch**. The released
  Python capability stays the default.
- Never put a fake or Python provider inside a released native mutation.
- When the entire call closure is native, rerun the baseline, the full
  differential verification, the resource runs and the user stories **at that
  closure**. Only then switch production dispatch.
- Each provider function has one native owner step. A function extracted early
  is never written again when the rest of its file ports later.
- `rust-00` maps function-level calls and strongly connected groups in an
  activation manifest. It must find any further cycles in the real source and
  assign their extraction or closure before implementation starts.

Known splits the manifest must hold:

| Current dependency | Native provider owner | Whole-capability activation gate |
| --- | --- | --- |
| config reads and writes -> store; config writes -> events | `rust-02` pure schema; `rust-05` settings IO and event emit | native storage, policy and settings contracts pass |
| vault -> backup key path and load | `rust-05` minimal key provider; `rust-06` archive operations | native key and storage interoperability passes |
| approvals -> encrypted store; role checks -> plugin and machine | `rust-04` pure decisions; `rust-05` receipts; `rust-07` live policy | all three providers pass before production authorization |
| `changes.check` -> plugins, research, interview, promo and planning | `rust-02` hash primitive; `rust-24` full orchestration | `rust-24` with its complete native provider closure |
| `jobs.strategy` -> `tutor.engine` gap items | `rust-20` shared queue and scheduler providers; `rust-19` strategy | `rust-19` + `rust-20` closure |
| teacher -> exercises and `jobs.strategy` | `rust-20` record primitives; `rust-22` runner; `rust-21` teacher | `rust-19` + `rust-20` + `rust-21` + `rust-22` closure |
| `workday.start`/`end` -> day log, handoff, PM; `workday.watch` -> `day.watch`; `day.watch` -> changes | `rust-15` record types; `rust-16` executor; `rust-17` day; `rust-24` changes | `rust-16` + `rust-17` + `rust-24` closure |

**Worked example: what must pass before `rust-05` may switch production
dispatch?** `rust-05` alone gives component parity only. Native settings IO
needs the `rust-02` schema. Native key loading needs the `rust-05` key provider
to pass key and storage interoperability. Native authorization needs all three
approval and role providers: `rust-04` decisions, `rust-05` receipts and
`rust-07` live policy. So the storage capability may switch only after
`rust-02`, `rust-04`, `rust-05` and `rust-07` are native, and the baseline,
differential verification, 30-pair resource runs, decide step and user stories
pass again at that closure.

### The activation manifest today

Three versioned data files hold the partial manifest. Each has a revision and
coverage header in its first row. `cargo test -p careerbrew-xtask --test activation_manifest`
checks them.

| File | Rows cover | Controls |
| --- | --- | --- |
| [activation manifest](plans/2026-11-04-rust-activation-manifest.tsv) | the config, vault, approvals and role rows of the table: 28 cross-module call sites (config 4, settings IO 2, key provider 2, vault 3, approvals 7, role and bridges 10) and 23 local bridge calls | 6 |
| [activation providers](plans/2026-11-04-rust-activation-providers.tsv) | the changes, job strategy and tutor gap, and teacher and exercise rows: 50 more cross-module sites (22, 11 and 17) and 29 local rows | 13 in all |
| [activation workday](plans/2026-11-04-rust-activation-workday.tsv) | work-day start, end, clock and the background watcher: 60 more cross-module sites and 22 local rows, 7 runtime boundaries | 20 in all |

What the manifest records and checks:

- A **node** row: function, proposed native owner, source file and definition line.
- An **edge** row: caller, callee, call line and source expression.
- An **unresolved** row: caller, source anchor and reason. It stays out of the
  cycle calculation until its target is known.
- A **data** row: artifact, access, caller, line and expression (for example the
  strategy snapshot producer and its two readers). It adds no reverse call.
- Each symbol has one valid owner. A duplicate node, repeated call site,
  undeclared endpoint or invalid owner is refused.
- Nested module paths and exact import aliases resolve. A wrong nested path or
  an alias bound to another module is refused.
- Deferred runtime boundaries stay visible: the conditional research HTTP call,
  the FSRS scheduler, the exercise subprocess, source selection, rebuild
  callbacks, cycle execution, proof commands, process start and replacement,
  and the start-up preflight runner.

Findings so far:

- The combined bounded graph has **zero** strongly connected groups with more
  than one function. A module import loop through store, vault and backup does
  not close a function loop on the key path.
- Planted back edges are found exactly: backup key loading to the vault key
  selector joins exactly three functions; a reverse tutor readiness edge joins
  the strategy builder and tutor readiness; an edge from `day.watch.stop` to
  `workday.watch.run_pass` joins `run_pass`, `tick`, `finish`,
  `workday.watch.end` and `day.watch.stop`.
- The work-day executor checks the background day watcher and stops it when it
  owns that loop. The watcher calls `changes.check(fetch=False)` through its
  source registry. It does not call work-day functions directly on these paths.
- The manifest is **partial**. It makes no activation decision and does not
  change dispatch. Re-review its anchors when the Python source changes (see
  the [source-map refresh rule](#source-map-refresh-rule)).
- Approval handlers in the manifest include receipt IO, which belongs to
  `rust-05`. Pure approval decisions still belong to `rust-04`.

Anchor: Tarjan 1972, *Depth-First Search and Linear Graph Algorithms*
(strongly connected components).

### Lanes and order

- `rust-00` to `rust-03` are done; `rust-04` is ready now. Each task's **Blocked** line is a hard gate.
- After the shared storage and policy steps, independent domain lanes may run
  in parallel. They must not share mutable fixture homes.
- Each domain step ports its own CLI handlers and test assertions, and routes
  its own commands to native code when its activation closure is native (see
  [CLI handlers](#cli-handlers-clipy-is-split-across-the-steps)). There is no
  big-bang CLI cutover.
- The HTTP server (`rust-25`) and the final entry-point switch (`rust-26`) wait
  for their complete native activation closures, including the cross-domain
  cycles.
- Retirement (`rust-28`) waits for both the runtime and the developer tools.

Simplified order (the **Blocked** line of each task is the authority):

```text
rust-00 -> rust-01 -> rust-02 -> rust-03 -> rust-04 -> rust-05
rust-05 -> rust-06, rust-07, rust-09 (with rust-07), rust-15 (with rust-09, rust-10)
rust-07 -> rust-08 -> rust-11, rust-13, rust-20 -> rust-22
rust-11 -> rust-12 (with rust-09)
rust-09 -> rust-10 -> rust-14 (with rust-11, rust-13) -> rust-16 (with rust-07, rust-15)
rust-16 -> rust-17 -> rust-18 -> rust-24
rust-20 -> rust-19 (with rust-08, rust-09, rust-12) -> rust-21 (with rust-22) -> rust-23
rust-00 -> rust-27a, rust-27b (start; each domain step ports its own handlers and tests)
rust-24 + all domain steps -> rust-25 -> rust-26 -> rust-27 (finish) -> rust-28
```

## Work breakdown

Every step below shares these rules. They are not repeated in each step.

- **Why:** remove the remaining non-UI implementation of the capability and keep
  its observable behaviour.
- **Owner:** careerbrew.
- **Common inputs:** the step's files (the task's **Files** line and its rows in
  the [owner map](#every-module-has-an-owner-step)); the landed outputs of the
  steps it depends on; the Python behaviour at the pinned source revision; the
  synthetic fixtures.
- **Common outputs:** the native code; native unit and integration tests that
  carry the assertions of the step's test files (see
  [Test files](#test-files-the-file-and-its-assertions-have-different-owners));
  the native CLI handlers of the capability and their per-command routes; the
  routed adapter for this slice; user and agent docs updated in the same pull
  request; the baseline, verify, measure and decide records; the rollback
  evidence.
- **Common gate:** the [measurement protocol](#measurement-protocol-for-every-step)
  and its limits, and green hosted checks. Then the step's own acceptance
  below. A pass is component parity only until the
  [activation closure](#build-order-versus-production-activation) is native.
- **Verification commands** (from `rust-01` on; created by `rust-00`):

  ```sh
  heavy-run --exclusive cargo xtask migration baseline --step rust-NN
  heavy-run cargo xtask migration verify --step rust-NN
  heavy-run --exclusive cargo xtask migration measure --step rust-NN --pairs 30
  cargo xtask migration decide --step rust-NN
  ```

- Split a large step into several measured pull requests. Never defer a step's
  tests to the final harness step.

### rust-00: Build the baseline harness and complete code inventory

- **Depends on:** nothing. This is the first preparation step.
- **Boundary:** measurement preparation only. No production handler changes.
- **Change:**
  - Create the minimal Rust workspace, the pinned toolchain, and the setup and
    doctor checks.
  - Give the Rust format, lint, test and inventory steps their own cache keys in
    `scripts/check.sh`, apart from the Python steps. Add a failing Rust control
    that proves a cached Python success cannot skip the native checks.
  - Inventory every tracked executable file, embedded script and external
    interpreter.
  - Capture the Python CLI, action, HTTP and data contracts, with synthetic fixtures.
  - Add positive controls, and planted negative controls: wrong output, a
    leaked canary, a skipped test, and a slow path.
  - Create `cargo xtask migration baseline`, `verify`, `measure` and `decide`,
    with a whole-process-tree resource collector they can trust.
  - Define the noise floors and the allowed normalization of volatile fields
    for each contract.
  - Land this preparation before measuring any port.
- **Inputs:** `scripts/api-timing.py`, `scripts/check.sh`, `docs/proof/`, the
  current Python code, the [code inventory](plans/2026-11-01-rust-code-inventory.csv).
- **Outputs:** `Cargo.toml`, `xtask/`, the four migration commands, the contract
  captures, the noise floors, the activation manifest, the complete inventory.
- **Gate:** the harness rejects 100% of planted defects and accepts 100% of
  unchanged-oracle trials. The inventory has 0 unassigned executable paths.
  `cargo xtask migration self-test` and `cargo xtask migration inventory --check`
  pass. The common parity, resource and privacy gates also apply.
- **Measurement now:** `cargo run -q --locked --offline -p careerbrew-xtask -- migration self-test`
  (22 harness controls today, exit 0) and
  `cargo run -q --locked --offline -p careerbrew-xtask -- migration inventory --check`
  (0 unassigned paths). Run both under `heavy-run`. The `baseline`, `verify`,
  `measure` and `decide` commands replace them when they exist.
- **Tools it uses:** the toolchain identity probe (see
  [Current state](#current-state)) records the installed compiler and Clippy
  evidence on supported hosts.
- **Where it stands:** see [State of rust-00](#state-of-rust-00).

### rust-01: Prove maintained dependency and data interoperability

- **Depends on:** `rust-00`.
- **Boundary:** dependency adapters only. No new data format.
- **Change:**
  - Evaluate maintained Rust libraries for JSON and YAML, Fernet, FSRS, ICS
    recurrence and time zones, HTTP, file locks, subprocess limits, and native
    parsers for the retained Python and JavaScript teaching examples.
  - Pin the accepted crates and their licences in `Cargo.lock`.
  - Run two-way Fernet vectors, old-data reads, FSRS schedule vectors, and
    recurrence and daylight-saving fixtures against Python.
  - Reject unsupported meanings before any domain port.
- **Inputs:** `pyproject.toml`, `src/careerbrew/vault.py`, `store.py`,
  `tutor/cards.py`, `calendar.py`.
- **Outputs:** pinned crates and licences; dependency contract vectors; a short
  decision note per library (kept or rejected, with the evidence).
- **Gate:** 100% of dependency contract vectors agree; 0 unsupported required
  features; 0 custom cryptographic primitives.
- **Measurement:** a preparation step, so harness and dependency controls
  replace speed claims. Run the `rust-00` self-test and
  `heavy-run cargo xtask migration verify --step rust-01` (each vector set
  against the Python oracle, with a planted wrong-vector control that must
  fail). No `measure` or `decide` resource run: there is no product path to
  time.

#### rust-01 vector framework

`migration verify --step rust-01` compares dependency vectors, not CLI runs.

- **Vector set:** a fixture `xtask/tests/fixtures/depvec/<set>.json`:
  `{"set": "<set>", "vectors": [{"id", "op", "input"}]}`. Vectors are
  synthetic only.
- **Oracle:** `xtask/tests/fixtures/depvec/driver.py <set> <file>` runs each
  vector with careerbrew's own code (for `json-yaml`: `store._json_text`,
  `store._SAFE_LOADER` and the `store.write_yaml` dump call).
- **Candidate:** `careerbrew-depvec <set> <file>` (`crates/depvec/`), built
  with `cargo build -p careerbrew-depvec`; verify runs
  `target/debug/careerbrew-depvec` unless `--test-candidate` names another.
- **Output:** each side prints `{"set", "results": [{"id", "output"}]}`;
  an output is `{"ok": <value>}` or `{"error": true}`. Values use a typed
  form (`crates/depvec/src/canon.rs`), so an int against a float or a date
  against a string is a difference.
- **Compare:** one case per vector, `<set>:<id>`, passed when the outputs are
  equal. The oracle must answer every vector, or verify is refused. A
  candidate that fails, misses a vector or adds one fails cases. Each side
  runs in its own fresh synthetic home.
- **Planted control:** `--test-wrong-vector <set>` wraps the set's first
  input for the candidate only; that case must fail and verify exits 1
  (`xtask/tests/depvec.rs`, one control per set). `migration self-test` also runs 5 compare
  controls on synthetic answers, plus the two shared vector-step controls
  (a planted home write is counted; each vector step is registered once).
- **Shared code:** `xtask/src/depvec.rs` holds one row per vector step
  (`STEPS`: `rust-01`, `rust-02`), so the steps share the compare and record
  code. rust-01's row does not count home writes, so its record is unchanged.

**Add a vector set** (one PR per set):

1. Add the fixture `xtask/tests/fixtures/depvec/<set>.json` (verify finds
   sets by file name; `xtask/src/depvec.rs` does not change), and its
   `.gitignore` allow line.
2. Add a module `crates/depvec/src/<set>.rs` with
   `pub fn run(op: &str, input: &Value) -> Result<Value, String>`.
3. Add the registry lines: `("<set>", <module>::run),` in `SETS` in
   `crates/depvec/src/lib.rs`, and `"<set>": <function>,` in `SETS` in
   `driver.py`.
4. Add the inventory rows for the new source files and stage them together.
5. Add the crate rows to the decision table below.

**Set `json-yaml` (52 vectors):** `json-read` (`json.loads`, key order,
repeated keys, Unicode, surrogate pairs, ints to 64 bits, floats, errors),
`json-write` (the exact `store._json_text` bytes: sorted keys, 2-space
indent, `ensure_ascii`, `default=str` dates), `yaml-read` (old data under
PyYAML's YAML 1.1 safe loader: `yes`/`on` bools, octal, hex, binary and
base-60 ints, `1_000`, `.inf`, dates and timestamps with offsets, anchors,
repeated keys, block scalars, one document) and `yaml-roundtrip` (each side
writes and reads its own text).

**Set `calendar` (22 vectors):** `day` runs `calendar.parse_ics` for one
local day: weekly `BYDAY` workday series, `UNTIL` and `COUNT` ends, EXDATE,
a daily series, the US daylight-saving end (2026-11-01) and the EU one
(2026-10-25) seen from New York, Berlin and UTC, a UTC event that falls on
the previous Los Angeles day, an all-day event, a transparent private
event, and an unknown zone (an error). `local-day` puts an instant in its
local day bucket. The Python side sorts events by start, end and title,
like the Rust side. Known gaps: RECURRENCE-ID overrides, DURATION, events
with no DTEND, zone-less (floating) times, and local times that a
daylight-saving jump skips or repeats.

Known gaps, not in the vectors yet (each is a rejected meaning until a later
PR covers it): YAML merge keys (`<<`) and tags other than
`!!str`; `.nan` (its sign bit differs by CPU in PyYAML); and writing YAML for
a string that looks like a date or `1_000` (the `yaml-rust2` emitter does not
quote these). JSON exponent forms, nonfinite floats and negative zero are
covered by rust-05h component controls rather than new depvec vectors.
Arbitrary-precision JSON/YAML integer reads, JSON integer
writes and YAML datetime writes now have vectors (rust-05f).

**Set `code-parse` (33 vectors):** does a teaching example parse, as
`scripts/check-card-code.py` decides it. `python-parse` (the oracle runs
`compile(code, id, "exec")` on Python 3.12: types, `match`, walrus, async,
PEP 701 f-strings, type parameters, `except*`; and rejects a missing colon,
bad indent, an open bracket, `print "x"`, assignment to a literal, an open
string, and the 3.14-only template strings and unparenthesized `except A,
B`) and `js-parse` (the oracle runs the native esbuild binary from
`web/node_modules/@esbuild/` with the card loader and `--format=esm`, the
same engine the script calls through node: JS, JSX, TS and TSX that parse,
and an open brace, a missing expression, a repeated `let`, JSX or types under
the `js` loader, JSX under `ts`, and a wrong closing tag). The oracle stops
when esbuild is missing, so verify is refused, never silently skipped. The
Rust side pins the Python target to 3.12, the version in `.python-version`.

Known gaps for `code-parse` (rejected meanings until a later PR covers them):
`compile()`'s scope checks (`return`, `yield` or `await` outside a function,
`break` outside a loop, a repeated argument name, `nonlocal` at module
level). `ruff_python_parser` reports these only through its
`SemanticSyntaxChecker`, which needs a scope model that ruff's own linter
supplies; a hand-written one would be new semantics, so it waits for
`rust-27`. Strict-mode-only JavaScript errors (`with`, legacy octal) also
stay out: esbuild treats a file as strict only when it has `import` or
`export`, while `oxc` parses every module as strict.

**Set `fsrs` (18 vectors):** `parameters` pins the FSRS-6 parameter set
`tutor/cards.py` uses (py-fsrs 6.3.2 `Scheduler()`: 21 weights with decay
0.1542, retention 0.9, learning steps 1 and 10 minutes, relearning step 10
minutes, maximum 36,500 days). `schedule` runs a card through a list of
grades at fixed minutes after a synthetic start, with `cards.RATINGS` for
the grade, and compares after each review: state, step, due time (seconds),
stability and difficulty (2 decimals). Vectors cover each first grade,
learning steps, same-day reviews, late and overdue reviews, lapses and
relearning, and bad input. The Rust side takes stability, difficulty and the
raw interval from the `fsrs` crate; a small adapter in
`crates/depvec/src/fsrs_schedule.rs` adds the steps, the state, py-fsrs's
stability floor (0.001; the crate's is 0.0001), the half-to-even whole-day
interval from 1 to 36,500, and the due time.

Known gaps for `fsrs` (rejected meanings until a later PR covers them):
`cards.review` fuzzes review intervals with Python's `random()`, so the
vectors pin fuzzing off on both sides, and the Rust port needs its own fuzz
with the same ranges; the crate computes in `f32` and py-fsrs in `f64`, so
values agree to 2 decimals here but long histories can drift; the crate caps
stability at 36,500 days and py-fsrs does not.

**Set `runtime` (38 vectors):** `http-fixture-path` (the
`connectors._fixture_path` key: netloc as written, path runs outside
`[A-Za-z0-9_.-]` made one `_`, `root` for no path, query and fragment
dropped), `http-fixture-read` (the file `connectors.http_get` reads from
synthetic fixtures: `.json`, then `.xml`, `.html`, `.csv`; no match is an
error), `lock-probe` (flock exclusion across processes and languages: a Rust
`File::lock` holder against a Python `fcntl.flock` peer process, and a
Python holder against a Rust probe; exclusive and shared modes, and a
released lock) and `run-limited` (the `exercises._limits` CPU and memory
limits and the `exercises._child` wall timeout and outcome rules, on fixed
`/bin/sh` programs: ok, exit 3, a CPU spin, a sleep). The lock peer on the
Rust side is `.venv/bin/python` under the repo root (verify runs the
candidate there), or `CAREERBREW_DEPVEC_PYTHON`.

Known gaps for `runtime`: hosts with capitals and URLs with non-ASCII or
space characters in the path (`http::Uri` refuses some that `urlsplit`
accepts); `fcntl.lockf` and `fcntl.LOCK_UN` from another process; process
groups (neither side kills grandchildren); no real network request, so no
HTTP client crate is pinned yet.

#### rust-01 dependency decisions

| Need | Crate | Version | Licence | Decision | Evidence |
| --- | --- | --- | --- | --- | --- |
| JSON read and write | `serde_json` | 1.0.154 | MIT OR Apache-2.0 | kept | already pinned by `xtask` and `crates/vault`; `json-read` and `json-write` vectors; no `preserve_order` feature, since workspace feature unification would change `xtask`'s map order (key order comes from a serde visitor instead) |
| YAML read and write | `yaml-rust2` | 0.13.0 | MIT OR Apache-2.0 | kept | maintained (release 2026-09); its parser events carry the scalar style, which PyYAML's YAML 1.1 plain-scalar rules need; `yaml-read` and `yaml-roundtrip` vectors. Pulls `arraydeque` (MIT OR Apache-2.0), `hashlink` and `hashbrown` (MIT OR Apache-2.0) and `foldhash` (Zlib) |
| YAML read and write | `serde_yaml` | 0.9.34 | MIT OR Apache-2.0 | rejected | archived and unmaintained since 2024 |
| YAML read and write | `serde_norway`, `serde_yaml_ng` | 0.9.42, 0.10.0 | MIT OR Apache-2.0 | rejected | serde forks of `serde_yaml`: their values do not keep the scalar style, so a quoted `'yes'` and a plain `yes` look the same and YAML 1.1 reads cannot match PyYAML |
| YAML read and write | `serde-saphyr` | 2.2.0 | MIT OR Apache-2.0 | not chosen yet | active, but its newest release is days old; look again if `yaml-rust2` falls behind |
| ICS parsing | `icalendar` | 0.17.17 | MIT OR Apache-2.0 | kept | active (release 2026-10); only its `parser` feature (`nom` 8 and `nom-language`, MIT; `iso8601`, MIT; `uuid`, MIT OR Apache-2.0); its properties keep their TZID and VALUE parameters; `calendar` vectors |
| RRULE expansion | `rrule` | 0.14.0 | MIT OR Apache-2.0 | kept | the Rust port of the `rrule.js` and `dateutil` rules; `BYDAY`, `UNTIL`, `COUNT` and EXDATE on zoned starts; pulls `regex` (MIT OR Apache-2.0) and `thiserror` (MIT OR Apache-2.0). Last release 2025-04: look again if it stays quiet past 2027 |
| RRULE expansion | `icalendar` `recurrence` feature | 0.17.17 | MIT OR Apache-2.0 | rejected | wraps the same `rrule` crate behind a model API; the plain parser plus `rrule` keeps each step visible and the feature set small |
| Time zones | `chrono-tz` | 0.10.4 | MIT OR Apache-2.0 | kept | the IANA database compiled in (`phf`, MIT), so a zone reads the same on every machine; the zone type `rrule` uses |
| Dates and times | `chrono` | 0.4.45 | MIT OR Apache-2.0 | kept | the date type `rrule` and `chrono-tz` share; `clock` pulls `iana-time-zone` (MIT OR Apache-2.0) |
| Python example syntax | `ruff_python_parser` (with `ruff_python_ast`) | 0.0.22 | MIT | kept, with the scope-check gap above | Astral publishes it to crates.io from the ruff repo (release 2026-11); `parse_unchecked` with a 3.12 target reports both parse errors and syntax newer than 3.12; `python-parse` vectors. Pulls `ruff_text_size`, `ruff_python_trivia`, `ruff_unicode_names2` (MIT), `bitflags`, `bstr`, `memchr`, `rustc-hash`, `unicode-ident`, `unicode_names2` and others under MIT or Apache-2.0 (`Cargo.lock`) |
| Python example syntax | `rustpython-parser` | 0.4.0 | MIT | rejected | last release 2024-08; the RustPython team moved to the ruff parser; no PEP 701 f-strings or 3.12 type parameters |
| JS and TS example syntax | `oxc_parser`, `oxc_semantic` (with `oxc_allocator`, `oxc_span`) | 0.159.0 | MIT | kept | active (release 2026-11); one parser for JS, JSX, TS and TSX; `oxc_semantic`'s syntax checks add esbuild's repeated-declaration error; `js-parse` vectors |
| JS and TS example syntax | `swc_ecma_parser` | 47.1.0 | Apache-2.0 | not chosen | maintained, but repeated declarations need extra swc crates and a resolver pass; `oxc` gives the same checks in fewer crates |
| Fernet tokens (data at rest, backups) | `fernet` (feature `rustcrypto`) | 0.2.2 | MIT | kept | already pinned by `crates/vault`; the `fernet` vector set: Python-made tokens open in Rust and Rust-made tokens open in Python, old plaintext and a 2020 token still read, a wrong key, a tampered or a truncated token fail on both sides, and a token stamped in the future opens on both sides: Python checks no time without a TTL, so `careerbrew_vault::decrypt_like_python` passes the crate a clock no earlier than the token's stamp (feature `fernet_danger_timestamps`; the MAC and cipher checks stay the crate's). Its AES, CBC, HMAC and SHA-256 come from the RustCrypto crates (`aes`, `cbc`, `hmac`, `sha2`, MIT OR Apache-2.0); no crypto code is ours. It has no way to pass the IV, so a seal vector compares the envelope's shape and plaintext, not its bytes |
| Fernet tokens | `fernet` (default feature `openssl`) | 0.2.2 | MIT | rejected | needs a system OpenSSL to build and link; `rustcrypto` gives the same tokens in pure Rust |
| Base64 for tokens | `base64` | 0.22.1 | MIT OR Apache-2.0 | kept | already pinned by `crates/vault`; the strict check `backup.verify` makes (`open-strict` vectors) and the Python-compatible lenient read in `careerbrew-vault` |
| Backup tar archives | `tar` | 0.4.46 | MIT OR Apache-2.0 | kept | maintained tar reader/writer behind `crates/vault/src/backup.rs`; default features off; Python↔Rust create/verify/restore controls in rust-06 |
| Backup gzip compression | `flate2` | 1.1.11 | MIT OR Apache-2.0 | kept | maintained gzip stream behind the same adapter; `rust_backend` only; Python↔Rust archive controls in rust-06 |
| Plugin resource paths | `glob` | 0.3.4 | MIT OR Apache-2.0 | kept | maintained glob matching behind the locked-plugin ladder adapter; valid, escaping and symlink controls in rust-06 |
| FSRS card schedule | `fsrs` | 7.0.0 | BSD-3-Clause | kept | official Open Spaced Repetition crate (release 2026-11); `FSRS6_DEFAULT_PARAMETERS` equal py-fsrs 6.3.2 `DEFAULT_PARAMETERS` (`parameters` vector) and its FSRS-6 formulas match py-fsrs's (18 `fsrs` vectors). Version 7 no longer pulls `burn`. New dependencies: `ndarray`, `rayon`, `rand` 0.10, `rand_distr`, `itertools`, `snafu`, `num-*`, `crossbeam-*`, `either`, `matrixmultiply`, `rawpointer`, `portable-atomic*`, `chacha20`, `cpufeatures`, `getrandom`, `heck`, `autocfg`, `r-efi` (MIT OR Apache-2.0, `r-efi` also LGPL-2.1-or-later as an option), `bytemuck`, `wide`, `safe_arch` (Zlib OR Apache-2.0 OR MIT), `libm` (MIT) |
| FSRS card schedule | `rs-fsrs` | 1.2.1 | non-standard (crates.io) | rejected | last release 2024-10; no SPDX licence on crates.io; it predates the 21-weight FSRS-6 set with a trainable decay that py-fsrs 6.3.2 uses |
| FSRS learning steps, state and due time | (none) | | | adapter | the `fsrs` crate has the memory model only; py-fsrs's step and state rules are a small adapter in `crates/depvec/src/fsrs_schedule.rs`, checked by the `schedule` vectors |
| HTTP fixture URL split | `http` (`http::Uri`) | 1.5.0 | MIT OR Apache-2.0 | kept | keeps the authority as written, like `urlsplit().netloc`, and drops the fragment; pulls only `bytes` (MIT OR Apache-2.0); `http-fixture-path` and `http-fixture-read` vectors |
| HTTP fixture URL split | `url` | 2.5.8 | MIT OR Apache-2.0 | rejected | lowercases hosts, drops default ports and percent-encodes paths, so the fixture key differs from `urlsplit`; pulls about 40 ICU crates (Unicode-3.0) |
| HTTP client | `ureq` | none | MIT OR Apache-2.0 | deferred | no vector makes a network request yet; picked and pinned in the step that first sends one |
| File locks | `std::fs::File::lock` | Rust 1.100.0 | MIT OR Apache-2.0 | kept | standard library since Rust 1.89; flock on Unix, the same lock as Python `fcntl.flock`; `lock-probe` vectors cross both directions |
| File locks | `fs4`, `fd-lock` | latest | MIT OR Apache-2.0 | rejected | the standard library now gives the same flock lock with no extra crate |
| Subprocess CPU and memory limits | `rlimit` | 0.11.0 | MIT | kept | maintained `setrlimit` wrapper over `libc` (already in the lock file), called in `pre_exec` like `exercises._limits`; `run-limited` vectors |
| Subprocess wall timeout | `wait-timeout` | 0.2.1 | MIT | kept | small, stable, widely used; kill and reap on timeout like `subprocess.run(timeout=)`; `run-limited` vectors |
| Subprocess limits | `process_control` | latest | MIT OR Apache-2.0 | rejected | has wall and memory limits but no CPU-time limit |

The two kept parser crates add 107 `Cargo.lock` entries. Most are MIT or
Apache-2.0. The others: `icu_*`, `zerovec`, `yoke`, `zerofrom`, `zerotrie`,
`tinystr`, `writeable`, `litemap` and `potential_utf` (Unicode-3.0), `ryu`
(Apache-2.0 OR BSL-1.0), `self_cell` (Apache-2.0 OR GPL-2.0-only, taken as
Apache-2.0) and `tinyvec` (Zlib OR Apache-2.0 OR MIT). None is copyleft-only.

Appended bytes, decided: a vault read (`vault.open_bytes`) keeps Python's
behavior. Python's base64 decoder stops at a token's completed padding, so
bytes appended after it are ignored, and Rust does the same
(`open-appended-after-padding` opens on both sides). This is safe: the HMAC
still covers every byte of the token, and appended bytes without padding
first change the token and fail (`open-appended-without-padding`). A backup
read (`backup.verify`) stays strict, and appended bytes fail there on both
sides (`strict-appended-after-padding`).

### rust-02: Port pure setting schemas, paths, clocks and hashes

- **Depends on:** `rust-01`.
- **Boundary:** process environment and immutable value types.
- **Change:**
  - Port the pure setting validators and defaults, and their precedence over
    supplied values.
  - Port the empty `CAREERBREW_HOME` refusal, local and UTC date values,
    durations and input hashes.
  - **Not here:** encrypted settings IO and minimal event emit (`rust-05`); the
    live `changes.check` orchestration (`rust-24`).
  - Freeze the clock and the environment in each test.
- **Inputs:** `src/careerbrew/config.py` (pure schema), `paths.py`,
  `changes.py` (`_h` only; `rust-24` owns the file), `calendar.py` (the pure
  clock helpers only; `rust-08` owns the file), `__init__.py`.
- **Outputs:** native value types and the hash primitive that `rust-24` reuses.
- **Gate:** 100% of precedence, invalid-input, daylight-saving and hash vectors
  agree; 0 writes to a real home.

#### rust-02 vector framework

`rust-02` ports pure functions with no CLI command, so it is a vector step
like `rust-01` (row `RUST_02` in `xtask/src/depvec.rs`). The DATA and role
contracts do not apply: the vectors make no writes and use no roles.

- **Vector set:** a fixture `xtask/tests/fixtures/purevec/<set>.json`, in the
  rust-01 shape. Four sets, 411 vectors: `clock` (95), `hash` (57), `paths`
  (69) and `settings` (190), each described below. Each set adds one module
  in `crates/purevec/src` and one line in each `SETS` registry (one entry
  per line, sorted by name).
- **Oracle:** `xtask/tests/fixtures/purevec/driver.py <set> <file>`, or
  `--all` (prints the set names) for `baseline` and `measure`.
- **Candidate:** `careerbrew-purevec <set> <file>` or `--all`
  (`crates/purevec/`), built with `cargo build -p careerbrew-purevec`.
- **Registration:** a `baseline` step (`driver.py --all`; its source hash
  also covers the driver and the five Python inputs), a `measure` candidate
  (`target/debug/careerbrew-purevec --all`) and `unmeasured` noise floors
  for macOS and Linux. All four commands accept `--step rust-02`.
- **No sets:** verify records one failed case, `no-vector-sets`, so a
  record with 0 vectors never passes.
- **Real-home writes:** verify snapshots the caller's real homes
  (the repo's `private/` folder, careerbrew's default data folder, and a
  set `CAREERBREW_HOME`; a home inside the other counts once; each file's
  size and modification time) before and after both sides. The record holds
  `real_home_writes` (paths added, removed or changed), `real_homes_checked`
  and `planted_home_write`. A count above 0 fails the case
  `real-home-writes`. Test-only: `--test-plant-home-write <file-name>` writes
  one new file into the caller's absolute `CAREERBREW_HOME` between the
  snapshots and removes only that file after; a name that already exists
  is refused (`--test-plant-home-write names an existing file`). That case
  must fail (`xtask/tests/purevec.rs`).
- **Known limit, noisy guard:** the snapshot spans the whole verify run. A
  running careerbrew watcher or web app writes bookkeeping files in the real
  home on its own schedule (`day/watch.json`, `day/cursors.json`,
  `work/feed/.lock`), so a run that overlaps one fails `real-home-writes`
  although neither side wrote. Stop the watcher before a verify run, and
  rerun once before you trust a failure of that case.
- **Set `clock`** (95 vectors, `clock.json`): the clock helpers of
  `calendar.py` (`_day_bounds`, `_aware`, `now`, `now_utc`, `system_tz`)
  and `config._tz`, with the zone, the pinned `CAREERBREW_NOW` and the wall
  clock injected by each vector (`crates/purevec/src/clock.rs`). It covers
  23 h, 25 h and 30-minute DST days (New York, Berlin, Lord Howe), zones
  whose midnight falls in the gap (Santiago, Havana), Kolkata and UTC; the
  real and the Python wall span of each day (Python reports 24 h for both);
  fold=0 in a gap (the wall time is kept, with the offset before) and in an
  overlap; `timedelta` sums across transitions (the `parse_ics` +1 h default
  end, a day, hours, negative and carried values); the `fromisoformat` forms
  `Z`, `+05:30`, `+0530`, naive, date only, hour only, a space separator,
  fractional seconds (`.` or `,`, after the hour, minute or second), an
  hour-only offset `+05`, full-width digits (refused by both) and invalid
  values; non-ASCII zone names for `config._tz` and `TZ`; an empty `CAREERBREW_NOW` counts as
  unset. Zone data: the oracle empties the tz path, so it reads the
  `tzdata` package from `uv.lock` (2026.5, IANA 2026e); the candidate uses
  `chrono-tz` 0.10.4 (IANA 2025b compiled in). Both are recorded in the
  fixture's `zone_data`; all 95 vectors agree in spite of the skew. Known
  gaps: `local_tz` (the `settings.yaml` read) and the `/etc/localtime`
  lookup of `system_tz` stay Python (rust-05), and the candidate refuses an
  empty `TZ` (or a bare `:`) instead of guessing; `fromisoformat` forms the
  candidate refuses (week dates, compact dates and times, offsets with
  seconds, more than 6 fraction digits); zone names that differ only by
  letter case (a case-insensitive disk lets Python load them). The four
  owner test files the plan names hold no clock assertions, so none were
  ported. `crates/purevec/tests/cli.rs` fails on a wall-clock read
  (`Utc::now`, `Local::now`, `SystemTime::now`) in vector code.
- **Set `paths` (69 vectors):** `careerbrew.paths` with `os.environ`
  replaced by the vector's `env` map, through `mock.patch.dict` on the
  oracle side and an injected map in `crates/purevec/src/paths.rs`. No side
  creates or changes a folder (`data_root(create=False)`; `mkdir` and
  `chmod` are rust-05). Ops: `data-root` (the folder pathlib prints: unset
  reads `<repo>/private`, a value is used unstripped, `~` expands with the
  map's `HOME`, the path is never resolved, and a relative value stays
  relative), `data-root-refusal` (the exact refusal text when the value
  holds only Python `str.isspace` characters, including U+001C to U+001F,
  U+0085, U+00A0 and U+3000; null otherwise), `extra-knowledge` (false only
  for exactly `0`) and `config-dir` (`<repo>/config`). The repo root prints
  as `<repo>`, so no output holds a machine path. Python anchor:
  `test_empty_careerbrew_home_is_an_error_not_the_real_data`
  (`tests/unit/test_regrade_notion.py`). Known gaps: `~user` reads the
  account database in Python, so the candidate refuses it (the vectors use
  only a missing user, which both sides refuse); a `~` value with no `HOME`
  in the map also reads the account database and is refused; `sub` and
  `require_outside_data_root` touch the file system and stay for rust-05.
- **Measure is per process:** both sides run every set with `--all`, so a
  ratio covers the whole step, not one slice. Each slice PR re-runs
  `baseline` (its source hash changes). The verdict is component parity
  only; it is not a user-facing speed claim.

Set `hash`: the `changes._h` input hash primitive that rust-24 reuses
(`crates/purevec/src/hash.rs`, `input_hash`). 57 vectors, op `input-hash`,
each output `[serialized text, 16 hex digest]`. The digest is the `sha2`
crate; the text is Python's `json.dumps(sort_keys=True, default=str)`
layout. The input is a typed value (the `canon.rs` form plus `tuple` and
`path`), so ints, floats, big ints and non-string keys stay apart. The
vectors cover YAML-typed leaves (the callers hash promo ladders and company
fields loaded from YAML): empty and nested maps, ASCII escapes, controls
(`\u007f` is escaped, not raw), BMP and astral text (surrogate pairs), int,
bool, float and `None` keys in Python's numeric order, exact int against
float keys, equal keys merged (`True` and `1`: first key, last value),
float `repr` (`1e+16`, `1e-05`, `-0.0`, `NaN`, `Infinity`, `5e-324`), ints
past i64 and at Python's 4300-digit limit, tuples, dates, datetimes and
paths through `default=str`, and five refusals (str against int keys,
`None` against str keys, a date key, a tuple key, a 4301-digit int).
Planted control: `a_planted_wrong_hash_vector_fails_its_case`
(`xtask/tests/purevec.rs`). Known gaps, no vectors: `NaN` keys (Python
sorts them in an order the candidate refuses), lone surrogates, `bytes`,
`set` and `Decimal` leaves, ints written with `_` or spaces, a changed
`PYTHONINTMAXSTRDIGITS`, recursion past Python's limit, and date or
datetime text that is not in canonical `isoformat` form. `experiments/hash`
stays: `tests/unit/test_rust_hash_experiment.py` still builds and checks
it, so retiring it is its own change.

Set `settings` (`crates/purevec/src/settings.rs`, 190 vectors): the pure
value logic of `config.py` over an in-memory settings map. Ops: `set` (the
`set_value` validators, the `unknown setting ...; known: ...` KeyError text
and the `<key>: <message>` re-raise, and the written map), `effective`
(precedence: a stored value of any type wins, a stored `None` stays `None`,
an absent key falls back to its default, unknown keys follow in order with
help `not a known setting (kept as is)`, and `DEFAULTS` in insertion order
with the 8 `time_baseline_*` keys), `get` (its own KeyError text) and `unset`
(a stored `None` is never removed). It covers the 13 self-contained
validators and the `timezone`, `stt_model` and `pm_send_via` lambdas with
hand-written matchers that follow Python's `re` on `str` (`\d` is any
Unicode decimal digit, `\s` is `str.isspace()`) and Python's `int()`
(Unicode digits, underscores, whitespace). It also ports the
`tests/e2e/us-64-fast-views.sh` settings checks (a bad `web_cache_seconds`
names the range, a bad `web_tab_cache` names the values); the other three
owner test files (`test_contributing.py`, `test_eval_case_loading.py`,
`test_issue_335_337.py`) hold no settings or validator assertion. The oracle
replaces config's file read, write and event with in-memory stand-ins, so no
vector touches a home. Known gaps:
- `work_steps`, `work_checkouts` and `work_signin_probe` call workday
  modules that do I/O, and `standup_tz` (`_tz`) reads tzdata: `set` answers
  `{"refused": "delegated"}` on both sides. `_tz` comes with the clock set;
  the workday validators need `rust-07` or later.
- The settings file read and write (`_file`, `store.write_yaml`) and the
  `config.set` event are `rust-05`.
- `int()` over 4,300 digits raises Python's digit-limit error; the
  candidate gives the range error.
- The digit table and the `repr()` non-printable table are Unicode
  15.0.0, the oracle Python's version.

#### rust-02 close-out

Done 2026-11-18, on macOS, with the four sets of #149. Records stay local.

- **verify:** 411 of 411 vectors pass; 0 real-home writes; 0 violations.
- **baseline (Python oracle, 30 runs):** CPU p50 108 ms, p95 113 ms; tree
  RSS p50 40.1 MB, p95 40.6 MB.
- **measure (30 pairs, CPU p95 / tree RSS p95):** cold: oracle 393 ms /
  45.4 MB, candidate 2.6 ms / 3.4 MB. Warm: oracle 89 ms / 37.1 MB,
  candidate 2.5 ms / 3.4 MB.
- **decide:** `activate`, no reasons. The noise floor is `unmeasured` on
  macOS, so the 1.10 ratio applied to every case.
- **What `activate` means here:** component parity only. No command routes
  to native code: `rust-02` owns no CLI command, and the config capability
  switches only with `rust-05` settings IO (see the activation manifest
  splits). Production dispatch waits for `rust-04`, `rust-05` and `rust-07`.
- **Known gaps:** the empty `TZ` (or bare `:`) refusal, `local_tz` and the
  `system_tz` disk lookup stay Python until `rust-05`; `sub`,
  `require_outside_data_root`, `~user` and a `~` with no `HOME` stay Python;
  the `fromisoformat` forms and case-only zone names listed in set `clock`;
  noise floors are `unmeasured` on both OSes; no Linux run yet.

### rust-03: Introduce the native CLI and one read-only pilot

- **Depends on:** `rust-02`.
- **Boundary:** the CLI parser and one generated action registry.
- **Change:**
  - Define one action registry. CLI help, agent discovery and HTTP forms all
    read it.
  - Port static `--help` and the action-schema discovery through a capability
    adapter. The pilot must not read encrypted settings.
  - During the move, generate the Python compatibility dispatch from that
    registry. Never keep a second list.
  - The generated dispatch routes one command at a time to the native binary.
    Each command has its own route switch and rollback. A domain step uses it
    to switch its own commands when its activation closure is native.
  - Keep argument parsing, exit codes, JSON, and the terminal-only and
    confirmation notes.
- **Inputs:** `src/careerbrew/cli.py`, `actions.py`, `__main__.py`, `agent_workflows.py`.
- **Outputs:** the native registry, the native CLI entry point, the generated
  Python dispatch with per-command routes.
- **Gate:** 100% of pilot CLI contracts agree; 0 action-registry discrepancies;
  the pilot runs with Python unavailable.
- **Progress:**
  - The registry exists. `scripts/gen-registry.py` walks the argparse tree
    (still the source until `rust-26`) and writes one committed file,
    `crates/cbcli/registry/registry.json`: `meta` (Python minor version, help
    width, row count), `notes` (the terminal-only, confirm, yes-from-confirm and
    agent-note maps), `rows` (the exact `careerbrew actions --json` rows) and
    `help` (each parser path's `--help` text, rendered at a pinned 80 columns
    on the pinned Python). It also writes `config/native-routes.txt`, one
    `<action> <python|native>` line per action, all `python` today; a
    regeneration keeps each existing route.
  - `actions.catalog()` reads that file, so `careerbrew actions`, the web app's
    Actions view and `actions.run()` read one list, and the catalog builds no
    parser. The parser walk stays as `actions.catalog_from_parser()`, the
    generator's source and the parity test's oracle.
  - Python `--help` still comes from argparse: that is the one second path, and
    the parity test binds it to the stored help text. `rust-26` flips the
    direction, so the registry generates the parser.
  - Parity: `tests/unit/test_action_registry.py` (committed files equal a fresh
    export; 240 of 240 rows equal the parser walk; the rows print byte-equal to
    the actions reference `capture_actions.rs` points at; every action has its
    help text and a route line; the Python minor version matches
    `.python-version`). The gate step `registry` runs
    `scripts/gen-registry.py --check` and prints the fix line.
  - `careerbrew actions --json` output did not change, so the frozen capture
    references stay as they are.
  - The pilot binary exists: crate `crates/cbcli` (package `careerbrew-cli`,
    binary `careerbrew-native`, no dependencies). It compiles the registry in
    and serves `actions [name] [--json]` and static `-h/--help` (root, group,
    leaf). Its JSON writer reproduces Python's `json.dumps(indent=1)` bytes
    (`\uXXXX` escapes with surrogate pairs, number text kept as written).
    It prints argparse's usage error for an unknown flag or an extra name
    (exit 2). Anything else exits 64 with `native: unsupported <command>`:
    other commands, root flags, `--`, `--json=x`, combined short flags, help
    at a width other than 80 columns (`COLUMNS` set to another value, or no
    `COLUMNS` and a terminal), and the text listing when the data root holds
    an input of Python's `privacy.redact` (`sensitive-terms.txt`,
    `profile.yaml`, `plugins.lock.json`). With events on it appends Python's
    content-free `cli.run` row after a successful `actions`; it never sends
    telemetry ([decision 0010](decisions/0010-native-pilot-hooks.md)).
  - The route prelude exists, default Python. `cli.main()` with no argv (the
    console script only; `actions.run()` and tests pass argv) calls
    `_native_route` before it builds the parser. In order: `CAREERBREW_NATIVE=off`
    keeps Python (the kill switch); a bare run or any leading flag keeps
    Python; `-h/--help` routes only with `COLUMNS=80`; the action is the
    two-word `<cmd>.<sub>` route key when the table has it, else `<cmd>`;
    `CAREERBREW_ROUTE_<ACTION>=native|python` (upper case, other characters
    `_`) wins over the table, for rollback; the binary is
    `CAREERBREW_NATIVE_BIN` or `target/release/careerbrew-native`. A missing
    or non-executable binary, or one that cannot start, logs
    `cli.native_fallback`, and exit 64 logs `cli.native_unsupported`; both
    then run Python. The binary runs as a child (not `exec`), so exit 64 can
    fall back; its stdout and stderr are held (the binary buffers both until
    exit anyway) and shown only when it did not refuse, so a refusal never
    duplicates output. A signal exits 128 + N, Ctrl-C exits 130. The binary
    writes its own `cli.run` row, so the prelude adds none. The prelude reads env and the plaintext route file
    only. All routes are still `python`, so nothing changes until the
    activation PR flips `actions`. Tests: `tests/unit/test_native_route.py`;
    story US-111 (`tests/e2e/us-111-native-actions.sh`: python, native and
    rollback print byte-identical `actions --json`). Measured cost on a
    Python-routed command: about 0.07 ms per call in process.
- **Verify scope:** the pilot binary serves no HTTP, no data and no
  role-checked command; `server.py` stays Python. So `rust-03` declares
  `http_contract`, `data_contract` and `role_contract` false, and verify lists
  `http`, `data` and `role` in `missing_contracts`. In their place it checks
  one offline contract, `registry-projection`: the candidate's
  `actions --json` stdout, wrapped as `{"actions": ...}`, must equal the body
  of the frozen capture-http `/api/actions` cases 0 and 4. The candidate is
  not checked on the other 19 HTTP cases, `http:side-effects`, DATA 1 to 10 or
  `jobs list`. Both sides run with `COLUMNS=80` and `LINES=24`: help is
  checked at 80 columns only, and Python `--help` stays argparse. Every
  candidate run has Python denied (`python_denied: "env"`, below). This
  narrowing needs the owner's sign-off.
- **Verify cases** (`baseline::RUST_03_CASES`, 14 CLI cases plus
  `registry-projection`): `actions-json`, `actions-text`,
  `actions-name-json` (`actions config.set --json`), `actions-prefix-json`
  (`day`), `actions-prefix-text` (`work`), `actions-none-json`,
  `actions-none-text`, `actions-bad-flag` (`actions --bogus`, exit 2),
  `actions-extra-pos` (`actions a b`, exit 2), `help-root`, `help-short`,
  `help-group` (`day --help`), `help-leaf` (`config set --help`) and
  `help-actions`. The two exit-2 cases compare exit and the empty stdout; their
  stderr is pinned in `crates/cbcli/tests/cli.rs`. The registered candidate
  is the release build `target/release/careerbrew-native` (rust-02 stays on
  debug); `measure` records it as `candidate_profile: "release"`. Build it
  first: `cargo build --release -p careerbrew-cli`. The events-on case (one
  `cli.run` row on both sides) is checked by the crate tests and by hand, not
  by verify yet.

#### rust-03 verdict

`decide` for `rust-03` said `activate` (2026-11-19). Numbers, Python oracle
then native candidate:

| Measure | Python | Native |
|---|---|---|
| Cold CPU p95 | 630 ms | 4.2 ms |
| Warm CPU p95 | 142 ms | 4.1 ms |
| Peak RSS | 67 MB | 5.1 MB |

The noise floors are still `unmeasured`, so `decide` used the 1.10 ratio
(task `rust-00-measure-noise-floors`).

So `config/native-routes.txt` now says `actions native`. The console script
runs `careerbrew actions` in `careerbrew-native` by default. Rollback needs no
code change:

- `CAREERBREW_ROUTE_ACTIONS=python` sends `actions` back to Python.
- `CAREERBREW_NATIVE=off` sends every action to Python.
- A missing or broken binary falls back to Python and logs
  `cli.native_fallback`. An exit 64 falls back and logs
  `cli.native_unsupported`.

With `CAREERBREW_HOME` unset, Python's data root is `<repo>/private`. The
binary cannot find the repo on its own, so the console script passes it in
`CAREERBREW_NATIVE_REPO`. The binary then writes its event row under
`<repo>/private`, as Python does. `~` and `~/...` expand from `HOME`, as
Python's `expanduser` does. An empty `CAREERBREW_HOME`, `~user`, or no repo
value exits 64, so Python runs and keeps its own behavior.

### rust-04: Port privacy, policy and approval enforcement

- **Depends on:** `rust-02`, `rust-03`.
- **Boundary:** all data exits and permission decisions.
- **Change:**
  - Port pure canary scanning, role and policy decisions over supplied typed
    records, allowlists, redaction, and approval-receipt validation.
  - **Not here:** receipt storage (`rust-05`); live plugin and machine
    resolution (`rust-07`).
  - Keep deny-by-default handling of malformed configuration.
  - Run these guards before any native mutation or network action.
- **Inputs:** `src/careerbrew/privacy.py`, `approvals.py`, `work/access.py`.
- **Outputs:** the native policy decisions that every later native step calls first.
- **Gate:** 0 canary leaks, unauthorized reads, or role or approval bypasses
  across all positive and negative fixtures.

#### Sub-steps (2026-11-19)

rust-04 ships as six tasks in `TASKS.md`. Preparation tasks may land
before `rust-03`. Candidate tasks wait for `rust-02` and `rust-03`.

| Task | Kind | What it does |
|---|---|---|
| `rust-04a` | preparation | Vector-driver harness: a driver kind in `baseline`, `verify` and `measure`; one scratch vector file per run; positive control, planted defects and refusals. |
| `rust-04b` | preparation | Python deny-by-default: `approvals.validate_records` and `access.validate_directory`, failing tests first. A behaviour change, so its own patch release. |
| `rust-04c` | preparation | Python oracle `tests/oracles/rust04.py`, the vector generator, and `baseline-rust-04.json` committed with the prediction. |
| `rust-04d` | candidate | `crates/policy` core: forbid-IO gate, canary engine, `filter_terms`, allowlists; sets `canary-*` and `allowlists`. |
| `rust-04e` | candidate | Four independent PRs: canary outputs; `public-scrub` alone; approvals; access. |
| `rust-04f` | candidate | 30-pair measure, decide record, "Current state", crate row (production use: none), inventory and source map refresh. |

`activate` is a component verdict only. `scripts/rust-shipped-crates.txt`
stays unchanged until the `rust-05` and `rust-07` closure is native.

#### Ownership split

The crate `crates/policy` does no IO. A gate step rejects `std::fs`,
`std::env`, `std::process`, `std::net`, `SystemTime` and RNG in it. The
caller supplies terms, records, the directory, `now` and `id`.

| Function or concern | Owner step |
|---|---|
| Pure decisions in `privacy.py`, `approvals.py`, `work/access.py` | `rust-04` |
| `approvals.load`, `write_json`, `events.emit` | `rust-05` |
| `privacy._read_terms`, `sensitive_terms` file and plugin resolution | `rust-07` |
| `scan_repo` git plumbing, `_staged_entries`, `_visual_snapshot_png` (moved out of rust-04) | `rust-27` or `rust-27b` |
| `_owner_terminal` (moved out of rust-04) | the CLI or goals step |

The fold and match engine lives once, in `crates/policy`. The rust-27b
leak check depends on it after a rebase. No forked copy.

#### CLI handler table

`src/careerbrew/cli.py` calls `privacy.`, `approvals.` and `access.` at 13
sites. Each row is either ported (calls the policy crate) or deferred.

| # | Handler | Call | Status | Reason |
|---|---|---|---|---|
| 1 | `say` | `privacy.redact` | ported | pure redaction over supplied terms; term loading stays with `rust-07` |
| 2 | `cmd_approvals_request` | `approvals.request` | ported (decision) | `request_item` builds the record; the save is deferred to `rust-05` |
| 3 | `cmd_approvals_request` | `approvals.guidance(5)` | ported | pure over supplied records |
| 4 | `cmd_approvals_guidance` | `approvals.guidance()` | ported | pure over supplied records |
| 5 | `cmd_approvals_list` | `approvals.load` | deferred to `rust-05` | reads the encrypted store |
| 6 | `cmd_approvals_show` | `approvals.get` | ported | pure lookup over validated records |
| 7 | `cmd_approvals_decide` | `approvals.decide` | ported (decision) | `decide_items` keeps Python's check order; the write is deferred to `rust-05` |
| 8 | `cmd_privacy_scan` | `privacy.scan_files` | ported (decision) | matching is ported; file reads and data-root paths are deferred to `rust-05` and `rust-07` |
| 9 | `cmd_privacy_scan` | `privacy.scan_repo` | deferred to `rust-27b` | git plumbing; it calls the shared engine after the rebase |
| 10 | `cmd_draft_route` | `access.route` | ported | insertion order kept |
| 11 | `cmd_draft_route` | `access.load` | deferred to `rust-05` | reads the directory file; `validate_directory` is ported |
| 12 | `cmd_draft_judge` | `access.judge` | ported | pure over supplied directory |
| 13 | `cmd_draft_judge` | `access.load` | deferred to `rust-05` | reads the directory file; `validate_directory` is ported |

The `privacy.PrivacyError` catch in `main` is an exception class, not a
call. The native error keeps the same message and exit code.

Each listed test file maps to vectors: `test_privacy_index.py`,
`test_privacy_scan_file.py` (decision parts), `test_plugins_privacy.py`
(decision parts), and the approvals and access tests.

#### Vector contract

- One generator makes the vectors. xtask runs it once per run into a
  scratch file and passes the same file to both drivers. Only the
  generator is tracked.
- Python driver: `python -I tests/oracles/rust04.py <set> <file>`. Rust
  driver: the `policy-vectors` binary. Both print with the same
  `json.dumps` settings the CLI uses.
- Sets: `canary-core`, `canary-match`, `leak-spans`, `assert-redact`,
  `allowlists`, `public-scrub`, `private-path`, `approvals`,
  `receipt-validate`, `access`, `assert-no-echo`.
- Canaries are synthetic. No output may hold term text (`assert-no-echo`).
- A malformed record gets a fixed, content-free refusal in Python first
  (`rust-04b`). Rust matches those bytes and exit codes. A panic or a
  different refusal is a failure. The out-of-contract table is empty.
- Planted defects, each must fail and name its case: one `EXTRA_CASES`
  entry removed; the `<` removed from the mention lookbehind; the receipt
  validator accepts an unknown status.

#### Vector-driver harness (rust-04a)

`cargo xtask migration vector-check` is the vector-driver kind
(`xtask/src/vecdriver.rs`). It runs the tracked generator once
(`python -I <generator> <file>`) into a scratch file of
`{"sets": {"<set>": [{"id", "op", "input"}]}}`, then gives that same path to
both drivers (`<driver> <set> <file>`, answer shape as in the rust-01 vector
framework). It hashes the file after the generator writes it and again after
every driver run; a driver that changes it is refused, and the record counts
these checks (`vector_file_hash_checks`). The record also names the
candidate that ran (`candidate.kind`: `rust-binary` or `python-test-script`)
and the plant (`candidate.plant`, `none` on a real run). Each driver first answers `unicode-version`; a mismatch records one
failed `unicode-version` case and runs no vector case. Cases are
`<set>:<id>`, compared with the shared `depvec::compare`.

- Exit 0 when every case passes, 1 when a case fails, 2 on a refusal
  (missing driver, generator failure, unreadable or malformed vector file,
  an `--out` that is relative or already exists, checked before the
  generator runs).
- `--generator`, `--driver` and `--test-candidate-script` must resolve
  (after `..` and symlinks) to files inside the repo root; any other path is
  refused with a fixed message that does not echo it.
- On a set run, a candidate that exits non-zero or prints JSON that is not
  a valid answer is not a refusal: each case of that set fails (exit 1). A
  bad Python oracle answer, or no Unicode version from either side, is a
  refusal.
- Planted controls (`--test-plant`): `remove-case`, `change-byte` (one byte of the first
  case's output value, never a key) and `unicode-mismatch`. Each fails and names its case
  (`xtask/tests/vecdriver.rs`, with the synthetic fixtures in
  `xtask/tests/fixtures/vecdriver/`). `migration self-test` runs 4 more
  compare controls.
- `vecdriver::STEPS` registers `rust-04` (`rust-04d`): generator
  `tests/oracles/rust04_vectors.py`, oracle `tests/oracles/rust04.py`,
  candidate `target/release/policy-vectors`, and the sets the candidate covers
  (`canary-core`, `canary-match`, `leak-spans`, `allowlists`). `verify`
  routes the step to this kind and keeps only those sets of the generated
  file; a registered set the file lacks is refused. `baseline` still runs
  the oracle's `--all`; `measure` still refuses the step.

#### Oracle and generator (rust-04c)

- Generator: `python -I tests/oracles/rust04_vectors.py <new-file>` writes
  every set's cases and the synthetic canary terms. It refuses an existing
  file. It reads no clock, randomness or data folder.
- Oracle: `python -I tests/oracles/rust04.py <set> <file>` prints one line,
  `{"set", "unidata_version", "results": [{"id", "output": {"ok": ...} |
  {"error": {"class", "message"}}}]}`. `--all [<file>]` runs every set (from
  the generator in memory when no file is given) and prints only set names,
  the case count, the Unicode version and a SHA-256 of the results.
- The real functions run on supplied data. For one case at a time the
  oracle replaces the approvals store, events, clock and id, and the data
  root and plugin list behind `sensitive_terms`.
- No term text in any output: matched text is a length, a term pattern is a
  hash, a filtered term is its `term_id`. Every output is checked against
  every term pattern; a hit exits 3 and names the case only.
- Until `rust-04b` lands, `validate-records` and `validate-directory` cases
  report `{"validator": "absent"}`.
- `cargo xtask migration baseline --step rust-04` runs the oracle with
  `--all`. `verify` runs the vector-driver kind (`rust-04d`) over all 11
  sets. `measure --step rust-04` runs the generator once into a scratch
  file and times `target/release/policy-vectors --all <file>` (every set,
  same no-echo check, no results digest) against the oracle's
  `--all <file>` over the same file, so neither side's samples include
  vector generation; both commands show the file as `<vectors-file>`
  (`rust-04f`). So measure's oracle args differ from baseline's (`--all`,
  generation in memory). `decide` judges CPU and RSS only on the measure
  record's own paired oracle samples, so the two regimes are never
  compared against each other; the baseline feeds only protocol checks and
  the warm noise floor, which is therefore measured with generation
  included (a slightly wider floor than the measure oracle's).

#### Policy crate core (rust-04d)

- `crates/policy` holds the canary engine and allowlists. `policy-vectors <set> <file>` prints the oracle's answer line in `json.dumps` form; an output that holds term text exits 3 and names the case only.
- Python `str` and `re` rules come from generated tables (`crates/policy/gen_tables.py` writes `src/tables.rs`): `isalnum`, `isalpha`, `isdigit`, `\d`, `isspace`, simple and full lowercase, and `EXTRA_CASES`. NFKC uses `unicode-normalization` (Unicode 18) but cuts the text at each char unassigned in Python's 15.0; the normalization stability policy makes that exact. Known gap: the final-sigma context reads Rust's own Unicode data.
- `filter_terms` keeps the first-seen order for terms of equal length. Python's order there follows the string hash seed, so the vectors avoid ties.
- Gate step `rust-policy-io` refuses `std::fs`, `std::env`, `std::process`, `std::net`, `std::io`, `std::os`, `std::time`, `std::thread`, `SystemTime`, `Instant`, RNG names, `unsafe`, print macros and impure dependencies in the library (not `src/bin/`). Test: `tests/unit/test_policy_no_io.py`.
- Controls (`crates/policy/tests/controls.rs`): removing any load-bearing `EXTRA_CASES` group fails exactly its case, `extra-cases-<index>`.
- Wired: the oracle answers `python -I tests/oracles/rust04.py unicode-version` (`15.0.0` on Python 3.12, as the Rust tables), and `cargo xtask migration verify --step rust-04 --python "$PWD/.venv/bin/python" --out <new-file>` runs `policy-vectors` over every oracle set (11 since `rust-04f`). The record adds `step`, `schema` and `planted_wrong_vector`; `--test-wrong-vector <set>` changes one byte of that set's first answer and must fail naming it (`xtask/tests/rust04_verify.rs`). `--test-oracle`, `--test-scope` and `--test-plant-home-write` are refused for this kind.

#### Policy crate core (rust-04d)

- `crates/policy` holds the canary engine and allowlists. `policy-vectors <set> <file>` prints the oracle's answer line in `json.dumps` form; an output that holds term text exits 3 and names the case only.
- Python `str` and `re` rules come from generated tables (`crates/policy/gen_tables.py` writes `src/tables.rs`): `isalnum`, `isalpha`, `isdigit`, `\d`, `isspace`, simple and full lowercase, and `EXTRA_CASES`. NFKC uses `unicode-normalization` (Unicode 18) but cuts the text at each char unassigned in Python's 15.0; the normalization stability policy makes that exact. Known gap: the final-sigma context reads Rust's own Unicode data.
- `filter_terms` keeps the first-seen order for terms of equal length. Python's order there follows the string hash seed, so the vectors avoid ties.
- Gate step `rust-policy-io` refuses `std::fs`, `std::env`, `std::process`, `std::net`, `std::io`, `std::os`, `std::time`, `std::thread`, `SystemTime`, `Instant`, RNG names, `unsafe`, print macros and impure dependencies in the library (not `src/bin/`). Test: `tests/unit/test_policy_no_io.py`.
- Controls (`crates/policy/tests/controls.rs`): removing any load-bearing `EXTRA_CASES` group fails exactly its case, `extra-cases-<index>`.
- Not wired yet: `vecdriver::STEPS` is still empty and the oracle has no `unicode-version` probe, so `vector-check` and `verify --step rust-04` cannot run the candidate yet.

#### Canary outputs (rust-04e, PR 1 of 4)

- `crates/policy/src/outputs.rs` ports `assert_clean`, `redact` and
  `_names_private_path`. `policy-vectors` now also answers `assert-redact`
  and `private-path`.
- `assert_clean` refuses with `PrivacyError` and the count, the place and
  term ids only. `redact` returns the input unchanged when no term matches;
  otherwise it normalizes once and replaces each term's matches in term
  order on the text so far, like `re.sub`.
- `_names_private_path` follows `(?<![\w.-])private/([\w.-]+)` with
  `finditer` steps; `\w` and `\s` use the Python 3.12 tables.
- Vectors map `test_redact_and_assert_clean` (`test_plugins_privacy.py`)
  and the path tests of `test_privacy_scan_file.py`. `test_privacy_index.py`
  covers the `scan_repo` index reads, which moved to `rust-27`; no vector.
- Byte compare (one-off script, both drivers on one generated file): every
  set the crate answers is byte-identical with the oracle.

#### Scrub, approvals and access (rust-04e, PRs 2 to 4)

- Landed as one change. `policy-vectors` now answers every rust-04 set:
  `public-scrub`, `approvals`, `receipt-validate` and `access` too.
- `pyre.rs` is a small backtracking matcher for the fixed `re` patterns of
  `privacy.py` and `work/access.py`: greedy quantifiers, groups,
  alternation, classes, `\w` `\s` `\d` `\b` `^` `$`, fixed-width lookbehind,
  lookahead and `re.IGNORECASE` (the canary fold, ranges checked on the
  lowercase and its uppercase, like `_sre`). `sub` and `finditer` follow
  sre's empty-match rule. Other syntax is refused when a pattern is built.
- `scrub.rs` ports `scrub_public` with the same pattern text and steps.
  It matched the oracle on the first round, so it stays native; the pivot
  (keep it in Python) was not needed. A replacement with a backslash (a
  `re` template) is out of contract and refused.
- `approvals.rs`: the receipt validator, `request_item`, `decide_items`
  (title or decision check, then the validator, then the first record
  with the id), `pending`, `guidance` (stable newest-first sort) and `get`.
  The caller supplies `now` (an ISO string) and the new id; records keep
  their key order and `dict.update` appends new keys.
- `access.rs`: `validate_directory`, `blocker`, `_topic`, `owner`,
  `existing`, `route` (keys in Python's insertion order), `mentions` and
  `judge`. `str()` of a non-scalar directory value is out of contract.
- Planted defects (`crates/policy/tests/controls.rs`): the `<` removed from
  the mention lookbehind fails exactly `mentions-angle`; a validator that
  accepts an unknown status fails exactly `validate-unknown-status`.
- New vectors map `test_privacy_scan_file.py` (line and match kind in
  `leak-spans`) and `test_public_scrub.py` (bypass, ordinary-text and
  suspects cases, canaries in place of its terms).
- Byte compare (one-off, both drivers on one generated file, 268 cases):
  all 11 sets byte-identical. A one-off random check of 600 scrub texts and
  200 mention texts also agreed. `verify --step rust-04` still waits for
  the step's registration in `vecdriver::STEPS` (rust-04f).
- Review fixes: two new vectors, `scrub-empty-term` (`terms: [""]`:
  the replacement goes in at every empty match, like `re.sub`) and
  `get-unknown-nonprint` (an approval id with `\x85`, `\u2028`, `\xa0`,
  `\u200b` and `\U000e0001`). `py_repr` now escapes every char that
  `str.isprintable` refuses (`text::is_printable`, a Unicode 15.0.0 table
  checked against CPython for every code point). These two vectors are new
  since the 268-case compare and are not yet compared by a driver run.

##### Matcher limits

- A greedy repeat of a one-char node (a char, a class, `\w` `\s` `\d`
  and their negations, `.`) runs in a loop: it counts the longest run,
  then backs off one char at a time, like sre's `REPEAT_ONE`. Stack use
  stays flat, so a 2 MB URL or base64 token is scrubbed without a crash
  (control `a_two_megabyte_token_is_scrubbed_and_searched_without_overflow`).
- A repeat of a wider body (a group or a sequence, such as
  `(?:[a-z0-9-]+\.){2,}`) still recurses once per repeat. A text with
  hundreds of thousands of repeats of such a body can overflow the stack.
  No vector covers it yet.
- `search` tries every start in turn. It builds the empty group record
  once per search and copies groups only when a group took part. A
  pattern that fails late at each start of a long `\w` run stays
  quadratic in the run's length. CPython is quadratic there too:
  `scrub_public` on an 80,000-char `\w` token took 38 s in Python 3.12.
  A caller must bound input length before either side sees it.

##### rust-05 boundary notes

- `approvals.load() or []` (an empty or missing file read as no records)
  is the storage caller's coercion. It belongs to rust-05; the rust-04
  functions take the record list as given.
- Float text in echoed records is out of contract: serde_json and
  `json.dumps` can print the same float differently. It stays out until a
  vector covers it.

#### Unicode version check

The verify record stores Python's `unicodedata.unidata_version` and the
Rust table version. When they differ, verify refuses before any case runs.
Case folding and word splits depend on this table.

#### Traps to remember

- Python `str.split()` and Rust `split_whitespace` differ on some Unicode
  spaces. Port the Python rule, not the Rust default.
- `re.IGNORECASE` is not full Unicode case folding. Match Python's rule.
- An approval id can appear twice with the first copy already decided.
  Keep Python's lookup order, and cover it in `receipt-validate`.
- Vectors come from one generator, never two. Two generators can agree
  with each other and both miss a case.


#### rust-04 verdict

`decide` for `rust-04` said `activate` (2026-11-20). `verify` passed 270 of
270 cases over all 11 sets. Numbers, Python oracle then native candidate (30
pairs per regime, macOS):

| Measure | Python | Native |
|---|---|---|
| Cold CPU p95 | 115 ms | 34 ms |
| Warm CPU p95 | 119 ms | 33 ms |
| Peak RSS | 41 MB | 4.1 MB |

The candidate is the release build: build it first with
`cargo build --release -p careerbrew-policy --bin policy-vectors`. The debug
build used 3.5 times the Python CPU (warm p95 412 ms) and got `keep-python`
with `cpu-regressed`. The noise floors are still `unmeasured`, so `decide`
used the 1.10 ratio. `activate` is a component verdict only: no caller uses
the crate until the rust-05 and rust-07 closure is native, so
`scripts/rust-shipped-crates.txt` stays unchanged.

### rust-05: Port encrypted storage and transaction locks

- **Depends on:** `rust-01`, `rust-02`, `rust-04`.
- **Boundary:** file codecs, key lifecycle, atomic files and cross-process locks.
- **Change:**
  - Keep the encryption envelope, key location and mode, missing-key refusal,
    JSON, YAML and JSONL meanings, atomic replacement, unique temporary names,
    lock paths and write-if-changed.
  - Extract here: the key-path and key-load functions now in `backup.py`,
    settings IO, and minimal content-free event emit. Native storage must never
    depend on later backup, config or event orchestration.
  - Include approval-receipt storage. One implementation per provider.
  - Prove mixed Python and Rust writers under the same advisory lock, and at
    crash and interruption boundaries.
  - Port whole transactions together. Never hold a parent lock while a child
    takes it again.
- **Inputs:** `src/careerbrew/store.py`, `vault.py`, `secrets.py`, `keyfile.py`
  (key provider), `config_io.py` (settings IO), `events.py` (emit primitive), `approvals.py`
  (receipt IO); the disconnected `crates/vault` codec.
- **Outputs:** the native storage engine and key provider.
- **Gate:** 100% old and new round trips; 0 lost or duplicate writes in 1,000
  mixed-writer operations; 0 plaintext secret exits or file-mode regressions.
  Production dispatch waits for the closure in the
  [worked example](#build-order-versus-production-activation).
- **Sub-step `rust-05a` (Python provider split, no behaviour change):**
  the seams the native port takes over now live in their own modules.
  - `keyfile.py` holds `key_path` and `load_key` (path, mode `0600`,
    missing-key refusal). `backup.key_path` and `backup.load_key` stay as
    thin names, so callers and test patches keep working.
  - `config_io.py` holds `read_settings` and `write_settings` for
    `settings.yaml`. `config` keeps the schema, validators and the settings
    lock.
  - `events.emit` is `events.record` (the clean row) then `events.write_row`
    (the append). The `ts` stamp stays on the wall clock.
  - Tests: `tests/unit/test_rust05a_provider_seams.py`.
- **Sub-step `rust-05b` (Python hardening, done first):** the Python oracle
  now holds the contract the Rust port must match.
  - `vault._rewrite` uses a temp name unique to each writer and removes it
    when the swap fails; `store.write_text` removes its temp file too.
  - `backup.load_key` creates a new key with `O_CREAT | O_EXCL` at mode
    `0600`. A first run that loses the race reads the other run's key.
  - `config.set_value` and `config.unset` hold `.settings.yaml.lock` (data
    root) across read, change and write.
  - A plaintext `store.append_jsonl` takes the same `.<name>.lock` that
    `upsert_jsonl` takes.
  - Tests: `tests/unit/test_rust05b_storage_hardening.py`.
- **Sub-step `rust-05b2` (Python hardening):** `approvals.request` and
  `approvals.decide` hold `.approvals.json.lock` (data root) across load,
  change and save, like `upsert_jsonl`. Readers take no lock. The event is
  emitted after the lock is released. Tests:
  `tests/unit/test_rust05b2_approvals_lock.py`.
- **Sub-step `rust-05c` (native key provider, component only):**
  `crates/vault/src/keyfile.rs` ports `keyfile.key_path` and
  `keyfile.load_key`. The caller supplies the facts Python reads from its
  process (`CAREERBREW_KEY_FILE`, home, data root, and whether sealed data
  exists), so the module needs no backup, config or event code.
  - Same contract: default path `~/.config/careerbrew/backup.key`, `~/`
    expansion, refusal inside the data root (non-strict resolve), stripped
    read, missing-key refusal without `create`, refusal when sealed data
    exists, `O_CREAT | O_EXCL` create at mode `0600`, and a race loser reads
    the winner's key (empty after 50 tries of 10 ms is an error).
  - The former `~user/...` gap is closed in rust-05h.
  - Unit tests sit in the module. No production dispatch and no Python
    cross-engine vectors yet.
- **Sub-step `rust-05d` (cross-engine key, native atomic write, component
  only):**
  - `crates/vault/src/atomic.rs` ports `store._tmp_for`, `store.write_text`
    and `store.write_text_if_changed`: a temp sibling
    `.<name>.<pid>.<8 hex>.tmp` made with `O_EXCL` (a name clash takes a new
    name), a rename over the target, the temp removed on any failure, and an
    equal stored text left alone (same bytes and mtime). An unreadable file is
    written again. The caller says whether the path is sealed.
  - `vault-store` (`crates/vault/src/bin/`) is a synthetic-only transport:
    `read`, `write` and `write-if-changed` with an explicit key file and data
    root. It never prints the key.
  - `tests/unit/test_rust05d_key_interop.py`: a key Python's `load_key`
    makes opens a file in Rust, a key Rust's `load_key` makes (mode `0600`)
    opens the Rust-sealed file in Python, missing-key and inside-data-root
    refusals, write-if-changed, and 20 concurrent Rust writers leave one
    whole file and no temp.
  - Why not the vector driver: these cases cross engines through files, so
    the two drivers' answers would not be byte-equal values. The pure
    vector set stays for the codec; this pairs real processes instead.
  - The lock port and the mixed-writer run are in rust-05e (this PR).
  - The former UTF-8 gap is closed in rust-05f: only a vault decode failure
    permits rewrite; invalid UTF-8 and other filesystem failures propagate.

- **Sub-step `rust-05f` (storage, receipts and interactive handlers, component
  only):**
  - `tests/oracles/rust05.py` calls the real Python providers with
    invocation-scoped synthetic adapters. The `rust-05` baseline runs its
    stateful `--all` workload; `target/release/vault-store --all` is the
    registered measure candidate. The custom `xtask/src/rust05.rs` verifier
    compares typed values, decrypted-content digests, modes and envelopes,
    never random ciphertext bytes. It also runs the ten DATA cases. Its record
    carries the shared evidence-source and fixture hashes, including the
    registered DATA corpus fixture, and rechecks those inputs after the runs.
    Failed DATA phases expose only allowlisted `candidate_errors` refusal ids
    and `call_mismatches` phase/index/field ids; raw or unknown process output,
    paths and payload text stay out of the record. Reports are regular files of
    at most 1,000,000 bytes with 1 to 16 calls. At most 16 mismatch entries plus
    one truncation count are retained. Reader defaults and `create_key` use
    the same typed argument encoding as Python, and direct controls compare all
    25 bound argument values with the registered DATA fixture. YAML writes omit
    yaml-rust2's document-start marker, matching Python `safe_dump` bytes; the
    case-4 serialized-text control pins the exact output. Simple quoted YAML
    scalars also use PyYAML's single-quote style rather than yaml-rust2's double
    quotes; escaped or embedded-single-quote forms retain safe double quoting.
    An integrated baseline/verify/measure/decide control builds the registered
    release candidate on a clean runner, keeps Python below the protocol
    minimum and never refuses matching identity. Its two explicit CLI
    refusal cases allow only the observed exits 1 and 3.
  - `crates/vault/src/store.rs` composes the existing key, codec, atomic and
    lock providers. It covers missing defaults, lazy keys, nofollow reads,
    plaintext replay rules and one-lock JSONL append/upsert transactions.
    `write_text_if_changed` now propagates UTF-8 failures; only a vault decode
    failure permits rewrite. No-op text, JSON, JSONL and receipt cases retain
    the exact ciphertext, inode and mtime; `data edit` still rewrites.
  - `canon.rs` and `json_yaml.rs` moved from depvec to vault, with depvec
    delegating so its vector registry is unchanged. JSON integers are
    arbitrary precision and YAML datetimes round-trip. Serde
    `preserve_order` stays disabled.
  - `receipts.rs` reuses `policy::approvals` for every pure decision. Writers
    lock across load/validation/change/write; readers are lock-free; events
    follow a successful write and lock release. `cli.rs` ports
    `cmd_data_edit` and `cmd_secret_set` behind terminal, editor, clock, id,
    event, policy and Keychain adapters. `secrets.rs` owns backend parsing,
    resolution and Keychain storage. A secret enters only hidden interactive
    stdin and never argv, logs, events or files.
  - `tests/unit/test_rust05f_component.py` uses synthetic homes, real PTYs and
    a fake `security` executable. It covers semantic cross-engine reads,
    malformed input, no-op identity, crash points before/after temp write and
    rename, event interruption, retry and lock release. Differential controls
    also cover exact one-field and multi-field objects with serde_json's
    private number key, planted component/DATA stdout and stderr canaries,
    exact getpass newline output and POSIX `a\q` editor parsing. A hard kill may leave a temp sibling; no
    stronger cleanup is claimed.
  - Production dispatch and `scripts/rust-shipped-crates.txt` are unchanged.
    HTTP and role contracts remain scoped out. macOS and Linux noise floors
    are registered as `unmeasured`. This slice does not close `rust-05` or
    activate storage.

- **Sub-step `rust-05g` (native settings and general event IO, component
  only):**
  - `crates/vault/src/settings.rs` ports `settings_path`, `read_settings` and
    `write_settings`. Missing, empty and other falsy YAML values read as no
    overrides; writes retain the existing encrypted atomic replacement and
    PyYAML's `null` spelling.
  - `crates/vault/src/events.rs` ports general `clean`, `record`, `write_row`
    and `emit`. It retains the string-key allowlist, lowercase error classes,
    short lowercase identifiers (including Python `$`'s one-terminal-newline
    exception), arbitrary-size integers, floats and booleans, fixed
    clock/version adapters, sorted plaintext JSON lines and the exact
    `CAREERBREW_EVENTS=off` early return before clock or writer adapters.
    Event-only float output matches `json.dumps`: NaN and infinities, negative
    zero, fixed/scientific thresholds and padded exponents. Storage JSON's
    former nonfinite refusal is closed in rust-05h; event bytes and finite
    storage JSON retain their existing behavior.
  - `tests/oracles/rust05.py` now exercises these providers beside the existing
    receipts. The `receipt-events` case still contains only the original
    request and decision rows. `tests/unit/test_rust05g_component.py` checks
    exact Python/Rust reports, sealed settings, rewrite and disabled-event
    identity, sorted output, identifier boundaries, exact float bytes and
    privacy. All rust-05f cases and the ten DATA
    cases remain in the same verifier and measurement workload.
  - Bulk vault/status helpers remain out of scope. Production routes,
    shipped crates and storage activation are unchanged.

- **Sub-step `rust-05h` (store helpers, key paths and data-show paths,
  component only):**
  - `crates/vault/src/helpers.rs` ports `store.outline`, `store.digest` and
    `store.cached`. Structure output retains insertion order, schema/data-key
    rules, strict business-map masking, merged list/map sizes, scalar and
    date/datetime types, JSON/YAML/JSONL line-only parse failures, text line
    counts and sealed-file reporting without values. Python `$`'s one-terminal-
    newline exception retains the original key text; 48/49-character and
    one/two-newline controls pin the boundary. Digest keys retain each
    part's sorted compact Python JSON, nonfinite floats, `default=str` and NUL
    delimiter. The encrypted cache retains content-derived paths, fixed-clock
    TTL checks, lazy compute, semantic values and exact hit identity.
  - The shared `json_yaml` storage boundary now matches Python `json.loads`
    and `json.dumps` for `NaN`, `Infinity`, `-Infinity`, negative zero and
    exponent forms. A lexical adapter replaces only nonfinite tokens outside
    strings for Serde validation and restores typed floats from the original
    syntax; arbitrary-precision integers and ordinary strings retain their
    existing paths. Real store write/read/no-op/outline and cached
    miss/hit/fresh/expiry controls carry all seven numeric cases, including an
    arbitrary-precision integer. A quoted-token near-miss proves strings stay
    unchanged. YAML and event behavior are unchanged.
  - `keyfile.rs` now expands `~user` and `~user/...` through the system account
    database in addition to `~` and `~/`. An unknown named user raises the
    native equivalent of Python's `RuntimeError`; the current account is
    covered on both hosted operating systems. Repeated slashes and lexical
    `..` suffixes retain Python's `Path.expanduser` result without an absolute
    `join` discarding the selected home or canonicalization changing `..`.
  - `cli.rs` adds component-only `cmd_data_show`. It removes one exact leading
    `private/` only at this boundary, then retains canonical containment and
    regular-file checks, restricted business output, personal-path allowlist,
    structure-only `--keys`, line-only parse diagnostics and content-free
    events. The Python issue-366 workload disables events and dynamically
    denies process, socket and secret adapters. The rust-05i follow-up adds
    native handler-internal denied-host instrumentation: all 15 groups complete
    with zero attempts, and one planted operation per channel enters the real
    `cmd_data_show` path at path resolution, personal-data policy or store read.
    The denied boundary records the attempt and refuses it before the operation
    closure executes. This proves mediation at those three injected boundaries;
    it does not claim to detect a future direct host call that bypasses them. All four primary and eleven control groups compare
    exact safe output and exit codes, and the whole synthetic parent snapshot
    retains every file's bytes and mtime. The rust-05 verifier's invocation-
    scoped observed-files walker records a retained symlink as the digest of
    its `read_link` target bytes without following it, including a dangling-
    link control. Shared capture and source-tree scans remain strict.
  - `tests/oracles/rust05.py` and
    `tests/unit/test_rust05h_component.py` compare the complete stateful
    Python/Rust reports, including retained settings, events, receipts,
    storage, no-op identities and all ten DATA cases. New source files are in
    `docs/plans/rust-inventory.d/rust-05h.csv`; the rust-05 baseline and
    semantic verifier bind them.
  - Production Python and routes, shipped crates and activation tables are
    unchanged. This slice does not close `rust-05` or activate storage.

- **Sub-step `rust-05i` (bulk vault/status and native denied-host controls,
  component only):**
  - `crates/vault/src/bulk.rs` ports `vault.files`, `status`, `seal_files`,
    `encrypt_all` and `decrypt_all`. Enumeration is independent of encryption
    mode, excludes plaintext-by-design directories, replay checkouts, symlinks
    and lock/temp files, and retains sealable near-miss replay paths. Explicit
    decryption includes real replay checkouts. Status reports mixed plaintext
    and sealed counts without reading values into the report.
  - Backup/sync suffix checks are case-insensitive; a raw Fernet bundle with no
    suffix is recognized too. Every bundle refusal happens before marker or
    file mutation. Encryption removes the marker only after that preflight;
    decryption writes the exact marker only after every selected rewrite
    succeeds. Missing keys are never replaced when sealed data exists; invalid
    keys retain Python's `ValueError`, while missing or wrong-key sealed-data
    refusals retain `VaultError`.
  - Each changed file uses a unique `.<name>.<pid>.<hex>.vault.tmp` sibling,
    becomes mode `0600`, and replaces the target atomically. A caught fault
    removes its temp file, preserves earlier completed rewrites, and prevents
    later targets from starting. Already-sealed encryption targets and already-
    plaintext decryption targets retain their bytes, inode and mtime. Repeated
    bulk operations retain those no-op identities; the marker is the only
    deliberate repeat-decryption write.
  - The synthetic-only `vault-store --bulk-faults` mode plants one write, flush,
    chmod or rename fault at the second of three targets for each of
    `seal_files`, `encrypt_all` and `decrypt_all`. All 12 faults remove the
    temporary file, preserve the completed first rewrite and the failed/later
    targets' exact prior identities, stop before the third target, and retain the
    operation's marker timing. Each retry completes the remaining two rewrites
    and reads back all three values. A separate before-rename mutation control
    records the existing per-file replacement behavior: the pending rename wins.
    This is not concurrent-mutation detection, vault-wide rollback or atomicity.
  - `cli.rs` has invocation-scoped `HostOperations` process, socket and secret
    boundaries inside component `cmd_data_show`: before path resolution, before
    personal-data policy and before store read. Normal data show performs no
    host operation, and all 15 issue-366 groups individually report zero
    attempts. Each planted control calls the real handler with one injected
    operation at the matching boundary. The denied adapter records exactly one
    attempt and returns its typed refusal without executing the operation. The
    Python oracle drives equivalent attempts through `cli.main` at the same
    three provider boundaries. This is dynamic evidence for those explicit
    mediation points only; a direct call that bypasses all injected boundaries
    remains outside the proof. No business value or operation error text enters
    the report.
  - `tests/oracles/rust05.py` and
    `tests/unit/test_rust05i_component.py` compare 19 bulk cases plus the
    denied-host case, including semantic state digests, modes, key presence,
    marker state, failure ordering and mutation-free refusals. Raw Fernet
    bundle output is normalized as a semantic bundle marker rather than
    hashing random token bytes. Every rust-05f/g/h case and all ten DATA cases
    remain in the same verifier and measurement workload. New source files are
    in `docs/plans/rust-inventory.d/rust-05i.csv` and the rust-05 evidence hash.
  - Production Python and routes, shipped crates and activation tables are
    unchanged. This slice does not close `rust-05` or activate storage.

- **Sub-step `rust-05e` (native advisory lock, mixed writers, component
  only):**
  - `crates/vault/src/lock.rs` ports the `store.append_jsonl` lock: an
    exclusive `flock` (`File::lock`) on the sibling `.<name>.lock`, opened in
    append mode and created if missing. A sealed append reads, adds and
    rewrites with `atomic::write_text` under the lock; a plain append just
    appends. `vault-store append` exposes it. No production dispatch.
  - `tests/unit/test_rust05e_mixed_writers.py`: 4 Python and 4 Rust writers
    make 1,000 appends to one sealed JSONL file; it opens and holds exactly
    1,000 distinct ids. A plain variant runs 200 appends.

- **rust-05 acceptance (2026-11-22, component only):**
  - One clean macOS chain at source revision
    `4c142884f2ab4590b7e3e69ec32a35aa998500d4` used release candidate SHA-256
    `cc6be50bc8f3cafc2a82bc8e9e503f4029009644a4ade9007fbb023e267fc013`.
    All three records share evidence-source hash
    `eb1f8bb5f5198e32434e0f2d241e5af7e872f21f0e8060b9ebd10d20fbfe3484`
    and fixture hash
    `1ab332eedbd15af660070a102f4dda8cc0ffefa6d66b5dc42554ce80a0474b3d`;
    verify and measure also share the candidate hash.
  - Baseline: 30 runs. Verify: 13/13 checks, including all ten DATA cases,
    with 0 violations. Measurement: 30 unique, uncached pairs in each regime.
    Cold CPU p95 was 822,211 to 84,154 microseconds; warm was 635,346 to
    77,824. Paired cold tree-RSS maximum was 87,834,624 to 4,816,896 bytes;
    warm was 63,324,160 to 4,833,280. Each ratio is below 1.10. The registered
    noise floors remain `unmeasured`, so the ratio applies fail-closed.
  - `migration decide --step rust-05` returned `activate` with no reasons.
    PR #215 also passed both hosted OS gates and public-readiness on the same
    source. This closes the component build/measurement dependency for
    rust-06. It does **not** activate production storage: that switch still
    requires the rust-02 + rust-04 + rust-05 + rust-07 activation closure,
    fresh acceptance records and rollback evidence.

### rust-06: Port backup, restore and cross-machine sync

- **Depends on:** `rust-05`.
- **Boundary:** encrypted archives and explicit data transfer.
- **Change:**
  - Port archive verification and restore, with strict base64 and rejection of
    trailing junk, conflict handling, and the sync allowlists.
  - Verify that backups made by either engine restore under both.
  - Any format change is a separate decision record and a separate measured task.
- **Inputs:** `src/careerbrew/backup.py`, `sync.py`.
- **Outputs:** native backup, restore and sync.
- **Gate:** 100% of valid archives restore; 100% of tampered, truncated or
  trailing-junk archives are rejected; 0 unapproved exports or lost files.
- **Component slice (native adapters, no production dispatch):**
  - `crates/vault/src/backup.rs` ports archive creation, strict verification,
    restore and retention. `sync.rs` ports pairing keys, the two-setting export
    allowlist, and corporate import validation. `transfer.rs` adapts
    `cmd_backup_create`, `cmd_backup_restore`, `cmd_backup_verify`,
    `cmd_data_status`, `cmd_pair_join`, `cmd_sync_export` and `cmd_sync_import`.
    The synthetic-only binary is `vault-sync`; it is not a shipped crate or a
    production route.
  - Archive base64 decoding is strict, including after completed padding. This
    intentionally remains different from the lenient vault-token reader.
    Restore validates every member, every corporate data rule and every
    existing destination link before the first write. `machine.yaml` does not
    cross machines. Backup, pairing and sync destinations stay outside the data
    root; new archives and keys use mode 0600. Corporate cloud detection keeps
    the exact Python set, including `pCloud Drive` and `Sync`, for both create
    and restore.
  - Ladder reads follow Python's source order: base resources, sorted locked
    plugins, then user ladders. Every source uses the encrypting store and its
    key adapter, including user ladders. The plugin adapter reads each lock
    entry and manifest, carries `origin` and source path, and rejects missing,
    escaping or symlinked resources before import. Validation requires id,
    levels and competencies, every competency field, an expectation per level,
    and mapping-shaped descriptions/examples. Differential cases apply valid
    plugin and encrypted user ladders, then prove escape, symlink and malformed
    resource refusals.
  - `tests/oracles/rust06.py` and `synthetic_transfer.rs` run twelve stateful
    groups: roundtrip and conflicts, strict rejection, malicious names and
    links, corporate restore, key/target guards, sync roundtrip/refusals,
    import allowlists, plugin ladders, user ladders and all seven successful
    handler outputs. `xtask` also
    creates a real archive with each engine, then verifies and restores it with
    the other. Both directions compare restored content and modes directly.
  - Random Fernet ciphertext and gzip bytes are not compared. The report uses
    typed results, decrypted content hashes, modes, semantic file state and
    error classes. Only the volatile archive timestamp/hash in CLI output is
    normalized, to `careerbrew-<archive>.cbk`; no other output is normalized.
    Reports contain no key, bundle plaintext or synthetic record text.
  - Verify runs every group as a phase in one owned synthetic home. It compares
    each phase's semantic write map and per-path write counts, including inode
    and modification time for same-byte rewrites. Extra, changed, missing and
    duplicate writes increment their own counters; policy output mismatches
    also increment role or authorization. Real extra, missing and duplicate
    native-write plants must each make `decide` return `keep-python`.
  - Write normalization is explicit: archive names, generated keys, sealed
    records and bundle ciphertext become type markers; plaintext YAML/JSON is
    hashed by parsed typed value; other plaintext keeps its content hash. A
    symlink target is normalized only after proving it stays inside the owned
    root. No absolute path or private value enters the record.
  - The step binds the Python oracle and call closure, native adapters, reused
    rust-05 providers, harness sources, candidate bytes and the generic ladder
    fixture. Baseline, verify and measure share source revision, evidence-source
    and fixture hashes; verify and measure also share candidate SHA-256. Both
    noise floors remain `unmeasured`, so the 1.10 ratio applies fail-closed.
  - Production Python, `config/native-routes.txt`, shipped crates and activation
    tables are unchanged. Component parity does not activate backup or sync.
- **Component measurement after all review fixes (2026-11-23, macOS):**
  - Clean source revision `82155d47240295fc0afd6e1c7bd2a480abbd37f6`.
    All three records share evidence-source hash
    `2436e1e3e21aade145bcb37b9e7c309cfd2419de033431429e0361557063ff38`
    and fixture hash
    `6d97d35d117ce9225bb95ac1d92dd2376b02fb25b588a5ea73f94526dcb82ad5`.
    Verify and measure share release-candidate SHA-256
    `25ef7a6b6fb3c8ff32b6dc03668a44b40c61267c617739225964526dcbbb59f2`.
    The earlier `394f7641` and `c1910a6b` verdicts are superseded and do not
    cover these bytes.
  - Baseline: 30 runs. Verify: 13/13 checks with 0 contract, privacy, role,
    authorization, side-effect, missing-write or duplicate-write violations.
    Measurement: 30 unique, uncached pairs in each regime.
  - Cold CPU p95 was 280,341 to 76,755 microseconds; warm was 285,561 to
    80,266. Paired cold tree-RSS maximum was 46,858,240 to 8,552,448 bytes;
    warm was 46,907,392 to 8,568,832. Each ratio is below 1.10. The
    unmeasured noise floors keep the ratio fail-closed.
  - `migration decide --step rust-06` returned `activate` with no reasons.
    Real extra, missing and duplicate native-write controls each returned
    `keep-python` with `verify-check-failed` and `verify-violation`; their
    side-effect, missing-write and duplicate-write counters respectively are
    nonzero. This is the component verdict only. Production backup, sync and
    storage routes remain Python; activation still needs the complete native
    closure, fresh acceptance records and rollback evidence.

### rust-07: Port plugin, machine and account setup

- **Depends on:** `rust-04`, `rust-05`.
- **Boundary:** plugin manifest, machine role and credential adapters.
- **Change:**
  - Port plugin validation, policy merging, corporate readiness, secret and
    OAuth references, and the preflight and self-update ownership checks.
  - Keep symlinks, modes, the managed-config refusal and explicit repair
    confirmation.
  - Tokens stay in local stores. Never put them in fixtures or subprocess logs.
- **Inputs:** `src/careerbrew/plugins.py`, `machine.py`, `msauth.py`,
  `workday/mcp_check.py`, `workday/self_update.py`, `workday/workflow_check.py`;
  the plugin fixtures and the sign-in HTTP fixtures (owner map).
- **Outputs:** native live policy, the third provider of the approval closure.
- **Gate:** 100% of readiness, manifest and repair cases agree; 0 secret
  disclosures, silent repairs or unmanaged configuration writes.
- **Component slice (native adapters, no production dispatch):**
  - `crates/setup/` holds manifest validation, strictest policy merge, plugin
    install/refresh/verify/remove, command-source ownership, machine role and
    readiness, OAuth classification and references, MCP preflight, managed
    repair, self-update and workflow checks. `cli.rs` adapts all eleven owned
    handlers: connectors list, init, machine set/show/split-plugin-evidence,
    pair init, and plugin add/list/refresh/remove/verify.
  - The synthetic-only binary is `setup-component`; it is not a shipped crate
    or a production route. It reuses the vault store, key, YAML and pairing
    providers. Plugin copies remain plaintext and encrypted records remain
    sealed. The connection-time GitHub helper is native, discards stderr,
    bounds output and time, and never logs the credential.
  - `tests/oracles/rust07.py` and `synthetic.rs` run nine stateful groups:
    manifest/policy, plugin lifecycle, command ownership, machine readiness,
    OAuth references, MCP preflight, managed repair, update/workflow and all
    CLI handlers. Cases cover symlink refusal and preservation, mode 0600
    metadata and 0640 repair targets, managed-config refusal, explicit `--yes`,
    owner-edit race checks, strictest policy and dependency order. Direct
    controls cover real pyproject version and work-step validation, boolean
    `requires`, exact corporate output cleanup, non-dry evidence moves,
    duplicates, preserved YAML fields and exact CLI success/refusal spacing.
    Further cases purge inbox items under their lock, refuse job-hunt or
    malformed goals from every collection, distinguish verify from cycle
    interpreter rules, reuse vault secret references, install from `~/` and a
    real local Git source, and refresh that source. Repair fixtures use owned
    executable success/failure scripts, not assumed system binary paths.
  - Verify compares typed output and semantic file state after every phase,
    including inode and modification time. Sealed records and generated keys
    become type markers; owned paths, timestamps and the engine-specific
    connection-helper command are normalized. A source fixture's internal Git
    bookkeeping is excluded; installed content and hashes are still compared.
    No credential value or local path enters the record. Real extra, missing and duplicate native-write
    plants must each make `decide` return `keep-python`.
  - The step binds the Python oracle and call closure, native modules, every
    reused vault provider source, harness, candidate bytes, plugin fixtures,
    sign-in fixtures and profile examples. A vault-only edit changes the
    evidence identity and rebuilds the dependent release candidate.
    Both noise floors remain `unmeasured`, so the 1.10 ratio applies
    fail-closed. Production Python, native routes, shipped crates and
    activation tables are unchanged.
- **Slice `rust-07b` (preparation):** pure decisions over supplied facts:
  `plugins.validate_manifest_data`, `merge_policy`, `refresh_order`;
  `machine.readiness_rows`, `cloud_service`, `per_user`, `guard_decision`,
  `suggest_decision`; `msauth.classify_poll`, `classify_refresh`. The old
  names probe, then call them.
- **Slice `rust-07c` (preparation):** `mcp_check.check_rows` decides over
  facts from `check_facts`; `plugins.installed()` re-checks every installed
  manifest with `validate_manifest` and refuses an invalid one (`remove` skips
  the check so the owner can still uninstall it).
- **Component oracle baseline before native implementation (2026-11-23, macOS):**
  clean source revision `acd35d36be893766d0653385c2308dec4b12f5c3`; 30 runs;
  CPU p50/p95 260,867/276,440 microseconds and tree-RSS p50/p95
  46,268,416/46,661,632 bytes. Evidence-source hash
  `374c894f3fc6c5f29261b8c48cc81472bda79aeef0531bc7222850afc4929e05`;
  fixture hash `940d3c5822659c7eaef80649a92c7a7582b3d6662f330a5fe12da3b8d1d3c1a2`.
  The baseline covers nine synthetic groups, including all eleven owned CLI
  handlers. This is the pre-native prediction record, not a speed result.
- **Component measurement (2026-11-23, macOS):**
  - Clean source revision `25ca1f273f19561eb5cbce03950d2634fee9e97d`.
    All three records share evidence-source hash
    `1e48227d826d1b01f30ca31688d541b1d4751b9540956c74537da4f1ad78b896`
    and fixture hash
    `940d3c5822659c7eaef80649a92c7a7582b3d6662f330a5fe12da3b8d1d3c1a2`.
    Verify and measure share release-candidate SHA-256
    `dd6830917ab0a532e45070308c89af11c3386ee5c45baba443a250b4ac1e9166`.
  - Baseline: 30 runs. Verify: 9/9 checks with 0 contract, privacy, role,
    authorization, side-effect, missing-write or duplicate-write violations.
    Measurement: 30 unique, uncached pairs in each regime.
  - Cold CPU p95 was 902,112 to 640,713 microseconds; warm was 896,527 to
    641,173. Cold tree-RSS p95 was 47,874,048 to 31,473,664 bytes; warm was
    47,710,208 to 31,490,048. Each ratio is below 1.10. The unmeasured noise
    floors keep the ratio fail-closed.
  - `migration decide --step rust-07` returned `activate` with no reasons.
    Real extra, missing and duplicate native-write controls each returned
    `keep-python` with `verify-check-failed` and `verify-violation`; their
    side-effect, missing-write and duplicate-write counters respectively are
    nonzero. This is the component verdict only. Production plugin, machine,
    account and storage routes remain unchanged; activation still needs the
    complete native closure, fresh acceptance records and rollback evidence.

### rust-08: Port HTTP sources, calendar and mail

- **Depends on:** `rust-04`, `rust-05`, `rust-07`.
- **Boundary:** network adapters and source cursors.
- **Change:**
  - Port bounded HTTP, retries, errors, auth and host validation, cursor
    handling, calendars and recurrence.
  - Replay the recorded synthetic HTTP fixtures. No live accounts.
  - Keep source-local failure isolation and the terms restrictions.
- **Inputs:** `src/careerbrew/connectors.py`, `calendar.py`, `mail.py`; the
  HTTP and calendar fixtures (owner map).
- **Outputs:** native connectors and recurrence.
- **Gate:** 100% of source and recurrence vectors agree; 0 wrong-host
  credentials, duplicate cursor items or cross-source suppression.
- **Component pre-registration (2026-11-23):** source base
  `027f9d80f4884f3758df535a837c033d8dcbc0e0`. Six differential groups cover
  HTTP/auth/host and redirect policy; connector evidence and bounded cursors;
  ICS/Graph calendar recurrence; Graph/JMAP/IMAP mail metadata and
  classification; source-local failures and literal-secret refusal; and the
  owned `connect` CLI handler. The suite replays every registered current
  fixture identity and adds only synthetic cursor, recurrence and refusal
  variants. It never uses a live account.
- **Comparison and normalization:** exact typed JSON stdout and exit status;
  semantic filesystem state and per-phase write counts. Owned paths normalize
  to `<owned>`, sealed records to type markers, and generated credentials never
  enter a report. No byte normalization hides a contract difference.
- **Prediction:** 100% agreement with zero privacy, policy, role,
  source-isolation, side-effect, missing-write or duplicate-write violations.
  Native process-tree CPU p95 and peak RSS stay at or below 1.10 times the
  matching baseline in both cache regimes. Both noise floors start unmeasured,
  so the ratio remains fail-closed. Any disagreement, violation or higher ratio
  keeps Python and requires a newly registered approach. This is component
  parity only: production routes, shipped crates and activation stay unchanged.
- **Component oracle baseline before native implementation (2026-11-23, macOS):**
  clean source revision `f00bc99357183904149c99288ef55418e4a0dd01`; 30 runs;
  CPU p50/p95 276,371/296,157 microseconds and tree-RSS p50/p95
  56,655,872/56,868,864 bytes. Evidence-source hash
  `ce97203f9f8a41ac40ee32676a098ef973ff08cf2ab7c96cf8e16f2499241b60`;
  fixture hash `6c6e83b98ed7ef0a84d82f6477a29704679d0eb44d5758b858a0e3f19d1f4439`.
  This is the pre-native prediction record, not a speed result.
- **Component implementation:** `crates/sources` provides bounded recorded HTTP,
  host and redirect authorization, source evidence and cursor adapters, ICS/Graph
  recurrence, Graph/JMAP/IMAP metadata, source-local failure isolation and the
  `connect` component handler. `sources-component` is synthetic-only. The custom
  verifier compares exact typed output, semantic writes and per-phase write
  transitions; planted extra, missing and duplicate writes must fail. Production
  Python, native routes, shipped crates and activation tables are unchanged.
- **Component measurement (2026-11-23, macOS):**
  - Clean source revision `23992167c93f1218f64a27c1b392833ee7b4fff8`.
    All three records share evidence-source hash
    `58289a84b6bcfded6e9f75829261fd46b1b059b5f8fb9d2c3640f1b6a80c5339`
    and fixture hash
    `6c6e83b98ed7ef0a84d82f6477a29704679d0eb44d5758b858a0e3f19d1f4439`.
    Verify and measure share release-candidate SHA-256
    `b4d1b4a1ba70be850a1d66b2a4554d3cbd275f11ceb90bba6d23d5c429c2771f`.
  - Baseline: 30 runs. Verify: 6/6 checks with zero contract, privacy, role,
    authorization, side-effect, missing-write or duplicate-write violations.
    Measurement: 30 unique, uncached pairs in each regime.
  - Cold CPU p95 was 292,729 to 12,072 microseconds; warm was 294,053 to
    11,926. Cold tree-RSS p95 was 56,934,400 to 9,977,856 bytes; warm was
    56,508,416 to 9,994,240. Each ratio is below 1.10. The unmeasured noise
    floors keep the ratio fail-closed.
  - `migration decide --step rust-08` returned `activate` with no reasons.
    Real extra, missing and duplicate native-write controls each returned
    `keep-python` with `verify-check-failed` and `verify-violation`; their
    side-effect, missing-write and duplicate-write counters respectively are
    nonzero. This is the component verdict only. Production source, calendar,
    mail and storage routes remain unchanged; activation still needs the
    complete native closure, fresh acceptance records and rollback evidence.
- **Review-correction pre-registration:** restrictive `allowed_hosts` controls
  cover connector, calendar, mail and `connect` calls; moved and cancelled
  RECURRENCE-ID controls cross the 2026-11-01 DST boundary; mail near-misses
  include `assessments` and `offering`. These controls fail the original
  candidate. Success keeps every prior vector and adds zero differential or
  privacy violations. Any host-policy bypass, duplicate master/exception,
  classification drift or resource ratio above 1.10 rejects the correction.
  Fresh baseline, verify, measure and decide records are required after the
  source change. Production activation remains unchanged.
- **Review-correction acceptance (2026-11-23, macOS):** clean source revision
  `a51632d96f3e570d783ec787bee4973fda67d93d`; evidence-source hash
  `4053bb14602326c5300fd7524639017f021d47d7ac4ae3e668a3ed7e7eb7435d`;
  fixture hash
  `6c6e83b98ed7ef0a84d82f6477a29704679d0eb44d5758b858a0e3f19d1f4439`;
  release-candidate SHA-256
  `c21fb93ac40451bb17367d3ab4f25dcb7149ca8e586a79209ee7a8e4162d6c05`.
  Verify passes 6/6 with zero violations. Cold CPU p95 is 319,796 to 15,015
  microseconds; warm is 351,931 to 16,159. Cold tree-RSS p95 is 58,458,112 to
  11,304,960 bytes; warm is 58,425,344 to 11,485,184. All 30 pairs per regime
  meet the 1.10 limit and `decide` returns component-only `activate`. Fresh
  extra, missing and duplicate write controls return `keep-python` with their
  respective counters nonzero. No route or activation state changes.

### rust-09: Port projects, priorities and promises

- **Depends on:** `rust-05`, `rust-07`.
- **Boundary:** project records and atomic project mutations.
- **Change:**
  - Port the project lifecycle, configured ticket identity, readiness gates,
    priority estimates and promise closure.
  - Keep finite-number checks, idempotent IDs, owner-edited closures and lock scope.
- **Inputs:** `src/careerbrew/work/projects.py`, `priorities.py`,
  `project_priority.py`, `promises.py`, `focus.py`, `team.py`.
- **Outputs:** native project writers under the project-file lock.
- **Gate:** 100% of project, gate and promise contracts agree; 0 duplicate
  promises, lost owner edits, or false ticket matches on `UTF-8` or `ISO-8601`.
- **Component pre-registration (2026-11-23):** source revision
  `6c40ef57b8a0d84c3f3ccc4f6a86f180314b224e`; Python oracle
  `tests/oracles/rust09.py`; native assertion `tests/unit/test_rust09_component.py`;
  fixtures `tests/fixtures/projects/cutline.json` and `team-goals.csv`. The six
  cases are `project-lifecycle`, `readiness-gates`, `priorities-tickets`,
  `promise-lifecycle`, `lock-and-team` and `cli-handlers`. They cover the four
  assigned CLI handlers and the assertions of `test_issue_230.py` and
  `test_issue_429.py`. The clock is fixed at `2026-11-02T09:00:00+00:00`.
  Reports use typed JSON; random incarnation IDs are compared only by shape
  and stability. Differential writes use semantic YAML values, invocation-owned
  paths and sealed-record markers, not ciphertext bytes. Baseline B is 30 fresh
  Python runs. Verify requires all six cases with zero privacy, side-effect,
  missing-write or duplicate-write violations. Measure requires 30 unique
  interleaved pairs in each cold and warm regime. CPU-time p95 and process-tree
  peak RSS must each be at most 1.10 x the paired oracle in each regime; any
  contract mismatch, duplicate promise, lost owner edit, false standard ticket
  or resource excess keeps Python. This is component parity only: production
  dispatch, shipped crates and activation state do not change.
- **Component acceptance (2026-11-23, macOS):** clean source revision
  `c2162b4d6b61363e574e9e5830eaa95e2712dc48`; evidence-source hash
  `d48421f10328f1d1f9b1b4f509d40b0bd879e71262df8227159e953da1030c51`;
  fixture hash
  `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `5c904021c407bf20b8691759eb2a1daf2bf20a1ed75417985a94d8a7d24a327d`.
  Baseline: 30 runs. Verify: 6/6 checks with zero contract, privacy,
  side-effect, missing-write or duplicate-write violations. Measurement: 30
  unique, uncached pairs in each regime. Cold CPU p95 was 329,757 to 48,909
  microseconds; warm was 316,996 to 47,573. Cold tree-RSS p95 was 51,265,536
  to 11,960,320 bytes; warm was 50,987,008 to 11,960,320. Each ratio is below
  1.10; the unmeasured noise floors apply that ratio without relaxation.
  `migration decide --step rust-09` returned component-only `activate` with no
  reasons. Fresh extra, missing and duplicate native-write controls each
  returned `keep-python` with `verify-check-failed` and `verify-violation`;
  their respective violation counters are nonzero. Production routes, shipped
  crates and activation remain unchanged; activation needs the complete native
  closure, fresh acceptance records and rollback evidence.
- **Review-correction pre-registration (2026-11-23):** source revision
  `7763c8cc50003e8558675b041c5de98482cd4012` adds direct controls for reused
  vault/setup evidence sources, Python ties-even rounding (`1.125` to `1.12`,
  negative and near ties), missing/invalid/repeated project identity repair,
  real daily-result first-match counts, and measured-impact normalization,
  evidence merging, note creation, encryption, modes and locks. Success keeps
  every prior contract, passes all six cases with zero violations and rejects
  every planted write. Fresh baseline, verify, 30-pair cold/warm measure and
  decide records are required after the source change; CPU p95 and tree RSS
  stay at or below 1.10 times their paired oracle. Any mismatch, lost control
  or resource excess keeps Python. Production activation remains unchanged.
- **Review-correction acceptance (2026-11-23, macOS):** clean source revision
  `dbaf46888935dcb84d5807a5c6c852cd3b0a7cab`; evidence-source hash
  `d3394204d04a81a57760e48317a0ef9dce934af2a25715b8183652842494cba0`;
  fixture hash
  `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `10a36eb286ecb8fb5e6125088026684a77f95aa7dad87042df24eb0aefa83655`.
  Baseline: 30 runs. Verify: 6/6 with zero violations, including every new
  direct control. Cold CPU p95 was 396,001 to 62,066 microseconds; warm was
  391,579 to 61,530. Cold tree-RSS p95 was 56,426,496 to 12,124,160 bytes;
  warm was 56,393,728 to 12,238,848. Thirty unique uncached pairs ran in each
  regime and each ratio is below 1.10. `decide` returned component-only
  `activate`; fresh extra, missing and duplicate write controls returned
  `keep-python` with their violation counters nonzero. Production routes,
  shipped crates and activation remain unchanged.
- **Second review-correction pre-registration (2026-11-23):** source revision
  `bef47bc5c222df48cbd87d76a633e9a496776d3d` adds a non-string 32-digit YAML
  identity control and evidence read/decrypt/parse failure controls. The identity
  must be a string of 32 lowercase hexadecimal characters. An evidence failure
  falls back to empty URLs while daily results still count by title and ID;
  result reads, writes and privacy checks remain strict. Success keeps all six
  cases and prior controls with zero violations, every planted write failing,
  and fresh 30-pair CPU/RSS ratios at most 1.10. Any disagreement or unexpected
  swallowed result error keeps Python. Production activation remains unchanged.
- **Second review-correction acceptance (2026-11-23, macOS):** clean source
  revision `6dd8c3eb64a386959ac851ee2805e185f40bacbe`; evidence-source hash
  `3c4fb8786c7cb865c8648aa3390017d30e50877eb16c77cfde40bcc1f79e2add`;
  fixture hash
  `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `e32dcd027fa0f264f7594b098db539e6010472955eb128c9d53df013d3956cbe`.
  Baseline: 30 runs. Verify: 6/6 with zero violations. The numeric identity is
  refused and every evidence read/decrypt/parse control retains the same daily
  counts and final writes, including the ID-only API result. Cold CPU p95 was
  444,504 to 89,940 microseconds; warm was 444,897 to 82,749. Cold tree-RSS p95
  was 60,309,504 to 12,812,288 bytes; warm was 60,506,112 to 12,337,152. Thirty unique uncached pairs ran
  in each regime and each ratio is below 1.10. `decide` returned component-only
  `activate`; all fresh planted-write controls returned `keep-python`.
  Production routes, shipped crates and activation remain unchanged.
- **External-rewrite and local-time acceptance (2026-11-23, macOS):** clean source
  `3f7b06d59b2773e70c528b7ee8575879dfaeff8f`; evidence-source hash
  `48d151216f14ecc89d2c355946d947908228b36cd53e0f04d450187ee2cae480`;
  fixture hash
  `6078f7e2b225285d1c80e90d19fe1aae91235ed89c7d9803da559dc54d6bcd42`;
  release-candidate SHA-256
  `54469e9ee2dd2361f4646bd59c98d098bbfd133f9e53f14815f4baf2a21b659d`.
  The standalone control uses a real external file and distinct synthetic data
  root, pins and restores its environment, preserves mode `0600`, and carries
  an owner append. Local-time controls resolve the configured or system zone,
  including an unset `TZ`, an unpinned clock, near-midnight notes and malformed
  Unicode without a panic. Baseline: 30 runs. Verify: 6/6 with zero violations.
  Cold CPU p95 was 445,732 to 326,920 microseconds; warm was 428,280 to 305,156.
  Paired cold tree-RSS maximum was 53,542,912 to 14,909,440 bytes; warm was
  53,428,224 to 14,696,448. Thirty unique uncached pairs ran in each regime and
  both ratios are below 1.10. The clean rerun returned component-only
  `activate` with no reasons. Production routes, shipped crates and activation
  remain unchanged.
- **Seventh-case registration correction (2026-11-23):** `local-time` is now in
  the native `synthetic::CASES`, `xtask/src/rust11.rs` `CASE_IDS` and the direct
  Python component test's expected set. The earlier 6/6 verify record did not
  cover local-time behavior. Acceptance now requires a clean 7/7 differential
  report with zero violations and a fresh 30-pair resource chain. A direct
  differential isolated the first 7/7 failure to section-header dates:
  Python returns an aware pinned timestamp unchanged, so native `local_now`
  now retains that offset; only naive pinned values receive the local zone.
  The semantic write comparison normalizes only the named unpinned fixture
  row's real-clock timestamp (`local:3` / `PROJ-1 unpinned`); its output and all
  other timestamps remain strict.
- **Seven-case acceptance (2026-11-23, macOS):** clean source
  `61aacad55b4f6da3df4f70ee2ff2220da255fa1b`; evidence-source hash
  `1c5b3cc04c405a11f41bbe9b7ac1c1f74e4b0510ae737348f504f6b62b4cd6bb`;
  fixture hash
  `6078f7e2b225285d1c80e90d19fe1aae91235ed89c7d9803da559dc54d6bcd42`;
  release-candidate SHA-256
  `9a926643a42fcc17f4215345ca8331653b4720275a2095c69a87cf1201532309`.
  Baseline: 30 runs. Verify: 7/7 with zero violations, including `local-time`.
  Cold CPU p95 was 407,528 to 296,053 microseconds; warm was 406,301 to 290,063.
  Paired cold tree-RSS maximum was 53,411,840 to 14,336,000 bytes; warm was
  53,510,144 to 14,008,320. Thirty unique uncached pairs ran in each regime and
  both ratios are below 1.10. `decide` returned component-only `activate` with
  no reasons. Production routes, shipped crates and activation remain unchanged.

### rust-10: Port sessions, claims, agents and reply drafts

- **Depends on:** `rust-09`.
- **Boundary:** session ownership and per-target write transactions.
- **Change:**
  - Port scope and claim checks, the activity lifecycle, expiry, and the draft
    clear, save and read-back locks.
  - Include the full-scope delivery checks and every fix that lands while this
    plan runs.
  - Keep the origin-session requirement and the newest-thread-state checks.
- **Inputs:** `src/careerbrew/work/sessions.py`, `agents.py`, `drafts.py`.
- **Outputs:** native sessions, claims and the safe draft save.
- **Gate:** 100% of session and draft cases agree; 0 cross-session writes,
  stale claims or conflicting target saves in 1,000 interleaved operations.
- **Component pre-registration (2026-11-23):** source revision
  `bd890013004e533d2c3b769a117b96ca3fec8f5e`; Python oracle
  `tests/oracles/rust10.py`; native assertion `tests/unit/test_rust10_component.py`.
  The six cases are `session-lifecycle`, `claims-and-drafts`, `agent-activity`,
  `reply-fit`, `prior-selection` and `cli-handlers`. They cover scope/claim checks,
  full-scope delivery instructions, activity restart/quiet/expiry, clear-save-
  read-back locks, origin-session identity, newest-thread state, and the six
  assigned CLI handlers. The clock and every home are invocation-owned and
  synthetic. Reports use typed JSON; random record IDs are compared by shape,
  uniqueness and stability. Differential writes use semantic JSON/JSONL values,
  owned paths and sealed-record markers, not ciphertext bytes. The final review
  correction uses 1,000 real claim/draft transactions with native lock controls.
  Baseline B is 30 fresh Python runs. Verify requires all six cases with zero
  privacy, side-effect, missing-write or duplicate-write violations. Measure
  requires 30 unique interleaved pairs in each cold and warm regime. CPU-time
  p95 and process-tree peak RSS must each be at most 1.10 x the paired oracle in
  each regime; any contract mismatch, cross-session write, stale claim,
  conflicting target save or resource excess keeps Python. This is component
  parity only: production dispatch, shipped crates and activation do not change.
- **Component oracle baseline before native implementation (2026-11-23, macOS):**
  clean source revision `1570ead209ecb4f6f8e1aa44fe23e96f5e26ab96`; 30 runs;
  CPU p50/p95 738,892/764,237 microseconds and tree-RSS p50/p95
  60,456,960/61,063,168 bytes. Evidence-source hash
  `ddcc848972979ac44fecb2d4e867ac30ad8e66276fe479c4688d69ba3dbdd429`;
  fixture hash `7d57e9b013641666e53d3a910d072222add8ec7041bade0ed0eee56dc67e797a`.
  This is the pre-native prediction record, not a speed result.
- **Component implementation:** `crates/sessions` provides session and claim
  transactions, loop/handoff lifecycle, activity restart/quiet/expiry, prior-day
  origin selection, newest-thread reply decisions, per-target draft locks and the
  six assigned component CLI adapters. `sessions-component` is synthetic-only.
  The custom verifier compares exact typed output, semantic writes and per-phase
  write transitions; planted extra, missing and duplicate writes must fail.
  Production Python, native routes, shipped crates and activation are unchanged.
- **Component acceptance (2026-11-23, macOS):** clean source revision
  `07f4e18a903dacda40cb6974f9e8c86d0d6f6d1c`; evidence-source hash
  `acda0222ee11b40e2c3a35b387d08935f3d06a3c38bc9d8467bdd74ab187243d`;
  fixture hash `5a22d252c0cd1fd4e2235907960fa9b7570ea1dd348af41f9bcb9ec715cf4895`;
  release-candidate SHA-256
  `946bb97799bf1ce325986758414b88926f91401ef99d7442a68ecc5692292531`.
  Baseline: 30 runs. Verify: 6/6 checks with zero contract, privacy,
  side-effect, missing-write or duplicate-write violations. Measurement: 30
  unique, uncached pairs in each regime. Cold CPU p95 was 735,739 to 156,838
  microseconds; warm was 736,824 to 156,492. Cold tree-RSS p95 was 60,555,264
  to 11,550,720 bytes; warm was 60,702,720 to 11,550,720. Each ratio is below
  1.10; the unmeasured noise floors apply that ratio without relaxation.
  `migration decide --step rust-10` returned component-only `activate` with no
  reasons. Fresh extra, missing and duplicate native-write controls each
  returned `keep-python` with `verify-check-failed` and `verify-violation`;
  their respective violation counters are nonzero. The native hosted test also
  executes 1,000 locked draft operations. Production routes, shipped crates and
  activation remain unchanged; activation needs the complete native closure,
  fresh acceptance records and rollback evidence. Integrated main and this
  review correction make the `07f4e18a` measurement historical, not valid for
  the final candidate.
- **Review-correction pre-registration (2026-11-23):** source revision
  `1026e1253491e3a5b2412e71266374274fb8085f` replaces the standalone claim map
  and sessionless draft writes with 1,000 real alternating take, contested-save,
  holder-save and release transactions. Two live origin sessions contend on each
  operation cycle; observed contentions must total 250, while cross-session
  writes, stale claims and conflicting target saves remain zero. A forced-write
  negative control must make the relevant counters nonzero. The duplicate plant
  moves to a separately observed write of an existing expected draft path: its
  semantic map and side-effect count stay unchanged, and its actual duplicate
  count is nonzero. Success retains all six ordinary cases and requires fresh
  integrated baseline, verify, 30-pair cold/warm measure and decide records at
  the same source, with CPU/RSS p95 at most 1.10 times the paired oracle. Any
  missing contention, counter violation, extra-path rejection or resource excess
  keeps Python. Production activation remains unchanged.
- **Review-correction acceptance (2026-11-23, macOS):** clean measured source
  `45b03e87ec2df31e1e4915acb0a72cb9bd25fbf4`; evidence-source hash
  `efafe802b692394c5648a95f1ea7a5d922403b630aa6c7698e2c5dc8b727f832`;
  fixture hash `5a22d252c0cd1fd4e2235907960fa9b7570ea1dd348af41f9bcb9ec715cf4895`;
  release-candidate SHA-256
  `b95dc95a60db6cf03e289b3ca4a2603661e5ec427e32e5cac4a7eb48d3f4a817`.
  Verify passes 6/6 with zero violations. The real transaction workload records
  1,000 operations, 250 contentions and 250 holder saves, with zero cross-session
  writes, stale claims or conflicting saves; its forced-write control makes the
  cross-session and conflict counters nonzero. The expected-path duplicate
  control records duplicate count 2 with zero contract, side-effect, missing-write
  or privacy violations. Baseline: 30 runs. Measurement: 30 unique, uncached
  pairs in each regime. Cold CPU p95 was 2,906,440 to 1,242,809 microseconds;
  warm was 2,780,748 to 1,260,299. Cold tree-RSS p95 was 63,094,784 to
  11,894,784 bytes; warm was 62,980,096 to 11,927,552. Each ratio is below 1.10.
  `decide` returned component-only `activate` with no reasons. Fresh extra,
  missing and duplicate controls each returned `keep-python` with
  `verify-check-failed` and `verify-violation`. Production routes, shipped crates
  and activation remain unchanged.
- **Hosted-review fixture acceptance (2026-11-23, macOS):** clean measured source
  `167a283f2968736f624afbd2c1f7ebcafd733a12`; evidence-source hash
  `8ac6130001aa8bee78290993fc9d7527c7f61a22677e408f940976e2566fd402`;
  fixture hash `5a22d252c0cd1fd4e2235907960fa9b7570ea1dd348af41f9bcb9ec715cf4895`;
  release-candidate SHA-256
  `cdbd0fceb92c34d1ef65fef9c31958d64552f2c83a5ae12f8d64cd76d5af92b9`.
  Both engines select the `pr-8` run by identity and report it quiet. Verify passes
  6/6 with zero violations. Baseline: 30 runs. Measurement: 30 unique, uncached
  pairs in each regime. Cold CPU p95 was 2,819,397 to 1,232,401 microseconds;
  warm was 2,854,529 to 1,235,597. Cold tree-RSS p95 was 63,160,320 to
  12,173,312 bytes; warm was 63,160,320 to 12,206,080. Each ratio is below 1.10.
  `decide` returned component-only `activate`; fresh extra, missing and duplicate
  controls returned `keep-python`. The crate table marks `careerbrew-sessions`
  `none: component parity only`; routes, shipped crates and activation are unchanged.

### rust-11: Port project imports, exact source links and memory sync

- **Depends on:** `rust-08`, `rust-09`.
- **Boundary:** source identity and owner-managed memory files.
- **Change:**
  - Port the exact URL and repository identity rules, imported project mapping,
    and digest-based memory sync.
  - Keep file modes and concurrent owner appends. Never widen an item URL to
    repository scope.
- **Inputs:** `src/careerbrew/work/research.py`, `project_import.py`, `memory_sync.py`.
- **Outputs:** native imports and memory sync.
- **Gate:** 100% of identity and sync fixtures agree; 0 wrong-project matches,
  lost appends or changed file modes.
- **Component pre-registration (2026-11-23):** source revision
  `4782c2f5dc870e803f9c1b32d8c399af92f8e1ff`; six fixed synthetic cases cover
  source identity, project imports, memory input, memory output, removal and
  research/CLI handlers. Exact typed results and semantic write maps are
  compared per phase; only owned paths, generated keys, random incarnation ids
  and sealed-record bytes are normalized. Thirty fresh Python runs establish
  baseline B. Verify requires all six cases with zero contract, privacy,
  side-effect, missing-write or duplicate-write violations. Measure requires
  30 unique interleaved pairs in each cold and warm regime. CPU-time p95 and
  process-tree peak RSS must each be at most 1.10 x the paired oracle in each
  regime; unmeasured noise floors use that ratio without relaxation. Any
  mismatch, wrong-project match, lost append, changed file mode, privacy or
  write violation, or resource excess keeps Python. This is component parity
  only: production dispatch, shipped crates and activation do not change.
- **Component oracle baseline before native implementation (2026-11-23, macOS):**
  source revision `4782c2f5dc870e803f9c1b32d8c399af92f8e1ff`; 30 runs;
  CPU p50/p95 659,087/719,078 microseconds and tree-RSS p50/p95
  75,661,312/76,546,048 bytes. Evidence-source hash
  `ca13daf196ac5017950b00bf5fbad0603a94bbabe670e3661a6d2c4838d13311`;
  fixture hash `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`.
  This is the pre-native prediction record, not a speed result.
- **Component implementation:** `crates/memory` provides exact item/repository
  identity, all-or-nothing CSV/JSON/Sheets imports, digest-based bidirectional
  memory sync, owner-append-preserving removal, and import/absorb/check-since/
  sync component CLI adapters. `memory-component` is synthetic-only. The custom
  verifier compares exact typed output, semantic writes and per-phase write
  transitions; planted extra, missing and duplicate writes must fail.
  Production Python, native routes, shipped crates and activation are unchanged.
- **Component acceptance (2026-11-23, macOS):** clean source revision
  `e9124bdb17c519beff534dd18675460ae152707e`; evidence-source hash
  `7ac2be7b8db1e42686c70c31e87f0fd8c822263edd6742e431a1550e1bc60328`;
  fixture hash `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `f2279115380af1974d4b23ed8316201ac037178b9b886f1de3f5813b2efe8a4b`.
  Baseline: 30 runs. Verify: 6/6 checks with zero contract, privacy,
  side-effect, missing-write or duplicate-write violations. Measurement: 30
  unique, uncached pairs in each regime. Cold CPU p95 was 375,988 to 187,147
  microseconds; warm was 350,239 to 188,572. Cold tree-RSS p95 was 56,754,176
  to 12,419,072 bytes; warm was 51,347,456 to 12,419,072. Each ratio is below
  1.10; the unmeasured noise floors apply that ratio without relaxation.
  `migration decide --step rust-11` returned component-only `activate` with no
  reasons. Fresh extra, missing and duplicate native-write controls each
  returned `keep-python` with `verify-check-failed` and `verify-violation`;
  their respective violation counters are nonzero. Production routes, shipped
  crates and activation remain unchanged; activation needs the complete native
  closure, fresh acceptance records and rollback evidence. The later time/alias
  correction makes the `e9124bdb` measurement historical, not valid for the
  final candidate.
- **Review-correction acceptance (2026-11-23, macOS):** clean measured source
  `1779d5ece017b793818c8bfffc3badb33759cee8`; evidence-source hash
  `5675d3b6bd28388939eae4fd446e2e8dfcda5b2e0379ad52ff64b0ddded3e524`;
  fixture hash `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `854641a8ae16599a6a474458445794f2c675b61fb85bbcd1db4bbdeb370d74d0`.
  Naive and second-resolution timestamps, Slack aliases and owner-text removal
  now match Python. Verify passes 6/6 with zero violations. Baseline: 30 runs.
  Measurement: 30 unique, uncached pairs in each regime. Cold CPU p95 was
  394,546 to 206,022 microseconds; warm was 403,479 to 210,327. Cold tree-RSS
  p95 was 51,773,440 to 13,402,112 bytes; warm was 51,789,824 to 13,271,040.
  Each ratio is below 1.10. `decide` returned component-only `activate`; fresh
  extra, missing and duplicate controls returned `keep-python`. Production
  routes, shipped crates and activation remain unchanged.
- **Hosted-lint correction acceptance (2026-11-24, macOS):** clean measured
  source `aa969d15639a8913959e8535270c9329c1b1aea7`; evidence-source hash
  `f1baeeb0836f7dfc56224f679e69ebfa05864af22249c2264a71c35b470a0583`;
  fixture hash `eadc98973473f7d9562f3e748c53abf34a9f6bc4698efc05d54b381f3db5ae8f`;
  release-candidate SHA-256
  `063663b919de19be1419de29203262130c9b36fa7bb631ebf467e43355f7ee56`.
  The eight hosted Clippy repairs change no behavior. Baseline: 30 runs. Verify
  passes 6/6 with zero violations. Measurement: 30 unique, uncached pairs in
  each regime. Cold CPU p95 was 387,176 to 206,597 microseconds; warm was
  369,880 to 211,143. Cold tree-RSS p95 was 51,920,896 to 13,025,280 bytes;
  warm was 51,707,904 to 13,500,416. Each ratio is below 1.10. `decide`
  returned component-only `activate`; fresh extra, missing and duplicate
  controls returned `keep-python`. Production routes, shipped crates and
  activation remain unchanged.
- **Second-review acceptance (2026-11-24, macOS):** clean measured source
  `d7f00d7e900077d707b99435a1f2c68f569ffc18`; evidence-source hash
  `02e5953d03a4aca4f40be3492defa97fedced26a1bb869f280d3ff1d3847ac2d`;
  fixture hash `6078f7e2b225285d1c80e90d19fe1aae91235ed89c7d9803da559dc54d6bcd42`;
  release-candidate SHA-256
  `08bd6ff334d4fb15c4590b3a65a7c68cf51809a712d1b0e67efb3989e6be8777`.
  Direct controls prove Python `ensure_ascii` digest continuity, Unicode item
  digits, malformed-Unicode timestamp refusal, remote UTF-8 BOM removal,
  local-date and mixed-offset instant ordering, canonical source aliases, and
  semantic readback of the final encrypted import. Baseline: 30 runs. Verify
  passes 6/6 with zero violations. Measurement: 30 unique, uncached pairs in
  each regime. Cold CPU p95 was 374,226 to 267,614 microseconds; warm was
  370,083 to 268,258. The acceptance memory statistic is tree-RSS p95, not the
  maximum: cold p95 was 52,756,480 to 13,828,096 bytes and warm p95 was
  52,690,944 to 13,844,480. The actual paired maxima were 53,493,760 to
  14,909,440 bytes cold and 53,510,144 to 14,778,368 bytes warm. Each p95 ratio
  is below 1.10. `decide`
  returned component-only `activate`; fresh extra, missing and duplicate
  controls returned `keep-python`. Production routes, shipped crates and
  activation remain unchanged.

### rust-12: Port project learning, replay, tours and explainers

- **Depends on:** `rust-09`, `rust-11`.
- **Boundary:** learning records and generated project artifacts.
- **Change:**
  - Port project learning, the replay and tour caches, the explanation
    contracts and PRD validation.
  - Keep prompts and reference material declarative, and source hashes stable.
  - Keep replay and local project material private.
- **Inputs:** `src/careerbrew/learn/`, `work/explain.py`, `work/prd.py`.
- **Outputs:** native learning and explainer builders.
- **Gate:** 100% of deterministic learning, replay and PRD contracts agree;
  0 new data exits; 0 repeated computation for unchanged inputs.
- **Component pre-registration (2026-11-24):** Python oracle
  `tests/oracles/rust12.py`; native assertion
  `tests/unit/test_rust12_component.py`. Six fixed synthetic cases cover source
  staging, concept validation, learning/scheduling, replay, tours and
  explainer/PRD CLI handlers. Exact typed results and semantic write maps are
  compared per phase. Only owned paths, generated keys, sealed-record bytes,
  replay Git internals and FSRS's random due-interval/memory representation are
  normalized; learning choices, mastery, retests, review counts, privacy and
  every other output remain strict. Unchanged staged inputs must not rewrite
  their stored content. Thirty fresh Python runs establish baseline B. Verify
  requires all six cases with zero contract, privacy, side-effect,
  missing-write or duplicate-write violations. Measure requires 30 unique
  interleaved pairs in each cold and warm regime. CPU-time p95 and process-tree
  peak RSS must each be at most 1.10 x the paired oracle in each regime;
  unmeasured noise floors use that ratio without relaxation. Any mismatch, new
  data exit, repeated unchanged-input write or resource excess keeps Python.
  This is component parity only: production dispatch, shipped crates and
  activation do not change.
- **Component oracle baseline before native implementation (2026-11-24, macOS):**
  clean source revision `008993072bc861d80b59b44b5eb484723516228e`; 30 runs;
  CPU p50/p95 6,983,766/7,132,482 microseconds and tree-RSS p50/p95
  71,647,232/72,089,600 bytes. Evidence-source hash
  `e3a206c8a6260e72288a7ef69427f42ed8d4658661c5e792b94281dd0188c040`;
  fixture hash `438cab9a16bcebdf143a1d49f3e5424c60e42ab1ac3c7340967ad9de03c1c7bc`.
  This is the pre-native prediction record, not a speed result.
- **Component implementation:** `crates/learn` provides source staging and
  citation checks, mastery/retests and project-scoped FSRS decks, isolated PR
  replay, fading tours, explainer/PRD validators and scaffolds, and component
  CLI adapters. Learning identities use Python-compatible full Unicode case
  folding, and retention excludes reviews of removed concepts.
  `learn-component` is synthetic-only. The custom verifier
  compares exact typed output, semantic writes and per-phase write transitions;
  planted extra, missing and duplicate writes must fail. Production Python,
  native routes, shipped crates and activation are unchanged.
- **Component acceptance (2026-11-24, macOS):** clean measured source
  `837a5907eb50a0840ffd8eb89b2898af55a447ad`; evidence-source hash
  `a38416cdb3bf0ffe515d187ac9e5c741634e4fef096df889bfccbf2ea5dd17b4`;
  fixture hash `438cab9a16bcebdf143a1d49f3e5424c60e42ab1ac3c7340967ad9de03c1c7bc`;
  release-candidate SHA-256
  `6229f42410f377a9ff26f70b3c1115ad7f4ae7a5cfe87db66952ed260649e36a`.
  Baseline: 30 runs; CPU p50/p95 5,185,416/5,254,055 microseconds and tree-RSS
  p50/p95 67,223,552/67,829,760 bytes. Verify passes 6/6 with zero contract,
  privacy, side-effect, missing-write or duplicate-write violations. Measurement:
  30 unique, uncached pairs in each regime. Cold CPU p95 was 9,225,618 to
  3,944,666 microseconds; warm was 5,033,498 to 3,846,202. Cold tree-RSS p95
  was 67,551,232 to 31,604,736 bytes; warm was 67,584,000 to 31,621,120. Each
  ratio is below 1.10; the unmeasured noise floors apply that ratio without
  relaxation. `migration decide --step rust-12` returned component-only
  `activate` with no reasons. Fresh extra, missing and duplicate native-write
  controls each returned `keep-python` with `verify-check-failed` and
  `verify-violation`. Production routes, shipped crates and activation remain
  unchanged; activation needs the complete native closure, fresh acceptance
  records and rollback evidence.
- **Review-correction acceptance (2026-11-24, macOS):** clean measured source
  `bc4f403f69caf5afe992f6d30c09d2f40af8e7f1`; evidence-source hash
  `50c2e3fb2dbc31f0482c80eafe9771f85113d1a9f0d7dc6f4904223d5b8cb58e`;
  fixture hash `438cab9a16bcebdf143a1d49f3e5424c60e42ab1ac3c7340967ad9de03c1c7bc`;
  release-candidate SHA-256
  `56cb50ca89154556931ec9e6cc088cd6ecc2e4b5c0d5e2277ed127330004d1e0`.
  ICU4X full case folding matches Python for `U+A7D0` and `Straße`; short-mode
  citations retain their sources, and the vault synthetic key-loader source is
  bound. Baseline: 30 runs; CPU p50/p95 5,362,879/5,438,987 microseconds and
  tree-RSS p50/p95 67,485,696/67,829,760 bytes. Verify passes 6/6 with zero
  violations. Measurement: 30 unique, uncached pairs in each regime. Cold CPU
  p95 was 8,919,980 to 3,903,879 microseconds; warm was 5,041,374 to 3,802,999.
  Cold tree-RSS p95 was 67,158,016 to 31,604,736 bytes; warm was 67,420,160 to
  31,604,736. Each ratio is below 1.10. `decide` returned component-only
  `activate`; fresh extra, missing and duplicate controls returned
  `keep-python` with `verify-check-failed` and `verify-violation`. Production
  routes, shipped crates and activation remain unchanged.
- **Hosted unit-control correction (2026-11-24):** both OS check parts failed
  only at the final raw-output comparison. A direct fresh synthetic comparison
  found identical parsed results; the raw JSON retained different owned
  `python/` and `rust/` root paths. The unit control now replaces each lexical
  and canonical owned root with `<owned>` before its byte comparison. Candidate
  bytes and every source bound by the review-correction acceptance record are
  unchanged, so that identity-bound acceptance chain remains valid. Hosted CI
  must still pass on the corrected head.

### rust-13: Port feed ingestion and transcript normalization

- **Depends on:** `rust-08`, `rust-09`.
- **Boundary:** raw events, original payloads and cursor transactions.
- **Change:**
  - Port source ingestion, raw tool replies, retained original payloads,
    edited-message upserts and deduplication.
  - Keep synthetic Slack DM channel metadata, source timestamps and the
    content-free cycle bookkeeping.
- **Inputs:** `src/careerbrew/work/feed.py`, `transcript.py`; the feed,
  team-feed and transcript fixtures (owner map).
- **Outputs:** native feed ingestion.
- **Gate:** 100% of event vectors agree; 0 dropped edits, duplicate events or
  idle change-version churn.
- **Component pre-registration (2026-11-24):** source revision
  `8b041f81eed739c98b95b39e611bf7aa58850276`; Python oracle
  `tests/oracles/rust13.py`. Eight fixed synthetic cases cover recorded source
  ingestion, edited upserts, raw ticket/mail/calendar replies, Claude and Codex
  transcripts, saved-output containment and `since`, open-ask/cycle closure, and
  team feeds. Exact typed results and semantic write maps are compared per
  phase. Only owned paths, generated keys and sealed-record bytes are
  normalized; event facts, source timestamps, cycle counts, privacy and write
  transitions remain strict. Thirty fresh Python runs establish baseline B.
  Verify requires all eight cases with zero contract, privacy, side-effect,
  missing-write or duplicate-write violations. Measure requires 30 unique
  interleaved pairs in each cold and warm regime. CPU-time p95 and process-tree
  peak RSS must each be at most 1.10 x the paired oracle in each regime;
  unmeasured noise floors use that ratio without relaxation. Any mismatch,
  dropped edit, duplicate event, idle write, containment or resource violation
  keeps Python. Not-yet-native health/live/completion and promise/evidence
  integration stay behind typed component providers. This is component parity
  only: production dispatch, shipped crates and activation do not change.
- **Component implementation:** `crates/feed` provides recorded Slack,
  PagerDuty and Outlook readers with configured/plugin source identities, raw
  ticket/mail/calendar conversion, exact original/analysis upserts,
  Claude/Codex transcript normalization, link and open-ask rules, project-aware
  team matching, partial transport retention and component CLI adapters. It reuses vault
  storage, atomic writes, exact locks and events, recorded source transport and
  native team matching. `feed-component` is synthetic-only. The custom verifier
  compares exact typed output, semantic writes, per-phase write transitions and
  every storage-write operation; planted extra, missing, phase-transition and
  same-phase duplicate writes must fail. Production Python,
  native routes, shipped crates and activation are unchanged.
- **Review-correction pre-registration (2026-11-25):** source revision
  `f0fd1132720c13165837c11de686ad2bfdbe2140` adds non-UTC naive/aware event
  cases, local workday boundaries, NaN/infinity controls and redacted bounded
  connector failures. The verifier also compares every storage-write operation,
  not only before/after state. Success remains 100% exact typed and write
  parity, zero privacy/side-effect violations and resource ratios at most 1.10;
  any mismatch, raw error leak, accepted non-finite timestamp or hidden write
  keeps Python. The hosted gap assertion must count every configured silent
  source. Production activation remains unchanged.
- **Review-correction acceptance (2026-11-25, macOS):** clean measured source
  `fdd2b506074b318528cd639c0cf119d7a602a449`; evidence-source hash
  `054f593cf37ddffdfa6fbeca7a835c7422486d5998a6ca4e882cec86de481dcc`;
  fixture hash `8bf29c5928700145219dd63b425271ade19e5c191df9aef1829e2357d8ebd583`;
  release-candidate SHA-256
  `9cee9a54238280ffa43ae025c7c78cdd7453da890f13f45995b8c04743a25af3`.
  Baseline: 30 runs; CPU p50/p95 308,097/332,005 microseconds and tree-RSS
  p50/p95 52,609,024/53,018,624 bytes. Verify passes 10/10 with zero contract,
  privacy, side-effect, missing-write or duplicate-write violations, including
  local-time/non-finite and redacted-source-failure cases. Measurement: 30
  unique, uncached pairs in each regime. Cold CPU p95 was 335,071 to 101,557
  microseconds; warm was 333,577 to 98,039. Cold tree-RSS p95 was 52,936,704 to
  13,058,048 bytes; warm was 52,854,784 to 13,025,280. Each ratio is below
  1.10. `decide` returned component-only `activate`; fresh extra, missing,
  prior-phase duplicate and same-phase operation-duplicate controls returned
  `keep-python`. Production routes, shipped crates and activation remain
  unchanged; activation needs the complete native closure and rollback evidence.
- **Second-review pre-registration (2026-11-25):** source revision
  `185aef0c8088f53922c993c7ded8cb89a6788475` adds configured source/plugin
  inventory and origin identity, project-linked and archived teammate controls,
  and partial Slack retention after teammate-search/reply transport failures.
  Direct output, operation and semantic-write parity remain mandatory; source
  errors emit only content-free diagnostics. All cases must pass with zero
  violations and each 30-pair resource ratio must remain at most 1.10. Any
  omitted source, wrong project, lost partial event or leaked error keeps Python.
  Event-log comparisons normalize only the wall-clock `ts` value; source event
  timestamps and every diagnostic field remain strict.
- **Privacy-review pre-registration (2026-11-25):** source revision
  `b161172d7d73543e20d91a862d9dfab40dd1abb1` adds an installed plugin's
  manifest-owned sensitive term beside a data-root term. Connector failures must
  redact both before the existing 200-character bound. The plugin path comes
  from the validated manifest below its own root, never from an unchanged lock
  entry. Any raw canary, unsafe path, or other regression keeps Python. All 12
  cases and four write controls stay mandatory, with fresh 30-pair resource
  acceptance and no activation change.
- **Third-review pre-registration (2026-11-25):** source revision
  `b9609ad334a54c2e05ba290e484b11d5ae4292b4` adds an absent plugin sensitive
  file, system `/etc/localtime`, colon-prefixed `TZ`, and a nonexistent DST wall
  time. Missing valid files supply no terms; containment remains mandatory.
  Reuse the rust-11 local-zone and fold=0 adapters instead of a silent UTC
  fallback. Bind every executed setup/native adapter, including `setup/lib.rs`.
  All 12 cases, four write controls, and 30-pair resource limits remain in force.
  Browser code stays unchanged. Fresh acceptance and hosted claims must name
  their exact head, with pending checks never described as green.
- **Third-review acceptance (2026-11-25, macOS):** clean measured source
  `62c321dde979738b9be76d47c172c79f47372caf`; evidence-source hash
  `d8ec1ce8f1e3436bf739969cd652326afccf57f986701e0d7d868f6a5ebe53d5`;
  fixture hash `8bf29c5928700145219dd63b425271ade19e5c191df9aef1829e2357d8ebd583`;
  release-candidate SHA-256
  `eed9d6df4f04c84c0fac6f7ecbbaf6d122eba00d1e71786ac37460d8825cac0d`.
  Baseline: 30 runs; CPU p50/p95 384,630/410,962 microseconds and tree-RSS
  p50/p95 53,231,616/53,592,064 bytes. Verify passes 12/12 with zero violations,
  including absent plugin files, system/colon timezones and DST gaps. Cold CPU
  p95 was 412,191 to 157,896 microseconds; warm was 412,247 to 157,820. Cold
  tree-RSS p95 was 53,624,832 to 14,450,688 bytes; warm was 53,821,440 to
  14,417,920. Each ratio is below 1.10. `decide` returned component-only
  `activate`; all four write controls returned `keep-python`. Hosted checks
  remain pending until a fresh run passes. Production routes, shipped crates
  and activation remain unchanged.
- **Final-review pre-registration (2026-11-25):** source revision
  `ce46adf14035174d921f59ac9c546d9881dc54cd` binds `policy/tables.rs`, which
  the policy text/canary code reads. Complete write tracing now covers the
  vault's direct event-log append as well as `Store::write_text`. A planted
  same-phase event-log duplicate must fail with a duplicate-write violation;
  ordinary event content and timestamps follow the existing normalization.
  All 12 cases, existing write controls, clean same-revision acceptance and
  1.10 resource limits remain mandatory. No behavior or activation changes.
- **Final-review acceptance (2026-11-25, macOS):** clean measured source
  `ded91e9af578be56c9188a0d38a404fed818f6c3`; evidence-source hash
  `cbb4a80609ece5909bd668799ab49393aab1e40cb57b45dd95f1a093211c7b6e`;
  fixture hash `8bf29c5928700145219dd63b425271ade19e5c191df9aef1829e2357d8ebd583`;
  release-candidate SHA-256
  `237895cea272b2ef699a3808bf2e1ad8dbfb1c2dc917f81ee673c49466092e79`.
  Baseline: 30 runs; CPU p50/p95 383,706/413,130 microseconds and tree-RSS
  p50/p95 53,264,384/53,592,064 bytes. Verify passes 12/12 with zero violations.
  Cold CPU p95 was 410,227 to 154,796 microseconds; warm was 410,713 to 160,171.
  Cold tree-RSS p95 was 53,477,376 to 14,073,856 bytes; warm was 53,460,992 to
  14,516,224. Each ratio is below 1.10. `decide` returned component-only
  `activate`; extra, missing, phase-transition duplicate, same-phase storage
  duplicate and direct event duplicate controls returned `keep-python`.
  Production routes, shipped crates and activation remain unchanged. Hosted
  claims remain pending until a fresh exact-head run passes.

### rust-14: Port live listening, review asks and notifications

- **Depends on:** `rust-10`, `rust-11`, `rust-13`.
- **Boundary:** live-cycle decisions and explicit external-action adapters.
- **Change:**
  - Port rule evaluation, project updates, reply and review drafts, reviewer
    selection, notification deduplication and source rosters.
  - Put fake notifiers first on PATH.
  - Prove draft-only defaults and approval-required sends.
- **Inputs:** `src/careerbrew/work/live.py`, `listen.py`, `review_ask.py`,
  `notify.py`; the live fixtures (owner map).
- **Outputs:** native live cycle and notifications.
- **Gate:** 100% of cycle, reviewer and notification cases agree; 0 unsolicited
  sends, duplicate notifications or content-bearing logs.
- **Component pre-registration (2026-11-25):** source revision
  `bbab16d06ea75ca8a39cc48e40fc9f3c509bfff9`; Python oracle
  `tests/oracles/rust14.py`. Twelve fixed synthetic cases cover source config
  and plugin rosters; live-cycle verdicts and locks; validated-unchanged,
  stale, malformed, future and reversed acquisition clocks; read-only direct
  health; reply/review drafts, project updates, goals and review sessions;
  link policy; reviewer selection and CODEOWNERS; notification priority,
  deduplication, reopening and batching; quiet hours, caps and snoozes;
  per-reference project holds and launch reservations; suggestion decisions;
  and the live/review/notify component CLI adapters. Exact typed results,
  semantic write maps and every storage/event write operation are compared.
  Only owned paths, generated record identities, sealed-record bytes and event
  wall-clock timestamps are normalized; source timestamps, health ages,
  notification states, privacy and write transitions remain strict. Thirty
  fresh Python runs establish baseline B. Verify requires all twelve cases
  with zero contract, privacy, side-effect, missing-write or duplicate-write
  violations. Measure requires 30 unique interleaved pairs in each cold and
  warm regime. CPU-time p95 and process-tree peak RSS must each be at most
  1.10 x the paired oracle in each regime; unmeasured noise floors use that
  ratio without relaxation. Any mismatch, unsolicited send, duplicate
  notification, content-bearing log or resource violation keeps Python.
  Later-owned inbox/results/standup/goal behavior stays behind typed component
  providers. This is component parity only: production dispatch, shipped
  crates and activation do not change.
- **Component implementation:** `crates/live` provides live source/cycle and
  direct-health adapters, draft-only listening, reviewer selection and
  notification state with fake notifier transport. `live-component` is
  synthetic-only. Production Python, native routes, shipped crates and
  activation are unchanged. Differential controls include re-review message
  dispatch, old-candidate freshness, quiet-project popup identity, exact
  goal-first schedule order and minute-precision work blocks, calendar
  state/error/capacity preservation, meeting-aware first-fit placement,
  stale-task rejection and new valid task inclusion, field-only generated
  identity normalization, and osascript transport. The full differential and
  paired-resource evidence is recorded only after every registered case and
  control passes.
- **Component acceptance (2026-11-25, macOS):** clean measured source
  `c877fc372629d76991f3835f8c6fe31c0495ce2b`; evidence-source hash
  `0ceea5604057951abce2721ef696bcbada01f094433b1b0c328f84de8017f4f8`;
  fixture hash `6a12e9c239da09f3797293717ff2035f41d745bd104655b58aa6b25e38ff5966`;
  release-candidate SHA-256
  `693640596d8e4ce57aa203da09ae5cdfa5099293c4f5ec70a1581037a0c644fa`.
  Baseline: 30 runs; CPU p50/p95 1,553,414/1,730,745 microseconds and tree-RSS
  p50/p95 65,323,008/65,863,680 bytes. Verify passes 12/12 with zero contract,
  privacy, side-effect, missing-write or duplicate-write violations, including
  partial research merge, wrapped CODEOWNERS, re-review dispatch, all suggestion
  kinds, exact calendar/task replanning and reserved-address controls.
  Measurement: 30 unique, uncached pairs in each regime. Cold CPU p95 was
  1,660,571 to 769,546 microseconds; warm was 1,678,979 to 796,834. Cold
  tree-RSS p95 was 65,814,528 to 18,055,168 bytes; warm was 65,880,064 to
  18,071,552. Each ratio is below 1.10. `decide` returned component-only
  `activate`; extra, missing, prior-phase duplicate, same-phase operation
  duplicate and direct event duplicate controls returned `keep-python`.
  Independent Kimi v7 review clarified its two initial notes as non-defects and
  returned `Ready to merge: Yes`. Production routes, shipped crates and
  activation remain unchanged; activation needs the complete native closure and
  rollback evidence.
- **Evidence-freshness refresh (2026-11-26, macOS):** clean source
  `fa43d00574271a1e804847f3f09020ef20666e5b` retains the same evidence-source,
  fixture and release-candidate hashes above. A fresh 30-run baseline has CPU
  p50/p95 1,527,761/1,607,853 microseconds and tree-RSS p50/p95
  65,290,240/65,503,232 bytes. Fresh 30-pair cold CPU p95 was 1,597,024 to
  784,704 microseconds; warm was 1,635,840 to 796,861. Cold tree-RSS p95 was
  65,830,912 to 18,038,784 bytes; warm was 65,912,832 to 18,055,168. Every ratio
  remains below 1.10. All five planted-write controls were regenerated at this
  exact source and each returned `keep-python` with `verify-check-failed` and
  `verify-violation`. The candidate and production boundary are unchanged.

### rust-15: Port canonical work-day records and questions

- **Depends on:** `rust-05`, `rust-09`, `rust-10`.
- **Boundary:** the versioned work-day schema and writer receipts.
- **Change:**
  - Port the landed canonical schema, the legacy-reader guards, stale-writer
    receipts, question records, and the private and public projections.
  - Keep raw running resource claims and the skipped-step reason rules.
  - Port the final landed canonical work-day schema, not a planning snapshot.
- **Inputs:** `src/careerbrew/workday/status.py`, `questions.py`, `steps.py`,
  `stages.py`, `__init__.py`, and any schema or dashboard module that lands
  before the step.
- **Outputs:** native work-day record types used by `rust-16` and `rust-17`.
- **Gate:** 100% of schema, legacy and question cases agree; 0 silent legacy
  overwrites, exposed business fields or dropped resource claims.

### rust-16: Port work-day execution, scheduling and watch loops

- **Depends on:** `rust-07`, `rust-14`, `rust-15`.
- **Boundary:** the work-day process lifecycle, step proofs and clock ownership.
- **Change:**
  - Port refresh before plan, sign-in blocking, readiness dispatch, proof
    checks, timeout and retry, the one-loop lock, cycle and watch, catch-up and
    the movable graceful wrap-up.
  - Keep the start-up and wrap-up behaviour of every change that has landed.
- **Inputs:** `src/careerbrew/workday/start.py`, `plan.py`, `gate.py`,
  `watch.py`, `end.py`, `timing.py`, `cron_check.py`.
- **Outputs:** the native work-day executor.
- **Gate:** 100% of execution and state-transition cases agree; 0 duplicate
  dispatches, false success claims or lost wrap-up requests in 1,000 interleavings.
  Activation needs the `rust-16` + `rust-17` + `rust-24` closure.

### rust-17: Port daily planning, results and PM learning

- **Depends on:** `rust-09`, `rust-13`, `rust-15`, `rust-16`.
- **Boundary:** day records, sent receipts and PM source-day transactions.
- **Change:**
  - Port results, inbox, plans, logs, handoff, done-elsewhere, PM drafts and
    lessons, watch cycles, and sent-Tomorrow promises.
  - Keep exact sent-message receipts, manual authority, edited DM
    reconciliation, the two-distinct-day lesson threshold, and no-op
    idempotence from the final landed fixes.
  - Keep the sent-day lock order: sent-day lock, then history or project lock.
- **Inputs:** `src/careerbrew/day/` (including `pm_promises.py`); the PM
  promise fixtures (owner map).
- **Outputs:** native day records and PM learning.
- **Gate:** 100% of day and PM vectors agree; 0 wrong-recipient lessons,
  duplicate promises or history, or idle writes.

### rust-18: Port evidence, promotion, standup and work profiles

- **Depends on:** `rust-09`, `rust-11`, `rust-17`.
- **Boundary:** career evidence and local report content.
- **Change:**
  - Port evidence mapping, ladder grading, promotion documents, the standup
    proof gates and talking-point bullets.
  - Keep the real source state and the business-data rules.
  - Keep the current HTML and CSS look and the browser code.
- **Inputs:** `src/careerbrew/work/evidence.py`, `ladder_grade.py`, `promo.py`,
  `standup.py`, `linkedin.py`; the standup fixtures (owner map).
- **Outputs:** native evidence and report builders.
- **Gate:** 100% of evidence, standup and report contracts agree; 0 unsupported
  claims, missing proof or unsafe HTML.

### rust-19: Port job search, interview prep and public research

- **Depends on:** `rust-08`, `rust-09`, `rust-12`, `rust-20`.
- **Boundary:** personal-role research and ranking.
- **Change:**
  - Port job platform adapters, criteria, wants, strategy, LinkedIn drafts,
    interview loops and prep gates.
  - `rust-20` first extracts the native gap-queue provider that `jobs.strategy`
    reads from `tutor.engine`. The rest of the teacher stays for `rust-21`.
  - Keep the corporate-role blocks, source terms and public-only research.
- **Inputs:** `src/careerbrew/jobs/`, `interview.py`, `prep.py`, `research.py`;
  the job-board fixtures (owner map).
- **Outputs:** native job search and prep.
- **Gate:** 100% of scoring, strategy and prep vectors agree; 0 corporate
  job-hunt access; 0 unapproved application or send actions.

### rust-20: Port flashcards, FSRS and Notion import

- **Depends on:** `rust-01`, `rust-05`, `rust-08`.
- **Boundary:** scheduler math and card and import identity.
- **Change:**
  - Port scheduling with a maintained FSRS library checked against the current
    vectors, card grading, public deck checks and Notion import.
  - Also port the shared `tutor.engine` gap-queue and provider primitives and
    the mistake-record helpers that `jobs.strategy` and exercise submission
    need, before their consumers. Leave the teacher orchestration to `rust-21`.
  - Keep private and public cards apart, learning timestamps, and import idempotence.
- **Inputs:** `src/careerbrew/tutor/cards.py`, `public.py`, `notion_import.py`,
  `tutor/engine.py` (shared providers), `tutor/mistakes.py` (record helpers);
  the Notion bundle and public card fixtures (owner map).
- **Outputs:** native cards, FSRS and the shared providers.
- **Gate:** 100% of fixed-clock FSRS decisions agree within pre-registered
  numeric tolerances; 0 private card leaks or duplicate imports.
- **Integration seam:** the shared harness registers the release
  `tutor-component`, its effective Python/Rust sources and fixtures, and
  unmeasured macOS/Linux noise-floor entries. The differential compares all
  eight domain cases and 18 dependency vectors. It preserves every due-time
  string exactly and allows only actual float leaves absolute `0.00001` and
  relative `0.000001` tolerance. Typed `Resources` discovery reads base and
  installed-plugin knowledge/rubrics through the existing plugin provider.
  Semantic phase writes, exact file modes, event rows, and
  extra/missing/duplicate/event-duplicate controls are observed. Sealed records are opened with their own synthetic
  workspace keys and compared as typed values with the same float tolerance;
  malformed JSONL is refused. The measured `--all` path must return the same
  complete eight-case report before timing starts. Production dispatch,
  shipped crates and activation remain unchanged.
- **Review correction (2026-11-26):** independent Kimi review at `5aa0aa2d`
  rejected the earlier component-only `activate` evidence. It found unbound
  vault/profile-example inputs, an unvalidated measured `--all` path, sealed
  and malformed-JSONL blind spots, generated-card ID collisions, fractional
  elapsed-day drift, process-versus-configured timezone drift, and malformed
  persisted-state acceptance. Follow-up controls also retain exact file modes,
  refuse missing or malformed step state, keep the review instant's UTC offset
  at local day end across a daylight-saving transition, and normalize pinned
  clocks to UTC without a wall-clock fallback. The DST control injects its zone
  instead of inheriting the host zone. Focused controls
  now cover and reject each defect. The earlier resource record is superseded
  by the clean record below.
- **Measured evidence (2026-11-26):** clean source
  `cd9f01998ceac6498c4269bacccca9ea0e881a4a` on macOS/aarch64 with Rust
  1.100.0 and Python 3.12.16. Baseline/verify/measure share evidence-source
  hash `0ad5a1486172751c1ccd1efc8a0246c94f45803813ba65daa0998f536a9d36a3`
  and fixture hash `e8957893448fb8c1396ab6deee329851658ebbeb2bddbc99195ffcb7f411a0ce`.
  Release-candidate SHA-256 is
  `878f0eca86820c2c2646d85185a7811b1d8e948526b69fe9017a6668c93d176e`.
  Thirty baseline runs gave CPU p50/p95 371,768/398,279 microseconds and
  tree-RSS p50/p95 45,121,536/45,531,136 bytes. Verify passed 8/8 domain
  cases, including all 18 FSRS vectors and exactly 14 full-precision float
  leaves, with exact due times and zero observed violations. Each regime has
  30 unique, uncached pairs. Cold CPU/RSS p95 ratios are 0.4540/0.3165; warm
  ratios are 0.4459/0.3168, all below 1.10. The decide paired-maximum RSS
  ratios are 0.3161/0.3165. `decide` returned component-only `activate`; extra,
  missing, duplicate and event-duplicate controls each returned `keep-python`.
  Local records: `.metrics/baseline-rust-20-cd9.json`,
  `.metrics/verify-rust-20-cd9.json`, `.metrics/measure-rust-20-cd9.json`,
  and `.metrics/decide-rust-20-cd9.json`.
- **Evidence limits:** HTTP, DATA and generic role contracts are explicitly
  scoped out of this component record; the domain's corporate-role refusal is
  covered. Python denial is environment-based. Cold differs only by missing
  warm-up on the Rust side, not by a cold OS page cache. Noise floors are
  unmeasured, so the 1.10 ratio applies. These are macOS component-parity
  results, not production activation; both hosted OS gates and public
  readiness remain required.
- **DST-isolated acceptance (2026-11-26, macOS):** clean source
  `563bf7b83a95b1de81142fdb83031fade47fece9`; evidence-source hash
  `e7e4f5745cf1af805d54cef97fe89ae4af4cf32181225dafb3334053b53b3a14`;
  fixture hash `e8957893448fb8c1396ab6deee329851658ebbeb2bddbc99195ffcb7f411a0ce`;
  release-candidate SHA-256
  `e5a244994a63075892670be67e432a64571474409ccdb4cadda5593dc5e38361`.
  Baseline: 30 runs; CPU p50/p95 533,702/593,652 microseconds and tree-RSS
  p50/p95 45,727,744/45,989,888 bytes. Verify passes 8/8 with zero violations.
  Cold CPU p95 was 403,880 to 202,733 microseconds; warm was 571,405 to
  255,998. Cold tree-RSS p95 was 45,711,360 to 14,516,224 bytes; warm was
  46,104,576 to 15,204,352. Each regime has 30 unique uncached pairs and every
  ratio is below 1.10. `decide` returned component-only `activate`; all four
  same-head planted controls returned `keep-python`. Production routes, shipped
  crates and activation remain unchanged.

### rust-21: Port teacher, goals, practice and mistake tracking

- **Depends on:** `rust-19`, `rust-20`, `rust-22`.
- **Boundary:** learner state and deterministic practice decisions.
- **Change:**
  - Port lesson selection, goals, practice progress, mistakes and improvement rules.
  - Keep local-day counting, the repeat-mistake metrics, and decisions that do
    not depend on a model.
- **Inputs:** `src/careerbrew/teacher.py`, `goals.py`, `tutor/engine.py`, `mistakes.py`.
- **Outputs:** the native teacher.
- **Gate:** 100% of practice, goal and mistake vectors agree; 0 lost grades;
  0 UTC versus local-day count errors. Activation needs the `rust-19` +
  `rust-20` + `rust-21` + `rust-22` closure.

### rust-22: Port exercise execution and complexity checks

- **Depends on:** `rust-04`, `rust-05`, `rust-08`, `rust-20`.
- **Boundary:** the subprocess lifecycle of untrusted learner code.
- **Change:**
  - Port sandbox orchestration, language runners, CPU and resource limits,
    repeated scaling measurements and complexity decisions.
  - Port the canonical reference solutions, scaling generators and verification
    fixtures to Rust, so the normal exercise gate needs no Python.
  - Keep every functional assertion, comparison mode and wrong-complexity control.
  - Python examples stay only as optional teaching data. An explicit Python
    practice session may start its optional interpreter. The native app and
    the normal gate run without Python.
  - Keep compilation apart from the timed algorithm samples. Keep resource
    limits for both.
  - Keep the cubic negative controls and fresh-execution timing evidence.
  - Coordinate the `scripts/check-card-code.py` checker move with `rust-27`.
- **Inputs:** `src/careerbrew/tutor/exercises.py`; `knowledge/exercises/`
  (the reference, test and scale-generator fields); `scripts/check-card-code.py`.
- **Outputs:** the native exercise runner and the native reference solutions.
- **Gate:** 0 correct-reference rejections in 10 fresh 8-worker batches; 100% of
  wrong-class negative controls rejected; 0 sandbox escapes or leaked private
  paths. The native exercise and card checks keep all original functional and
  invalid-code controls with Python denied.
- **Component pre-registration (2026-11-27):** the first rust-22 PR ports
  only the runner core: the sandboxed child lifecycle (fresh empty `cb-run-*`
  folder, cleared environment with `PATH` and `PYTHONHASHSEED=0`, CPU and
  address-space limits set before `exec`, a wall deadline that kills and reaps
  the child, last-stderr-line-only errors), the tests-then-scaling run, the
  CPU-time scaling protocol, the slope fit (CPython 3.12 compensated `sum`),
  the complexity decision, the four comparison modes, grading and reference
  validation. Crate `crates/exercises/`, candidate `exercises-component`.
  Both sides start one synthetic language runner, `exercise-fixture-runner`,
  whose solutions spend calibrated process CPU time (linear, quadratic or
  cubic in `n`), so the decision is the same in debug, release and under load.
  The oracle `tests/oracles/rust22.py` drives the real `exercises._child`,
  `run`, `validate`, `grade` and the harness `same` over
  `tests/fixtures/rust22/cases.json`. Live timings compare at decision level
  only (`<timed>`); pure decision vectors compare exact float reprs.
  Hypothesis: 6/6 cases byte-identical, including the cubic wrong-class
  negative control (rejected as `complexity_rejected`), the CPU-limit control
  (`signal_exit`, signal 24), the wall-limit control (`wall_timeout`) and a
  private stderr canary that never leaves. Success: 6/6 agree and both
  planted controls (`extra` write, `accept-wrong-class`) make verify fail.
  Pivot: any disagreement or an accepted planted control keeps Python.
  Readability (ruff), the CLI handlers, `check_all`, the learner Python
  harness itself and every reference solution and generator stay in Python
  for later rust-22 PRs.
- **State (2026-11-27):** component-only. Verify passes 6/6 cases locally
  (macOS) with zero violations; `extra` and `accept-wrong-class` controls
  each fail verify and `decide` returns `keep-python`. Baseline and measure:
  pending the batched hosted run (owner decision 2026-11-27). Production
  dispatch, native routes and shipped crates are unchanged.

### rust-23: Port local voice, model routes and judge adapters

- **Depends on:** `rust-04`, `rust-05`, `rust-21`.
- **Boundary:** external engine processes and the model request boundary.
- **Change:**
  - Port the `whisper-cli` and `ffmpeg` process adapters, the text fallback,
    local-first model routing, cache keys, bounded requests, and judge and
    owner-label agreement.
  - Do not change the speech engine or model. Never make up owner labels.
  - Keep audio and transcripts temporary.
- **Inputs:** `src/careerbrew/tutor/voice.py`, `models.py`, `judge.py`,
  `config/models.yaml`; the voice fixtures (owner map).
- **Outputs:** native voice and model adapters.
- **Gate:** 100% of synthetic process and request contracts agree; 0 retained
  audio or transcripts; 0 route or policy bypasses. The owner-label agreement
  bar stays as it is.

### rust-24: Port reports, metrics, checkups and remaining domain helpers

- **Depends on:** `rust-06`, `rust-17`, `rust-18`, `rust-19`, `rust-23`.
- **Boundary:** content-free observability and local presentation builders.
- **Change:**
  - Port report builders and their lifecycle, charts, metrics exports, review
    outcomes, checkups, wizards, checklist state, and the `changes.check` and
    pivot orchestration.
  - Reuse the native event and hash primitives. Never write them twice.
  - Keep public allowlists, local business detail, stable cache keys and the
    current UI templates.
  - Audit the inventory for any domain module that is still Python.
- **Inputs:** `src/careerbrew/{report,runs,charts,metrics_share,repo_health,changes,events,telemetry,field,time_saved,signals,review,checkup,checkup_runs,wizard,checklist}.py`.
- **Outputs:** native report and metric builders; the full `changes.check`.
- **Gate:** 100% of report, metric and export contracts agree; 0 new public
  fields, content-bearing telemetry or missing long-run pages.

### rust-25: Replace the HTTP server behind the unchanged UI

- **Depends on:** `rust-03`, `rust-04`, `rust-14`, `rust-16`, `rust-17`,
  `rust-18`, `rust-19`, `rust-20`, `rust-21`, `rust-22`, `rust-23`, `rust-24`.
- **Boundary:** localhost HTTP, auth, cookies, cache and the change feed.
- **Change:**
  - Port routes, error envelopes, request limits, action dispatch,
    authentication, cookies and origins, static serving, cache and change
    versions, and concurrent request behaviour.
  - Keep the React views, the TypeScript UI tooling and the browser tests.
  - Replace backend-specific launch and probe commands inside the kept browser
    tests (for example `web/e2e/signin.spec.ts`) with native CLI or test
    adapters. Their generated shell stubs and seed helpers port in `rust-27`.
  - Run the real built UI against the native server.
- **Inputs:** `src/careerbrew/server.py`; the `web/src/api.ts` contract;
  `web/e2e/`; the server-facing web files in the owner map.
- **Outputs:** the native HTTP server.
- **Gate:** 100% of HTTP and action contracts and all existing browser stories
  pass; 0 auth or origin bypasses, burst resets or idle refetch churn; 0
  intended visual changes.

### rust-26: Complete native CLI coverage and default entry point

- **Depends on:** `rust-25` and every step that ports a CLI handler: `rust-02`
  to `rust-10` and `rust-12` to `rust-24` (see the
  [owner map](#files-per-step)).
- **Boundary:** argument parsing, the dispatcher, the action registry wiring
  and process start-up. **Not** the command handlers: each domain step ports
  and switches its own (see
  [CLI handlers](#cli-handlers-clipy-is-split-across-the-steps)).
- **Change:**
  - Replace the Python argument parser and the generated Python dispatch with
    the native ones. By now every handler is native and routed.
  - Run the generated action inventory, the argument-free UI path, the
    confirmation and terminal-only rules, and each CLI action story against the
    native binaries.
  - Switch the default launcher only after full parity. This is the final
    switch of the entry point, not of the handlers.
  - Update the command lines that agents and the UI show (`commands/`, the
    skills, the start text in `web/src/act.ts`).
- **Inputs:** `src/careerbrew/cli.py` (parser and dispatch only), `actions.py`,
  `__main__.py`, `commands/`, `Agentfile.yaml`.
- **Outputs:** the native default entry point.
- **Gate:** 100% of CLI actions have native handlers and the required story
  coverage; 0 commands still routed to Python; 0 Python subprocesses outside an
  explicitly selected Python exercise.

### rust-27: Port developer tools, test harnesses, installers and release checks

- **Depends on:** `rust-00` to start. Each sub-step names what it waits for.
  The step ends after `rust-26`.
- **Boundary:** repository operations, checks and distribution.
- **Rules for every sub-step:**
  - Domain steps port the assertions for their own capability (see
    [Test files](#test-files-the-file-and-its-assertions-have-different-owners)).
    This step owns the shared harness and the assertions no domain step owns.
  - Keep the browser tests in TypeScript.
  - Inline shell in docs and hooks becomes a native command. Only an
    explicitly inventoried minimal OS bootstrap may stay.
  - Keep the existing gate rules: input-key caching, leak checks, real exit
    codes, the fast and full split, the eval and owner-label rules, the
    content-free run pages and the safe batch-merge guards.
- **Sub-steps** (file counts from the [owner map](#files-per-step)):

  | Sub-step | Files | Waits for | Change | Gate |
  | --- | --- | --- | --- | --- |
  | `rust-27a` shared test harness | 283 | `rust-00`; its last part waits for `rust-26` | Port `conftest.py`, the synthetic fakes, fixture servers, and the claim audit oracle to native test support. Retire the Python observer and capture drivers with the Python oracle. Port the 57 unit and safety files that no domain step owns. | 0 lost negative controls; every remaining unit assertion runs natively |
  | `rust-27b` developer scripts, hooks and installers | 38 | `rust-00`; installers wait for `rust-26` | Move setup, doctor, the gate, checkup, batch gate, metrics and proof, leak checks and release packaging to `xtask`. Port the embedded Node and esbuild backend checker in `check-card-code.py`; the kept UI TypeScript is not an exception for this maintenance code. Check the syntax of Python teaching examples with a maintained native parser, checked against the current `compile()` oracle, with the invalid-code controls; never skip them silently. | clean install, doctor, fast and full gates pass without Python |
  | `rust-27c` e2e stories | 114 | the story's domain step; `lib.sh` and the 11 stories no domain step owns wait for `rust-26` | Port the story runner (`lib.sh`, `browser_story`, the seed helpers) to native test support. Each domain step ports its own stories on that runner. | every story id still has a native e2e test per case |
  | `rust-27d` evals | 39 | `rust-23` (judge adapters) | Port `evals/run.py` and `run.sh` orchestration. Keep the cases, the rubric and the owner labels as data. | eval agreement and owner-label checks match the Python run |
  | `rust-27e` CI workflows | 3 | `rust-27a` to `rust-27d` | Switch the hosted jobs to the native commands. Keep the required check names `gate (ubuntu-24.04)`, `gate (macos-15)` and `public-readiness`. | the three hosted checks are green without Python |

- **Inputs:** `scripts/`, `hooks/`, `evals/run.py`, `evals/run.sh`,
  `tests/unit/`, `tests/e2e/`, `lefthook.yml`, `INSTALL.md`, the hosted CI
  workflows.
- **Outputs:** native developer tools and test harness.
- **Gate:** 100% of old assertions map to native tests or a reviewed stronger
  replacement; 0 lost negative controls; clean install, doctor, fast, full and
  eval gates pass without Python.

### rust-28: Retire the old implementation and verify fresh-machine releases

- **Depends on:** `rust-26`, `rust-27`.
- **Boundary:** supported platforms, release artifacts and rollback.
- **Change:**
  - Delete the retired Python and shell code, only after native parity.
  - Remove the Python runtime and developer dependencies, old launchers and
    import paths.
  - Package native binaries for the supported targets, with checksums and the
    dependency and licence inventory.
  - Test clean and existing-data installs, start-up, watch and shut-down, the
    UI, backup restore and safe downgrade.
  - Keep an immutable last-Python release for rollback. It is not a hidden
    runtime fallback.
- **Inputs:** the Cargo workspace, package and install metadata, all remaining
  inventory rows, `README.md`, `INSTALL.md`, `AGENTS.md`, `pyproject.toml`,
  `uv.lock`, `.python-version`.
- **Outputs:** the native release and the last-Python rollback release.
- **Gate:** 0 unassigned first-party executable files; 0 Python app or
  developer-tool requirements; 100% of supported install, upgrade and rollback
  cases and the whole-repo gates pass. The final manifest shows no application
  or developer-tool Python requirement and no unassigned executable code.

## Rollout, rollback, security and regression coverage

**Rollout:**

- Land `rust-00` first, then the dependency proof (`rust-01`) and the read-only
  pilot (`rust-03`).
- Turn on each native capability only after its exact revision passes the
  registered differential suite at its full activation closure.
- During rollout prefer internal or test selection. Keep one documented
  default handler per released capability.

**Rollback:**

- Roll back by selecting the last verified handler or release. The data stays
  readable.
- Never erase records, weaken approval gates, downgrade encrypted formats, or
  force-push to recover from a failed experiment.
- `rust-28` keeps an immutable last-Python release for rollback.

**Security:**

- Policy runs before any native mutation or network call (`rust-04`).
- Synthetic fixtures only. No live sends. No private data in tests, logs or
  receipts.
- Keys and tokens stay in local stores. Never in fixtures or subprocess logs.
- Fernet comes from a maintained crate. No custom cryptography.
- Never start a Python fallback after a Rust mutation may have happened.

**Regression coverage:**

- Each migrated step brings unit tests, CLI story cases, invalid and malformed
  inputs, privacy canaries, role restrictions, and race and crash tests where
  it writes.
- Keep action parity, the HTTP contracts, the browser controls, every visual
  baseline and the comfort budgets as regression coverage.
- No UI redesign is part of this migration.
- An agent or prompt change needs evals or a documented artifact-test exemption.
- A count of tests or lines is not parity.
- Port the shell and Python gate logic before retiring it. Rust `fmt`,
  `clippy`, test and dependency checks add to those rules; they never replace them.
- Never turn off a gate to get a step through.

**Open decisions (preparation work, never a reason to narrow the final state):**

- `rust-01`: crate compatibility and licences, with evidence.
- `rust-00`: noise floors, and the allowed normalization of volatile fields for
  every contract.
- `rust-28`: the real supported OS and architecture matrix, code-signing and
  install needs, and clean-machine setup.
- Refresh this page and the inventory as fixes land.

## Source-map refresh rule

The pinned source maps must follow the Python code. When you change code,
refresh the map in the **same** pull request:

| When this changes | Do this | Then run |
| --- | --- | --- |
| a Python source file that the activation tables name | refresh the pinned line cells (definition and call lines, and duplicated expected-line data) in the three [activation tables](#the-activation-manifest-today) | `cargo test -p careerbrew-xtask --test activation_manifest` |
| a tracked source file is added (any executable mode, or a Python, shell, JavaScript, TypeScript or Rust extension) | add its row to a fragment in [`plans/rust-inventory.d/`](plans/rust-inventory.d/README.md) (`<name>.csv`, one per step, same columns, no header; the main [code inventory](plans/2026-11-01-rust-code-inventory.csv) stays as is, so parallel step PRs do not conflict): path, disposition (`port`, `keep-ui` or `keep-native`), task, reason; the task is empty for `keep-ui`, `rust-00` for `keep-native`, or the two-digit migration step for `port`; then add it to the [owner map](#every-module-has-an-owner-step) | `cargo xtask migration inventory --check` |
| a reviewed interpreter call site in source moves | update its row in the [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) | `cargo xtask migration inventory --embedded-check` |
| an embedded code site (an interpreter with `-c` or `-e` in shell or argument-array form, an interpreter heredoc, an inline `<script>`) or a generated entry point (a pyproject script or entry point, a `package.json` script or bin, an Agentfile command, skill or hook, a git hook, a lefthook or workflow `run`) is added, changed or removed | add, change or remove its row in the [embedded sites](plans/2026-11-17-rust-embedded-sites.csv): kind, path, key (the trimmed line or the name; a repeated identical line in one file gets ` #2`, ` #3`), owner task. `--sites-list` prints every discovered site as a manifest row, with its current task or an empty task for a new site | `cargo xtask migration inventory --check` |
| `src/careerbrew/vault.py` | update the pinned `SOURCE_SHA` in `tests/unit/test_rust_vault_interop.py` and `tests/unit/test_rust_vault_base64_parity.py` | `uv run pytest -q tests/unit/test_rust_vault_interop.py tests/unit/test_rust_vault_base64_parity.py` |

Rules of the refresh:

- Keep every existing assignment and the UI exception. One missing or
  reassigned existing path is a failure: stop and fix it.
- Keep **zero** unlisted direct code paths.
- The inventory check reads the staged Git index. Stage a new source path and
  its inventory row together.
- Refresh only the recorded cells. Keep the partial graph scope, the owners,
  the edges and the checker logic.
- The direct-path scan covers direct paths. The embedded-site scan covers
  inline code on one line of an executed file (under `scripts/`, `hooks/`,
  `commands/`, `skills/`, `src/`, `tests/e2e/`, `.github/`, `evals/`,
  `web/src/`, `web/e2e/`, `web/visual/`, and `lefthook.yml`) and the
  generated entry points. It finds an interpreter word (`sh`, `bash`,
  `python3`, `node` and others in `xtask/src/sites.rs`) with `-c`, `-e` or a
  heredoc, a quoted `"-c"` or `"-e"` in an argument array (not after `git` or
  `grep`), and fixed markers. A site key is its marker line, not the heredoc
  body: a body edit keeps its owner. A non-UTF-8 text file in a scanned path,
  an unreadable `package.json` or an Agentfile list it cannot read fails the
  check; a file with a NUL byte is binary and skipped. Prose in `docs/` is out
  of scope. Process
  observation is task `rust-00-inventory-process-observation`.

Anchor: Basili, Caldiera and Rombach 1994 (Goal Question Metric).

## Current state

Read from the code on 2026-11-15, at `main` `ecbbb7fc`. Check it again with
the commands in each row before you rely on it.

**Summary:** the Rust code is measurement preparation and two disconnected
component experiments. No CLI command, HTTP route or watcher calls Rust. The
app runs on Python. `rust-00`, `rust-01` and `rust-02` are done (2026-11-18). For
`rust-01`, six dependency vector sets (`calendar`, `code-parse`, `fernet`,
`fsrs`, `json-yaml`, `runtime`) agree with the Python oracle on every vector,
and each set's planted wrong vector fails. For `rust-02`, see
[rust-02 close-out](#rust-02-close-out). The next task is `rust-03`.

### Crates (`ls crates xtask experiments`, `Cargo.toml`)

| Path | Package | What it is | Production use |
| --- | --- | --- | --- |
| `xtask/` | `careerbrew-xtask` | the migration developer commands and their tests | none |
| `crates/vault/` | `careerbrew-vault` | the `CBV1` codec, key/atomic/lock providers, rust-05f storage readers/writers, receipt IO and adapter-driven `data edit`/`secret set`, rust-05g settings/general-event IO, rust-05h outline/digest/cache helpers, named-user key paths and component `data show`, and rust-05i bulk vault/status operations, boundary-fault controls and dynamic denied-host controls; synthetic `vault-store` transport only | none: component parity only |
| `crates/setup/` | `careerbrew-setup` | the rust-07 plugin, machine, OAuth reference, MCP repair, self-update and workflow component, with all eleven owned handler adapters; synthetic `setup-component` transport only | none: component parity only |
| `crates/sources/` | `careerbrew-sources` | the rust-08 bounded recorded HTTP, source cursor, calendar recurrence, mail metadata and `connect` component; synthetic `sources-component` transport only | none: component parity only |
| `crates/projects/` | `careerbrew-projects` | the rust-09 project lifecycle, readiness, priority, promise, focus, team and four-handler CLI component; synthetic `projects-component` transport only | none: component parity only |
| `crates/sessions/` | `careerbrew-sessions` | the rust-10 session, claim, agent activity, prior-selection, reply-draft and six-handler CLI component; synthetic `sessions-component` transport only | none: component parity only |
| `crates/memory/` | `careerbrew-memory` | the rust-11 source identity, project import, memory sync and component CLI adapters; synthetic `memory-component` transport only | none: component parity only |
| `crates/learn/` | `careerbrew-learn` | the rust-12 learning, project-scoped FSRS, replay, tour, explainer, PRD and CLI component; synthetic `learn-component` transport only | none: component parity only |
| `crates/feed/` | `careerbrew-feed` | the rust-13 recorded feed readers, original/analysis transactions, raw replies, Claude/Codex transcript normalization, open asks, cycle and CLI component; synthetic `feed-component` transport only | none: component parity only |
| `crates/live/` | `careerbrew-live` | the rust-14 live-cycle, acquisition/direct health, listening, reviewer and notification component; synthetic `live-component` transport only | none: component parity only |
| `crates/tutor/` | `careerbrew-tutor` | the rust-20 FSRS, card, public-deck, Notion, shared-provider, mistake and installed-plugin resource component; synthetic `tutor-component` transport and shared migration registration only | none: component parity only |
| `crates/purevec/` | `careerbrew-purevec` | the rust-02 pure value vector candidate: sets `clock` (`chrono` =0.4.45 without the `clock` feature, `chrono-tz` =0.10.4), `hash` (`changes._h` over `sha2 =0.10.9`), `paths` (std only) and `settings` (config validators, `DEFAULTS`, pure precedence); `--all` lists them, for `migration verify`, `baseline` and `measure --step rust-02` | none: component parity only (decide `activate`, 2026-11-18) |
| `crates/policy/` | `careerbrew-policy` | the rust-04d policy core: the `privacy.py` canary engine (`term_id`, `normalize`, `term_pattern`, `find_leaks`, `leak_spans`, `_match_kind`), the `sensitive_terms` filter rules and the allowlists, on Python 3.12 Unicode 15.0.0 tables (`gen_tables.py`). No IO: gate step `rust-policy-io` (`scripts/policy-no-io.py`). Binary `policy-vectors` answers `unicode-version` and the sets `canary-core`, `canary-match`, `leak-spans`, `assert-redact`, `allowlists`, `private-path`, `assert-no-echo`; rust-04e adds `assert_clean`, `redact` and `_names_private_path` (`outputs.rs`), `scrub_public` on a `re`-compatible matcher (`scrub.rs`, `pyre.rs`), the approval decisions (`approvals.rs`) and the access decisions (`access.rs`), with the sets `public-scrub`, `approvals`, `receipt-validate` and `access`; `--all <file>` runs every set for `measure --step rust-04` (`rust-04f`) | none: component parity only |
| `crates/cbcli/` | `careerbrew-cli` | the rust-03 native CLI pilot, binary `careerbrew-native`: `actions [name] [--json]` and static `-h/--help` from the compiled-in registry; exit 64 for anything else | active since 2026-11-19: `cli.main` routes `actions` native by default (`config/native-routes.txt`); every other action defaults to Python. Rollback: `CAREERBREW_ROUTE_ACTIONS=python` or `CAREERBREW_NATIVE=off`. `setup.sh` rebuilds it after each pull (`scripts/rust-shipped-crates.txt`) |
| `crates/depvec/` | `careerbrew-depvec` | the rust-01 dependency vector candidate: maps each synthetic vector of a set (`calendar`, `code-parse`, `fernet`, `fsrs`, `json-yaml` and `runtime` today) to an output with maintained crates, for `migration verify --step rust-01` | none: dependency evidence only |
| `experiments/hash/` | `careerbrew-hash-experiment` | the pure `changes._h` hash for a supported synthetic JSON subset. It refuses values outside that subset. | none: component parity only |

- No crate ships: `scripts/rust-shipped-crates.txt` is empty. A crate whose
  "Production use" stops being "none" must be on that list, and setup must
  rebuild it after a pull (`tests/unit/test_rust_shipped_crates.py`).
- The workspace pins Rust `1.100.0` (`rust-toolchain.toml`, minimal profile with
  `clippy` and `rustfmt`).
- `.cargo/config.toml` defines `cargo xtask` as
  `run --locked --offline --package careerbrew-xtask --`.
- Pinned crates: `serde`, `serde_json`, `sha2`, `tempfile`, `libc`,
  `unicode-normalization` (xtask);
  `fernet` 0.2.2, `base64` (vault). Licences are in
  [public-readiness-licences.md](proof/public-readiness-licences.md).

### xtask commands (the dispatch in `xtask/src/main.rs`)

The tool has no `--help`. A wrong argument prints the usage and exits 2.

| Command | What it does |
| --- | --- |
| `cargo xtask leak-check [--staged] [--repo <dir>] --terms-file <file>` | rust-27b slice 1 (`xtask/src/leak_check.rs`): the native port of the repo scan behind `scripts/leak-check.sh` (`privacy.scan_repo` and the `cmd_privacy_scan` output). Same inputs (staged blobs from the cached raw diff, then tracked working files unless `--staged`), the same `private/` path rule, term matching (NFKC, zero-width removal, separators and leetspeak for terms of 5+ characters), the same `LEAK` and summary lines with term ids only, and exit 0 clean or 1 on a finding; 2 is a usage or terms-file error. Differences, so the wrapper still runs Python: terms come from a plaintext file, not the encrypted data root, profile and plugins (rust-04, rust-05, rust-07); a staged visual snapshot PNG is `scan-incomplete` (its validated pixel scan is a later slice); no `--file` or `--show`. Tests: `xtask/tests/leak_check.rs`. |
| `cargo xtask input-key [--repo <dir>] [--] [pathspec...]` | rust-27 slice 2 (`xtask/src/input_key.rs`): the native port of `scripts/input-key.sh`, the gate step cache key. The same steps: refuse a folder outside a checkout, drop the variables `git rev-parse --local-env-vars` names, copy the index with its mtime, record modified and untracked non-ignored files with `update-index --info-only`, then print the first 16 hex characters of a SHA-1 over `git ls-files -s` (with pathspecs: the pathspecs one per line first, then the matching files). Exit 0 with the key, 1 with no output on any git error, 2 on usage. `--repo` replaces the script's own checkout. Gaps, so the wrapper still runs the script: the script's last pipeline can print a key and exit nonzero when the final `ls-files` fails, while the native command prints nothing; a pathspec starting with `--repo` or `--` must follow a `--`; a leading `--` is a separator natively, while the script hashes it as a pathspec line and passes it to git (`scripts/check.sh` never passes one; pinned by `a_leading_double_dash_is_a_separator_only_natively`). The default repo is the build checkout (`CARGO_MANIFEST_DIR/..`), so a copied binary needs `--repo`. Tests: `xtask/tests/input_key.rs`. |
| `cargo xtask check-docs [--repo <dir>]` | rust-27b slice 3 (`xtask/src/check_docs.rs`): the native port of `scripts/check-docs.sh`, the docs drift check. The same three checks and the same lines: each `docs/*.md` but `README.md` has a `(<name>)` link in `docs/README.md`; each `tests/e2e/us-*` path that `docs/user-stories.md` names exists; each regular file `tests/e2e/us-*` is named there and names its own story id. Exit 0 with `check-docs: docs match the code`, 1 on drift, 2 on usage. It reads files only. The script's `grep` patterns use the platform's POSIX basic regular expression engine, including `[`, `*`, `^`, `$` and backslash; names sort bytewise, like the script under `LC_ALL=C`. Missing-index and malformed-BRE grep diagnostics go to stderr. Tests compare exact stdout, stderr and exit status in synthetic repos, including unclosed `[` and a trailing backslash. The script stays the gate entry point in this slice; no production activation changes. The default repo is the build checkout, so a copied binary needs `--repo`. Tests: `xtask/tests/check_docs.rs`. |
| `cargo xtask browser-test-receipts <discovery.json> <execution.json>` | rust-27b slice 4 (`xtask/src/browser_test_receipts.rs`): the native component port of `scripts/browser-test-receipts.py`. It requires identical nonempty unique `(spec id, projectId)` sets (10,000 identities and 256 Python code points maximum), expected passed execution with exactly one passed result at retry 0, and the Python script's exact stdout, stderr and exit 0/1 behavior. A linear lexical adapter leaves valid surrogate pairs and literal non-BMP text to maintained serde_json, reversibly tags lone surrogates and sentinel literals, and rejects container nesting above 900 before parsing. Serde's `unbounded_depth` parser and recursive `Value` drop run on a 64 MiB worker stack; only flat identities cross the join. Identities compare and sort as Python code-point sequences. `CAREERBREW_BROWSER_RECEIPT` writes sorted version-1 identities with CPython `ensure_ascii` escaping (including lone surrogates and surrogate pairs) through a mode-0600 temporary file and atomic no-clobber hard link; existing files and symlinks stay untouched and temporary files are removed. Differential and filesystem controls include accepted depths 128/200/800 on both engines, native fail-closed refusal at 2,000 with hosted CPython acceptance, native-only hostile-input refusal at 20,000, native total-depth 900 acceptance and 901 refusal with CPython accepting both boundary inputs, lone and paired surrogates, literal non-BMP, sentinel/collision near-misses, sorting and exact receipt bytes: `xtask/tests/browser_test_receipts.rs`. The Python script remains the US-72 entry point; no activation changes. |
| `cargo xtask migration self-test` | runs the 8 paired-verdict controls, the 6 trial-planner controls, the 3 planted slow-path decide controls and the 7 vector-step controls (5 compare, a planted home write, the step registry), all on synthetic data |
| `cargo xtask migration inventory --check` | checks that every tracked direct code path in the staged Git index has exactly one row in the [code inventory](plans/2026-11-01-rust-code-inventory.csv) plus every staged fragment in [`plans/rust-inventory.d/`](plans/rust-inventory.d/README.md) (sorted by name, no header; a path in two files is refused), then runs the `--sites-check` below |
| `cargo xtask migration inventory --sites-check` | discovers embedded code and generated entry points in the staged Git index and checks each against the [embedded sites](plans/2026-11-17-rust-embedded-sites.csv); an unassigned site, a row whose site is gone, a repeated row or a bad task is refused |
| `cargo xtask migration inventory --sites-list` | prints every discovered site as a `kind,path,key,task` manifest row; a new site has an empty task to fill in |
| `cargo xtask migration inventory --embedded-check` | checks the reviewed interpreter call sites in the [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) against staged source |
| `cargo xtask migration toolchain-identity --rustc <absolute-path>` | checks the pinned compiler release, its full commit hash in the Clippy driver and the shared compiler library. Read-only, no network. It supports the thin macOS Mach-O layout only; other layouts are refused. |
| `cargo xtask migration capture-actions --python <absolute-path> --out <new-file>` | runs `python -B -m careerbrew actions --json` twice in fresh synthetic homes and compares stdout, stderr, exit, schema and writes |
| `cargo xtask migration capture-http --python <absolute-path> --out <new-file>` | runs two fresh synthetic server processes over a frozen HTTP corpus (21 request cases) |
| `cargo xtask migration capture-data --python <absolute-path> --out <new-file>` | runs a ten-case write and read corpus twice and compares typed values, encrypted boundaries and declared writes |
| `cargo xtask migration resources --cmd <absolute-path> [args]` | runs one owned child, reaps it with `wait4`, and prints JSON: outcome (exit code or signal), user and system CPU time, peak RSS raw and in bytes. The process exit code is not the child's exit code: 0 means the child exited cleanly with 0; 1 means it was killed by a signal or exited nonzero; 2 is a usage error (a relative path); 3 is a measurement error (spawn, `wait4` or `getrusage` failed), printed as a JSON `error` object. On Linux, `rss_may_include_parent` marks a child peak at or below the measuring process's own peak. macOS and Linux only; another OS is refused. Process-group descendants are counted in the `tree_*` fields. On Linux with a delegated cgroup v2, tree CPU also counts descendants that leave the group (`tree_method` `cgroup-v2-cpu+subreaper-wait4`); without delegation they are not (see "Escapees: cgroup v2 CPU scope"). |
| `cargo xtask migration observer-probe --image <immutable-id> [--out <new-directory> [--freeze-only\|--fault-controls-only]]` | the fixed Linux process-observer probe in an owned, network-disabled container |

| `cargo xtask migration baseline --step <id> --python <absolute-path> --out <absolute-new-file> [--runs <n>]` | runs the step's registered Python oracle N times (default 30) through the process-tree collector. Each run gets a fresh synthetic home. The oracle starts in the repo root. It writes a content-free JSON record: step, command, OS, arch, Rust and Python versions, source revision, `source_tree_dirty` (from `git status --porcelain`), a hash of the relevant sources (the capture sources plus `baseline.rs` and `resources.rs`), the same value as the shared `evidence_sources_sha256`, `fixtures_sha256` (`tests/fixtures/http` plus the step's registered `Step::fixtures` files), `cache_state` (`warm-imports-fresh-synthetic-home`: imports and OS caches are warm, only the data folder is fresh), `meets_protocol_minimum`, raw samples (tree CPU user and system time, tree peak RSS, wall time, `parent_max_rss_bytes`, `rss_may_include_parent`) and p50 and p95. It refuses an unknown step, fewer than 30 runs (the test-only flag `--test-allow-few-runs` allows them and records `meets_protocol_minimum: false`), a relative `--python` or `--out`, an oracle run that does not exit 0, a set but empty `CAREERBREW_HOME`, an existing output, and a source or fixture change during the runs. Registered steps: `rust-02` (`xtask/tests/fixtures/purevec/driver.py --all`, a vector step; its source hash also covers the driver and its Python inputs) and `rust-03` (`careerbrew actions --json`; its source hash also covers the `crates/cbcli` sources, the registry, `scripts/gen-registry.py` and `config/native-routes.txt`). A step's extra environment (`Step::env`; `rust-03`: `COLUMNS=80`, `LINES=24`) is set on every baseline and measure run and on verify's CLI and registry projection runs (not its HTTP, DATA or role runs), on both sides, and is bound into the source hash as `step-env` (a step with none keeps its hash inputs). |
| `cargo xtask migration decide --step <id> --baseline <absolute-file> --verify <absolute-file> --measure <absolute-file> --out <absolute-new-file>` | reads one step's baseline record (`careerbrew-migration-baseline-v1`), a verify record (`careerbrew-migration-verify-v1`: `step`, a nonempty `checks` list of `{id, passed}`, and a `violations` map with the counts `contract`, `privacy`, `authorization`, `role`, `side_effect`, `missing_writes`, `duplicate_writes`, an `unobserved_violations` list of those names, and a `missing_contracts` list of contracts verify did not check) and a measure record (`careerbrew-migration-measure-v1`: `step`, a `regimes` list of `{cache_state, pair_ids, cached, candidate_cpu_us, candidate_tree_max_rss_bytes}`, cache state cold or warm; both regimes must be present). The three input paths must be absolute. Their full `source_revision`, shared `evidence_sources_sha256` and `fixtures_sha256` must be present, lowercase hexadecimal and equal; verify and measure must also carry the same `candidate_sha256`. A missing, malformed or mixed identity is refused with no verdict. The baseline's `meets_protocol_minimum` must agree with its sample count (30 or more), and its `cache_state` must be the baseline regime; otherwise the record is refused. It writes a content-free `careerbrew-migration-decide-v1` record: `activate`, or `keep-python` with fixed reason ids (`baseline-below-protocol-minimum`, `baseline-dirty-tree`, `regime-missing`, `verify-check-failed`, `verify-violation`, `verify-violation-unobserved` (a violation kind verify could not observe), `verify-contract-missing` (a contract verify did not check that the step does not scope out), `verify-contract-scope-mismatch` (the record leaves out of `missing_contracts` a contract the step scopes out: it was made under another scope; `missing_contracts` must equal the step's declared scope (a vector step such as `rust-02` declares none), `rust-03`: `http`, `data`, `role`), `verify-test-scope` (the record was made under a test-only `--test-scope`), `verify-python-not-denied` (a `no_python` step's record lacks `python_denied: "env"`), `too-few-unique-pairs`, `cached-sample`, `cpu-regressed`, `rss-regressed`). CPU compares the candidate p95 with the paired oracle p95 in the measure record, and memory compares the candidate maximum with the paired oracle maximum (the baseline `cpu_us_p95` is only echoed as `baseline_cpu_us_p95`); exactly 110% passes (activates). The baseline's `os` (`macos` or `linux`; another is refused) picks the step's entry in the noise floor registry (`xtask/src/noise_floor.rs`): below a measured floor the candidate must be at or under its ceiling, at or above it the ratio applies; an `unmeasured` entry (`rust-02` and `rust-03` on both OSes today) uses the ratio for every case. The measure record's `os` must equal the baseline's (a missing or other OS is refused). The record adds `noise_floor` (`measured`, `unmeasured` or `test-override`), `noise_floor_os`, `test_noise_floor`, `noise_floor_limits` (the four limits it used; all 0 for `unmeasured`), `noise_floor_method` and `noise_floor_record` (the registry entry's method and source record; null unless `measured`), and `sources_sha256` (`decide.rs`, `noise_floor.rs`, `baseline_verdict.rs` and the step registry `baseline.rs` as built into the binary). Test-only: `--test-noise-floor <absolute-file>` (`{cpu_floor_us, cpu_absolute_us, rss_floor_bytes, rss_absolute_bytes}`; a missing field is refused). Exit 0 activates, 1 keeps Python, 2 refuses a malformed record, a wrong schema, a step mismatch, an unknown step, a relative input path or an existing `--out` and writes nothing. The verify shape is what `migration verify` writes; the measure shape is minimal. |
| `cargo xtask migration noise-floor --baseline <absolute-file>` | proposes a noise floor from one baseline record (30 or more samples, `runs` equal to the sample count, clean tree, the baseline `cache_state`, OS `macos` or `linux`) and prints content-free JSON: step, OS, sample count, method, `cpu_floor_us`, `cpu_absolute_us`, `rss_floor_bytes`, `rss_absolute_bytes`. Floor = 10 x (nearest-rank p95 - p5) of `tree_cpu_us` or `tree_max_rss_bytes`; ceiling = floor x 1.10. It writes nothing; registering a floor is a reviewed edit of `xtask/src/noise_floor.rs`. Exit 2 refuses a relative path, a wrong schema, an unknown step or OS, a record below the protocol minimum, from a dirty tree, with another cache state or with a run count that differs from its samples. |

| `cargo xtask migration measure --step <id> --python <absolute-path> --out <absolute-new-file> [--pairs <n>]` | step 4 of the protocol (`xtask/src/measure.rs`). For each cache regime, cold then warm, it runs N pairs (default 30) of the step's Python oracle and its Rust candidate through the process-tree collector (`resources::measure_in`). The trial planner fixes which side runs first: at 30 pairs or more `measure` calls `trial_plan::plan` (with its own 30-pair refusal and one shared `MIN_PAIRS`); a test-flag run below 30 uses the same `trial_plan::order` rule. Cold odd pairs and warm even pairs start with the oracle. Each side of each pair gets a fresh synthetic home and starts in the repo root. **Cold:** no warm-up run, and each oracle run gets its own empty `PYTHONPYCACHEPREFIX`, so Python compiles every module from source. Cold changes only the Python side: for the Rust candidate, cold differs from warm only by the missing warm-up run, and the OS page cache is still warm from earlier runs, so a candidate's cold sample is not a cold start. **Warm:** one discarded warm-up run per side first; the oracle warm-up (without `-B`) fills one regime-owned `PYTHONPYCACHEPREFIX`, and the samples read it with `-B`. The two regimes never share a bytecode folder. Every sample is a real execution, so `cached` is 0. The record (`careerbrew-migration-measure-v1`) holds step, `oracle_command`, `candidate_command`, `candidate_sha256`, `candidate_profile` (`release` or `debug` from the registered path's `target/<profile>/` folder; `test-override` for a test candidate), `test_override`, OS, arch, Rust and Python versions, source revision, `source_tree_dirty`, `sources_sha256` (also covers `measure.rs` and `trial_plan.rs`), the shared `evidence_sources_sha256`, `fixtures_sha256`, `meets_protocol_minimum`, `pairs`, and a `regimes` list of `{cache_state, pair_ids, cached, first, orders, oracle_cpu_us, oracle_tree_max_rss_bytes, oracle_wall_us, candidate_cpu_us, candidate_tree_max_rss_bytes, candidate_wall_us, summary}` (one value per pair; `orders` logs the side order each pair actually ran in, and `first` is its first entry; `summary` has p50 and p95 per side). `decide` reads it without a schema change and judges each regime's candidate samples against that regime's oracle samples. It refuses a step with no Rust candidate (`step rust-NN has no Rust candidate yet`; `rust-02` has one, a debug binary with four vector sets: clock, hash, paths, settings, `rust-03` has the release `careerbrew-native` with `actions --json`, and `rust-04` has the release `policy-vectors --all <vectors-file>`, where measure writes the file once with the step's generator before the runs, appends it to the oracle's `--all` too, and records it as `<vectors-file>` in both commands), an unknown step, 0 pairs, fewer than 30 pairs without `--test-allow-few-pairs`, a relative `--python` or `--out`, a run that does not exit 0, a set but empty `CAREERBREW_HOME`, an existing output, and a source, fixture or candidate executable change during the runs (only the executable file is hashed). The interim `--runs` mode is gone: `--runs` exits 2 with `--runs is replaced by --pairs`. Test-only: `--test-candidate <absolute-path>` measures that executable (no arguments) instead of the registry; it is refused without `--test-allow-few-pairs`, and its record always has `meets_protocol_minimum: false`. |
| `cargo xtask migration verify --step <id> --python <absolute-path> --out <absolute-new-file>` | runs each of the step's registered cases on the Python oracle (`<python> -B -m careerbrew <args>`) and on the step's Rust candidate (`<candidate> <args>`). Each side of each case gets a fresh synthetic home and starts in the repo root. It compares up to six contracts; a step's flags (`http_contract`, `data_contract`, `role_contract`, `registry_projection` in `xtask/src/baseline.rs`) choose which run, and the ones it scopes out are listed in `missing_contracts`. CLI: stdout and the exit code, byte for byte, after the case's registered normalization rules (`xtask/src/normalize.rs`; the record lists them per case as `normalization`, and the stdout hashes are of the normalized text). One rule today: `synthetic-home` (the side's `CAREERBREW_HOME`, raw and canonical, reads `<home>`). A stdout that still holds a local path after the rules (the repo root, the side's scratch folder, and the caller's `HOME` when it is set; with `HOME` unset only the first two are checked) refuses verify with no record and exit 1 (`case <id> prints a local path with no registered normalization rule; verify refused`). Side-effect entries are keyed by their path inside each side's own synthetic home, so they need no path rule; the HTTP comparison uses the `capture-http` header normalization. Side effects: the entries each side adds, changes or removes in its synthetic home against a setup snapshot taken before it runs (path, mode, content hash), counted as extra, missing or changed. This compares final state, not a write trace: a rewrite with the same bytes, or a write that is later removed, is not seen (`side_effects_observed: "final-state"` for the CLI and HTTP cases; the DATA cases trace writes per phase, below). HTTP (steps marked `http_contract`; none today): the 21 `capture-http` fixture cases sent over a real socket to the oracle (`http_driver.py`) and to the candidate started as `<candidate> --careerbrew-http-contract` (it reads the capture control file, publishes `ready.json` with its pid, host `127.0.0.1` and port, and stops on the `stop` file); status, normalized headers and body must match, and the live oracle must still match the frozen `capture-http` reference or verify refuses. The HTTP side effects are one more case, `http:side-effects`, so a difference there names a failed case. A candidate server that exits stops the requests at once and leaves the rest unanswered. Registry projection (steps marked `registry_projection`; `rust-03` is): the candidate's `actions --json` stdout, wrapped as `{"actions": ...}`, must equal the body of the frozen capture-http cases 0 (`actions`) and 4 (`localhost`), both `GET /api/actions`; no server starts. The live Python oracle must match the same bodies or verify refuses. One case, `registry-projection` (`kind` `registry-projection`, with `candidate_ran`, `exit_zero`, `body_matches`, `http_cases` and both body hashes); a failure is a `contract` count. DATA (steps marked `data_contract`): the `capture-data` corpus (10 cases, 19 phases, `docs/plans/2026-11-05-rust-data-fixture-v1.json`) runs in order in one fresh synthetic home per side (the cases build on each other). The oracle is the Python data driver (`<python> -B -S data_driver.py <trial> <case> <role>`); it runs twice, in two fresh homes, and verify refuses when the two runs differ or a phase fails. The candidate runs as `<candidate> --careerbrew-data-contract <trial> <case> <role>` with the same environment (`CAREERBREW_DATA_FIXTURE`, `CAREERBREW_DATA_OWNED`, `CAREERBREW_DATA_REPORT`, `CAREERBREW_CAPTURE_ROOT`) and writes the same phase report. Each phase's API calls (function, purpose, target, argument hash, returned value) must match. A write trace snapshots the home after each phase (mode, inode, mtime; `protocol/` left out): a path the candidate writes in more phases than the oracle is a `duplicate_writes` count (`writes_traced: "per-phase"`; two writes inside one phase count as one; a write the candidate makes in another phase than the oracle, in the same number of phases, is not counted, and with the same bytes no other check sees it). File contents are left out of the DATA side effects (rule `data-ciphertext`: each home has its own key); the reader phases prove the content. One case per corpus case, `data:<id>`. Role (steps marked `role_contract`): the Python oracle makes a fresh home a corporate machine (`machine set corporate`), then each side runs `jobs list`; the oracle must refuse it (nonzero exit) or verify refuses. A candidate with another exit code, stdout or writes fails `role:jobs-list`. `rust-03` scopes out HTTP, DATA and role (`missing_contracts: ["http", "data", "role"]`): its pilot binary serves none of them. No Python: a step marked `no_python` (`rust-03` is) runs every candidate side with Python denied. The synthetic environment has no `PATH`, and `PYTHONHOME` points at a missing folder, so a CPython started by absolute path cannot load its standard library and exits nonzero (`python_denied: "env"`; null when not denied). This is an environment denial, not a process-level proof: a candidate that clears its environment or starts Python with `-E` or `-I` is not caught. The planted control is a candidate that execs the Python CLI by absolute path: it fails under the denial and passes with `--test-scope allow-python`. It writes a content-free JSON record: step, source revision, `source_tree_dirty`, `sources_sha256` (the capture sources plus `baseline.rs`, `verify.rs`, `normalize.rs`, `capture_http.rs`, the HTTP fixtures, driver and reference, and the DATA driver `data_driver.py` and fixture `2026-11-05-rust-data-fixture-v1.json`), `test_candidate` (true when `--test-candidate` chose the candidate), `test_oracle` (true when `--test-oracle` replaced the CLI oracle) and `candidate_sha256` (hashed before and after the runs; a change refuses the record), `test_scope` (the test-only scope, or null), `python_denied`, `contracts` (the contracts it ran), `case_count`, `passed_count`, `failed_cases`, and per case its id, `kind` (`cli`, `http`, `http-side-effect`, `registry-projection`, `data` or `role`) and pass or fail: a CLI case adds `candidate_ran`, `exit_matches`, `stdout_matches`, `normalization`, both stdout hashes, `writes_match` and the `extra_writes`, `missing_writes` and `changed_writes` counts with both write-set hashes; an HTTP case adds `candidate_answered`, `status_matches`, `headers_match`, `body_matches` and both body hashes; a data case adds `phases`, `candidate_phases_ran`, `calls_match`, `normalization`, both call hashes, the write counts with `duplicate_writes`, `canary_leaked` and both write-set hashes; the role case adds `candidate_ran`, `oracle_refused`, `exit_matches`, `stdout_matches` and the write counts. It is also `decide`'s verify input: `checks` (one `{id, passed}` per case), `violations` (`contract`: failed CLI or HTTP cases and data cases whose calls differ; `privacy`: channels that hold the synthetic HTTP canary, and data cases in which a file starts to hold the corpus canary (a leak counts once, in the case that made it); `authorization`: a 401 or 403 case answered with another status, another body, or no answer; `role`: a failed role case; `side_effect`: extra or changed entries; `missing_writes`: missing entries; `duplicate_writes`: extra per-phase writes of a data path) and `unobserved_violations` (empty now) and `missing_contracts` (the step's scoped-out contracts) (each kind has its planted-defect control in `xtask/tests/verify.rs`). A candidate that cannot start, times out, or prints output that is not UTF-8 or is too large fails its case (`candidate_ran: false`). Exit 0 only when every case matches and every count is 0; a differing case writes the record, names the case and exits 1. It refuses an unknown step, a step with no Rust candidate yet (`step rust-NN has no Rust candidate yet; verify refused`), a CLI oracle case that exits other than 0 or 2 (2 is an argparse usage error, which the candidate must then match; the role oracle must exit nonzero instead), a relative `--python` or `--out`, a set but empty `CAREERBREW_HOME`, an existing output, and a source change during the run. The test-only flag `--test-candidate <absolute-path>` points at a candidate before a port registers one; the test-only flag `--test-oracle <absolute-path>` runs a script in place of the Python oracle for the CLI cases (the HTTP, registry projection, DATA and role oracles stay Python); the test-only flag `--test-scope full` runs HTTP, DATA and role whatever the step scopes, with Python allowed and no registry projection (the harness controls use it), and `--test-scope allow-python` keeps the step's scope but lifts the Python denial; both are recorded as `test_scope`, and `full` on `rust-03` makes `missing_contracts` empty, which `decide` holds as a scope mismatch. Registered cases: `rust-03` has 14 CLI cases (see the rust-03 step) and `registry-projection` (15 cases; 47 under `--test-scope full`); its candidate is `target/release/careerbrew-native`, refused with `the candidate binary does not exist` until it is built. Not yet covered: the native tests, the step's user stories, a process-level Python denial, writes inside one phase, and a same-count write moved to another phase. A vector step (`rust-01`, `rust-02`) skips all of this and compares vectors instead (see the rust-01 and rust-02 vector framework sections); `--test-wrong-vector <set>` and `--test-plant-home-write <file-name>` apply only there. |

**Built:** the paired, cold and warm `measure --pairs` runs of step 4. No
step has a registered Rust candidate yet, so no real record exists.

### Tests and gate steps

- `scripts/check.sh` runs `rust-fmt`, `rust-clippy`, `rust-test` and
  `rust-inventory` when `Cargo.toml` exists. These steps never reuse a cached
  pass.
- `scripts/setup.sh` and `scripts/doctor.sh` call `scripts/rust-readiness.sh`.
  It refuses a missing or mismatched Rust before any fetch. It never runs an
  installer.
- The hosted CI installs the pinned toolchain and runs the gate on Ubuntu and
  macOS ([docs/ci.md](ci.md)). It was green on `main` at `ecbbb7fc`.
- Integration tests in `xtask/tests/`: `activation_manifest`, `capture_actions`,
  `baseline`, `capture_data`, `capture_data_cli`, `capture_http`, `decide`, `i01_fixture`,
  `observer_deadline`, `observer_owner`, `observer_parser`,
  `observer_transport`, `resources`, `store_supervisor`, `verify`,
  `leak_check`, `input_key`, `check_docs`.
- Python cross-checks in `tests/unit/`: `test_rust_vault_interop.py`,
  `test_rust_vault_base64_parity.py`, `test_rust_hash_experiment.py`,
  `test_rust_readiness.py`, `test_doctor_rust_deps.py`,
  `test_gate_cache_runtime.py`.

### History and proof

- `git log --oneline -- crates xtask experiments` shows two commits on the
  public history: the initial public release, and the hosted CI change. The
  earlier work came in with the initial public release.
- `docs/proof/` holds no Rust migration result. Its only Rust content is the
  crate licence list.

## State of rust-00

**rust-00 is done (signed off 2026-11-18).** Its gate holds: the harness
rejects every planted defect (wrong output, leaked canary, skipped test, slow
path), accepts the unchanged oracle, and the inventory has 0 unassigned
paths, and `migration self-test` (17 controls) and `inventory --check` pass in
the hosted gate on `main` (run 44284505238, after #131). The real noise-floor
values are deferred follow-ups, not part of the gate. Complete descendant
coverage remains a documented limit, not an activation claim. The open items
below either have their own tasks in `TASKS.md`
(`rust-00-measure-noise-floors`, `rust-00-inventory-process-observation`) or
say why they block nothing; none blocks `rust-01`.

Noise-floor runs (2026-11-20): the manual workflow
[`noise-floor.yml`](../.github/workflows/noise-floor.yml) (`gh workflow run
noise-floor.yml -f step=rust-03`) runs the step's baseline twice on a clean
checkout on `ubuntu-24.04` and `macos-15`, then `migration noise-floor` on each
record, and prints the two proposals per OS with the run id and commit in the
job summary. No floor is registered yet: every entry stays `unmeasured` until
a run's two proposals agree within 3x, and a local Mac run is not a source,
because `decide` judges hosted baselines.

This section is the record of the step. It replaces the old continuation notes. Those notes and the old
draft pull request are not on the public repository.

### Done (in the public tree)

| Part | Result | Command |
| --- | --- | --- |
| Workspace, toolchain pin, `cargo xtask` alias | in place | `cargo xtask migration self-test` |
| Paired verdict (`baseline_verdict.rs`) | 8 of 8 synthetic controls pass; an empty check suite is refused | `cargo xtask migration self-test` |
| Trial planner (`trial_plan.rs`) | 6 of 6 controls pass | `cargo xtask migration self-test` |
| Planted slow path (`decide::controls`) | 3 of 3 self-test controls (3x CPU holds with `cpu-regressed`, 2x RSS holds with `rss-regressed`, equal cost activates); each slow control fails an always-activate harness, and the equal-cost control fails an always-hold harness; 3 end-to-end controls on real `rust-03` records (slow CPU and 512 MiB candidates pass `verify`, and `decide` holds them; an equal-cost candidate gets no resource reason). See "Planted slow-path controls" | `cargo xtask migration self-test`; `cargo test -p careerbrew-xtask --test measure --test decide` |
| Direct-path inventory | 0 unassigned tracked direct paths; 4 negative controls (removed, duplicated, untracked row, broken CSV field) refused; runs in the normal gate | `cargo xtask migration inventory --check` |
| Embedded anchors | the reviewed sites match staged source; a missing or repeated anchor is refused | `cargo xtask migration inventory --embedded-check` |
| Embedded sites and entry points | 191 discovered sites assigned (123 embedded lines, 27 agent skills, 17 workflow runs, 8 npm scripts, 7 agent commands, 4 lefthook runs, 3 git hooks, 1 agent hook, 1 console script); 13 controls (each kind found and prose skipped, full pass, unassigned planted site, removed site, duplicate row and bad task, unlisted real interpreter forms, tool flags skipped, a repeated identical line, YAML run blocks, skill directories and flow lists, malformed entry-point files, non-UTF-8 files, list rows as manifest rows) in `xtask/src/sites_tests.rs`; runs in the normal gate inside `inventory --check` | `cargo xtask migration inventory --sites-check` |
| Toolchain identity probe | 7 cases (valid, release mismatch, malformed identity, missing driver, outside symlink, exact token, bounded scan) | `cargo test -p careerbrew-xtask toolchain_identity` |
| Action contract capture | 7 integration controls | `cargo test -p careerbrew-xtask --test capture_actions` |
| HTTP contract capture | 13 control groups; 21 request cases in each of two fresh processes | `cargo test -p careerbrew-xtask --test capture_http` |
| Data contract capture | 10 corpus cases in each of two fresh trials; 13 control groups | `cargo test -p careerbrew-xtask --test capture_data` |
| Activation manifest | three partial slices; 20 of 20 controls; zero multi-function cycles | `cargo test -p careerbrew-xtask --test activation_manifest` |
| Baseline record | 1 positive control (3 test-flag runs of `rust-03` write a valid record, no local path, no overwrite with the bytes unchanged); 5 refusals (unknown step, planted oracle that exits 7, empty `CAREERBREW_HOME`, 29 runs without the test flag, relative `--out`) | `cargo test -p careerbrew-xtask --test baseline` |
| Paired measure record | 1 paired positive control (3 test-flag pairs of a `--test-candidate` script against the `rust-03` oracle write cold and warm regimes, and `orders` logs the executed order of every pair, no local path, no overwrite); 1 end-to-end control (`decide` reads a real 1-pair measure record and a real 1-run baseline record and keeps Python for `too-few-unique-pairs` and `baseline-below-protocol-minimum`, not `regime-missing`); 12 refusals (unknown step, candidate that exits 7, relative `--test-candidate`, relative `--python`, 29 pairs without the test flag, 0 pairs, relative `--out`, an existing `--out` before any run, empty `CAREERBREW_HOME`, test candidate without the test flag, old `--runs` flag, candidate change during the runs); 3 in-crate input-binding controls on a scratch copy of the inputs (a `measure.rs` or `trial_plan.rs` change, a fixture change, a candidate change) | `cargo test -p careerbrew-xtask --test measure`; `cargo test -p careerbrew-xtask --bin careerbrew-xtask measure::controls` |
| Differential verify | 1 positive control (a test candidate that runs the oracle for the CLI and HTTP contracts passes all 47 `rust-03` cases under `--test-scope full` with all seven violation counts 0, no local path, no overwrite with the bytes unchanged); 5 planted defects (a different stdout fails and names `actions-json`; the same stdout with exit 3 fails; a wrong HTTP body that also prints the canary fails `http:ping` with contract, authorization and privacy counts; an extra entry in the synthetic home gives `side_effect` 1; a missing entry gives `missing_writes` 1); 2 refusals (unknown step, relative `--out`); in-crate, an unbuilt registered `rust-03` candidate is refused | `cargo test -p careerbrew-xtask --test verify` |
| Noise floors and normalization | 6 decide controls (unmeasured floor falls back to the ratio; below-floor within and beyond the ceiling; at-floor ratio; the warm floor does not cover the cold regime; malformed floor and unknown OS refused) and the `noise-floor` measurement control; 2 verify controls (`synthetic-home` normalizes the CLI stdout, with a candidate that serves the Python HTTP driver; an unregistered repo-root path is refused). `rust-03` floors are `unmeasured` until hosted runs (task `rust-00-measure-noise-floors`) | `cargo test -p careerbrew-xtask --test decide --test verify` |
| Owned-child resources | 4 pre-registered controls: 64 MiB calibration, planted wrong unit refused, CPU burn above 0, `SIGKILL` reported as `signaled` and exit 1. 4 added after registration (marked as such): JSON shape and relative-path refusal; malformed `rusage` fields refused (from draft #64); idle child run through the CLI from a parent that holds 128 MiB does not inherit it; a spawn failure is exit 3 with a JSON error. 2 process-tree controls (unwaited grandchild CPU burn and 64 MiB hold). Ratios: see "Owned-child resource accounting". | `cargo test -p careerbrew-xtask --test resources` |
| Synthetic store supervisor | 16 controls for the future mixed Python and Rust store race | `cargo test -p careerbrew-xtask --test store_supervisor` |
| Local Rust readiness | 12 readiness controls and the Serde doctor cases | `uv run pytest -q tests/unit/test_rust_readiness.py tests/unit/test_doctor_rust_deps.py` |
| Linux observer | one qualified trial on one Linux/arm64 tuple: 5 of 5 event classes, the incomplete (no-follow) trace refused, both real fault controls passed, owned containers absent after cleanup | `heavy-run --exclusive cargo xtask migration observer-probe --image <id>` |

Component experiments, built early as an explicit build-order exception. They
are **component parity only** and switch nothing:

- **Vault codec:** 24 of 24 cross-language round trips; 12 plaintext cases
  pass through; 13 invalid tokens refused; a tampered token refused. A bounded
  base64 parity set agrees on 1,036 of 1,036 decisions, with 4 tampered tokens
  refused. Finite cases do not prove every token spelling.
- **Hash experiment:** 40 of 40 supported cases match in bytes and digest;
  7 of 7 unsupported inputs refused; a planted wrong serializer fails.

### Failed or not proved

- The observer result qualifies only the registered synthetic observer and its
  inputs. It does not qualify app parity, CPU or RSS, other platforms, or the
  full harness. A first offline image build failed (the pinned `strace`
  package was not in the cache); a separate network-enabled preparation then
  built the image. Raw receipts stay local and are not in the repository.
- Whole-process-tree resource collection is calibrated on one hosted run
  (44099007218: macOS 0.996x, Linux 0.972x; a second run is pending; see "Process-tree resource accounting"
  below). It counts descendants that stay in the child's process group. Hosted
  Ubuntu could not create a delegated cgroup v2: PR 192's run 44718131412
  recorded `method=subreaper-wait4 escaped_cpu_us=0 supported=false`. The
  pre-registered pivot therefore marks escapee CPU accounting unsupported on
  hosted Ubuntu and keeps the group-only fallback. A delegated cgroup on
  another Linux host may count a `setsid` or `setpgid` descendant's CPU, but
  that path has no hosted qualification. macOS still misses escapee CPU, and
  no OS counts escapee peak RSS.
- One full local gate on the old draft failed one owned-process cleanup test
  (all others passed). Its cause was never found. A later fast gate passed
  without explaining it. The hosted gate on `main` is green today; treat any
  repeat as a real failure and read its log. Not a gate item: no task until it
  repeats.
- A nonempty check label does not prove that a test ran. The future collector
  must bind that evidence. Not a rust-00 gate item: each port step's `verify`
  record names its checks, and binding them to executed tests is work for the
  collector those steps use.
- Embedded-code discovery is done (`inventory --sites-check`). Process
  observation of what a run really starts is open (task
  `rust-00-inventory-process-observation`). Slice 1 (2026-11-20) adds the
  comparison step: `cargo xtask migration inventory --exec-check <trace-file>
  --root <dir>` reads one absolute exec target per line, counts a target under
  `--root` that is an inventory row as assigned and one outside it as external,
  and prints every other target as unassigned (exit 1; refused input exit 2).
  A relative target or one with a `.` or `..` part counts as unassigned.
  Slice 2 (2026-11-21) adds strict extraction of successful `execve` targets
  from complete fixed-protocol traces. The positive observer receipt records
  them as `observation.exec_targets`; failed executions start no process, and
  malformed or unsupported records refuse the whole observation. Offline replay
  of the byte-pinned v7 fixture extracts `/usr/local/bin/python` and compares it
  as one external target with 0 unassigned. Controls also map a listed `/work`
  script and report a planted unlisted script. No live gate-story trace has run:
  the 0-unassigned result covers only the registered synthetic replay, not the
  task's full story hypothesis.

### Owned-child resource accounting (pre-registered 2026-11-16)

Registered before the code. Scope: one owned child, reaped with `wait4`.
Grandchildren are **not qualified** by this part.

- **Hypothesis:** `cargo xtask migration resources --cmd <absolute-path>`
  reports the child's user and system CPU time and its peak RSS in bytes on
  macOS and Linux. The raw `ru_maxrss` unit differs: bytes on Darwin, KiB on
  Linux. With the right unit, a child that allocates and touches 64 MiB shows
  a peak RSS 64 MiB above an idle child of the same binary, within tolerance.
- **Measurement:** the hosted gate runs `cargo test --all`, which includes
  `cargo test -p careerbrew-xtask --test resources` on `ubuntu-24.04` and
  `macos-15`. Four controls:
  1. a 64 MiB allocation child: the RSS delta over an idle child is in range;
  2. a CPU-burn child: user plus system CPU time is above 0;
  3. a child killed by `SIGKILL`: reported as `signaled`, exit 1, never a pass;
  4. a planted wrong unit (the other OS's unit applied to the same raw value):
     the calibration check must refuse it.
- **Success:** all 4 controls pass on both hosted OSes. The calibration delta
  is between 0.75x and 1.5x of 64 MiB (48 to 96 MiB) on each OS.
- **Pivot:** if the delta falls outside 0.5x to 2x on either OS in two hosted
  runs, `wait4` `ru_maxrss` is not a usable RSS source there. Then stop and
  read `/proc/<pid>/status` (Linux) or `proc_pid_rusage` (macOS) instead.
- **Anchor:** POSIX `getrusage`/`wait4` (IEEE Std 1003.1-2017); Linux
  `getrusage(2)` ("ru_maxrss ... in kilobytes"); Darwin `getrusage(2)`
  (bytes). Measurement method: Kalibera and Jones 2013, "Rigorous
  Benchmarking in Reasonable Time" (ISMM).

Observed (filled from hosted CI logs, `calibration` and `inheritance` lines):

- macOS local debug build: delta 1.000x (67,125,248 of 67,108,864 bytes), 3 of 3 runs.
- Hosted `macos-15` (run 44001512071): idle 1,359,872, loaded 68,485,120,
  delta 67,125,248 bytes, ratio 1.000.
- Hosted `ubuntu-24.04` (same run): idle 3,481,600, loaded 69,201,920,
  delta 65,720,320 bytes, ratio 0.979.
- Parent inheritance, from a test parent that holds 128 MiB: on Linux a
  child spawned directly reports the parent's peak (137,568,256 bytes,
  `rss_may_include_parent: true`); the same idle child run through the
  `resources` CLI reports 4,104,192 bytes. macOS does not inherit
  (1,359,872 bytes both ways). So the qualified path is the CLI, from a small
  parent; a direct in-process call on Linux reports a floor, flagged as such.
- Hosted run 44099007218 (PR 115): `macos-15` idle 1,458,176, loaded
  68,534,272, delta 67,076,096 bytes, ratio 1.000; `ubuntu-24.04` idle
  4,218,880, loaded 69,193,728, delta 64,974,848 bytes, ratio 0.968. Both OSes
  are inside 0.75x to 1.5x in two hosted runs, so the pivot does not apply.
- The gate step runs `cargo test --all` without `--nocapture` (the gate's
  frozen tool contract, `G03` in `tests/unit/test_gate_cache.py`, refuses
  extra arguments). So the tests also append each `calibration`,
  `inheritance` and `tree calibration` line to the file that
  `CAREERBREW_RESOURCES_RECORD` names. The hosted gate sets it, and its
  "Resource calibration lines" step prints the file to the job log and the job
  summary: `gh run view --job <native-job-id> --log | rg calibration`.

### Process-tree resource accounting (pre-registered 2026-11-17)

Registered before the code. Scope: the owned child and every descendant that
stays in the child's process group. The owned-child fields and their
calibration above stay unchanged; the tree adds `descendants`,
`tree_user_cpu_us`, `tree_system_cpu_us`, `tree_max_rss_bytes` (the largest
peak of any one process, not a sum) and `tree_method`.

- **Method:** the child starts in its own process group. Linux: the measuring
  process sets `PR_SET_CHILD_SUBREAPER`, so orphans come back to it, and it
  reaps the group with `wait4(-pgid)`; on `ECHILD` it keeps waiting while
  `kill(-pgid, 0)` finds a member (see "Member whose parent left the group"
  below). macOS has no subreaper:
  it samples each group member with `proc_pid_rusage` (`RUSAGE_INFO_V4`,
  Mach ticks converted with `mach_timebase_info`) every 2 ms until the group
  is empty. A descendant's peak is
  `max(ri_lifetime_max_phys_footprint, ri_resident_size)`. Sampling can miss
  the last CPU slice and a process shorter than one interval. A descendant
  that never exits keeps the command waiting.
- **Hypothesis:** a child that starts a grandchild and exits without waiting
  shows the grandchild's 64 MiB allocation and its CPU burn in the tree
  fields, where the owned-child collector shows about 0.
- **Measurement:** `cargo test -p careerbrew-xtask --test resources` on hosted
  `ubuntu-24.04` and `macos-15`, two added controls:
  1. `orphan burn 300`: tree CPU at least 100 ms above the child's CPU;
  2. `orphan hold <bytes> 200`: the tree peak delta against `orphan hold 0
     200` passes the same 0.75x to 1.5x calibration.
- **Success:** both controls and the existing controls pass on both OSes.
- **Pivot:** if either OS falls outside 0.5x to 2x in two hosted runs, mark
  tree accounting unsupported there and require measured commands to wait
  for their own children.
- **Anchor:** POSIX `getrusage`/`wait4` (IEEE Std 1003.1-2017); Linux
  `prctl(2)` `PR_SET_CHILD_SUBREAPER`; Darwin `proc_pid_rusage`; Kalibera and
  Jones 2013 (ISMM).

Observed (hosted run 44099007218, PR 115, `tree calibration` lines):

- `macos-15`, method `sampled-proc-pid-rusage`: delta 66,846,720 bytes,
  ratio 0.996.
- `ubuntu-24.04`, method `subreaper-wait4`: delta 65,200,128 bytes, ratio
  0.972.
- Both inside 0.75x to 1.5x, and the grandchild CPU control passed on both,
  so the pivot does not apply. One hosted run so far; the pivot needs two
  runs outside 0.5x to 2x, and every gate run now prints the line.

Review gaps closed (PR 113 review):

- **Mixed peak metric (macOS).** The JSON field `tree_rss_metric` names what
  `tree_max_rss_bytes` compares. macOS:
  `child=ru_maxrss;descendants=max(lifetime_phys_footprint,resident_size)`,
  because no lifetime resident peak exists for another process. Linux:
  `child=ru_maxrss;descendants=ru_maxrss`. Control:
  `the_tree_peak_names_the_metric_of_each_part`.
- **Member whose parent left the group (Linux).** When `wait4(-pgid)` says
  `ECHILD`, the collector checks the group with `kill(-pgid, 0)` and keeps
  waiting while a member lives; that member comes back to the subreaper when
  its parent exits. The parent that left the group is not counted. It is reaped
  only when the invocation's cgroup observed its PID. The fallback does not use
  `waitid(P_ALL)`: it cannot distinguish that parent from an unrelated direct
  child owned by a parallel invocation, so an unproved parent can stay waitable
  until the measuring process exits. Any
  `kill` error other than `ESRCH` is an error, not an empty group. If that
  parent never exits (a daemon), the call polls every 2 ms forever. Control
  (Linux only): `a_member_whose_parent_left_the_group_is_counted`.
  Unsupported targets report `tree_rss_metric` `unsupported`.
  Control: `a_member_whose_parent_left_the_group_is_counted` (fixture mode
  `split 500 burn 300`).

### Escapees: cgroup v2 CPU scope (pre-registered 2026-11-20)

Registered before the hosted run. Scope: Linux tree CPU only.

- **Method:** before the spawn, the measuring process creates
  `careerbrew-measure-<pid>-<n>` under its own cgroup (from
  `/proc/self/cgroup`, line `0::`). The child writes `0` to its
  `cgroup.procs` between fork and exec. If the cgroup cannot be created or
  joined, the child never ran and the call spawns again without it (the old
  method; `tree_method` stays `subreaper-wait4`). With the cgroup, after the
  group is reaped the call waits for `populated 0` in `cgroup.events`, reaps
  each pid it saw in `cgroup.procs`, reads `user_usec` and `system_usec`
  from `cpu.stat`, subtracts the child's own CPU, and keeps the larger of
  that and the group sum. Then it removes the empty cgroup. `tree_method` is
  `cgroup-v2-cpu+subreaper-wait4`. Peak RSS stays per process: `memory.peak`
  needs the memory controller, and a cgroup that holds processes cannot
  enable it for its children. A descendant that never exits keeps the call
  waiting, as before.
- **Hypothesis:** a grandchild that calls `setsid` and burns 300 ms of CPU
  is counted within 0.75x to 1.5x, where the group method reports about 0.
- **Measurement:** `cargo test -p careerbrew-xtask --test resources` on
  hosted `gate (ubuntu-24.04)`, control
  `a_descendant_that_calls_setsid_is_counted_by_the_cgroup` (fixture mode
  `escape burn 300`). It records `escapee ...: method=... escaped_cpu_us=...
  supported=...` in the resources record.
- **Success:** `supported=true` and 225,000 to 450,000 us, with all other
  controls passing.
- **Pivot:** if the hosted record says `supported=false` (no delegated
  cgroup), mark escapee accounting unsupported on hosted runners, keep the
  fallback, and stop.
- **Anchor:** Linux cgroup v2 (`cgroup.procs`, `cgroup.events`, `cpu.stat`);
  Kalibera and Jones 2013 (ISMM).

Observed (hosted run 44718131412, PR 192 at `99ef5535`, native Ubuntu job
136226651307):

```text
escapee "linux": method=subreaper-wait4 escaped_cpu_us=0 supported=false
```

All 15 resource tests passed, but the positive 225,000 to 450,000 us control
did not run. A green capability-conditional test is not qualification. This
record meets the pivot: escapee CPU accounting is **unsupported on hosted
Ubuntu**. The group-only fallback stays, and this investigation stops without
changing collector behavior or production activation. The exact-PID ownership
rule and unrelated-child waitability control above stay unchanged.

Final `rust-05` timing on hosted Ubuntu must therefore read `tree_*` as the
owned child plus counted process-group members only. It must not claim CPU or
peak RSS from descendants that leave the group. A workload that needs those
escapees remains unqualified for whole-tree accounting.

### Noise floors and normalization rules (pre-registered 2026-11-17)

Registered before the code, with its controls, for "Next" item 4.

**Hypothesis.** A registered floor per step and OS lets `decide` judge a tiny
baseline by an absolute ceiling instead of a ratio that noise can break. A
registered rule per case lets `verify` compare outputs that hold a fresh
synthetic path, and refuses any other local path.

**Noise floor registry** (`xtask/src/noise_floor.rs`). One entry per step and
OS (`macos`, `linux`). Each entry is `unmeasured`, or `measured` with CPU floor
and ceiling (us), RSS floor and ceiling (bytes), the method and the record it
came from. The OS is the baseline record's `os`; another OS is refused.

- Measured: below the floor, the candidate must be at or under the ceiling.
  At or above it, the 1.10 ratio applies (`baseline_verdict::within_limit`).
- Unmeasured: every case uses the 1.10 ratio. This is stricter, never looser,
  so a missing floor can hold a step but never activate one.
- The record says `noise_floor` (`measured`, `unmeasured` or
  `test-override`), `noise_floor_os` and `test_noise_floor`.
- Test-only: `--test-noise-floor <absolute-file>` gives the four numbers.
- How a floor is measured: `cargo xtask migration noise-floor --baseline <file>`
  reads one baseline record (30 or more samples, clean tree). Spread is the
  nearest-rank p95 minus p5 of the samples. Floor = 10 x spread, so the 10%
  margin is at least the spread. Ceiling = floor x 1.10, so the two rules
  meet at the floor. CPU uses `tree_cpu_us`, RSS uses `tree_max_rss_bytes`.
- `rust-03` is `unmeasured` on both OSes. No number is invented here: the
  hosted baseline runs come first (task in `TASKS.md`).
  The hosted runs come from `.github/workflows/noise-floor.yml`.

**Normalization registry** (`xtask/src/normalize.rs`). One list of rule ids
per step and case. Rules apply to both sides before the compare. One rule
today: `synthetic-home` replaces the side's `CAREERBREW_HOME` (raw and
canonical) with `<home>`. Registered: every `rust-03` CLI case:
`synthetic-home`. After the rules, a stdout that still holds a local path (the
repo root, the side's scratch folder, or the caller's `HOME`) refuses verify
with no record. Test-only: `--test-oracle <absolute-path>` runs a script in
place of the Python oracle; the record says `test_oracle`.

**Controls** (registered before the code):

| Control | Expected |
| --- | --- |
| unmeasured floor, candidate 1,500 us on B 1,000 us (activates under the test floor below) | `keep-python`, `cpu-regressed`, `noise_floor: unmeasured`, limits all 0, `sources_sha256` set |
| B 1,000 us below a 2,000 us floor, candidate 1,500 us, ceiling 1,600 us | `activate` |
| same floor, candidate 1,601 us | `keep-python`, `cpu-regressed` |
| B at the floor, candidate 1,101 us / 1,100 us | hold / activate (ratio) |
| floor file missing a field; baseline OS `plan9` | refused, exit 2, no record |
| `noise-floor` on 30 samples 1,000..1,029 us | floor 270, ceiling 297 |
| oracle and candidate print their own home | pass after `synthetic-home` |
| oracle and candidate print the repo root | refused, exit 1, no record |
| measure record from another OS, or with no `os` (added after review) | refused, exit 2, no record |
| test floor record (added after review) | `noise_floor_limits` equals the floor file |
| `noise-floor` on another cache state or `runs` 31 (added after review) | refused, exit 2 |

**Measurement:** `cargo test -p careerbrew-xtask --test decide --test verify`
on the hosted gate. **Success:** 11 of 11 controls pass (8 pre-registered, 3 added after review) and the earlier decide
and verify controls stay green. **Pivot:** if a real floor from two hosted
baselines differs by more than 3x between runs on one OS, the p95-p5 method
is too noisy; switch to a bootstrap interval before registering numbers.
**Anchor:** Kalibera and Jones 2013 (variation and minimum effect size).

### Planted slow-path controls (pre-registered 2026-11-18)

Registered before the code, with its controls, for "Next" item 5.

**Hypothesis.** A candidate that prints the oracle's exact output but costs
far more CPU or memory passes `verify` and still gets `keep-python` from
`decide`. An equal-cost candidate gets no resource reason. So the decide step
holds a slow path on its own, not only through the output compare.

**Controls, end to end** (real `baseline`, `verify`, `measure --pairs` and
`decide` records on `rust-03`, test flags, 3 runs and 3 pairs; in
`xtask/tests/measure.rs`). Each candidate runs the real Python oracle:

| Candidate | Expected |
| --- | --- |
| slow CPU: runs the oracle 3 times, prints once (about 3x CPU) | `verify` passes; `decide` `keep-python` with `cpu-regressed` |
| big memory: a child holds 512 MiB, then the oracle | `verify` passes; `decide` `keep-python` with `rss-regressed` |
| equal cost: `exec` of the oracle | `verify` passes; no `cpu-regressed` and no `rss-regressed` |

`decide` reads the real `verify` record, with its `checks` and `violations`,
so each run proves the whole baseline, verify, measure and decide chain. A test
candidate gets no arguments from `measure`, so each candidate names
`actions --json` itself.

With test flags, every verdict also holds `too-few-unique-pairs` and
`baseline-below-protocol-minimum`, so the equal-cost run cannot activate end
to end. Activation of an equal-cost candidate is proved in the self-test on
synthetic records of 30 samples and pairs.

The equal-cost run passes `decide` a test noise floor above every sample,
with an absolute limit of 3x the baseline. A hosted runner can swing CPU
time by more than 10% between the baseline and the measure runs, so the
1.10 ratio would make this run a coin flip, not a check. The 3x limit still
holds a real slow path. The slow CPU and 512 MiB runs keep the live 1.10
ratio.

**Controls, self-test** (`decide::controls`, joined to
`cargo xtask migration self-test`): on synthetic 30-sample records, a 3x CPU
candidate holds with `cpu-regressed`, a 2x RSS candidate holds with
`rss-regressed`, and an equal-cost candidate activates. A harness that
accepts each slow path must fail its control: a unit test runs each control
against an always-`activate` harness and expects it to fail, and the
equal-cost control runs against an always-`keep-python` harness and expects
it to fail. The controls pass a fixed zero noise floor, so they prove the
1.10 ratio whatever floor the registry holds for `rust-03` later.
`tests/decide.rs` runs `migration self-test` and expects 24 controls
(8 paired verdict, 6 trial planner, 3 decide, 7 vector step: 5 compare, a
planted home write and the step registry).

**Measurement:** `cargo test -p careerbrew-xtask --test measure --test decide`
and `cargo test -p careerbrew-xtask --bin careerbrew-xtask decide::controls`
on the hosted gate. **Success:** 3 of 3 end-to-end controls and 3 of 3
self-test controls pass, and the always-accept and always-hold runs fail. **Pivot:** if the
equal-cost end-to-end run shows `cpu-regressed` in 2 or more hosted runs, a
3-sample p95 is too noisy for this check; `decide` now compares each
regime against its own paired oracle samples, so judge it by those.
**Anchor:** Kalibera and Jones 2013 (variation and minimum effect size);
Jia and Harman 2011 (planted faults prove a check can fail).

### Next, in order

`rust-00` is done. Its follow-ups run beside the next steps:

1. `rust-01`: prove maintained dependency and data interoperability.
2. `rust-03` is active ([verdict](#rust-03-verdict)). Next: `rust-04`, and
   the real `rust-03` noise floors from hosted baselines (task
   `rust-00-measure-noise-floors`).
3. Follow-up that does not block a step: `rust-00-inventory-process-observation`.

## Every module has an owner step

Each file below has exactly one owner step. The owner step ports it, or (for a
data file or a kept TypeScript file) keeps it valid when its step changes the
code around it.

**Count: 955 files mapped of 955 files in scope (100%).** Generated on
2026-11-15 from `git ls-files` at `ecbbb7fc`.

How the owner is chosen (first match wins):

1. **Inventory row with a task** (738 files). The [code inventory](plans/2026-11-01-rust-code-inventory.csv)
   is the reviewed source map, and it wins. Cross-check: all 738 inventory rows
   with a task are tracked files, and no inventory row names a missing file.
2. **Server-facing web files** (11 files). They stay TypeScript. `rust-25` keeps
   the API contract, the dev proxy and the backend launchers in the Playwright
   configs and fixtures working. `rust-26` updates the start-command text in
   `web/src/act.ts` and its tests.
3. **Path rules** (206 files: data, fixtures and build files that the inventory
   does not list):

   | Path | Owner |
   | --- | --- |
   | `docs/plans/` (the kept data files), `crates/vault/`, `experiments/hash/`, `xtask/` (fixtures and manifests) | `rust-00` |
   | `tests/fixtures/http/login.microsoftonline.com/`, `tests/fixtures/plugin-*`, `.env.example` | `rust-07` |
   | `tests/fixtures/http/` (other hosts), `tests/fixtures/calendar/` | `rust-08` |
   | `tests/fixtures/projects/` | `rust-09` |
   | `tests/fixtures/http/feed/`, `tests/fixtures/http/team-feed*/`, `tests/fixtures/feed/`, `tests/fixtures/transcripts/` | `rust-13` |
   | `tests/fixtures/live/`, `tests/fixtures/notifier/` | `rust-14` |
   | `tests/fixtures/pm-promise-outcomes/` | `rust-17` |
   | `tests/fixtures/standup/` | `rust-18` |
   | `tests/fixtures/ats/` | `rust-19` |
   | `tests/fixtures/notion-bundle/`, `tests/fixtures/public-cards/` | `rust-20` |
   | `tests/fixtures/exercises/` | `rust-22` |
   | `tests/fixtures/voice/`, `config/models.yaml` | `rust-23` |
   | `commands/`, `Agentfile.yaml` | `rust-26` |
   | `evals/` (cases, rubric, README), `tests/` (other), `scripts/`, `hooks/`, `.github/workflows/`, `lefthook.yml`, `INSTALL.md`, `tests/fixtures/claims/` | `rust-27` (files only: see [Test files](#test-files-the-file-and-its-assertions-have-different-owners)) |
   | `pyproject.toml`, `uv.lock`, `.python-version`, `src/careerbrew/py.typed` | `rust-28` |

**In scope** (955 files): every tracked file under `src/careerbrew/`,
`scripts/`, `tests/`, `evals/`, `hooks/`, `xtask/`, `crates/`, `experiments/`,
`commands/` and `.github/workflows/` (770); `knowledge/exercises/` (150); the
root build files `Cargo.toml`, `Cargo.lock`, `.cargo/config.toml`,
`rust-toolchain.toml`, `pyproject.toml`, `uv.lock`, `.python-version`,
`lefthook.yml`, `INSTALL.md`, `Agentfile.yaml`, `.env.example` and
`config/models.yaml` (12); the 12 data files in `docs/plans/` that
`config/public-release-exclude.txt` keeps in the public tree with a `!` line
(12); and the 11 server-facing web files.

**Out of scope, no step** (they stay as they are):

- The rest of `web/` (571 files, including the visual-test pictures): the UI
  stays TypeScript. The browser and visual tests are regression coverage for
  every step.
- Declarative data and docs: `skills/`, `prompts/`, `knowledge/` (except
  `exercises/`), `config/` (except `models.yaml`), `profile/`, `docs/`. A step
  that changes a command updates the command lines in them (`rust-26` for
  skills and commands, `rust-27` for inline shell in docs).

**Check the count:**

```sh
git ls-files src/careerbrew scripts tests evals hooks xtask crates experiments commands .github/workflows | wc -l   # 770
git ls-files knowledge/exercises | wc -l                                                                            # 150
git ls-files docs/plans | wc -l                                                                                     # 12
```

A new file in scope needs a row: an inventory row for code (see the
[source-map refresh rule](#source-map-refresh-rule)), or a path rule above for
data. Then regenerate this list.

### The kept data files in `docs/plans/`

The public tree leaves out `docs/plans/`, except these 12 data files. All 12
belong to `rust-00`, because the `rust-00` harness reads each one. A step that
changes the code they describe updates them in the same pull request.

| File | Read by | What it holds |
| --- | --- | --- |
| [code inventory](plans/2026-11-01-rust-code-inventory.csv) and its [fragments](plans/rust-inventory.d/README.md) | `xtask/src/inventory.rs` | path, disposition, owner task and reason for each executable file |
| [activation manifest](plans/2026-11-04-rust-activation-manifest.tsv) | `xtask/tests/activation_manifest.rs` | config, vault, approvals and role call graph rows; one file, not fragments: its rows pin source line numbers and are refreshed in place when code moves, so a fragment would not stop the conflict |
| [activation providers](plans/2026-11-04-rust-activation-providers.tsv) | `xtask/tests/activation_manifest.rs` | changes, strategy, tutor and teacher call graph rows |
| [activation workday](plans/2026-11-04-rust-activation-workday.tsv) | `xtask/tests/activation_manifest.rs` | work-day start, end, clock and watcher call graph rows |
| [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) | `xtask/src/inventory.rs` | each embedded script, with its owner task and source anchor |
| [embedded sites](plans/2026-11-17-rust-embedded-sites.csv) | `xtask/src/sites.rs` | each discovered embedded code line and generated entry point, with its kind, key and owner task |
| [DATA bootstrap schema](plans/2026-11-05-rust-data-bootstrap-schema.json) | `xtask/src/capture_data.rs` | the DATA capture start-up contract |
| [DATA fault schema](plans/2026-11-05-rust-data-fault-schema.json) | `xtask/src/capture_data.rs` | the reached-stage record of the synthetic fault protocol |
| [DATA fixture v1](plans/2026-11-05-rust-data-fixture-v1.json) | `xtask/src/capture_data.rs`, `xtask/tests/capture_data.rs` | the synthetic DATA fixture |
| [DATA phase schema](plans/2026-11-05-rust-data-phase-schema.json) | `xtask/src/capture_data.rs`, `xtask/tests/capture_data.rs` | the DATA capture phases |
| [DATA value schema](plans/2026-11-05-rust-data-value-schema.json) | `xtask/src/capture_data.rs`, `xtask/tests/capture_data.rs` | the DATA value shapes |
| [promise action delta](plans/2026-11-08-promise-action-delta.json) | `xtask/src/capture_actions.rs`, `xtask/tests/support/http_controls.rs` | the expected action change for the promise commands |
| [rules action delta](plans/2026-11-08-rules-action-delta.json) | `xtask/src/capture_actions.rs`, `xtask/tests/support/http_controls.rs` | the expected action change for the rules commands |

### Test files: the file and its assertions have different owners

The inventory assigns every test file to `rust-27`. That row stays. It names
the owner of the **file**: the shared harness that runs it, and the deletion of
the empty Python file at the end. The **assertions** in a test file port with
the capability they check:

- A domain step ports the assertions for its capability into native tests in
  the same pull request as the code. It never defers them to `rust-27`.
- `rust-27` owns only the shared harness (`conftest.py`, `lib.sh`, the fakes,
  fixture servers and runners) and the assertions that no domain step owns.
- First assignment (generated at `ecbbb7fc`, columns in the table below):
  - A unit or safety test goes to the **latest** domain step that owns a
    `careerbrew` module it imports.
  - An e2e story goes to the latest domain step that owns a CLI handler it
    runs (`cb <command>`).
  - "Latest" is the first step at which every module the test touches is
    native, so the test can run against native code there.
  - A test with no domain module stays with `rust-27`.
- The step confirms its list when it starts. A move to another step is a plan
  change on this page.

### CLI handlers: `cli.py` is split across the steps

`cli.py` holds 168 `cmd_*` handlers. Its inventory row names `rust-26`. That
row stays, but it means the **dispatcher**, not the handlers:

- Each domain step ports the CLI handlers of its own capability. A handler goes
  to the latest domain step that owns a module it calls (generated at
  `ecbbb7fc`; column in the table below).
- When the step's [activation closure](#build-order-versus-production-activation)
  is native, the generated Python dispatch (`rust-03`) routes those commands to
  the native binary. The domain step switches its own commands. It does not
  wait for `rust-26`.
- `rust-26` owns only argument parsing, the dispatcher, the action registry
  wiring and the final switch of the default entry point.

### Files per step

| Step | Files it owns | CLI handlers it ports | Test files whose assertions it ports |
| --- | --- | --- | --- |
| [`rust-00`](#rust-00-build-the-baseline-harness-and-complete-code-inventory) | 72 | 0 | 0 |
| [`rust-01`](#rust-01-prove-maintained-dependency-and-data-interoperability) | 0 | 0 | 0 |
| [`rust-02`](#rust-02-port-pure-setting-schemas-paths-clocks-and-hashes) | 3 | 1 | 4 |
| [`rust-03`](#rust-03-introduce-the-native-cli-and-one-read-only-pilot) | 2 | 1 | 1 |
| [`rust-04`](#rust-04-port-privacy-policy-and-approval-enforcement) | 3 | 8 | 4 |
| [`rust-05`](#rust-05-port-encrypted-storage-and-transaction-locks) | 5 | 2 | 1 |
| [`rust-06`](#rust-06-port-backup-restore-and-cross-machine-sync) | 2 | 7 | 6 |
| [`rust-07`](#rust-07-port-plugin-machine-and-account-setup) | 22 | 11 | 9 |
| [`rust-08`](#rust-08-port-http-sources-calendar-and-mail) | 58 | 1 | 5 |
| [`rust-09`](#rust-09-port-projects-priorities-and-promises) | 9 | 4 | 2 |
| [`rust-10`](#rust-10-port-sessions-claims-agents-and-reply-drafts) | 4 | 6 | 5 |
| [`rust-11`](#rust-11-port-project-imports-exact-source-links-and-memory-sync) | 3 | 0 | 6 |
| [`rust-12`](#rust-12-port-project-learning-replay-tours-and-explainers) | 6 | 14 | 5 |
| [`rust-13`](#rust-13-port-feed-ingestion-and-transcript-normalization) | 23 | 2 | 7 |
| [`rust-14`](#rust-14-port-live-listening-review-asks-and-notifications) | 11 | 3 | 5 |
| [`rust-15`](#rust-15-port-canonical-work-day-records-and-questions) | 5 | 1 | 2 |
| [`rust-16`](#rust-16-port-work-day-execution-scheduling-and-watch-loops) | 7 | 7 | 14 |
| [`rust-17`](#rust-17-port-daily-planning-results-and-pm-learning) | 12 | 14 | 25 |
| [`rust-18`](#rust-18-port-evidence-promotion-standup-and-work-profiles) | 7 | 9 | 24 |
| [`rust-19`](#rust-19-port-job-search-interview-prep-and-public-research) | 13 | 18 | 17 |
| [`rust-20`](#rust-20-port-flashcards-fsrs-and-notion-import) | 12 | 11 | 7 |
| [`rust-21`](#rust-21-port-teacher-goals-practice-and-mistake-tracking) | 5 | 10 | 9 |
| [`rust-22`](#rust-22-port-exercise-execution-and-complexity-checks) | 152 | 4 | 8 |
| [`rust-23`](#rust-23-port-local-voice-model-routes-and-judge-adapters) | 6 | 1 | 9 |
| [`rust-24`](#rust-24-port-reports-metrics-checkups-and-remaining-domain-helpers) | 16 | 29 | 91 |
| [`rust-25`](#rust-25-replace-the-http-server-behind-the-unchanged-ui) | 9 | 4 | 40 |
| [`rust-26`](#rust-26-complete-native-cli-coverage-and-default-entry-point) | 12 | 0 | 0 |
| [`rust-27`](#rust-27-port-developer-tools-test-harnesses-installers-and-release-checks) | 477 | 0 | 68 |
| [`rust-28`](#rust-28-retire-the-old-implementation-and-verify-fresh-machine-releases) | 4 | 0 | 0 |
| **All steps** | **960 of 960** | **168 of 168** | **374 of 374** |

#### rust-00: Build the baseline harness and complete code inventory (72 files)

- `.cargo/` (1): `config.toml`
- `./` (3): `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`
- `crates/vault/` (1): `Cargo.toml`
- `crates/vault/src/` (2): `lib.rs`, `main.rs`
- `docs/plans/` (13): `2026-11-01-rust-code-inventory.csv`, `2026-11-04-rust-activation-manifest.tsv`, `2026-11-04-rust-activation-providers.tsv`, `2026-11-04-rust-activation-workday.tsv`, `2026-11-04-rust-embedded-anchors.csv`, `2026-11-05-rust-data-bootstrap-schema.json`, `2026-11-05-rust-data-fault-schema.json`, `2026-11-05-rust-data-fixture-v1.json`, `2026-11-05-rust-data-phase-schema.json`, `2026-11-05-rust-data-value-schema.json`, `2026-11-08-promise-action-delta.json`, `2026-11-08-rules-action-delta.json`, `2026-11-17-rust-embedded-sites.csv`
- `experiments/hash/` (1): `Cargo.toml`
- `experiments/hash/src/` (1): `main.rs`
- `xtask/` (1): `Cargo.toml`
- `xtask/src/` (10): `baseline_verdict.rs`, `capture_actions.rs`, `capture_data.rs`, `capture_http.rs`, `inventory.rs`, `main.rs`, `observer_probe.rs`, `resources.rs`, `sites.rs`, `sites_tests.rs`, `toolchain_identity.rs`, `trial_plan.rs`
- `xtask/src/bin/` (2): `resource_fixture.rs`, `store_supervisor_fixture.rs`
- `xtask/src/observer_probe/` (3): `ownership.rs`, `parser.rs`, `transport.rs`
- `xtask/tests/` (12): `activation_manifest.rs`, `capture_actions.rs`, `capture_data.rs`, `capture_data_cli.rs`, `capture_http.rs`, `i01_fixture.rs`, `observer_deadline.rs`, `observer_owner.rs`, `observer_parser.rs`, `observer_transport.rs`, `resources.rs`, `store_supervisor.rs`
- `xtask/tests/fixtures/capture/` (12): `actions-reference-promise-v2.json`, `actions-reference-rules-promise-v3.json`, `actions-reference-rules-v2.json`, `actions-reference-v1.json`, `actions-v1.json`, `data_reference_v1.provenance.json`, `data_reference_v1.semantic.json`, `http-reference-promise-v2.json`, `http-reference-rules-promise-v3.json`, `http-reference-rules-v2.json`, `http-reference-v1.json`, `http-v1.json`
- `xtask/tests/fixtures/i01/` (1): `pending-identities-v1.tsv`
- `xtask/tests/fixtures/linux-observer-replay-v7/` (5): `README.md`, `manifest.json`, `status.json`, `trace.10`, `trace.11`
- `xtask/tests/fixtures/linux-observer/` (1): `Dockerfile`
- `xtask/tests/support/` (4): `actions_controls.rs`, `http_controls.rs`, `owner_controls.rs`, `store_supervisor.rs`

#### rust-01: Prove maintained dependency and data interoperability (0 files)

- No file of its own. It reads the files of other steps (see its **Inputs**).

#### rust-02: Port pure setting schemas, paths, clocks and hashes (3 files)

- `src/careerbrew/` (3): `__init__.py`, `config.py`, `paths.py`
- Also reads, without owning: `changes.py` (`_h` only; owned by `rust-24`) and
  `calendar.py` (the pure clock helpers only; owned by `rust-08`)
- CLI handlers it ports out of `cli.py` (1): `cmd_config`
- Assertions it ports (4 test files, the files themselves are listed under `rust-27`): `us-64-fast-views.sh`, `test_contributing.py`, `test_eval_case_loading.py`, `test_issue_335_337.py`

#### rust-03: Introduce the native CLI and one read-only pilot (2 files)

- `src/careerbrew/` (2): `actions.py`, `agent_workflows.py`
- CLI handlers it ports out of `cli.py` (1): `cmd_actions`
- Native crate it adds (`keep-native` in the inventory): `crates/cbcli/src/` (`lib.rs`, `main.rs`, `registry.rs`, `render.rs`, `help.rs`, `events.rs`) and `crates/cbcli/tests/cli.rs`.
- Generated files it adds (not code, so not in the inventory): `crates/cbcli/registry/registry.json`, `config/native-routes.txt`. Its generator `scripts/gen-registry.py` is owned by `rust-26`, which retires it. The registry is a capture source input (`capture_actions::sources()`), so capture, baseline and measure records hash it; the source-copy probe count moved from 139 to 140 (and to 142 when `rust-05a` added `keyfile.py` and `config_io.py`).
- Assertions it ports (1 test file, the files themselves are listed under `rust-27`): `test_every_action_has_an_e2e.py`

#### rust-04: Port privacy, policy and approval enforcement (3 files)

- `src/careerbrew/` (2): `approvals.py`, `privacy.py`
- `src/careerbrew/work/` (1): `access.py`
- CLI handlers it ports out of `cli.py` (8): `cmd_approvals_decide`, `cmd_approvals_guidance`, `cmd_approvals_list`, `cmd_approvals_request`, `cmd_approvals_show`, `cmd_draft_judge`, `cmd_draft_route`, `cmd_privacy_scan`
- Assertions it ports (4 test files, the files themselves are listed under `rust-27`): `test_issue_314.py`, `test_privacy_index.py`, `test_privacy_scan_file.py`, `test_public_scrub.py`

#### rust-05: Port encrypted storage and transaction locks (5 files)

- `src/careerbrew/` (5): `config_io.py`, `keyfile.py`, `secrets.py`, `store.py`, `vault.py`
- CLI handlers it ports out of `cli.py` (3): `cmd_data_edit`, `cmd_data_show`, `cmd_secret_set`
- Assertions it ports (1 test file, the files themselves are listed under `rust-27`): `test_issue_366.py`

#### rust-06: Port backup, restore and cross-machine sync (2 files)

- `src/careerbrew/` (2): `backup.py`, `sync.py`
- CLI handlers it ports out of `cli.py` (7): `cmd_backup_create`, `cmd_backup_restore`, `cmd_backup_verify`, `cmd_data_status`, `cmd_pair_join`, `cmd_sync_export`, `cmd_sync_import`
- Assertions it ports (6 test files, the files themselves are listed under `rust-27`): `us-08-backup.sh`, `test_issue_423_bundles.py`, `test_issue_423_keys.py`, `test_issue_423_missing_key.py`, `test_rust_vault_base64_parity.py`, `test_rust_vault_interop.py`

#### rust-07: Port plugin, machine and account setup (22 files)

- `./` (1): `.env.example`
- `src/careerbrew/` (3): `machine.py`, `msauth.py`, `plugins.py`
- `src/careerbrew/workday/` (3): `mcp_check.py`, `self_update.py`, `workflow_check.py`
- `tests/fixtures/http/login.microsoftonline.com/` (5): `organizations_oauth2_v2.0_devicecode.json`, `organizations_oauth2_v2.0_token.json`, `synthetic-declined-tenant_oauth2_v2.0_devicecode.json`, `synthetic-declined-tenant_oauth2_v2.0_token.json`, `synthetic-expired-tenant_oauth2_v2.0_token.json`
- `tests/fixtures/plugin-acme-platform/` (2): `careerbrew-plugin.yaml`, `sensitive-terms.txt`
- `tests/fixtures/plugin-acme-platform/ladders/` (1): `acme-platform-eng.yaml`
- `tests/fixtures/plugin-acme/` (5): `careerbrew-plugin.yaml`, `evidence.yaml`, `job-sources.yaml`, `rules.md`, `sensitive-terms.txt`
- `tests/fixtures/plugin-acme/knowledge/` (1): `acme-items.yaml`
- `tests/fixtures/plugin-acme/ladders/` (1): `acme-eng.yaml`
- CLI handlers it ports out of `cli.py` (11): `cmd_connectors_list`, `cmd_init`, `cmd_machine_set`, `cmd_machine_show`, `cmd_machine_split_plugin_evidence`, `cmd_pair_init`, `cmd_plugin_add`, `cmd_plugin_list`, `cmd_plugin_refresh`, `cmd_plugin_remove`, `cmd_plugin_verify`
- Assertions it ports (9 test files, the files themselves are listed under `rust-27`): `test_data_show.py`, `test_gate_corporate.py`, `test_issue_183.py`, `test_issue_422.py`, `test_issue_423_metadata_temp.py`, `test_mcp_fix.py`, `test_plugin_commands.py`, `test_second_user.py`, `test_vault_plugins.py`
- Native crate it adds (`keep-native` in the inventory): `crates/setup/` (`plugins.rs`, `machine.rs`, `oauth.rs`, `preflight.rs`, `cli.rs`, `synthetic.rs`, `value.rs`, `lib.rs` and `bin/setup-component.rs`), plus `tests/oracles/rust07.py`, `tests/unit/test_rust07_component.py`, `xtask/src/rust07.rs` and `xtask/tests/rust07.rs`.

#### rust-08: Port HTTP sources, calendar and mail (58 files)

- `src/careerbrew/` (3): `calendar.py`, `connectors.py`, `mail.py`
- `tests/fixtures/calendar/` (1): `work.ics`
- `tests/fixtures/http/api.adzuna.com/` (1): `v1_api_jobs_gb_search_1.json`
- `tests/fixtures/http/api.atlassian.com/` (2): `ex_jira_synthetic-cloud-1_rest_api_3_myself.json`, `ex_jira_synthetic-cloud-2_rest_api_3_myself.json`
- `tests/fixtures/http/api.fastmail.com/` (2): `jmap_api.json`, `jmap_session.json`
- `tests/fixtures/http/api.github.com/` (22): `repos_acme_billing_pulls.json`, `repos_acme_billing_pulls_41.json`, `repos_example_app_pulls_1_reviews.json`, `repos_example_shop_branches_main_protection.json`, `repos_example_shop_contents_.github_CODEOWNERS.json`, `repos_example_shop_pulls.json`, `repos_example_shop_pulls_5_files.json`, `repos_example_shop_pulls_5_reviews.json`, `repos_example_shop_pulls_7.json`, `repos_example_shop_pulls_7_files.json`, `repos_example_shop_rules_branches_main.json`, `repos_example_tools_branches_main_protection.json`, `repos_example_tools_contents_.github_CODEOWNERS.json`, `repos_example_tools_contents_CODEOWNERS.json`, `repos_example_tools_pulls.json`, `repos_example_tools_pulls_2_files.json`, `repos_example_tools_pulls_2_reviews.json`, `repos_example_tools_pulls_3.json`, `repos_example_tools_pulls_3_files.json`, `repos_example_tools_rules_branches_main.json`, `search_issues.json`, `user.json`
- `tests/fixtures/http/api.zoom.us/` (1): `v2_users_me_meetings.json`
- `tests/fixtures/http/bad.atlassian.example.com/` (2): `_edge_tenant_info.json`, `rest_api_3_myself.json`
- `tests/fixtures/http/careers.example.com/` (1): `interview-process.html`
- `tests/fixtures/http/docs.google.com/` (2): `spreadsheets_d_sheet-cutline-q4_export.csv`, `spreadsheets_d_sheet-private_export.html`
- `tests/fixtures/http/graph.microsoft.com/` (3): `v1.0_me.json`, `v1.0_me_calendarView.json`, `v1.0_me_messages.json`
- `tests/fixtures/http/himalayas.app/` (1): `jobs_api_search.json`
- `tests/fixtures/http/hn.algolia.com/` (2): `api_v1_items_49922569.json`, `api_v1_search_by_date.json`
- `tests/fixtures/http/jira.example.com/` (3): `rest_api_3_issue_PROJ-7.json`, `rest_api_3_myself.json`, `rest_api_3_search_jql.json`
- `tests/fixtures/http/leetcode.com/` (1): `graphql.json`
- `tests/fixtures/http/remoteok.com/` (1): `api.json`
- `tests/fixtures/http/remotive.com/` (1): `api_remote-jobs.json`
- `tests/fixtures/http/scoped.atlassian.example.com/` (2): `_edge_tenant_info.json`, `rest_api_3_myself.json`
- `tests/fixtures/http/slack.com/` (2): `api_chat.postMessage.json`, `api_search.messages.json`
- `tests/fixtures/http/status.example.com/` (1): `budget.html`
- `tests/fixtures/http/weworkremotely.com/` (1): `categories_remote-programming-jobs.rss.xml`
- `tests/fixtures/http/www.googleapis.com/` (3): `drive_v3_files.json`, `drive_v3_files_doc-prd-1.json`, `drive_v3_files_doc-prd-1_comments.json`
- CLI handlers it ports out of `cli.py` (1): `cmd_connect`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-42-connect.sh`, `test_connect_wizard.py`, `test_issue_199.py`, `test_jira_connector.py`, `test_outlook_connector.py`
- Native crate it adds (`keep-native` in the inventory): `crates/sources/` (`http.rs`, `connectors.rs`, `calendar.rs`, `mail.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/sources-component.rs`), plus `tests/oracles/rust08.py`, `tests/unit/test_rust08_component.py`, `xtask/src/rust08.rs` and `xtask/tests/rust08.rs`.

#### rust-09: Port projects, priorities and promises (9 files)

- `src/careerbrew/work/` (7): `__init__.py`, `focus.py`, `priorities.py`, `project_priority.py`, `projects.py`, `promises.py`, `team.py`
- `tests/fixtures/projects/` (2): `cutline.json`, `team-goals.csv`
- CLI handlers it ports out of `cli.py` (4): `cmd_work_done`, `cmd_work_impact`, `cmd_work_priority`, `cmd_work_task_set`
- Assertions it ports (2 test files, the files themselves are listed under `rust-27`): `test_issue_230.py`, `test_issue_429.py`
- Native crate it adds (`keep-native` in the inventory): `crates/projects/` (`projects.rs`, `priorities.rs`, `project_priority.rs`, `promises.rs`, `focus.rs`, `team.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/projects-component.rs`), plus `tests/oracles/rust09.py`, `tests/unit/test_rust09_component.py`, `xtask/src/rust09.rs` and `xtask/tests/rust09.rs`.

#### rust-10: Port sessions, claims, agents and reply drafts (4 files)

- `src/careerbrew/work/` (4): `agents.py`, `drafts.py`, `prior_selection.py`, `sessions.py`
- CLI handlers it ports out of `cli.py` (6): `cmd_draft_check`, `cmd_draft_fit`, `cmd_draft_replace`, `cmd_draft_show`, `cmd_work_agent`, `cmd_work_claim`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-91-agent-activity.sh`, `us-98-reply-drafts.sh`, `test_issue_357_session_claim.py`, `test_issue_401.py`, `test_prior_session_selection.py`
- Native crate it adds (`keep-native` in the inventory): `crates/sessions/` (`sessions.rs`, `agents.rs`, `drafts.rs`, `prior_selection.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/sessions-component.rs`), plus `tests/oracles/rust10.py`, `tests/unit/test_rust10_component.py`, `xtask/src/rust10.rs` and `xtask/tests/rust10.rs`.

#### rust-11: Port project imports, exact source links and memory sync (3 files)

- `src/careerbrew/work/` (3): `memory_sync.py`, `project_import.py`, `research.py`
- Assertions it ports (6 test files, the files themselves are listed under `rust-27`): `test_issue_184.py`, `test_issue_309.py`, `test_memory_sync.py`, `test_project_import.py`, `test_project_research.py`, `test_project_since.py`
- Native crate it adds (`keep-native` in the inventory): `crates/memory/` (`research.rs`, `project_import.rs`, `memory_sync.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/memory-component.rs`), plus `tests/oracles/rust11.py`, `tests/unit/test_rust11_component.py`, `xtask/src/rust11.rs` and `xtask/tests/rust11.rs`.

#### rust-12: Port project learning, replay, tours and explainers (6 files)

- `src/careerbrew/learn/` (4): `__init__.py`, `project.py`, `replay.py`, `tour.py`
- `src/careerbrew/work/` (2): `explain.py`, `prd.py`
- CLI handlers it ports out of `cli.py` (14): `cmd_learn_answer`, `cmd_learn_concepts`, `cmd_learn_list`, `cmd_learn_next`, `cmd_learn_note`, `cmd_learn_replay_check`, `cmd_learn_replay_list`, `cmd_learn_replay_measure`, `cmd_learn_replay_start`, `cmd_learn_start`, `cmd_learn_status`, `cmd_learn_tour_add`, `cmd_learn_tour_approve`, `cmd_learn_tour_export`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-80-learn-project.sh`, `us-83-learn-replay-tour.sh`, `test_issue_223.py`, `test_learn_replay_tour.py`, `test_project_explain.py`
- Native crate it adds (`keep-native` in the inventory): `crates/learn/` (`project.rs`, `cards.rs`, `replay.rs`, `tour.rs`, `explain.rs`, `prd.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/learn-component.rs`), plus `crates/learn/tests/component.rs`, `tests/oracles/rust12.py`, `tests/unit/test_rust12_component.py`, `xtask/src/rust12.rs` and `xtask/tests/rust12.rs`.

#### rust-13: Port feed ingestion and transcript normalization (23 files)

- `src/careerbrew/work/` (2): `feed.py`, `transcript.py`
- `tests/fixtures/feed/` (2): `next-step-ask.jsonl`, `web-asks.jsonl`
- `tests/fixtures/http/feed/api.pagerduty.com/` (3): `incidents.json`, `users_PME1_log_entries.json`, `users_me.json`
- `tests/fixtures/http/feed/graph.microsoft.com/` (3): `v1.0_me.json`, `v1.0_me_calendarView.json`, `v1.0_me_messages.json`
- `tests/fixtures/http/feed/jira.example.com/` (2): `rest_api_3_myself.json`, `rest_api_3_search_jql.json`
- `tests/fixtures/http/feed/slack.com/` (3): `api_auth.test.json`, `api_conversations.replies.json`, `api_search.messages.json`
- `tests/fixtures/http/team-feed-noscope/slack.com/` (3): `api_auth.test.json`, `api_conversations.replies.json`, `api_search.messages.json`
- `tests/fixtures/http/team-feed/slack.com/` (3): `api_auth.test.json`, `api_conversations.replies.json`, `api_search.messages.json`
- `tests/fixtures/transcripts/` (1): `feed-cycle.jsonl`
- `tests/fixtures/transcripts/feed-cycle/tool-results/` (1): `big.txt`
- CLI handlers it ports out of `cli.py` (2): `cmd_work_feed`, `cmd_work_team`
- Assertions it ports (7 test files, the files themselves are listed under `rust-27`): `us-63-today-open-asks.sh`, `us-78-team-feed.sh`, `test_issue_180.py`, `test_issue_228.py`, `test_issue_234.py`, `test_issue_316.py`, `test_issue_374_legacy_feed.py`
- Native crate it adds (`keep-native` in the inventory): `crates/feed/` (`feed.rs`, `sources.rs`, `transcript.rs`, `cli.rs`, `synthetic.rs`, `lib.rs` and `bin/feed-component.rs`), plus `crates/feed/tests/component.rs`, `tests/oracles/rust13.py`, `tests/unit/test_rust13_component.py`, `xtask/src/rust13.rs` and `xtask/tests/rust13.rs`.

#### rust-14: Port live listening, review asks and notifications (11 files)

- `src/careerbrew/` (1): `notify.py`
- `src/careerbrew/work/` (4): `health.py`, `listen.py`, `live.py`, `review_ask.py`
- `tests/fixtures/live/` (4): `chat.json`, `mail.json`, `sources.yaml`, `tickets.json`
- `tests/fixtures/live/listen/` (2): `chat.json`, `sources.yaml`
- CLI handlers it ports out of `cli.py` (3): `cmd_notify`, `cmd_work_live`, `cmd_work_review_ask`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-90-review-ask.sh`, `us-97-live-feed.sh`, `test_issue_267.py`, `test_issue_270.py`, `test_issue_313.py`

#### rust-15: Port canonical work-day records and questions (5 files)

- `src/careerbrew/workday/` (5): `__init__.py`, `questions.py`, `stages.py`, `status.py`, `steps.py`
- CLI handlers it ports out of `cli.py` (1): `cmd_work_session`
- Assertions it ports (2 test files, the files themselves are listed under `rust-27`): `test_issue_311.py`, `test_issue_355_owner_steps.py`

#### rust-16: Port work-day execution, scheduling and watch loops (7 files)

- `src/careerbrew/workday/` (7): `cron_check.py`, `end.py`, `gate.py`, `plan.py`, `start.py`, `timing.py`, `watch.py`
- CLI handlers it ports out of `cli.py` (7): `cmd_day_check`, `cmd_day_end`, `cmd_day_history`, `cmd_day_loop`, `cmd_day_question`, `cmd_day_start`, `cmd_day_wrap`
- Assertions it ports (14 test files, the files themselves are listed under `rust-27`): `us-103-work-day-watch.sh`, `test_issue_258.py`, `test_issue_305.py`, `test_issue_308.py`, `test_issue_310.py`, `test_issue_312.py`, `test_issue_330.py`, `test_issue_333.py`, `test_issue_339.py`, `test_issue_356.py`, `test_issue_395_skill.py`, `test_issue_408.py`, `test_issue_426.py`, `test_issue_437.py`

#### rust-17: Port daily planning, results and PM learning (12 files)

- `src/careerbrew/day/` (10): `__init__.py`, `done_elsewhere.py`, `handoff.py`, `inbox.py`, `log.py`, `planner.py`, `pm_learn.py`, `pm_promises.py`, `results.py`, `watch.py`
- `tests/fixtures/pm-promise-outcomes/` (2): `histories.json`, `skipped-workday.json`
- CLI handlers it ports out of `cli.py` (14): `cmd_day_decide`, `cmd_day_group`, `cmd_day_handoff`, `cmd_day_inbox`, `cmd_day_log`, `cmd_day_next`, `cmd_day_pm_sent`, `cmd_day_results`, `cmd_day_run`, `cmd_day_suggest`, `cmd_day_suggestions`, `cmd_day_watch`, `cmd_work_promise`, `cmd_work_task`
- Assertions it ports (25 test files, the files themselves are listed under `rust-27`): `us-110-pm-promise-outcomes.sh`, `us-24-day-next.sh`, `us-62-notifications.sh`, `us-70-task-links.sh`, `us-82-active-now.sh`, `us-85-handoff.sh`, `test_import_memory_questions.py`, `test_issue_145.py`, `test_issue_210.py`, `test_issue_226.py`, `test_issue_239.py`, `test_issue_264.py`, `test_issue_334.py`, `test_issue_372.py`, `test_issue_389_baseline.py`, `test_issue_389_outcomes.py`, `test_issue_389_resolution.py`, `test_issue_60.py`, `test_issue_68.py`, `test_issue_71.py`, `test_issues_43_46.py`, `test_pm_history_concurrency.py`, `test_pm_sent_coverage.py`, `test_state_concurrency.py`, `test_watch_probe.py`

#### rust-18: Port evidence, promotion, standup and work profiles (7 files)

- `src/careerbrew/work/` (5): `evidence.py`, `ladder_grade.py`, `linkedin.py`, `promo.py`, `standup.py`
- `tests/fixtures/standup/` (2): `extra.json`, `proof.json`
- CLI handlers it ports out of `cli.py` (9): `cmd_day_pm_grade`, `cmd_ladder_import`, `cmd_ladder_levels`, `cmd_ladder_list`, `cmd_work_gaps`, `cmd_work_linkedin`, `cmd_work_note`, `cmd_work_plan`, `cmd_work_standup`
- Assertions it ports (24 test files, the files themselves are listed under `rust-27`): `us-10-plan.sh`, `us-23-day-watch.sh`, `us-28-ladder-import.sh`, `us-30-hot-reload.sh`, `us-95-standup-proofs.sh`, `test_connectors_machines.py`, `test_focus.py`, `test_hot_reload.py`, `test_issue_200.py`, `test_issue_214.py`, `test_issue_214_why.py`, `test_issue_216.py`, `test_issue_307.py`, `test_issue_315.py`, `test_issue_331.py`, `test_issue_336.py`, `test_issue_338.py`, `test_issue_52.py`, `test_issue_72.py`, `test_issue_73.py`, `test_linkedin.py`, `test_pm_sent_reconciliation.py`, `test_promo_matching.py`, `test_split_plugin_evidence.py`

#### rust-19: Port job search, interview prep and public research (13 files)

- `src/careerbrew/` (3): `interview.py`, `prep.py`, `research.py`
- `src/careerbrew/jobs/` (7): `__init__.py`, `criteria.py`, `linkedin.py`, `platforms.py`, `sources.py`, `strategy.py`, `want.py`
- `tests/fixtures/ats/` (3): `ashby-fabrikam.json`, `greenhouse-northwind.json`, `lever-contoso.json`
- CLI handlers it ports out of `cli.py` (18): `cmd_interview`, `cmd_interview_patterns`, `cmd_jobs_choose`, `cmd_jobs_interview_gaps`, `cmd_jobs_learn`, `cmd_jobs_linkedin_import`, `cmd_jobs_list`, `cmd_jobs_log`, `cmd_jobs_preset`, `cmd_jobs_rate`, `cmd_jobs_scan`, `cmd_jobs_strategy`, `cmd_jobs_unchoose`, `cmd_mail_scan`, `cmd_prep_check`, `cmd_prep_story`, `cmd_research_page`, `cmd_research_stale`
- Assertions it ports (17 test files, the files themselves are listed under `rust-27`): `us-02-jobs-scan-filter.sh`, `us-03-want-rate-learn.sh`, `us-15-machines-sync.sh`, `us-17-interview.sh`, `us-19-job-platforms.sh`, `us-20-linkedin-import.sh`, `us-21-mail.sh`, `us-26-sources.sh`, `us-32-research.sh`, `us-33-patterns.sh`, `us-48-job-choice.sh`, `test_interview_prep.py`, `test_issue_185_gaps.py`, `test_mail_linkedin.py`, `test_platforms.py`, `test_region_labels.py`, `test_watch_mail_plan.py`

#### rust-20: Port flashcards, FSRS and Notion import (12 files)

- `src/careerbrew/` (1): `notion_import.py`
- `src/careerbrew/tutor/` (2): `cards.py`, `public.py`
- `tests/fixtures/notion-bundle/` (8): `flashcards.json`, `jobs.json`, `manifest.json`, `pages.json`, `past_job.json`, `plan.json`, `skills.json`, `stories.json`
- `tests/fixtures/public-cards/` (1): `out.yaml`
- CLI handlers it ports out of `cli.py` (11): `cmd_cards_add`, `cmd_cards_explain_queue`, `cmd_cards_grade`, `cmd_cards_next`, `cmd_cards_public_check`, `cmd_cards_review`, `cmd_cards_stats`, `cmd_cards_unsuspend`, `cmd_import_notion`, `cmd_library_list`, `cmd_library_show`
- Assertions it ports (7 test files, the files themselves are listed under `rust-27`): `us-38-notion-import.sh`, `us-39-flashcards.sh`, `test_cards.py`, `test_learn.py`, `test_public_cards.py`, `test_regrade_notion.py`, `test_vault.py`

#### rust-21: Port teacher, goals, practice and mistake tracking (5 files)

- `src/careerbrew/` (2): `goals.py`, `teacher.py`
- `src/careerbrew/tutor/` (3): `__init__.py`, `engine.py`, `mistakes.py`
- CLI handlers it ports out of `cli.py` (10): `cmd_drill_answer`, `cmd_drill_list`, `cmd_drill_next`, `cmd_drill_session`, `cmd_goals_long_term`, `cmd_goals_set`, `cmd_goals_show`, `cmd_teach_feedback`, `cmd_teach_improvements`, `cmd_teach_next`
- Assertions it ports (9 test files, the files themselves are listed under `rust-27`): `us-04-strategy.sh`, `us-05-drill.sh`, `us-18-prep.sh`, `us-45-teacher.sh`, `test_goals.py`, `test_issue_266.py`, `test_issue_276.py`, `test_issue_58.py`, `test_jobs.py`

#### rust-22: Port exercise execution and complexity checks (152 files)

- `knowledge/exercises/` (150): `001-contains-duplicate.yaml`, `002-valid-anagram.yaml`, `003-two-sum.yaml`, `004-group-anagrams.yaml`, `005-top-k-frequent-elements.yaml`, `006-encode-and-decode-strings.yaml`, `007-product-of-array-except-self.yaml`, `008-valid-sudoku.yaml`, `009-longest-consecutive-sequence.yaml`, `010-valid-palindrome.yaml`, `011-two-sum-ii-input-array-is-sorted.yaml`, `012-3sum.yaml`, `013-container-with-most-water.yaml`, `014-trapping-rain-water.yaml`, `015-best-time-to-buy-and-sell-stock.yaml`, `016-longest-substring-without-repeating-characters.yaml`, `017-longest-repeating-character-replacement.yaml`, `018-permutation-in-string.yaml`, `019-minimum-window-substring.yaml`, `020-sliding-window-maximum.yaml`, `021-valid-parentheses.yaml`, `022-min-stack.yaml`, `023-evaluate-reverse-polish-notation.yaml`, `024-generate-parentheses.yaml`, `025-daily-temperatures.yaml`, `026-car-fleet.yaml`, `027-largest-rectangle-in-histogram.yaml`, `028-binary-search.yaml`, `029-search-a-2d-matrix.yaml`, `030-koko-eating-bananas.yaml`, `031-find-minimum-in-rotated-sorted-array.yaml`, `032-search-in-rotated-sorted-array.yaml`, `033-time-based-key-value-store.yaml`, `034-median-of-two-sorted-arrays.yaml`, `035-reverse-linked-list.yaml`, `036-merge-two-sorted-lists.yaml`, `037-reorder-list.yaml`, `038-remove-nth-node-from-end-of-list.yaml`, `039-copy-list-with-random-pointer.yaml`, `040-add-two-numbers.yaml`, `041-linked-list-cycle.yaml`, `042-find-the-duplicate-number.yaml`, `043-lru-cache.yaml`, `044-merge-k-sorted-lists.yaml`, `045-reverse-nodes-in-k-group.yaml`, `046-invert-binary-tree.yaml`, `047-maximum-depth-of-binary-tree.yaml`, `048-diameter-of-binary-tree.yaml`, `049-balanced-binary-tree.yaml`, `050-same-tree.yaml`, `051-subtree-of-another-tree.yaml`, `052-lowest-common-ancestor-of-a-binary-search-tree.yaml`, `053-binary-tree-level-order-traversal.yaml`, `054-binary-tree-right-side-view.yaml`, `055-count-good-nodes-in-binary-tree.yaml`, `056-validate-binary-search-tree.yaml`, `057-kth-smallest-element-in-a-bst.yaml`, `058-construct-binary-tree-from-preorder-and-inorder-traversal.yaml`, `059-binary-tree-maximum-path-sum.yaml`, `060-serialize-and-deserialize-binary-tree.yaml`, `061-implement-trie-prefix-tree.yaml`, `062-design-add-and-search-words-data-structure.yaml`, `063-word-search-ii.yaml`, `064-kth-largest-in-stream.yaml`, `065-last-stone-weight.yaml`, `066-k-closest-points.yaml`, `067-kth-largest-in-array.yaml`, `068-task-scheduler.yaml`, `069-design-twitter.yaml`, `070-median-of-stream.yaml`, `071-subsets.yaml`, `072-combination-sum.yaml`, `073-permutations.yaml`, `074-subsets-with-duplicates.yaml`, `075-combination-sum-once.yaml`, `076-word-search.yaml`, `077-palindrome-partitioning.yaml`, `078-phone-letter-combinations.yaml`, `079-n-queens.yaml`, `080-number-of-islands.yaml`, `081-clone-graph.yaml`, `082-max-area-of-island.yaml`, `083-pacific-atlantic.yaml`, `084-surrounded-regions.yaml`, `085-rotting-oranges.yaml`, `086-walls-and-gates.yaml`, `087-course-schedule.yaml`, `088-course-order.yaml`, `089-redundant-connection.yaml`, `090-connected-components.yaml`, `091-graph-valid-tree.yaml`, `092-word-ladder.yaml`, `093-reconstruct-itinerary.yaml`, `094-min-cost-connect-points.yaml`, `095-network-delay-time.yaml`, `096-swim-in-rising-water.yaml`, `097-alien-dictionary.yaml`, `098-cheapest-flights-k-stops.yaml`, `099-climbing-stairs.yaml`, `100-min-cost-climbing-stairs.yaml`, `101-house-robber.yaml`, `102-house-robber-ii.yaml`, `103-longest-palindromic-substring.yaml`, `104-palindromic-substrings.yaml`, `105-decode-ways.yaml`, `106-coin-change.yaml`, `107-maximum-product-subarray.yaml`, `108-word-break.yaml`, `109-longest-increasing-subsequence.yaml`, `110-partition-equal-subset-sum.yaml`, `111-unique-paths.yaml`, `112-longest-common-subsequence.yaml`, `113-stock-cooldown.yaml`, `114-coin-change-ii.yaml`, `115-target-sum.yaml`, `116-interleaving-string.yaml`, `117-longest-increasing-path-matrix.yaml`, `118-distinct-subsequences.yaml`, `119-edit-distance.yaml`, `120-burst-balloons.yaml`, `121-regex-matching.yaml`, `122-maximum-subarray.yaml`, `123-jump-game.yaml`, `124-jump-game-ii.yaml`, `125-gas-station.yaml`, `126-hand-of-straights.yaml`, `127-merge-triplets-to-form-target-triplet.yaml`, `128-partition-labels.yaml`, `129-valid-parenthesis-string.yaml`, `130-insert-interval.yaml`, `131-merge-intervals.yaml`, `132-non-overlapping-intervals.yaml`, `133-meeting-rooms.yaml`, `134-meeting-rooms-ii.yaml`, `135-minimum-interval-to-include-each-query.yaml`, `136-rotate-image.yaml`, `137-spiral-matrix.yaml`, `138-set-matrix-zeroes.yaml`, `139-happy-number.yaml`, `140-plus-one.yaml`, `141-powx-n.yaml`, `142-multiply-strings.yaml`, `143-detect-squares.yaml`, `144-single-number.yaml`, `145-number-of-1-bits.yaml`, `146-counting-bits.yaml`, `147-reverse-bits.yaml`, `148-missing-number.yaml`, `149-sum-of-two-integers.yaml`, `150-reverse-integer.yaml`
- `src/careerbrew/tutor/` (1): `exercises.py`
- `tests/fixtures/exercises/` (1): `001-contains-duplicate.yaml`
- CLI handlers it ports out of `cli.py` (4): `cmd_exercises_check`, `cmd_exercises_list`, `cmd_exercises_submit`, `cmd_exercises_sync`
- Assertions it ports (8 test files, the files themselves are listed under `rust-27`): `us-46-exercises.sh`, `us-49-mistakes.sh`, `test_exercise_cache_runtime_probe.py`, `test_exercise_complexity.py`, `test_exercise_diagnostics.py`, `test_exercises_cache.py`, `test_issue_185_teach.py`, `test_teacher.py`

#### rust-23: Port local voice, model routes and judge adapters (6 files)

- `config/` (1): `models.yaml`
- `src/careerbrew/` (2): `judge.py`, `models.py`
- `src/careerbrew/tutor/` (1): `voice.py`
- `tests/fixtures/voice/` (2): `behavioral-answer.txt`, `behavioral-answer.wav`
- CLI handlers it ports out of `cli.py` (1): `cmd_drill_speak`
- Assertions it ports (9 test files, the files themselves are listed under `rust-27`): `us-57-voice-practice.sh`, `test_agent_skill_declaration.py`, `test_issue_326.py`, `test_issue_371.py`, `test_issue_385.py`, `test_issue_423_eval_inputs.py`, `test_issue_423_eval_output.py`, `test_judge_completion.py`, `test_llm_evals.py`

#### rust-24: Port reports, metrics, checkups and remaining domain helpers (16 files)

- `src/careerbrew/` (16): `changes.py`, `charts.py`, `checklist.py`, `checkup.py`, `checkup_runs.py`, `events.py`, `field.py`, `metrics_share.py`, `repo_health.py`, `report.py`, `review.py`, `runs.py`, `signals.py`, `telemetry.py`, `time_saved.py`, `wizard.py`
- CLI handlers it ports out of `cli.py` (29): `cmd_cards_publish`, `cmd_changes`, `cmd_checklist`, `cmd_checkup`, `cmd_data_decrypt`, `cmd_data_encrypt`, `cmd_data_set`, `cmd_data_show`, `cmd_day_plan`, `cmd_day_render`, `cmd_day_send`, `cmd_day_update`, `cmd_drill_progress`, `cmd_machine_new_user`, `cmd_machine_ready`, `cmd_metrics_field`, `cmd_metrics_time_saved`, `cmd_metrics_weekly`, `cmd_plugin_new`, `cmd_report`, `cmd_report_public`, `cmd_review`, `cmd_signals`, `cmd_telemetry`, `cmd_time`, `cmd_work_collect`, `cmd_work_focus`, `cmd_work_project`, `cmd_work_promo`
- Assertions it ports (91 test files, the files themselves are listed under `rust-27`): `us-06-progress-chart.sh`, `us-07-evidence.sh`, `us-09-promo.sh`, `us-100-work-day-truth.sh`, `us-101-live-listening.sh`, `us-102-work-day-start.sh`, `us-104-question-records.sh`, `us-105-work-day-timing.sh`, `us-106-end-of-day.sh`, `us-107-project-sessions.sh`, `us-11-review.sh`, `us-12-plugin.sh`, `us-13-privacy.sh`, `us-14-connectors.sh`, `us-16-linkedin.sh`, `us-22-plugin-new.sh`, `us-25-day-plan.sh`, `us-29-second-person.sh`, `us-31-changes.sh`, `us-40-corporate-rehearsal.sh`, `us-41-focus-dashboard.sh`, `us-43-data-show.sh`, `us-43-encryption-at-rest.sh`, `us-44-public-study-deck.sh`, `us-47-goals.sh`, `us-50-work-projects.sh`, `us-51-project-research.sh`, `us-53-actions-and-slack.sh`, `us-54-metrics-share.sh`, `us-55-second-user.sh`, `us-55-split-plugin-evidence.sh`, `us-56-project-explain.sh`, `us-59-pm-update.sh`, `us-60-project-import.sh`, `us-61-work-feed.sh`, `us-65-standup-report.sh`, `us-67-run-pages.sh`, `us-68-project-memory-sync.sh`, `us-69-project-push.sh`, `us-74-inbox-for-new-users.sh`, `us-77-project-watch.sh`, `us-79-repo-health.sh`, `us-81-outlook-sign-in.sh`, `us-84-decision-signals.sh`, `us-86-time-saved.sh`, `us-87-impact.sh`, `us-88-ladder-grade.sh`, `us-89-field-telemetry.sh`, `us-93-prep-readiness.sh`, `us-94-work-day.sh`, `us-96-work-day-stages.sh`, `us-99-promises.sh`, `test_corporate_exits.py`, `test_corporate_ready.py`, `test_deployed_commands.py`, `test_grader_findings.py`, `test_issue_101.py`, `test_issue_185_feed.py`, `test_issue_185_pm.py`, `test_issue_185_signals.py`, `test_issue_186.py`, `test_issue_232.py`, `test_issue_238.py`, `test_issue_259.py`, `test_issue_260.py`, `test_issue_261.py`, `test_issue_263.py`, `test_issue_265.py`, `test_issue_269.py`, `test_issue_412.py`, `test_issue_423.py`, `test_issue_423_metrics_output.py`, `test_issue_423_plugin_source.py`, `test_issue_432.py`, `test_issue_438.py`, `test_issue_70.py`, `test_issues_38_39.py`, `test_metrics_share.py`, `test_pinned_clock.py`, `test_plugins_privacy.py`, `test_projects.py`, `test_redirect_host_policy.py`, `test_repo_health.py`, `test_story_metrics.py`, `test_telemetry.py`, `test_tutor.py`, `test_v2_grader.py`, `test_web_grader.py`, `test_wizard.py`, `test_work.py`, `test_workday.py`

#### rust-25: Replace the HTTP server behind the unchanged UI (9 files)

- `src/careerbrew/` (1): `server.py`
- `web/e2e/` (3): `mcp-repair-fixture.ts`, `pm-slack-fixture.ts`, `signin.spec.ts`
- `web/` (4): `playwright.config.ts`, `playwright.promise.config.ts`, `playwright.visual.config.ts`, `vite.config.ts`
- `web/src/` (1): `api.ts`
- CLI handlers it ports out of `cli.py` (4): `cmd_open`, `cmd_serve`, `cmd_start`, `cmd_web_rotate_token`
- Assertions it ports (40 test files, the files themselves are listed under `rust-27`): `us-37-web-api.sh`, `us-52-daily-log.sh`, `us-71-web-sign-in.sh`, `test_action_parity.py`, `test_api_cache.py`, `test_checkup.py`, `test_day.py`, `test_daylog.py`, `test_issue_110.py`, `test_issue_118.py`, `test_issue_141.py`, `test_issue_147.py`, `test_issue_172.py`, `test_issue_180_threads.py`, `test_issue_198.py`, `test_issue_227.py`, `test_issue_229.py`, `test_issue_254.py`, `test_issue_268.py`, `test_issue_271.py`, `test_issue_272.py`, `test_issue_274.py`, `test_issue_377_closed_today.py`, `test_issue_406.py`, `test_issue_409.py`, `test_issue_74.py`, `test_live_health_seed.py`, `test_notion_import.py`, `test_pm_send.py`, `test_pm_sent_cache.py`, `test_regrade.py`, `test_run_pages.py`, `test_server.py`, `test_slow_start.py`, `test_task_labels.py`, `test_task_links.py`, `test_time_saved.py`, `test_voice.py`, `test_watch_idle.py`, `test_web_blocks.py`

#### rust-26: Complete native CLI coverage and default entry point (12 files)

- `./` (1): `Agentfile.yaml`
- `commands/` (6): `careerbrew-checkup.md`, `careerbrew-corporate.md`, `careerbrew-grind.md`, `careerbrew-learn.md`, `careerbrew.md`, `work.md`
- `src/careerbrew/` (2): `__main__.py`, `cli.py`
- `web/e2e/` (1): `feedback.spec.ts`
- `web/src/` (2): `act-status.test.tsx`, `act.ts`
- `cli.py` stays listed here, but `rust-26` ports only the dispatcher, argument parsing, the action registry wiring and the final switch. The 168 domain handlers port in the steps above. Every handler calls a domain module, so none is left for `rust-26`.

#### rust-27: Port developer tools, test harnesses, installers and release checks (477 files)

- **rust-27a, shared test harness** (283 files):
  - `src/careerbrew/work/` (1): `claim_audit.py`
  - `tests/` (1): `__init__.py`
  - `tests/fixtures/claims/candidates/` (1): `records.json`
  - `tests/fixtures/claims/` (1): `manifest.json`
  - `tests/fixtures/claims/truth/` (1): `records.json`
  - `tests/fixtures/notifier/` (1): `fake-notifier.sh`
  - `tests/fixtures/plugin-acme/bin/` (1): `tickets.py`
  - `tests/fixtures/voice/` (1): `fake-stt.sh`
  - `tests/safety/` (2): `test_issue_327_native.py`, `test_issue_327_native_cleanup.py`
  - `tests/unit/` (266): `__init__.py`, `conftest.py`, `test_action_parity.py`, `test_agent_skill_declaration.py`, `test_api_cache.py`, `test_batch_gate.py`, `test_browser_test_receipts.py`, `test_cards.py`, `test_check_stories.py`, `test_checkup.py`, `test_checkup_command.py`, `test_checkup_html.py`, `test_claim_audit.py`, `test_clock_reads.py`, `test_connect_wizard.py`, `test_connectors_machines.py`, `test_contributing.py`, `test_corporate_exits.py`, `test_corporate_ready.py`, `test_data_show.py`, `test_day.py`, `test_daylog.py`, `test_deployed_commands.py`, `test_doctor_rust_deps.py`, `test_drill_session_input_ends.py`, `test_e2e_cache_runtime.py`, `test_e2e_lib.py`, `test_e2e_results.py`, `test_e2e_runner.py`, `test_e2e_us52_pipes.py`, `test_eval_case_loading.py`, `test_every_action_has_an_e2e.py`, `test_exercise_cache_runtime_probe.py`, `test_exercise_complexity.py`, `test_exercise_diagnostics.py`, `test_exercises_cache.py`, `test_focus.py`, `test_format_cache_runtime.py`, `test_gate_cache.py`, `test_gate_cache_index.py`, `test_gate_cache_runtime.py`, `test_gate_corporate.py`, `test_gate_inputs.py`, `test_goals.py`, `test_grader_findings.py`, `test_grind_command.py`, `test_hosted_ci.py`, `test_hot_reload.py`, `test_import_memory_questions.py`, `test_install_prompt.py`, `test_interview_prep.py`, `test_issue_101.py`, `test_issue_110.py`, `test_issue_118.py`, `test_issue_141.py`, `test_issue_145.py`, `test_issue_147.py`, `test_issue_172.py`, `test_issue_175.py`, `test_issue_180.py`, `test_issue_180_threads.py`, `test_issue_183.py`, `test_issue_184.py`, `test_issue_185_feed.py`, `test_issue_185_gaps.py`, `test_issue_185_pm.py`, `test_issue_185_signals.py`, `test_issue_185_teach.py`, `test_issue_186.py`, `test_issue_198.py`, `test_issue_199.py`, `test_issue_200.py`, `test_issue_210.py`, `test_issue_214.py`, `test_issue_214_why.py`, `test_issue_216.py`, `test_issue_221.py`, `test_issue_223.py`, `test_issue_226.py`, `test_issue_227.py`, `test_issue_228.py`, `test_issue_229.py`, `test_issue_230.py`, `test_issue_232.py`, `test_issue_234.py`, `test_issue_238.py`, `test_issue_239.py`, `test_issue_254.py`, `test_issue_258.py`, `test_issue_259.py`, `test_issue_260.py`, `test_issue_261.py`, `test_issue_263.py`, `test_issue_264.py`, `test_issue_265.py`, `test_issue_266.py`, `test_issue_267.py`, `test_issue_268.py`, `test_issue_269.py`, `test_issue_270.py`, `test_issue_271.py`, `test_issue_272.py`, `test_issue_274.py`, `test_issue_276.py`, `test_issue_280.py`, `test_issue_305.py`, `test_issue_307.py`, `test_issue_308.py`, `test_issue_309.py`, `test_issue_310.py`, `test_issue_311.py`, `test_issue_312.py`, `test_issue_313.py`, `test_issue_314.py`, `test_issue_315.py`, `test_issue_316.py`, `test_issue_326.py`, `test_issue_327.py`, `test_issue_330.py`, `test_issue_331.py`, `test_issue_333.py`, `test_issue_334.py`, `test_issue_335_337.py`, `test_issue_336.py`, `test_issue_338.py`, `test_issue_339.py`, `test_issue_355_owner_steps.py`, `test_issue_356.py`, `test_issue_357_session_claim.py`, `test_issue_366.py`, `test_issue_371.py`, `test_issue_372.py`, `test_issue_374_legacy_feed.py`, `test_issue_377_closed_today.py`, `test_issue_385.py`, `test_issue_389_baseline.py`, `test_issue_389_outcomes.py`, `test_issue_389_resolution.py`, `test_issue_395_skill.py`, `test_issue_401.py`, `test_issue_406.py`, `test_issue_408.py`, `test_issue_409.py`, `test_issue_412.py`, `test_issue_422.py`, `test_issue_423.py`, `test_issue_423_bundles.py`, `test_issue_423_eval_inputs.py`, `test_issue_423_eval_output.py`, `test_issue_423_keys.py`, `test_issue_423_metadata_temp.py`, `test_issue_423_metrics_output.py`, `test_issue_423_missing_key.py`, `test_issue_423_plugin_source.py`, `test_issue_426.py`, `test_issue_429.py`, `test_issue_432.py`, `test_issue_437.py`, `test_issue_438.py`, `test_issue_52.py`, `test_issue_58.py`, `test_issue_60.py`, `test_issue_68.py`, `test_issue_70.py`, `test_issue_71.py`, `test_issue_72.py`, `test_issue_73.py`, `test_issue_74.py`, `test_issues_38_39.py`, `test_issues_43_46.py`, `test_jira_connector.py`, `test_jobs.py`, `test_judge_completion.py`, `test_launcher.py`, `test_learn.py`, `test_learn_replay_tour.py`, `test_learnings_table.py`, `test_linkedin.py`, `test_lint_cache_runtime.py`, `test_live_health_seed.py`, `test_llm_evals.py`, `test_mail_linkedin.py`, `test_mcp_browser_fixture.py`, `test_mcp_fix.py`, `test_memory_sync.py`, `test_metrics_report.py`, `test_metrics_share.py`, `test_migrate_history.py`, `test_no_conflict_markers.py`, `test_notion_import.py`, `test_open_prs.py`, `test_open_work.py`, `test_outlook_connector.py`, `test_ownership_docs.py`, `test_pinned_clock.py`, `test_platforms.py`, `test_playwright_server.py`, `test_plugin_commands.py`, `test_plugins_privacy.py`, `test_pm_history_concurrency.py`, `test_pm_send.py`, `test_pm_sent_cache.py`, `test_pm_sent_coverage.py`, `test_pm_sent_reconciliation.py`, `test_pre_push_root_guard.py`, `test_pre_push_secret_range.py`, `test_prior_session_selection.py`, `test_privacy_index.py`, `test_privacy_scan_file.py`, `test_project_explain.py`, `test_project_import.py`, `test_project_research.py`, `test_project_since.py`, `test_projects.py`, `test_promise_reference_successor.py`, `test_promo_matching.py`, `test_proof.py`, `test_public_cards.py`, `test_public_readiness.py`, `test_public_scrub.py`, `test_push_context.py`, `test_redirect_host_policy.py`, `test_region_labels.py`, `test_regrade.py`, `test_regrade_notion.py`, `test_repo_health.py`, `test_rules_reference_successor.py`, `test_run_pages.py`, `test_rust_hash_experiment.py`, `test_rust_readiness.py`, `test_rust_vault_base64_parity.py`, `test_rust_vault_interop.py`, `test_second_user.py`, `test_server.py`, `test_setup_story_health_check.py`, `test_slow_start.py`, `test_split_plugin_evidence.py`, `test_state_concurrency.py`, `test_story_metrics.py`, `test_task_labels.py`, `test_task_links.py`, `test_teacher.py`, `test_telemetry.py`, `test_tests_affected.py`, `test_time_saved.py`, `test_tutor.py`, `test_type_cache_runtime.py`, `test_types_cache_safeguard.py`, `test_types_command_collision_probe.py`, `test_unit_cache_runtime.py`, `test_v2_grader.py`, `test_vault.py`, `test_vault_plugins.py`, `test_visual_pictures.py`, `test_voice.py`, `test_watch_idle.py`, `test_watch_mail_plan.py`, `test_watch_probe.py`, `test_web_blocks.py`, `test_web_grader.py`, `test_web_seed.py`, `test_web_type_cache_probe.py`, `test_wizard.py`, `test_work.py`, `test_workday.py`, `watch_gate_child.py`
  - `xtask/tests/fixtures/capture/` (2): `data_driver.py`, `http_driver.py`
  - `xtask/tests/fixtures/capture/observer/` (1): `sitecustomize.py`
  - `xtask/tests/fixtures/linux-observer/` (4): `child.py`, `export.py`, `parent.py`, `supervisor.py`
- **rust-27b, developer scripts, hooks and installers** (38 files):
  - `./` (2): `INSTALL.md`, `lefthook.yml`
  - `hooks/lefthook/pre-push/` (1): `privacy.sh`
  - `hooks/` (2): `pre-commit`, `pre-push`
  - `scripts/` (33): `api-timing.py`, `batch-gate.sh`, `benchmark-day-start-discovery.py`, `browser-test-receipts.py`, `check-card-code.py`, `check-deployed-commands.py`, `check-docs.sh`, `check-sources.py`, `check-stories.py`, `check.sh`, `checkup.sh`, `doctor.sh`, `e2e.sh`, `import-research.py`, `input-key.sh`, `install-launcher.sh`, `install.sh`, `leak-check.sh`, `learnings-table.py`, `metrics.sh`, `migrate-history.sh`, `open-prs.sh`, `open-work.sh`, `pm-sent-coverage.py`, `proof.py`, `public-readiness.py`, `public-readiness.sh`, `report-html.sh`, `rust-readiness.sh`, `setup.sh`, `tests_affected.py`, `verify-push-context.py`, `visual-pictures.sh`
- **rust-27c, e2e stories** (114 files):
  - `tests/e2e/` (114): `fake-judge.py`, `lib.sh`, `live_health_seed.py`, `us-01-setup.sh`, `us-02-jobs-scan-filter.sh`, `us-03-want-rate-learn.sh`, `us-04-strategy.sh`, `us-05-drill.sh`, `us-06-progress-chart.sh`, `us-07-evidence.sh`, `us-08-backup.sh`, `us-09-promo.sh`, `us-10-plan.sh`, `us-100-work-day-truth.sh`, `us-101-live-listening.sh`, `us-102-work-day-start.sh`, `us-103-work-day-watch.sh`, `us-104-question-records.sh`, `us-105-work-day-timing.sh`, `us-106-end-of-day.sh`, `us-107-project-sessions.sh`, `us-11-review.sh`, `us-110-pm-promise-outcomes.sh`, `us-12-plugin.sh`, `us-13-privacy.sh`, `us-14-connectors.sh`, `us-15-machines-sync.sh`, `us-16-linkedin.sh`, `us-17-interview.sh`, `us-18-prep.sh`, `us-19-job-platforms.sh`, `us-20-linkedin-import.sh`, `us-21-mail.sh`, `us-22-plugin-new.sh`, `us-23-day-watch.sh`, `us-24-day-next.sh`, `us-25-day-plan.sh`, `us-26-sources.sh`, `us-27-stories-skills.sh`, `us-28-ladder-import.sh`, `us-29-second-person.sh`, `us-30-hot-reload.sh`, `us-31-changes.sh`, `us-32-research.sh`, `us-33-patterns.sh`, `us-34-skill-practice.sh`, `us-35-web-app.sh`, `us-36-plan-ahead.sh`, `us-37-web-api.sh`, `us-38-notion-import.sh`, `us-39-flashcards.sh`, `us-40-corporate-rehearsal.sh`, `us-41-focus-dashboard.sh`, `us-42-connect.sh`, `us-43-data-show.sh`, `us-43-encryption-at-rest.sh`, `us-44-public-study-deck.sh`, `us-45-teacher.sh`, `us-46-exercises.sh`, `us-47-goals.sh`, `us-48-job-choice.sh`, `us-49-mistakes.sh`, `us-50-work-projects.sh`, `us-51-project-research.sh`, `us-52-daily-log.sh`, `us-53-actions-and-slack.sh`, `us-54-metrics-share.sh`, `us-55-second-user.sh`, `us-55-split-plugin-evidence.sh`, `us-56-project-explain.sh`, `us-57-voice-practice.sh`, `us-58-action-feedback.sh`, `us-59-pm-update.sh`, `us-60-project-import.sh`, `us-61-work-feed.sh`, `us-62-notifications.sh`, `us-63-today-open-asks.sh`, `us-64-fast-views.sh`, `us-65-standup-report.sh`, `us-66-llm-evals.sh`, `us-67-run-pages.sh`, `us-68-project-memory-sync.sh`, `us-69-project-push.sh`, `us-70-task-links.sh`, `us-71-web-sign-in.sh`, `us-72-every-control.sh`, `us-73-page-order-and-paging.sh`, `us-74-inbox-for-new-users.sh`, `us-75-visual-stories.sh`, `us-76-comfort-budget.sh`, `us-77-project-watch.sh`, `us-78-team-feed.sh`, `us-79-repo-health.sh`, `us-80-learn-project.sh`, `us-81-outlook-sign-in.sh`, `us-82-active-now.sh`, `us-83-learn-replay-tour.sh`, `us-84-decision-signals.sh`, `us-85-handoff.sh`, `us-86-time-saved.sh`, `us-87-impact.sh`, `us-88-ladder-grade.sh`, `us-89-field-telemetry.sh`, `us-90-review-ask.sh`, `us-91-agent-activity.sh`, `us-93-prep-readiness.sh`, `us-94-work-day.sh`, `us-95-standup-proofs.sh`, `us-96-work-day-stages.sh`, `us-97-live-feed.sh`, `us-98-reply-drafts.sh`, `us-99-promises.sh`, `web-seed-and-serve.sh`, `web_seed.py`
- **rust-27d, evals** (39 files):
  - `evals/` (4): `README.md`, `rubric.md`, `run.py`, `run.sh`
  - `evals/cases/` (35): `001-start-session.yaml`, `002-job-hunt.yaml`, `003-interview-drill.yaml`, `004-work-companion.yaml`, `005-promo-research.yaml`, `006-corporate-setup.yaml`, `007-weekly-review.yaml`, `008-day-planner.yaml`, `009-day-watch.yaml`, `010-privacy-guard.yaml`, `011-linkedin-profile.yaml`, `012-interview-coach.yaml`, `013-inbox-triage.yaml`, `014-company-research.yaml`, `015-best-practices.yaml`, `016-browser-research.yaml`, `017-notion-import.yaml`, `018-card-explainer.yaml`, `018-corporate-plugin.yaml`, `019-teacher.yaml`, `020-learn-project.yaml`, `021-voice-practice.yaml`, `022-project-push.yaml`, `023-project-explainer.yaml`, `024-live-feed.yaml`, `024-work-day.yaml`, `025-promises.yaml`, `026-project-sessions.yaml`, `027-access-blockers.yaml`, `027-cross-host.yaml`, `027-work-mcp-repair.yaml`, `027-work-readiness.yaml`, `027-work-start-freshness.yaml`, `028-late-wrap-up.yaml`, `028-pm-content.yaml`
- **rust-27e, CI workflows** (3 files):
  - `.github/workflows/` (3): `ci.yml`, `public-readiness.yml`, `visual-pictures.yml`
- **Assertions left for rust-27** (68 test files with no domain owner): `us-01-setup.sh`, `us-27-stories-skills.sh`, `us-34-skill-practice.sh`, `us-35-web-app.sh`, `us-36-plan-ahead.sh`, `us-58-action-feedback.sh`, `us-66-llm-evals.sh`, `us-72-every-control.sh`, `us-73-page-order-and-paging.sh`, `us-75-visual-stories.sh`, `us-76-comfort-budget.sh`, `test_issue_327_native.py`, `test_issue_327_native_cleanup.py`, `test_batch_gate.py`, `test_browser_test_receipts.py`, `test_check_stories.py`, `test_checkup_command.py`, `test_checkup_html.py`, `test_claim_audit.py`, `test_clock_reads.py`, `test_doctor_rust_deps.py`, `test_drill_session_input_ends.py`, `test_e2e_cache_runtime.py`, `test_e2e_lib.py`, `test_e2e_results.py`, `test_e2e_runner.py`, `test_e2e_us52_pipes.py`, `test_format_cache_runtime.py`, `test_gate_cache.py`, `test_gate_cache_index.py`, `test_gate_cache_runtime.py`, `test_gate_inputs.py`, `test_grind_command.py`, `test_hosted_ci.py`, `test_install_prompt.py`, `test_issue_175.py`, `test_issue_221.py`, `test_issue_280.py`, `test_issue_327.py`, `test_launcher.py`, `test_learnings_table.py`, `test_lint_cache_runtime.py`, `test_mcp_browser_fixture.py`, `test_metrics_report.py`, `test_migrate_history.py`, `test_no_conflict_markers.py`, `test_open_prs.py`, `test_open_work.py`, `test_ownership_docs.py`, `test_playwright_server.py`, `test_pre_push_root_guard.py`, `test_pre_push_secret_range.py`, `test_promise_reference_successor.py`, `test_proof.py`, `test_public_readiness.py`, `test_push_context.py`, `test_rules_reference_successor.py`, `test_rust_hash_experiment.py`, `test_rust_readiness.py`, `test_setup_story_health_check.py`, `test_tests_affected.py`, `test_type_cache_runtime.py`, `test_types_cache_safeguard.py`, `test_types_command_collision_probe.py`, `test_unit_cache_runtime.py`, `test_visual_pictures.py`, `test_web_seed.py`, `test_web_type_cache_probe.py`

#### rust-28: Retire the old implementation and verify fresh-machine releases (4 files)

- `./` (3): `.python-version`, `pyproject.toml`, `uv.lock`
- `src/careerbrew/` (1): `py.typed`

## Keep this page true

- This page is the plan. A Rust task block in [`TASKS.md`](../TASKS.md) holds
  the task's own Hypothesis, Measurement, Success, Pivot and Anchor. When they
  differ, fix both in the same pull request.
- A step that lands updates [Current state](#current-state), its row in the
  [owner map](#every-module-has-an-owner-step), and (for `rust-00`)
  [State of rust-00](#state-of-rust-00).
- A new or moved CLI handler or test file changes the owner map. Regenerate the
  handler and test-file columns in the same pull request.
- A change to the order, a limit or a contract is a plan change. Pre-register
  it here before the code, with its reason.
- Plain, short sentences. No private data, no machine paths, no old pull
  request numbers. Check with `uv run careerbrew privacy scan --file docs/rust-migration.md`.
