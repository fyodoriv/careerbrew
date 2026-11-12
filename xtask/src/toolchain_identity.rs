//! Compare the installed compiler and Clippy driver without activating a toolchain.

use std::fs::{self, File};
use std::io::{self, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const DRIVER_LIMIT: u64 = 64 * 1024 * 1024;
const METADATA_LIMIT: u64 = 64 * 1024;
const MACH_O: [u8; 4] = [0xcf, 0xfa, 0xed, 0xfe];
const LINKAGE_DENIED: &str = "compiler library linkage is unsupported";

pub fn run(compiler: &Path) -> Result<String, &'static str> {
    if !cfg!(target_os = "macos") {
        return Err("toolchain identity is unsupported on this platform");
    }
    if !compiler.is_absolute() {
        return Err("provide the absolute path to an installed rustc");
    }
    let compiler = compiler
        .canonicalize()
        .map_err(|_| "installed rustc is unavailable")?;
    let install = compiler
        .parent()
        .and_then(Path::parent)
        .filter(|root| compiler == root.join("bin/rustc"))
        .ok_or("provide a direct installed rustc binary")?;
    let otool = installed_otool()?;
    let compiler_links = inspect(&otool, &compiler)?;
    let library = compiler_library(&compiler_links)?;
    let lib_dir = install.join("lib");
    if lib_dir.canonicalize().ok().as_ref() != Some(&lib_dir)
        || !fs::symlink_metadata(lib_dir.join(library)).is_ok_and(|meta| meta.is_file())
    {
        return Err("linked compiler library is absent from the sysroot");
    }

    let release = include_str!("../../rust-toolchain.toml")
        .lines()
        .find_map(|line| line.trim().strip_prefix("channel = \"")?.strip_suffix('"'))
        .ok_or("rustc release pin is unavailable")?;
    let version = query(&compiler, &["-Vv".as_ref()])?;
    let hash = parse_identity(&version, release)?;
    let root = query(&compiler, &["--print".as_ref(), "sysroot".as_ref()])?;
    let root = root.trim_end_matches(['\r', '\n']);
    let root = absolute_directory(root).ok_or("rustc sysroot is unavailable")?;
    if root.as_path() != install {
        return Err("rustc is outside its canonical sysroot bin directory");
    }

    let driver = driver_path(&root)?;
    verify_driver(&root, hash, DRIVER_LIMIT)?;
    let clippy_links = inspect(&otool, &driver)?;
    verify_linkage(&compiler_links, &clippy_links)?;
    Ok(format!("release={release} commit={hash} library={library}"))
}

fn absolute_directory(value: &str) -> Option<PathBuf> {
    let path = Path::new(value);
    if !path.is_absolute() || value.contains(['\r', '\n']) {
        return None;
    }
    let directory = path.canonicalize().ok()?;
    directory.is_dir().then_some(directory)
}

fn installed_otool() -> Result<PathBuf, &'static str> {
    const UNAVAILABLE: &str = "installed Mach-O inspector is unavailable";
    let selected =
        query(Path::new("/usr/bin/xcode-select"), &["-p".as_ref()]).map_err(|_| UNAVAILABLE)?;
    let developer =
        absolute_directory(selected.trim_end_matches(['\r', '\n'])).ok_or(UNAVAILABLE)?;
    [
        "Toolchains/XcodeDefault.xctoolchain/usr/bin/otool",
        "usr/bin/otool",
    ]
    .into_iter()
    .filter_map(|candidate| developer.join(candidate).canonicalize().ok())
    .find(|path| path.starts_with(&developer) && path.is_file())
    .ok_or(UNAVAILABLE)
}

