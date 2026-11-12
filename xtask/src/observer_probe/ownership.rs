use serde_json::Value;
use std::{path::Path, process::Command};
pub const TMPFS: &str = "rw,noexec,nosuid,nodev,size=67108864,mode=0700,uid=65534,gid=65534";
pub struct Owner {
    pub daemon: String,
    pub name: String,
    pub label: String,
    pub id: String,
    pub image: String,
}
fn full_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
impl Owner {
    pub fn validate(&self, daemon: &str, m: &Value) -> Result<(), String> {
        if self.daemon != daemon
            || !full_id(&self.id)
            || m["Id"] != self.id
            || m["Name"] != format!("/{}", self.name)
            || m["Config"]["Labels"]["careerbrew.observer"] != self.label
            || m["Image"] != self.image
            || m["Config"]["Image"] != self.image
            || m["Platform"] != "linux"
            || m["Config"]["User"] != "65534:65534"
        {
            return Err("container identity mismatch".into());
        }
        let h = &m["HostConfig"];
        if h["NetworkMode"] != "none"
            || h["PidMode"] != ""
            || h["IpcMode"] != "private"
            || h["Privileged"] != false
            || h["ReadonlyRootfs"] != true
            || h["SecurityOpt"] != serde_json::json!(["no-new-privileges"])
            || h["CapDrop"] != serde_json::json!(["ALL"])
            || !(h["CapAdd"].is_null() || h["CapAdd"] == serde_json::json!([]))
            || h["PidsLimit"] != 32
            || h["Memory"] != 268435456u64
            || h["MemorySwap"] != 268435456u64
            || h["Tmpfs"] != serde_json::json!({"/work":TMPFS})
            || m["Mounts"] != serde_json::json!([])
        {
            return Err("container isolation or bounds mismatch".into());
        }
        Ok(())
    }
    pub fn recover(&mut self, daemon: &str, m: &Value) -> Result<(), String> {
        if !self.id.is_empty() {
            return Err("recovery already has ID".into());
        }
        let id = m["Id"].as_str().ok_or("missing recovery ID")?.to_owned();
        self.id = id;
        if let Err(error) = self.validate(daemon, m) {
            self.id.clear();
            return Err(error);
        }
        Ok(())
    }
}
pub fn command(cli: &Path, owned: &Path, endpoint: &str) -> Command {
    let mut c = Command::new(cli);
    c.env_clear()
        .arg("--host")
        .arg(endpoint)
        .env("PATH", "/usr/bin:/bin")
        .env("HOME", owned.join("home"))
        .env("DOCKER_CONFIG", owned.join("config"))
        .env("TMPDIR", owned.join("tmp"))
        .env("LC_ALL", "C")
        .env("LANG", "C");
    c
}
pub fn client_status(code: Option<i32>) -> Result<(), String> {
    if code == Some(0) {
        Ok(())
    } else {
        Err(format!("Docker client failed with {code:?}"))
    }
}
pub fn absence(target: &str, code: Option<i32>, stdout: &str, stderr: &str) -> Result<(), String> {
    let valid_target = !target.is_empty()
        && target
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
    let empty_array = serde_json::from_str::<Value>(stdout)
        .is_ok_and(|value| value.as_array().is_some_and(Vec::is_empty));
    if valid_target
        && code == Some(1)
        && empty_array
        && stderr == format!("Error response from daemon: No such container: {target}\n")
    {
        Ok(())
    } else {
        Err("exact resource absence unverified".into())
    }
}
pub fn sizes(stdout: u64, stderr: u64, trace: u64) -> Result<(), String> {
    if stdout > 1048576 || stderr > 1048576 || trace > 8388608 {
        Err("probe output bound exceeded".into())
    } else {
        Ok(())
    }
}

pub fn completion(status: &Value, expected_argv: &[String]) -> Result<(), String> {
    if status["complete"] != true
        || status["bounds_ok"] != true
        || status["actual_tracer_wait"] != 0
        || status["transient_absent"] != true
        || status["observer_argv"] != serde_json::json!(expected_argv)
        || !status["tracer_pid"]
            .as_u64()
            .is_some_and(|pid| pid > 0 && pid <= u64::from(u32::MAX))
    {
        return Err("actual tracer identity, argv, completion or bounds failed".into());
    }
    Ok(())
}
