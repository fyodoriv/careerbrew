//! Fixed Linux/arm64 observer probe. State belongs to one capture invocation.
mod ownership;
mod parser;
mod transport;
use crate::capture_actions;
use ownership::Owner;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
const CLI: &str = "/Applications/Docker.app/Contents/Resources/bin/docker";
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn read(path: &Path) -> Result<Vec<u8>, String> {
    fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))
}
fn outcome(value: &Result<Value, String>) -> Value {
    match value {
        Ok(value) => json!({"ok":value}),
        Err(error) => json!({"error":error}),
    }
}
fn store(path: &Path, value: &Value) -> Result<(), String> {
    fs::write(
        path,
        serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}
struct Docker {
    endpoint: String,
    owned: PathBuf,
    cli: PathBuf,
    cli_sha: String,
    daemon: String,
    calls: Vec<Value>,
    deadline: Option<Instant>,
}
impl Docker {
    fn budget(&self) -> Result<Duration, String> {
        match self.deadline {
            Some(end) => {
                let remaining = end
                    .checked_duration_since(Instant::now())
                    .ok_or("aggregate Docker phase deadline exceeded")?;
                if remaining.is_zero() {
                    Err("aggregate Docker phase deadline exceeded".into())
                } else {
                    Ok(remaining.min(Duration::from_secs(30)))
                }
            }
            None => Ok(Duration::from_secs(30)),
        }
    }
    fn observed(
        &mut self,
        args: &[String],
        limit: Duration,
    ) -> Result<capture_actions::OwnerExecution, capture_actions::OwnerFailure> {
        let before = |message: String| capture_actions::OwnerFailure {
            cause: capture_actions::OwnerFailureCause::Setup,
            observation: None,
            message,
        };
        if digest(&read(&self.cli).map_err(before)?) != self.cli_sha {
            return Err(before("Docker executable changed".into()));
        }
        let budget = self.budget().map_err(before)?.min(limit);
        let mut command = ownership::command(&self.cli, &self.owned, &self.endpoint);
        command.args(args);
        let result = capture_actions::execute_during_observed(command, budget, |_, _| Ok(()));
        self.calls.push(match &result {Ok(run)=>json!({"argv":args,"execution":observation_json(&run.observation),"stdout":run.stdout,"stderr":run.stderr}),Err(failure)=>json!({"argv":args,"owner_cause":format!("{:?}",failure.cause),"execution":failure.observation.as_ref().map(observation_json),"process_owner_error":failure.message})});
        result
    }
    fn raw(&mut self, args: &[String]) -> Result<(Option<i32>, String, String), String> {
        let run = self
            .observed(args, Duration::from_secs(30))
            .map_err(|failure| failure.message)?;
        if run.stdout.len() > 1048576 || run.stderr.len() > 1048576 {
            return Err("Docker command output exceeds 1 MiB".into());
        }
        Ok((run.observation.wait_code, run.stdout, run.stderr))
    }
    fn check_daemon(&mut self) -> Result<(), String> {
        let (status, out, _) = self.raw(&["info".into(), "--format".into(), "{{.ID}}".into()])?;
        ownership::client_status(status)?;
        if out.trim() != self.daemon {
            return Err("Docker daemon identity changed".into());
        }
        Ok(())
    }
    fn call(&mut self, args: &[String]) -> Result<(Option<i32>, String, String), String> {
        self.check_daemon()?;
        self.raw(args)
    }
    fn ok(&mut self, args: &[String]) -> Result<String, String> {
        let (status, out, err) = self.call(args)?;
        ownership::client_status(status).map_err(|e| format!("{e}: {err}"))?;
        Ok(out)
    }
    fn inspect(&mut self, target: &str) -> Result<Value, String> {
        let out = self.ok(&[
            "inspect".into(),
            "--type".into(),
            "container".into(),
            target.into(),
        ])?;
        let value: Value =
            serde_json::from_str(&out).map_err(|_| "invalid container inspect JSON")?;
        value
            .as_array()
            .filter(|a| a.len() == 1)
            .and_then(|a| a.first())
            .cloned()
            .ok_or("inspect result not unique".into())
    }
    fn validate(&mut self, owner: &Owner) -> Result<Value, String> {
        let value = self.inspect(&owner.id)?;
        owner.validate(&self.daemon, &value)?;
        Ok(value)
    }
    fn recover(&mut self, owner: &mut Owner) -> Result<(), String> {
        let value = self.inspect(&owner.name)?;
        owner.recover(&self.daemon, &value)
    }
    fn cleanup(&mut self, owner: &mut Owner) -> Result<Value, String> {
        self.deadline = Some(Instant::now() + Duration::from_secs(60));
        if owner.id.is_empty() {
            self.recover(owner)?;
        }
        let state = self.validate(owner)?;
        let never_started = state["State"]["Status"] == "created";
        let actual_container_wait = if never_started {
            if state["State"]["Running"] != false {
                return Err("created container unexpectedly running".into());
            }
            None
        } else {
            if state["State"]["Running"] == true {
                self.ok(&["stop".into(), "--time".into(), "1".into(), owner.id.clone()])?;
            }
            self.validate(owner)?;
            let waited = self
                .ok(&["wait".into(), owner.id.clone()])?
                .trim()
                .parse::<i32>()
                .map_err(|_| "missing actual cleanup wait")?;
            let stopped = self.validate(owner)?;
            if stopped["State"]["Running"] != false || stopped["State"]["ExitCode"] != waited {
                return Err("cleanup wait differs from stopped container metadata".into());
            }
            Some(waited)
        };
        self.validate(owner)?;
        self.ok(&["rm".into(), owner.id.clone()])?;
        for target in [&owner.id, &owner.name] {
            let (status, output, error) = self.call(&[
                "inspect".into(),
                "--type".into(),
                "container".into(),
                target.clone(),
            ])?;
            ownership::absence(target, status, &output, &error)?;
        }
        self.check_daemon()?;
        self.deadline = None;
        Ok(
            json!({"same_daemon":self.daemon,"id":owner.id,"name":owner.name,"exact_id_absent":true,"exact_name_absent":true,"never_started":never_started,"actual_container_wait":actual_container_wait}),
        )
    }
}
fn owner(docker: &Docker, image: &str, suffix: &str) -> Owner {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let label = format!("{}-{nonce}-{suffix}", std::process::id());
    Owner {
        daemon: docker.daemon.clone(),
        name: format!("cb-observer-{label}"),
        label,
        id: String::new(),
        image: image.into(),
    }
}
fn create_args(own: &Owner, mode: &str) -> Vec<String> {
    [
        "create",
        "--pull=never",
        "--name",
        &own.name,
        "--label",
        &format!("careerbrew.observer={}", own.label),
        "--network",
        "none",
        "--ipc=private",
        "--read-only",
        "--cap-drop",
        "ALL",
        "--security-opt",
        "no-new-privileges",
        "--pids-limit",
        "32",
        "--memory",
        "268435456",
        "--memory-swap",
        "268435456",
        "--tmpfs",
        &format!("/work:{}", ownership::TMPFS),
        "--user",
        "65534:65534",
        "--env",
        "PYTHONDONTWRITEBYTECODE=1",
        "--env",
        "HOME=/work",
        "--env",
        "TMPDIR=/work",
        "--env",
        "LC_ALL=C",
        "--entrypoint",
        "/usr/local/bin/python",
        &own.image,
        "/probe/supervisor.py",
        mode,
    ]
    .iter()
    .map(|s| s.to_string())
    .collect()
}
fn create(docker: &mut Docker, own: &mut Owner, mode: &str) -> Result<(), String> {
    let created = docker.ok(&create_args(own, mode));
    match created {
        Ok(id) => {
            own.id = id.trim().into();
            docker.validate(own)?;
            Ok(())
        }
        Err(error) => {
            docker
                .recover(own)
                .map_err(|recovery| format!("create: {error}; exact recovery: {recovery}"))?;
            Err(error)
        }
    }
}
fn trial(
    docker: &mut Docker,
    own: &mut Owner,
    mode: &str,
    status_budget: Duration,
) -> Result<Value, String> {
    create(docker, own, mode)?;
    docker.validate(own)?;
    docker.ok(&["start".into(), own.id.clone()])?;
    let deadline = Instant::now() + status_budget;
    let status = loop {
        if Instant::now() >= deadline {
            return Err("tracer status deadline exceeded".into());
        }
        let (code, out, error) = docker.call(&[
            "exec".into(),
            own.id.clone(),
            "/usr/local/bin/python".into(),
            "/probe/supervisor.py".into(),
            "--status".into(),
        ])?;
        if code == Some(75) {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        ownership::client_status(code).map_err(|e| format!("{e}: {error}"))?;
        let status: Value =
            serde_json::from_str(&out).map_err(|_| "malformed supervisor status")?;
        break status;
    };
    ownership::completion(&status, &trace_argv(mode))?;
    let state = docker.validate(own)?;
    if state["State"]["Running"] != true {
        return Err("container stopped before trace copy".into());
    }
    Ok(status)
}
fn snapshot(docker: &mut Docker, own: &mut Owner, folder: &Path) -> Result<Value, String> {
    let previous = docker
        .deadline
        .replace(Instant::now() + Duration::from_secs(60));
    let result = (|| {
        if own.id.is_empty() {
            docker.recover(own)?;
        }
        let state = docker.validate(own)?;
        let call = |arguments: &[String]| {
            let mut command = vec![
                "exec".into(),
                own.id.clone(),
                "/usr/local/bin/python".into(),
                "/probe/supervisor.py".into(),
            ];
            command.extend_from_slice(arguments);
            let output = docker.ok(&command);
            if arguments
                .first()
                .is_some_and(|argument| argument == "--export-chunk")
            {
                if let Some(record) = docker.calls.last_mut() {
                    if let Some(stdout) = record["stdout"].as_str().map(str::to_owned) {
                        let object = record
                            .as_object_mut()
                            .ok_or("invalid Docker call receipt")?;
                        object.remove("stdout");
                        object.insert("stdout_bytes".into(), json!(stdout.len()));
                        object.insert("stdout_sha256".into(), json!(digest(stdout.as_bytes())));
                    }
                }
            }
            output
        };
        #[cfg(not(test))]
        let mut exported = transport::export(folder, call)?;
        #[cfg(test)]
        let mut exported = transport::export_with_uid(folder, unsafe { libc::geteuid() }, call)?;
        exported["container_running_before_copy"] = state["State"]["Running"].clone();
        Ok(exported)
    })();
    docker.deadline = previous;
    result
}
fn qualify_snapshot(
    docker: &mut Docker,
    own: &mut Owner,
    mode: &str,
    folder: &Path,
    status: &Value,
    copied: &Value,
) -> Result<Value, String> {
    if copied["container_running_before_copy"] != true || copied["raw_evidence_complete"] != true {
        return Err("raw snapshot incomplete or tmpfs unavailable after container stop".into());
    }
    let mut traces = BTreeMap::new();
    let mut streams = BTreeMap::new();
    let mut trace_bytes = 0u64;
    for entry in fs::read_dir(folder).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if !metadata.file_type().is_file() {
            return Err("unexpected nonregular copied entry".into());
        }
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| "non-ASCII copied path")?;
        if name.starts_with("trace.") {
            trace_bytes += metadata.len();
            if trace_bytes > 8388608 {
                return Err("trace byte bound exceeded".into());
            }
            let pid: u32 = name
                .strip_prefix("trace.")
                .unwrap()
                .parse()
                .map_err(|_| "invalid PID filename")?;
            if pid == 0 {
                return Err("invalid zero trace PID".into());
            }
            let bytes = read(&path)?;
            let text = String::from_utf8(bytes).map_err(|_| "non-UTF8 trace")?;
            if traces.insert(pid, text).is_some() {
                return Err("duplicate trace PID".into());
            }
        } else if ["stdout", "stderr", "status.json"].contains(&name.as_str()) {
            if metadata.len() > 1048576 {
                return Err("target stream bound exceeded".into());
            }
            streams.insert(name, read(&path)?);
        } else {
            return Err("unexpected fixture write".into());
        }
    }
    ownership::sizes(
        streams.get("stdout").ok_or("missing target stdout")?.len() as u64,
        streams.get("stderr").ok_or("missing target stderr")?.len() as u64,
        trace_bytes,
    )?;
    let copied_status: Value =
        serde_json::from_slice(streams.get("status.json").ok_or("missing copied status")?)
            .map_err(|_| "invalid copied status")?;
    if status != &copied_status {
        return Err("status changed while copying".into());
    }
    ownership::completion(status, &trace_argv(mode))?;
    let decision = trace_decision(mode, &traces)?;
    docker.validate(own)?;
    docker.ok(&[
        "exec".into(),
        own.id.clone(),
        "/usr/local/bin/python".into(),
        "/probe/supervisor.py".into(),
        "--release".into(),
    ])?;
    let actual_wait = docker.ok(&["wait".into(), own.id.clone()])?;
    if actual_wait.trim() != "0" {
        return Err("actual container wait nonzero".into());
    }
    let final_state = docker.validate(own)?;
    if final_state["State"]["Running"] != false
        || final_state["State"]["ExitCode"] != 0
        || final_state["State"]["OOMKilled"] != false
    {
        return Err("container exit metadata mismatch".into());
    }
    Ok(
        json!({"mode":mode,"container_id":own.id,"actual_tracer_status":status,"actual_container_wait":0,"observation":decision,"trace_bytes":trace_bytes}),
    )
}
fn trace_argv(mode: &str) -> Vec<String> {
    let mut args = vec![
        "/usr/bin/strace",
        "-v",
        "-xx",
        "-yy",
        "-s",
        "4096",
        "-e",
        "trace=%process,%file,%network,write,close,close_range,dup,dup2,dup3,fcntl",
        "-o",
        "/work/trace",
        "--output-separately",
    ];
    if mode == "positive" {
        args.push("--follow-forks");
    }
    args.extend([
        "/usr/local/bin/python",
        "/probe/parent.py",
        "write(3, \"CAREERBREW-TRANSIENT-V1\", 24) = 24 unlink() connect()",
    ]);
    args.into_iter().map(str::to_owned).collect()
}
fn sources(root: &Path) -> Result<Value, String> {
    let mut hashes = BTreeMap::new();
    for name in [
        ".gitignore",
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        ".cargo/config.toml",
        "xtask/Cargo.toml",
        "xtask/src/main.rs",
        "xtask/src/capture_http.rs",
        "xtask/src/baseline_verdict.rs",
        "xtask/src/inventory.rs",
        "xtask/src/trial_plan.rs",
        "xtask/src/toolchain_identity.rs",
        "xtask/tests/capture_actions.rs",
        "xtask/tests/capture_http.rs",
        "xtask/tests/observer_parser.rs",
        "xtask/tests/fixtures/linux-observer-replay-v7/trace.10",
        "xtask/tests/fixtures/linux-observer-replay-v7/trace.11",
        "xtask/tests/fixtures/linux-observer-replay-v7/status.json",
        "xtask/tests/fixtures/linux-observer-replay-v7/manifest.json",
        "xtask/tests/fixtures/linux-observer-replay-v7/README.md",
        "xtask/tests/observer_owner.rs",
        "xtask/tests/observer_deadline.rs",
        "xtask/src/capture_actions.rs",
        "xtask/src/observer_probe.rs",
        "xtask/src/observer_probe/parser.rs",
        "xtask/src/observer_probe/ownership.rs",
        "xtask/src/observer_probe/transport.rs",
        "xtask/tests/observer_transport.rs",
        "xtask/tests/fixtures/linux-observer/export.py",
        "xtask/tests/fixtures/linux-observer/Dockerfile",
        "xtask/tests/fixtures/linux-observer/parent.py",
        "xtask/tests/fixtures/linux-observer/child.py",
        "xtask/tests/fixtures/linux-observer/supervisor.py",
    ] {
        hashes.insert(name, digest(&read(&root.join(name))?));
    }
    Ok(json!(hashes))
}
pub fn run(
    root: &Path,
    image: &str,
    out: &Path,
    freeze_only: bool,
    faults_only: bool,
) -> Result<(), String> {
    if !image.starts_with("sha256:")
        || image.len() != 71
        || !image[7..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("require full immutable image ID".into());
    }
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::create_dir(out).map_err(|e| format!("new output directory required: {e}"))?;
    fs::set_permissions(out, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
    let canonical_out = fs::canonicalize(out).map_err(|e| e.to_string())?;
    let out = canonical_out.as_path();
    for name in ["home", "config", "tmp"] {
        fs::create_dir(out.join(name)).map_err(|e| e.to_string())?;
    }
    let cli = fs::canonicalize(CLI).map_err(|_| "missing local Docker CLI")?;
    let socket = PathBuf::from(
        std::env::var_os("HOME").ok_or("host HOME needed to locate the local Docker socket")?,
    )
    .join(".docker/run/docker.sock");
    if !fs::metadata(&socket)
        .map_err(|_| "missing local Docker socket")?
        .file_type()
        .is_socket()
    {
        return Err("Docker endpoint is not a local socket".into());
    }
    let endpoint = format!("unix://{}", socket.display());
    let mut docker = Docker {
        endpoint,
        owned: out.to_owned(),
        cli_sha: digest(&read(&cli)?),
        cli,
        daemon: String::new(),
        calls: Vec::new(),
        deadline: None,
    };
    let result = (|| {
        let (code, daemon, _) =
            docker.raw(&["info".into(), "--format".into(), "{{.ID}}".into()])?;
        ownership::client_status(code)?;
        if daemon.trim().is_empty() {
            return Err("missing daemon ID".into());
        }
        docker.daemon = daemon.trim().into();
        let image_json: Value =
            serde_json::from_str(&docker.ok(&["image".into(), "inspect".into(), image.into()])?)
                .map_err(|_| "invalid image JSON")?;
        let metadata = image_json
            .as_array()
            .filter(|a| a.len() == 1)
            .and_then(|a| a.first())
            .ok_or("image not unique")?;
        if metadata["Id"] != image
            || metadata["Os"] != "linux"
            || metadata["Architecture"] != "arm64"
        {
            return Err("wrong image/platform".into());
        }
        let source_hashes = sources(root)?;
        let mut context = BTreeMap::new();
        for name in [
            "Dockerfile",
            "parent.py",
            "child.py",
            "supervisor.py",
            "export.py",
        ] {
            context.insert(
                name,
                digest(&read(
                    &root.join("xtask/tests/fixtures/linux-observer").join(name),
                )?),
            );
        }
        let context_digest = digest(&serde_json::to_vec(&context).map_err(|e| e.to_string())?);
        if metadata["Config"]["Labels"]["careerbrew.observer.context"] != context_digest {
            return Err("image build context label does not match frozen sources".into());
        }
        let freeze = json!({"schema":"careerbrew-linux-observer-v1","scope":"fixed Linux/arm64 capability only; full Rust00 unqualified","daemon":docker.daemon,"endpoint":docker.endpoint,"docker_cli":docker.cli,"docker_cli_sha256":docker.cli_sha,"executable_sha256":digest(&read(&std::env::current_exe().map_err(|e|e.to_string())?)?),"image":image,"image_os":"linux","image_arch":"arm64","context_hashes":context,"context_sha256":context_digest,"source_hashes":source_hashes,"positive_trace_argv":trace_argv("positive"),"negative_change":"omit --follow-forks only","controls":"14 parser +12 controller decision groups before execution; actual O08/O09 precede traces"});
        store(&out.join("freeze.json"), &freeze)?;
        if freeze_only {
            return Ok(json!({"frozen":true,"actual_trace_trials":0}));
        }
        let actual_faults = fault_controls(&mut docker, image, out)?;
        if faults_only {
            if sources(root)? != source_hashes {
                return Err("source inputs changed during fault controls".into());
            }
            return Ok(
                json!({"actual_fault_controls":actual_faults,"actual_trace_trials":0,"full_rust00_qualified":false}),
            );
        }
        let mut results = Vec::new();
        for mode in ["positive", "negative"] {
            let mut own = owner(&docker, image, mode);
            results.push(finish_trial(
                &mut docker,
                &mut own,
                mode,
                out,
                Duration::from_secs(30),
            )?);
        }
        if sources(root)? != source_hashes {
            return Err("source inputs changed during probe".into());
        }
        Ok(
            json!({"results":results,"full_rust00_qualified":false,"actual_fault_controls":actual_faults}),
        )
    })();
    store(&out.join("docker-calls.json"), &json!(docker.calls))?;
    store(
        &out.join("result.json"),
        &json!({"result":outcome(&result)}),
    )?;
    result.map(|_| ())
}

pub fn fault_client(request: &Path, endpoint: &str) -> Result<(), String> {
    use std::io::Write;
    let value: Value =
        serde_json::from_slice(&read(request)?).map_err(|_| "invalid fault client request")?;
    if value["endpoint"] != endpoint || !endpoint.starts_with("unix:///") {
        return Err("fault endpoint differs from controller pin".into());
    }
    let owned = PathBuf::from(value["owned"].as_str().ok_or("missing owned root")?);
    if request.parent() != Some(owned.as_path())
        || fs::canonicalize(&owned).map_err(|e| e.to_string())? != owned
    {
        return Err("fault request root not exact".into());
    }
    let cli = fs::canonicalize(CLI).map_err(|e| e.to_string())?;
    let mut docker = Docker {
        endpoint: endpoint.into(),
        owned: owned.clone(),
        cli,
        cli_sha: value["cli_sha256"]
            .as_str()
            .ok_or("missing CLI pin")?
            .into(),
        daemon: value["daemon"].as_str().ok_or("missing daemon pin")?.into(),
        calls: Vec::new(),
        deadline: None,
    };
    let own = Owner {
        daemon: docker.daemon.clone(),
        name: value["name"].as_str().ok_or("missing name")?.into(),
        label: value["label"].as_str().ok_or("missing label")?.into(),
        id: String::new(),
        image: value["image"].as_str().ok_or("missing image")?.into(),
    };
    if !own.name.starts_with("cb-observer-")
        || !own
            .name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        || own.label.is_empty()
    {
        return Err("invalid fault ownership".into());
    }
    let id = docker.ok(&create_args(&own, "--hold"))?;
    let inner = json!({"actual_create_exit":0,"actual_create_id":id.trim(),"calls":docker.calls,"fault":"SIGKILL Rust adapter before ID forwarding"});
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(owned.join("fault-create-inner.json"))
        .map_err(|e| e.to_string())?;
    file.write_all(&serde_json::to_vec_pretty(&inner).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    file.sync_all().map_err(|e| e.to_string())?;
    // Deliberate fault in this exact adapter process, not lifecycle cleanup.
    unsafe {
        libc::raise(libc::SIGKILL);
    }
    Err("fault adapter survived its deliberate signal".into())
}
fn lost_output(docker: &mut Docker, own: &mut Owner) -> Result<Value, String> {
    let request = docker.owned.join("fault-create-request.json");
    store(
        &request,
        &json!({"endpoint":docker.endpoint,"owned":docker.owned,"cli_sha256":docker.cli_sha,"daemon":docker.daemon,"name":own.name,"label":own.label,"image":own.image}),
    )?;
    let executable = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut command = ownership::command(&executable, &docker.owned, &docker.endpoint);
    command.arg("observer-fault-client").arg(&request);
    let (pid, code, stdout, stderr) =
        capture_actions::execute_bounded(command, Duration::from_secs(30))?;
    docker
        .calls
        .push(json!({"fault_adapter_pid":pid,"actual_exit":code,"stdout":stdout,"stderr":stderr}));
    if code.is_some() || !stdout.is_empty() || !stderr.is_empty() {
        return Err("expected actual fault adapter death without forwarded ID".into());
    }
    // Decision must happen before the independent inner-ID oracle is read.
    docker.recover(own)?;
    let inner_bytes = read(&docker.owned.join("fault-create-inner.json"))?;
    let inner: Value =
        serde_json::from_slice(&inner_bytes).map_err(|_| "invalid inner create receipt")?;
    if inner["actual_create_exit"] != 0 || inner["actual_create_id"] != own.id {
        return Err("recovery differs from independent actual create ID".into());
    }
    Ok(
        json!({"control":"O08","passed":true,"fault_boundary":"Rust client adapter death after actual Docker create zero","adapter_pid":pid,"actual_adapter_exit":code,"recovered_id":own.id,"inner_receipt_sha256":digest(&inner_bytes),"independent_inner_id_match_after_recovery":true}),
    )
}
fn finish_trial(
    docker: &mut Docker,
    own: &mut Owner,
    mode: &str,
    out: &Path,
    status_budget: Duration,
) -> Result<Value, String> {
    let primary = trial(docker, own, mode, status_budget);
    let raw_copy = snapshot(docker, own, &out.join(mode));
    let qualification = match (&primary, &raw_copy) {
        (Ok(status), Ok(copied)) => {
            qualify_snapshot(docker, own, mode, &out.join(mode), status, copied)
        }
        (Err(error), _) => Err(error.clone()),
        (_, Err(error)) => Err(format!("raw snapshot failed: {error}")),
    };
    let cleanup = docker.cleanup(own);
    store(
        &out.join(format!("{mode}-receipt.json")),
        &json!({"primary":outcome(&primary),"raw_copy":outcome(&raw_copy),"qualification":outcome(&qualification),"cleanup":outcome(&cleanup)}),
    )?;
    let qualified = qualification?;
    cleanup?;
    Ok(qualified)
}
fn observation_json(observed: &capture_actions::OwnerObservation) -> Value {
    json!({"pid":observed.pid,"program":observed.program.to_string_lossy(),"argv":observed.args.iter().map(|arg|arg.to_string_lossy().into_owned()).collect::<Vec<_>>(),"actual_wait_code":observed.wait_code,"actual_wait_signal":observed.wait_signal,"wait_observed":observed.wait_observed,"reaped":observed.reaped,"cleanup_succeeded":observed.cleanup_succeeded})
}
fn attached_outcome(
    attached: &Result<capture_actions::OwnerExecution, capture_actions::OwnerFailure>,
    program: &std::ffi::OsStr,
    args: &[std::ffi::OsString],
) -> Result<Value, String> {
    let failure = attached
        .as_ref()
        .err()
        .ok_or("attached client did not reach its owned deadline")?;
    let observed = failure
        .observation
        .as_ref()
        .ok_or("attached client never spawned")?;
    if failure.cause != capture_actions::OwnerFailureCause::Deadline
        || observed.pid == 0
        || observed.program != program
        || observed.args != args
        || observed.wait_code.is_some()
        || observed.wait_signal != Some(libc::SIGKILL)
        || !observed.wait_observed
        || !observed.reaped
        || !observed.cleanup_succeeded
    {
        return Err(
            "attached client lacks exact deadline, execution and cleanup/reap proof".into(),
        );
    }
    Ok(observation_json(observed))
}
fn client_death(docker: &mut Docker, own: &mut Owner) -> Result<Value, String> {
    create(docker, own, "--hold")?;
    docker.validate(own)?;
    let prior = docker
        .deadline
        .replace(Instant::now() + Duration::from_secs(2));
    let precheck = docker.check_daemon();
    docker.deadline = prior;
    precheck?;
    let args = vec!["start".into(), "--attach".into(), own.id.clone()];
    let mut expected_args = vec![
        std::ffi::OsString::from("--host"),
        std::ffi::OsString::from(&docker.endpoint),
    ];
    expected_args.extend(args.iter().map(std::ffi::OsString::from));
    let attached = docker.observed(&args, Duration::from_secs(2));
    let execution = attached_outcome(&attached, docker.cli.as_os_str(), &expected_args)?;
    let state = docker.validate(own)?;
    if state["State"]["Running"] != true {
        return Err("container did not survive actual Docker client death".into());
    }
    Ok(
        json!({"control":"O09","passed":true,"attached_execution":execution,"same_owned_container_still_running":true,"container_id":own.id}),
    )
}

fn fault_controls(docker: &mut Docker, image: &str, out: &Path) -> Result<Value, String> {
    let mut results = Vec::new();
    for case in ["O08", "O09"] {
        let mut own = owner(docker, image, case);
        let primary = if case == "O08" {
            lost_output(docker, &mut own)
        } else {
            client_death(docker, &mut own)
        };
        let cleanup = docker.cleanup(&mut own);
        store(
            &out.join(format!("{case}-receipt.json")),
            &json!({"primary":outcome(&primary),"cleanup":outcome(&cleanup)}),
        )?;
        results.push(primary?);
        cleanup?;
    }
    Ok(json!(results))
}

fn trace_decision(mode: &str, traces: &BTreeMap<u32, String>) -> Result<Value, String> {
    match (mode, parser::observe(traces)?) {
        ("positive", parser::Observation::Complete(verdict)) => {
            Ok(json!({"accepted":true,"events":verdict.events}))
        }
        (
            "negative",
            parser::Observation::ExpectedMissingChild {
                parent_pid,
                child_pid,
            },
        ) => Ok(
            json!({"accepted":false,"reason":"expected missing child trace after validated parent contract","parent_pid":parent_pid,"child_pid":child_pid}),
        ),
        (_, parser::Observation::ExpectedMissingChild { .. }) => Err("missing child trace".into()),
        (_, parser::Observation::Complete(_)) => {
            Err("incomplete no-follow observer accepted".into())
        }
    }
}

#[cfg(test)]
mod repair_controls {
    use super::*;
    fn negative_parent() -> BTreeMap<u32, String> {
        let hex = |s: &str| {
            format!(
                "\"{}\"",
                s.bytes().map(|b| format!("\\x{b:02x}")).collect::<String>()
            )
        };
        [(11, format!("execve({}, [{}, {}], 0x0 /* 0 vars */) = 0\nclone(child_stack=NULL, flags=CLONE_VM|CLONE_VFORK|SIGCHLD) = 22\nwait4(22, [{{WIFEXITED(s) && WEXITSTATUS(s) == 0}}], 0, NULL) = 22\nexit_group(0) = ?\n+++ exited with 0 +++\n", hex("/usr/local/bin/python"),hex("/usr/local/bin/python"),hex("/probe/parent.py")))].into()
    }
    #[test]
    fn negative_admission_valid_parent() {
        assert!(parser::verify(&negative_parent()).is_err());
        assert_eq!(
            trace_decision("negative", &negative_parent()).unwrap()["accepted"],
            false
        );
    }
    #[test]
    fn negative_admission_missing_wait() {
        let mut traces = negative_parent();
        let text = traces.get_mut(&11).unwrap();
        *text = text
            .lines()
            .filter(|line| !line.starts_with("wait4("))
            .map(|line| format!("{line}\n"))
            .collect();
        assert!(
            trace_decision("negative", &traces).is_err(),
            "unsafe missing-wait parent credited as intended negative"
        );
    }
    #[test]
    fn negative_admission_extra_parent_unlink() {
        let mut traces = negative_parent();
        let text = traces.get_mut(&11).unwrap();
        *text = text.replace("exit_group", "unlinkat(AT_FDCWD, \"\\x2f\\x77\\x6f\\x72\\x6b\\x2f\\x75\\x6e\\x65\\x78\\x70\\x65\\x63\\x74\\x65\\x64\", 0) = 0\nexit_group");
        assert!(
            trace_decision("negative", &traces).is_err(),
            "unsafe parent unlink credited as intended negative"
        );
    }
    // Unit transport only. No request reaches a Docker daemon.
    const CONTROLLER_TRANSPORT: &str = r#"
import json
import os
from pathlib import Path
import shutil
import sys
import time
root = Path(__file__).parent
assert sys.argv[1:3] == ["--host", "unix:///synthetic/docker.sock"]
args = sys.argv[3:]
op = args[0]
with (root / "operations").open("a") as stream:
    stream.write(op + "\n")
mode = (root / "mode").read_text()
if op == "info":
    counter = root / "info-count"
    count = int(counter.read_text()) + 1 if counter.exists() else 1
    counter.write_text(str(count))
    if mode == "precheck-timeout" and count == 4:
        (root / "precheck-paused").touch()
        time.sleep(3)
    print("daemon-v1")
elif op == "create":
    (root / "created").touch()
    print("a" * 64)
elif op == "inspect":
    if (root / "removed").exists():
        print("[]")
        print("Error response from daemon: No such container: " + args[-1], file=sys.stderr)
        raise SystemExit(1)
    metadata = json.loads((root / "metadata.json").read_text())
    stopped = (root / "stopped").exists()
    running = not stopped and (mode == "precheck-timeout" or (root / "started").exists())
    metadata["State"] = {"Running": running, "Status": "exited" if stopped else "running" if running else "created", "ExitCode": 137 if stopped else 0, "OOMKilled": False}
    print(json.dumps([metadata]))
elif op == "start":
    (root / "started").touch()
    if "--attach" in args:
        (root / "attached").touch()
        time.sleep(3)
elif op == "exec":
    if args[4].startswith("--export-"):
        if mode == "copy-failure":
            raise SystemExit(24)
        sys.dont_write_bytecode = True
        sys.path.insert(0, str(root))
        import exporter
        exporter.main(root / "work", os.getuid(), args[4:])
    elif args[-1] == "--status":
        (root / "status-polled").touch()
        if mode == "nonzero-status":
            raise SystemExit(23)
        if mode == "pending-status":
            raise SystemExit(75)
        if mode in {"stopped-status", "failed-tracer"}:
            status = json.loads((root / "unit-status.json").read_text())
            if mode == "stopped-status":
                (root / "stopped").touch()
            else:
                status["actual_tracer_wait"] = 7
            print(json.dumps(status))
        else:
            print("{malformed-status")
elif op == "cp":
    if mode in {"empty-cp", "transport-invalid-peer"}:
        raise SystemExit(0)
    if mode == "copy-failure":
        raise SystemExit(24)
    destination = Path(args[-1])
    destination.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(root / "raw-available", destination / "stderr")
    (root / "copied").touch()
elif op == "stop":
    (root / "stopped").touch()
elif op == "wait":
    print(137 if (root / "stopped").exists() else 0)
elif op == "rm":
    if mode == "cleanup-failure":
        raise SystemExit(25)
    (root / "raw-available").unlink(missing_ok=True)
    shutil.rmtree(root / "work", ignore_errors=True)
    (root / "removed").touch()
else:
    raise AssertionError(op)
"#;
    fn fixture(mode: &str) -> (tempfile::TempDir, Docker, Owner) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for name in ["home", "config", "tmp"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        fs::write(root.join("mode"), mode).unwrap();
        fs::write(root.join("raw-available"), b"synthetic failure evidence").unwrap();
        let image = format!("sha256:{}", "b".repeat(64));
        let metadata = json!({"Id":"a".repeat(64),"Name":"/cb-observer-unit","Image":image,"Platform":"linux","Config":{"Image":image,"User":"65534:65534","Labels":{"careerbrew.observer":"unit-label"}},"HostConfig":{"NetworkMode":"none","PidMode":"","IpcMode":"private","Privileged":false,"ReadonlyRootfs":true,"SecurityOpt":["no-new-privileges"],"CapDrop":["ALL"],"CapAdd":null,"Memory":268435456,"MemorySwap":268435456,"PidsLimit":32,"Tmpfs":{"/work":ownership::TMPFS}},"Mounts":[]});
        store(&root.join("metadata.json"), &metadata).unwrap();
        store(&root.join("unit-status.json"),&json!({"tracer_pid":17,"observer_argv":trace_argv("positive"),"complete":true,"bounds_ok":true,"actual_tracer_wait":0,"transient_absent":true})).unwrap();
        let work = root.join("work");
        fs::create_dir(&work).unwrap();
        fs::write(work.join("stderr"), b"synthetic failure evidence").unwrap();
        if mode == "empty-cp" {
            fs::write(work.join("stdout"), b"").unwrap();
            fs::copy(root.join("unit-status.json"), work.join("status.json")).unwrap();
            fs::write(work.join("trace.7"), b"synthetic raw trace").unwrap();
        }
        if mode == "transport-invalid-peer" {
            std::os::unix::fs::symlink(work.join("stderr"), work.join("trace.8")).unwrap();
        }
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/linux-observer/export.py"),
            root.join("exporter.py"),
        )
        .unwrap();
        let python = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".venv/bin/python");
        let cli = root.join("unit-transport");
        fs::write(
            &cli,
            format!("#!{}\n{}", python.display(), CONTROLLER_TRANSPORT),
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o700)).unwrap();
        let docker = Docker {
            endpoint: "unix:///synthetic/docker.sock".into(),
            owned: root.to_owned(),
            cli_sha: digest(&fs::read(&cli).unwrap()),
            cli,
            daemon: "daemon-v1".into(),
            calls: Vec::new(),
            deadline: None,
        };
        let owner = Owner {
            daemon: "daemon-v1".into(),
            name: "cb-observer-unit".into(),
            label: "unit-label".into(),
            id: String::new(),
            image,
        };
        (dir, docker, owner)
    }
    #[test]
    fn transport_t01_empty_cp_is_not_complete_evidence() {
        let (dir, mut docker, mut own) = fixture("empty-cp");
        let result = snapshot(&mut docker, &mut own, &dir.path().join("snapshot")).unwrap();
        assert_eq!(
            result["raw_evidence_complete"], true,
            "successful empty cp lost the actual source bytes"
        );
        assert_eq!(
            fs::read(dir.path().join("snapshot/stderr")).unwrap(),
            b"synthetic failure evidence"
        );
    }
    #[test]
    fn transport_t11_safe_peer_survives_invalid_peer_and_primary_failure() {
        let (dir, mut docker, mut own) = fixture("transport-invalid-peer");
        assert!(finish_trial(
            &mut docker,
            &mut own,
            "positive",
            dir.path(),
            Duration::from_millis(100)
        )
        .is_err());
        assert_eq!(
            fs::read(dir.path().join("positive/stderr")).unwrap(),
            b"synthetic failure evidence"
        );
        let receipt: Value =
            serde_json::from_slice(&fs::read(dir.path().join("positive-receipt.json")).unwrap())
                .unwrap();
        assert!(receipt["primary"]["error"]
            .as_str()
            .unwrap()
            .contains("status"));
        assert_eq!(receipt["raw_copy"]["ok"]["raw_evidence_complete"], false);
        assert!(receipt["raw_copy"]["ok"]["issues"]
            .as_array()
            .is_some_and(|issues| !issues.is_empty()));
        assert!(receipt["cleanup"]["ok"].is_object());
    }
    #[test]
    fn o09_daemon_timeout_cannot_prove_attached_execution() {
        let (dir, mut docker, mut own) = fixture("precheck-timeout");
        let result = client_death(&mut docker, &mut own);
        assert!(dir.path().join("precheck-paused").exists());
        assert!(!dir.path().join("attached").exists());
        assert!(
            result.is_err(),
            "accepted a daemon timeout without attached execution"
        );
    }
    #[test]
    fn o09_cleanup_failure_is_not_reap_proof() {
        let program = std::ffi::OsString::from("/unit-docker");
        let args = vec![std::ffi::OsString::from("start")];
        let mut observed = capture_actions::OwnerObservation {
            pid: 17,
            program: program.clone(),
            args: args.clone(),
            wait_code: None,
            wait_signal: Some(libc::SIGKILL),
            wait_observed: true,
            reaped: true,
            cleanup_succeeded: false,
        };
        for variant in 0..6 {
            let mut current = observed.clone();
            let mut cause = capture_actions::OwnerFailureCause::Deadline;
            match variant {
                0 => {}
                1 => {
                    current.cleanup_succeeded = true;
                    current.pid = 0;
                }
                2 => {
                    current.cleanup_succeeded = true;
                    current.program = "/other".into();
                }
                3 => {
                    current.cleanup_succeeded = true;
                    current.args = vec!["other".into()];
                }
                4 => {
                    current.cleanup_succeeded = true;
                    cause = capture_actions::OwnerFailureCause::Callback;
                }
                _ => {
                    current.cleanup_succeeded = true;
                    current.reaped = false;
                }
            }
            let result = Err(capture_actions::OwnerFailure {
                cause,
                observation: Some(current),
                message: "capture child exceeded timeout; child cleanup failed: synthetic failure"
                    .into(),
            });
            assert!(attached_outcome(&result, &program, &args).is_err());
        }
        observed.cleanup_succeeded = true;
        let valid = Err(capture_actions::OwnerFailure {
            cause: capture_actions::OwnerFailureCause::Deadline,
            observation: Some(observed),
            message: "diagnostic is not authority".into(),
        });
        assert!(attached_outcome(&valid, &program, &args).is_ok());
    }
    #[test]
    fn o11_available_raw_precedes_destructive_cleanup() {
        for mode in [
            "malformed-status",
            "nonzero-status",
            "pending-status",
            "stopped-status",
            "failed-tracer",
        ] {
            let (dir, mut docker, mut own) = fixture(mode);
            assert!(finish_trial(
                &mut docker,
                &mut own,
                "positive",
                dir.path(),
                Duration::from_millis(100)
            )
            .is_err());
            assert!(dir.path().join("status-polled").exists());
            assert!(
                !dir.path().join("raw-available").exists(),
                "cleanup did not delete its source"
            );
            assert_eq!(
                fs::read(dir.path().join("positive/stderr")).unwrap(),
                b"synthetic failure evidence"
            );
            let receipt: Value = serde_json::from_slice(
                &fs::read(dir.path().join("positive-receipt.json")).unwrap(),
            )
            .unwrap();
            assert!(receipt["primary"]["error"].is_string());
            assert_eq!(receipt["raw_copy"]["ok"]["copy_completed"], true);
            assert!(receipt["qualification"]["error"].is_string());
        }
        for mode in ["copy-failure", "cleanup-failure"] {
            let (dir, mut docker, mut own) = fixture(mode);
            assert!(finish_trial(
                &mut docker,
                &mut own,
                "positive",
                dir.path(),
                Duration::from_millis(100)
            )
            .is_err());
            let receipt: Value = serde_json::from_slice(
                &fs::read(dir.path().join("positive-receipt.json")).unwrap(),
            )
            .unwrap();
            assert!(receipt["primary"]["error"].is_string());
            assert!(receipt["qualification"]["error"].is_string());
            if mode == "copy-failure" {
                assert!(receipt["raw_copy"]["error"].is_string());
                assert!(receipt["cleanup"]["ok"].is_object());
            } else {
                assert_eq!(receipt["raw_copy"]["ok"]["copy_completed"], true);
                assert!(receipt["cleanup"]["error"].is_string());
                assert_eq!(
                    fs::read(dir.path().join("positive/stderr")).unwrap(),
                    b"synthetic failure evidence"
                );
            }
        }
    }
}
