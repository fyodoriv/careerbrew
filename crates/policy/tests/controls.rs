//! rust-04d controls on synthetic canaries only.
//!
//! - Planted defect: removing one `EXTRA_CASES` group must fail a case that
//!   names it (`extra-cases-<index>`), for every group that NFKC leaves
//!   load-bearing.
//! - The answer form is `json.dumps`: ASCII escapes and Python separators.
//! - The no-echo check refuses an output that holds term text.
//! - rust-04e planted defects: the `<` removed from the mention lookbehind
//!   fails exactly `mentions-angle`; a receipt validator that accepts an
//!   unknown status fails exactly `validate-unknown-status`.
use careerbrew_policy::{tables, text, vectors, Rules};
use serde_json::{json, Value};

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
    let file = json!({"terms": [], "sets": {"no-such-set": []}});
    assert_eq!(
        vectors::answer(&Rules::python(), "no-such-set", &file),
        Err(vectors::Refusal::UnknownSet("no-such-set".into()))
    );
}

/// The ids of the cases whose output under `planted` differs from CPython's rules.
fn differing(planted: &Rules, set: &str, cases: Value) -> Vec<String> {
    let file = json!({"terms": ["Zyxwvq"], "sets": {set: cases}});
    let rows = |rules: &Rules| -> Vec<Value> {
        let line = vectors::answer(rules, set, &file).unwrap();
        let parsed: Value = serde_json::from_str(&line).unwrap();
        parsed["results"].as_array().unwrap().clone()
    };
    rows(&Rules::python())
        .into_iter()
        .zip(rows(planted))
        .filter(|(a, b)| a != b)
        .map(|(a, _)| a["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn a_mention_lookbehind_without_the_angle_fails_and_names_its_case() {
    let cases = json!([
        {"id": "mentions-slack", "op": "mentions", "text": "ping <@U1> and <@U9>"},
        {"id": "mentions-at", "op": "mentions", "text": "ask @alex and @bob.k-2"},
        {"id": "mentions-email-like", "op": "mentions", "text": "mail x@y.example"},
        {"id": "mentions-angle", "op": "mentions", "text": "<@lower> stays"},
        {"id": "mentions-none", "op": "mentions", "text": "no mentions"},
    ]);
    assert_eq!(
        differing(&Rules::without_mention_angle(), "access", cases),
        ["mentions-angle"]
    );
}

#[test]
fn a_validator_that_accepts_an_unknown_status_fails_and_names_its_case() {
    let rec = |id: &str, status: &str| {
        json!({"id": id, "title": "t", "detail": "", "by": "agent",
               "at": "2026-01-05T09:00:00+00:00", "status": status, "note": "",
               "decided_at": "2026-01-05T09:00:00+00:00"})
    };
    let cases = json!([
        {"id": "validate-valid", "op": "validate-records", "records": [rec("c1", "pending"), rec("c2", "approved")]},
        {"id": "validate-non-dict", "op": "validate-records", "records": ["text"]},
        {"id": "validate-unknown-status", "op": "validate-records", "records": [rec("c3", "cancelled")]},
        {"id": "validate-duplicate-id", "op": "validate-records", "records": [rec("c4", "approved"), rec("c4", "pending")]},
    ]);
    assert_eq!(
        differing(
            &Rules::accepting_unknown_status(),
            "receipt-validate",
            cases
        ),
        ["validate-unknown-status"]
    );
}

#[test]
fn scrub_public_drops_refs_and_replaces_terms() {
    let terms = vec!["Zyxwvq".to_owned()];
    let rules = Rules::python();
    let scrub = |t: &str| {
        careerbrew_policy::scrub::scrub_public(&rules, t, &terms, "an internal project").unwrap()
    };
    assert_eq!(
        scrub("see https://docs.example.com/a/b for details"),
        "see for details"
    );
    assert_eq!(
        scrub("we shipped the Zyxwvq launch"),
        "we shipped an internal project launch"
    );
    assert_eq!(scrub("pushed to https://x.example.com and"), "");
    assert_eq!(scrub("this and or that"), "this and that");
}

/// A 2 MB URL token: each repeat of a one-char node used to take one stack
/// frame, so `\S*` over it overflowed the stack. CPython gives
/// "see for details" in about 0.1 s for the same text.
#[test]
fn a_two_megabyte_token_is_scrubbed_and_searched_without_overflow() {
    let rules = Rules::python();
    let token = format!("https://x.example.com/{}", "a/".repeat(1_000_000));
    let text_in = format!("see {token} for details");
    assert!(token.len() >= 2_000_000);
    let terms = vec!["Zyxwvq".to_owned()];
    assert_eq!(
        careerbrew_policy::scrub::scrub_public(&rules, &text_in, &terms, "an internal project")
            .unwrap(),
        "see for details"
    );
    let url = careerbrew_policy::pyre::Regex::new(r"(?:https?|ftp)s?:/+\S*", true, &rules).unwrap();
    let chars: Vec<char> = text_in.chars().collect();
    let found = url.matches(&chars);
    assert_eq!(found.len(), 1);
    assert_eq!((found[0].0, found[0].1), (4, 4 + token.chars().count()));
    let word = careerbrew_policy::pyre::Regex::new(r"(\w+)$", false, &rules).unwrap();
    assert_eq!(
        word.findall_groups(&"b".repeat(2_000_000)),
        [vec!["b".repeat(2_000_000)]]
    );
}

/// `repr` follows `str.isprintable`: C1 controls and no-break space as
/// `\xNN`, the line separator as `\uNNNN`, a tag char as `\UNNNNNNNN`.
#[test]
fn an_unknown_approval_id_with_non_printable_chars_is_reprd_like_python() {
    let id = "a\u{85}\u{2028}\u{a0}\u{200b}\u{e0001}\u{e9}";
    assert_eq!(
        careerbrew_policy::approvals::py_repr(id),
        "'a\\x85\\u2028\\xa0\\u200b\\U000e0001\u{e9}'"
    );
}