fn query(program: &Path, args: &[&std::ffi::OsStr]) -> Result<String, &'static str> {
    const UNAVAILABLE: &str = "toolchain metadata query failed";
    let mut child = Command::new(program)
        .args(args)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| UNAVAILABLE)?;
    let mut output = Vec::new();
    let read = child
        .stdout
        .take()
        .expect("stdout is piped")
        .take(METADATA_LIMIT + 1)
        .read_to_end(&mut output);
    if read.is_err() || output.len() as u64 > METADATA_LIMIT {
        let _ = child.kill();
        let _ = child.wait();
        return Err("toolchain metadata could not be read within the limit");
    }
    if !child.wait().map_err(|_| UNAVAILABLE)?.success() {
        return Err(UNAVAILABLE);
    }
    String::from_utf8(output).map_err(|_| UNAVAILABLE)
}

fn inspect(otool: &Path, path: &Path) -> Result<String, &'static str> {
    if !fs::symlink_metadata(path).is_ok_and(|meta| meta.is_file() && meta.len() <= DRIVER_LIMIT) {
        return Err("toolchain executable is not a bounded regular file");
    }
    let mut header = [0; 8];
    File::open(path)
        .and_then(|mut file| file.read_exact(&mut header))
        .map_err(|_| "toolchain executable is unreadable")?;
    let cpu = match std::env::consts::ARCH {
        "aarch64" => [12, 0, 0, 1],
        "x86_64" => [7, 0, 0, 1],
        _ => return Err("toolchain architecture is unsupported"),
    };
    if header[..4] != MACH_O || header[4..] != cpu {
        return Err("toolchain executable is not a supported native Mach-O binary");
    }
    query(otool, &["-l".as_ref(), path.as_os_str()])
}

fn command_value(value: &str) -> Result<&str, &'static str> {
    let (value, offset) = value.rsplit_once(" (offset ").ok_or(LINKAGE_DENIED)?;
    offset
        .strip_suffix(')')
        .and_then(|offset| offset.parse::<u32>().ok())
        .ok_or(LINKAGE_DENIED)?;
    Ok(value)
}

fn compiler_library(report: &str) -> Result<&str, &'static str> {
    let mut library = None;
    let mut has_rpath = false;
    for block in report.split("\nLoad command ") {
        let command = block
            .lines()
            .find_map(|line| line.trim().strip_prefix("cmd "));
        if command == Some("LC_RPATH") {
            let path = block
                .lines()
                .find_map(|line| line.trim().strip_prefix("path "));
            if has_rpath || path.map(command_value).transpose()? != Some("@loader_path/../lib") {
                return Err("compiler library search path is unsupported");
            }
            has_rpath = true;
        }
        for name in block
            .lines()
            .filter_map(|line| line.trim().strip_prefix("name "))
        {
            if !name.contains("librustc_driver") {
                continue;
            }
            let base = command_value(name)?
                .strip_prefix("@rpath/")
                .ok_or(LINKAGE_DENIED)?;
            let hash = base
                .strip_prefix("librustc_driver-")
                .and_then(|name| name.strip_suffix(".dylib"))
                .ok_or(LINKAGE_DENIED)?;
            if command != Some("LC_LOAD_DYLIB")
                || !lower_hex(hash, 16)
                || library.replace(base).is_some()
            {
                return Err(LINKAGE_DENIED);
            }
        }
    }
    if !has_rpath {
        return Err("compiler library search path is unsupported");
    }
    library.ok_or(LINKAGE_DENIED)
}

fn verify_linkage<'a>(compiler: &'a str, clippy: &str) -> Result<&'a str, &'static str> {
    let library = compiler_library(compiler)?;
    if library != compiler_library(clippy)? {
        return Err("Clippy driver links a different compiler library");
    }
    Ok(library)
}

fn lower_hex(value: &str, size: usize) -> bool {
    value.len() == size
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn parse_identity<'a>(report: &'a str, pin: &str) -> Result<&'a str, &'static str> {
    const MALFORMED: &str = "rustc identity is malformed";
    let field = |key| {
        let mut values = report.lines().filter_map(|line| line.strip_prefix(key));
        match (values.next(), values.next()) {
            (Some(value), None) => Ok(value),
            _ => Err(MALFORMED),
        }
    };
    let release = field("release: ")?;
    let hash = field("commit-hash: ")?;
    if !lower_hex(hash, 40) {
        return Err(MALFORMED);
    }
    if release != pin {
        return Err("rustc release does not match the pin");
    }
    Ok(hash)
}

