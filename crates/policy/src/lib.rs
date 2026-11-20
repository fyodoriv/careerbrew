//! `careerbrew-policy`: the rust-04 policy core (rust-04d).
//!
//! Pure decisions from `src/careerbrew/privacy.py` over supplied inputs: the
//! canary engine (`term_id`, `normalize`, `term_pattern`, `find_leaks`,
//! `leak_spans`, `_match_kind`), `sensitive_terms`' filter rules and the
//! allowlists (`_identifier_shaped`, `public_suspects` and their sets), and
//! the canary outputs (`assert_clean` with ids only, `redact`,
//! `_names_private_path`; rust-04e), `scrub_public` (`scrub.rs`, on the
//! `re`-compatible matcher in `pyre.rs`), the approval decisions of
//! `approvals.py` (`approvals.rs`) and the access decisions of
//! `work/access.py` (`access.rs`).
//!
//! No IO: no files, environment, processes, network, clock or randomness.
//! The gate step `rust-policy-io` (`scripts/policy-no-io.py`) refuses any of
//! them in this library. The caller supplies terms, texts and records.
//! Production use: none.

pub mod access;
pub mod allow;
pub mod approvals;
pub mod canary;
pub mod outputs;
pub mod pyre;
pub mod scrub;
pub mod tables;
pub mod text;
pub mod vectors;

pub use canary::{find_leaks, leak_spans, term_id, term_pattern_source, Rules, Span};
pub use outputs::{assert_clean, names_private_path, redact};
pub use text::normalize;
