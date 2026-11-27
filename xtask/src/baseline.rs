//! `migration baseline --step <id>`: run a registered step's Python oracle
//! N times through the process-tree collector and write a content-free JSON
//! record (step, command, machine, toolchains, source revision and hash, raw
//! samples, p50 and p95). Each run gets a fresh synthetic home that lives for
//! that run only; nothing is kept between calls.
use crate::{baseline_verdict, capture_actions, resources};
use serde_json::{json, Value};
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub const SCHEMA: &str = "careerbrew-migration-baseline-v1";
pub const DEFAULT_RUNS: usize = 30;
/// The protocol's minimum sample count; fewer runs need the test-only flag.
pub const MIN_RUNS: usize = 30;
/// The oracle imports from the checkout with warm OS file caches; only the
/// careerbrew data folder is fresh for each run.
pub const CACHE_STATE: &str = "warm-imports-fresh-synthetic-home";

/// A registered step: its id and the Python oracle arguments after the
/// interpreter. The oracle must be read-only and must exit 0.
pub struct Step {
    pub id: &'static str,
    pub oracle: &'static [&'static str],
    /// `verify` cases: an id and the CLI arguments after `careerbrew`.
    pub cases: &'static [(&'static str, &'static [&'static str])],
    /// The step's built Rust candidate, relative to the repo root. None until
    /// the port lands; `verify` refuses the step until then.
    pub candidate: Option<&'static str>,
    /// `verify` also compares the HTTP handler contract (the capture-http
    /// fixture cases) when true.
    pub http_contract: bool,
    /// `verify` also runs the DATA contract (the capture-data corpus) when
    /// true.
    pub data_contract: bool,
    /// `verify` also runs the corporate-role case (`jobs list`) when true.
    pub role_contract: bool,
    /// `verify` also compares the candidate's `actions --json` stdout,
    /// wrapped as `{"actions": ...}`, with the body of the frozen
    /// capture-http `/api/actions` cases (offline: no server starts).
    pub registry_projection: bool,
    /// `verify` runs every candidate CLI case with Python denied (see
    /// `verify::NO_PYTHON_HOME`). A step whose candidate may still start
    /// Python leaves it false.
    pub no_python: bool,
    /// Extra environment for both sides of every baseline, measure and
    /// verify run (after the synthetic environment).
    pub env: &'static [(&'static str, &'static str)],
    /// Extra files, relative to the repo root, hashed into the baseline and
    /// measure source hash (a vector step's driver and the Python it reads).
    pub sources: &'static [&'static str],
    /// Exact fixture files, relative to the repo root, whose bytes join the
    /// shared evidence identity. Directories and unregistered files are not read.
    pub fixtures: &'static [&'static str],
}

