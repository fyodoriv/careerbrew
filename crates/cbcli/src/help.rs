//! Static `--help` from the registry's pre-rendered pages (80 columns).

/// True when argparse would format at 80 columns, the width the registry
/// pages were rendered at: `COLUMNS=80`, or no `COLUMNS` and stdout is not a
/// terminal (argparse then falls back to 80). Any other case goes to Python.
pub fn width_is_80(columns: Option<&str>, stdout_is_terminal: bool) -> bool {
    match columns {
        Some(value) => value == "80",
        None => !stdout_is_terminal,
    }
}

/// A help request outside `actions`: the parser path before a lone trailing
/// `-h` or `--help`, or None when the arguments are anything else. A token
/// with whitespace is one argparse choice that no command has, so it goes to
/// Python; joining it would alias a multi-token path.
pub fn path_of(args: &[String]) -> Option<String> {
    let (last, path) = args.split_last()?;
    if last != "-h" && last != "--help" {
        return None;
    }
    if path.iter().any(|token| {
        token.is_empty() || token.starts_with('-') || token.chars().any(char::is_whitespace)
    }) {
        return None;
    }
    Some(path.join(" "))
}
