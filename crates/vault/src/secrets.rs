//! Secret references behind invocation-scoped adapters. Values never enter
//! argv, logs, events or files. This is the complete owned `secrets.py` slice.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SecretError {
    pub missing: bool,
    pub message: String,
}

impl SecretError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            missing: false,
            message: message.into(),
        }
    }

    fn missing(kind: &str) -> Self {
        Self {
            missing: true,
            message: format!("secret {kind} reference did not resolve (fix: `careerbrew connect <name>` or check the reference)"),
        }
    }

    pub fn keychain_write() -> Self {
        Self::new("could not write to the Keychain (fix: unlock the login keychain)")
    }
}

pub trait Adapter {
    /// `None` means the plugin policy has no backend restriction.
    fn allowed_backends(&self) -> Option<Vec<String>>;
    fn env(&self, name: &str) -> Option<String>;
    fn keychain_read(&self, service: &str, user: &str) -> Option<String>;
    fn op_read(&self, reference: &str) -> Option<String>;
    fn keychain_write(&self, service: &str, user: &str, value: &str) -> Result<(), SecretError>;
    fn user(&self) -> String;
    fn missing_event(&self, kind: &str);
}

pub fn backend_of(reference: &str) -> Result<&'static str, SecretError> {
    if reference.starts_with("keychain:") {
        Ok("keychain")
    } else if reference.starts_with("op://") {
        Ok("op")
    } else if reference.starts_with("env:") {
        Ok("env")
    } else {
        Err(SecretError::new(
            "a secret must be a reference (keychain:<service>, op://vault/item/field, or env:NAME), never a literal value",
        ))
    }
}

fn check_policy(adapter: &dyn Adapter, kind: &str) -> Result<(), SecretError> {
    if let Some(allowed) = adapter.allowed_backends() {
        if !allowed.iter().any(|item| item == kind) {
            let allowed = format!(
                "[{}]",
                allowed
                    .iter()
                    .map(|item| format!("'{item}'"))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            return Err(SecretError::new(format!(
                "secret backend {kind} is not allowed by plugin policy (allowed: {allowed})"
            )));
        }
    }
    Ok(())
}

pub fn resolve(adapter: &dyn Adapter, reference: &str) -> Result<String, SecretError> {
    let kind = backend_of(reference)?;
    check_policy(adapter, kind)?;
    let value = match kind {
        "env" => adapter.env(&reference[4..]),
        "keychain" => adapter.keychain_read(&reference[9..], &adapter.user()),
        _ => adapter.op_read(reference),
    };
    match value {
        Some(value) if !value.is_empty() => Ok(value),
        _ => {
            adapter.missing_event(kind);
            Err(SecretError::missing(kind))
        }
    }
}

pub fn store_keychain(
    adapter: &dyn Adapter,
    service: &str,
    value: &str,
) -> Result<String, SecretError> {
    check_policy(adapter, "keychain")?;
    adapter.keychain_write(service, &adapter.user(), value)?;
    Ok(format!("keychain:{service}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake;
    impl Adapter for Fake {
        fn allowed_backends(&self) -> Option<Vec<String>> {
            Some(vec!["env".into(), "keychain".into()])
        }
        fn env(&self, name: &str) -> Option<String> {
            (name == "SYNTHETIC").then(|| "synthetic".into())
        }
        fn keychain_read(&self, _: &str, _: &str) -> Option<String> {
            None
        }
        fn op_read(&self, _: &str) -> Option<String> {
            None
        }
        fn keychain_write(&self, _: &str, _: &str, _: &str) -> Result<(), SecretError> {
            Ok(())
        }
        fn user(&self) -> String {
            "synthetic".into()
        }
        fn missing_event(&self, _: &str) {}
    }

    #[test]
    fn literals_and_disallowed_backends_are_refused() {
        assert!(backend_of("literal").is_err());
        assert!(resolve(&Fake, "op://synthetic/item/field").is_err());
        assert_eq!(resolve(&Fake, "env:SYNTHETIC").unwrap(), "synthetic");
        assert_eq!(
            store_keychain(&Fake, "synthetic", "value").unwrap(),
            "keychain:synthetic"
        );
    }
}
