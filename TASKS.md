# Tasks

Format: [tasks.md](https://github.com/tasksmd/tasks.md). Remove a task block when it is done.
Every task carries Hypothesis, Measurement (a runnable command), Success, Pivot, and Anchor,
written before the work starts. Calendar work lives in `RECURRING.md`.

## P0

- [ ] Company plugin: adopt the 2026-11 core (public repo, hosted CI, telemetry, Rust, CI cache)
  - **ID**: company-plugin-adopt-core-2026-11
  - **Blocked**: owner step: runs only on a company machine, in the private `careerbrew-<org-overlay>` repo; nothing company-specific comes back here
  - **Details**: Follow `docs/company-plugin-update.md` steps (a) to (e): a private-pattern leak check in the plugin repo; plugin CI on hosted runners and no local core suite (hook `hooks/block-local-tests.sh` covers only a core checkout at `~/apps/careerbrew`, opt-out `CAREERBREW_ALLOW_LOCAL_TESTS=1`, on `main` since PR #108); `policy.telemetry: false` unless the company approves (`telemetry_sink` is not a field yet, task `telemetry-plugin-sink`); only verified manifest fields, ready for `rust-07`; no plugin CI cache until a run passes 5 minutes (`docs/ci.md`).
  - **Files**: docs/company-plugin-update.md; the company plugin repository (outside this repository)
  - **Hypothesis**: a plugin that follows the plan keeps `corporate readiness` at or above its baseline and leaks 0 company terms, because every step uses only fields and gates that exist on `main`.
  - **Measurement**: `uv run careerbrew machine ready` (the `corporate readiness: N/N` line, before and after); `uv run careerbrew plugin verify`; the plugin's `scripts/plugin-leak-check.sh <draft>` count.
  - **Success**: readiness after is N/N with N at or above the baseline; `plugin verify` ok for every plugin; leak count 0; plugin CI green on the pinned and newest core.
  - **Pivot**: if readiness drops by 1 or more and the cause is a core field change, revert the plugin commit, pin the company Mac to the prior core commit, and file a generic core issue after `careerbrew privacy scan --file <draft>` passes.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Saltzer & Schroeder 1975 (fail-safe defaults, least privilege).

- [ ] Telemetry key: set the PostHog write-only key once on each other machine, and decide on a committed key
  - **ID**: telemetry-key-owner
  - **Blocked**: owner step: only the owner holds the key, and `careerbrew secret set` reads it from a hidden terminal prompt (never from chat or a file)
  - **Details**: No key is committed: a shipped `phc_` key fails public-readiness surface S22 ("machine and account identifiers", rule `key` in `scripts/public-readiness.py`), while S11 (gitleaks) passes. Done on the owner's main Mac (2026-11-15: `key: keychain`, consent yes, 8 events sent and read back in PostHog). Steps. (1) On each other machine run `careerbrew secret set posthog_key` and type the key; then `careerbrew telemetry status` shows `key: keychain`. (2) Decide later whether a committed write-only key is allowed. If yes, the change is an owner decision on S22 (an allow line repeats the matched text, so prefer a rule change that clears one named key file) plus the key file and its lookup step in `src/careerbrew/telemetry.py`; never an agent-added allow entry.
  - **Files**: src/careerbrew/telemetry.py; scripts/public-readiness.py; config/public-readiness-allow.txt
  - **Hypothesis**: with the key set on a machine that said yes, that machine's events reach PostHog within a day, so the tiles get data.
  - **Measurement**: `careerbrew telemetry status` (expect `key: keychain` and `last send:` a date within one day of the yes); `scripts/public-readiness.sh --released` (S22 `pass`).
  - **Success**: every owner machine shows `key: keychain`; S22 stays `pass`.
  - **Pivot**: if no machine has sent after a week, check `careerbrew telemetry send --dry-run` for the reason before any committed-key change.
  - **Anchor**: Kohavi, Tang & Xu 2020 (trustworthy online experiments: data quality before analysis).

- [ ] Telemetry sink 1 of 2: let a company plugin send telemetry to its own log system (Splunk HEC or a file) instead of PostHog
  - **ID**: telemetry-plugin-sink
  - **Details**: A company may forbid a third-party analytics service and still want usage counts in its own log system. Today a plugin can only say `policy.telemetry: false`. This task adds a sink that only a plugin can set. Iron rule: this base repository must never hold a company name, an internal host, an index name, or a token; examples use `splunk.example.com`. Read `docs/plugins.md` (manifest and policy), `src/careerbrew/plugins.py` (`effective_policy`, manifest validation), `src/careerbrew/secrets.py` (secret references), `src/careerbrew/connectors.py` (`check_host`, `open_url`) and `src/careerbrew/telemetry.py` first. Steps. (1) Manifest. Add an optional block to `careerbrew-plugin.yaml` under `policy`: `telemetry_sink: {kind: posthog | splunk-hec | file | off, url: https://..., token: <secret reference>, index: <string>, sourcetype: <string>, path: <file path>, consent: ask | managed, error_logs: false}`. Validation at `plugin add` and `plugin verify`: `kind` is one of the four; `splunk-hec` needs `url` (`https` only; plain `http` is accepted only for a loopback host, `127.0.0.1`, `localhost` or `::1`, so tests and a local forwarder work and no token crosses a network in clear text) and `token`, and `token` must be a secret reference (`keychain:`, `op://`, `env:`), never a literal; `file` needs `path`, which must be absolute or start with `~` and must not be inside a cloud-synced folder (`machine.cloud_synced`); `posthog` and `off` take no `url`, `token` or `path`; `consent: managed` and `error_logs: true` are allowed only for `splunk-hec` and `file`. An invalid block refuses the install with a message that names the field. (2) Merge rule in `effective_policy`: strictest wins. Any plugin with `telemetry: false` or `kind: off` turns every send off. Two plugins with different non-off sinks: refuse both and say so (never send to two places, never pick one silently). (3) Sending. Put sink code in a new module `src/careerbrew/telemetry_sink.py` with one function per kind and one `deliver(events, policy)` entry; `telemetry.send()` calls it. The same allowlisted event objects go to every sink: a plugin sink gets no extra field. Splunk HEC body: one JSON object per event, `{"time": <epoch seconds>, "host": "careerbrew", "source": "careerbrew", "sourcetype": <sourcetype or "careerbrew:event">, "index": <index, only if set>, "event": {<allowlisted fields>, "install_id": <id>}}`, objects joined by newlines, POST to `<url>` (the plugin gives the full collector URL, for example `https://splunk.example.com:8088/services/collector/event`) with header `Authorization: Splunk <token>`. Send through the policy-aware opener `connectors.open_url` (it checks every redirect against `allowed_hosts`, refuses HTTPS to plain HTTP, and never lets a request with a token follow a redirect to another origin). It arrives with the pull request "close judge round findings" for the public release; if `rg -n 'def open_url' src/careerbrew/connectors.py` finds nothing, stop: that pull request must merge first. Never send the token with a bare `urllib.request.urlopen`. Resolve the token at send time with `secrets.resolve`; never store it, log it, or print it; the dry run shows `<token>`. File sink: append one JSON line per event to the path, create it with mode 600, and when it passes 5 MB rename it to `<path>.1` and start a new file; refuse the write when the file itself, its `<path>.1` rotation target, or any parent folder is a symlink (open with `O_NOFOLLOW` and check each parent), and when the resolved path is in a cloud-synced folder. With a plugin sink of kind `splunk-hec`, `file` or `off` set, nothing goes to PostHog (`kind: posthog` keeps the default service). (4) Consent. `consent: ask` keeps the user's question, with the destination named as "your organization's log system at <host>". `consent: managed` means the company decided: no question, sends are on, and `careerbrew telemetry status` and the first-run notice say who set it and where it goes; `careerbrew telemetry off` still turns it off for a user unless a later company policy field forbids that (do not add such a field now). `DO_NOT_TRACK=1` always wins. (5) `error_logs`: default false. The exception class already leaves as `error` on every `cli.error` (since 2026-11-15), so this flag adds nothing to that: when true, a failed web control (`web.act` with `status: error`) may also carry the class; never a message, a path, or a stack trace. (6) Status and dry run: `careerbrew telemetry status` prints the sink kind, the host or the path, the consent mode, and which plugin set it; `careerbrew telemetry send --dry-run` prints the exact body for the active sink. Update `careerbrew actions --json` parity if a command changes. (7) Tests first, with a local fake HEC server (a `ThreadingHTTPServer` on `127.0.0.1` port 0 inside a pytest fixture; copy the `redirector` fixture in `tests/unit/test_redirect_host_policy.py` if that file exists, else write the fixture new): the body shape and header are exact; the token never appears in stdout, the event log, or an error message (plant a canary token through `env:`); a literal token in the manifest refuses the install; `http://` to a host that is not loopback refuses; a host outside `allowed_hosts` sends nothing; a redirect to another origin sends nothing; two different sinks refuse; `kind: off` and `telemetry: false` send nothing; with a `splunk-hec`, `file` or `off` sink set, the PostHog fake receives zero requests; a failed send keeps the events queued and never fails a command; the file sink rotates, and refuses a symlinked file, a symlinked rotation target and a symlinked parent; a canary in any event text field never reaches any sink. (8) Docs: a "Company log sink" section in `docs/plugins.md` (the contract for plugin authors, with the example above), rows in `docs/privacy.md`, lines in `docs/corporate-machine.md` and `docs/observability.md`, a new user story with acceptance lines that name the tests, and one plain line in the corporate setup skill: "telemetry destination: `careerbrew telemetry status`".
  - **Files**: src/careerbrew/telemetry.py; src/careerbrew/telemetry_sink.py; src/careerbrew/plugins.py; src/careerbrew/cli.py; tests/unit/test_telemetry_sink.py; tests/unit/test_telemetry.py; docs/plugins.md; docs/privacy.md; docs/corporate-machine.md; docs/observability.md; docs/user-stories.md; skills/careerbrew-corporate-setup/SKILL.md
  - **Hypothesis**: a company that forbids third-party analytics can still see usage counts, with 0 events sent to PostHog from its machines and 0 content fields in its log system, because the plugin sink replaces the default sink and reuses the same allowlist.
  - **Measurement**: `uv run pytest tests/unit/test_telemetry_sink.py tests/unit/test_telemetry.py -q`; a new end-to-end script `tests/e2e/us-<n>-telemetry-sink.sh` (copy the shape of a neighbour in `tests/e2e/`) that, in a scratch data folder, copies `tests/fixtures/plugin-acme` to a temp folder, adds a `policy.telemetry_sink` block with `kind: splunk-hec`, `url: http://127.0.0.1:<port>/services/collector/event`, and `token: env:CB_TEST_HEC_TOKEN`, starts a fake collector, installs the plugin, saves consent `yes`, runs `careerbrew telemetry send`, and compares the received body with the documented shape.
  - **Success**: 0 requests reach the PostHog fake when a plugin sink is set; 0 test outputs hold the canary token; 100% of the listed refusal cases refuse; the dry run body matches the documented HEC shape byte for byte.
  - **Pivot**: if the HEC shape cannot be proven against a real collector in task `telemetry-plugin-sink-company`, keep only the `file` sink (a company forwarder can ship a file) and mark `splunk-hec` experimental in `docs/plugins.md`.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Splunk Docs, "Format events for HTTP Event Collector"; Saltzer & Schroeder 1975 (fail-safe defaults, least privilege).

- [ ] Telemetry sink 2 of 2: turn the company sink on in a real company plugin and prove it end to end
  - **ID**: telemetry-plugin-sink-company
  - **Blocked**: prerequisite: telemetry-plugin-sink must land first; this task runs only on a company machine, by a person or agent the company allows, in the company's own plugin repository. Nothing from it is written to this repository except the two generic results in the last step.
  - **Details**: Work order for the company side. Never copy a company host, index, token, name, or log line into this repository, a pull request here, or an issue here. Steps. (1) Ask the company's log-system owner for: approval to send careerbrew usage counts; a collector URL; an HEC token limited to one index; the index and sourcetype names. Show them the field list from `careerbrew telemetry send --dry-run`. (2) Store the token where company policy allows. First read which secret backends the policy permits (`careerbrew connectors list` prints a `policy:` line with `secret_backends`; `None` means no limit; an empty list `[]` means no backend is allowed, so stop and ask the company's plugin owner). If the Keychain is allowed, run `careerbrew secret set <name>` and use the reference `keychain:<name>`. If only another backend is allowed (for example a company password manager), store the token there with that tool and use its reference (`op://...`). If no allowed backend exists, stop and ask the company's plugin owner. Never put the token in a file, a chat, or the plugin repository. (3) In the company plugin's `careerbrew-plugin.yaml`, add the `policy.telemetry_sink` block with `kind: splunk-hec`, the URL, `token: <the reference from step 2>`, the index, the sourcetype, and `consent: managed` only if the company's privacy owner approved that in writing; else `consent: ask`. Add the collector host to `allowed_hosts`. (4) `careerbrew plugin refresh`. With `consent: ask`, the person who uses the machine then runs `careerbrew telemetry on` themself after reading the destination line (a corporate machine starts at `no`; an agent never answers for them, and an earlier `no` stays `no`). Then `careerbrew telemetry status` (expect the sink kind, the host, the consent mode, the plugin name), then `careerbrew telemetry send --dry-run` (read every field), then `careerbrew telemetry send`. (5) Count per send, in windows that do not overlap: note the time T0, run one `careerbrew telemetry send`, note T1, wait until T1 plus 2 minutes, then search the index for `sourcetype=<sourcetype>` by the time the collector received the events (index time, `_indextime` from T0 to T1 plus 2 minutes), not by event time: queued events keep their older original timestamps. Compare only with the count that the send itself prints (a `status` call logs its own run event, so its waiting count is one behind). That count must equal the count the send printed, and no event may hold free text. Stop the day watcher for the trial, because it is what sends telemetry once a day: `careerbrew day watch off`, then check with `careerbrew day watch status` (confirm both command names with `careerbrew day watch --help`); turn it back on with `careerbrew day watch on` when the 3 sends are done. Repeat for 3 sends. (6) Confirm nothing reached the default service: with the sink set, `careerbrew telemetry status` must not name PostHog. (7) Report back to this repository only: "HEC shape accepted: yes or no" and "events received equal events sent: yes or no", as a comment on this task's pull request, after `careerbrew privacy scan --file <draft>` passes and company policy allows the report. If the collector rejects the body, record the HTTP status and the generic reason (for example "index field not allowed"), fix the shape in the base repository under task `telemetry-plugin-sink`, and repeat.
  - **Files**: the company plugin repository (outside this repository); docs/plugins.md only if the generic contract needs a correction
  - **Hypothesis**: the documented HEC body is accepted by a real collector unchanged, because it follows the public HEC event format.
  - **Measurement**: `uv run careerbrew telemetry send` on the company machine (its printed count), and the log-system search count for that send's own time window, 3 times.
  - **Success**: events received equal events sent on 3 sends in a row; 0 events with free text; 0 requests to the default service.
  - **Pivot**: if the collector cannot be reached from the machine or the token cannot be approved within two weeks, switch the plugin to `kind: file` and let the company's own log forwarder read the file.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Splunk Docs, "Format events for HTTP Event Collector".

- [ ] Rust migration: Port encrypted storage and transaction locks
  - **ID**: rust-05
  - **Details**: Preserve encryption envelope, key location/mode, missing-key refusal, JSON/YAML/JSONL semantics, atomic replacement, unique temporary names, lock paths and write-if-changed. Extract the key-path/load functions currently in backup.py, settings IO and minimal content-free event emission here, so native storage never depends on later backup/config/event orchestration. Include approval-receipt storage, with one implementation per provider. Prove mixed Python/Rust writers under the same advisory lock and crash/interruption boundaries. Port whole transactions together; never hold a parent lock while a child reacquires it. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-05` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Progress**: rust-05a done (#167): key provider in `keyfile.py`, settings IO in `config_io.py`, `events.emit` split into `record` and `write_row`; no behaviour change (`tests/unit/test_rust05a_provider_seams.py`). rust-05b done (#165); rust-05b2 (approvals lock) done (#185). rust-05c: native key provider `crates/vault/src/keyfile.rs` (component only, unit tests in the module). rust-05d: native atomic write `crates/vault/src/atomic.rs` and cross-engine key checks through the synthetic `vault-store` binary (`tests/unit/test_rust05d_key_interop.py`). Next: rust-05e (native advisory lock, 1,000-operation mixed Python and Rust writer run).
  - **Files**: src/careerbrew/store.py; vault.py; secrets.py; keyfile.py (key provider); config_io.py (settings IO); events.py (emit primitive); approvals.py (receipt IO)
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-05`, `heavy-run cargo xtask migration verify --step rust-05`, `heavy-run --exclusive cargo xtask migration measure --step rust-05 --pairs 30`, then `cargo xtask migration decide --step rust-05`. Commit the baseline and prediction before implementation.
  - **Success**: 100% old/new round trips; 0 lost/duplicate writes in 1000 mixed-writer operations; 0 plaintext secret exits or mode regressions. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port backup, restore and cross-machine sync
  - **ID**: rust-06
  - **Blocked**: prerequisite: rust-05 must land and pass its acceptance gates first
  - **Details**: Port archive verification and restore, including strict base64/trailing-junk rejection, conflict handling and sync allowlists. Verify backups made by either engine restore under both. Keep any format change a separate ADR and measured task. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-06` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/backup.py; sync.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-06`, `heavy-run cargo xtask migration verify --step rust-06`, `heavy-run --exclusive cargo xtask migration measure --step rust-06 --pairs 30`, then `cargo xtask migration decide --step rust-06`. Commit the baseline and prediction before implementation.
  - **Success**: 100% valid archives restore; 100% tampered/truncated/trailing-junk archives rejected; 0 unapproved exports or lost files. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port plugin, machine and account setup
  - **ID**: rust-07
  - **Blocked**: prerequisite: rust-05 must land and pass its acceptance gates first
  - **Details**: Port plugin validation, policy merging, corporate readiness, secret/OAuth references and preflight/self-update ownership checks. Preserve symlinks, modes, managed-config refusal and explicit repair confirmation. Keep tokens in local stores, never in fixtures or subprocess logs. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-07` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/plugin-acme/`, `tests/fixtures/plugin-acme-platform/`, `tests/fixtures/http/login.microsoftonline.com/` and `.env.example` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/plugins.py; machine.py; msauth.py; workday/mcp_check.py; workday/self_update.py; workday/workflow_check.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-07`, `heavy-run cargo xtask migration verify --step rust-07`, `heavy-run --exclusive cargo xtask migration measure --step rust-07 --pairs 30`, then `cargo xtask migration decide --step rust-07`. Commit the baseline and prediction before implementation.
  - **Success**: 100% readiness/manifest/repair cases agree; 0 secret disclosures, silent repairs or unmanaged configuration writes. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port HTTP sources, calendar and mail
  - **ID**: rust-08
  - **Blocked**: prerequisite: rust-05, rust-07 must land and pass its acceptance gates first
  - **Details**: Port bounded HTTP, retries, errors, auth/host validation, cursor handling, calendars and recurrence. Replay current recorded synthetic HTTP fixtures without live accounts. Preserve source-local failure isolation and terms restrictions. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-08` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/http/` (every host folder except the feed and team-feed folders and the sign-in host) and `tests/fixtures/calendar/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/connectors.py; calendar.py; mail.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-08`, `heavy-run cargo xtask migration verify --step rust-08`, `heavy-run --exclusive cargo xtask migration measure --step rust-08 --pairs 30`, then `cargo xtask migration decide --step rust-08`. Commit the baseline and prediction before implementation.
  - **Success**: 100% source/recurrence vectors agree; 0 wrong-host credentials, duplicate cursor items or cross-source suppression. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port projects, priorities and promises
  - **ID**: rust-09
  - **Blocked**: prerequisite: rust-05, rust-07 must land and pass its acceptance gates first
  - **Details**: Port project lifecycle, configured ticket identity, readiness gates, priority estimates and promise closure. Preserve finite-number checks, idempotent IDs, owner-edited closures and lock scope. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-09` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/projects/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/projects.py; priorities.py; project_priority.py; promises.py; focus.py; team.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-09`, `heavy-run cargo xtask migration verify --step rust-09`, `heavy-run --exclusive cargo xtask migration measure --step rust-09 --pairs 30`, then `cargo xtask migration decide --step rust-09`. Commit the baseline and prediction before implementation.
  - **Success**: 100% project/gate/promise contracts agree; 0 duplicate promises, lost owner edits or UTF-8/ISO-8601 false ticket matches. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port sessions, claims, agents and reply drafts
  - **ID**: rust-10
  - **Blocked**: prerequisite: rust-09 must land and pass its acceptance gates first
  - **Details**: Port scope/claim checks, activity lifecycle, expiry and draft clear-save-readback locks. Include full-scope delivery checks and any fixes landed while this migration is planned. Preserve origin-session requirements and newest-thread-state checks. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-10` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/sessions.py; agents.py; drafts.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-10`, `heavy-run cargo xtask migration verify --step rust-10`, `heavy-run --exclusive cargo xtask migration measure --step rust-10 --pairs 30`, then `cargo xtask migration decide --step rust-10`. Commit the baseline and prediction before implementation.
  - **Success**: 100% session/draft cases agree; 0 cross-session writes, stale claims or conflicting target saves in 1000 interleaved operations. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port project imports, exact source links and memory sync
  - **ID**: rust-11
  - **Blocked**: prerequisite: rust-08, rust-09 must land and pass its acceptance gates first
  - **Details**: Port exact URL and repository identity rules, imported project mapping and digest-based memory sync. Preserve modes and concurrent owner appends; avoid broadening an item URL to repository scope. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-11` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/research.py; project_import.py; memory_sync.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-11`, `heavy-run cargo xtask migration verify --step rust-11`, `heavy-run --exclusive cargo xtask migration measure --step rust-11 --pairs 30`, then `cargo xtask migration decide --step rust-11`. Commit the baseline and prediction before implementation.
  - **Success**: 100% identity/sync fixtures agree; 0 wrong-project matches, lost appends or changed file modes. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port project learning, replay, tours and explainers
  - **ID**: rust-12
  - **Blocked**: prerequisite: rust-09, rust-11 must land and pass its acceptance gates first
  - **Details**: Port project learning, replay/tour caches, explanation contracts and PRD validation. Keep prompts/reference material declarative and source hashes stable. Preserve privacy of replay and local project material. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-12` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/learn/; work/explain.py; work/prd.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-12`, `heavy-run cargo xtask migration verify --step rust-12`, `heavy-run --exclusive cargo xtask migration measure --step rust-12 --pairs 30`, then `cargo xtask migration decide --step rust-12`. Commit the baseline and prediction before implementation.
  - **Success**: 100% deterministic learning/replay/PRD contracts agree; 0 new data exits or repeated computation for unchanged inputs. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port feed ingestion and transcript normalization
  - **ID**: rust-13
  - **Blocked**: prerequisite: rust-08, rust-09 must land and pass its acceptance gates first
  - **Details**: Port source ingestion, raw tool replies, retained original payloads, edited-message upserts and deduplication. Preserve synthetic Slack DM channel metadata, source timestamps and content-free cycle bookkeeping. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-13` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/http/feed/`, `tests/fixtures/http/team-feed/`, `tests/fixtures/http/team-feed-noscope/`, `tests/fixtures/feed/` and `tests/fixtures/transcripts/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/feed.py; transcript.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-13`, `heavy-run cargo xtask migration verify --step rust-13`, `heavy-run --exclusive cargo xtask migration measure --step rust-13 --pairs 30`, then `cargo xtask migration decide --step rust-13`. Commit the baseline and prediction before implementation.
  - **Success**: 100% event vectors agree; 0 dropped edits, duplicate events or idle change-version churn. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port live listening, review asks and notifications
  - **ID**: rust-14
  - **Blocked**: prerequisite: rust-10, rust-11, rust-13 must land and pass its acceptance gates first
  - **Details**: Port rule evaluation, project updates, reply/review drafts, reviewer selection, notification deduplication and source rosters. Use fake notifiers first on PATH. Prove draft-only defaults and approval-required sends. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-14` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/live/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/live.py; listen.py; review_ask.py; notify.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-14`, `heavy-run cargo xtask migration verify --step rust-14`, `heavy-run --exclusive cargo xtask migration measure --step rust-14 --pairs 30`, then `cargo xtask migration decide --step rust-14`. Commit the baseline and prediction before implementation.
  - **Success**: 100% cycle/reviewer/notification cases agree; 0 unsolicited sends, duplicated notifications or content-bearing logs. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port canonical work-day records and questions
  - **ID**: rust-15
  - **Blocked**: prerequisite: rust-05, rust-09, rust-10 must land and pass its acceptance gates first
  - **Details**: Port the actual landed canonical schema, legacy-reader guards, stale-writer receipts, question records and private/public projections. Preserve raw running resource claims and skipped-step reason rules. Port the final landed canonical work-day schema rather than a planning snapshot. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-15` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/workday/status.py; questions.py; steps.py; stages.py; pending schema/dashboard modules
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-15`, `heavy-run cargo xtask migration verify --step rust-15`, `heavy-run --exclusive cargo xtask migration measure --step rust-15 --pairs 30`, then `cargo xtask migration decide --step rust-15`. Commit the baseline and prediction before implementation.
  - **Success**: 100% schema/legacy/question cases agree; 0 silent legacy overwrites, exposed business fields or dropped resource claims. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port work-day execution, scheduling and watch loops
  - **ID**: rust-16
  - **Blocked**: prerequisite: rust-07, rust-14, rust-15 must land and pass its acceptance gates first
  - **Details**: Port refresh-before-plan, sign-in blocking, readiness dispatch, proof checking, timeout/retry, one-loop lock, cycle/watch, catch-up and movable graceful wrap-up. Preserve startup and wrap-up changes from pending PRs. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-16` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `src/careerbrew/workday/cron_check.py` (an inventory row) ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/workday/start.py; plan.py; gate.py; watch.py; end.py; timing.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-16`, `heavy-run cargo xtask migration verify --step rust-16`, `heavy-run --exclusive cargo xtask migration measure --step rust-16 --pairs 30`, then `cargo xtask migration decide --step rust-16`. Commit the baseline and prediction before implementation.
  - **Success**: 100% execution/state-transition cases agree; 0 duplicate dispatches, false success claims or lost wrap-up requests in 1000 interleavings. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port daily planning, results and PM learning
  - **ID**: rust-17
  - **Blocked**: prerequisite: rust-09, rust-13, rust-15, rust-16 must land and pass its acceptance gates first
  - **Details**: Port results, inbox, plans, logs, handoff, done-elsewhere, PM drafts/lessons, watch cycles and sent-Tomorrow promises. Preserve exact sent-message receipts, manual authority, edited DM reconciliation, distinct-day lesson thresholds and no-op idempotence from final landed fixes. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-17` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/pm-promise-outcomes/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/day/ including pending pm_promises.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-17`, `heavy-run cargo xtask migration verify --step rust-17`, `heavy-run --exclusive cargo xtask migration measure --step rust-17 --pairs 30`, then `cargo xtask migration decide --step rust-17`. Commit the baseline and prediction before implementation.
  - **Success**: 100% day/PM vectors agree; 0 wrong-recipient lessons, duplicate promises/history or idle writes. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port evidence, promotion, standup and work profiles
  - **ID**: rust-18
  - **Blocked**: prerequisite: rust-09, rust-11, rust-17 must land and pass its acceptance gates first
  - **Details**: Port evidence mapping, ladder grading, promotion documents, standup proof gates and talking-point bullets. Preserve actual source state and business-data restrictions. Keep current HTML/CSS appearance and browser code. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-18` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/standup/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/work/evidence.py; ladder_grade.py; promo.py; standup.py; linkedin.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-18`, `heavy-run cargo xtask migration verify --step rust-18`, `heavy-run --exclusive cargo xtask migration measure --step rust-18 --pairs 30`, then `cargo xtask migration decide --step rust-18`. Commit the baseline and prediction before implementation.
  - **Success**: 100% evidence/standup/report semantic contracts agree; 0 unsupported claims, missing proof or unsafe HTML. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port job search, interview prep and public research
  - **ID**: rust-19
  - **Blocked**: prerequisite: rust-08, rust-09, rust-12, rust-20 must land and pass its acceptance gates first
  - **Details**: Port job platform adapters, criteria, wants, strategy, LinkedIn drafts, interview loops and prep gates. Extract the native gap-queue provider used by jobs.strategy from tutor.engine in rust-20 first; the remaining teacher orchestration stays in rust-21. Preserve corporate-role blocks, source terms and public-only research. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-19` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/ats/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/jobs/; interview.py; prep.py; research.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-19`, `heavy-run cargo xtask migration verify --step rust-19`, `heavy-run --exclusive cargo xtask migration measure --step rust-19 --pairs 30`, then `cargo xtask migration decide --step rust-19`. Commit the baseline and prediction before implementation.
  - **Success**: 100% scoring/strategy/prep vectors agree; 0 corporate job-hunt access or unapproved application/send actions. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port flashcards, FSRS and Notion import
  - **ID**: rust-20
  - **Blocked**: prerequisite: rust-05, rust-08 must land and pass its acceptance gates first
  - **Details**: Port scheduling using a maintained FSRS implementation validated against current vectors, card grading, public deck checks and Notion import. Also port the shared tutor.engine gap-queue/provider primitives and mistake-record helpers needed by jobs.strategy and exercise submission before their consumers; leave teacher orchestration for rust-21. Preserve private/public card separation, learning timestamps and import idempotence. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-20` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/notion-bundle/` and `tests/fixtures/public-cards/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/tutor/cards.py; public.py; notion_import.py; tutor/engine.py (shared providers); tutor/mistakes.py (record helpers)
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-20`, `heavy-run cargo xtask migration verify --step rust-20`, `heavy-run --exclusive cargo xtask migration measure --step rust-20 --pairs 30`, then `cargo xtask migration decide --step rust-20`. Commit the baseline and prediction before implementation.
  - **Success**: 100% fixed-clock FSRS decisions agree within preregistered numeric tolerances; 0 private card leaks or duplicate imports. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port teacher, goals, practice and mistake tracking
  - **ID**: rust-21
  - **Blocked**: prerequisite: rust-19, rust-20, rust-22 must land and pass its acceptance gates first
  - **Details**: Port lesson selection, goals, practice progress, mistakes and improvement rules. Preserve local-day counting, repeat-mistake metrics and model-independent deterministic decisions. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-21` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/teacher.py; goals.py; tutor/engine.py; mistakes.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-21`, `heavy-run cargo xtask migration verify --step rust-21`, `heavy-run --exclusive cargo xtask migration measure --step rust-21 --pairs 30`, then `cargo xtask migration decide --step rust-21`. Commit the baseline and prediction before implementation.
  - **Success**: 100% practice/goal/mistake vectors agree; 0 lost grades or UTC/local-day count errors. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port exercise execution and complexity checks
  - **ID**: rust-22
  - **Blocked**: prerequisite: rust-05, rust-08, rust-20 must land and pass its acceptance gates first
  - **Details**: Port sandbox orchestration, language runners, CPU/resource limits, repeated scaling measurements and complexity decisions. Port canonical executable reference solutions, scaling generators and verification fixtures to Rust so the normal exercise gate needs no Python. Preserve each functional assertion, comparison mode and wrong-complexity control. Retain Python examples only as optional language-specific teaching/practice data; an explicit Python practice session may invoke its optional interpreter. The native app and normal gate must run without Python. Separate compilation from timed algorithm samples and retain resource limits for both. Keep cubic negative controls and fresh-execution timing evidence. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-22` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/exercises/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/tutor/exercises.py; knowledge/exercises/ (executable reference, tests and scale generator fields); scripts/check-card-code.py (checker migration coordinated with rust-27)
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-22`, `heavy-run cargo xtask migration verify --step rust-22`, `heavy-run --exclusive cargo xtask migration measure --step rust-22 --pairs 30`, then `cargo xtask migration decide --step rust-22`. Commit the baseline and prediction before implementation.
  - **Success**: 0 correct-reference rejections in 10 fresh 8-worker batches; 100% wrong-class negative controls rejected; 0 sandbox escapes or leaked private paths. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port local voice, model routes and judge adapters
  - **ID**: rust-23
  - **Blocked**: prerequisite: rust-05, rust-21 must land and pass its acceptance gates first
  - **Details**: Port existing whisper-cli/ffmpeg process adapters, text fallback, local-first model routing, cache keys, bounded requests and judge/owner-label agreement. Do not change speech engine/model or manufacture owner labels; keep audio/transcripts ephemeral. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-23` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `tests/fixtures/voice/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/tutor/voice.py; models.py; judge.py; config/models.yaml
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-23`, `heavy-run cargo xtask migration verify --step rust-23`, `heavy-run --exclusive cargo xtask migration measure --step rust-23 --pairs 30`, then `cargo xtask migration decide --step rust-23`. Commit the baseline and prediction before implementation.
  - **Success**: 100% synthetic process/request contracts agree; 0 retained audio/transcripts or route/policy bypasses. Existing owner-label agreement bar remains unchanged. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port reports, metrics, checkups and remaining domain helpers
  - **ID**: rust-24
  - **Blocked**: prerequisite: rust-06, rust-17, rust-18, rust-19, rust-23 must land and pass its acceptance gates first
  - **Details**: Port report builders and lifecycle, charts, metrics exports, review outcomes, checkups, wizards, checklist state and changes.check/pivot orchestration. Reuse the already-native event/hash primitives; do not duplicate their implementations. Preserve public allowlists, local business detail, stable cache keys and current UI templates. Audit the inventory for any remaining domain module. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-24` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/{report,runs,charts,metrics_share,repo_health,changes,events,telemetry,field,time_saved,signals,review,checkup,checkup_runs,wizard,checklist}.py
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-24`, `heavy-run cargo xtask migration verify --step rust-24`, `heavy-run --exclusive cargo xtask migration measure --step rust-24 --pairs 30`, then `cargo xtask migration decide --step rust-24`. Commit the baseline and prediction before implementation.
  - **Success**: 100% report/metric/export contracts agree; 0 new public fields, content-bearing telemetry or missing long-run pages. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Replace the HTTP server behind the unchanged UI
  - **ID**: rust-25
  - **Blocked**: prerequisite: rust-14, rust-16, rust-17, rust-18, rust-19, rust-20, rust-21, rust-22, rust-23, rust-24 must land and pass its acceptance gates first
  - **Details**: Port routes, error envelopes, request limits, action dispatch, authentication, cookies/origins, static serving, cache/change versions and concurrent request behavior. Keep React views and TypeScript UI tooling/browser tests. Replace backend-specific launch/probe commands inside retained browser tests (for example signin.spec.ts) with native CLI/test adapters; port their generated shell stubs and seed helpers in rust-27. Run the real built UI against the native server. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-25` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns the server-facing web files `web/src/api.ts`, `web/vite.config.ts`, `web/playwright.config.ts`, `web/playwright.promise.config.ts`, `web/playwright.visual.config.ts`, `web/e2e/signin.spec.ts`, `web/e2e/pm-slack-fixture.ts` and `web/e2e/mcp-repair-fixture.ts` (they stay TypeScript; keep them working against the native server) ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs. Port this capability's CLI handlers and the assertions of its test files in this step, and route its own commands to native code when its activation closure is native; do not defer them to rust-26 or rust-27 ("Every module has an owner step" lists them). A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/server.py; web/src/api.ts contract; web/e2e/
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-25`, `heavy-run cargo xtask migration verify --step rust-25`, `heavy-run --exclusive cargo xtask migration measure --step rust-25 --pairs 30`, then `cargo xtask migration decide --step rust-25`. Commit the baseline and prediction before implementation.
  - **Success**: 100% HTTP/action contracts and existing browser stories pass; 0 auth/origin bypasses, burst resets or idle refetch churn; 0 intended visual changes. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Complete native CLI coverage and default entry point
  - **ID**: rust-26
  - **Blocked**: prerequisite: rust-05, rust-06, rust-07, rust-08, rust-09, rust-10, rust-12, rust-13, rust-14, rust-15, rust-16, rust-17, rust-18, rust-19, rust-20, rust-21, rust-22, rust-23, rust-24, rust-25 must land and pass its acceptance gates first
  - **Details**: Each domain step ports and switches its own CLI handlers ("CLI handlers: `cli.py` is split across the steps" in `docs/rust-migration.md`). This step replaces only argument parsing, the dispatcher and the action registry wiring, and makes the final switch of the default entry point; it is not a big-bang handler cutover. Run the generated action inventory, argument-free UI path, confirmation and terminal-only rules and each CLI action story against native binaries. Switch the default launcher only after full parity. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-26` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns the start-command text in `web/src/act.ts`, `web/src/act-status.test.tsx` and `web/e2e/feedback.spec.ts` (they stay TypeScript) ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs; do not defer their tests to the final harness task. A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: src/careerbrew/cli.py; actions.py; __main__.py; commands/; Agentfile.yaml
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-26`, `heavy-run cargo xtask migration verify --step rust-26`, `heavy-run --exclusive cargo xtask migration measure --step rust-26 --pairs 30`, then `cargo xtask migration decide --step rust-26`. Commit the baseline and prediction before implementation.
  - **Success**: 100% CLI actions have native handlers and required story coverage; 0 commands still routed to Python; 0 Python subprocesses outside explicitly selected Python exercise execution. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Port developer tools, test harnesses, installers and release checks
  - **ID**: rust-27
  - **Blocked**: prerequisite: rust-00 must land first to start rust-27a and rust-27b; rust-26 must land and pass its acceptance gates before the step finishes
  - **Details**: Move first-party executable checks, HTTP fixture servers, synthetic fakes, CLI story assertions, eval orchestration, batching, cache keys, setup, doctor, leak checks, metrics/proof and release packaging to Rust xtask/integration tests. Domain steps port the assertions for their own capability; this step owns only the shared harness and the assertions no domain step owns. Work in five sub-steps: rust-27a shared test harness, rust-27b developer scripts/hooks/installers, rust-27c e2e stories, rust-27d evals, rust-27e CI workflows (each with its own wait and gate in the plan). Retain browser tests in TypeScript. Inline shell in docs/hooks becomes a native command; only an explicitly inventoried minimal OS bootstrap may remain. Port the embedded Node/esbuild backend checker in check-card-code.py too; retained UI TypeScript is not an exception for this maintenance implementation. Preserve syntax validation of Python teaching examples using a maintained native parser validated against the current compile() oracle, including invalid-code controls; never silently skip those examples. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-27` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `evals/cases/`, `evals/rubric.md`, `evals/README.md`, `tests/fixtures/claims/` and `.github/workflows/` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs; do not defer their tests to the final harness task. A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Progress**: rust-27b slice 1 (2026-11-18): `cargo xtask leak-check` ports the repo scan engine of `scripts/leak-check.sh` over a supplied terms file; the wrapper still runs Python until the encrypted terms source (rust-04/05/07) and the staged snapshot PNG scan are native. Slice 2 (2026-11-19): `cargo xtask input-key` ports `scripts/input-key.sh` (gate cache key) with script parity tests in `xtask/tests/input_key.rs` (also odd file names, a linked worktree, pathspecs with no `--`, and the pinned leading-`--` gap); the script stays the entry point. Slice 3 (2026-11-20): `cargo xtask check-docs` ports `scripts/check-docs.sh` (docs drift check) with script parity tests in `xtask/tests/check_docs.rs` over synthetic temp repos (missing rows, missing and unlisted stories, wrong story ids, the BRE `.` wildcard, missing index files); the script stays the gate's entry point.
  - **Files**: scripts/; hooks/; evals/run.py; evals/run.sh; tests/unit/; tests/e2e/; lefthook.yml; INSTALL.md
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-27`, `heavy-run cargo xtask migration verify --step rust-27`, `heavy-run --exclusive cargo xtask migration measure --step rust-27 --pairs 30`, then `cargo xtask migration decide --step rust-27`. Commit the baseline and prediction before implementation.
  - **Success**: 100% old assertions mapped to native tests or a reviewed stronger replacement; 0 lost negative controls; clean install/doctor/fast/full/eval gates pass without Python. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Rust migration: Retire the old implementation and verify fresh-machine releases
  - **ID**: rust-28
  - **Blocked**: prerequisite: rust-26, rust-27 must land and pass its acceptance gates first
  - **Details**: Delete retired Python/shell implementation only after native parity. Remove Python runtime/dev dependencies, obsolete launchers and import paths. Package supported-target native binaries, checksums and dependency/license inventory. Test clean and existing-data installs, startup/watch/shutdown, UI, backup restore and safe downgrade. Keep an immutable last-Python release for rollback, not a hidden runtime fallback. Follow the measured Rust migration plan in `docs/rust-migration.md`: step `rust-28` in "Work breakdown" (its inputs, outputs and gate), "Scope", and "Measurement protocol for every step". This step also owns `pyproject.toml`, `uv.lock`, `.python-version` and `src/careerbrew/py.typed` ("Every module has an owner step" in the same page). Split large capabilities into independently measured PRs; do not defer their tests to the final harness task. A component-only result does not enable production dispatch: the plan's full native activation closure must be measured before switching.
  - **Files**: Cargo workspace; package/install metadata; all remaining inventory rows; README.md; INSTALL.md; AGENTS.md
  - **Hypothesis**: replacing this slice preserves 100% of its registered behavior with 0 privacy/policy/side-effect violations; native CPU-time p95 and peak RSS stay at or below 1.10 times measured baseline B. Preparation steps instead prove their harness/dependency controls before any product switch. No current speed result is claimed.
  - **Measurement**: After rust-00 creates the harness: `heavy-run --exclusive cargo xtask migration baseline --step rust-28`, `heavy-run cargo xtask migration verify --step rust-28`, `heavy-run --exclusive cargo xtask migration measure --step rust-28 --pairs 30`, then `cargo xtask migration decide --step rust-28`. Commit the baseline and prediction before implementation.
  - **Success**: 0 unassigned first-party executable files; 0 Python app/developer-tool requirements; 100% supported install/upgrade/rollback and whole-repo gates pass. All applicable shared parity/resource gates pass; whole-repo fast/full checks remain green.
  - **Pivot**: any contract disagreement, privacy violation or missing/duplicate write, or CPU/RSS ratio >1.10 B, blocks rollout. Rework the adapter or dependency, register the new approach and remeasure; keep the last verified handler. Preparation fails on any accepted planted defect or unsupported required contract.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration); Kalibera & Jones 2013 (rigorous benchmarking).

