#[allow(dead_code)]
mod capture_actions {
    include!("../src/capture_actions.rs");
}
#[path = "../src/observer_probe/ownership.rs"]
mod ownership;
use serde_json::{json, Value};
fn owner() -> ownership::Owner {
    ownership::Owner {
        daemon: "daemon-v1".into(),
        name: "cb-observer-123".into(),
        label: "invocation123".into(),
        id: "a".repeat(64),
        image: format!("sha256:{}", "b".repeat(64)),
    }
}
fn metadata() -> Value {
    json!({"Id":"a".repeat(64),"Name":"/cb-observer-123","Image":format!("sha256:{}","b".repeat(64)),"Platform":"linux","Config":{"Image":format!("sha256:{}","b".repeat(64)),"User":"65534:65534","Labels":{"careerbrew.observer":"invocation123"}},"HostConfig":{"NetworkMode":"none","PidMode":"","IpcMode":"private","Privileged":false,"ReadonlyRootfs":true,"SecurityOpt":["no-new-privileges"],"CapDrop":["ALL"],"CapAdd":null,"Memory":268435456,"MemorySwap":268435456,"PidsLimit":32,"Tmpfs":{"/work":"rw,noexec,nosuid,nodev,size=67108864,mode=0700,uid=65534,gid=65534"}},"Mounts":[]})
}
#[test]
fn o01_bound() {
    owner().validate("daemon-v1", &metadata()).unwrap();
    let expected = vec!["fixed-observer".to_owned(), "fixed-fixture".to_owned()];
    let status = json!({"complete":true,"bounds_ok":true,"actual_tracer_wait":0,"transient_absent":true,"observer_argv":expected,"tracer_pid":17});
    ownership::completion(&status, &expected).unwrap();
    for (field, value) in [
        ("tracer_pid", json!(0)),
        ("complete", json!(false)),
        ("bounds_ok", json!(false)),
        ("actual_tracer_wait", json!(1)),
        ("transient_absent", json!(false)),
        ("observer_argv", json!(["different"])),
    ] {
        let mut changed = status.clone();
        changed[field] = value;
        assert!(ownership::completion(&changed, &expected).is_err());
    }
}
#[test]
fn o02_foreign() {
    let mut m = metadata();
    m["Config"]["Labels"] = json!({});
    assert!(owner().validate("daemon-v1", &m).is_err());
}
#[test]
fn o03_name() {
    let mut m = metadata();
    m["Name"] = json!("/other");
    assert!(owner().validate("daemon-v1", &m).is_err());
}
#[test]
fn o04_label() {
    let mut m = metadata();
    m["Config"]["Labels"]["careerbrew.observer"] = json!("other");
    assert!(owner().validate("daemon-v1", &m).is_err());
}
#[test]
fn o05_id() {
    let mut m = metadata();
    m["Id"] = json!("c".repeat(64));
    assert!(owner().validate("daemon-v1", &m).is_err());
}
#[test]
fn o06_daemon() {
    assert!(owner().validate("daemon-v2", &metadata()).is_err());
}
#[test]
fn o07_environment() {
    let command = ownership::command(
        std::path::Path::new("/docker"),
        std::path::Path::new("/owned"),
        "unix:///synthetic/docker.sock",
    );
    assert_eq!(command.get_program(), "/docker");
    let env = command
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_str().unwrap().to_owned(),
                v.unwrap().to_str().unwrap().to_owned(),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(env.len(), 6);
    assert!(env.keys().all(
        |k| ["PATH", "HOME", "DOCKER_CONFIG", "TMPDIR", "LC_ALL", "LANG"].contains(&k.as_str())
    ));
    let args = command.get_args().collect::<Vec<_>>();
    assert_eq!(args, ["--host", "unix:///synthetic/docker.sock"]);

    struct Restore(Vec<(String, Option<std::ffi::OsString>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }
    let keys = [
        "DOCKER_HOST",
        "DOCKER_CONTEXT",
        "DOCKER_TLS_VERIFY",
        "DOCKER_CERT_PATH",
        "HTTPS_PROXY",
        "HTTP_PROXY",
    ];
    let _restore = Restore(
        keys.iter()
            .map(|key| (key.to_string(), std::env::var_os(key)))
            .collect(),
    );
    for key in keys {
        std::env::set_var(key, "synthetic-redirect");
    }
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("environment-check");
    std::fs::write(&script,"#!/bin/sh\nprintf '%s\\n' \"${DOCKER_HOST+set}\" \"${DOCKER_CONTEXT+set}\" \"${DOCKER_TLS_VERIFY+set}\" \"${DOCKER_CERT_PATH+set}\" \"${HTTPS_PROXY+set}\" \"${HTTP_PROXY+set}\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let command = ownership::command(&script, dir.path(), "unix:///synthetic/docker.sock");
    let (_, exit, out, err) =
        capture_actions::execute_bounded(command, std::time::Duration::from_secs(2)).unwrap();
    assert_eq!(exit, Some(0));
    assert!(err.is_empty());
    assert_eq!(out.lines().count(), 6);
    assert!(out.lines().all(str::is_empty));
}
#[test]
fn o08_recovery_decision_only() {
    let mut own = owner();
    own.id.clear();
    own.recover("daemon-v1", &metadata()).unwrap();
    assert_eq!(own.id, "a".repeat(64));
    let mut wrong = metadata();
    wrong["Config"]["Labels"] = json!({});
    own.id.clear();
    assert!(own.recover("daemon-v1", &wrong).is_err());
}
#[test]
fn o09_client_death_decision_only() {
    assert!(ownership::client_status(Some(0)).is_ok());
    for code in [None, Some(137), Some(1)] {
        assert!(ownership::client_status(code).is_err());
    }
    owner().validate("daemon-v1", &metadata()).unwrap();
}
#[test]
fn o10_image_platform() {
    for (field, val) in [
        ("Image", json!("sha256:wrong")),
        ("Platform", json!("darwin")),
    ] {
        let mut m = metadata();
        m[field] = val;
        assert!(owner().validate("daemon-v1", &m).is_err());
    }
}
#[test]
fn o11_absence() {
    for target in [
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "synthetic-owned-name",
    ] {
        let error = format!("Error response from daemon: No such container: {target}\n");
        assert!(ownership::absence(target, Some(1), "[]\n", &error).is_ok());
        for code in [None, Some(0), Some(2), Some(137)] {
            assert!(ownership::absence(target, code, "[]\n", &error).is_err());
        }
        for output in [
            "",
            "[{}]\n",
            "{}\n",
            "null",
            "[",
            "[] {}",
            "[] trailing",
            "[\"object\"]",
        ] {
            assert!(ownership::absence(target, Some(1), output, &error).is_err());
        }
        for text in [
            format!("Error response from daemon: No such container: wrong-{target}\n"),
            format!("Error response from daemon: No such container: {target}"),
            format!(" {error}"),
            format!("{error}trailing"),
            format!("{error}{error}"),
            format!("Error: No such object: {target}\n"),
            "Cannot connect to the Docker daemon".into(),
            "Error response from daemon: permission denied\n".into(),
        ] {
            assert!(ownership::absence(target, Some(1), "[]\n", &text).is_err());
        }
    }
    for invalid in ["", "bad\nname", "../name", " name"] {
        let error = format!("Error response from daemon: No such container: {invalid}\n");
        assert!(ownership::absence(invalid, Some(1), "[]\n", &error).is_err());
    }
}
#[test]
fn o12_limits() {
    for (field, value) in [
        ("PidsLimit", json!(33)),
        ("PidsLimit", Value::Null),
        ("Memory", json!(268435457)),
        ("Memory", Value::Null),
        ("ReadonlyRootfs", json!(false)),
        ("NetworkMode", json!("host")),
        ("PidMode", json!("host")),
        ("Privileged", json!(true)),
        ("SecurityOpt", json!([])),
        ("CapDrop", json!([])),
        ("MemorySwap", Value::Null),
        ("Tmpfs", json!({})),
        ("CapAdd", json!(["SYS_ADMIN"])),
    ] {
        let mut m = metadata();
        m["HostConfig"][field] = value;
        assert!(
            owner().validate("daemon-v1", &m).is_err(),
            "accepted {field}"
        );
    }
    for sizes in [(1048577, 0, 0), (0, 1048577, 0), (0, 0, 8388609)] {
        assert!(ownership::sizes(sizes.0, sizes.1, sizes.2).is_err());
    }
    assert!(ownership::sizes(1048576, 1048576, 8388608).is_ok());
}