pub const STEPS: &[Step] = &[
    Step {
        // rust-02 is a vector step (`depvec::RUST_02`): verify compares pure
        // value vectors, so it has no CLI cases. Its oracle and candidate run
        // every vector set in one process with `--all`.
        id: "rust-02",
        oracle: &["-B", "xtask/tests/fixtures/purevec/driver.py", "--all"],
        cases: &[],
        candidate: Some("target/debug/careerbrew-purevec"),
        // A vector step never reaches the CLI contracts (`verify` hands it to
        // `depvec`), and `decide` gives a vector step no declared scope, so
        // these flags have no effect on its verdict.
        http_contract: false,
        data_contract: true,
        role_contract: true,
        registry_projection: false,
        no_python: false,
        env: &[],
        sources: &[
            "xtask/tests/fixtures/purevec/driver.py",
            "src/careerbrew/__init__.py",
            "src/careerbrew/config.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/changes.py",
            "src/careerbrew/calendar.py",
        ],
        fixtures: &[
            "xtask/tests/fixtures/purevec/clock.json",
            "xtask/tests/fixtures/purevec/hash.json",
            "xtask/tests/fixtures/purevec/paths.json",
            "xtask/tests/fixtures/purevec/settings.json",
        ],
    },
    Step {
        // rust-03 ports the native CLI with one read-only pilot: the action catalog.
        id: "rust-03",
        oracle: &["-B", "-m", "careerbrew", "actions", "--json"],
        cases: RUST_03_CASES,
        candidate: Some("target/release/careerbrew-native"),
        // The pilot binary serves no HTTP, no data and no role-checked
        // command: `server.py` stays Python. The catalog it shares with
        // /api/actions is checked offline by `registry_projection` instead.
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: true,
        no_python: true,
        // Help text wraps at the terminal width: pin it on both sides.
        env: &[("COLUMNS", "80"), ("LINES", "24")],
        // The pilot crate and the registry it compiles in.
        sources: &[
            "crates/cbcli/Cargo.toml",
            "crates/cbcli/src/lib.rs",
            "crates/cbcli/src/main.rs",
            "crates/cbcli/src/registry.rs",
            "crates/cbcli/src/render.rs",
            "crates/cbcli/src/help.rs",
            "crates/cbcli/src/events.rs",
            "crates/cbcli/registry/registry.json",
            "scripts/gen-registry.py",
            "config/native-routes.txt",
        ],
        fixtures: &[],
    },
    Step {
        // rust-05f/g/h/i is a stateful component workload. The real Python oracle
        // uses only its invocation's synthetic home and injected adapters.
        id: "rust-05",
        oracle: &["-I", "-B", "tests/oracles/rust05.py", "--all"],
        cases: &[],
        candidate: Some("target/release/vault-store"),
        http_contract: false,
        data_contract: true,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust05.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/config_io.py",
            "src/careerbrew/events.py",
            "src/careerbrew/approvals.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/calendar.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/bin/vault-store.rs",
            "crates/policy/src/approvals.rs",
            "crates/policy/src/vectors.rs",
            "xtask/src/rust05.rs",
        ],
        fixtures: &["docs/plans/2026-11-05-rust-data-fixture-v1.json"],
    },
    Step {
        // rust-06 is a stateful, component-only backup/sync workload. Its
        // custom verifier also orchestrates both cross-engine restore directions.
        id: "rust-06",
        oracle: &["-I", "-B", "tests/oracles/rust06.py", "--all"],
        cases: &[],
        candidate: Some("target/release/vault-sync"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust06.py",
            "src/careerbrew/backup.py",
            "src/careerbrew/sync.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/goals.py",
            "src/careerbrew/work/promo.py",
            "src/careerbrew/events.py",
            "src/careerbrew/calendar.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/transfer.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/bin/vault-sync.rs",
            "xtask/src/rust06.rs",
        ],
        fixtures: &["knowledge/ladders/generic-swe.yaml"],
    },
    Step {
        // rust-07 starts with its real Python oracle. The synthetic-only Rust
        // candidate is registered with the component implementation.
        id: "rust-07",
        oracle: &["-I", "-B", "tests/oracles/rust07.py", "--all"],
        cases: &[],
        candidate: Some("target/release/setup-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust07.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/msauth.py",
            "src/careerbrew/workday/mcp_check.py",
            "src/careerbrew/workday/self_update.py",
            "src/careerbrew/workday/workflow_check.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/sync.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/config.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/agent_workflows.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/goals.py",
            "src/careerbrew/day/inbox.py",
            "src/careerbrew/privacy.py",
            "src/careerbrew/wizard.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/work/promo.py",
            "src/careerbrew/workday/steps.py",
            "src/careerbrew/workday/watch.py",
            "src/careerbrew/workday/start.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/main.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/transfer.rs",
            "crates/vault/src/bin/vault-store.rs",
            "crates/vault/src/bin/vault-sync.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/value.rs",
            "crates/setup/src/plugins.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/oauth.rs",
            "crates/setup/src/preflight.rs",
            "crates/setup/src/cli.rs",
            "crates/setup/src/synthetic.rs",
            "crates/setup/src/bin/setup-component.rs",
            "xtask/src/rust07.rs",
        ],
        fixtures: &[
            ".env.example",
            "profile/examples/day/calendars.yaml",
            "profile/examples/jobs/criteria.yaml",
            "profile/examples/jobs/sources.yaml",
            "profile/examples/mail.yaml",
            "profile/examples/profile.yaml",
            "profile/examples/sensitive-terms.txt",
            "profile/examples/settings.yaml",
            "profile/examples/work/sources.yaml",
            "tests/fixtures/plugin-acme/careerbrew-plugin.yaml",
            "tests/fixtures/plugin-acme/evidence.yaml",
            "tests/fixtures/plugin-acme/job-sources.yaml",
            "tests/fixtures/plugin-acme/knowledge/acme-items.yaml",
            "tests/fixtures/plugin-acme/ladders/acme-eng.yaml",
            "tests/fixtures/plugin-acme/rules.md",
            "tests/fixtures/plugin-acme/sensitive-terms.txt",
            "tests/fixtures/plugin-acme-platform/careerbrew-plugin.yaml",
            "tests/fixtures/plugin-acme-platform/ladders/acme-platform-eng.yaml",
            "tests/fixtures/plugin-acme-platform/sensitive-terms.txt",
        ],
    },
    Step {
        // rust-08 starts with its real Python oracle and current synthetic
        // fixtures. The component-only candidate is registered separately.
        id: "rust-08",
        oracle: &["-I", "-B", "tests/oracles/rust08.py", "--all"],
        cases: &[],
        candidate: Some("target/release/sources-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust08.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/mail.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/msauth.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/store.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/events.py",
            "src/careerbrew/config.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/work/feed.py",
            "src/careerbrew/jobs/linkedin.py",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/main.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/transfer.rs",
            "crates/vault/src/bin/vault-store.rs",
            "crates/vault/src/bin/vault-sync.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/oauth.rs",
            "crates/setup/src/value.rs",
            "crates/sources/Cargo.toml",
            "crates/sources/src/lib.rs",
            "crates/sources/src/http.rs",
            "crates/sources/src/connectors.rs",
            "crates/sources/src/calendar.rs",
            "crates/sources/src/mail.rs",
            "crates/sources/src/cli.rs",
            "crates/sources/src/synthetic.rs",
            "crates/sources/src/bin/sources-component.rs",
            "xtask/src/rust08.rs",
        ],
        fixtures: &[
            "tests/fixtures/calendar/work.ics",
            "tests/fixtures/http/api.adzuna.com/v1_api_jobs_gb_search_1.json",
            "tests/fixtures/http/api.atlassian.com/ex_jira_synthetic-cloud-1_rest_api_3_myself.json",
            "tests/fixtures/http/api.atlassian.com/ex_jira_synthetic-cloud-2_rest_api_3_myself.json",
            "tests/fixtures/http/api.fastmail.com/jmap_api.json",
            "tests/fixtures/http/api.fastmail.com/jmap_session.json",
            "tests/fixtures/http/api.github.com/repos_acme_billing_pulls.json",
            "tests/fixtures/http/api.github.com/repos_acme_billing_pulls_41.json",
            "tests/fixtures/http/api.github.com/repos_example_app_pulls_1_reviews.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_branches_main_protection.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_contents_.github_CODEOWNERS.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_pulls.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_pulls_5_files.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_pulls_5_reviews.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_pulls_7.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_pulls_7_files.json",
            "tests/fixtures/http/api.github.com/repos_example_shop_rules_branches_main.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_branches_main_protection.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_contents_.github_CODEOWNERS.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_contents_CODEOWNERS.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_pulls.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_pulls_2_files.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_pulls_2_reviews.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_pulls_3.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_pulls_3_files.json",
            "tests/fixtures/http/api.github.com/repos_example_tools_rules_branches_main.json",
            "tests/fixtures/http/api.github.com/search_issues.json",
            "tests/fixtures/http/api.github.com/user.json",
            "tests/fixtures/http/api.zoom.us/v2_users_me_meetings.json",
            "tests/fixtures/http/bad.atlassian.example.com/_edge_tenant_info.json",
            "tests/fixtures/http/bad.atlassian.example.com/rest_api_3_myself.json",
            "tests/fixtures/http/careers.example.com/interview-process.html",
            "tests/fixtures/http/docs.google.com/spreadsheets_d_sheet-cutline-q4_export.csv",
            "tests/fixtures/http/docs.google.com/spreadsheets_d_sheet-private_export.html",
            "tests/fixtures/http/graph.microsoft.com/v1.0_me.json",
            "tests/fixtures/http/graph.microsoft.com/v1.0_me_calendarView.json",
            "tests/fixtures/http/graph.microsoft.com/v1.0_me_messages.json",
            "tests/fixtures/http/himalayas.app/jobs_api_search.json",
            "tests/fixtures/http/hn.algolia.com/api_v1_items_49922569.json",
            "tests/fixtures/http/hn.algolia.com/api_v1_search_by_date.json",
            "tests/fixtures/http/jira.example.com/rest_api_3_issue_PROJ-7.json",
            "tests/fixtures/http/jira.example.com/rest_api_3_myself.json",
            "tests/fixtures/http/jira.example.com/rest_api_3_search_jql.json",
            "tests/fixtures/http/leetcode.com/graphql.json",
            "tests/fixtures/http/remoteok.com/api.json",
            "tests/fixtures/http/remotive.com/api_remote-jobs.json",
            "tests/fixtures/http/scoped.atlassian.example.com/_edge_tenant_info.json",
            "tests/fixtures/http/scoped.atlassian.example.com/rest_api_3_myself.json",
            "tests/fixtures/http/slack.com/api_chat.postMessage.json",
            "tests/fixtures/http/slack.com/api_search.messages.json",
            "tests/fixtures/http/status.example.com/budget.html",
            "tests/fixtures/http/weworkremotely.com/categories_remote-programming-jobs.rss.xml",
            "tests/fixtures/http/www.googleapis.com/drive_v3_files.json",
            "tests/fixtures/http/www.googleapis.com/drive_v3_files_doc-prd-1.json",
            "tests/fixtures/http/www.googleapis.com/drive_v3_files_doc-prd-1_comments.json",
        ],
    },
    Step {
        // rust-09 starts with its real Python oracle. The component candidate
        // and native source bindings join only with the implementation.
        id: "rust-09",
        oracle: &["-I", "-B", "tests/oracles/rust09.py", "--all"],
        cases: &[],
        candidate: Some("target/release/projects-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust09.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/work/priorities.py",
            "src/careerbrew/work/project_priority.py",
            "src/careerbrew/work/promises.py",
            "src/careerbrew/work/focus.py",
            "src/careerbrew/work/team.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/work/feed.py",
            "src/careerbrew/work/ladder_grade.py",
            "src/careerbrew/work/live.py",
            "src/careerbrew/work/promo.py",
            "src/careerbrew/work/research.py",
            "src/careerbrew/day/inbox.py",
            "src/careerbrew/day/pm_promises.py",
            "src/careerbrew/day/results.py",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/projects/src/projects.rs",
            "crates/projects/src/priorities.rs",
            "crates/projects/src/project_priority.rs",
            "crates/projects/src/promises.rs",
            "crates/projects/src/focus.rs",
            "crates/projects/src/team.rs",
            "crates/projects/src/cli.rs",
            "crates/projects/src/synthetic.rs",
            "crates/projects/src/bin/projects-component.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/transfer.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/cli.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/oauth.rs",
            "crates/setup/src/plugins.rs",
            "crates/setup/src/preflight.rs",
            "crates/setup/src/synthetic.rs",
            "crates/setup/src/value.rs",
            "xtask/src/rust09.rs",
        ],
        fixtures: &[
            "tests/fixtures/projects/cutline.json",
            "tests/fixtures/projects/team-goals.csv",
        ],
    },
    Step {
        // rust-10 compares its real Python oracle with the component-only
        // native candidate; no production route selects the candidate.
        id: "rust-10",
        oracle: &["-I", "-B", "tests/oracles/rust10.py", "--all"],
        cases: &[],
        candidate: Some("target/release/sessions-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust10.py",
            "src/careerbrew/work/sessions.py",
            "src/careerbrew/work/agents.py",
            "src/careerbrew/work/drafts.py",
            "src/careerbrew/work/prior_selection.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/workday/questions.py",
            "src/careerbrew/workday/steps.py",
            "src/careerbrew/workday/watch.py",
            "src/careerbrew/day/handoff.py",
            "crates/sessions/Cargo.toml",
            "crates/sessions/src/lib.rs",
            "crates/sessions/src/sessions.rs",
            "crates/sessions/src/agents.rs",
            "crates/sessions/src/drafts.rs",
            "crates/sessions/src/prior_selection.rs",
            "crates/sessions/src/cli.rs",
            "crates/sessions/src/synthetic.rs",
            "crates/sessions/src/bin/sessions-component.rs",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/projects/src/projects.rs",
            "crates/projects/src/priorities.rs",
            "crates/projects/src/project_priority.rs",
            "crates/projects/src/promises.rs",
            "crates/projects/src/focus.rs",
            "crates/projects/src/team.rs",
            "crates/projects/src/cli.rs",
            "crates/projects/src/synthetic.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/transfer.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/cli.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/oauth.rs",
            "crates/setup/src/plugins.rs",
            "crates/setup/src/preflight.rs",
            "crates/setup/src/synthetic.rs",
            "crates/setup/src/value.rs",
            "xtask/src/rust10.rs",
        ],
        fixtures: &[
            "profile/examples/day/calendars.yaml",
            "profile/examples/jobs/criteria.yaml",
            "profile/examples/jobs/sources.yaml",
            "profile/examples/mail.yaml",
            "profile/examples/profile.yaml",
            "profile/examples/sensitive-terms.txt",
            "profile/examples/settings.yaml",
            "profile/examples/work/sources.yaml",
        ],
    },
    Step {
        // rust-11 starts with its real Python oracle. The native candidate and
        // its source bindings join with the component implementation.
        id: "rust-11",
        oracle: &["-I", "-B", "tests/oracles/rust11.py", "--all"],
        cases: &[],
        candidate: Some("target/release/memory-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust11.py",
            "src/careerbrew/work/research.py",
            "src/careerbrew/work/project_import.py",
            "src/careerbrew/work/memory_sync.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/work/priorities.py",
            "src/careerbrew/work/project_priority.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/work/feed.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "crates/memory/Cargo.toml",
            "crates/memory/src/lib.rs",
            "crates/memory/src/research.rs",
            "crates/memory/src/project_import.rs",
            "crates/memory/src/memory_sync.rs",
            "crates/memory/src/cli.rs",
            "crates/memory/src/synthetic.rs",
            "crates/memory/src/bin/memory-component.rs",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/projects/src/projects.rs",
            "crates/projects/src/priorities.rs",
            "crates/projects/src/project_priority.rs",
            "crates/projects/src/promises.rs",
            "crates/projects/src/focus.rs",
            "crates/projects/src/team.rs",
            "crates/projects/src/cli.rs",
            "crates/projects/src/synthetic.rs",
            "crates/sources/Cargo.toml",
            "crates/sources/src/lib.rs",
            "crates/sources/src/http.rs",
            "crates/sources/src/connectors.rs",
            "crates/sources/src/calendar.rs",
            "crates/sources/src/mail.rs",
            "crates/sources/src/cli.rs",
            "crates/sources/src/synthetic.rs",
            "crates/policy/Cargo.toml",
            "crates/policy/src/lib.rs",
            "crates/policy/src/text.rs",
            "crates/policy/src/tables.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/backup.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/cli.rs",
            "crates/vault/src/data.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/receipts.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/settings.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/sync.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/synthetic_bulk.rs",
            "crates/vault/src/synthetic_transfer.rs",
            "crates/vault/src/transfer.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/cli.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/oauth.rs",
            "crates/setup/src/plugins.rs",
            "crates/setup/src/preflight.rs",
            "crates/setup/src/synthetic.rs",
            "crates/setup/src/value.rs",
            "xtask/src/rust11.rs",
        ],
        fixtures: &[
            "tests/fixtures/projects/cutline.json",
            "tests/fixtures/projects/team-goals.csv",
        ],
    },
    Step {
        // rust-12 uses its real Python oracle and synthetic native component.
        id: "rust-12",
        oracle: &["-I", "-B", "tests/oracles/rust12.py", "--all"],
        cases: &[],
        candidate: Some("target/release/learn-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust12.py",
            "src/careerbrew/learn/project.py",
            "src/careerbrew/learn/replay.py",
            "src/careerbrew/learn/tour.py",
            "src/careerbrew/work/explain.py",
            "src/careerbrew/work/prd.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/tutor/cards.py",
            "src/careerbrew/tutor/public.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "Cargo.toml",
            "Cargo.lock",
            "crates/learn/Cargo.toml",
            "crates/learn/src/lib.rs",
            "crates/learn/src/cards.rs",
            "crates/learn/src/project.rs",
            "crates/learn/src/replay.rs",
            "crates/learn/src/tour.rs",
            "crates/learn/src/explain.rs",
            "crates/learn/src/prd.rs",
            "crates/learn/src/cli.rs",
            "crates/learn/src/synthetic.rs",
            "crates/learn/src/bin/learn-component.rs",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/projects/src/projects.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/value.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/transfer.rs",
            "xtask/src/rust12.rs",
        ],
        fixtures: &[
            "profile/examples/day/calendars.yaml",
            "profile/examples/jobs/criteria.yaml",
            "profile/examples/jobs/sources.yaml",
            "profile/examples/mail.yaml",
            "profile/examples/profile.yaml",
            "profile/examples/sensitive-terms.txt",
            "profile/examples/settings.yaml",
            "profile/examples/work/sources.yaml",
            "skills/careerbrew-project-explainer/references/example-careerbrew.md",
            "skills/careerbrew-project-explainer/references/example-prd.md",
        ],
    },
    Step {
        // rust-13 starts with its real Python oracle. The native candidate and
        // its source bindings join with the component implementation.
        id: "rust-13",
        oracle: &["-I", "-B", "tests/oracles/rust13.py", "--all"],
        cases: &[],
        candidate: Some("target/release/feed-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust13.py",
            "src/careerbrew/work/feed.py",
            "src/careerbrew/work/transcript.py",
            "src/careerbrew/work/team.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/work/memory_sync.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/work/health.py",
            "src/careerbrew/work/live.py",
            "src/careerbrew/work/promises.py",
            "src/careerbrew/work/project_priority.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/cli.py",
            "Cargo.toml",
            "Cargo.lock",
            "crates/feed/Cargo.toml",
            "crates/feed/src/lib.rs",
            "crates/feed/src/feed.rs",
            "crates/feed/src/sources.rs",
            "crates/feed/src/transcript.rs",
            "crates/feed/src/cli.rs",
            "crates/feed/src/synthetic.rs",
            "crates/feed/src/bin/feed-component.rs",
            "crates/projects/src/lib.rs",
            "crates/projects/src/team.rs",
            "crates/projects/src/project_priority.rs",
            "crates/projects/src/projects.rs",
            "crates/memory/Cargo.toml",
            "crates/memory/src/lib.rs",
            "crates/memory/src/memory_sync.rs",
            "crates/memory/src/research.rs",
            "crates/sources/src/lib.rs",
            "crates/sources/src/http.rs",
            "crates/sources/src/connectors.rs",
            "crates/setup/src/lib.rs",
            "crates/setup/src/value.rs",
            "crates/setup/src/plugins.rs",
            "crates/policy/Cargo.toml",
            "crates/policy/src/lib.rs",
            "crates/policy/src/outputs.rs",
            "crates/policy/src/canary.rs",
            "crates/policy/src/text.rs",
            "crates/policy/src/tables.rs",
            "crates/policy/src/pyre.rs",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/transfer.rs",
            "xtask/src/rust13.rs",
        ],
        fixtures: &[
            "tests/fixtures/feed/next-step-ask.jsonl",
            "tests/fixtures/feed/web-asks.jsonl",
            "tests/fixtures/transcripts/feed-cycle.jsonl",
            "tests/fixtures/transcripts/feed-cycle/tool-results/big.txt",
        ],
    },
    Step {
        // rust-14 starts with its real Python oracle. The native candidate and
        // its effective source bindings join with the component implementation.
        id: "rust-14",
        oracle: &["-I", "-B", "tests/oracles/rust14.py", "--all"],
        cases: &[],
        candidate: Some("target/release/live-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust14.py",
            "src/careerbrew/work/live.py",
            "src/careerbrew/work/listen.py",
            "src/careerbrew/work/review_ask.py",
            "src/careerbrew/work/health.py",
            "src/careerbrew/notify.py",
            "src/careerbrew/goals.py",
            "src/careerbrew/day/inbox.py",
            "src/careerbrew/day/results.py",
            "src/careerbrew/work/standup.py",
            "src/careerbrew/work/priorities.py",
            "src/careerbrew/work/drafts.py",
            "src/careerbrew/work/projects.py",
            "src/careerbrew/work/research.py",
            "src/careerbrew/work/feed.py",
            "src/careerbrew/work/team.py",
            "src/careerbrew/work/project_priority.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/privacy.py",
            "src/careerbrew/connectors.py",
            "src/careerbrew/secrets.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/cli.py",
            "src/careerbrew/paths.py",
            "Cargo.toml",
            "Cargo.lock",
            "crates/live/Cargo.toml",
            "crates/live/src/lib.rs",
            "crates/live/src/live.rs",
            "crates/live/src/health.rs",
            "crates/live/src/listen.rs",
            "crates/live/src/review.rs",
            "crates/live/src/notify.rs",
            "crates/live/src/cli.rs",
            "crates/live/src/synthetic.rs",
            "crates/live/src/bin/live-component.rs",
            "crates/sessions/src/drafts.rs",
            "crates/feed/Cargo.toml",
            "crates/feed/src/lib.rs",
            "crates/feed/src/feed.rs",
            "crates/feed/src/sources.rs",
            "crates/feed/src/transcript.rs",
            "crates/feed/src/cli.rs",
            "crates/feed/src/synthetic.rs",
            "crates/feed/src/bin/feed-component.rs",
            "crates/projects/src/lib.rs",
            "crates/projects/src/team.rs",
            "crates/projects/src/project_priority.rs",
            "crates/projects/src/projects.rs",
            "crates/projects/src/priorities.rs",
            "crates/memory/Cargo.toml",
            "crates/memory/src/lib.rs",
            "crates/memory/src/memory_sync.rs",
            "crates/memory/src/research.rs",
            "crates/sources/src/lib.rs",
            "crates/sources/src/http.rs",
            "crates/sources/src/connectors.rs",
            "crates/setup/src/lib.rs",
            "crates/setup/src/value.rs",
            "crates/setup/src/plugins.rs",
            "crates/policy/Cargo.toml",
            "crates/policy/src/lib.rs",
            "crates/policy/src/outputs.rs",
            "crates/policy/src/canary.rs",
            "crates/policy/src/text.rs",
            "crates/policy/src/tables.rs",
            "crates/policy/src/pyre.rs",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/transfer.rs",
            "xtask/src/rust14.rs",
        ],
        fixtures: &[
            "tests/fixtures/live/chat.json",
            "tests/fixtures/live/mail.json",
            "tests/fixtures/live/sources.yaml",
            "tests/fixtures/live/tickets.json",
            "tests/fixtures/live/listen/chat.json",
            "tests/fixtures/live/listen/sources.yaml",
            "tests/fixtures/notifier/fake-notifier.sh",
        ],
    },
    Step {
        // rust-20 uses its real Python tutor oracle and synthetic native component.
        id: "rust-20",
        oracle: &["-I", "-B", "tests/oracles/rust20.py", "--all"],
        cases: &[],
        candidate: Some("target/release/tutor-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust20.py",
            "xtask/tests/fixtures/depvec/driver.py",
            "src/careerbrew/notion_import.py",
            "src/careerbrew/tutor/cards.py",
            "src/careerbrew/tutor/public.py",
            "src/careerbrew/tutor/engine.py",
            "src/careerbrew/tutor/mistakes.py",
            "src/careerbrew/jobs/strategy.py",
            "src/careerbrew/prep.py",
            "src/careerbrew/work/evidence.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/plugins.py",
            "src/careerbrew/privacy.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "src/careerbrew/cli.py",
            "Cargo.toml",
            "Cargo.lock",
            "crates/tutor/Cargo.toml",
            "crates/tutor/src/lib.rs",
            "crates/tutor/src/fsrs.rs",
            "crates/tutor/src/cards.rs",
            "crates/tutor/src/public.rs",
            "crates/tutor/src/notion.rs",
            "crates/tutor/src/engine.rs",
            "crates/tutor/src/mistakes.rs",
            "crates/tutor/src/synthetic.rs",
            "crates/tutor/src/bin/tutor-component.rs",
            "crates/depvec/Cargo.toml",
            "crates/depvec/src/lib.rs",
            "crates/depvec/src/fsrs_schedule.rs",
            "crates/learn/Cargo.toml",
            "crates/learn/src/lib.rs",
            "crates/learn/src/cards.rs",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/memory/Cargo.toml",
            "crates/memory/src/lib.rs",
            "crates/memory/src/memory_sync.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/plugins.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/value.rs",
            "crates/policy/Cargo.toml",
            "crates/policy/src/lib.rs",
            "crates/policy/src/canary.rs",
            "crates/policy/src/text.rs",
            "crates/policy/src/tables.rs",
            "crates/policy/src/pyre.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/bulk.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/events.rs",
            "crates/vault/src/helpers.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/secrets.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/synthetic.rs",
            "crates/vault/src/transfer.rs",
            "xtask/src/rust20.rs",
        ],
        fixtures: &[
            "xtask/tests/fixtures/depvec/fsrs.json",
            "tests/fixtures/rust20-notion-bundle/manifest.json",
            "tests/fixtures/rust20-notion-bundle/pages.json",
            "tests/fixtures/rust20-notion-bundle/jobs.json",
            "tests/fixtures/rust20-notion-bundle/flashcards.json",
            "tests/fixtures/rust20-notion-bundle/plan.json",
            "tests/fixtures/rust20-notion-bundle/skills.json",
            "tests/fixtures/rust20-notion-bundle/stories.json",
            "tests/fixtures/rust20-notion-bundle/past_job.json",
            "knowledge/topics.yaml",
            "knowledge/rubrics/behavioral.yaml",
            "knowledge/rubrics/coding.yaml",
            "knowledge/rubrics/skill.yaml",
            "knowledge/rubrics/system-design.yaml",
            "prompts/card-explainer.md",
            "profile/examples/profile.yaml",
        ],
    },
    Step {
        id: "rust-21",
        oracle: &["-I", "-B", "tests/oracles/rust21.py", "--all"],
        cases: &[],
        candidate: Some("target/release/teacher-component"),
        http_contract: false,
        data_contract: false,
        role_contract: false,
        registry_projection: false,
        no_python: true,
        env: &[],
        sources: &[
            "tests/oracles/rust21.py",
            "src/careerbrew/teacher.py",
            "src/careerbrew/goals.py",
            "src/careerbrew/approvals.py",
            "src/careerbrew/interview.py",
            "src/careerbrew/tutor/cards.py",
            "src/careerbrew/tutor/public.py",
            "src/careerbrew/tutor/engine.py",
            "src/careerbrew/tutor/exercises.py",
            "src/careerbrew/tutor/mistakes.py",
            "src/careerbrew/jobs/strategy.py",
            "src/careerbrew/day/planner.py",
            "src/careerbrew/machine.py",
            "src/careerbrew/calendar.py",
            "src/careerbrew/config.py",
            "src/careerbrew/events.py",
            "src/careerbrew/keyfile.py",
            "src/careerbrew/paths.py",
            "src/careerbrew/store.py",
            "src/careerbrew/vault.py",
            "Cargo.toml",
            "Cargo.lock",
            "crates/teacher/Cargo.toml",
            "crates/teacher/src/lib.rs",
            "crates/teacher/src/goals.rs",
            "crates/teacher/src/seams.rs",
            "crates/teacher/src/teacher.rs",
            "crates/teacher/src/synthetic.rs",
            "crates/teacher/src/bin/teacher-component.rs",
            "crates/tutor/Cargo.toml",
            "crates/tutor/src/lib.rs",
            "crates/tutor/src/fsrs.rs",
            "crates/tutor/src/cards.rs",
            "crates/tutor/src/public.rs",
            "crates/tutor/src/engine.rs",
            "crates/tutor/src/mistakes.rs",
            "crates/projects/Cargo.toml",
            "crates/projects/src/lib.rs",
            "crates/memory/Cargo.toml",
            "crates/memory/src/lib.rs",
            "crates/memory/src/memory_sync.rs",
            "crates/setup/Cargo.toml",
            "crates/setup/src/lib.rs",
            "crates/setup/src/machine.rs",
            "crates/setup/src/value.rs",
            "crates/policy/Cargo.toml",
            "crates/policy/src/lib.rs",
            "crates/policy/src/allow.rs",
            "crates/policy/src/approvals.rs",
            "crates/policy/src/canary.rs",
            "crates/policy/src/vectors.rs",
            "crates/vault/Cargo.toml",
            "crates/vault/src/lib.rs",
            "crates/vault/src/atomic.rs",
            "crates/vault/src/canon.rs",
            "crates/vault/src/json_yaml.rs",
            "crates/vault/src/keyfile.rs",
            "crates/vault/src/lock.rs",
            "crates/vault/src/store.rs",
            "crates/vault/src/transfer.rs",
            "xtask/src/rust21.rs",
        ],
        fixtures: &[
            "tests/fixtures/rust21/runner.json",
            "tests/fixtures/rust21/loops.json",
            "tests/fixtures/rust21/exercises/syn-001.yaml",
            "tests/fixtures/rust21/exercises/syn-002.yaml",
            "tests/fixtures/rust21/exercises/syn-003.yaml",
            "tests/fixtures/rust21/exercises/syn-004.yaml",
            "knowledge/teaching.yaml",
        ],
    },
    Step {
        // rust-04 is a vector step (privacy, policy and approval decisions).
        // Its Python oracle `--all` runs every set over the generator's
        // vectors in one isolated process. `verify` routes the step to the
        // vector-driver kind (`vecdriver::RUST_04`); `measure` times the
        // `policy-vectors --all <file>` candidate over a file the same
        // generator writes (`measure::CANDIDATES`).
        id: "rust-04",
        oracle: &["-I", "-B", "tests/oracles/rust04.py", "--all"],
        cases: &[],
        candidate: Some("target/release/policy-vectors"),
        // As for rust-02: a vector step reaches no CLI contract.
        http_contract: false,
        data_contract: true,
        role_contract: true,
        registry_projection: false,
        no_python: false,
        env: &[],
        sources: &[
            "tests/oracles/rust04.py",
            "tests/oracles/rust04_vectors.py",
            "src/careerbrew/privacy.py",
            "src/careerbrew/approvals.py",
            "src/careerbrew/work/access.py",
        ],
        fixtures: &[],
    },
];