## P1

- [ ] Pro data: a free sample of maintained data in the open tool
  - **ID**: pro-data-free-sample
  - **Details**: First task of the careerbrew income bet (a yearly licence for maintained data, tested by a priced waitlist). Ship a small free sample the open tool reads through the signed-plugin path: one public company ladder mapped to levels, and three sourced interview loops from `knowledge/research/`, each with a dated changelog line. The promotion and interview run pages show one static local link to the waitlist page (`pro_data_url` setting, empty by default, so nothing shows until the owner sets it). No hosted service, no network call, no buyer data.
  - **Files**: knowledge/research/; src/careerbrew/plugins.py; src/careerbrew/runs.py; src/careerbrew/config.py; tests/unit/; docs/plugins.md; README.md
  - **Hypothesis**: with a sample in the tool and a link on two run pages, page visitors who come from the tool are more than 0, so the waitlist reading can tell "no traffic" from "no demand".
  - **Measurement**: `gh pr checks <pr>` (a unit case: the sample loads through the plugin path, and the link shows only when `pro_data_url` is set); in use, the owner reads unique page visitors (cookie-free page analytics) beside each waitlist reading.
  - **Success**: the cases pass; 300 or more unique page visitors by the week-4 reading.
  - **Pivot**: under 300 visitors by week 4, repeat the launch once before the reading counts; if the sample needs a hosted part, stop and keep the tool local.
  - **Anchor**: Ries 2011 (minimum viable product, pivot or persevere); Kohavi, Tang & Xu 2020 (guardrail beside the primary metric).