fn driver_path(root: &Path) -> Result<PathBuf, &'static str> {
    const ABSENT: &str = "Clippy driver is absent from the sysroot";
    const INVALID: &str = "Clippy driver is not a regular sysroot file";
    let bin = root.canonicalize().map_err(|_| ABSENT)?.join("bin");
    if bin.canonicalize().map_err(|_| ABSENT)? != bin {
        return Err(INVALID);
    }
    let driver = bin.join("clippy-driver");
    let metadata = fs::symlink_metadata(&driver).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            ABSENT
        } else {
            INVALID
        }
    })?;
    if !metadata.is_file() {
        return Err(INVALID);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o111 == 0 {
            return Err(INVALID);
        }
    }
    Ok(driver)
}

fn verify_driver(root: &Path, hash: &str, limit: u64) -> Result<(), &'static str> {
    const UNREADABLE: &str = "Clippy driver could not be read";
    const OVERSIZED: &str = "Clippy driver exceeds the verification limit";
    let file = File::open(driver_path(root)?).map_err(|_| UNREADABLE)?;
    if file.metadata().map_err(|_| UNREADABLE)?.len() > limit {
        return Err(OVERSIZED);
    }
    let mut reader = BufReader::new(file.take(limit + 1));
    let mut header = [0; 4];
    reader.read_exact(&mut header).map_err(|_| UNREADABLE)?;
    if header != MACH_O {
        return Err("Clippy driver executable format is unsupported");
    }

    let mut token = [0; 40];
    let mut length = 0usize;
    let mut count = 4;
    let mut found = false;
    for byte in reader.bytes() {
        let byte = byte.map_err(|_| UNREADABLE)?;
        count += 1;
        if count > limit {
            return Err(OVERSIZED);
        }
        if byte.is_ascii_hexdigit() {
            if length < token.len() {
                token[length] = byte;
            }
            length += 1;
        } else {
            found |= length == token.len() && token.as_slice() == hash.as_bytes();
            length = 0;
        }
    }
    if found || (length == token.len() && token.as_slice() == hash.as_bytes()) {
        Ok(())
    } else {
        Err("Clippy driver lacks the exact rustc commit hash")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::{env, fs};

    const HASH: &str = "58cf8f9c35b355199727627d07ae34a4e2846d67";
    const RELEASE: &str = "1.99.1";

    struct Sysroot(PathBuf);

    impl Sysroot {
        fn new() -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = env::temp_dir().join(format!(
                "careerbrew-toolchain-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            fs::create_dir(path.join("bin")).unwrap();
            Self(path)
        }

        fn driver(&self) -> PathBuf {
            self.0.join("bin/clippy-driver")
        }

        fn write_driver(&self, content: &[u8]) {
            let mut bytes = b"\xcf\xfa\xed\xfe".to_vec();
            bytes.extend_from_slice(content);
            fs::write(self.driver(), bytes).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                fs::set_permissions(self.driver(), fs::Permissions::from_mode(0o700)).unwrap();
            }
        }
    }

    impl Drop for Sysroot {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn linkage(library: &str, rpath: &str) -> String {
        format!(
            "Load command 1\n      cmd LC_LOAD_DYLIB\n         name @rpath/{library} (offset 24)\nLoad command 2\n      cmd LC_RPATH\n         path {rpath} (offset 12)\n"
        )
    }

    fn version(release: &str, hash: &str) -> String {
        format!("rustc {release}\nrelease: {release}\ncommit-hash: {hash}\n")
    }

    #[test]
    fn accepts_exact_toolchain_identity_across_read_boundary() {
        let root = Sysroot::new();
        let mut content = vec![0; 8180];
        content.extend_from_slice(HASH.as_bytes());
        assert_eq!(parse_identity(&version(RELEASE, HASH), RELEASE), Ok(HASH));
        root.write_driver(&content);
        assert_eq!(verify_driver(&root.0, HASH, 16_384), Ok(()));
        let library = "librustc_driver-e4ae23791dbdbd23.dylib";
        let report = linkage(library, "@loader_path/../lib");
        assert_eq!(verify_linkage(&report, &report), Ok(library));
    }

    #[test]
    fn refuses_release_mismatch() {
        assert_eq!(
            parse_identity(&version("1.98.0", HASH), RELEASE),
            Err("rustc release does not match the pin")
        );
    }

    #[test]
    fn refuses_malformed_identity() {
        for report in [
            version(RELEASE, &HASH[..39]),
            version(RELEASE, &"z".repeat(40)),
            format!("{}commit-hash: {HASH}\n", version(RELEASE, HASH)),
        ] {
            assert_eq!(
                parse_identity(&report, RELEASE),
                Err("rustc identity is malformed")
            );
        }
    }

    #[test]
    fn refuses_missing_driver() {
        let root = Sysroot::new();
        assert_eq!(
            verify_driver(&root.0, HASH, 1024),
            Err("Clippy driver is absent from the sysroot")
        );
    }

    #[test]
    #[cfg(unix)]
    fn refuses_external_symlink_and_nonregular_driver() {
        let root = Sysroot::new();
        let other = Sysroot::new();
        other.write_driver(HASH.as_bytes());
        std::os::unix::fs::symlink(other.driver(), root.driver()).unwrap();
        assert_eq!(
            verify_driver(&root.0, HASH, 1024),
            Err("Clippy driver is not a regular sysroot file")
        );
        fs::remove_file(root.driver()).unwrap();
        fs::create_dir(root.driver()).unwrap();
        assert_eq!(
            verify_driver(&root.0, HASH, 1024),
            Err("Clippy driver is not a regular sysroot file")
        );
        fs::remove_dir(root.driver()).unwrap();
        fs::remove_dir(root.0.join("bin")).unwrap();
        std::os::unix::fs::symlink(other.0.join("bin"), root.0.join("bin")).unwrap();
        assert_eq!(
            verify_driver(&root.0, HASH, 1024),
            Err("Clippy driver is not a regular sysroot file")
        );
    }

    #[test]
    fn refuses_short_mismatched_embedded_or_stripped_hashes() {
        let root = Sysroot::new();
        let rustc = linkage(
            "librustc_driver-e4ae23791dbdbd23.dylib",
            "@loader_path/../lib",
        );
        let wrong = linkage(
            "librustc_driver-0000000000000000.dylib",
            "@loader_path/../lib",
        );
        let outside = linkage("librustc_driver-e4ae23791dbdbd23.dylib", "/outside");
        let misleading = linkage(
            "librustc_driver-e4ae23791dbdbd23.dylib (offset spoof",
            "@loader_path/../lib",
        );
        assert_eq!(verify_linkage(&rustc, &misleading), Err(LINKAGE_DENIED));
        assert_eq!(
            verify_linkage(&rustc, &wrong),
            Err("Clippy driver links a different compiler library")
        );
        assert_eq!(
            verify_linkage(&rustc, &outside),
            Err("compiler library search path is unsupported")
        );
        for value in [
            HASH[..7].to_owned(),
            "b".repeat(40),
            format!("a{HASH}"),
            format!("{HASH}f"),
            String::new(),
        ] {
            root.write_driver(value.as_bytes());
            assert_eq!(
                verify_driver(&root.0, HASH, 1024),
                Err("Clippy driver lacks the exact rustc commit hash")
            );
        }
    }

    #[test]
    fn refuses_over_budget_driver() {
        let root = Sysroot::new();
        root.write_driver(HASH.as_bytes());
        assert_eq!(
            verify_driver(&root.0, HASH, 16),
            Err("Clippy driver exceeds the verification limit")
        );
    }
}
