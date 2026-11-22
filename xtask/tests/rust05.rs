//! Registration controls for the custom rust-05 component runner.

#[test]
fn rust_05_registers_the_release_candidate_and_effective_inputs() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let baseline = std::fs::read_to_string(manifest.join("src/baseline.rs")).unwrap();
    assert!(baseline.contains("id: \"rust-05\""));
    assert!(baseline.contains("target/release/vault-store"));
    for source in [
        "tests/oracles/rust05.py",
        "crates/vault/src/store.rs",
        "crates/vault/src/receipts.rs",
        "crates/vault/src/cli.rs",
        "crates/vault/src/secrets.rs",
        "crates/vault/src/data.rs",
        "crates/vault/src/synthetic.rs",
        "xtask/src/rust05.rs",
    ] {
        assert!(baseline.contains(source), "missing source binding {source}");
    }
    let measure = std::fs::read_to_string(manifest.join("src/measure.rs")).unwrap();
    assert!(measure.contains("step: \"rust-05\""));
    assert!(measure.contains("target/release/vault-store"));
    let floors = std::fs::read_to_string(manifest.join("src/noise_floor.rs")).unwrap();
    assert_eq!(floors.matches("step: \"rust-05\"").count(), 2);
}

use std::path::Path;
