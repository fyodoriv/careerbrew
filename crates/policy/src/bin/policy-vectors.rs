//! `policy-vectors`: the rust-04 Rust driver (docs/rust-migration.md,
//! rust-04 "Vector contract"). The one IO edge of `careerbrew-policy`.
//!
//! - `policy-vectors unicode-version` prints the Unicode table version.
//! - `policy-vectors <set> <vectors-file>` prints the set's answer line, in
//!   the oracle's shape and `json.dumps` form.
//!
//! Exit 2 on a usage error, an unknown set or an unreadable file; exit 3
//! when an output holds term text (the message names the case only).
use careerbrew_policy::{vectors, Rules};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args == ["unicode-version"] {
        println!("{}", vectors::UNICODE_VERSION);
        return ExitCode::SUCCESS;
    }
    let [set, path] = args.as_slice() else {
        eprintln!("usage: policy-vectors unicode-version | <set> <vectors-file>");
        return ExitCode::from(2);
    };
    let Some(file) = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
    else {
        eprintln!("policy-vectors: cannot read the vectors file");
        return ExitCode::from(2);
    };
    match vectors::answer(&Rules::python(), set, &file) {
        Ok(line) => {
            println!("{line}");
            ExitCode::SUCCESS
        }
        Err(vectors::Refusal::Echo(case)) => {
            eprintln!("{case}");
            ExitCode::from(3)
        }
        Err(vectors::Refusal::UnknownSet(name)) => {
            eprintln!("unknown set: {name}");
            ExitCode::from(2)
        }
        Err(vectors::Refusal::Malformed) => {
            eprintln!("malformed vectors file");
            ExitCode::from(2)
        }
    }
}
