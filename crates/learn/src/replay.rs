//! Replay small squash-merged PRs in shared clones under the learn data root.

use crate::{append_jsonl, error, field, project, read_json, stamp, text, write_json, Result};
use regex::Regex;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;
use wait_timeout::ChildExt;

pub const SMALL_FILES: usize = 12;
pub const SMALL_LINES: usize = 500;
pub const TEST_TIMEOUT: Duration = Duration::from_secs(300);

fn git(repo: &Path, args: &[&str], check: bool) -> Result<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()?;
    if check && !out.status.success() {
        let shown = args.iter().take(2).copied().collect::<Vec<_>>().join(" ");
        let message = String::from_utf8_lossy(&out.stderr)
            .trim()
            .chars()
            .take(200)
            .collect::<String>();
        return Err(error(
            "RuntimeError",
            format!(
                "git {shown} failed in {}: {message}",
                repo.file_name().unwrap_or_default().to_string_lossy()
            ),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

fn changes(repo: &Path, sha: &str) -> Result<Vec<(String, usize, String)>> {
    let stats = git(repo, &["show", "--numstat", "--format=", sha], true)?;
    let status = git(
        repo,
        &["show", "--name-status", "--format=", "--no-renames", sha],
        true,
    )?;
    let statuses = status
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut out = Vec::new();
    for line in stats.lines() {
        let mut parts = line.splitn(3, '\t');
        let add = parts.next().unwrap_or("0");
        let delete = parts.next().unwrap_or("0");
        let path = parts.next().unwrap_or("").to_owned();
        let changed = add.parse::<usize>().unwrap_or(0) + delete.parse::<usize>().unwrap_or(0);
        out.push((
            path.clone(),
            changed,
            statuses.get(path.as_str()).unwrap_or(&"M").to_string(),
        ));
    }
    Ok(out)
}

pub fn candidates(repo: &Path, limit: usize) -> Result<Vec<Value>> {
    let log = git(
        repo,
        &["log", "--first-parent", "--format=%H%x09%s", "HEAD"],
        true,
    )?;
    let pr_pattern = Regex::new(r"\(#(\d+)\)$").expect("PR regex");
    let test_pattern =
        Regex::new(r"(^|/)(tests?/.+\.py|test_[^/]+\.py|[^/]+_test\.py)$").expect("test regex");
    let mut out = Vec::new();
    for line in log.lines() {
        let (sha, subject) = line.split_once('\t').unwrap_or((line, ""));
        let Some(found) = pr_pattern.captures(subject) else {
            continue;
        };
        let files = changes(repo, sha)?;
        if files.len() > SMALL_FILES
            || files.iter().map(|(_, lines, _)| lines).sum::<usize>() > SMALL_LINES
        {
            continue;
        }
        let tests = files
            .iter()
            .filter(|(path, _, status)| {
                test_pattern.is_match(path) && status != "D" && !path.ends_with("conftest.py")
            })
            .map(|(path, _, _)| path.clone())
            .collect::<Vec<_>>();
        let sources = files
            .iter()
            .filter(|(path, _, _)| !tests.contains(path))
            .map(|(path, _, _)| path.clone())
            .collect::<Vec<_>>();
        let pr = found[1]
            .parse::<u64>()
            .map_err(|_| error("ValueError", "invalid replay PR number"))?;
        let title = pr_pattern.replace(subject, "").trim().to_owned();
        out.push(json!({"pr": pr, "sha": sha, "title": title, "tests": tests, "sources": sources, "files": files.len()}));
        if out.len() >= limit {
            break;
        }
    }
    Ok(out)
}

pub fn repo(root: &Path, project_name: &str, supplied: Option<&str>) -> Result<PathBuf> {
    if let Some(repo) = supplied {
        return PathBuf::from(repo)
            .canonicalize()
            .map_err(|_| error("FileNotFoundError", "replay repo does not exist"));
    }
    let manifest = project::manifest(root, project_name)?;
    let repo = manifest.get("sources").and_then(Value::as_array).into_iter().flatten().find(|source| text(field(source, "kind")) == "repo").ok_or_else(|| error("ValueError", format!("{project_name} has no staged repo: pass --repo PATH or run `careerbrew learn start {project_name} --repo PATH`")))?;
    Ok(PathBuf::from(text(field(repo, "path"))))
}

fn command(tests: &[String], python: Option<&str>) -> Vec<String> {
    let selected = python
        .map(str::to_owned)
        .or_else(|| std::env::var("CAREERBREW_RUST12_PYTHON").ok())
        .unwrap_or_else(|| "python3".into());
    vec![
        selected,
        "-m".into(),
        "pytest".into(),
        "-q".into(),
        "-x".into(),
        "-p".into(),
        "no:cacheprovider".into(),
        "-p".into(),
        "no:xdist".into(),
    ]
    .into_iter()
    .chain(tests.iter().cloned())
    .collect()
}

fn run(folder: &Path, command: &[String]) -> Result<(bool, String)> {
    let mut process = Command::new(&command[0]);
    process
        .args(&command[1..])
        .current_dir(folder)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    process.envs(
        std::env::vars()
            .filter(|(key, _)| !key.starts_with("CAREERBREW_") || key == "CAREERBREW_NOW"),
    );
    process.env_remove("PYTHONHOME");
    process.env(
        "PYTHONPATH",
        format!(
            "{}{}{}",
            folder.join("src").display(),
            if cfg!(windows) { ";" } else { ":" },
            folder.display()
        ),
    );
    let mut child = process.spawn()?;
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    let out = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout.read_to_end(&mut bytes).map(|_| bytes)
    });
    let err = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr.read_to_end(&mut bytes).map(|_| bytes)
    });
    let timed_out = child.wait_timeout(TEST_TIMEOUT)?.is_none();
    if timed_out {
        child.kill()?;
    }
    let status = child.wait()?;
    let stdout = out
        .join()
        .map_err(|_| error("RuntimeError", "replay output reader failed"))??;
    let stderr = err
        .join()
        .map_err(|_| error("RuntimeError", "replay error reader failed"))??;
    if timed_out {
        return Ok((
            false,
            format!("timed out after {} s", TEST_TIMEOUT.as_secs()),
        ));
    }
    let combined =
        String::from_utf8_lossy(&stdout).into_owned() + &String::from_utf8_lossy(&stderr);
    let last = combined.trim().lines().last().unwrap_or("").to_owned();
    Ok((status.success(), last))
}

