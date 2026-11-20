//! `careerbrew-policy`: the rust-04 policy core (rust-04d).
//!
//! Pure decisions from `src/careerbrew/privacy.py` over supplied inputs: the
//! canary engine (`term_id`, `normalize`, `term_pattern`, `find_leaks`,
//! `leak_spans`, `_match_kind`), `sensitive_terms`' filter rules and the
//! allowlists (`_identifier_shaped`, `public_suspects` and their sets).
//!
//! No IO: no files, environment, processes, network, clock or randomness.
//! The gate step `rust-policy-io` (`scripts/policy-no-io.py`) refuses any of
//! them in this library. The caller supplies terms, texts and records.
//! Production use: none.

pub mod allow;
pub mod canary;
pub mod tables;
pub mod text;
pub mod vectors;

pub use canary::{find_leaks, leak_spans, term_id, term_pattern_source, Rules, Span};
pub use text::normalize;
