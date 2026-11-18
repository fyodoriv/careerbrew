//! Controls for embedded-site and entry-point discovery.

use super::*;

const HEADER_LINE: &str = "kind,path,key,task\n";

fn files() -> Vec<(String, Vec<u8>)> {
    text_files()
        .into_iter()
        .map(|(path, text)| (path, text.into_bytes()))
        .collect()
}

fn found(files: &[(String, Vec<u8>)]) -> BTreeSet<Site> {
    discover(files).unwrap()
}

fn with(path: &str, text: &str) -> Vec<(String, Vec<u8>)> {
    let mut all = files();
    all.push((path.to_owned(), text.as_bytes().to_vec()));
    all
}

fn text_files() -> Vec<(String, String)> {
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
        (
            "docs/notes.md".to_owned(),
            "python3 -c 'prose only'\n".to_owned(),
        ),
    ]
}

fn manifest() -> String {
    let mut out = HEADER_LINE.to_owned();
    for site in found(&files()) {
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
    let found = found(&files());
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
        found
            .iter()
            .filter(|site| site.path == "scripts/a.sh")
            .count(),
        2
    );
}

#[test]
fn complete_assignments_pass() {
    let count = verify_sites(&found(&files()), &manifest()).unwrap();
    assert_eq!(count, found(&files()).len());
}

#[test]
fn unassigned_planted_site_is_refused() {
    let mut planted = files();
    planted[0].1.extend_from_slice(b"bash -c 'planted'\n");
    assert_eq!(
        verify_sites(&found(&planted), &manifest()),
        Err("unassigned embedded site or entry point")
    );
}

#[test]
fn removed_site_is_refused() {
    let mut removed = files();
    removed[0].1 = b"echo hi\nnode -e 'x'\n".to_vec();
    assert_eq!(
        verify_sites(&found(&removed), &manifest()),
        Err("assigned site no longer exists")
    );
}

#[test]
fn duplicate_and_bad_task_rows_are_refused() {
    let base = manifest();
    let first = base.lines().nth(1).unwrap();
    assert_eq!(
        verify_sites(&found(&files()), &format!("{base}{first}\n")),
        Err("duplicate site assignment")
    );
    let bad = base.replacen("rust-27", "later", 1);
    assert_eq!(
        verify_sites(&found(&files()), &bad),
        Err("invalid site assignment")
    );
    assert_eq!(
        verify_sites(&found(&files()), "kind,path\n"),
        Err("invalid site inventory header")
    );
}

#[test]
fn unlisted_real_interpreter_forms_are_found() {
    for (path, line) in [
        ("src/a.py", "argv = [\"sh\", \"-c\", OPEN_PR]"),
        ("src/b.py", "[sys.executable, \"-I\", \"-c\", HARNESS]"),
        ("web/e2e/c.ts", "spawn(py, [\"-B\", \"-c\", code])"),
        ("web/e2e/d.ts", "execFileSync(python, ['-c', code])"),
        ("scripts/e.sh", "zsh -c \"$cmd\""),
        ("scripts/f.sh", "/usr/bin/env python3 - \"$x\" <<EOF"),
    ] {
        let sites = found(&with(path, &format!("{line}\n")));
        assert!(
            sites.iter().any(|s| s.kind == "embedded" && s.path == path),
            "missed {line}"
        );
    }
}

#[test]
fn tool_flags_that_take_no_code_are_skipped() {
    let line = "run([\"git\", \"-c\", \"core.x=y\", \"status\"])\n";
    assert_eq!(found(&with("scripts/g.py", line)), found(&files()));
}

#[test]
fn a_repeated_identical_line_is_a_second_site() {
    let mut twice = files();
    twice[0].1.extend_from_slice(b"python3 -c 'print(1)'\n");
    assert_eq!(found(&twice).len(), found(&files()).len() + 1);
    assert_eq!(
        verify_sites(&found(&twice), &manifest()),
        Err("unassigned embedded site or entry point")
    );
}

#[test]
fn yaml_run_blocks_and_lefthook_bodies_are_found() {
    let workflow =
        "jobs:\n  a:\n    steps:\n      - run: |\n          echo hi\n      - run: make\n";
    let sites = found(&with(".github/workflows/x.yml", workflow));
    assert_eq!(sites.iter().filter(|s| s.kind == "workflow-run").count(), 2);
    let hook = "pre-push:\n  commands:\n    a:\n      run: |\n        python3 -c 'x'\n";
    let mut all = files();
    all.retain(|(p, _)| p != "lefthook.yml");
    all.push(("lefthook.yml".to_owned(), hook.as_bytes().to_vec()));
    let sites = found(&all);
    assert!(sites
        .iter()
        .any(|s| s.kind == "embedded" && s.path == "lefthook.yml"));
}

#[test]
fn skill_directories_and_flow_lists_are_entry_points() {
    let mut all = files();
    all.retain(|(p, _)| p != "Agentfile.yaml");
    all.push((
        "Agentfile.yaml".to_owned(),
        b"commands: [./commands]\nskillDirectories:\n  - ./skills\n".to_vec(),
    ));
    all.push(("skills/s1/SKILL.md".to_owned(), b"x\n".to_vec()));
    let sites = found(&all);
    assert!(sites
        .iter()
        .any(|s| s.kind == "agent-command" && s.path == "commands/go.md"));
    assert!(sites
        .iter()
        .any(|s| s.kind == "agent-skill" && s.path == "skills/s1/SKILL.md"));
    all.retain(|(p, _)| p != "Agentfile.yaml");
    all.push((
        "Agentfile.yaml".to_owned(),
        b"commands: ./commands\n".to_vec(),
    ));
    assert_eq!(discover(&all), Err("unreadable Agentfile entry list"));
}

#[test]
fn malformed_entry_point_files_fail_closed() {
    assert_eq!(
        discover(&with("web/x/package.json", "{not json")),
        Err("unreadable package.json")
    );
    let mut all = files();
    all.retain(|(p, _)| p != "pyproject.toml");
    all.push((
        "pyproject.toml".to_owned(),
        b"[project.gui-scripts]\ng = \"a:b\"\n[project.entry-points.\"x\"]\ne = \"c:d\"\n".to_vec(),
    ));
    let names: BTreeSet<String> = found(&all)
        .into_iter()
        .filter(|s| s.kind == "pyproject-script")
        .map(|s| s.key)
        .collect();
    assert!(names.contains("g") && names.contains("e"));
}

#[test]
fn non_utf8_executed_file_fails_closed() {
    let mut all = files();
    all.push(("scripts/bad.sh".to_owned(), vec![b'a', 0xff, b'\n']));
    assert_eq!(discover(&all), Err("non-UTF-8 file in an executed path"));
    let mut all = files();
    all.push(("docs/bad.bin".to_owned(), vec![0xff]));
    all.push(("web/visual/a.png".to_owned(), vec![0x89, 0xff, 0]));
    assert!(discover(&all).is_ok());
}

#[test]
fn list_rows_are_valid_manifest_rows() {
    let rows = list_rows(&found(&files()), &manifest());
    assert!(rows.starts_with(HEADER_LINE));
    assert_eq!(
        verify_sites(&found(&files()), &rows),
        Ok(found(&files()).len())
    );
}
