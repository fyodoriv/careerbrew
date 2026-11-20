//! rust-27 slice 2: the native port of `scripts/input-key.sh`, the gate
//! cache key. It prints 16 hex characters of a SHA-1 over `git ls-files -s`
//! of a temporary index that also records modified and untracked
//! (non-ignored) files, like the script. With pathspecs, the pathspecs
//! themselves (one per line) come first and only matching files count.
//! Any git error, or a folder outside a git checkout, gives exit 1 and no
//! output. It reads the repo only: `update-index --info-only` writes no
//! objects, and the temporary index is removed afterwards.
use std::ffi::OsString;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub struct Args {
    pub repo: PathBuf,
    pub pathspecs: Vec<OsString>,
}

/// `[--repo <dir>] [--] [pathspec...]`; `--repo` only as the first argument.
/// A leading `--` is a separator here; the script instead hashes it as a
/// pathspec line (`scripts/check.sh` never passes one).
pub fn parse(rest: &[OsString], default_repo: &Path) -> Option<Args> {
    let mut rest = rest;
    let mut repo = default_repo.to_path_buf();
    if rest.first().is_some_and(|a| a == "--repo") {
        repo = PathBuf::from(rest.get(1)?);
        rest = &rest[2..];
    }
    if rest.first().is_some_and(|a| a == "--") {
        rest = &rest[1..];
    }
    Some(Args {
        repo,
        pathspecs: rest.to_vec(),
    })
}

fn git(repo: &Path, scrub: &[String], index: Option<&Path>) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(repo);
    for name in scrub {
        cmd.env_remove(name);
    }
    if let Some(index) = index {
        cmd.env("GIT_INDEX_FILE", index);
    }
    cmd
}

fn output(mut cmd: Command) -> Option<Vec<u8>> {
    let out = cmd.stderr(Stdio::null()).output().ok()?;
    out.status.success().then_some(out.stdout)
}

/// The key, or `None` on any git error.
pub fn key(args: &Args) -> Option<String> {
    let mut probe = git(&args.repo, &[], None);
    probe.args(["rev-parse", "--git-dir"]);
    output(probe)?;
    // A git hook exports GIT_DIR and friends; hash this checkout instead.
    let mut vars = git(&args.repo, &[], None);
    vars.args(["rev-parse", "--local-env-vars"]);
    let scrub: Vec<String> = String::from_utf8(output(vars)?)
        .ok()?
        .split_whitespace()
        .map(str::to_string)
        .collect();
    let mut path = git(&args.repo, &scrub, None);
    path.args(["rev-parse", "--git-path", "index"]);
    let real = String::from_utf8(output(path)?).ok()?;
    let real = args.repo.join(real.trim_end_matches('\n'));
    let dir = tempfile::Builder::new()
        .prefix("check-index.")
        .tempdir()
        .ok()?;
    let tmp = dir.path().join("index");
    // Like `cp -p`: keep the mtime so racy git re-reads same-second edits.
    // A failed or partial copy is removed, like the script's `cp || rm -f`.
    if std::fs::copy(&real, &tmp).is_err() {
        let _ = std::fs::remove_file(&tmp);
    } else {
        let kept = std::fs::metadata(&real)
            .and_then(|m| m.modified())
            .and_then(|t| {
                std::fs::File::options()
                    .write(true)
                    .open(&tmp)?
                    .set_modified(t)
            });
        if kept.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
    }
    let mut changed = git(&args.repo, &scrub, Some(&tmp));
    changed.args([
        "ls-files",
        "-z",
        "--modified",
        "--others",
        "--exclude-standard",
    ]);
    let list = output(changed)?;
    let mut update = git(&args.repo, &scrub, Some(&tmp));
    update
        .args([
            "update-index",
            "-z",
            "--add",
            "--remove",
            "--info-only",
            "--stdin",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = update.spawn().ok()?;
    // Write, then drop the pipe (end of input) before waiting on the child.
    let wrote = child.stdin.take()?.write_all(&list);
    let status = child.wait().ok()?;
    if wrote.is_err() || !status.success() {
        return None;
    }
    let mut stage = git(&args.repo, &scrub, Some(&tmp));
    stage.args(["ls-files", "-s"]);
    let mut text = Vec::new();
    if !args.pathspecs.is_empty() {
        stage.arg("--").args(&args.pathspecs);
        for spec in &args.pathspecs {
            text.extend_from_slice(spec.as_encoded_bytes());
            text.push(b'\n');
        }
    }
    text.extend(output(stage)?);
    Some(sha1_hex(&text)[..16].to_string())
}

/// SHA-1 (FIPS 180-4), to match `shasum`'s default; no extra crate needed.
pub fn sha1_hex(data: &[u8]) -> String {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for block in msg.chunks(64) {
        let mut w = [0u32; 80];
        for (i, word) in block.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes(word.try_into().unwrap());
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::sha1_hex;

    #[test]
    fn sha1_matches_published_vectors() {
        assert_eq!(sha1_hex(b""), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(sha1_hex(b"abc"), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let long = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq";
        assert_eq!(sha1_hex(long), "84983e441c3bd26ebaae4aa1f95129e5e54670f1");
    }
}
