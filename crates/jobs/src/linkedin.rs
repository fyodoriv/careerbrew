//! `careerbrew.jobs.linkedin`: postings from LinkedIn job-alert subjects.
//!
//! rust-followup: `import_export` (the "Download your data" zip) needs a zip
//! reader, which the workspace lockfile does not hold yet.

use crate::sources::{self, Posting, Vocab};
use crate::Result;
use regex::Regex;
use std::path::Path;
use std::sync::OnceLock;

pub struct Alert {
    pub id: String,
    pub subject: String,
}

fn alert() -> &'static Regex {
    static ALERT: OnceLock<Regex> = OnceLock::new();
    ALERT.get_or_init(|| {
        Regex::new(
            r#"(?i)^(?:"[^"]+":\s*)?(?P<company>[^\-–]+?)\s+[\-–]\s+(?P<title>.+?)(?:\s+and\s+more)?$"#,
        )
        .expect("the alert pattern compiles")
    })
}

pub fn postings_from_alerts(msgs: &[Alert], vocab: &Vocab) -> Vec<Posting> {
    let mut out = Vec::new();
    for message in msgs {
        let Some(found) = alert().captures(message.subject.trim()) else {
            continue;
        };
        let title = found["title"].trim();
        let id = format!(
            "linkedin-alert:{}",
            &careerbrew_tutor::digest(&message.id)[..12]
        );
        let mut posting = Posting::new(
            &id,
            found["company"].trim(),
            title,
            "",
            title.to_lowercase().contains("remote"),
            "https://www.linkedin.com/jobs/",
            "",
        );
        posting.seniority = sources::seniority_of(title);
        posting.skills = sources::extract_skills(title, vocab);
        posting.origin = "user".into();
        posting.source_name = "LinkedIn alert".into();
        out.push(posting);
    }
    out
}

pub fn add_alert_postings(root: &Path, repo: &Path, msgs: &[Alert]) -> Result<usize> {
    let mut current = sources::load(root)?;
    let vocab = sources::skill_vocab(repo)?;
    let new = postings_from_alerts(msgs, &vocab)
        .into_iter()
        .filter(|p| !current.iter().any(|c| c.id == p.id))
        .collect::<Vec<_>>();
    let added = new.len();
    for posting in new {
        if !current.iter().any(|c| c.id == posting.id) {
            current.push(posting);
        }
    }
    sources::save(root, &current)?;
    Ok(added)
}
