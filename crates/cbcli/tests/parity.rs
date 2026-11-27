//! rust-26 end-to-end parity over every registry command.
//!
//! For each action in the generated registry:
//! - it has exactly one line in `config/native-routes.txt`;
//! - a `native` route is either `actions` (rust-03, `cli.rs` and US-111) or
//!   one of `routed::COMMANDS`, and has scenarios in `CASES`;
//! - in each scenario's fresh synthetic home, the console script with the
//!   committed routes, and the native binary run directly (when the scenario
//!   says it serves), give the same stdout, exit code, data-root files and
//!   content-free event rows as the console script with `CAREERBREW_NATIVE=off`.
//!
//! The Python side is the repo's own `.venv` interpreter running
//! `python -m careerbrew`. Telemetry sends stay off (`DO_NOT_TRACK=1`): the
//! native binary never sends (docs/decisions/0010-native-pilot-hooks.md).
mod common;

use careerbrew_cli::registry::Registry;
use careerbrew_cli::routed::COMMANDS;
use careerbrew_cli::steps;
use common::*;

const CONNECTORS: &str =
    "connectors:\n- kind: github\n  token: env:SYNTHETIC_GH\n- kind: jira\n  token: keychain:synthetic\n";

/// The routed commands that read only the data root (no backup).
const READERS: &[&str] = &[
    "machine.show",
    "plugin.list",
    "plugin.verify",
    "connectors.list",
    "data.status",
];

const ALL: &[&str] = &[
    "machine.show",
    "plugin.list",
    "plugin.verify",
    "connectors.list",
    "data.status",
    "backup.verify",
];

const SCENARIOS: &[Scenario] = &[
    Scenario {
        id: "empty-home",
        cli: &[],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "personal-labelled",
        cli: &[&["machine", "set", "personal", "--label", "desk-1"]],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "corporate",
        cli: &[&["machine", "set", "corporate"]],
        files: &[],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "connectors-plaintext",
        cli: &[],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "connectors-encrypted",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: READERS,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "backup-made",
        cli: &[
            &["machine", "set", "personal"],
            &["data", "encrypt"],
            &["backup", "create"],
        ],
        files: &[("work/connectors.yaml", CONNECTORS)],
        serves: ALL,
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "plugin-installed",
        cli: &[&["plugin", "add", "tests/fixtures/plugin-acme"]],
        files: &[],
        serves: &[],
        must_refuse: true,
        odd_key_path: false,
    },
    Scenario {
        id: "redaction-input",
        cli: &[],
        files: &[("profile.yaml", "name: Synthetic Person\n")],
        serves: &[],
        must_refuse: true,
        odd_key_path: false,
    },
    Scenario {
        id: "malformed-machine",
        cli: &[],
        files: &[("machine.yaml", "role: [\n")],
        serves: &[],
        must_refuse: false,
        odd_key_path: false,
    },
    Scenario {
        id: "odd-key-path",
        cli: &[&["machine", "set", "personal"], &["data", "encrypt"]],
        files: &[],
        serves: &["machine.show", "connectors.list"],
        must_refuse: false,
        odd_key_path: true,
    },
];

#[test]
fn every_registry_action_has_one_route_line() {
    let reg = Registry::load().unwrap();
    let names: Vec<String> = reg
        .rows
        .iter()
        .map(|row| row.get("name").and_then(|n| n.as_str()).unwrap().to_owned())
        .collect();
    let routed: Vec<String> = routes().into_iter().map(|(name, _)| name).collect();
    assert_eq!(names, routed, "run: uv run python scripts/gen-registry.py");
}

#[test]
fn every_native_route_has_a_native_handler() {
    let served = |name: &str| COMMANDS.contains(&name) || steps::commands().any(|c| c == name);
    for (name, route) in routes() {
        assert!(route == "native" || route == "python", "{name}: {route}");
        if route == "native" {
            assert!(
                name == "actions" || served(&name),
                "{name} is routed native but careerbrew-native has no handler for it"
            );
        }
    }
    for name in COMMANDS.iter().copied().chain(steps::commands()) {
        assert!(
            routes().iter().any(|(n, r)| n == name && r == "native"),
            "{name} has a native handler but is not a registry action routed native"
        );
    }
}

