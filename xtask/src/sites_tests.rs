//! Controls for embedded-site and entry-point discovery.

use super::*;

const HEADER_LINE: &str = "kind,path,key,task\n";

fn files() -> Vec<(String, String)> {
    vec![
        (
            "scripts/a.sh".to_owned(),
            "echo hi\npython3 -c 'print(1)'\nnode -e 'x'\n".to_owned(),
        ),
        (
            "pyproject.toml".to_owned(),
            "[project.scripts]\ntool = \"pkg.cli:main\"\n\n[tool.x]\ny = 1\n".to_owned(),
        ),
        (
            "web/package.json".to_owned(),
            r#"{"scripts":{"build":"vite build"},"bin":{"t":"bin/t.js"}}"#.to_owned(),
        ),
        (
            "lefthook.yml".to_owned(),
            "pre-commit:\n  commands:\n    docs:\n      run: scripts/check-docs.sh\n".to_owned(),
        ),
        (
            "Agentfile.yaml".to_owned(),
            "commands:\n  - ./commands\nhooks:\n  - command: 'bash x.sh'\n".to_owned(),
        ),
        ("commands/go.md".to_owned(), "Run it.\n".to_owned()),
        ("hooks/pre-push".to_owned(), "#!/bin/sh\n".to_owned()),
        ("docs/notes.md".to_owned(), "python3 -c 'prose only'\n".to_owned()),
    ]
}

fn manifest() -> String {
    let mut out = HEADER_LINE.to_owned();
    for site in discover(&files()) {
        out.push_str(&format!(
            "{},{},\"{}\",rust-27\n",
            site.kind,
            site.path,
            site.key.replace('"', "\"\"")
        ));
    }
    out
}

#[test]
fn discovery_finds_each_kind_and_skips_prose() {
    let found = discover(&files());
    let kinds: BTreeSet<&str> = found.iter().map(|site| site.kind).collect();
    for kind in [
        "embedded",
        "pyproject-script",
        "npm-script",
        "npm-bin",
        "lefthook-run",
        "agent-hook",
        "agent-command",
        "git-hook",
    ] {
        assert!(kinds.contains(kind), "missing kind {kind}");
    }
    assert!(found.iter().all(|site| site.path != "docs/notes.md"));
    assert_eq!(
        found.iter().filter(|site| site.path == "scripts/a.sh").count(),
        2
    );
}

#[test]
fn complete_assignments_pass() {
    let count = verify_sites(&discover(&files()), &manifest()).unwrap();
    assert_eq!(count, discover(&files()).len());
}

#[test]
fn unassigned_planted_site_is_refused() {
    let mut planted = files();
    planted[0].1.push_str("bash -c 'planted'\n");
    assert_eq!(
        verify_sites(&discover(&planted), &manifest()),
        Err("unassigned embedded site or entry point")
    );
}

#[test]
fn removed_site_is_refused() {
    let mut removed = files();
    removed[0].1 = "echo hi\nnode -e 'x'\n".to_owned();
    assert_eq!(
        verify_sites(&discover(&removed), &manifest()),
        Err("assigned site no longer exists")
    );
}

#[test]
fn duplicate_and_bad_task_rows_are_refused() {
    let base = manifest();
    let first = base.lines().nth(1).unwrap();
    assert_eq!(
        verify_sites(&discover(&files()), &format!("{base}{first}\n")),
        Err("duplicate site assignment")
    );
    let bad = base.replacen("rust-27", "later", 1);
    assert_eq!(
        verify_sites(&discover(&files()), &bad),
        Err("invalid site assignment")
    );
    assert_eq!(
        verify_sites(&discover(&files()), "kind,path\n"),
        Err("invalid site inventory header")
    );
}
