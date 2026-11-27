//! Pure rust-18 helpers: tag rules, keyword patterns, ranking and the scrub.

use careerbrew_career::{evidence, linkedin, promo};
use serde_json::json;
use std::collections::BTreeSet;

#[test]
fn tags_read_the_commit_kind_and_signal_words() {
    assert_eq!(
        evidence::tags_for("perf(api): cut latency"),
        ["impact", "quality"]
    );
    assert_eq!(evidence::tags_for("Mentored a hire"), ["leadership"]);
    assert!(evidence::tags_for("").is_empty());
}

#[test]
fn a_star_keyword_matches_a_word_start_and_a_plain_one_the_whole_word() {
    let comp = |k: &str| json!({"id": "c", "signals": {"keywords": [k]}});
    let item = |t: &str| json!({"title": t, "tags": []});
    assert!(promo::matches(&item("Migrating jobs"), &comp("migrat*")));
    assert!(!promo::matches(&item("remigrate"), &comp("migrat*")));
    assert!(!promo::matches(&item("shipped"), &comp("ship")));
    assert!(promo::matches(
        &json!({"title": "x", "tags": ["dim:c"]}),
        &comp("none")
    ));
}

#[test]
fn the_clean_drops_tickets_prefixes_repos_and_optional_numbers() {
    let repos: BTreeSet<String> = ["shopfront".to_owned()].into();
    let kept = linkedin::clean(
        "feat: add the shopfront export 2x faster",
        &repos,
        &[],
        true,
    )
    .unwrap();
    assert_eq!(kept, "Added an internal service export 2x faster.");
    let qualitative = linkedin::clean("cut $1.5m cost", &repos, &[], false).unwrap();
    assert_eq!(qualitative, "Cut significantly cost.");
    assert_eq!(linkedin::clean("fix: ", &repos, &[], true).unwrap(), "");
}

#[test]
fn impact_and_numbers_rank_first() {
    let plain = linkedin::rank(&json!({"title": "x", "tags": ["quality"], "at": "a"}));
    let strong =
        linkedin::rank(&json!({"title": "cut 40%", "impact": "y", "tags": ["impact"], "at": "b"}));
    assert_eq!(plain, (-2, 0, "a".to_owned()));
    assert_eq!(strong, (-10, 0, "b".to_owned()));
}
