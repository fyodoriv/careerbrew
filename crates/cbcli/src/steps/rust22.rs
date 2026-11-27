//! Native CLI handlers owned by `rust-22` (see `steps/mod.rs`).
use super::Ctx;
use crate::args::Parsed;
use crate::Outcome;

/// The route keys this module serves. Each one must be routed `native` in
/// `config/native-routes.txt` and have a parity case in
/// `crates/cbcli/tests/step_rust22.rs`.
pub const COMMANDS: &[&str] = &[];

/// Runs one of `COMMANDS`; None refuses (exit 64) and Python runs it.
pub fn run(_name: &str, _args: &Parsed, _ctx: &Ctx) -> Option<Outcome> {
    None
}
