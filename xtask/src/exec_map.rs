//! Compare traced exec targets with the code inventory
//! (task `rust-00-inventory-process-observation`, slice 1).
//!
//! Input: one absolute exec target path per line, as a trace records the
//! first `execve` argument. A path under the checkout root must be an
//! inventory row; a path outside it is an external tool. A relative path or a
//! path with a `.` or `..` component cannot be mapped and counts as
//! unassigned, so the check fails closed.

use std::collections::BTreeSet;

#[derive(Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub assigned: usize,
    pub external: usize,
    pub unassigned: BTreeSet<String>,
}

pub fn compare(trace: &str, root: &str, inventory: &BTreeSet<String>) -> Report {
    compare_targets(
        trace
            .lines()
            .map(str::trim_end)
            .filter(|line| !line.is_empty()),
        root,
        inventory,
    )
}

pub fn compare_targets<'a>(
    targets: impl IntoIterator<Item = &'a str>,
    root: &str,
    inventory: &BTreeSet<String>,
) -> Report {
    let root = root.trim_end_matches('/');
    let mut report = Report::default();
    let targets: BTreeSet<&str> = targets.into_iter().collect();
    for target in targets {
        let unmappable =
            !target.starts_with('/') || target.split('/').any(|part| part == "." || part == "..");
        if unmappable {
            report.unassigned.insert(target.to_owned());
            continue;
        }
        match target
            .strip_prefix(root)
            .and_then(|rest| rest.strip_prefix('/'))
        {
            Some(relative) if inventory.contains(relative) => report.assigned += 1,
            Some(_) => {
                report.unassigned.insert(target.to_owned());
            }
            None => report.external += 1,
        }
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inventory() -> BTreeSet<String> {
        ["scripts/check.sh", "src/careerbrew/cli.py"]
            .into_iter()
            .map(str::to_owned)
            .collect()
    }

    #[test]
    fn listed_targets_and_external_tools_pass() {
        let trace = "/work/scripts/check.sh\n/usr/bin/python3\n/work/scripts/check.sh\n";
        let report = compare(trace, "/work/", &inventory());
        assert_eq!((report.assigned, report.external), (1, 1));
        assert!(report.unassigned.is_empty());
    }

    #[test]
    fn a_planted_unlisted_script_is_reported() {
        let trace = "/work/scripts/check.sh\n/work/scripts/planted.sh\n";
        let report = compare(trace, "/work", &inventory());
        assert_eq!(
            report.unassigned.into_iter().collect::<Vec<_>>(),
            ["/work/scripts/planted.sh"]
        );
    }

    #[test]
    fn unmappable_targets_fail_closed() {
        let trace = "scripts/check.sh\n/work/scripts/../scripts/check.sh\n/workx/a\n";
        let report = compare(trace, "/work", &inventory());
        assert_eq!(report.unassigned.len(), 2);
        assert_eq!(report.external, 1);
    }

    #[test]
    fn a_doubled_slash_after_a_trailing_slash_root_fails_closed() {
        let report = compare("/work//scripts/check.sh\n", "/work/", &inventory());
        assert_eq!(report.assigned, 0);
        assert_eq!(report.unassigned.len(), 1);
    }
}