/// rust-03's CLI cases: the `actions` listing in both forms and its filters,
/// two argparse usage errors (exit 2; their stderr is pinned in
/// `crates/cbcli/tests/cli.rs`), and static help at the root, a group, a
/// leaf and `actions` itself. `actions-json` stays first.
pub const RUST_03_CASES: &[(&str, &[&str])] = &[
    ("actions-json", &["actions", "--json"]),
    ("actions-text", &["actions"]),
    ("actions-name-json", &["actions", "config.set", "--json"]),
    ("actions-prefix-json", &["actions", "day", "--json"]),
    ("actions-prefix-text", &["actions", "work"]),
    ("actions-none-json", &["actions", "nosuch", "--json"]),
    ("actions-none-text", &["actions", "nosuch"]),
    ("actions-bad-flag", &["actions", "--bogus"]),
    ("actions-extra-pos", &["actions", "a", "b"]),
    ("help-root", &["--help"]),
    ("help-short", &["-h"]),
    ("help-group", &["day", "--help"]),
    ("help-leaf", &["config", "set", "--help"]),
    ("help-actions", &["actions", "--help"]),
];

/// The contracts a CLI step can scope out, in record order.
pub const SCOPED: &[&str] = &["http", "data", "role"];

impl Step {
    /// The scoped contracts this step leaves out: `verify` lists them as
    /// `missing_contracts`, and `decide` requires that exact list.
    pub fn scoped_out(&self) -> Vec<&'static str> {
        SCOPED
            .iter()
            .zip([self.http_contract, self.data_contract, self.role_contract])
            .filter(|(_, on)| !on)
            .map(|(name, _)| *name)
            .collect()
    }
}