- [ ] Pro data: the waitlist page with a double opt-in form
  - **ID**: pro-data-waitlist-page
  - **Details**: One static page: what the yearly data licence holds, its price for one person and for a team, the first data release date, and a form (email plus a required choice of "individual" or "team"). The page takes no money. The owner creates the mailing-list and page-analytics accounts and types the form address; an agent drafts the page and fills everything else. Comfort budget applies (phone and desktop, light and dark, 44 px targets, contrast 4.5:1).
  - **Files**: docs/ (page source, where the owner picks); web/e2e/ or a page test
  - **Blocked**: owner step: owner creates the two accounts and approves publishing the page (a public post under the owner's name).
  - **Hypothesis**: a priced page with a confirmed-email form gives one readable number, confirmed subscribers, that moves from 0 only when someone wants the paid data.
  - **Measurement**: the owner reads the confirmed subscriber count (own addresses removed) and records it with `careerbrew indie measure careerbrew --value <n>` on the two reading dates; the page passes an axe run with 0 serious problems.
  - **Success**: 30 confirmed subscribers by the week-8 reading.
  - **Pivot**: under 5 at two readings in a row holds the bet.
  - **Anchor**: Ries 2011 (smoke test before build); Munafo et al. 2017 (pre-registration).

- [ ] Pro data: launch drafts for the owner to post
  - **ID**: pro-data-launch-drafts
  - **Details**: Agent-drafted, owner-posted: one post each for Hacker News, Lobsters and two Reddit communities, plus a write-up "a promotion doc from your git history in 10 minutes" built from personal repos only. Drafts stay local until the owner posts them; an agent never posts. No xAI or X channels.
  - **Files**: local drafts only (no tracked file); docs/ for the write-up after the owner approves
  - **Blocked**: owner step: owner reads, edits and posts each draft.
  - **Hypothesis**: four posts and one write-up bring 300 or more unique visitors to the page in 4 weeks.
  - **Measurement**: the owner reads unique page visitors from the page analytics; `gh api repos/{owner}/{repo}/traffic/views -q .uniques` saved each week for the repo.
  - **Success**: 300 or more unique page visitors by week 4.
  - **Pivot**: under 300 after one repeat launch, change the channel, not the offer.
  - **Anchor**: Ries 2011 (pivot or persevere); Doerr 2018 (outcomes, not activities).

- [ ] Measure and register the rust-03 noise floors on hosted macOS and Linux
  - **ID**: rust-00-measure-noise-floors
  - **Details**: `xtask/src/noise_floor.rs` registers `rust-03` as `unmeasured` on both OSes, so `decide` uses the 1.10 ratio for every case. On a clean tree on each hosted OS, run `heavy-run --exclusive cargo xtask migration baseline --step rust-03 --python "$PWD/.venv/bin/python" --out <new-file>` twice, then `cargo xtask migration noise-floor --baseline <file>` on each record. Register the result as `Floor::Measured` with its method and record (run id, source revision), per docs/rust-migration.md "Noise floors and normalization rules".
  - **Files**: xtask/src/noise_floor.rs; docs/rust-migration.md; .github/workflows/
  - **Progress**: 2026-11-20: `noise-floor.yml` ran on `main` for `rust-03` (run 44733269058) and `rust-04` (run 44733273092). The pairs agree within 3x (Linux 2.6x to 2.8x, macOS within 1.1x), but every proposed CPU floor is larger than the whole oracle cost: macOS about 2.1 s to 2.5 s, Linux about 0.09 s to 0.30 s, against an oracle CPU p95 near 0.12 s. Below a floor, `decide` checks only the ceiling, so registering these would activate a candidate 10 to 20 times slower. Not registered: every floor stays `unmeasured` and `decide` keeps the 1.10 ratio. Left: a tighter method (a bootstrap interval on the paired ratio, or more samples on a quieter runner), then re-run and register.
  - **Hypothesis**: the two baselines per OS give CPU and RSS floors within 3x of each other, so one registered floor per OS is stable enough for `decide`.
  - **Measurement**: `cargo xtask migration noise-floor --baseline <file>` on two hosted baseline records per OS; compare `cpu_floor_us` and `rss_floor_bytes`.
  - **Success**: both pairs agree within 3x; the larger value of each pair is registered as `measured`, and `cargo test -p careerbrew-xtask --test decide` stays green.
  - **Pivot**: a pair differs by more than 3x: drop the p95-p5 method and register a bootstrap interval instead.
  - **Anchor**: Kalibera & Jones 2013, *Rigorous Benchmarking in Reasonable Time*.

- [ ] README Proof numbers come from hosted CI, not a local gate
  - **ID**: proof-metrics-from-hosted-ci
  - **Details**: `scripts/metrics.sh proof` writes `docs/proof/metrics.json` and the README Proof section from a local green `scripts/check.sh` and `scripts/e2e.sh` (`docs/metrics.md` "README proof"). Local test runs are blocked now (`hooks/block-local-tests.sh`, PR #108), so every proof number is stuck at its 2026-10-26 date. Make the hosted gate upload the inputs `metrics.sh proof` reads (e2e results, gate step events, eval and leak counts) as an artifact. Then make `metrics.sh proof` read them from a named run (`gh run download`), or add a `workflow_dispatch` job that runs the proof step and opens a PR with the refreshed files. Label each number with the run id and runner OS, never a machine of the owner. Keep `tests/unit/test_proof.py` (README and JSON must match). Coordinate with the open PR that owns `docs/ci.md` and `.github/workflows/`.
  - **Files**: scripts/metrics.sh; scripts/proof.py; .github/workflows/; docs/metrics.md; docs/proof/metrics.json; tests/unit/test_proof.py
  - **Hypothesis**: proof numbers built from hosted artifacts take the age of the README Proof section from 21+ days (2026-10-26 on 2026-11-16) to 7 days or less, with no local test run.
  - **Measurement**: `jq -r '.metrics[].date' docs/proof/metrics.json | sort | head -1` compared with today's date, and `rg -c 'run [0-9]+' docs/proof/metrics.json` for the run-id label on each metric.
  - **Success**: every metric in `docs/proof/metrics.json` names a hosted run id and is at most 7 days old, and `test_proof.py` passes on the hosted gate.
  - **Pivot**: if the hosted artifacts cannot give a metric's inputs (for example the multi-day gate pass rate from `.metrics/events.jsonl`), drop that metric from the Proof section rather than keep a stale number.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (Goal-Question-Metric); Forsgren, Humble & Kim 2018, *Accelerate* (measure from the delivery pipeline).

- [ ] The us-58 loading-label check can miss its 100 ms limit on hosted macOS
  - **ID**: flaky-us58-loading-label-macos
  - **Details**: On 2026-11-17 the hosted `e2e-1 (macos-15)` job of PR 116 (head `001ddab6`, run 44115274163) failed in `tests/e2e/us-58-action-feedback.sh` (`web/e2e/feedback.spec.ts`, Goals): "ms from opening the view to a visible loading label", expected < 100, received 118.2. The PR changed only xtask and docs. A rerun of the failed job passed. Find what the first paint waits behind on a slow runner (a font, a first API call, the bundle parse). Fix that cause; a looser limit is the last resort.
  - **Files**: web/e2e/feedback.spec.ts; tests/e2e/us-58-action-feedback.sh; web/src/views.tsx
  - **Hypothesis**: with the cause fixed, the hosted macOS us-58 loading-label check fails 0 times in 20 runs, against 1 seen so far.
  - **Measurement**: `gh run list --workflow ci.yml -L 20 --json conclusion,headSha` plus `gh run view <id> --log-failed | rg 'loading label'` for each failed run.
  - **Success**: 0 failures of this check in the next 20 hosted macOS runs.
  - **Pivot**: if it still fails 2 or more times in 20 runs after the fix, measure the label time relative to a same-page baseline paint instead of an absolute 100 ms.
  - **Anchor**: Luo et al. 2014, "An empirical analysis of flaky tests" (FSE; timing-dependent tests).

- [ ] Fail the gate when the allowlist .gitignore hides a new xtask source file
  - **ID**: gitignore-xtask-new-files-check
  - **Details**: On 2026-11-17 PR 116 pushed `mod baseline;` without `xtask/src/baseline.rs` and `xtask/tests/baseline.rs`: `.gitignore` lines `/xtask/src/*` and `/xtask/tests/*` ignore every new file unless it has a `!` line, and `git add -A` skipped both in silence. The hosted `rust-fmt` step failed with "failed to resolve mod `baseline`". Add a gate step (or a pre-commit check) that lists `git ls-files --others --ignored --exclude-standard xtask/src xtask/tests` for `.rs` files and fails with the `!` line to add.
  - **Files**: scripts/check.sh; .gitignore; tests/unit/test_gate_inputs.py
  - **Hypothesis**: with the check, an ignored new xtask source file is caught before push in 100% of cases, against 0% now (1 hosted failure seen).
  - **Measurement**: a unit test that creates an ignored `xtask/src/new.rs` in a scratch repo and expects the check to fail; `gh run list` for later `failed to resolve mod` errors.
  - **Success**: the planted control fails the check, the clean tree passes, and 0 hosted `failed to resolve mod` errors in the next 20 runs.
  - **Pivot**: if the check gives false alarms on build output under xtask, switch to comparing `mod` declarations in tracked files with tracked paths.
  - **Anchor**: Reason 1990, *Human Error* (defence in depth); Forsgren, Humble & Kim 2018 (DORA change failure rate).

- [ ] The macOS browser control inventory can time out on a 15 s element wait
  - **ID**: flaky-controls-inventory-macos
  - **Details**: On 2026-11-16 the hosted `gate (macos-15)` job of PR #110 (head `16b1aefd`) failed in `web/e2e/controls.spec.ts:287` ("inventory on a ... machine: each control is in controls.json with a test that exists"): a 15 s wait for an element ran out. A rerun passed with no change. Read the failed log with `gh run view <run-id> --log-failed` and its Playwright trace first. Find which element the wait was for and what it waited behind (a slow first API call, a cached block refresh). Fix the cause; a longer timeout is the last resort (AGENTS.md "A wall-clock limit in a test is a hang guard").
  - **Files**: web/e2e/controls.spec.ts; web/e2e/controls.json
  - **Hypothesis**: with the cause fixed, the hosted macOS `controls.spec.ts` inventory fails 0 times in 20 runs, against 1 seen so far.
  - **Measurement**: `gh run list --workflow ci.yml -L 20 --json conclusion,headSha` plus `gh run view <id> --log-failed | rg 'controls.spec.ts'` for each failed run.
  - **Success**: 0 failures of this test in the next 20 hosted macOS runs.
  - **Pivot**: if it still fails 2 or more times in 20 runs after the fix, mark the test serial on macOS and split the inventory per view.
  - **Anchor**: Luo et al. 2014, "An empirical analysis of flaky tests" (FSE; async wait is the top cause).

- [ ] The Day log unit test "shows 10 suggestions at a time" saw 10 items where it expected 4
  - **ID**: flaky-daylog-suggestions-filter
  - **Details**: In hosted CI run 43992348423 the web unit test `web/src/daylog.test.tsx` "shows 10 suggestions at a time with a kind filter, and Housekeeping folded with its count" failed in the `checks (ubuntu-24.04)` part; `gh run view 43992348423 --log-failed` shows `AssertionError: expected [ …(10) ] to have a length of 4 but got 10` (after the click on "goal (4)"). The assert runs right after `fireEvent.click`, so the filtered list may not have rendered yet, or the hash change did not reach the panel. Reproduce on hosted CI with a repeat run, then wait for the filtered state (`findAllByRole`, or wait for the "Showing" text) instead of a sync read.
  - **Files**: web/src/daylog.test.tsx; web/src/daylog.tsx
  - **Hypothesis**: an async wait for the filtered list takes this test's hosted failure rate from 1 seen to 0 in 20 runs.
  - **Measurement**: `gh run list --workflow ci.yml -L 20` and `gh run view <id> --log-failed | rg 'shows 10 suggestions'`.
  - **Success**: 0 failures in the next 20 hosted runs.
  - **Pivot**: if it fails again with the async wait, the bug is in the filter itself, not the test: stop changing the test and write a failing reproduction of the filter.
  - **Anchor**: Luo et al. 2014, "An empirical analysis of flaky tests" (FSE).

- [ ] The gitleaks download in the public-readiness job can fail the job
  - **ID**: gitleaks-download-cache
  - **Details**: Hosted run 43992355392 failed in the `e2e-2 (ubuntu-24.04)` part at the gitleaks download, not the scan; `gh run view 43992355392 --log-failed` shows `curl: (22) The requested URL returned error: 502`. Both `.github/workflows/gate-part.yml` and `.github/workflows/public-readiness.yml` download the release; the step already pins `GITLEAKS_VERSION` and checks `GITLEAKS_SHA256`. Cache the checked tarball with `actions/cache` keyed by version and hash, or retry the download a bounded number of times; keep the SHA-256 check. Coordinate with the open PR that owns `docs/ci.md` and `.github/workflows/`.
  - **Files**: .github/workflows/gate-part.yml; .github/workflows/public-readiness.yml; docs/ci.md
  - **Hypothesis**: a cached, hash-checked binary takes gitleaks download failures in hosted runs from 1 seen to 0 in 30 runs.
  - **Measurement**: `gh run list -L 30 --json conclusion,databaseId,workflowName` and `gh run view <id> --log-failed | rg -i 'gitleaks|curl'`.
  - **Success**: 0 gitleaks download failures in 30 runs, and the SHA-256 check still runs on every cache miss.
  - **Pivot**: if the cache adds more than 20 s to the job or the hash check can be skipped on a hit, drop the cache and use a bounded retry.
  - **Anchor**: Forsgren, Humble & Kim 2018, *Accelerate* (change failure rate); SLSA v1.0 (pinned, verified build inputs).

- [ ] `rust-00`: observe the processes a run really starts, for the inventory
  - **ID**: rust-00-inventory-process-observation
  - **Details**: The inventory finds executable paths, embedded code and entry points by reading files (`inventory --check`, `--sites-check`). It does not see an interpreter or tool a run starts at runtime from a computed path. Trace the exec calls of the gate's own stories with the Linux observer (`docs/rust-migration.md` "Linux observer") and compare them with the inventory.
  - **Files**: xtask/src/inventory.rs; xtask/src/observer.rs; docs/plans/2026-11-01-rust-code-inventory.csv; docs/rust-migration.md
  - **Hypothesis**: 100% of the exec targets in the traced stories map to an inventory row; a planted exec of an unlisted script is reported as unassigned.
  - **Measurement**: `heavy-run --exclusive cargo xtask migration observer-probe --image <id>` with an inventory comparison step (to be built), plus a planted unlisted-exec control.
  - **Success**: 0 unassigned exec targets on the traced stories, and the planted control is reported.
  - **Pivot**: if tracing the stories costs more than 3x the gate's time, trace a fixed sample of stories and record that the observation is partial.
  - **Anchor**: Linux `strace` exec tracing; Basili, Caldiera & Rombach 1994 (GQM).
  - **Progress**: slice 1 done: `cargo xtask migration inventory --exec-check <trace-file> --root <dir>` (`xtask/src/exec_map.rs`) maps exec targets to inventory rows and reports the planted unlisted script. Left: extract `execve` targets from an observer trace of the gate stories, run it, and record the unassigned count.

- [ ] `rust-00`: count descendants that leave the process group
  - **ID**: rust-00-process-tree-escapees
  - **Details**: The tree collector counts only process-group members (`docs/rust-migration.md` "Process-tree resource accounting"). A descendant that calls `setsid` or `setpgid`, and a parent that left the group, are not counted; a parent that never exits keeps the Linux call polling. A cgroup v2 scope per call (Linux) would close this gap.
  - **Progress**: 2026-11-20: Linux cgroup v2 CPU scope with a fallback, fixture mode `escape`, control `a_descendant_that_calls_setsid_is_counted_by_the_cgroup` (records `supported=`). Left: read the hosted `ubuntu-24.04` record; on `supported=false`, apply the pivot. macOS escapees and escapee peak RSS stay uncounted.
  - **Files**: xtask/src/resources.rs; xtask/tests/resources.rs; docs/rust-migration.md
  - **Hypothesis**: a cgroup collector counts a `setsid` descendant's 300 ms CPU burn within 0.75x to 1.5x on Linux, where the group collector reports about 0.
  - **Measurement**: `cargo test -p careerbrew-xtask --test resources` on hosted `gate (ubuntu-24.04)`, with a `setsid` burn control.
  - **Success**: the control counts 225 to 450 ms and all existing controls pass.
  - **Pivot**: if hosted runners cannot create a delegated cgroup, document the gap as unsupported and stop.
  - **Anchor**: Linux cgroup v2 (`cpu.stat`, `memory.peak`); Kalibera & Jones 2013 (ISMM).

- [ ] careerbrew uses levels.fyi data for job search and interview prep
  - **ID**: levels-fyi-integration
  - **Details**: Integrate levels.fyi sources and tools end to end: (1) job finding: its job listings and compensation data feed `careerbrew-job-hunt` ranking (pay band, level mapping across companies); (2) interview prep: its interview and leveling guides feed `careerbrew-interview-coach` and `knowledge/research/interview-research.yaml` strategy rules. First check its terms of use and any official API or export; record the allowed access per source in the `docs/integrations.md` terms column. If a source forbids automated access, use only what the owner opens by hand (browser research skill) and say so. Add a platform in `src/careerbrew/jobs/platforms.py` with an HTTP fixture in `tests/fixtures/http/`, a story and e2e test, and an eval case. Job-hunt commands stay blocked on a corporate machine.
  - **Files**: src/careerbrew/jobs/platforms.py; skills/careerbrew-job-hunt/; skills/careerbrew-interview-coach/; knowledge/research/; docs/integrations.md; docs/user-stories.md; tests/
  - **Hypothesis**: adding levels.fyi pay and level data raises the share of ranked job matches that carry a pay band and a mapped level from the current share to 80% or more, and adds sourced interview strategy rules for the top 10 target companies.
  - **Measurement**: `uv run careerbrew jobs` summary counts of matches with a pay band and level (synthetic fixture run in the story e2e), and `scripts/check-sources.py` count of levels.fyi-sourced rules.
  - **Success**: 80% or more of fixture matches carry pay and level; 10 or more sourced strategy rules pass `check-sources.py`.
  - **Pivot**: if the terms allow no automated access and manual import covers under 30% of matches, drop the platform and keep only cited guide links in `knowledge/`.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Kohavi, Tang & Xu 2020 (trustworthy online experiments).

- [ ] `scripts/check.sh` and `scripts/e2e.sh` queue themselves through heavy-run
  - **ID**: gate-scripts-self-queue
  - **Details**: On 2026-11-15 an unqueued `scripts/public-readiness.sh --first-run` (which runs the fast gate) ran beside `dotfiles apply` and a real-time app, and the Mac froze (forced off, no memory pressure). dotfiles PR 713 makes the agent hook refuse unwrapped gate scripts, but the hook guards only agent shells: a terminal, a git hook or another agent host still runs them unqueued. `scripts/batch-gate.sh` already wraps its gate in `heavy-run` (lines 64-65). Make `check.sh` and `e2e.sh` re-exec themselves under `heavy-run` when it is installed and `HEAVY_RUN_HELD` is unset; keep `check.sh inputs` and `--help` unqueued. Hosted runners have no `heavy-run` and must run as now.
  - **Files**: scripts/check.sh; scripts/e2e.sh; tests/unit/test_gate_inputs.py (or a new test)
  - **Hypothesis**: a self-queuing gate takes the count of unqueued full-gate runs from any entry point from "every non-agent run" to 0, with no change on hosted runners.
  - **Measurement**: a unit test with a stub `heavy-run` on PATH that records its calls: `scripts/check.sh fast` and `scripts/e2e.sh` from a plain shell call the stub once each; with `HEAVY_RUN_HELD=1` or no stub they do not; `scripts/check.sh inputs unit` never does. `~/.local/state/dotfiles/heavy-run-waits.log` shows each local gate run.
  - **Success**: the test passes, and the hosted `gate` jobs stay green.
  - **Pivot**: if re-exec breaks exit codes, signals or the pre-push hook, drop it and keep only the agent hook plus a doctor warning.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA change failure rate); Reason 1990, *Human Error* (defence in depth).

- [ ] Prove the company plugin works on the corporate machine after the move to the new history
  - **ID**: company-plugin-after-history-move
  - **Blocked**: a step only the corporate machine can do: the plugin and its tools live there. Check: `git -C "$(uv run careerbrew where)" rev-list --max-parents=0 HEAD` prints `22e3e591b70dd8a5f8a1111355201b85c8f68ed5` on that machine.
  - **Details**: The repository moved to a one-commit history on 2026-11-12. A company plugin lives in its own repository and its interface did not change, so it must work as before. Steps on the corporate machine: (1) before the move, run `uv run careerbrew machine ready` and note N in `corporate readiness: N/N`. (2) Follow `docs/migrate-to-public-history.md`. (3) Run `scripts/setup.sh`, then `scripts/doctor.sh`. (4) Run `uv run careerbrew machine ready` again. (5) Run `uv run careerbrew day start --update-only`. Never paste plugin names, company names or command output with work data into an issue: report counts only. Do the same on each other machine that uses careerbrew (4 machines in total), without step 1 and step 4 where no plugin is installed.
  - **Files**: docs/migrate-to-public-history.md; scripts/migrate-history.sh
  - **Hypothesis**: the move changes no plugin behaviour, because the plugin folders are ignored files and the script leaves ignored files in place.
  - **Measurement**: `uv run careerbrew machine ready` before and after the move, and `scripts/doctor.sh`.
  - **Success**: the readiness line shows the same N/N after the move as before, the doctor ends with 0 failed, and `day start --update-only` no longer reports `diverged`.
  - **Pivot**: if readiness drops, restore the old checkout from the bundle the script wrote, file the failing check by its id, and fix the script before the next machine moves.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA change failure rate).