fn clone(repo: &Path, dest: &Path, parent: &str, sha: &str, tests: &[String]) -> Result<()> {
    if dest.exists() {
        std::fs::remove_dir_all(dest)?;
    }
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("git")
        .args(["clone", "-q", "--shared", "--no-checkout"])
        .arg(repo)
        .arg(dest)
        .status()?;
    if !status.success() {
        return Err(error("RuntimeError", "git clone failed"));
    }
    git(dest, &["checkout", "-q", "--detach", parent], true)?;
    let mut args = vec!["checkout", sha, "--"];
    args.extend(tests.iter().map(String::as_str));
    git(dest, &args, true)?;
    Ok(())
}

fn pick(repo: &Path, pr: u64) -> Result<Value> {
    for candidate in candidates(repo, 10_000)? {
        if candidate["pr"].as_u64() == Some(pr) {
            return Ok(candidate);
        }
    }
    Err(error(
        "KeyError",
        format!(
            "no small squash-merged PR #{pr} in {}; see `careerbrew learn replay list`",
            repo.file_name().unwrap_or_default().to_string_lossy()
        ),
    ))
}

pub fn start(
    root: &Path,
    project_name: &str,
    pr: u64,
    supplied_repo: Option<&str>,
    python: Option<&str>,
) -> Result<Value> {
    let repo = repo(root, project_name, supplied_repo)?;
    let candidate = pick(&repo, pr)?;
    let tests = crate::strings(field(&candidate, "tests"));
    if tests.is_empty() {
        return Err(error("ValueError", format!("PR #{pr} changed no Python test file, so nothing can grade it; pick another (`careerbrew learn replay list`)")));
    }
    let sha = text(field(&candidate, "sha"));
    let parent = git(&repo, &["rev-parse", &format!("{sha}^")], true)?
        .trim()
        .to_owned();
    let dest = project::folder(root, project_name)
        .join("replay")
        .join(pr.to_string());
    clone(&repo, &dest, &parent, &sha, &tests)?;
    let task = json!({"pr": pr, "title": text(field(&candidate, "title")), "sha": sha, "parent": parent, "tests": tests, "dir": dest, "cmd": command(&tests, python), "started": stamp()});
    write_json(
        root,
        &format!("learn/{}/replay/{pr}.json", project::slug(project_name)),
        &task,
    )?;
    Ok(task)
}

