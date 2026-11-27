//! careerbrew-web (rust-25): the native localhost HTTP front for the
//! unchanged web UI. Opt-in only: `careerbrew config set web_server native`.
//! The Python server stays the default.

pub mod cases;
pub mod http;
pub mod json;
pub mod mime;
pub mod server;
pub mod token;
pub mod version;