- [ ] Measure the PM why-it-matters notes and the named results in real use
  - **ID**: pm-why-measure
  - **Blocked**: time: needs 2 weeks of PM updates from a user in a real workspace after the merge on 2026-10-27; run the measurement on or after 2026-11-10
  - **Details**: PR for task pm-why-it-matters added owner-only "why it matters" notes per PM update line and named every result per project (no line cap). The mechanics are tested; the outcome needs 2 weeks of use by a user in a real workspace. Compare added-context PM edits before and after the merge day.
  - **Hypothesis**: a user's edits to the PM draft that add context drop, because the draft carries the reason and the full work already.
  - **Measurement**: `uv run careerbrew signals --json` (PM edit counts per day, issue #185), 2 weeks before vs 2 weeks after the merge.
  - **Success**: added-context edits fall by half over 2 weeks.
  - **Pivot**: if edits do not fall, ask the user which lines they rewrite and why before adding more text.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Verify teacher mode in real use: full LeetCode sync and self-fixing
  - **ID**: teacher-mode-verify
  - **Blocked**: a step only a user can do (the cookie sync uses an unofficial endpoint, may be against the site's terms, and is off by default): a user puts the browser's LeetCode cookies in `.env` (`LEETCODE_SESSION`, `LEETCODE_CSRF`), runs `uv run careerbrew exercises sync --leetcode <username>`, then uses Teach me daily for 4 weeks; an agent cannot read a user's browser cookies or practice for a user
  - **Details**: Teacher mode shipped on branch `dev/sleepy-noether-xmtb6s` (exercises, mistakes, goals queue, job choice, auto-fixes). Two things need a user: (1) put the browser's `LEETCODE_SESSION` and `csrftoken` cookies in `.env` as `LEETCODE_SESSION` / `LEETCODE_CSRF`, run `uv run careerbrew exercises sync --leetcode <username>`; the cookie path is tested only against a fake response (the public list returned no recent solves for a profile with solves). (2) Use Teach me daily and watch `careerbrew teach improvements`.
  - **Hypothesis**: the cookie sync marks every solved curriculum problem, and the auto-fixes keep repeat mistakes rare, because watch-out tips show before each attempt.
  - **Measurement**: `uv run careerbrew exercises sync --leetcode <username>` and `uv run careerbrew teach improvements`.
  - **Success**: sync marks > 0 solved; mistake_repeat_rate < 15% after 4 weeks.
  - **Pivot**: if the cookie query fails, switch to the LeetCode `userProgressQuestionList` query; if repeats stay high, show the watch-out tip as a required checklist.
  - **Anchor**: Roediger & Butler 2011; Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Use careerbrew for 4 weeks in work mode and record the outcome metrics
  - **ID**: dogfood-work-mode
  - **Blocked**: owner step: a user runs work mode for 4 weeks in a real workspace with its work sources connected, with a weekly `uv run careerbrew review`; an agent cannot do the work weeks
  - **Details**: A real workspace's sources (git, code review, tickets). Weekly `careerbrew review`.
  - **Hypothesis**: weekly evidence capture lifts promo_coverage by at least 2 points a week, because invisible work gets recorded.
  - **Measurement**: `uv run careerbrew review` (gap_closure, promo_coverage in private/review.jsonl).
  - **Success**: gap_closure >= 0.02 per week on average over 4 weeks.
  - **Pivot**: gap_closure <= 0 for 3 weeks: re-weight the ladder.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Doerr 2018.

- [ ] Refuse redirects on model calls that carry a key
  - **ID**: model-call-redirect-guard
  - **Tags**: security, corporate, open-source
  - **Details**: `_http_transport` in `src/careerbrew/models.py` opens the configured model URL with `urllib.request.urlopen`, which follows redirects and keeps the `Authorization` header. The corporate rule (model calls only to a local address) checks only the first URL. An outside review of the public tree noted this as a hardening gap; no real leak was shown. Steps. (1) Test first, in a new file `tests/unit/test_model_redirects.py` (add its row to `docs/plans/2026-11-01-rust-code-inventory.csv`; `heavy-run cargo xtask migration inventory --check` names it) with a local server like the one in `tests/unit/test_redirect_host_policy.py`: a model call with a key whose server answers 302 to another origin raises an error and the second server sees no request; a redirect on the same origin still works; on a corporate role a redirect from a local address to a non-local one is refused also with no key. (2) Fix: open the request through a redirect handler with the same rules as `connectors._PolicyRedirect` (no token to another origin, no HTTPS to plain HTTP) and run the corporate local-address check on each redirect target. Reuse the handler; do not copy it. (3) `src/careerbrew/models.py` line numbers are pinned in the three Rust activation tables (`2026-11-04-rust-activation-manifest.tsv`, `-providers.tsv` and `-workday.tsv` in the plans folder under `docs/`): after the edit, update the pinned lines until `heavy-run cargo test -p careerbrew-xtask --test activation_manifest` passes. (4) Add one sentence to the model row of `docs/privacy.md`.
  - **Files**: `src/careerbrew/models.py`, `tests/unit/test_model_redirects.py`, `docs/plans/2026-11-01-rust-code-inventory.csv`, `docs/privacy.md`
  - **Hypothesis**: model calls that send a key to a second origin after a redirect fall from possible (1 path today) to 0, because each redirect target gets the same checks as the first URL.
  - **Measurement**: `uv run pytest tests/unit/test_model_redirects.py -q`.
  - **Success**: the new tests fail before the fix and pass after it; 0 requests reach the second server in the test.
  - **Pivot**: if a supported local model server needs a cross-origin redirect to work, allow only same-host redirects without the key and document it.
  - **Anchor**: Saltzer & Schroeder 1975 (complete mediation); Basili, Caldiera & Rombach 1994 (GQM).

## P2

- [ ] Exercise 052's complexity check cannot reject an O(n) walk
  - **ID**: exercise-052-reject-linear-walk
  - **Details**: In `knowledge/exercises/052-lowest-common-ancestor-of-a-binary-search-tree.yaml` a solution that walks every node first measured a slope of 1.01 (1.09 ms, 3.46 ms, 11.08 ms at 10,000, 31,623 and 100,000 nodes), but `_complexity` in `src/careerbrew/tutor/exercises.py` passes any run whose largest size takes under 50 ms, so it counts as `ok` against `O(log n)`. A memo keyed by a child node (the generator reuses the tree below each fresh root) would also escape the check. Larger trees cost CPU, which the hosted scaling limit cannot spare, so the fix is likely a lower floor for sublinear classes or a per-exercise floor.
  - **Files**: src/careerbrew/tutor/exercises.py; knowledge/exercises/052-lowest-common-ancestor-of-a-binary-search-tree.yaml; tests/unit/test_exercise_complexity.py
  - **Hypothesis**: with a floor that sublinear exercises can reach, a planted linear walk on 052 is rejected in 100% of runs while the reference still passes 100%.
  - **Measurement**: a unit test that runs a planted linear-walk solution and the reference through `exercises.check` for 052.
  - **Success**: the planted walk fails and the reference passes, and the reference scaling run stays under 30 s of local CPU.
  - **Pivot**: if no floor separates them without flakes, mark 052's complexity as advisory instead of a gate.
  - **Anchor**: Goldsmith, Aiken & Wilkerson 2007, "Measuring empirical computational complexity" (FSE).

- [ ] public-readiness S22 host scan flags a Python attribute access as a host name
  - **ID**: s22-host-scan-code-attr
  - **Details**: The `HOST` pattern in `scripts/public-readiness.py` matched the calendar module's local time-zone attribute three times in the rust-02 purevec driver (PR #149), which worked around it with `getattr`. `HOST_CODE` allows only names like `self` and `config`. Fix the scan so attribute access on a Python identifier followed by `_` (and other code shapes) is not a host. Add a planted control: a real host name in the local zone, built at runtime in the test, must still fail.
  - **Files**: scripts/public-readiness.py; tests/unit/test_public_readiness.py
  - **Hypothesis**: code attribute access causes the S22 false positives; a code-shape rule drops them to 0 while every planted real host still fails.
  - **Measurement**: `scripts/public-readiness.sh` S22 count on main before and after, plus unit cases for the calendar module's local time-zone attribute (pass) and a real host name in the local zone, built at runtime in the test (fail) in `tests/unit/test_public_readiness.py` on hosted CI.
  - **Success**: 0 S22 hits on code attribute access; a planted real host name in the local zone, built at runtime in the test, still fails S22.
  - **Pivot**: if a code-shape rule lets any planted host through, keep the current pattern and add an explicit per-line allow marker instead.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Sadowski et al. 2018, "Lessons from Building Static Analysis Tools at Google" (CACM; false positives).

- [ ] CI: stop macOS gate parts from being cancelled while they wait for a runner
  - **ID**: ci-macos-runner-wait
  - **Details**: On 2026-11-18 macOS `part` jobs of the CI workflow sat queued for over 2 hours with 0 steps, then ended `cancelled`, so `gate (macos-15)` failed on PRs #136 to #140 and #146 with no code fault; each needed `gh run rerun <id> --failed`, some twice. Find what cancels a queued job (workflow `concurrency`, a `timeout-minutes` that counts queue time, or GitHub's own queue limit), then make a queued-too-long part rerun once by itself or report "no runner" instead of a failed gate.
  - **Files**: .github/workflows/ci.yml; docs/ci.md
  - **Hypothesis**: most failed `gate (macos-15)` results in a week come from parts cancelled with 0 steps, so making the gate tell "no runner" apart from a test failure cuts manual reruns to 0 for that cause.
  - **Measurement**: `gh run list --workflow CI --limit 100 --json databaseId` then, per run, `gh api repos/{owner}/{repo}/actions/runs/<id>/jobs --jq '[.jobs[]|select(.conclusion=="cancelled" and (.steps|length)==0)]|length'`; count over 7 days before and after.
  - **Success**: 0 manual reruns needed for 0-step cancelled macOS parts over 7 days.
  - **Pivot**: if the cancel comes from GitHub's queue limit and cannot be changed, move the macOS parts to a scheduled run on `main` and keep Linux as the pull request gate; record it in `docs/ci.md`.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA: lead time and change failure rate).

- [ ] Web test flake: control inventory misses the sign-in heading on macOS
  - **ID**: flake-controls-signin-heading
  - **Details**: `web/e2e/controls.spec.ts:287` ("inventory on a personal machine") failed once on the hosted macOS `e2e-1` part of PR #146 (run 44396901477): `getByRole('heading', { name: 'Sign in to careerbrew' })` was not visible in 15 s, while the server log showed many `BrokenPipeError` lines; 195 of 196 tests passed and a rerun passed. The PR changed no web code. Reproduce with repeated runs of that spec, then wait for the sign-in page's own ready signal instead of the heading alone.
  - **Files**: web/e2e/controls.spec.ts; tests/e2e/us-72-every-control.sh
  - **Hypothesis**: the heading wait races a page that is still loading behind earlier aborted requests; waiting for the page's own ready state removes the failure.
  - **Measurement**: `cd web && npx playwright test e2e/controls.spec.ts --repeat-each 20` on a hosted macOS runner (a `workflow_dispatch` job), failures counted before and after.
  - **Success**: 0 failures in 20 repeats after the change.
  - **Pivot**: if it still fails, capture the trace on failure and look for a server error instead of a timing race.
  - **Anchor**: Luo et al. 2014, "An Empirical Analysis of Flaky Tests" (FSE).

- [ ] Investigate RSS calibration below the success band on hosted Linux
  - **ID**: flaky-rss-calibration-linux
  - **Details**: On 2026-11-18, PR 129 (no change to resources code) failed `xtask/tests/resources.rs` `known_allocation_is_within_tolerance_on_this_os`: RSS delta 43,909,120 bytes, outside 50,331,648..=100,663,296 (0.65x of 64 MiB). Earlier hosted runs read 0.979x. 0.65x is inside the pre-registered pivot band (0.5x to 2x) but outside the success band (0.75x to 1.5x). Find why one Linux run touched fewer pages (allocator, transparent huge pages, page reclaim).
  - **Files**: xtask/tests/resources.rs; xtask/src/resources.rs; xtask/src/bin/resource_fixture.rs
  - **Hypothesis**: the low read comes from the fixture not touching every page or from reclaim on a busy runner; making the fixture touch and hold every page and read the peak after a short hold brings 20 of 20 hosted Linux runs inside 0.75x to 1.5x.
  - **Measurement**: `gh run list --workflow CI` over the next 20 runs on main; count `native (ubuntu-24.04)` failures in `known_allocation_is_within_tolerance_on_this_os`.
  - **Success**: 0 of 20 runs outside the success band.
  - **Pivot**: 2 runs outside 0.5x to 2x means `wait4` `ru_maxrss` is not usable there; switch to `/proc/<pid>/status` as the plan says.
  - **Anchor**: Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Keep main's CI result under 20 minutes when two runs overlap (free macOS 5-job limit)
  - **ID**: ci-macos-queue-overlap
  - **Details**: Since the gate split (#80), one run alone gives main's result in 14m01s (run 43706837756). Each run needs 4 macOS jobs, and the free plan runs 5 macOS jobs at once, so a second run waits: main runs 43702544809 and 43704358195 took 30m15s and 30m34s from push, queued 8 to 18 min behind other runs. Pull request runs already cancel older runs of the same PR (`concurrency` in `.github/workflows/ci.yml`). Options to measure, in order: (1) merge one PR at a time (no code; write it in `docs/ci.md`); (2) run the macOS `checks` and `native` parts in one job only on pull requests, keeping 4 parts on main; (3) cancel an older main run when a newer main commit arrives, and let `scripts/hosted-gate.sh` keep skipping cancelled runs. Never drop a required check or the macOS gate.
  - **Files**: .github/workflows/ci.yml; docs/ci.md; scripts/hosted-gate.sh
  - **Hypothesis**: with the chosen option, main's push-to-result time stays at or under 20 minutes even while one pull request run is active.
  - **Measurement**: `gh run list --workflow ci.yml --branch main --event push --limit 10 --json createdAt,updatedAt` (minutes from created to updated per run), for 5 main runs that overlap a pull request run.
  - **Success**: 5 of 5 overlapping main runs finish in 20 minutes or less.
  - **Pivot**: if option (2) or (3) still leaves 2 of 5 above 20 minutes, keep option (1) as the rule and stop changing the workflow.
  - **Anchor**: Forsgren, Humble & Kim 2018 (Accelerate: lead time for changes).

- [ ] Make migrate-history recover reflog tips the header promises, or narrow the header
  - **ID**: migrate-history-reflog-scope
  - **Details**: `scripts/migrate-history.sh` scans the reflog only of branches whose tip is already on the public history. Three cases are not scanned: a branch still on old history that was reset back (its newer old tip is only in its reflog), a deleted branch (git drops its reflog), and the `HEAD` reflog. Either scan the `HEAD` reflog and every reflog entry of old branches too, or make the header and `docs/migrate-to-public-history.md` name these limits.
  - **Files**: scripts/migrate-history.sh; tests/unit/test_migrate_history.py; docs/migrate-to-public-history.md
  - **Hypothesis**: old tips that exist only in a scanned-but-skipped reflog (old-branch reset, `HEAD` after a branch delete) drop from 2 of 2 test cases lost to 0 of 2.
  - **Measurement**: `uv run pytest tests/unit/test_migrate_history.py -q` with one new temp-repo test per case.
  - **Success**: both new tests pass and the 21 existing tests still pass.
  - **Pivot**: if scanning the `HEAD` reflog saves more than 3 tips in a 500-entry synthetic reflog, narrow the header instead of scanning.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA change failure rate).

- [ ] Save a recovered reflog tip when a parent ref name blocks it
  - **ID**: migrate-history-ref-name-clash
  - **Details**: in `scripts/migrate-history.sh` step 1, an existing `refs/pre-public/a` makes `git update-ref refs/pre-public/a/b` fail (a file and folder clash), and `a/b-reflog` fails the same way. The stamped fallback then fails too, and the ERR trap stops the run. Pick a fallback name outside the clashing path, for example `refs/pre-public-reflog/<stamp>/<branch>`.
  - **Files**: scripts/migrate-history.sh; tests/unit/test_migrate_history.py
  - **Hypothesis**: runs that stop on a parent-name clash drop from 1 of 1 test cases to 0 of 1.
  - **Measurement**: `uv run pytest tests/unit/test_migrate_history.py -q` with a temp-repo test that parks `refs/pre-public/a`, then recovers branch `a/b` from its reflog.
  - **Success**: the new test exits 0 and the bundle holds the `a/b` tip; all existing tests pass.
  - **Pivot**: if no ref name avoids the clash, stop with a clear message that names the blocking ref instead.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA change failure rate).

- [ ] Stop migrate-history from walking every reflog entry on clean clones
  - **ID**: migrate-history-reflog-speed
  - **Details**: `reflog_tips` in `scripts/migrate-history.sh` runs `git rev-list --max-parents=0` once per reflog entry until it finds an old one. A clean clone has no old entry, so each run walks every entry of every branch. Stop early, for example check each distinct commit once, or test the whole reflog with one `git rev-list --max-parents=0 --stdin` call.
  - **Files**: scripts/migrate-history.sh; tests/unit/test_migrate_history.py
  - **Hypothesis**: on a synthetic clone with 1,000 public reflog entries, the "Nothing to do" run time drops by 80% or more.
  - **Measurement**: `heavy-run --exclusive hyperfine 'bash scripts/migrate-history.sh'` in a temp clone with 1,000 public reflog entries (`CAREERBREW_PUBLIC_ROOT` set), before and after.
  - **Success**: median run time drops 80% or more, and `uv run pytest tests/unit/test_migrate_history.py -q` passes.
  - **Pivot**: if the drop is under 30%, keep the current loop and close the task.
  - **Anchor**: Kalibera & Jones 2013 (rigorous benchmarking).

- [ ] Guard the watcher test child against file access outside its scratch folder
  - **ID**: watch-gate-filesystem-guard
  - **Details**: `tests/unit/watch_gate_child.py` blocks only process and network audit events. Unfinished work from before the history move adds a file guard: an audit hook that refuses reads, writes, listing and renames outside the scratch root, and refuses `.env`, credential files, `.git`, the data folder, `..`, symlinks and `dir_fd` opens. It has five cases: F01 scratch write is allowed and public reads work; F02 outside and secret reads are refused; F03 symlink and `..` escapes are refused; F04 outside write and rename are refused and a canary file is unchanged; F05 a `dir_fd` write is refused. A saved copy is on the owner's Mac only (`~/.local/state/careerbrew-cutover/this-mac-2026-11-13/dirty/`, the watch-gate-isolation archive). It needs rework before it applies: the child now uses `LC_CTYPE: C.UTF-8` and the fixed zone `Etc/GMT+4`, and the old copy pins a tzdata path. Without the copy, write the guard from this description.
  - **Files**: tests/unit/watch_gate_child.py; tests/unit/test_watch_gate_child_boundary.py; tests/unit/watch_gate_filesystem_probe.py; tests/unit/test_day.py
  - **Hypothesis**: a watcher test can no longer touch a file outside its scratch folder without a failing test, so reads of owner files from the gate fall to 0.
  - **Measurement**: `uv run pytest tests/unit/test_watch_gate_child_boundary.py -q`.
  - **Success**: all five cases pass, and `uv run pytest tests/unit/test_day.py -q` still passes.
  - **Pivot**: if the audit hook makes the watcher tests more than 20% slower, guard only writes and renames.
  - **Anchor**: Saltzer & Schroeder 1975 (least privilege).

- [ ] Finish or drop the claim audit measurement harness
  - **ID**: claim-audit-harness
  - **Details**: `src/careerbrew/work/claim_audit.py` is imported only by its unit test. Unfinished work from before the history move holds the harness that would exercise it: an isolated driver that captures handler output in a throwaway HOME (`tests/claim_audit_driver.py`), a measurement script that writes to `.metrics/claim-audit` (`scripts/measure-evidence-claims.py`), safety tests for guard escape, compatibility and quote scope (`tests/safety/test_claim_audit_safety.py` with its worker and cases file), and public-input tests (`tests/safety/test_claim_audit_public_inputs.py`). A saved copy is on the owner's Mac only (`~/.local/state/careerbrew-cutover/this-mac-2026-11-13/dirty/`, the cross-host and 358 archives). It does not run as it is: its cases file pins old file hashes, and the script needs a source pin under `docs/plans/`, which the public tree leaves out. Decide first: continue the measurement (issues #7 "evidence cache + claim check for career docs" and #14 "claim set, 3-item cap, and graceful degrade for reviews" need it), or remove `claim_audit.py` and its unit test as unused code.
  - **Files**: src/careerbrew/work/claim_audit.py; tests/unit/test_claim_audit.py; tests/claim_audit_driver.py; scripts/measure-evidence-claims.py; tests/safety/
  - **Hypothesis**: after the decision, no module in `src/careerbrew` is used only by its own unit test for claim audit: either the harness runs it, or it is gone.
  - **Measurement**: `rg -l "claim_audit" src tests scripts | wc -l`, and `uv run pytest tests/safety -q -k claim_audit` when the harness is kept.
  - **Success**: the harness tests pass with pins that match the public tree, or the count of files that name `claim_audit` is 0.
  - **Pivot**: if re-pinning takes more than one working day, drop the harness and remove the module.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Keep intended eval cases separate and reject duplicate YAML keys
  - **ID**: eval-yaml-doc-boundaries
  - **PR**: #420 (open in the pre-release repository)
  - **Details**: At source revision `a1ab35d79cb8e6118ba34c5604e11a46000c3dcb`, `evals/cases/024-live-feed.yaml` has no document separator before `live-feed-standup-failed-is-not-a-failed-cycle`; it shares a document with `live-feed-pulls-in-one-parallel-batch`. `evals/run.py:load_cases` uses `yaml.safe_load_all`. Preserve this evidence. Reproduce through that actual loader; first add a strict duplicate-key regression, then repair the separator. This is outside #406 implementation and makes no model-quality claim. Wrap-up: repair and strict duplicate-key controls are committed at d7c2c7f8 and published as draft PR #420. Both intended cases survive and all 188 original records remain equal after excluding the restored record. The normal push gate passed; the retained full US-72 browser failure remains unresolved. A separate cleanup failure is tracked in #419. Preserve all failed evidence, validate and rebind the older diagnostic capsule metadata before any resumed execution, and obtain a causally justified repair plus a green full gate before merging.
  - **Hypothesis**: intended case integrity rises from a predicted 1/2 to 2/2 because neither intended case is overwritten or merged.
  - **Measurement**: `uv run python -c 'import runpy; cases=runpy.run_path("evals/run.py")["load_cases"](); wanted={"live-feed-pulls-in-one-parallel-batch","live-feed-standup-failed-is-not-a-failed-cycle"}; got=[c for c in cases if c.get("id") in wanted]; assert {c["id"] for c in got} == wanted and len(got) == 2'`; add and run the strict duplicate-key regression through the same loader.
  - **Success**: both intended records survive separately (2/2), with zero lost or merged cases; duplicate keys are refused by the loader.
  - **Pivot**: if the actual loader already rejects the file or preserves both intended records, stop and diagnose that result before changing the separator or expectations.
  - **Anchor**: Basili, Caldiera and Rombach 1994 (GQM); source revision and loader path above.

- [ ] The exercises gate step never fails a correct O(n^2) reference on a busy machine
  - **ID**: exercises-slope-noise
  - **Details**: `unique-paths` failed the `exercises` step with growth n^2.52 (limit 2.5) in a full gate on 2026-10-29, and passed 2 of 2 runs alone at load 3; the 2026-10-22 gotcha saw n^2.54. CPU time still moves with cache pressure under 8 workers. Options: fit the slope on the median of the runs, or retry one failed exercise once alone before failing the step. Also: `test_two_sizes_become_three_and_each_size_runs_several_times` still failed 1 of 4 full gates at load ~11 after `SCALE_CPU_SECONDS` went to 60 (#318), while alone it takes 14.5 s wall and 10.4 s CPU; its assertion now prints the child's `reason`, so read that first.
  - **Files**: `src/careerbrew/tutor/exercises.py`, `tests/unit/test_exercise_complexity.py`
  - **Hypothesis**: the exercises step fails 0 times in 10 full gates at load 8 or more, down from about 1 in 5.
  - **Measurement**: `for i in $(seq 10); do scripts/check.sh fast; done` with the step result counted from each log.
  - **Success**: 0 of 10 fail, and a wrong-class (O(n^3)) solution still fails.
  - **Pivot**: if a retry alone still flakes, raise the per-class limit to 2.6 and keep the O(n^3) test.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).
- [ ] PM update: name a led meeting's goal and outcome, and keep low-signal work that fulfils a promise
  - **ID**: pm-meeting-outcome
  - **Blocked**: data source: careerbrew has no meeting-notes connector yet; build one (or a plugin that writes notes into the work feed) first
  - **Details**: Issue #239 shipped led meetings (calendar), launches and check-ins (work feed), and low-signal work last. Two parts need a source careerbrew does not read yet. (1) A led meeting's goal and outcome ("aligned on next steps") need meeting notes; core has no meeting-notes connector (Zoom and Outlook give only the subject). A plugin could send them as feed events of kind `meeting-notes` with the meeting's id. (2) "Unless they fulfil a promise the owner made" needs the promise: a task or an ask the owner answered with "I will", which core does not link to a PR yet.
  - **Hypothesis**: owner edits that add a meeting's outcome to the PM update drop to near 0, because the draft names it.
  - **Measurement**: `uv run careerbrew signals --json` (PM edits per day; issue #238 adds the `added` lesson kind with its source hint) for 2 weeks after a notes source exists.
  - **Success**: `added` lessons with source hint `meeting` fall by half.
  - **Pivot**: if no plugin can send meeting notes, ask the owner to type the outcome with `careerbrew work note` and read it from there.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Prove the README's outcome claims with real-use numbers
  - **ID**: proof-real-use-outcomes
  - **Blocked**: owner step: 4 weeks of real use (see `dogfood-work-mode`); the numbers come from `private/`, which an agent may not read
  - **Details**: The README Proof section (`scripts/proof.py`, `docs/proof/metrics.json`) shows only local runs on synthetic data. Add content-free outcome aggregates from `careerbrew review` (promo_coverage change, readiness change, postings rated) to `scripts/proof.py collect`, labeled `real use`.
  - **Hypothesis**: the Proof section can show at least 3 outcome metrics labeled `real use`, because `careerbrew review` already computes them as numbers.
  - **Measurement**: `scripts/metrics.sh proof && grep -c 'real use' README.md`.
  - **Success**: 3 or more Proof rows labeled `real use`.
  - **Pivot**: if review output cannot be made content-free, publish only `careerbrew report public` counts and drop outcome rows.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Measure sent PM edit and DM learning over two weeks
  - **ID**: pm-sent-edits-and-dms
  - **Blocked**: owner step and measurement prerequisite: after reconciliation mechanics and pm-sent-coverage-audit land, register a two-week window and use the app; an agent cannot create a user's PM updates in a real workspace or read restricted business records
  - **Details**: The feed already replaces changed Slack originals and searches `from:me`. Locally qualified reconciliation mechanics handle cycles with no new results, retain the original created day, require exact DM recipient metadata, follow encrypted app-send receipts, and preserve manual records. Audit composition and delivery remain unfinished. Verify the real-use outcome separately. Only qualifying edited sent days belong in the audit denominator; an unchanged sent draft intentionally produces no empty lesson. Missing recipient metadata and legacy sends without receipts retain the manual path. Synthetic mechanics tests do not prove two weeks of use in a real workspace.
  - **Hypothesis**: every qualifying edited sent day in the registered two-week window has one latest matching sent-history day, with zero stale or wrong-day rows and zero duplicate days.
  - **Measurement**: after pm-sent-coverage-audit creates and validates the proposed interface, a user runs `uv run python scripts/pm-sent-coverage.py --from <registered-start> --to <registered-end> --json`. Freeze dates and audit revision before observations; do not use `signals --json` or `data show --keys` as a coverage proxy.
  - **Success**: coverage is 100% for at least one qualifying edited sent day over the two-week window; stale, wrong-day and duplicate counts are all 0. Zero qualifying days is insufficient data, not success.
  - **Pivot**: coverage below 100% or any wrong-recipient match blocks the automatic-learning claim. Investigate source identity using approved local diagnostics; if trustworthy metadata is unavailable, retain manual `day pm-sent` and document that limit.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration).

- [ ] Owner: label 20 eval answers so the local LLM judge can be trusted
  - **ID**: llm-evals
  - **Blocked**: owner step: label 20 eval answers yourself (`evals/run.sh show`, then `evals/run.sh label <case> pass|fail`); the judge is trusted only when it agrees with your labels, so an agent's labels do not count
  - **Details**: The judge path is built (US-66, `src/careerbrew/judge.py`, `evals/README.md`): `EVALS_LLM=1 evals/run.sh` answers and grades the 65 `input:`/`expect:` cases on the `local` route, and judged failures fail the run only after the judge agrees with your labels. Agreement needs your labels, so only you can finish this. (1) Make the `local` route answer: `ollama pull llama3.1:8b`, or set the `local` route's `model` in `config/models.yaml` to a model you have (`ollama list`; when the local models do not include it, every judge call stops with an HTTP 404 that names this fix). (2) Run `evals/run.sh show`: it prints the next unlabeled case, its criteria, and the model's answer. Read it, then run `evals/run.sh label <case> pass` if the answer meets every criterion, else `evals/run.sh label <case> fail`. Repeat until it says 20/20 labels. (3) Run `evals/run.sh agreement` and commit `evals/labels.jsonl`. Labeling needs no full run; each labeled case takes 2 to 4 model calls (one answer, one per criterion; about 35 s each with a small local model), and replies are cached.
  - **Hypothesis**: the judge agrees with owner labels on >= 85% of cases.
  - **Measurement**: `evals/run.sh agreement` (also printed by `EVALS_LLM=1 evals/run.sh`).
  - **Success**: >= 85% agreement on 20 labeled cases.
  - **Pivot**: < 70%: switch to code graders per criterion.
  - **Anchor**: Zheng et al. 2023, "Judging LLM-as-a-Judge".

- [ ] Verify the approved GitHub connector repair in one work day in a real workspace
  - **ID**: work-mcp-launchd-fix
  - **Blocked**: owner step: verify source ownership, approve the exact owner-managed Claude configuration repair if it is still needed (registered connectors need a source change and sync), reconnect with the host's supported tool, and verify a read-only GitHub MCP request through that client; this run changes only synthetic settings
  - **Details**: The implementation adds `careerbrew day start --fix --yes`, symlink-target/mode preservation, atomic conflict detection, idempotence, and the shared confirmed Actions handler. It stores a connection-time `headersHelper`, never a new token value. Unit/e2e proof covers synthetic settings. A local wiring check cannot prove the real client executed the helper; confirm it with one work day in a real workspace. Read `docs/integrations.md`, Repair an empty GitHub header in Claude.
  - **Files**: `src/careerbrew/workday/mcp_check.py`, `tests/unit/test_mcp_fix.py`
  - **Hypothesis**: one approved repair reduces manual GitHub connector fixes to zero because the client reads the existing gh sign-in at connection time.
  - **Measurement**: `uv run pytest -q tests/unit/test_mcp_fix.py`, then approved `careerbrew day start --fix --yes`, host reconnect, and one read-only GitHub MCP request through that client.
  - **Success**: all synthetic controls pass and the intended client completes its GitHub MCP request in one work day in a real workspace.
  - **Pivot**: if the client ignores headersHelper or cannot reconnect, retain the read-only preflight and manual recovery guidance; do not claim local wiring proves authentication.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] /work reviews new landed plugin commits and drafts issue pairs
  - **ID**: work-plugin-review-landed-commits
  - **Blocked**: needs-owner — the step would open issues in plugin repos, which are public writes outside the watched repos. Question: may /work file those issue pairs without a per-run yes, or should it only list them?
  - **Details**: Moved from agentbrew issue #549 when /work moved to careerbrew (agentbrew #610). careerbrew's runner left plugin review out on purpose (`skills/careerbrew-work-day/LESSONS.md`: the old block counted linked worktrees and remote URLs as plugin checkouts). Keep a last-reviewed SHA per plugin checkout; on each day start, list the plugin's new landed commits and draft, for each generic one, an issue on the parent app plus a link on the plugin.
  - **Files**: `src/careerbrew/workday/`, `skills/careerbrew-work-day/SKILL.md`
  - **Hypothesis**: generic plugin changes reach the parent app: plugin commits never reviewed drop from all to 0 per day.
  - **Measurement**: a pytest with two scratch git repos (plugin + parent) that checks a second run lists 0 commits.
  - **Success**: the test passes and one real day lists each new plugin commit once.
  - **Pivot**: if the owner keeps the current flow, close this task with that reason.
  - **Anchor**: Forsgren, Humble & Kim 2018 (lead time).