pub fn check(root: &Path, project_name: &str, pr: u64) -> Result<Value> {
    let relative = format!("learn/{}/replay/{pr}.json", project::slug(project_name));
    let task = read_json(root, &relative, Value::Null)?;
    if task.is_null() {
        return Err(error("KeyError", format!("no replay task #{pr} in {project_name}; start it with `careerbrew learn replay start {project_name} {pr}`")));
    }
    let command = crate::strings(field(&task, "cmd"));
    let (passed, last) = run(Path::new(&text(field(&task, "dir"))), &command)?;
    let row = json!({"pr": pr, "passed": passed, "at": stamp()});
    append_jsonl(
        root,
        &format!("learn/{}/replay.jsonl", project::slug(project_name)),
        std::slice::from_ref(&row),
    )?;
    let mut answer = row.as_object().expect("replay row").clone();
    answer.insert("last".into(), json!(last));
    answer.insert(
        "tests".into(),
        field(&task, "tests").cloned().unwrap_or_else(|| json!([])),
    );
    Ok(Value::Object(answer))
}

pub fn runnable(repo: &Path, candidate: &Value, python: Option<&str>) -> Result<(bool, String)> {
    let tests = crate::strings(field(candidate, "tests"));
    if tests.is_empty() {
        return Ok((false, "no Python test file".into()));
    }
    if !crate::strings(field(candidate, "sources"))
        .iter()
        .any(|source| !source.ends_with(".md"))
    {
        return Ok((false, "only docs changed besides the tests".into()));
    }
    let sha = text(field(candidate, "sha"));
    let parent = git(repo, &["rev-parse", &format!("{sha}^")], true)?
        .trim()
        .to_owned();
    let temp = tempfile::Builder::new()
        .prefix("careerbrew-replay-")
        .tempdir()?;
    let dest = temp.path().join("clone");
    clone(repo, &dest, &parent, &sha, &tests)?;
    let command = command(&tests, python);
    let (before, _) = run(&dest, &command)?;
    if before {
        return Ok((false, "the tests already pass before the PR".into()));
    }
    git(&dest, &["checkout", "-q", "--detach", &sha], true)?;
    let (after, last) = run(&dest, &command)?;
    if after {
        Ok((true, "fails before, passes after".into()))
    } else {
        Ok((
            false,
            format!(
                "the tests fail on the PR itself: {}",
                last.chars().take(80).collect::<String>()
            ),
        ))
    }
}

pub fn measure(repo: &Path, limit: usize, python: Option<&str>) -> Result<Value> {
    let mut rows = Vec::new();
    for candidate in candidates(repo, limit)? {
        let (runnable, why) = runnable(repo, &candidate, python)?;
        rows.push(json!({"pr": candidate["pr"], "runnable": runnable, "why": why}));
    }
    let runnable = rows.iter().filter(|row| row["runnable"] == true).count();
    let share = if rows.is_empty() {
        Value::Null
    } else {
        json!(careerbrew_projects::python_round(
            runnable as f64 / rows.len() as f64,
            3
        ))
    };
    Ok(json!({"prs": rows.len(), "runnable": runnable, "share": share, "rows": rows}))
}
