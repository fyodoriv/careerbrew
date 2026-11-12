#[path = "../src/observer_probe/parser.rs"]
mod parser;
use std::collections::BTreeMap;
fn hex(s: &str) -> String {
    format!(
        "\"{}\"",
        s.bytes().map(|b| format!("\\x{b:02x}")).collect::<String>()
    )
}
fn valid() -> BTreeMap<u32, String> {
    let parent = format!("execve({}, [{}, {}], 0x0 /* 0 vars */) = 0\nclone(child_stack=NULL, flags=CLONE_VM|CLONE_VFORK|SIGCHLD) = 22\nwait4(22, [{{WIFEXITED(s) && WEXITSTATUS(s) == 0}}], 0, NULL) = 22\nexit_group(0) = ?\n+++ exited with 0 +++\n", hex("/usr/local/bin/python"), hex("/usr/local/bin/python"), hex("/probe/parent.py"));
    let child = format!("execve({}, [{}, {}], 0x0 /* 0 vars */) = 0\nopenat(AT_FDCWD, {}, O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC, 0600) = 3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74>\nwrite(3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74>, {}, 24) = 24\nclose(3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74>) = 0\nunlinkat(AT_FDCWD, {}, 0) = 0\nsocket(AF_INET, SOCK_STREAM|SOCK_CLOEXEC, IPPROTO_IP) = 3<TCP:[123]>\nconnect(3<TCP:[123]>, {{sa_family=AF_INET, sin_port=htons(9), sin_addr=inet_addr(\"\\x31\\x32\\x37\\x2e\\x30\\x2e\\x30\\x2e\\x31\")}}, 16) = -1 ECONNREFUSED (Connection refused)\nexit_group(0) = ?\n+++ exited with 0 +++\n", hex("/usr/local/bin/python"), hex("/usr/local/bin/python"), hex("/probe/child.py"), hex("/work/transient"), hex("CAREERBREW-TRANSIENT-V1\n"), hex("/work/transient"));
    [(11, parent), (22, child)].into()
}
fn reject_mutations(pid: u32, pairs: &[(&str, &str)]) {
    for (a, b) in pairs {
        let mut t = valid();
        assert!(t[&pid].contains(a));
        *t.get_mut(&pid).unwrap() = t[&pid].replace(a, b);
        assert!(parser::verify(&t).is_err(), "accepted mutation {a} -> {b}");
    }
}
#[test]
fn p01_complete() {
    assert_eq!(parser::verify(&valid()).unwrap().events, 5);
    let padded = valid()
        .into_iter()
        .map(|(pid, text)| {
            (
                pid,
                text.replace(
                    "exit_group(0) = ?",
                    "exit_group(0)                           = ?",
                ),
            )
        })
        .collect();
    assert_eq!(parser::verify(&padded).unwrap().events, 5);
}
#[test]
fn p02_relation() {
    reject_mutations(11, &[(" = 22\nwait4", " = 23\nwait4")]);
    let mut zero = valid();
    let parent = zero.remove(&11).unwrap();
    zero.insert(0, parent);
    assert!(parser::verify(&zero).is_err());
}
#[test]
fn p03_exec() {
    reject_mutations(
        22,
        &[
            ("vars */) = 0", "vars */) = -1 ENOENT (No such file)"),
            ("execve(", "access("),
        ],
    );
}
#[test]
fn p04_write_pid() {
    let mut t = valid();
    let write = t[&22]
        .lines()
        .find(|l| l.starts_with("write("))
        .unwrap()
        .to_owned();
    *t.get_mut(&22).unwrap() = t[&22].replace(&(write.clone() + "\n"), "");
    t.get_mut(&11).unwrap().insert_str(0, &(write + "\n"));
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p05_descriptor() {
    reject_mutations(22, &[("write(3<", "write(4<")]);
}
#[test]
fn p06_bytes_and_short() {
    reject_mutations(
        22,
        &[("\\x43\\x41", "\\x58\\x41"), ("24) = 24", "24) = 23")],
    );
}
#[test]
fn p07_unlink() {
    reject_mutations(
        22,
        &[
            ("unlinkat(", "unlink("),
            ("unlinkat(AT_FDCWD,", "unlinkat(99,"),
            (
                "unlinkat(AT_FDCWD, \"\\x2f\\x77",
                "unlinkat(AT_FDCWD, \"\\x2f\\x78",
            ),
        ],
    );
    let mut t = valid();
    let row = t[&22]
        .lines()
        .find(|l| l.starts_with("unlinkat("))
        .unwrap()
        .to_owned();
    *t.get_mut(&22).unwrap() = t[&22].replace(&(row.clone() + "\n"), "");
    t.get_mut(&11).unwrap().insert_str(0, &(row + "\n"));
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p08_connect_types() {
    reject_mutations(
        22,
        &[
            (
                "\\x31\\x32\\x37\\x2e\\x30\\x2e\\x30\\x2e\\x31",
                "\\x31\\x32\\x37\\x2e\\x30\\x2e\\x30\\x2e\\x32",
            ),
            ("htons(9)", "htons(10)"),
            ("-1 ECONNREFUSED (Connection refused)", "0"),
        ],
    );
}
#[test]
fn p09_exit() {
    reject_mutations(22, &[("+++ exited with 0 +++", "+++ exited with 1 +++")]);
}
#[test]
fn p10_wait() {
    reject_mutations(11, &[("wait4(", "notwait(")]);
}
#[test]
fn p11_argv_decoy() {
    let mut t = valid();
    t.remove(&22);
    t.get_mut(&11).unwrap().insert_str(
        0,
        "execve(\"write(3) unlink() connect()\", [], NULL) = -1 ENOENT (missing)\n",
    );
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p12_stream_decoy() {
    let mut t = valid();
    t.remove(&22);
    t.get_mut(&11).unwrap().insert_str(0,"write(1, \"execve() write() unlink() connect()\", 37) = 37\nwrite(2, \"execve() write() unlink() connect()\", 37) = 37\n");
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p13_unfinished() {
    reject_mutations(
        22,
        &[
            ("close(3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74>) = 0", "<... close resumed>) = 0"),
            (
                "close(3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74>) = 0",
                "close(3<\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x74\\x72\\x61\\x6e\\x73\\x69\\x65\\x6e\\x74> <unfinished ...>",
            ),
        ],
    );
}
#[test]
fn p14_truncation_terminal() {
    reject_mutations(
        22,
        &[("+++ exited with 0 +++\n", ""), ("24) = 24", "24) = ...")],
    );
}

fn append_before_exit(trace: &mut BTreeMap<u32, String>, pid: u32, row: &str) {
    let text = trace.get_mut(&pid).unwrap();
    *text = text.replace("exit_group(0)", &format!("{row}\nexit_group(0)"));
}
#[test]
fn p07_extra_unlink_keeps_all_required_events() {
    for pid in [11, 22] {
        let mut t = valid();
        append_before_exit(
            &mut t,
            pid,
            &format!("unlinkat(AT_FDCWD, {}, 0) = 0", hex("/work/unexpected")),
        );
        assert!(
            parser::verify(&t).is_err(),
            "accepted extra unlink by PID {pid}"
        );
    }
}
#[test]
fn p07_unlisted_mutators_parent_and_child() {
    let path = hex("/work/unexpected");
    for pid in [11, 22] {
        for row in [
            format!("mkdirat(AT_FDCWD, {path}, 0700) = 0"),
            format!("truncate({path}, 0) = 0"),
            format!("renameat(AT_FDCWD, {path}, AT_FDCWD, {path}) = 0"),
            format!("linkat(AT_FDCWD, {path}, AT_FDCWD, {path}, 0) = 0"),
        ] {
            let mut t = valid();
            append_before_exit(&mut t, pid, &row);
            assert!(
                parser::verify(&t).is_err(),
                "accepted extra mutator by PID {pid}: {row}"
            );
        }
    }
}
#[test]
fn p10_wait_before_child_creation() {
    let mut t = valid();
    let text = t.get_mut(&11).unwrap();
    let wait = text
        .lines()
        .find(|line| line.starts_with("wait4("))
        .unwrap()
        .to_owned()
        + "\n";
    *text = text.replace(&wait, "");
    let at = text.find("clone(").unwrap();
    text.insert_str(at, &wait);
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p10_parent_exec_after_child_creation() {
    let mut t = valid();
    let text = t.get_mut(&11).unwrap();
    let exec = text
        .lines()
        .find(|line| line.starts_with("execve("))
        .unwrap()
        .to_owned()
        + "\n";
    *text = text.replace(&exec, "");
    let at = text.find("wait4(").unwrap();
    text.insert_str(at, &exec);
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p05_closed_marker_descriptor_before_write() {
    let mut t = valid();
    let text = t.get_mut(&22).unwrap();
    let close = text
        .lines()
        .find(|line| line.starts_with("close("))
        .unwrap()
        .to_owned();
    *text = text.replace("write(", &format!("{close}\nwrite("));
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p05_marker_descriptor_reused_before_write() {
    let mut t = valid();
    let text = t.get_mut(&22).unwrap();
    let overwrite = format!(
        "openat(AT_FDCWD, {}, O_RDONLY|O_CLOEXEC) = 3<{}>",
        hex("/work/other"),
        hex("/work/other").trim_matches('"')
    );
    *text = text.replace("write(", &format!("{overwrite}\nwrite("));
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p08_closed_socket_descriptor_before_connect() {
    let mut t = valid();
    let text = t.get_mut(&22).unwrap();
    *text = text.replace("connect(", "close(3<TCP:[123]>) = 0\nconnect(");
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p08_failed_socket_constructor_cannot_grant_live_fd() {
    let mut t = valid();
    let text = t.get_mut(&22).unwrap();
    *text = text
        .replace(
            "IPPROTO_IP) = 3<TCP:[123]>",
            "IPPROTO_IP) = -1 EMFILE (Too many open files)",
        )
        .replace("connect(3<TCP:", "connect(-1<TCP:");
    assert!(parser::verify(&t).is_err());
}
#[test]
fn p08_extra_failed_network_attempt_keeps_required_event() {
    for pid in [11, 22] {
        let mut t = valid();
        append_before_exit(&mut t,pid,&format!("connect(3<TCP:[123]>, {{sa_family=AF_INET, sin_port=htons(10), sin_addr=inet_addr({})}}, 16) = -1 ECONNREFUSED (Connection refused)",hex("127.0.0.2")));
        assert!(parser::verify(&t).is_err());
    }
}

#[test]
fn p05_descriptor_transition_variants() {
    let other = hex("/work/other");
    let other_fd = other.trim_matches('"');
    let rows = [
        "close_range(3, 3, 0) = 0".to_owned(),
        format!("dup(4<{other_fd}>) = 3<{other_fd}>"),
        format!("dup2(4<{other_fd}>, 3) = 3<{other_fd}>"),
        format!("dup3(4<{other_fd}>, 3, O_CLOEXEC) = 3<{other_fd}>"),
        format!("fcntl(4<{other_fd}>, F_DUPFD_CLOEXEC, 3) = 3<{other_fd}>"),
    ];
    for call in rows {
        for target in ["write(", "connect("] {
            let mut t = valid();
            let text = t.get_mut(&22).unwrap();
            let open = format!("openat(AT_FDCWD, {other}, O_RDONLY|O_CLOEXEC) = 4<{other_fd}>");
            *text = text.replace(target, &format!("{open}\n{call}\n{target}"));
            assert!(
                parser::verify(&t).is_err(),
                "accepted transition before {target}: {call}"
            );
        }
    }
}
#[test]
fn p05_malformed_duplication_cannot_preserve_live_binding() {
    let path = hex("/work/transient");
    let path = path.trim_matches('"');
    for row in [
        format!("dup2(3<{path}>, 4) = 5<{path}>"),
        format!("fcntl(3<{path}>, F_GETFD, 0) = FD_CLOEXEC"),
        format!("fcntl(3<{path}>, F_DUPFD_CLOEXEC) = 4<{path}>"),
        "close(99, 0) = 0".to_owned(),
        format!("dup2(3<{path}>, 4, 0) = 4<{path}>"),
        format!("dup3(3<{path}>, 4) = 4<{path}>"),
        format!("dup3(3<{path}>, 4, O_NONBLOCK) = 4<{path}>"),
    ] {
        let mut t = valid();
        let text = t.get_mut(&22).unwrap();
        *text = text.replace("write(", &format!("{row}\nwrite("));
        assert!(
            parser::verify(&t).is_err(),
            "accepted malformed descriptor operation: {row}"
        );
    }
}
#[test]
fn p07_extra_file_open_and_write() {
    for pid in [11, 22] {
        for row in [
            format!(
                "openat(AT_FDCWD, {}, O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC, 0600) = 99<{}>",
                hex("/work/unexpected"),
                hex("/work/unexpected").trim_matches('"')
            ),
            format!(
                "write(99<{}>, {}, 1) = 1",
                hex("/work/unexpected").trim_matches('"'),
                hex("X")
            ),
        ] {
            let mut t = valid();
            append_before_exit(&mut t, pid, &row);
            assert!(parser::verify(&t).is_err());
        }
    }
}
#[test]
fn p01_declared_hex_stream_writes_are_not_child_effects() {
    let mut t = valid();
    let message = "execve() write() unlinkat() connect()\n";
    for (descriptor, path) in [(1, "/work/stdout"), (2, "/work/stderr")] {
        append_before_exit(
            &mut t,
            11,
            &format!(
                "write({descriptor}<{}>, {}, {}) = {}",
                hex(path).trim_matches('"'),
                hex(message),
                message.len(),
                message.len()
            ),
        );
    }
    assert_eq!(parser::verify(&t).unwrap().events, 5);
}

const REPLAY_CWD: &str = r"AT_FDCWD<\x2f\x77\x6f\x72\x6b>";
fn actual_v7_pair() -> BTreeMap<u32, String> {
    [
        (
            10,
            include_str!("fixtures/linux-observer-replay-v7/trace.10").to_owned(),
        ),
        (
            11,
            include_str!("fixtures/linux-observer-replay-v7/trace.11").to_owned(),
        ),
    ]
    .into()
}
#[test]
fn replay_actual_v7_pair() {
    use sha2::{Digest, Sha256};
    let metadata: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/linux-observer-replay-v7/manifest.json"
    ))
    .unwrap();
    let files: [(&str, &[u8], usize, &str); 3] = [
        (
            "trace.10",
            include_bytes!("fixtures/linux-observer-replay-v7/trace.10"),
            167570,
            "e1e48db2e65ebef62e292a3786203aa510457a5031f2aeb229c7a84c89084996",
        ),
        (
            "trace.11",
            include_bytes!("fixtures/linux-observer-replay-v7/trace.11"),
            113415,
            "5ac8d3455b53ecdb253e786a94469d2d5131d52c68fb0c0a9680ae4bf0233ed6",
        ),
        (
            "status.json",
            include_bytes!("fixtures/linux-observer-replay-v7/status.json"),
            436,
            "c382f032b3584530f8d69b656593af85425ea5578e28cbf9c0462d59e2156e1f",
        ),
    ];
    for (name, bytes, size, sha) in files {
        assert_eq!(bytes.len(), size);
        assert_eq!(format!("{:x}", Sha256::digest(bytes)), sha);
        assert_eq!(metadata["files"][name]["sha256"], sha);
        assert_eq!(metadata["files"][name]["bytes"], size);
    }
    assert_eq!(metadata["image_working_directory"], "/work");
    assert_eq!(metadata["normalization"], "none; full original bytes");
    assert_eq!(
        metadata["image"],
        "sha256:49a40cea1045b08d757533d31dc9182bade988fdacc60b8799cc25d90da1b1c0"
    );
    assert_eq!(
        metadata["source_tree"],
        "7c96ec74d08db7546a4123e5aaf7dbd5dd93289b"
    );
    assert_eq!(
        metadata["executed_binary_sha256"],
        "686e4f5da82643305234bb96222c2de4d8ef0e26d40a2adb59556c686b17e710"
    );
    let status: serde_json::Value = serde_json::from_str(include_str!(
        "fixtures/linux-observer-replay-v7/status.json"
    ))
    .unwrap();
    assert_eq!(status["actual_tracer_wait"], 0);
    assert_eq!(status["complete"], true);
    assert_eq!(status["bounds_ok"], true);
    assert_eq!(parser::verify(&actual_v7_pair()).unwrap().events, 5);
}
#[test]
fn replay_exact_cwd_forms() {
    assert_eq!(parser::verify(&valid()).unwrap().events, 5);
    for operation in ["openat", "unlinkat"] {
        let old = format!("{operation}(AT_FDCWD,");
        let mut decorated = valid();
        let text = decorated.get_mut(&22).unwrap();
        assert!(text.contains(&old));
        *text = text.replacen(&old, &format!("{operation}({REPLAY_CWD},"), 1);
        assert_eq!(parser::verify(&decorated).unwrap().events, 5);
        for bad in [
            r"AT_FDCWD<\x2f\x77\x72\x6f\x6e\x67>",
            r"AT_FDCWD<\x2f\x77\x6f\x72\x6b",
            r"AT_FDCWD<\x2f\x77\x6f\x72\x6>",
            r"AT_FDCWD<\x2f\x77\x6f\x72\x6b>>",
            r"AT_FDCWD<\x2f\x77\x6f\x72\x6b>trailing",
            r"AT_FDCWD<\x2f\x77\x6f\x72\x6b><other>",
            "AT_FDCWD<>",
            "AT_FDCWD</work>",
            "AT_FDCWD< /work>",
            "-100</work>",
        ] {
            let mut corrupted = valid();
            let text = corrupted.get_mut(&22).unwrap();
            *text = text.replacen(&old, &format!("{operation}({bad},"), 1);
            assert!(
                parser::verify(&corrupted).is_err(),
                "accepted {operation} cwd {bad}"
            );
        }
    }
    let mut both = valid();
    *both.get_mut(&22).unwrap() = both[&22].replace("AT_FDCWD,", &format!("{REPLAY_CWD},"));
    assert_eq!(parser::verify(&both).unwrap().events, 5);
}
#[test]
fn replay_actual_semantic_refusals() {
    let original = actual_v7_pair();
    assert_eq!(parser::verify(&original).unwrap().events, 5);
    for (pid, old, new) in [
        (
            11,
            hex("CAREERBREW-TRANSIENT-V1\n"),
            hex("CAREERBREW-WRONG-V1\n"),
        ),
        (
            11,
            format!("write(3<{}>", hex("/work/transient").trim_matches('"')),
            format!("write(4<{}>", hex("/work/transient").trim_matches('"')),
        ),
        (11, "sin_port=htons(9)".into(), "sin_port=htons(8)".into()),
        (
            10,
            "WEXITSTATUS(s) == 0".into(),
            "WEXITSTATUS(s) == 1".into(),
        ),
    ] {
        let mut corrupted = original.clone();
        let text = corrupted.get_mut(&pid).unwrap();
        assert!(text.contains(&old), "mutation source absent: {old}");
        *text = text.replacen(&old, &new, 1);
        assert!(
            parser::verify(&corrupted).is_err(),
            "accepted actual semantic mutation {old}"
        );
    }
    let mut extra = original.clone();
    let text = extra.get_mut(&11).unwrap();
    *text = text.replacen(
        "exit_group(0)",
        &format!(
            "unlinkat({REPLAY_CWD}, {}, 0) = 0\nexit_group(0)",
            hex("/work/unexpected")
        ),
        1,
    );
    assert_ne!(extra, original);
    assert!(
        parser::verify(&extra).is_err(),
        "accepted extra mutation in actual pair"
    );
}