- [ ] Stop a card publish from bringing back a company-named "Why this company?" card
  - **ID**: card-publish-company-guard
  - **Tags**: cards, privacy, open-source
  - **Details**: The public deck has one generic card "Why this company?" with `match: []` (id `kb-182800cfcc4c`). The old card named one company outside the five samples. A private deck that still holds the company-named question does not match the generic card, so the next `careerbrew cards publish` could add the company-named card to the public deck again, and the private deck asks that question again as new. Add a check to the publish path and to `careerbrew cards public-check`: a card whose title or prompt names a company that has no file in `knowledge/companies/` fails with the card id and the fix. Decide the mapping from the old card to the generic one, so review history carries over.
  - **Files**: `src/careerbrew/tutor/cards.py`, `knowledge/cards/behavioural.yaml`, `tests/unit/test_public_cards.py`
  - **Hypothesis**: with the check, a publish of a deck that holds a card naming a company outside `knowledge/companies/` adds 0 such cards to the public deck (today: 1).
  - **Measurement**: a unit test that publishes a synthetic deck with one such card and counts company-named cards in the result; `uv run careerbrew cards public-check`.
  - **Success**: 0 company-named cards published in the test; `public-check` exits non-zero on a planted one.
  - **Pivot**: if a name check gives more than 2 false matches on the current public deck, replace it with an explicit `public: false` field on cards and check that field.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Saltzer & Schroeder 1975 (fail-safe defaults).

