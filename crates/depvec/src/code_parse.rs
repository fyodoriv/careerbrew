//! Vector set `code-parse`: does a teaching example parse? The Python side
//! is `scripts/check-card-code.py`'s check: `compile(code, id, "exec")` for
//! Python and esbuild's transform for JavaScript and TypeScript. Here
//! `ruff_python_parser` (target Python 3.12, the pinned version) and
//! `oxc_parser` with `oxc_semantic`'s syntax checks answer the same
//! question. The output is only "parsed" or an error.
//!
//! Known gap: `compile()` also runs scope checks (`return` outside a
//! function, a repeated argument name). `ruff_python_parser` leaves those
//! to ruff's own checker, so the vectors do not cover them
//! (`docs/rust-migration.md`, set `code-parse`).
use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_semantic::SemanticBuilder;
use oxc_span::SourceType;
use ruff_python_ast::PythonVersion;
use ruff_python_parser::{parse_unchecked, Mode, ParseOptions};
use serde_json::{json, Value};

pub fn run(op: &str, input: &Value) -> Result<Value, String> {
    match op {
        "python-parse" => python(input.as_str().ok_or("input is not text")?),
        "js-parse" => {
            let loader = input["loader"].as_str().ok_or("no loader")?;
            let code = input["code"].as_str().ok_or("no code")?;
            javascript(loader, code)
        }
        _ => Err("unknown op".to_owned()),
    }
}

fn python(code: &str) -> Result<Value, String> {
    let options = ParseOptions::from(Mode::Module).with_target_version(PythonVersion::PY312);
    if parse_unchecked(code, options).has_no_syntax_errors() {
        Ok(json!("parsed"))
    } else {
        Err("does not parse".to_owned())
    }
}

fn javascript(loader: &str, code: &str) -> Result<Value, String> {
    // The loaders `check-card-code.py` passes to esbuild, as ES modules.
    let source_type = match loader {
        "js" => SourceType::mjs(),
        "jsx" => SourceType::jsx(),
        "ts" => SourceType::ts(),
        "tsx" => SourceType::tsx(),
        _ => return Err("unknown loader".to_owned()),
    };
    let allocator = Allocator::default();
    let parsed = Parser::new(&allocator, code, source_type).parse();
    if parsed.fatal_error || parsed.diagnostics.has_errors() {
        return Err("does not parse".to_owned());
    }
    let checked = SemanticBuilder::new()
        .with_check_syntax_error(true)
        .build(&parsed.program);
    if !checked.diagnostics.has_errors() {
        Ok(json!("parsed"))
    } else {
        Err("does not parse".to_owned())
    }
}
