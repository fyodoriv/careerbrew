//! Fixture preparation controls. The original inventory contract runs separately.

use std::collections::BTreeSet;

const ANCHOR_COMMIT: &str = "96b9a3a4dac73c685129d3ea90bab4ff82869921";
const ANCHOR_TREE: &str = "bec6c96a05326bbc510476bc43d947c701f9d60a";
const POLICY: &str = include_str!("fixtures/i01/pending-identities-v1.tsv");
const FILE: &str = "src/pending.rs\tfile\t0\t0\t0";
const REVIEWED_FILE: &str = "src/reviewed.rs\tfile\t0\t0\t0";
const REGION: &str = "web/panel.ts\tembedded-code\t1\t1\t0";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Review {
    Pending,
    Reviewed,
}

type Row = (&'static str, Review);

const EXACT: [Row; 3] = [
    (FILE, Review::Pending),
    (REVIEWED_FILE, Review::Reviewed),
    (REGION, Review::Pending),
];

fn pending(rows: &[Row]) -> BTreeSet<&'static str> {
    rows.iter()
        .filter_map(|&(identity, review)| (review == Review::Pending).then_some(identity))
        .collect()
}

fn matches_fixture_policy(policy: &str, rows: &[Row]) -> bool {
    let header = format!(
        "careerbrew-inventory-pending\t1\t{ANCHOR_COMMIT}\t{ANCHOR_TREE}\npath\tkind\tstart\tend\tcolumn\n"
    );
    let Some(body) = policy
        .strip_prefix(&header)
        .and_then(|body| body.strip_suffix('\n'))
    else {
        return false;
    };
    let mut identities = body.split('\n');
    let (Some(file), Some(region), None) =
        (identities.next(), identities.next(), identities.next())
    else {
        return false;
    };
    pending(rows) == BTreeSet::from([file, region])
}

#[test]
fn i01_fixture_accepts_the_exact_anchor_bound_pending_identities() {
    assert!(matches_fixture_policy(POLICY, &EXACT));

    for anchor in [ANCHOR_COMMIT, ANCHOR_TREE] {
        let wrong_anchor = POLICY.replacen(anchor, &"0".repeat(40), 1);
        assert!(!matches_fixture_policy(&wrong_anchor, &EXACT));
    }
}

#[test]
fn i01_fixture_refuses_a_same_kind_substitution_with_equal_counts() {
    let mut substituted = EXACT;
    substituted[0].1 = Review::Reviewed;
    substituted[1].1 = Review::Pending;
    assert_eq!(EXACT.map(|row| row.0), substituted.map(|row| row.0));
    for kind in ["\tfile\t", "\tembedded-code\t"] {
        let count = |rows: &[Row]| {
            pending(rows)
                .iter()
                .filter(|identity| identity.contains(kind))
                .count()
        };
        assert_eq!(count(&EXACT), 1);
        assert_eq!(count(&EXACT), count(&substituted));
    }
    assert!(!matches_fixture_policy(POLICY, &substituted));
}
