//! The noise floor registry for `decide`, one entry per step and OS, and the
//! `migration noise-floor --baseline <file>` measurement that proposes one.
//! Rules (docs/rust-migration.md, "Noise floors and normalization rules"):
//! below a measured floor the candidate must be at or under the ceiling; at
//! or above it the 1.10 ratio applies. An unmeasured floor uses the ratio for
//! every case, which is stricter, never looser.
use crate::baseline_verdict::ResourceLimit;
use serde_json::{json, Value};
use std::path::Path;

pub enum Floor {
    /// No floor yet: every case uses the 1.10 ratio.
    Unmeasured,
    #[allow(dead_code)]
    Measured {
        cpu: ResourceLimit,
        rss: ResourceLimit,
        /// How it was measured and from which record (content-free).
        method: &'static str,
        record: &'static str,
    },
}

pub struct Entry {
    pub step: &'static str,
    pub os: &'static str,
    pub floor: Floor,
}

/// rust-02, rust-03 and rust-04 wait for hosted baseline runs on each OS
/// (tasks in TASKS.md).
pub const REGISTRY: &[Entry] = &[
    Entry {
        step: "rust-02",
        os: "macos",
        floor: Floor::Unmeasured,
    },
    Entry {
        step: "rust-02",
        os: "linux",
        floor: Floor::Unmeasured,
    },
    Entry {
        step: "rust-03",
        os: "macos",
        floor: Floor::Unmeasured,
    },
    Entry {
        step: "rust-03",
        os: "linux",
        floor: Floor::Unmeasured,
    },
    Entry {
        step: "rust-04",
        os: "macos",
        floor: Floor::Unmeasured,
    },
    Entry {
        step: "rust-04",
        os: "linux",
        floor: Floor::Unmeasured,
    },
];

/// The limits decide used, with their state label and provenance.
pub struct Chosen {
    pub cpu: ResourceLimit,
    pub rss: ResourceLimit,
    pub state: &'static str,
    pub method: Option<&'static str>,
    pub record: Option<&'static str>,
}

impl Chosen {
    /// The four limits as content-free JSON, in the test-floor shape.
    pub fn limits_json(&self) -> Value {
        json!({
            "cpu_floor_us": self.cpu.floor,
            "cpu_absolute_us": self.cpu.absolute,
            "rss_floor_bytes": self.rss.floor,
            "rss_absolute_bytes": self.rss.absolute,
        })
    }
}

/// The CPU and RSS limits for one step and OS, with their provenance.
pub fn limits(step: &str, os: &str) -> Result<Chosen, String> {
    let entry = REGISTRY
        .iter()
        .find(|entry| entry.step == step && entry.os == os)
        .ok_or("no noise floor entry for this step and OS")?;
    let ratio = ResourceLimit {
        floor: 0,
        absolute: 0,
    };
    Ok(match &entry.floor {
        Floor::Unmeasured => Chosen {
            cpu: ratio,
            rss: ratio,
            state: "unmeasured",
            method: None,
            record: None,
        },
        Floor::Measured {
            cpu,
            rss,
            method,
            record,
        } => Chosen {
            cpu: *cpu,
            rss: *rss,
            state: "measured",
            method: Some(method),
            record: Some(record),
        },
    })
}

/// Reads a test-only floor: `{cpu_floor_us, cpu_absolute_us,
/// rss_floor_bytes, rss_absolute_bytes}`.
pub fn from_test(value: &Value) -> Result<(ResourceLimit, ResourceLimit), String> {
    let field = |name: &str| {
        value[name]
            .as_u64()
            .ok_or_else(|| format!("malformed test noise floor {name}"))
    };
    Ok((
        ResourceLimit {
            floor: field("cpu_floor_us")?,
            absolute: field("cpu_absolute_us")?,
        },
        ResourceLimit {
            floor: field("rss_floor_bytes")?,
            absolute: field("rss_absolute_bytes")?,
        },
    ))
}

/// Nearest-rank percentile of sorted values, `pct` in whole percent.
fn rank(sorted: &[u64], pct: usize) -> u64 {
    let index = (sorted.len() * pct).div_ceil(100).max(1) - 1;
    sorted[index.min(sorted.len() - 1)]
}

/// Floor = 10 x (p95 - p5); ceiling = floor x 1.10.
fn floor_of(mut values: Vec<u64>) -> (u64, u64) {
    values.sort_unstable();
    let spread = rank(&values, 95) - rank(&values, 5);
    let floor = spread * 10;
    (floor, floor * 11 / 10)
}

/// Proposes a floor from one baseline record; content-free JSON.
pub fn measure(path: &Path) -> Result<Value, String> {
    if !path.is_absolute() {
        return Err("--baseline must be an absolute path".into());
    }
    let bytes = std::fs::read(path).map_err(|_| "cannot read the baseline record")?;
    let base: Value =
        serde_json::from_slice(&bytes).map_err(|_| "the baseline record is not JSON")?;
    if base["schema"].as_str() != Some(crate::baseline::SCHEMA) {
        return Err("the baseline record has the wrong schema".into());
    }
    let step = crate::baseline::step(base["step"].as_str().unwrap_or(""))?;
    let os = base["os"]
        .as_str()
        .filter(|os| matches!(*os, "macos" | "linux"))
        .ok_or("the baseline record has an unknown OS")?;
    if base["meets_protocol_minimum"].as_bool() != Some(true) {
        return Err("the baseline record is below the protocol minimum".into());
    }
    if base["source_tree_dirty"].as_bool() != Some(false) {
        return Err("the baseline record is from a dirty tree".into());
    }
    if base["cache_state"].as_str() != Some(crate::baseline::CACHE_STATE) {
        return Err("the baseline record has another cache state".into());
    }
    let samples = base["samples"]
        .as_array()
        .filter(|samples| samples.len() >= crate::baseline::MIN_RUNS)
        .ok_or("the baseline record has too few samples")?;
    if base["runs"].as_u64() != Some(samples.len() as u64) {
        return Err("the baseline run count differs from its samples".into());
    }
    let column = |name: &str| {
        samples
            .iter()
            .map(|sample| sample[name].as_u64().ok_or(format!("malformed {name}")))
            .collect::<Result<Vec<_>, _>>()
    };
    let (cpu_floor, cpu_absolute) = floor_of(column("tree_cpu_us")?);
    let (rss_floor, rss_absolute) = floor_of(column("tree_max_rss_bytes")?);
    Ok(json!({
        "step": step.id,
        "os": os,
        "samples": samples.len(),
        "method": "floor = 10 x (nearest-rank p95 - p5); ceiling = floor x 1.10",
        "cpu_floor_us": cpu_floor,
        "cpu_absolute_us": cpu_absolute,
        "rss_floor_bytes": rss_floor,
        "rss_absolute_bytes": rss_absolute,
    }))
}