## P3

- [ ] Indie: a red-first signal from GitHub for each bet
  - **ID**: indie-red-first-signal
  - **Details**: `careerbrew indie run` picks a bet's next action from its `TASKS.md` only. Add an optional read of the bet's own open pull requests with failing checks (through the GitHub connector, metadata only), so a red pull request is the next action before a new task.
  - **Files**: src/careerbrew/indie.py; tests/unit/test_indie.py; tests/fixtures/http/; docs/indie.md
  - **Hypothesis**: bets with a red own pull request get it as their next action, so the median age of a red pull request in a bet's repo drops.
  - **Measurement**: `uv run pytest tests/unit/test_indie.py -q -k red_first` on hosted CI; in use, `careerbrew indie review --json` outcomes per week before and after 14 days.
  - **Success**: the test passes; outcomes per week does not drop by 10% or more.
  - **Pivot**: if outcomes per week drops 10% or more over 14 days, remove the signal and keep the task queue order.
  - **Anchor**: Forsgren, Humble & Kim 2018 (DORA: change failure rate, time to restore).

- [ ] Indie: carry `/work` lessons from a corporate machine to the repo
  - **ID**: indie-peer-corporate-carry
  - **Details**: A corporate machine never pushes, so a `/work` lesson learned there reaches `skills/careerbrew-work-day/LESSONS.md` only when the owner types it on a personal machine. Add `careerbrew peer export` (prints the pending lesson lines, process rules only, after the privacy scan) and `careerbrew peer import --file` for the personal side.
  - **Files**: src/careerbrew/peers.py; src/careerbrew/cli.py; tests/unit/test_peers.py; tests/e2e/us-113-peer-improvement.sh; docs/indie.md
  - **Hypothesis**: with export and import, the count of closed `/work` trials whose lesson line is missing from the repo goes to 0.
  - **Measurement**: `careerbrew peer status --for indie --json` (`unreviewed_lessons`) on the personal machine after one corporate week; unit test on hosted CI.
  - **Success**: 0 closed corporate trials without a line after 14 days.
  - **Pivot**: if the privacy scan blocks more than half of the lines, keep the manual carry and close this task.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Kohavi, Tang & Xu 2020 (one change per experiment).