pub fn step(id: &str) -> Result<&'static Step, String> {
    STEPS
        .iter()
        .find(|step| step.id == id)
        .ok_or_else(|| format!("unknown step id: {id}"))
}

/// Nearest-rank median.
pub(crate) fn p50(samples: &[u64]) -> u64 {
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    sorted[sorted.len().div_ceil(2) - 1]
}

pub(crate) fn toolchain(root: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(root.join("rust-toolchain.toml"))
        .map_err(|_| "cannot read rust-toolchain.toml")?;
    text.lines()
        .find_map(|line| line.trim().strip_prefix("channel = "))
        .map(|value| value.trim_matches('"').to_owned())
        .ok_or_else(|| "rust-toolchain.toml has no channel".to_owned())
}

pub(crate) fn revision(root: &Path) -> Result<String, String> {
    let mut command = Command::new("/usr/bin/git");
    command
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .env_clear();
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    if exit != Some(0) || !stderr.is_empty() {
        return Err("cannot observe source revision".into());
    }
    Ok(stdout.trim().to_owned())
}

/// True when `git status --porcelain` lists any change.
pub(crate) fn dirty(root: &Path) -> Result<bool, String> {
    let mut command = Command::new("/usr/bin/git");
    command
        .args(["status", "--porcelain"])
        .current_dir(root)
        .env_clear();
    let (_, exit, stdout, stderr) = capture_actions::execute(command)?;
    if exit != Some(0) || !stderr.is_empty() {
        return Err("cannot observe tree state".into());
    }
    Ok(!stdout.trim().is_empty())
}

