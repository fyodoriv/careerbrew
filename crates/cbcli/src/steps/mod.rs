//! Native CLI handlers, one module per migration step, so that parallel step
//! pull requests edit only their own file (and their own test file
//! `crates/cbcli/tests/step_<module>.rs`).
//!
//! A step module lists its route keys in `COMMANDS` and serves them in
//! `run`. Arguments arrive parsed by `crate::args` from the registry spec;
//! any form the reader is unsure of never reaches `run`. A handler returns
//! None to refuse (exit 64): Python then runs the command, so a handler may
//! serve only the cases it has parity cases for.
//!
//! The rules of `crate::routed` hold here too: a refusal must come before
//! any write, and a handler must not print anything Python would redact
//! unless it applies the same redaction (`Ctx::redact_inputs_present`).
use crate::args::Parsed;
use crate::registry::{Json, Registry};
use crate::{Env, Outcome, REDACT_INPUTS};
use std::path::PathBuf;

// The domain crates step modules may call, linked up front (Cargo.toml) so
// that parallel step pull requests never edit the manifest. A module that
// calls one makes its line redundant; leave the line until every step lands.
use careerbrew_career as _;
use careerbrew_day as _;
use careerbrew_exercises as _;
use careerbrew_feed as _;
use careerbrew_jobs as _;
use careerbrew_learn as _;
use careerbrew_live as _;
use careerbrew_memory as _;
use careerbrew_policy as _;
use careerbrew_projects as _;
use careerbrew_purevec as _;
use careerbrew_reports as _;
use careerbrew_sessions as _;
use careerbrew_sources as _;
use careerbrew_teacher as _;
use careerbrew_tutor as _;
use careerbrew_voice as _;
use careerbrew_web as _;
use careerbrew_workday as _;
use careerbrew_workexec as _;

pub mod rust02;
pub mod rust04;
pub mod rust05;
pub mod rust06;
pub mod rust07;
pub mod rust08;
pub mod rust09;
pub mod rust10;
pub mod rust12;
pub mod rust13;
pub mod rust14;
pub mod rust15;
pub mod rust16;
pub mod rust17;
pub mod rust18;
pub mod rust19;
pub mod rust20;
pub mod rust21;
pub mod rust22;
pub mod rust23;
pub mod rust24;
pub mod rust25;
pub mod unowned;

/// What a handler may read: the run's environment, the resolved data root
/// (also exported as `CAREERBREW_HOME`) and the registry.
pub struct Ctx<'a> {
    pub env: &'a Env,
    pub root: PathBuf,
    pub reg: &'a Registry,
}

impl Ctx<'_> {
    /// True when Python's `privacy.redact` would read an input file.
    pub fn redact_inputs_present(&self) -> bool {
        REDACT_INPUTS
            .iter()
            .any(|file| self.root.join(file).exists())
    }
}

/// One step's handlers.
pub struct Step {
    pub id: &'static str,
    pub commands: &'static [&'static str],
    pub run: fn(&str, &Parsed, &Ctx) -> Option<Outcome>,
}

pub const STEPS: &[Step] = &[
    Step {
        id: "rust-02",
        commands: rust02::COMMANDS,
        run: rust02::run,
    },
    Step {
        id: "rust-04",
        commands: rust04::COMMANDS,
        run: rust04::run,
    },
    Step {
        id: "rust-05",
        commands: rust05::COMMANDS,
        run: rust05::run,
    },
    Step {
        id: "rust-06",
        commands: rust06::COMMANDS,
        run: rust06::run,
    },
    Step {
        id: "rust-07",
        commands: rust07::COMMANDS,
        run: rust07::run,
    },
    Step {
        id: "rust-08",
        commands: rust08::COMMANDS,
        run: rust08::run,
    },
    Step {
        id: "rust-09",
        commands: rust09::COMMANDS,
        run: rust09::run,
    },
    Step {
        id: "rust-10",
        commands: rust10::COMMANDS,
        run: rust10::run,
    },
    Step {
        id: "rust-12",
        commands: rust12::COMMANDS,
        run: rust12::run,
    },
    Step {
        id: "rust-13",
        commands: rust13::COMMANDS,
        run: rust13::run,
    },
    Step {
        id: "rust-14",
        commands: rust14::COMMANDS,
        run: rust14::run,
    },
    Step {
        id: "rust-15",
        commands: rust15::COMMANDS,
        run: rust15::run,
    },
    Step {
        id: "rust-16",
        commands: rust16::COMMANDS,
        run: rust16::run,
    },
    Step {
        id: "rust-17",
        commands: rust17::COMMANDS,
        run: rust17::run,
    },
    Step {
        id: "rust-18",
        commands: rust18::COMMANDS,
        run: rust18::run,
    },
    Step {
        id: "rust-19",
        commands: rust19::COMMANDS,
        run: rust19::run,
    },
    Step {
        id: "rust-20",
        commands: rust20::COMMANDS,
        run: rust20::run,
    },
    Step {
        id: "rust-21",
        commands: rust21::COMMANDS,
        run: rust21::run,
    },
    Step {
        id: "rust-22",
        commands: rust22::COMMANDS,
        run: rust22::run,
    },
    Step {
        id: "rust-23",
        commands: rust23::COMMANDS,
        run: rust23::run,
    },
    Step {
        id: "rust-24",
        commands: rust24::COMMANDS,
        run: rust24::run,
    },
    Step {
        id: "rust-25",
        commands: rust25::COMMANDS,
        run: rust25::run,
    },
    Step {
        id: "unowned",
        commands: unowned::COMMANDS,
        run: unowned::run,
    },
];

/// Every route key a step module serves.
pub fn commands() -> impl Iterator<Item = &'static str> {
    STEPS.iter().flat_map(|step| step.commands.iter().copied())
}

/// The step command `args` names, longest registry key first (three words,
/// then two, then one), with the number of words it takes.
pub fn command_of(args: &[String]) -> Option<(&'static str, usize)> {
    for take in (1..=3.min(args.len())).rev() {
        if args[..take]
            .iter()
            .any(|word| word.starts_with('-') || word.contains('.'))
        {
            continue;
        }
        let key = args[..take].join(".");
        if let Some(name) = commands().find(|name| *name == key) {
            return Some((name, take));
        }
    }
    None
}

fn refuse(name: &str) -> Outcome {
    Outcome {
        stdout: String::new(),
        stderr: format!("native: unsupported {name}\n"),
        code: crate::UNSUPPORTED,
        emit_event: false,
    }
}

/// Runs a step command, or returns None when `args` names none.
pub fn run(args: &[String], env: &Env, reg: &Registry) -> Option<Outcome> {
    let (name, take) = command_of(args)?;
    let known = reg
        .rows
        .iter()
        .any(|row| row.get("name").and_then(Json::as_str) == Some(name));
    let Some(root) = env.caps.data_root().filter(|_| known) else {
        return Some(refuse(name));
    };
    if std::env::var_os("CAREERBREW_HOME").as_deref() != Some(root.as_os_str()) {
        return Some(refuse(name));
    }
    let Some(parsed) = crate::args::parse(reg, name, &args[take..]) else {
        return Some(refuse(name));
    };
    let step = STEPS.iter().find(|step| step.commands.contains(&name))?;
    let ctx = Ctx { env, root, reg };
    Some((step.run)(name, &parsed, &ctx).unwrap_or_else(|| refuse(name)))
}