- [ ] Indie: a first reading date for a bet, and a way to remove a wrong reading
  - **ID**: indie-first-reading-date
  - **Details**: A bet whose metric starts at zero reads PIVOT on day 1, and two PIVOT readings in a row hold it. `careerbrew indie measure` has no guard against a reading taken before the bet's first reading date, and no way to remove one taken by mistake (this happened on 2026-11-19, on the day a bet was saved). Add `--first-reading <date>` to `careerbrew indie bet`: before that date `indie measure` refuses with the date, and `indie run` does not ask for a reading. Add `careerbrew indie measure <id> --remove-last --yes` for a wrong reading.
  - **Files**: src/careerbrew/indie.py; src/careerbrew/cli.py; src/careerbrew/actions.py; tests/unit/test_indie.py; tests/e2e/us-112-indie-day.sh; docs/indie.md; skills/careerbrew-bet-options/SKILL.md
  - **Hypothesis**: with a first reading date, no bet is held by readings taken before its traffic exists, so the count of readings dated before a bet's first reading date goes from 1 to 0.
  - **Measurement**: `uv run pytest tests/unit/test_indie.py -q -k first_reading` on hosted CI; in use, `careerbrew indie bets --json` (readings dated before the first reading date).
  - **Success**: the tests pass; 0 early readings over 4 weeks.
  - **Pivot**: if owners clear the date to read early more than once in 4 weeks, drop the refusal and keep only a warning.
  - **Anchor**: Ries 2011 (pivot or persevere); Munafò et al. 2017 (pre-registration).

