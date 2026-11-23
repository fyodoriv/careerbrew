//! Component-only HTTP sources, calendar and mail ports for rust-08.
//!
//! No production route selects this crate. Network reads use invocation-owned
//! recorded fixtures; credential values never enter differential reports.

pub mod calendar;
pub mod cli;
pub mod connectors;
pub mod http;
pub mod mail;
pub mod synthetic;

use careerbrew_setup::value;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceError {
    pub class: &'static str,
    pub message: String,
    pub status: Option<u16>,
}

impl SourceError {
    pub fn new(class: &'static str, message: impl Into<String>) -> Self {
        Self {
            class,
            message: message.into(),
            status: None,
        }
    }

    pub fn connector(message: impl Into<String>) -> Self {
        Self::new("ConnectorError", message)
    }

    pub fn http(status: u16) -> Self {
        Self {
            class: "HTTPError",
            message: format!("HTTP status {status}"),
            status: Some(status),
        }
    }

    pub fn missing() -> Self {
        Self::new("FileNotFoundError", "recorded fixture not found")
    }

    pub fn value(message: impl Into<String>) -> Self {
        Self::new("ValueError", message)
    }
}

impl From<std::io::Error> for SourceError {
    fn from(error: std::io::Error) -> Self {
        let class = if error.kind() == std::io::ErrorKind::NotFound {
            "FileNotFoundError"
        } else {
            "OSError"
        };
        Self::new(class, "source file operation failed")
    }
}

impl From<serde_json::Error> for SourceError {
    fn from(_: serde_json::Error) -> Self {
        Self::value("invalid JSON")
    }
}

impl From<careerbrew_vault::atomic::StoreError> for SourceError {
    fn from(_: careerbrew_vault::atomic::StoreError) -> Self {
        Self::new("StoreError", "source state write failed")
    }
}

impl From<careerbrew_vault::secrets::SecretError> for SourceError {
    fn from(error: careerbrew_vault::secrets::SecretError) -> Self {
        Self::new("SecretError", error.message)
    }
}

impl From<careerbrew_vault::transfer::TransferError> for SourceError {
    fn from(error: careerbrew_vault::transfer::TransferError) -> Self {
        Self::new(error.class, error.message)
    }
}

impl From<careerbrew_setup::SetupError> for SourceError {
    fn from(error: careerbrew_setup::SetupError) -> Self {
        Self::new(error.class, error.message)
    }
}

pub type Result<T> = std::result::Result<T, SourceError>;

pub fn context() -> Result<careerbrew_vault::transfer::Context> {
    Ok(careerbrew_vault::transfer::Context::from_env()?)
}

pub fn resolve(reference: &str) -> Result<String> {
    match careerbrew_vault::secrets::backend_of(reference)? {
        "env" => std::env::var(reference.strip_prefix("env:").unwrap_or_default())
            .map_err(|_| SourceError::new("SecretError", "environment secret is not set")),
        _ => Err(SourceError::new(
            "SecretError",
            "the synthetic component accepts env secret references only",
        )),
    }
}

pub fn secret_backend(reference: &str) -> Result<&'static str> {
    Ok(careerbrew_vault::secrets::backend_of(reference)?)
}

/// Effective host policy snapshot for one component invocation. `None` is the
/// existing unrestricted policy; a malformed list is never read as unrestricted.
pub fn allowed_hosts() -> Result<Option<Vec<String>>> {
    let policy = careerbrew_setup::plugins::effective_policy()?;
    match field(&policy, "allowed_hosts") {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Array(hosts)) => hosts
            .iter()
            .map(|host| {
                host.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| SourceError::value("allowed_hosts must contain strings"))
            })
            .collect::<Result<Vec<_>>>()
            .map(Some),
        Some(_) => Err(SourceError::value("allowed_hosts must be a list")),
    }
}

pub fn field<'a>(value: &'a serde_json::Value, key: &str) -> Option<&'a serde_json::Value> {
    value::get(value, key)
}

pub fn text(value: Option<&serde_json::Value>) -> String {
    value::text(value)
}

pub fn typed(value: serde_json::Value) -> serde_json::Value {
    value::typed(&value)
}
