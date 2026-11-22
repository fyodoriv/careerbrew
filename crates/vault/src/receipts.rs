//! Approval receipt IO over the pure rust-04 decisions. Writers hold the
//! shared receipt lock across load, validation, change and atomic write;
//! readers take no lock. Events are emitted only after write and lock release.

use crate::atomic::StoreError;
use crate::canon::Canon;
use crate::lock;
use crate::store::Store;
use careerbrew_policy::allow::PyError;
use careerbrew_policy::approvals;
use careerbrew_policy::vectors::Out;
use careerbrew_policy::Rules;
use num_bigint::BigInt;
use std::path::PathBuf;

#[derive(Debug)]
pub enum ReceiptError {
    Store(StoreError),
    Policy(PyError),
}

impl From<StoreError> for ReceiptError {
    fn from(error: StoreError) -> Self {
        Self::Store(error)
    }
}

impl From<PyError> for ReceiptError {
    fn from(error: PyError) -> Self {
        Self::Policy(error)
    }
}

impl From<std::io::Error> for ReceiptError {
    fn from(error: std::io::Error) -> Self {
        Self::Store(StoreError::Io(error))
    }
}

/// Invocation-scoped clock, id and event adapters. No implementation in this
/// module reads a wall clock, randomness, environment or a real terminal.
pub trait Runtime {
    fn now(&self) -> String;
    fn new_id(&self) -> String;
    fn emit(&self, event: &str, status: &str) -> Result<(), ReceiptError>;
}

pub struct Receipts<'a, 'root> {
    store: &'a Store<'root>,
    runtime: &'a dyn Runtime,
    path: PathBuf,
}

impl<'a, 'root> Receipts<'a, 'root> {
    pub fn new(store: &'a Store<'root>, runtime: &'a dyn Runtime) -> Self {
        Self {
            store,
            runtime,
            path: store.root.join("approvals.json"),
        }
    }

    pub fn load(&self) -> Result<Out, ReceiptError> {
        let loaded = self
            .store
            .read_json(&self.path, Canon::List(Vec::new()), true)?;
        Ok(if truthy(&loaded) {
            to_out(&loaded)?
        } else {
            Out::List(Vec::new())
        })
    }

    pub fn request(&self, title: &str, detail: &str, by: &str) -> Result<Out, ReceiptError> {
        // Python rejects an empty title before it reads the id or clock.
        approvals::request_title(title)?;
        let id = self.runtime.new_id();
        let now = self.runtime.now();
        std::fs::create_dir_all(&self.store.root)?;
        let item = lock::with_lock(&self.path, || -> Result<Out, ReceiptError> {
            let (item, saved) = approvals::request_item(
                &Rules::python(),
                &self.load()?,
                title,
                detail,
                by,
                &now,
                &id,
            )?;
            self.store
                .write_json(&self.path, &from_out(&Out::List(saved))?)?;
            Ok(item)
        })??;
        self.runtime.emit("approvals.request", "pending")?;
        Ok(item)
    }

    pub fn decide(&self, item_id: &str, decision: &str, note: &str) -> Result<Out, ReceiptError> {
        approvals::validate_decision(decision)?;
        std::fs::create_dir_all(&self.store.root)?;
        let decided = lock::with_lock(&self.path, || -> Result<Out, ReceiptError> {
            let (decided, saved) = approvals::decide_items_with(
                &Rules::python(),
                &self.load()?,
                item_id,
                decision,
                note,
                || self.runtime.now(),
            )?;
            self.store
                .write_json(&self.path, &from_out(&Out::List(saved))?)?;
            Ok(decided)
        })??;
        self.runtime.emit("approvals.decide", decision)?;
        Ok(decided)
    }

    pub fn pending(&self) -> Result<Vec<Out>, ReceiptError> {
        Ok(approvals::pending(&Rules::python(), &self.load()?)?)
    }

    pub fn guidance(&self, limit: i64) -> Result<Vec<Out>, ReceiptError> {
        Ok(approvals::guidance(&Rules::python(), &self.load()?, limit)?)
    }

