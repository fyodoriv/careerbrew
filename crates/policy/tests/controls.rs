//! rust-04d controls on synthetic canaries only.
//!
//! - Planted defect: removing one `EXTRA_CASES` group must fail a case that
//!   names it (`extra-cases-<index>`), for every group that NFKC leaves
//!   load-bearing.
//! - The answer form is `json.dumps`: ASCII escapes and Python separators.
//! - The no-echo check refuses an output that holds term text.
use careerbrew_policy::{tables, text, vectors, Rules};
use serde_json::json;

/// The groups whose two ends stay single, different chars under NFKC and
/// simple lowercase (computed with CPython 3.12, Unicode 15.0.0). For each,
/// only the `EXTRA_CASES` group makes the two match.
const LOAD_BEARING: &[usize] = &[0, 3, 12, 14, 15, 16, 17, 18, 19, 20, 21];

/// The case for one group: a 3-char term (exact `re.IGNORECASE` match) and a
/// text with the group's other end in the middle.
fn case(index: usize) -> (String, String, String) {
    let group = tables::EXTRA_CASES[index];
    let ch = |n: u32| char::from_u32(n).unwrap();
    let term = format!("q{}q", ch(group[0]));
    let text_in = format!("q{}q", ch(group[group.len() - 1]));
    (format!("extra-cases-{index}"), term, text_in)
}

/// The names of the cases whose answer differs from CPython's rules.
fn failing(rules: &Rules) -> Vec<String> {
    let python = Rules::python();
    (0..tables::EXTRA_CASES.len())
        .map(case)
        .filter(|(_, term, text_in)| {
            let terms = vec![term.clone()];
            rules.find_leaks(text_in, &terms) != python.find_leaks(text_in, &terms)
        })
        .map(|(name, _, _)| name)
        .collect()
}

#[test]
fn every_extra_case_group_matches_under_python_rules() {
    let rules = Rules::python();
    for index in 0..tables::EXTRA_CASES.len() {
        let (name, term, text_in) = case(index);
        assert_eq!(
            rules.find_leaks(&text_in, &[term]).len(),
            1,
            "{name} must match"
        );
    }
}

#[test]
fn a_removed_extra_case_group_fails_and_names_its_case() {
    assert!(failing(&Rules::python()).is_empty());
    for &index in LOAD_BEARING {
        assert_eq!(
            failing(&Rules::without_extra_case(index)),
            [format!("extra-cases-{index}")],
            "removing group {index} must fail exactly its own case"
        );
    }
}

#[test]
fn the_load_bearing_list_follows_the_tables() {
    let lower_of = |c: char| {
        let n = text::normalize(&c.to_string());
        let mut chars = n.chars();
        match (chars.next(), chars.next()) {
            (Some(one), None) => Some(text::simple_lower(one)),
            _ => None,
        }
    };
    let found: Vec<usize> = (0..tables::EXTRA_CASES.len())
        .filter(|&i| {
            let group = tables::EXTRA_CASES[i];
            let a = lower_of(char::from_u32(group[0]).unwrap());
            let b = lower_of(char::from_u32(group[group.len() - 1]).unwrap());
            a.is_some() && b.is_some() && a != b
        })
        .collect();
    assert_eq!(found, LOAD_BEARING);
}

#[test]
fn answers_use_python_json_dumps_form() {
    let file = json!({
        "terms": ["Zyxwvq"],
        "sets": {"canary-core": [
            {"id": "n", "op": "normalize", "text": "\u{ff21}\u{200b}\u{e9}\n\"\\"},
            {"id": "bad", "op": "nope"},
            {"id": "missing", "op": "term-id"},
        ]},
    });
    let line = vectors::answer(&Rules::python(), "canary-core", &file).unwrap();
    assert_eq!(
        line,
        format!(
            "{{\"set\": \"canary-core\", \"unidata_version\": \"{}\", \"results\": [\
             {{\"id\": \"n\", \"output\": {{\"ok\": \"A\\u00e9\\n\\\"\\\\\"}}}}, \
             {{\"id\": \"bad\", \"output\": {{\"error\": {{\"class\": \"ValueError\", \"message\": \"unknown op nope\"}}}}}}, \
             {{\"id\": \"missing\", \"output\": {{\"error\": {{\"class\": \"KeyError\", \"message\": \"'term'\"}}}}}}]}}",
            vectors::UNICODE_VERSION
        )
    );
}

#[test]
fn an_output_that_holds_a_term_is_refused_by_case() {
    let file = json!({
        "terms": ["Zyxwvq"],
        "sets": {"canary-core": [{"id": "echo", "op": "normalize", "text": "a Zyxwvq b"}]},
    });
    assert_eq!(
        vectors::answer(&Rules::python(), "canary-core", &file),
        Err(vectors::Refusal::Echo(
            "canary-core/echo: output holds term text".into()
        ))
    );
}

#[test]
fn a_set_this_crate_does_not_answer_is_refused() {
    let file = json!({"terms": [], "sets": {"approvals": []}});
    assert_eq!(
        vectors::answer(&Rules::python(), "approvals", &file),
        Err(vectors::Refusal::UnknownSet("approvals".into()))
    );
}