/// Each step module's commands have a parity test file that names each of
/// them, and no command is served by two modules.
#[test]
fn every_step_command_has_a_parity_test() {
    let mut seen: Vec<&str> = COMMANDS.to_vec();
    for step in steps::STEPS {
        if step.commands.is_empty() {
            continue;
        }
        let module = step.id.replace('-', "");
        let path = repo().join(format!("crates/cbcli/tests/step_{module}.rs"));
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("{} has commands but no {}", step.id, path.display()));
        for name in step.commands {
            assert!(!seen.contains(name), "{name} is served twice");
            seen.push(name);
            assert!(
                text.contains(&format!("\"{name}\"")),
                "{} names no case for {name}",
                path.display()
            );
        }
    }
}

#[test]
fn every_native_routed_command_matches_python_in_every_scenario() {
    let native: Vec<String> = routes()
        .into_iter()
        .filter(|(name, route)| route == "native" && COMMANDS.contains(&name.as_str()))
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        native.len(),
        COMMANDS.len(),
        "every routed handler is switched on"
    );
    let mut cases = 0;
    for scenario in SCENARIOS {
        let home = prepare(scenario);
        for name in &native {
            let args = argv(name);
            let want = observe(&home, &args, &Side::Python);
            let direct = observe(&home, &args, &Side::Native);
            let refused = direct.code == Some(64);
            if refused {
                assert!(
                    !scenario.serves.contains(&name.as_str()),
                    "{name} refused in {}, which it must serve",
                    scenario.id
                );
                assert!(direct.stdout.is_empty(), "{name} in {}", scenario.id);
                assert!(direct.events.is_empty(), "{name} in {}", scenario.id);
            } else {
                assert_eq!(
                    direct, want,
                    "{name} in careerbrew-native in {}",
                    scenario.id
                );
            }
            if scenario.must_refuse {
                assert!(refused, "{name} must refuse in {}", scenario.id);
            }
            let mut console = observe(&home, &args, &Side::Console);
            if refused {
                // A refusal is logged once, content-free, before Python runs.
                let refusal = console
                    .events
                    .iter()
                    .position(|row| row.contains("cli.native_unsupported"))
                    .unwrap_or_else(|| panic!("{name}: no refusal row in {}", scenario.id));
                console.events.remove(refusal);
            }
            assert_eq!(
                console, want,
                "{name} via the console script in {}",
                scenario.id
            );
            cases += 1;
        }
    }
    assert_eq!(cases, SCENARIOS.len() * COMMANDS.len());
}

fn scenario(id: &str) -> &'static Scenario {
    SCENARIOS.iter().find(|s| s.id == id).unwrap()
}

/// Negative controls: the comparison sees a real difference, and the
/// refusal rules fire. The corporate and personal homes print different
/// `machine show` lines; a flag the native handler does not take, and a key
/// path `pathlib` would fold, are refused, not served.
#[test]
fn the_parity_comparison_detects_a_planted_difference() {
    let personal = observe(
        &prepare(scenario("personal-labelled")),
        &["machine", "show"],
        &Side::Native,
    );
    let corporate = observe(
        &prepare(scenario("corporate")),
        &["machine", "show"],
        &Side::Python,
    );
    assert_ne!(personal, corporate);
    assert!(personal.stdout.starts_with("role: personal label: desk-1"));
    assert!(corporate.stdout.starts_with("role: corporate"));
    let empty = prepare(scenario("empty-home"));
    let extra = observe(&empty, &["machine", "show", "--json"], &Side::Native);
    assert_eq!(extra.code, Some(64));
    let odd = prepare(scenario("odd-key-path"));
    assert_eq!(
        observe(&odd, &["data", "status"], &Side::Native).code,
        Some(64)
    );
    let python = observe(&odd, &["data", "status"], &Side::Python);
    assert!(python.stdout.contains("/backup.key"));
    assert!(!python.stdout.contains("//backup.key"));
}
