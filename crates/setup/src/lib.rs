//! Component-only plugin, machine, account and setup ports for rust-07.
//!
//! No production route selects this crate. Filesystem state is scoped to the
//! invocation's synthetic data root; credential values never enter reports.

pub mod cli;
pub mod machine;
pub mod oauth;
pub mod plugins;
pub mod preflight;
pub mod synthetic;
pub mod value;
pub mod workflow;

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetupError {
    pub class: &'static str,
    pub message: String,
}

impl SetupError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
        }
    }

    pub fn plugin(message: impl Into<String>) -> Self {
        Self::new("PluginError", message)
    }

    pub fn role(message: impl Into<String>) -> Self {
        Self::new("RoleError", message)
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }

    pub fn io() -> Self {
        Self::new("OSError", "setup file operation failed")
    }
}

impl From<std::io::Error> for SetupError {
    fn from(_: std::io::Error) -> Self {
        Self::io()
    }
}

impl From<careerbrew_vault::atomic::StoreError> for SetupError {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "encrypted store operation failed")
    }
}

impl From<careerbrew_vault::secrets::SecretError> for SetupError {
    fn from(error: careerbrew_vault::secrets::SecretError) -> Self {
        Self::new("SecretError", error.message)
    }
}

impl From<serde_json::Error> for SetupError {
    fn from(_: serde_json::Error) -> Self {
        Self::value("invalid JSON")
    }
}

impl From<careerbrew_vault::transfer::TransferError> for SetupError {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

pub type Result<T> = std::result::Result<T, SetupError>;

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn expand_user(path: &Path, home: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    if text == "~" {
        home.to_path_buf()
    } else if let Some(rest) = text.strip_prefix("~/") {
        home.join(rest)
    } else {
        path.to_path_buf()
    }
}

pub fn resolved(path: &Path) -> PathBuf {
    if path.exists() || path.is_symlink() {
        path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
    } else if let Some(parent) = path.parent() {
        let base = resolved(parent);
        base.join(path.file_name().unwrap_or_default())
    } else {
        path.to_path_buf()
    }
}

pub fn inside(path: &Path, root: &Path) -> bool {
    resolved(path).starts_with(resolved(root))
}