/// The capture sources, the baseline and collector sources, and the step's
/// own sources.
fn hashed_sources(root: &Path, step: &Step) -> Result<Value, String> {
    let mut sources = hashed_sources_with(root, step.sources)?;
    bind_env(&mut sources, step)?;
    Ok(sources)
}

/// The source and fixture identity every producer shares with `decide`.
/// Producer-specific `sources_sha256` fields keep their separate coverage.
pub(crate) fn evidence_hashes(root: &Path, step: &Step) -> Result<(String, String), String> {
    let sources = hashed_sources(root, step)?;
    Ok((
        capture_actions::digest(sources.to_string().as_bytes()),
        fixtures_sha256(root, step)?,
    ))
}

/// Adds the step's extra environment to a source map, so a record's source
/// hash also binds the environment its runs had. A step with no extra
/// environment keeps its map unchanged.
pub(crate) fn bind_env(sources: &mut Value, step: &Step) -> Result<(), String> {
    if step.env.is_empty() {
        return Ok(());
    }
    let pairs: Vec<Value> = step.env.iter().map(|(k, v)| json!([k, v])).collect();
    sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?
        .insert("step-env".to_owned(), json!(pairs));
    Ok(())
}

/// The capture sources, the baseline and collector sources, and `extra`.
pub(crate) fn hashed_sources_with(root: &Path, extra: &[&str]) -> Result<Value, String> {
    let mut sources = capture_actions::sources(root)?;
    let map = sources
        .as_object_mut()
        .ok_or("capture sources are not a map")?;
    let base = ["xtask/src/baseline.rs", "xtask/src/resources.rs"];
    for name in base.iter().chain(extra) {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "baseline source missing")?;
        map.insert((*name).to_owned(), json!(capture_actions::digest(&bytes)));
    }
    Ok(sources)
}

