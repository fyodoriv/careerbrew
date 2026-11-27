//! careerbrew-native: the rust-03 pilot of the native CLI. It serves
//! `careerbrew actions [name] [--json]` and static `-h/--help` from the
//! generated registry, byte-equal to Python's argparse at 80 columns.
//! rust-26 adds the parity-tested read-only commands of merged domain crates
//! (`routed`). Anything else exits 64 (`UNSUPPORTED`), and the console
//! script runs it in Python and logs `cli.native_unsupported`.
pub mod events;
pub mod help;
pub mod registry;
pub mod render;
pub mod routed;

/// The exit code for argv outside the pilot.
pub const UNSUPPORTED: i32 = 64;

/// What one run prints and returns. `emit_event` is true when Python would
/// append its `cli.run` row (a successful `actions`).
#[derive(Debug, PartialEq)]
pub struct Outcome {
    pub stdout: String,
    pub stderr: String,
    pub code: i32,
    pub emit_event: bool,
}

fn done(stdout: String) -> Outcome {
    Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: false,
    }
}

/// The exit-64 refusal. It names only a registry command, never free text.
fn unsupported(args: &[String], reg: &registry::Registry) -> Outcome {
    let first = args.first().map(String::as_str).unwrap_or("");
    let shown = if !first.is_empty() && !first.contains(' ') && reg.help(first).is_some() {
        first
    } else {
        "argv"
    };
    Outcome {
        stdout: String::new(),
        stderr: format!("native: unsupported {shown}\n"),
        code: UNSUPPORTED,
        emit_event: false,
    }
}

/// The environment one run reads.
pub struct Env {
    pub columns: Option<String>,
    pub stdout_is_terminal: bool,
    pub caps: events::Caps,
}

/// How one `actions` argument reads, the way argparse reads it.
enum Token<'a> {
    Help,
    Json,
    Unknown(&'a str),
    Positional(&'a str),
    /// A form the pilot does not port (`--`, `--json=x`, `-hx`, `-1`, ...).
    Other,
}

fn token(arg: &str) -> Token<'_> {
    let prefix = |long: &str| arg.len() >= 3 && long.starts_with(arg);
    if arg == "-h" || (arg.starts_with("--") && prefix("--help")) {
        return Token::Help;
    }
    if arg.starts_with("--") && prefix("--json") {
        return Token::Json;
    }
    if !arg.starts_with('-') {
        return Token::Positional(arg);
    }
    let bytes = arg.as_bytes();
    let long_unknown = arg.len() > 2
        && arg.starts_with("--")
        && bytes[2].is_ascii_lowercase()
        && bytes[2..]
            .iter()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-');
    let short_unknown = bytes.len() == 2 && bytes[1].is_ascii_alphabetic() && bytes[1] != b'h';
    if long_unknown || short_unknown {
        Token::Unknown(arg)
    } else {
        Token::Other
    }
}

/// Text mode prints through Python's `privacy.redact`, which reads these
/// files in the data root. With any of them present (or the root not
/// resolvable) the listing goes to Python.
pub(crate) const REDACT_INPUTS: &[&str] =
    &["sensitive-terms.txt", "profile.yaml", "plugins.lock.json"];

fn actions(args: &[String], reg: &registry::Registry, env: &Env) -> Outcome {
    let mut json = false;
    let mut name: Option<&str> = None;
    let mut unknown: Vec<&str> = Vec::new();
    let mut help = false;
    for arg in args {
        match token(arg) {
            Token::Help => help = true,
            Token::Json => json = true,
            Token::Unknown(arg) => unknown.push(arg),
            Token::Positional(arg) if name.is_none() => name = Some(arg),
            Token::Positional(arg) => unknown.push(arg),
            Token::Other => return unsupported(&["actions".to_owned()], reg),
        }
    }
    let wide = help::width_is_80(env.columns.as_deref(), env.stdout_is_terminal);
    if help {
        return match reg.help("actions") {
            Some(page) if wide => done(page.to_owned()),
            _ => unsupported(&["actions".to_owned()], reg),
        };
    }
    if !unknown.is_empty() {
        return match reg.root_usage() {
            Some(usage) if wide => Outcome {
                stdout: String::new(),
                stderr: format!(
                    "{usage}careerbrew: error: unrecognized arguments: {}\n",
                    unknown.join(" ")
                ),
                code: 2,
                emit_event: false,
            },
            _ => unsupported(&["actions".to_owned()], reg),
        };
    }
    // Python writes the event row into the data root after printing; a root
    // it cannot resolve would crash it there, so such a run goes to Python.
    if env.caps.events_on && env.caps.home.is_none() {
        return unsupported(&["actions".to_owned()], reg);
    }
    let rows = render::select(&reg.rows, name);
    let stdout = if json {
        render::json_listing(&rows)
    } else {
        let Some(root) = env.caps.data_root() else {
            return unsupported(&["actions".to_owned()], reg);
        };
        if REDACT_INPUTS.iter().any(|file| root.join(file).exists()) {
            return unsupported(&["actions".to_owned()], reg);
        }
        render::text_listing(&rows)
    };
    Outcome {
        stdout,
        stderr: String::new(),
        code: 0,
        emit_event: true,
    }
}

/// One run: `args` are the arguments after the program name.
pub fn run(args: &[String], env: &Env) -> Outcome {
    let reg = match registry::Registry::load() {
        Ok(reg) => reg,
        Err(_) => {
            return Outcome {
                stdout: String::new(),
                stderr: "native: the compiled-in registry does not parse\n".to_owned(),
                code: UNSUPPORTED,
                emit_event: false,
            }
        }
    };
    if args.first().map(String::as_str) == Some("actions") {
        return actions(&args[1..], &reg, env);
    }
    if let Some(path) = help::path_of(args) {
        if let Some(page) = reg.help(&path) {
            if help::width_is_80(env.columns.as_deref(), env.stdout_is_terminal) {
                return done(page.to_owned());
            }
        }
    }
    if let Some(outcome) = routed::run(args, env) {
        return outcome;
    }
    unsupported(args, &reg)
}
