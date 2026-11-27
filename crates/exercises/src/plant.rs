//! Test-only planted defect for the differential gate.
//!
//! Scope: process. Only the synthetic component sets it, from its own
//! `CAREERBREW_RUST22_PLANT=accept-wrong-class` environment value, before
//! any run starts; nothing clears it, and it ends with the process. With it
//! set, the complexity decision accepts every class, so the cubic negative
//! controls must make the differential gate fail.

use crate::decide;
use serde_json::Value;
use std::sync::atomic::{AtomicBool, Ordering};

static ACCEPT_WRONG_CLASS: AtomicBool = AtomicBool::new(false);

pub fn accept_wrong_class() {
    ACCEPT_WRONG_CLASS.store(true, Ordering::SeqCst);
}

/// The complexity decision the runner uses (the real one unless planted).
pub fn complexity(spec: Option<&Value>, scale: &[Value]) -> Option<Value> {
    let mut decision = decide::complexity(spec, scale)?;
    if ACCEPT_WRONG_CLASS.load(Ordering::SeqCst) {
        decision["ok"] = Value::Bool(true);
    }
    Some(decision)
}