- [ ] Indie: the plan names an active bet's own task first
  - **ID**: indie-bet-task-first
  - **Details**: `careerbrew indie run` names the highest-priority unblocked task of a bet's project as its next action. For the careerbrew bet (a yearly data licence, saved 2026-11-19) it names `telemetry-plugin-sink` (P0), not the bet's first task `pro-data-free-sample` (P1), so a session started from the plan does not work on the bet. Let a task name its bet (a `**Bet**: <bet id>` line), and let `indie run` prefer an unblocked task that names the active bet over other tasks of the same project. A project with no such task keeps today's order. Until this ships, start a session with the task id in the prompt.
  - **Files**: src/careerbrew/indie.py; tests/unit/test_indie.py; tests/e2e/us-112-indie-day.sh; skills/careerbrew-indie/SKILL.md; skills/careerbrew-bet-options/SKILL.md; docs/indie.md
  - **Hypothesis**: with the bet line, the next action of an active bet is one of its own tasks whenever one is unblocked, so the share of active bets whose next action is a bet task goes from 0 of 1 to 1 of 1.
  - **Measurement**: `gh pr checks <pr>` (a new case in `tests/unit/test_indie.py`: a P1 task that names the bet is named before a P0 task that does not); in use, `careerbrew indie run`.
  - **Success**: the case passes; `careerbrew indie run` names `pro-data-free-sample` for careerbrew.
  - **Pivot**: if a bet line on tasks is skipped in more than half of new bets over 4 weeks, read the first task from the bet record instead.
  - **Anchor**: Ries 2011 (build, measure, learn); Doerr 2018 (outcomes, not activities).

- [ ] Indie: close two ways an agent could work out an owner number
  - **ID**: indie-owner-number-oracles
  - **Details**: A review of PR 175 found two indirect reads. (1) `careerbrew indie measure <bet> --value N` answers KEEP, WATCH, or PIVOT to an agent, so about 20 typed values find the bet's success and pivot lines (each one also records a false reading). (2) `careerbrew indie income --amount` and `indie goal --monthly` have no owner gate, so an agent that records a known amount reads the target from the percentage. Both are only asked of agents in `actions.AGENT_NOTES` today. Fix: require `--yes` ("the owner said so") for a typed reading, an income record, and a goal change; add them to `actions.CONFIRM`; keep the stored measure command free. A new flag changes the action catalog, so follow the frozen-reference gotcha (a successor fixture pair and both pins).
  - **Files**: src/careerbrew/cli.py; src/careerbrew/actions.py; tests/unit/test_indie.py; skills/careerbrew-indie/SKILL.md; commands/indie.md; xtask/tests/fixtures/capture/
  - **Hypothesis**: with the gate, an agent call with no `--yes` is refused, so the count of owner numbers an agent can work out with these three commands falls from 3 (success line, pivot line, income target) to 0.
  - **Measurement**: `gh pr checks <pr>` (new cases in `tests/unit/test_indie.py`: each of the three calls with no `--yes` exits 2 and writes nothing).
  - **Success**: 3 of 3 calls refused.
  - **Pivot**: if the web Actions form cannot pass the confirm as `--yes`, refuse by caller (`may_show_business()`) instead.
  - **Anchor**: Saltzer & Schroeder 1975 (complete mediation).

- [ ] Indie: the asks trend counts an adapter week with no day record as zero asks
  - **ID**: indie-asks-trend-adapter-week
  - **Details**: With `indie_agent_time_command` set, a week can have agent-hours and no day rows. `indie.review` then shows that week's `asks_per_outcome` as no-data, but the trend's `cost()` counts it as 0 asks, so the trend reads lower than the weeks do. Count a week in the asks trend only when it has day rows.
  - **Files**: src/careerbrew/indie.py; tests/unit/test_indie.py
  - **Hypothesis**: the asks trend equals total asks over total outcomes of the weeks that have day rows; the gap to the per-week numbers falls from 1 case to 0.
  - **Measurement**: `gh pr checks <pr>` (a new case in `tests/unit/test_indie.py` with adapter hours and one week without day rows).
  - **Success**: the case passes.
  - **Pivot**: none needed; a wrong trend is a bug, not an approach.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Indie: an income measure command for a product bet
  - **ID**: indie-income-measure
  - **Blocked**: owner step: needs the owner's yes to read a payment or sponsor total, and the owner's own credential typed with `careerbrew secret set`
  - **Details**: A product or sponsors bet has no built-in way to read its income metric. Add a documented measure command per source (for example a sponsors count from the GitHub connector) that prints one number, so `careerbrew indie measure` runs with no typed value.
  - **Files**: src/careerbrew/indie.py; src/careerbrew/connectors.py; docs/indie.md; docs/privacy.md
  - **Hypothesis**: bets with a measure command get a reading every week, so the share of active bets with a reading in the last 7 days rises from 0% to 80% or more.
  - **Measurement**: `careerbrew indie bets --json` (count of bets whose last reading is 7 days old or less).
  - **Success**: 80% or more of active bets after 4 weeks.
  - **Pivot**: under 40% after 4 weeks: drop the connector read and keep typed values.
  - **Anchor**: Ries 2011 (innovation accounting); Basili, Caldiera & Rombach 1994 (GQM).

- [ ] Find why story US-72 can miss its 15 s wait on the hosted macOS job
  - **ID**: us-72-macos-timing-miss
  - **Details**: On 2026-11-14 the hosted `gate (macos-15)` job failed once on the control inventory (story US-72, `web/e2e/controls.spec.ts`, the `heading(...)` helper): the heading "Sign in to careerbrew" was not found after 15000 ms; the run ended `1 failed`, `195 passed`. A rerun of that job on the same commit passed, the pull request changed no web code, and the Linux job passed. So far this is one event; the cause is not known. Do not raise the timeout first. Steps. (1) Count: list failed `gate (macos-15)` jobs with `gh run list --workflow ci.yml --status failure` and read each log (`gh run view <id> --log-failed`) for the same line. (2) If it shows again, find which visit failed (the test walks many views; add the visit name to the failure message) and whether the page was still loading, signed out, or slow. (3) Fix the cause in the test or the app, with a test that fails before the fix. A rerun is the answer for a single event: `gh run rerun <id> --failed`.
  - **Files**: web/e2e/controls.spec.ts; tests/e2e/us-72-every-control.sh; docs/ci.md
  - **Hypothesis**: hosted macOS gate failures caused by this line go from 1 seen to 0 in the next 20 runs, because the failing visit is named and its cause fixed.
  - **Measurement**: `gh run list --workflow ci.yml --status failure --limit 50 --json databaseId --jq '.[].databaseId'`, then `gh run view <id> --log-failed | grep -c 'Sign in to careerbrew'` for each id.
  - **Success**: 0 failures with this line in 20 hosted macOS runs in a row after the fix.
  - **Pivot**: if it does not show again in 40 hosted runs, remove this task with no code change; if a fix leaves 2 or more failures in 20 runs, stop tuning waits and move the control inventory to its own retried step.
  - **Anchor**: Luo, Hariri, Eloussi & Marinov 2014, "An empirical analysis of flaky tests" (FSE); Forsgren, Humble & Kim 2018 (change failure rate).

- [ ] Deliver the requesting-code-review counterexample proposal
  - **ID**: retro-requesting-review-counterexamples
  - **Blocked**: owner approval: changing or publishing to the third-party skill source is outside this repo; no upstream post is authorized
  - **Details**: The exact source addition, four PM parser/matcher counterexamples, proposed prompt and 7-criterion rubric are in the work journal `2026-11-01-pm-review-skill-retro` (kept out of the public release). Preserve RED-before-fix evidence, exact-source fix-round review and the distinction between reviewer PASS and a queued full gate. Inspect the current upstream owner/runner after approval; never edit an installed mirror.
  - **Hypothesis**: the proposed review eval detects all 4 behavior gaps and preserves the 3 review/verification guard criteria, instead of accepting a passing focused suite as sufficient.
  - **Measurement**: `scripts/check-docs.sh`, `npx --no-install @tasks-md/lint TASKS.md` and `git diff --check` validate this local proposal only. Before an approved upstream change, register its exact behavioral-eval command and run the unchanged skill baseline.
  - **Success**: local documentation checks have 0 errors; future upstream eval scores 7/7 and its counterexample fails before the correction and passes after it.
  - **Pivot**: below 7/7, revise the prompt or fixture before rollout. If upstream already covers this scenario, add a distinct eval rather than duplicate instructions. No publication without target-specific approval.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration).

- [ ] Deliver the test-driven-development retro proposal to its source owner
  - **ID**: retro-tdd-ingestion
  - **Blocked**: owner approval: publication to obra/superpowers is outside this repo; no upstream post is authorized
  - **Details**: The exact local source addition, eval prompt and expected behavior are in the work journal `2026-11-01-third-party-skill-retro` (kept out of the public release). Do not edit the installed mirror. After approval, inspect the current upstream source and use its own eval runner before posting.
  - **Hypothesis**: the proposed counterexample makes an incomplete verification claim fail its behavioral eval, because it exercises the actual source path or evidence boundary.
  - **Measurement**: `scripts/check-docs.sh` and `git diff --check` validate this local proposal only; register the current upstream behavioral-eval command before implementing there.
  - **Success**: local documentation gates pass; upstream delivery is kept only when its registered counterexample fails before and passes after the source change.
  - **Pivot**: if the current upstream skill already covers this case, add a distinct eval rather than duplicate the instruction; do not publish without approval.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration).

- [ ] Deliver the verification-before-completion retro proposal to its source owner
  - **ID**: retro-verification-cache
  - **Blocked**: owner approval: publication to obra/superpowers is outside this repo; no upstream post is authorized
  - **Details**: The exact local source addition, eval prompt and expected behavior are in the work journal `2026-11-01-third-party-skill-retro` (kept out of the public release). Do not edit the installed mirror. After approval, inspect the current upstream source and use its own eval runner before posting.
  - **Hypothesis**: the proposed counterexample makes an incomplete verification claim fail its behavioral eval, because it exercises the actual source path or evidence boundary.
  - **Measurement**: `scripts/check-docs.sh` and `git diff --check` validate this local proposal only; register the current upstream behavioral-eval command before implementing there.
  - **Success**: local documentation gates pass; upstream delivery is kept only when its registered counterexample fails before and passes after the source change.
  - **Pivot**: if the current upstream skill already covers this case, add a distinct eval rather than duplicate the instruction; do not publish without approval.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration).

- [ ] Deliver the playwright-best-practices retro proposal to its source owner
  - **ID**: retro-playwright-baselines
  - **Blocked**: owner approval: publication to currents-dev/playwright-best-practices-skill is outside this repo; no upstream post is authorized
  - **Details**: The exact local source addition, eval prompt and expected behavior are in the work journal `2026-11-01-third-party-skill-retro` (kept out of the public release). Do not edit the installed mirror. After approval, inspect the current upstream source and use its own eval runner before posting.
  - **Hypothesis**: the proposed counterexample makes an incomplete verification claim fail its behavioral eval, because it exercises the actual source path or evidence boundary.
  - **Measurement**: `scripts/check-docs.sh` and `git diff --check` validate this local proposal only; register the current upstream behavioral-eval command before implementing there.
  - **Success**: local documentation gates pass; upstream delivery is kept only when its registered counterexample fails before and passes after the source change.
  - **Pivot**: if the current upstream skill already covers this case, add a distinct eval rather than duplicate the instruction; do not publish without approval.
  - **Anchor**: Basili, Caldiera & Rombach 1994 (GQM); Munafò et al. 2017 (pre-registration).

- [ ] Apply to open-source AI maintainer programs when careerbrew has traction
  - **ID**: oss-ai-maintainer-programs
  - **Tags**: funding, ai-cost, owner-action
  - **Details**: Some vendors give open-source maintainers free AI. Research on 2026-11-10 found that careerbrew misses every published threshold, so applying now is a long shot. Recheck when the repo grows. Programs and rules as read on 2026-11-10 (reread each page before acting):
    - Claude for Open Source (https://claude.com/contact-sales/claude-for-oss): 6 months of the top Claude plan, paid billing pauses. Any one of: 20 or more external contributors with merged PRs in 12 months, 200,000 or more monthly downloads, 500 or more dependent repos, OpenSSF criticality 0.4 or above. It also has a discretionary "apply anyway" track.
    - OpenAI Codex for Open Source (https://developers.openai.com/codex/community/codex-for-oss): 6 months of ChatGPT Pro plus API credits. No numeric rule; it wants a "widely used public project".
    - Greptile for open source (https://www.greptile.com/pricing/open-source): free AI review, automatic at 50 stars.
    - Atlas Cloud OSS program (https://atlascloud.ai/docs/en/oss-program): monthly credits from about 100 stars.
    - GitHub Copilot Pro for maintainers: automatic for a "popular" repo; check https://github.com/settings/copilot.
    - No application needed on a public repo: CodeRabbit and Sourcery free AI review, Sentry for Open Source (includes Seer).
    - Publishing the repo does not change the AI bill by itself. It only opens these programs and free CI runners.
    - **Hypothesis**: once careerbrew passes one threshold, one accepted application removes 6 months of AI subscription cost for the owner.
    - **Measurement**: `gh api repos/{owner}/{repo} -q .stargazers_count` and `gh api repos/{owner}/{repo}/contributors -q length`
    - **Success**: at least one program accepts and the benefit is active on the owner's account.
    - **Pivot**: if two applications made after a threshold is met are both rejected, close this task and stop applying.
    - **Anchor**: Anthropic Claude for Open Source program terms (https://www.anthropic.com/claude-for-oss-terms); Basili, Caldiera and Rombach (1994), GQM.
  - **Files**: none (owner fills web forms); this task records the outcome
  - **Output**: decision
  - **Acceptance**: the owner has applied to each program whose threshold careerbrew meets, and this task records each answer, or records why no application was made.
  - **Blocked**: needs-traction-and-owner-application — 2026-11-10: careerbrew is below every threshold (it is public since 2026-11-13). Agents never submit applications or forms; the owner applies. Unblock when the measurement shows 50 or more stars or 20 or more contributors, or when the owner chooses to try the discretionary tracks.
