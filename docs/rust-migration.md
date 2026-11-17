# Rust migration plan

This page is the complete plan to move careerbrew's code outside the UI from
Python and shell to Rust. It is the one public source for the Rust tasks
`rust-00` to `rust-28` in [`TASKS.md`](../TASKS.md). A worker with only this
repository can do any step from this page and the task block.

- **Status:** a plan. No Rust code runs in production today. No speed or memory
  gain is claimed. See [Current state](#current-state) and
  [State of rust-00](#state-of-rust-00).
- **Next task:** `rust-00` (the measurement harness). Every other step waits for it.
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
| `cargo xtask migration baseline`, `verify`, `measure` and `decide` do not exist yet | [Current state](#current-state) | `rust-00` must create them |
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
5. **Decide.** Run `cargo xtask migration decide --step rust-NN`.
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

- Each decision covers one cache state.
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
| [activation manifest](plans/2026-11-04-rust-activation-manifest.tsv) | the config, vault, approvals and role rows of the table: 24 cross-module call sites (config 4, vault 3, approvals 7, role and bridges 10) and 22 local bridge calls | 6 |
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

- Only `rust-00` is ready now. Each task's **Blocked** line is a hard gate.
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
  (14 harness controls today, exit 0) and
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
  `changes.py` (`_h` only), `__init__.py`.
- **Outputs:** native value types and the hash primitive that `rust-24` reuses.
- **Gate:** 100% of precedence, invalid-input, daylight-saving and hash vectors
  agree; 0 writes to a real home.

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
- **Inputs:** `src/careerbrew/store.py`, `vault.py`, `secrets.py`, `backup.py`
  (key provider), `config.py` (IO), `events.py` (emit primitive), `approvals.py`
  (receipt IO); the disconnected `crates/vault` codec.
- **Outputs:** the native storage engine and key provider.
- **Gate:** 100% old and new round trips; 0 lost or duplicate writes in 1,000
  mixed-writer operations; 0 plaintext secret exits or file-mode regressions.
  Production dispatch waits for the closure in the
  [worked example](#build-order-versus-production-activation).

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
| a tracked source file is added (any executable mode, or a Python, shell, JavaScript, TypeScript or Rust extension) | add its row to the [code inventory](plans/2026-11-01-rust-code-inventory.csv): path, disposition (`port`, `keep-ui` or `keep-native`), task, reason; then add it to the [owner map](#every-module-has-an-owner-step) | `cargo xtask migration inventory --check` |
| a reviewed interpreter call site in source moves | update its row in the [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) | `cargo xtask migration inventory --embedded-check` |
| `src/careerbrew/vault.py` | update the pinned `SOURCE_SHA` in `tests/unit/test_rust_vault_interop.py` and `tests/unit/test_rust_vault_base64_parity.py` | `uv run pytest -q tests/unit/test_rust_vault_interop.py tests/unit/test_rust_vault_base64_parity.py` |

Rules of the refresh:

- Keep every existing assignment and the UI exception. One missing or
  reassigned existing path is a failure: stop and fix it.
- Keep **zero** unlisted direct code paths.
- The inventory check reads the staged Git index. Stage a new source path and
  its inventory row together.
- Refresh only the recorded cells. Keep the partial graph scope, the owners,
  the edges and the checker logic.
- The direct-path scan covers direct paths. The embedded-anchor list covers
  the reviewed sites only. Discovery of other embedded code and process
  observation are still open `rust-00` work.

Anchor: Basili, Caldiera and Rombach 1994 (Goal Question Metric).

## Current state

Read from the code on 2026-11-15, at `main` `ecbbb7fc`. Check it again with
the commands in each row before you rely on it.

**Summary:** the Rust code is measurement preparation and two disconnected
component experiments. No CLI command, HTTP route or watcher calls Rust. The
app runs on Python. No step has a merged decide result. The next task is
`rust-00`.

### Crates (`ls crates xtask experiments`, `Cargo.toml`)

| Path | Package | What it is | Production use |
| --- | --- | --- | --- |
| `xtask/` | `careerbrew-xtask` | the migration developer commands and their tests | none |
| `crates/vault/` | `careerbrew-vault` | a byte-level `CBV1` vault codec on the maintained `fernet` crate. It takes a supplied synthetic key. No file, network, keychain or policy work. | none: component parity only |
| `experiments/hash/` | `careerbrew-hash-experiment` | the pure `changes._h` hash for a supported synthetic JSON subset. It refuses values outside that subset. | none: component parity only |

- No crate ships: `scripts/rust-shipped-crates.txt` is empty. A crate whose
  "Production use" stops being "none" must be on that list, and setup must
  rebuild it after a pull (`tests/unit/test_rust_shipped_crates.py`).
- The workspace pins Rust `1.100.0` (`rust-toolchain.toml`, minimal profile with
  `clippy` and `rustfmt`).
- `.cargo/config.toml` defines `cargo xtask` as
  `run --locked --offline --package careerbrew-xtask --`.
- Pinned crates: `serde`, `serde_json`, `sha2`, `tempfile`, `libc` (xtask);
  `fernet` 0.2.2, `base64` (vault). Licences are in
  [public-readiness-licences.md](proof/public-readiness-licences.md).

### xtask commands (the dispatch in `xtask/src/main.rs`)

The tool has no `--help`. A wrong argument prints the usage and exits 2.

| Command | What it does |
| --- | --- |
| `cargo xtask migration self-test` | runs the 8 paired-verdict controls and the 6 trial-planner controls, all on synthetic data |
| `cargo xtask migration inventory --check` | checks that every tracked direct code path in the staged Git index has exactly one row in the [code inventory](plans/2026-11-01-rust-code-inventory.csv) |
| `cargo xtask migration inventory --embedded-check` | checks the reviewed interpreter call sites in the [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) against staged source |
| `cargo xtask migration toolchain-identity --rustc <absolute-path>` | checks the pinned compiler release, its full commit hash in the Clippy driver and the shared compiler library. Read-only, no network. It supports the thin macOS Mach-O layout only; other layouts are refused. |
| `cargo xtask migration capture-actions --python <absolute-path> --out <new-file>` | runs `python -B -m careerbrew actions --json` twice in fresh synthetic homes and compares stdout, stderr, exit, schema and writes |
| `cargo xtask migration capture-http --python <absolute-path> --out <new-file>` | runs two fresh synthetic server processes over a frozen HTTP corpus (21 request cases) |
| `cargo xtask migration capture-data --python <absolute-path> --out <new-file>` | runs a ten-case write and read corpus twice and compares typed values, encrypted boundaries and declared writes |
| `cargo xtask migration observer-probe --image <immutable-id> [--out <new-directory> [--freeze-only\|--fault-controls-only]]` | the fixed Linux process-observer probe in an owned, network-disabled container |

**Not built yet:** `baseline`, `verify`, `measure` and `decide`. Every port step
needs them.

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
  `capture_data`, `capture_data_cli`, `capture_http`, `i01_fixture`,
  `observer_deadline`, `observer_owner`, `observer_parser`,
  `observer_transport`, `store_supervisor`.
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

This section replaces the old continuation notes. Those notes and the old
draft pull request are not on the public repository.

### Done (in the public tree)

| Part | Result | Command |
| --- | --- | --- |
| Workspace, toolchain pin, `cargo xtask` alias | in place | `cargo xtask migration self-test` |
| Paired verdict (`baseline_verdict.rs`) | 8 of 8 synthetic controls pass; an empty check suite is refused | `cargo xtask migration self-test` |
| Trial planner (`trial_plan.rs`) | 6 of 6 controls pass | `cargo xtask migration self-test` |
| Direct-path inventory | 0 unassigned tracked direct paths; 4 negative controls (removed, duplicated, untracked row, broken CSV field) refused; runs in the normal gate | `cargo xtask migration inventory --check` |
| Embedded anchors | the reviewed sites match staged source; a missing or repeated anchor is refused | `cargo xtask migration inventory --embedded-check` |
| Toolchain identity probe | 7 cases (valid, release mismatch, malformed identity, missing driver, outside symlink, exact token, bounded scan) | `cargo test -p careerbrew-xtask toolchain_identity` |
| Action contract capture | 7 integration controls | `cargo test -p careerbrew-xtask --test capture_actions` |
| HTTP contract capture | 13 control groups; 21 request cases in each of two fresh processes | `cargo test -p careerbrew-xtask --test capture_http` |
| Data contract capture | 10 corpus cases in each of two fresh trials; 13 control groups | `cargo test -p careerbrew-xtask --test capture_data` |
| Activation manifest | three partial slices; 20 of 20 controls; zero multi-function cycles | `cargo test -p careerbrew-xtask --test activation_manifest` |
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
- Whole-process-tree resource collection is **not qualified**. The old draft
  had raw owned-child `wait4` accounting and a prepared Darwin RSS unit
  calibration (it predicted bytes, not KiB, in 6 of 6 fixed runs). That work
  was not merged and is not in the public tree. Redo it here.
- One full local gate on the old draft failed one owned-process cleanup test
  (all others passed). Its cause was never found. A later fast gate passed
  without explaining it. The hosted gate on `main` is green today; treat any
  repeat as a real failure and read its log.
- A nonempty check label does not prove that a test ran. The future collector
  must bind that evidence.
- Embedded-code discovery (beyond the reviewed anchors) and process
  observation for the inventory are open.

### Next, in order

1. Recheck the gate: `gh pr checks` on a fresh pull request, and
   `cargo xtask migration self-test` and `inventory --check` locally.
2. Build owned-child resource accounting with calibrated units on each
   supported host (macOS and Linux). Pre-register the calibration first.
3. Build `cargo xtask migration baseline`, `verify`, `measure` and `decide` on
   top of the verdict, the trial planner, the captures and the collector.
4. Register the noise floors and the per-contract normalization rules.
5. Add the planted slow-path control and prove the decide step holds it.
6. Extend the inventory to embedded code and generated entry points.
7. Then mark `rust-00` done, remove its task block, and unblock `rust-01`.

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
| [code inventory](plans/2026-11-01-rust-code-inventory.csv) | `xtask/src/inventory.rs` | path, disposition, owner task and reason for each executable file |
| [activation manifest](plans/2026-11-04-rust-activation-manifest.tsv) | `xtask/tests/activation_manifest.rs` | config, vault, approvals and role call graph rows |
| [activation providers](plans/2026-11-04-rust-activation-providers.tsv) | `xtask/tests/activation_manifest.rs` | changes, strategy, tutor and teacher call graph rows |
| [activation workday](plans/2026-11-04-rust-activation-workday.tsv) | `xtask/tests/activation_manifest.rs` | work-day start, end, clock and watcher call graph rows |
| [embedded anchors](plans/2026-11-04-rust-embedded-anchors.csv) | `xtask/src/inventory.rs` | each embedded script, with its owner task and source anchor |
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
| [`rust-00`](#rust-00-build-the-baseline-harness-and-complete-code-inventory) | 69 | 0 | 0 |
| [`rust-01`](#rust-01-prove-maintained-dependency-and-data-interoperability) | 0 | 0 | 0 |
| [`rust-02`](#rust-02-port-pure-setting-schemas-paths-clocks-and-hashes) | 3 | 1 | 4 |
| [`rust-03`](#rust-03-introduce-the-native-cli-and-one-read-only-pilot) | 2 | 1 | 1 |
| [`rust-04`](#rust-04-port-privacy-policy-and-approval-enforcement) | 3 | 8 | 4 |
| [`rust-05`](#rust-05-port-encrypted-storage-and-transaction-locks) | 3 | 2 | 1 |
| [`rust-06`](#rust-06-port-backup-restore-and-cross-machine-sync) | 2 | 7 | 6 |
| [`rust-07`](#rust-07-port-plugin-machine-and-account-setup) | 22 | 11 | 9 |
| [`rust-08`](#rust-08-port-http-sources-calendar-and-mail) | 58 | 1 | 5 |
| [`rust-09`](#rust-09-port-projects-priorities-and-promises) | 9 | 4 | 2 |
| [`rust-10`](#rust-10-port-sessions-claims-agents-and-reply-drafts) | 4 | 6 | 5 |
| [`rust-11`](#rust-11-port-project-imports-exact-source-links-and-memory-sync) | 3 | 0 | 4 |
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
| [`rust-25`](#rust-25-replace-the-http-server-behind-the-unchanged-ui) | 9 | 4 | 42 |
| [`rust-26`](#rust-26-complete-native-cli-coverage-and-default-entry-point) | 12 | 0 | 0 |
| [`rust-27`](#rust-27-port-developer-tools-test-harnesses-installers-and-release-checks) | 477 | 0 | 68 |
| [`rust-28`](#rust-28-retire-the-old-implementation-and-verify-fresh-machine-releases) | 4 | 0 | 0 |
| **All steps** | **955 of 955** | **168 of 168** | **374 of 374** |

#### rust-00: Build the baseline harness and complete code inventory (69 files)

- `.cargo/` (1): `config.toml`
- `./` (3): `Cargo.lock`, `Cargo.toml`, `rust-toolchain.toml`
- `crates/vault/` (1): `Cargo.toml`
- `crates/vault/src/` (2): `lib.rs`, `main.rs`
- `docs/plans/` (12): `2026-11-01-rust-code-inventory.csv`, `2026-11-04-rust-activation-manifest.tsv`, `2026-11-04-rust-activation-providers.tsv`, `2026-11-04-rust-activation-workday.tsv`, `2026-11-04-rust-embedded-anchors.csv`, `2026-11-05-rust-data-bootstrap-schema.json`, `2026-11-05-rust-data-fault-schema.json`, `2026-11-05-rust-data-fixture-v1.json`, `2026-11-05-rust-data-phase-schema.json`, `2026-11-05-rust-data-value-schema.json`, `2026-11-08-promise-action-delta.json`, `2026-11-08-rules-action-delta.json`
- `experiments/hash/` (1): `Cargo.toml`
- `experiments/hash/src/` (1): `main.rs`
- `xtask/` (1): `Cargo.toml`
- `xtask/src/` (9): `baseline_verdict.rs`, `capture_actions.rs`, `capture_data.rs`, `capture_http.rs`, `inventory.rs`, `main.rs`, `observer_probe.rs`, `toolchain_identity.rs`, `trial_plan.rs`
- `xtask/src/bin/` (1): `store_supervisor_fixture.rs`
- `xtask/src/observer_probe/` (3): `ownership.rs`, `parser.rs`, `transport.rs`
- `xtask/tests/` (11): `activation_manifest.rs`, `capture_actions.rs`, `capture_data.rs`, `capture_data_cli.rs`, `capture_http.rs`, `i01_fixture.rs`, `observer_deadline.rs`, `observer_owner.rs`, `observer_parser.rs`, `observer_transport.rs`, `store_supervisor.rs`
- `xtask/tests/fixtures/capture/` (12): `actions-reference-promise-v2.json`, `actions-reference-rules-promise-v3.json`, `actions-reference-rules-v2.json`, `actions-reference-v1.json`, `actions-v1.json`, `data_reference_v1.provenance.json`, `data_reference_v1.semantic.json`, `http-reference-promise-v2.json`, `http-reference-rules-promise-v3.json`, `http-reference-rules-v2.json`, `http-reference-v1.json`, `http-v1.json`
- `xtask/tests/fixtures/i01/` (1): `pending-identities-v1.tsv`
- `xtask/tests/fixtures/linux-observer-replay-v7/` (5): `README.md`, `manifest.json`, `status.json`, `trace.10`, `trace.11`
- `xtask/tests/fixtures/linux-observer/` (1): `Dockerfile`
- `xtask/tests/support/` (4): `actions_controls.rs`, `http_controls.rs`, `owner_controls.rs`, `store_supervisor.rs`

#### rust-01: Prove maintained dependency and data interoperability (0 files)

- No file of its own. It reads the files of other steps (see its **Inputs**).

#### rust-02: Port pure setting schemas, paths, clocks and hashes (3 files)

- `src/careerbrew/` (3): `__init__.py`, `config.py`, `paths.py`
- CLI handlers it ports out of `cli.py` (1): `cmd_config`
- Assertions it ports (4 test files, the files themselves are listed under `rust-27`): `us-64-fast-views.sh`, `test_contributing.py`, `test_eval_case_loading.py`, `test_issue_335_337.py`

#### rust-03: Introduce the native CLI and one read-only pilot (2 files)

- `src/careerbrew/` (2): `actions.py`, `agent_workflows.py`
- CLI handlers it ports out of `cli.py` (1): `cmd_actions`
- Assertions it ports (1 test file, the files themselves are listed under `rust-27`): `test_every_action_has_an_e2e.py`

#### rust-04: Port privacy, policy and approval enforcement (3 files)

- `src/careerbrew/` (2): `approvals.py`, `privacy.py`
- `src/careerbrew/work/` (1): `access.py`
- CLI handlers it ports out of `cli.py` (8): `cmd_approvals_decide`, `cmd_approvals_guidance`, `cmd_approvals_list`, `cmd_approvals_request`, `cmd_approvals_show`, `cmd_draft_judge`, `cmd_draft_route`, `cmd_privacy_scan`
- Assertions it ports (4 test files, the files themselves are listed under `rust-27`): `test_issue_314.py`, `test_privacy_index.py`, `test_privacy_scan_file.py`, `test_public_scrub.py`

#### rust-05: Port encrypted storage and transaction locks (3 files)

- `src/careerbrew/` (3): `secrets.py`, `store.py`, `vault.py`
- CLI handlers it ports out of `cli.py` (2): `cmd_data_edit`, `cmd_secret_set`
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

#### rust-09: Port projects, priorities and promises (9 files)

- `src/careerbrew/work/` (7): `__init__.py`, `focus.py`, `priorities.py`, `project_priority.py`, `projects.py`, `promises.py`, `team.py`
- `tests/fixtures/projects/` (2): `cutline.json`, `team-goals.csv`
- CLI handlers it ports out of `cli.py` (4): `cmd_work_done`, `cmd_work_impact`, `cmd_work_priority`, `cmd_work_task_set`
- Assertions it ports (2 test files, the files themselves are listed under `rust-27`): `test_issue_230.py`, `test_issue_429.py`

#### rust-10: Port sessions, claims, agents and reply drafts (4 files)

- `src/careerbrew/work/` (4): `agents.py`, `drafts.py`, `prior_selection.py`, `sessions.py`
- CLI handlers it ports out of `cli.py` (6): `cmd_draft_check`, `cmd_draft_fit`, `cmd_draft_replace`, `cmd_draft_show`, `cmd_work_agent`, `cmd_work_claim`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-91-agent-activity.sh`, `us-98-reply-drafts.sh`, `test_issue_357_session_claim.py`, `test_issue_401.py`, `test_prior_session_selection.py`

#### rust-11: Port project imports, exact source links and memory sync (3 files)

- `src/careerbrew/work/` (3): `memory_sync.py`, `project_import.py`, `research.py`
- Assertions it ports (4 test files, the files themselves are listed under `rust-27`): `test_issue_184.py`, `test_issue_309.py`, `test_project_research.py`, `test_project_since.py`

#### rust-12: Port project learning, replay, tours and explainers (6 files)

- `src/careerbrew/learn/` (4): `__init__.py`, `project.py`, `replay.py`, `tour.py`
- `src/careerbrew/work/` (2): `explain.py`, `prd.py`
- CLI handlers it ports out of `cli.py` (14): `cmd_learn_answer`, `cmd_learn_concepts`, `cmd_learn_list`, `cmd_learn_next`, `cmd_learn_note`, `cmd_learn_replay_check`, `cmd_learn_replay_list`, `cmd_learn_replay_measure`, `cmd_learn_replay_start`, `cmd_learn_start`, `cmd_learn_status`, `cmd_learn_tour_add`, `cmd_learn_tour_approve`, `cmd_learn_tour_export`
- Assertions it ports (5 test files, the files themselves are listed under `rust-27`): `us-80-learn-project.sh`, `us-83-learn-replay-tour.sh`, `test_issue_223.py`, `test_learn_replay_tour.py`, `test_project_explain.py`

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
- Assertions it ports (42 test files, the files themselves are listed under `rust-27`): `us-37-web-api.sh`, `us-52-daily-log.sh`, `us-71-web-sign-in.sh`, `test_action_parity.py`, `test_api_cache.py`, `test_checkup.py`, `test_day.py`, `test_daylog.py`, `test_issue_110.py`, `test_issue_118.py`, `test_issue_141.py`, `test_issue_147.py`, `test_issue_172.py`, `test_issue_180_threads.py`, `test_issue_198.py`, `test_issue_227.py`, `test_issue_229.py`, `test_issue_254.py`, `test_issue_268.py`, `test_issue_271.py`, `test_issue_272.py`, `test_issue_274.py`, `test_issue_377_closed_today.py`, `test_issue_406.py`, `test_issue_409.py`, `test_issue_74.py`, `test_live_health_seed.py`, `test_memory_sync.py`, `test_notion_import.py`, `test_pm_send.py`, `test_pm_sent_cache.py`, `test_project_import.py`, `test_regrade.py`, `test_run_pages.py`, `test_server.py`, `test_slow_start.py`, `test_task_labels.py`, `test_task_links.py`, `test_time_saved.py`, `test_voice.py`, `test_watch_idle.py`, `test_web_blocks.py`

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