/// One hash over the HTTP fixtures and the step's registered fixture files.
pub(crate) fn fixtures_sha256(root: &Path, step: &Step) -> Result<String, String> {
    let mut files = capture_actions::files(root, &root.join("tests/fixtures/http"), None)?;
    for name in step.fixtures {
        let bytes = std::fs::read(root.join(name)).map_err(|_| "evidence fixture missing")?;
        files.insert((*name).to_owned(), capture_actions::digest(&bytes));
    }
    Ok(capture_actions::digest(json!(files).to_string().as_bytes()))
}

pub fn record(
    root: &Path,
    id: &str,
    python: &Path,
    runs: usize,
    allow_few_runs: bool,
) -> Result<Value, String> {
    let step = step(id)?;
    if runs == 0 {
        return Err("--runs must be at least 1".into());
    }
    if runs < MIN_RUNS && !allow_few_runs {
        return Err(format!(
            "--runs {runs} is below the protocol minimum of {MIN_RUNS}"
        ));
    }
    if !python.is_absolute() {
        return Err("--python must be an absolute path".into());
    }
    if std::env::var_os("CAREERBREW_HOME").is_some_and(|home| home.is_empty()) {
        return Err("CAREERBREW_HOME is set but empty".into());
    }
    let python_version = capture_actions::version(python)?;
    let sources = hashed_sources(root, step)?;
    let fixtures = fixtures_sha256(root, step)?;
    let sources_sha256 = capture_actions::digest(sources.to_string().as_bytes());
    let args = step.oracle.iter().map(OsString::from).collect::<Vec<_>>();
    let (samples, summary, method) =
        sample(root, python, &args, runs, "oracle", "baseline", step.env)?;
    if hashed_sources(root, step)? != sources || fixtures_sha256(root, step)? != fixtures {
        return Err("a relevant source changed during the baseline".into());
    }
    let (os, _) = resources::host_unit()?;
    let mut command = vec!["<python>".to_owned()];
    command.extend(step.oracle.iter().map(|arg| (*arg).to_owned()));
    Ok(json!({
        "schema": SCHEMA,
        "step": step.id,
        "command": command,
        "os": os,
        "arch": std::env::consts::ARCH,
        "rust_toolchain": toolchain(root)?,
        "python_version": python_version.trim(),
        "source_revision": revision(root)?,
        "source_tree_dirty": dirty(root)?,
        "sources_sha256": sources_sha256.clone(),
        "evidence_sources_sha256": sources_sha256,
        "fixtures_sha256": fixtures,
        "cache_state": CACHE_STATE,
        "meets_protocol_minimum": runs >= MIN_RUNS,
        "tree_method": method,
        "runs": runs,
        "samples": samples,
        "summary": summary,
    }))
}

