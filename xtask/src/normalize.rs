//! The per-case normalization registry for `verify`. Rules apply to both
//! sides' stdout before the byte compare. After the rules, a stdout that
//! still holds a local path (the repo root, the side's scratch folder or the
//! caller's HOME) refuses verify: an unregistered path is never compared.
use std::path::Path;

/// `synthetic-home`: the side's `CAREERBREW_HOME`, raw and canonical, reads
/// `<home>`.
pub const SYNTHETIC_HOME: &str = "synthetic-home";
pub const HOME_PLACEHOLDER: &str = "<home>";

/// (step, case, rule ids).
pub const REGISTRY: &[(&str, &str, &[&str])] = &[
    ("rust-03", "actions-json", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-text", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-name-json", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-prefix-json", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-prefix-text", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-none-json", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-none-text", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-bad-flag", &[SYNTHETIC_HOME]),
    ("rust-03", "actions-extra-pos", &[SYNTHETIC_HOME]),
    ("rust-03", "help-root", &[SYNTHETIC_HOME]),
    ("rust-03", "help-short", &[SYNTHETIC_HOME]),
    ("rust-03", "help-group", &[SYNTHETIC_HOME]),
    ("rust-03", "help-leaf", &[SYNTHETIC_HOME]),
    ("rust-03", "help-actions", &[SYNTHETIC_HOME]),
];

/// The registered rule ids of one case; none is an empty list.
pub fn rules(step: &str, case: &str) -> &'static [&'static str] {
    REGISTRY
        .iter()
        .find(|(s, c, _)| *s == step && *c == case)
        .map_or(&[], |(_, _, rules)| rules)
}

/// A path and its canonical form, longest first, without empty strings.
fn forms(path: &Path) -> Vec<String> {
    let mut forms = vec![path.to_string_lossy().into_owned()];
    if let Ok(canonical) = path.canonicalize() {
        forms.push(canonical.to_string_lossy().into_owned());
    }
    forms.retain(|form| form.len() > 1);
    forms.sort_by_key(|form| std::cmp::Reverse(form.len()));
    forms.dedup();
    forms
}

/// What one side ran with: its synthetic home and scratch folder.
pub struct Side<'a> {
    pub home: &'a Path,
    pub scratch: &'a Path,
}

/// Applies the case's rules, then refuses any remaining local path.
pub fn apply(rules: &[&str], side: &Side<'_>, root: &Path, stdout: &str) -> Result<String, ()> {
    let mut text = stdout.to_owned();
    for rule in rules {
        if *rule == SYNTHETIC_HOME {
            for form in forms(side.home) {
                text = text.replace(&form, HOME_PLACEHOLDER);
            }
        }
    }
    let mut local = forms(root);
    local.extend(forms(side.scratch));
    if let Some(home) = std::env::var_os("HOME") {
        local.extend(forms(Path::new(&home)));
    }
    if local.iter().any(|path| text.contains(path.as_str())) {
        return Err(());
    }
    Ok(text)
}
