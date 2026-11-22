//! Component-only settings file IO over the invocation-scoped storage provider.
//! Schema, validation and settings locks remain outside this module.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::store::Store;
use num_bigint::BigInt;
use std::path::{Path, PathBuf};

pub fn settings_path(root: &Path) -> PathBuf {
    root.join("settings.yaml")
}

fn empty_map() -> Canon {
    Canon::Map(Vec::new())
}

fn truthy(value: &Canon) -> bool {
    match value {
        Canon::Null => false,
        Canon::Bool(value) => *value,
        Canon::Int(value) => *value != BigInt::from(0),
        Canon::Float(value) => *value != 0.0,
        Canon::Str(value) => !value.is_empty(),
        Canon::Date(_) | Canon::DateTime(_) => true,
        Canon::List(items) => !items.is_empty(),
        Canon::Map(items) => !items.is_empty(),
    }
}

/// The saved overrides. Missing, empty and other falsy YAML values read as an
/// empty map, matching `store.read_yaml(path, {}) or {}`.
pub fn read_settings(store: &Store) -> Result<Canon, StoreError> {
    let value = store.read_yaml(&settings_path(&store.root), empty_map(), true)?;
    Ok(if truthy(&value) { value } else { empty_map() })
}

pub fn write_settings(store: &Store, value: &Canon) -> Result<(), StoreError> {
    store.write_yaml(&settings_path(&store.root), value)
}