/// Runs `program args` `runs` times, each in a fresh synthetic home, through
/// the process-tree collector. Returns the raw samples, the p50 and p95
/// summary and the tree method. A run that does not exit 0 is refused.
pub(crate) fn sample(
    root: &Path,
    program: &Path,
    args: &[OsString],
    runs: usize,
    role: &str,
    kind: &str,
    env: &[(&str, &str)],
) -> Result<(Vec<Value>, Value, &'static str), String> {
    let mut samples = Vec::with_capacity(runs);
    let (mut cpu, mut rss, mut wall) = (Vec::new(), Vec::new(), Vec::new());
    let mut method = "unknown";
    for run in 1..=runs {
        let scratch = tempfile::tempdir().map_err(|_| format!("cannot create {kind} home"))?;
        let mut environment =
            capture_actions::owned_environment(root, &scratch.path().join("owned"))?;
        for (key, value) in env {
            environment.insert((*key).to_owned(), (*value).to_owned());
        }
        if environment
            .get("CAREERBREW_HOME")
            .is_none_or(|home| home.is_empty())
        {
            return Err("synthetic CAREERBREW_HOME is empty".into());
        }
        let usage = resources::measure_in(program, args, Some(&environment), Some(root))?;
        if !usage.passed() {
            return Err(format!("{role} run {run} did not exit 0; {kind} refused"));
        }
        method = usage.tree_method;
        let cpu_us = usage
            .tree_user_cpu_us
            .saturating_add(usage.tree_system_cpu_us);
        cpu.push(cpu_us);
        rss.push(usage.tree_max_rss_bytes);
        wall.push(usage.wall_us);
        samples.push(json!({
            "run": run,
            "tree_user_cpu_us": usage.tree_user_cpu_us,
            "tree_system_cpu_us": usage.tree_system_cpu_us,
            "tree_cpu_us": cpu_us,
            "tree_max_rss_bytes": usage.tree_max_rss_bytes,
            "descendants": usage.descendants,
            "wall_us": usage.wall_us,
            "parent_max_rss_bytes": usage.parent_max_rss_bytes,
            "rss_may_include_parent": usage.rss_may_include_parent(),
        }));
    }
    let summary = json!({
        "cpu_us_p50": p50(&cpu),
        "cpu_us_p95": baseline_verdict::cpu_p95(cpu.clone()),
        "tree_max_rss_bytes_p50": p50(&rss),
        "tree_max_rss_bytes_p95": baseline_verdict::cpu_p95(rss.clone()),
        "wall_us_p50": p50(&wall),
        "wall_us_p95": baseline_verdict::cpu_p95(wall.clone()),
    });
    Ok((samples, summary, method))
}

pub fn run(
    root: &Path,
    id: &str,
    python: &Path,
    runs: usize,
    allow_few_runs: bool,
    output: &Path,
) -> Result<Value, String> {
    if !output.is_absolute() {
        return Err("--out must be an absolute path".into());
    }
    if output.exists() {
        return Err("baseline output already exists".into());
    }
    let record = record(root, id, python, runs, allow_few_runs)?;
    capture_actions::write_receipt(&record, output)?;
    Ok(record)
}