    pub fn get(&self, item_id: &str) -> Result<Out, ReceiptError> {
        Ok(approvals::get_item(
            &Rules::python(),
            &self.load()?,
            item_id,
        )?)
    }
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

fn to_out(value: &Canon) -> Result<Out, ReceiptError> {
    Ok(match value {
        Canon::Null => Out::Null,
        Canon::Bool(value) => Out::Bool(*value),
        Canon::Int(value) => i64::try_from(value)
            .map(Out::Int)
            .unwrap_or_else(|_| Out::Num(value.to_string())),
        Canon::Float(value) => Out::Num(
            serde_json::Number::from_f64(*value)
                .ok_or(StoreError::Codec("approval-float"))?
                .to_string(),
        ),
        Canon::Str(value) => Out::Str(value.clone()),
        Canon::Date(value) | Canon::DateTime(value) => Out::Str(value.clone()),
        Canon::List(items) => Out::List(items.iter().map(to_out).collect::<Result<Vec<_>, _>>()?),
        Canon::Map(items) => Out::Dict(
            items
                .iter()
                .map(|(key, value)| {
                    let Canon::Str(key) = key else {
                        return Err(StoreError::Codec("approval-key").into());
                    };
                    Ok((key.clone(), to_out(value)?))
                })
                .collect::<Result<Vec<_>, ReceiptError>>()?,
        ),
    })
}

fn from_out(value: &Out) -> Result<Canon, ReceiptError> {
    Ok(match value {
        Out::Null => Canon::Null,
        Out::Bool(value) => Canon::Bool(*value),
        Out::Int(value) => Canon::Int(BigInt::from(*value)),
        Out::Num(value) => value
            .parse::<BigInt>()
            .map(Canon::Int)
            .or_else(|_| value.parse::<f64>().map(Canon::Float))
            .map_err(|_| StoreError::Codec("approval-number"))?,
        Out::Str(value) => Canon::Str(value.clone()),
        Out::List(items) => Canon::List(items.iter().map(from_out).collect::<Result<Vec<_>, _>>()?),
        Out::Dict(items) => Canon::Map(
            items
                .iter()
                .map(|(key, value)| Ok((Canon::Str(key.clone()), from_out(value)?)))
                .collect::<Result<Vec<_>, ReceiptError>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::os::unix::fs::MetadataExt;

    const KEY: &str = "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=";

    struct FakeRuntime {
        emitted: RefCell<Vec<(String, String)>>,
    }

    impl Runtime for FakeRuntime {
        fn now(&self) -> String {
            "2026-03-01T09:30:00+00:00".into()
        }
        fn new_id(&self) -> String {
            "a1b2c3d4".into()
        }
        fn emit(&self, event: &str, status: &str) -> Result<(), ReceiptError> {
            self.emitted
                .borrow_mut()
                .push((event.into(), status.into()));
            Ok(())
        }
    }

    #[test]
    fn request_and_decide_emit_after_a_readable_write() {
        let root = std::env::temp_dir().join(format!(
            "cb-receipts-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let keys = |_: bool| Ok(KEY.to_owned());
        let store = Store::new(&root, true, &keys);
        let runtime = FakeRuntime {
            emitted: RefCell::new(Vec::new()),
        };
        let receipts = Receipts::new(&store, &runtime);
        receipts
            .request(" Synthetic  request ", "detail", "agent")
            .unwrap();
        assert_eq!(receipts.pending().unwrap().len(), 1);
        receipts.decide("a1b2c3d4", "approved", " note ").unwrap();
        assert!(receipts.pending().unwrap().is_empty());
        assert_eq!(receipts.guidance(10).unwrap().len(), 1);
        assert_eq!(runtime.emitted.borrow().len(), 2);
        let path = root.join("approvals.json");
        let before = std::fs::read(&path).unwrap();
        let inode = std::fs::metadata(&path).unwrap().ino();
        let mtime = std::fs::metadata(&path).unwrap().mtime_nsec();
        assert!(receipts.decide("a1b2c3d4", "rejected", "").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().ino(), inode);
        assert_eq!(std::fs::metadata(&path).unwrap().mtime_nsec(), mtime);
        assert_eq!(runtime.emitted.borrow().len(), 2);
        std::fs::remove_dir_all(root).unwrap();
    }
}
