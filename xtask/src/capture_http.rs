// Observe a bounded handler corpus through real invocation-owned sockets.
use crate::capture_actions as shared;
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{SocketAddr, TcpStream},
    path::Path,
    process::Command,
    thread,
    time::Duration,
};

const CASE: &str = include_str!("../tests/fixtures/capture/http-v1.json");
const DRIVER: &str = include_str!("../tests/fixtures/capture/http_driver.py");
const OBSERVER: &str = include_str!("../tests/fixtures/capture/observer/sitecustomize.py");
const HISTORICAL_REFERENCE: &str = include_str!("../tests/fixtures/capture/http-reference-v1.json");
const HISTORICAL_REFERENCE_SHA256: &str =
    "e58db0db976b3205bc206437141e3877a440a3a8a46f9da713e4a215fdb9eaf3";
const REFERENCE: &str =
    include_str!("../tests/fixtures/capture/http-reference-rules-promise-v3.json");
const REFERENCE_SHA256: &str = "bddc10911112bb79118ff109e0466426f912121996c4d7027276ee97ccdf8e64";
const ACTIONS: &str =
    include_str!("../tests/fixtures/capture/actions-reference-rules-promise-v3.json");
const LIMIT: usize = 4 * 1024 * 1024;

fn live(check: &mut dyn FnMut() -> Result<bool, String>) -> Result<(), String> {
    if check()? {
        return Err("HTTP oracle exited before completing socket observations".into());
    }
    Ok(())
}

fn request(
    port: u16,
    case: &Value,
    token: &str,
    cookie: &str,
    check: &mut dyn FnMut() -> Result<bool, String>,
) -> Result<Value, String> {
    live(check)?;
    let name = case["name"].as_str().ok_or("case name missing")?;
    let method = case["method"].as_str().ok_or("method missing")?;
    let path = case["path"].as_str().ok_or("path missing")?;
    let host = if name == "bad-host" {
        "hostile.example.com".to_owned()
    } else if name.starts_with("localhost") {
        format!("localhost:{port}")
    } else {
        format!("127.0.0.1:{port}")
    };
    let mut headers = BTreeMap::from([
        ("Host".to_owned(), host),
        ("Connection".to_owned(), "close".into()),
        ("Content-Type".to_owned(), "application/json".into()),
    ]);
    if name != "missing-token" {
        headers.insert(
            "X-Careerbrew-Token".into(),
            if name.starts_with("wrong") {
                "synthetic-wrong-token"
            } else {
                token
            }
            .into(),
        );
    }
    if matches!(name, "restore-cookie" | "restore-cross-site") {
        headers.insert("Cookie".into(), cookie.into());
    }
    if matches!(
        name,
        "set-cookie" | "restore-cookie" | "set-cross-site" | "restore-cross-site"
    ) {
        headers.insert(
            "Sec-Fetch-Site".into(),
            if name.ends_with("cross-site") {
                "cross-site"
            } else {
                "same-origin"
            }
            .into(),
        );
    }
    let body = match name {
        "invalid-values" => "{\"values\":[1]}",
        "malformed-json" => "{",
        "array-json" => "[]",
        _ if method == "POST" => "{}",
        _ => "",
    };
    headers.insert(
        "Content-Length".into(),
        if name == "oversized-request" {
            "64001".into()
        } else {
            body.len().to_string()
        },
    );
    let mut wire = format!("{method} {path} HTTP/1.0\r\n");
    for (name, value) in &headers {
        wire.push_str(&format!("{name}: {value}\r\n"));
    }
    wire.push_str("\r\n");
    wire.push_str(body);
    let sent_body = body.to_owned();
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(2))
        .map_err(|_| "HTTP socket connect failed")?;
    stream
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|_| "cannot bound HTTP reads")?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|_| "cannot bound HTTP writes")?;
    stream
        .write_all(wire.as_bytes())
        .map_err(|_| "HTTP request write failed")?;
    let mut response = Vec::new();
    loop {
        live(check)?;
        let mut chunk = [0_u8; 8192];
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(count) => {
                if response.len() + count > LIMIT {
                    return Err("HTTP response exceeds limit".into());
                }
                response.extend_from_slice(&chunk[..count]);
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(_) => return Err("HTTP response read failed".into()),
        }
    }
    let raw = String::from_utf8(response).map_err(|_| "HTTP response is not UTF-8")?;
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or("HTTP response headers incomplete")?;
    if head.len() > 64_000 {
        return Err("HTTP headers exceed limit".into());
    }
    let mut lines = head.split("\r\n");
    let status_line = lines.next().ok_or("HTTP status missing")?;
    let pieces = status_line.splitn(3, ' ').collect::<Vec<_>>();
    if pieces.len() != 3 || pieces[0] != "HTTP/1.0" || pieces[2].is_empty() {
        return Err("HTTP status framing invalid".into());
    }
    if pieces[1].len() != 3 || !pieces[1].bytes().all(|b| b.is_ascii_digit()) {
        return Err("HTTP status invalid".into());
    }
    let status: u16 = pieces[1].parse().map_err(|_| "HTTP status invalid")?;
    if !(100..=599).contains(&status) {
        return Err("HTTP status invalid".into());
    }
    let mut received = BTreeMap::new();
    for line in lines {
        let (name, value) = line.split_once(':').ok_or("HTTP header malformed")?;
        if name.is_empty()
            || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            || value.contains(['\r', '\n'])
        {
            return Err("HTTP header malformed".into());
        }
        if received
            .insert(name.to_ascii_lowercase(), value.trim().to_owned())
            .is_some()
        {
            return Err("duplicate HTTP header refused".into());
        }
    }
    if received.contains_key("transfer-encoding") {
        return Err("HTTP transfer encoding unsupported".into());
    }
    let length: usize = received
        .get("content-length")
        .ok_or("HTTP content length missing")?
        .parse()
        .map_err(|_| "HTTP content length invalid")?;
    if length != body.len() {
        return Err("HTTP body length differs".into());
    }
    let decoded: Value = serde_json::from_str(body).map_err(|_| "HTTP response JSON invalid")?;
    let date = received.get("date").ok_or("HTTP Date missing")?;
    if !valid_http_date(date) {
        return Err("HTTP Date invalid".into());
    }
    let raw_headers = received.clone();
    received.insert("date".into(), "<observed-http-date>".into());
    if let Some(value) = received.get_mut("set-cookie") {
        let prefix = format!("careerbrew-{port}=");
        if !value.starts_with(&prefix) {
            return Err("HTTP cookie name differs".into());
        }
        *value = value.replacen(&prefix, "careerbrew-<port>=", 1);
    }
    Ok(
        json!({"name":name,"method":method,"path":path,"request_headers":headers,"request_body":sent_body,"status":status,"status_line":status_line,"raw_headers":raw_headers,"headers":received,"body":decoded,"raw_body":body}),
    )
}

fn valid_http_date(value: &str) -> bool {
    use std::ffi::{CStr, CString};
    if !value.is_ascii() || value.len() != 29 {
        return false;
    }
    let Ok(input) = CString::new(value) else {
        return false;
    };
    let format = c"%a, %d %b %Y %H:%M:%S GMT";
    let mut parsed: libc::tm = unsafe { std::mem::zeroed() };
    let end = unsafe { libc::strptime(input.as_ptr(), format.as_ptr(), &mut parsed) };
    if end.is_null() || unsafe { *end } != 0 {
        return false;
    }
    // timegm normalizes impossible calendar dates; exact formatting must match.
    unsafe { libc::timegm(&mut parsed) };
    let mut output = [0_i8; 64];
    let count =
        unsafe { libc::strftime(output.as_mut_ptr(), output.len(), format.as_ptr(), &parsed) };
    count == 29 && unsafe { CStr::from_ptr(output.as_ptr()) }.to_bytes() == value.as_bytes()
}

fn validate_case(response: &Value, case: &Value, token: &str) -> Result<(), String> {
    if response["status"] != case["status"] {
        return Err(format!("HTTP status differs for {}", case["name"]));
    }
    for (name,value) in [
        ("content-type","application/json"), ("cache-control","no-store"),
        ("x-content-type-options","nosniff"), ("referrer-policy","no-referrer"),
        ("cross-origin-resource-policy","same-origin"),
        ("content-security-policy","default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
    ] { if response["headers"][name] != value { return Err(format!("HTTP security header differs: {name}")); } }
    let name = case["name"].as_str().unwrap();
    if matches!(name, "actions" | "localhost") {
        if response["body"]
            != json!({"actions":serde_json::from_str::<Value>(ACTIONS).map_err(|_| "action reference invalid")?})
        {
            return Err("HTTP action reference differs".into());
        }
    } else if case["status"] != 200 {
        let object = response["body"]
            .as_object()
            .ok_or("HTTP error body not object")?;
        if object.len() != 1
            || object
                .get("error")
                .and_then(Value::as_str)
                .is_none_or(str::is_empty)
        {
            return Err("HTTP error schema differs".into());
        }
    } else if name == "restore-cookie" {
        if response["body"] != json!({"token":token}) {
            return Err("HTTP restored token differs".into());
        }
    } else if name == "ping" && response["body"] != json!({"ok":true,"warm":"off"}) {
        return Err("HTTP ping differs".into());
    } else if name == "set-cookie" || name == "localhost-cookie" {
        if response["body"]["signed_in"] != true
            || !response["body"]["days_left"].is_i64()
            || response["body"]["cookie"] != (name == "set-cookie")
        {
            return Err("HTTP session schema differs".into());
        }
        if name == "set-cookie" {
            let value = response["headers"]["set-cookie"]
                .as_str()
                .ok_or("HTTP session cookie missing")?;
            if !value.starts_with(&format!("careerbrew-<port>={token}; Max-Age="))
                || !value.ends_with("; Path=/; HttpOnly; SameSite=Strict")
                || value.contains("Domain=")
            {
                return Err("HTTP session cookie differs".into());
            }
        } else if !response["headers"]["set-cookie"].is_null() {
            return Err("localhost set a cookie".into());
        }
    }
    Ok(())
}

fn observe(
    root: &Path,
    python: &Path,
    fixture: &Value,
    sources: &Value,
    fault: &str,
    pid_path: &Path,
) -> Result<Value, String> {
    let scratch = tempfile::tempdir().map_err(|_| "cannot create HTTP invocation")?;
    let owned = scratch.path().join("owned");
    let mut environment = shared::owned_environment(root, &owned)?;
    let helpers = root.join("xtask/tests/fixtures/capture");
    for (path, expected) in [
        (helpers.join("http_driver.py"), DRIVER),
        (helpers.join("observer/sitecustomize.py"), OBSERVER),
    ] {
        if fs::read(&path).map_err(|_| "HTTP helper missing")? != expected.as_bytes() {
            return Err("HTTP helper changed".into());
        }
    }
    let control = scratch.path().join("control.json");
    let observer = scratch.path().join("observer.json");
    let protocol = scratch.path().join("protocol");
    fs::create_dir(&protocol).map_err(|_| "cannot prepare HTTP protocol")?;
    fs::write(&control,serde_json::to_vec(&json!({"fault":fault,"canary":fixture["canary"],"token":fixture["token"],"protocol":protocol})).unwrap()).map_err(|_| "cannot write HTTP control")?;
    for (key, value) in [
        ("PYTHONPATH", helpers.join("observer")),
        ("CAREERBREW_CAPTURE_ROOT", root.to_owned()),
        ("CAREERBREW_CAPTURE_INPUT", control),
        ("CAREERBREW_CAPTURE_OBSERVER", observer.clone()),
        (
            "CAREERBREW_CAPTURE_DESCENDANT",
            scratch.path().join("descendant"),
        ),
    ] {
        environment.insert(key.into(), value.to_string_lossy().into_owned());
    }
    environment.insert(
        "PYTHONPATH".into(),
        format!(
            "{}:{}",
            helpers.join("observer").display(),
            root.join("src").display()
        ),
    );
    if fault == "hang" {
        environment.insert(
            "CAREERBREW_HTTP_TEST_PID".into(),
            pid_path.to_string_lossy().into_owned(),
        );
    }
    let before = shared::files(&owned, &owned, None)?;
    let mut command = Command::new(python);
    command
        .args(["-B"])
        .arg(helpers.join("http_driver.py"))
        .current_dir(root)
        .env_clear()
        .envs(&environment);
    let mut responses = Vec::new();
    let mut ready = Value::Null;
    let mut catalog_matches = true;
    let token = fixture["token"]
        .as_str()
        .ok_or("HTTP token fixture missing")?;
    let (pid, exit, stdout, stderr) = shared::execute_during(command, |pid, check| {
        let path = protocol.join("ready.json");
        if matches!(fault, "paused-readiness" | "malformed-readiness") {
            let marker = protocol.join(if fault == "paused-readiness" {
                "publication-paused.json"
            } else {
                "publication-closed.json"
            });
            while !marker.is_file() {
                live(check)?;
                thread::sleep(Duration::from_millis(10));
            }
            let marker: Value = serde_json::from_str(&shared::bounded(&marker)?)
                .map_err(|_| "HTTP publication marker malformed")?;
            if marker["pid"] != pid {
                return Err("HTTP publication writer PID differs".into());
            }
            let final_bytes = match fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(_) => return Err("cannot inspect HTTP final publication".into()),
            };
            let mut acknowledged = false;
            if fault == "paused-readiness" {
                let opened = if final_bytes.is_some() {
                    path.clone()
                } else {
                    protocol.join(".ready.json.tmp")
                };
                if marker["opened_path"].as_str() != opened.to_str()
                    || fs::read(&opened).map_err(|_| "HTTP paused prefix missing")? != b"{"
                {
                    return Err("HTTP publication pause is not the actual prefix write".into());
                }
                if final_bytes.is_none() {
                    fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(protocol.join("publication-ack"))
                        .map_err(|_| "cannot acknowledge HTTP publication")?;
                    acknowledged = true;
                }
            } else if marker["final_path"].as_str() != path.to_str()
                || final_bytes.as_deref() != Some(b"{\"pid\":".as_slice())
            {
                return Err("HTTP completed malformed publication differs".into());
            }
            let evidence = json!({"actual_parent_observed_pid":pid,"marker":marker,"final_present_while_paused":final_bytes.is_some(),"actual_final_bytes":final_bytes,"acknowledged":acknowledged,"closed_publication":fault == "malformed-readiness"});
            let mut file = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(pid_path.with_extension("publication.json"))
                .map_err(|_| "cannot retain HTTP publication evidence")?;
            file.write_all(&serde_json::to_vec_pretty(&evidence).unwrap())
                .map_err(|_| "cannot retain HTTP publication evidence")?;
        }
        loop {
            live(check)?;
            if path.is_file() {
                break;
            }
            thread::sleep(Duration::from_millis(10));
        }
        ready = serde_json::from_str(&shared::bounded(&path)?)
            .map_err(|_| "HTTP readiness malformed")?;
        if fault == "paused-readiness" && protocol.join(".ready.json.tmp").exists() {
            return Err("HTTP readiness temporary file remained after publication".into());
        }
        if ready["pid"] != pid || ready["host"] != "127.0.0.1" {
            return Err("HTTP server identity differs".into());
        }
        let port = ready["port"]
            .as_u64()
            .filter(|p| *p > 0 && *p <= 65535)
            .ok_or("HTTP port invalid")? as u16;
        let mut cookie = String::new();
        for case in fixture["cases"].as_array().ok_or("HTTP cases missing")? {
            let response = request(port, case, token, &cookie, check)?;
            if let Err(error) = validate_case(&response, case, token) {
                if error == "HTTP action reference differs" {
                    catalog_matches = false;
                } else {
                    return Err(error);
                }
            }
            if case["name"] == "set-cookie" {
                cookie = response["raw_headers"]["set-cookie"]
                    .as_str()
                    .ok_or("observed cookie missing")?
                    .split(';')
                    .next()
                    .unwrap()
                    .to_owned();
            }
            responses.push(response);
        }
        fs::write(protocol.join("stop"), b"stop").map_err(|_| "cannot stop HTTP oracle")?;
        Ok(())
    })?;
    if exit != Some(0) {
        return Err(format!("HTTP oracle exit differs: {exit:?}"));
    }
    let canary = fixture["canary"].as_str().unwrap();
    if stdout.contains(canary)
        || stderr.contains(canary)
        || serde_json::to_string(&responses).unwrap().contains(canary)
    {
        return Err("HTTP canary leaked in streams or response".into());
    }
    let after = shared::files(&owned, &owned, Some(canary))?;
    if before != after {
        return Err("HTTP retained owned writes differ".into());
    }
    let provenance: Value = serde_json::from_str(&shared::bounded(&observer)?)
        .map_err(|_| "HTTP provenance malformed")?;
    shared::validate_loaded_modules(&provenance, root, sources)?;
    if provenance["catalog_calls"] != 2 || provenance["modules"]["careerbrew.server"].is_null() {
        return Err("HTTP server execution provenance missing".into());
    }
    let source = root
        .join("src/careerbrew/server.py")
        .canonicalize()
        .map_err(|_| "HTTP server source missing")?;
    if ready["server_path"].as_str() != source.to_str()
        || ready["server_sha256"]
            != shared::digest(&fs::read(&source).map_err(|_| "cannot hash server")?)
    {
        return Err("HTTP served source differs".into());
    }
    let requests: Value = serde_json::from_str(&shared::bounded(&protocol.join("requests.json"))?)
        .map_err(|_| "HTTP dispatch trace missing")?;
    let requests = requests.as_array().ok_or("HTTP dispatch trace invalid")?;
    if requests.len() != responses.len() {
        return Err("HTTP handler dispatch count differs".into());
    }
    for (trace, response) in requests.iter().zip(&responses) {
        if trace["pid"] != pid
            || trace["method"] != response["method"]
            || trace["path"] != response["path"]
            || trace["source"].as_str() != source.to_str()
        {
            return Err("HTTP handler dispatch differs".into());
        }
    }
    Ok(
        json!({"pid":pid,"exit":exit,"argv":[python,"-B",helpers.join("http_driver.py")],"server":ready,"requests":requests,"stdout":stdout,"stderr":stderr,"responses":responses,"catalog_reference_matches":catalog_matches,"observer":provenance,"retained_before":before,"retained_after":after}),
    )
}

fn projection(execution: &Value) -> Value {
    json!(execution["responses"].as_array().unwrap().iter().map(|r|json!({"name":r["name"],"method":r["method"],"path":r["path"],"status":r["status"],"status_line":r["status_line"],"headers":r["headers"],"body":r["body"]})).collect::<Vec<_>>())
}

pub fn run(root: &Path, python: &Path, output: &Path) -> Result<(), String> {
    run_mode(root, python, output, "none")
}

fn run_mode(root: &Path, python: &Path, output: &Path, fault: &str) -> Result<(), String> {
    let canonical_root = root
        .canonicalize()
        .map_err(|_| "HTTP source root missing")?;
    let root = canonical_root.as_path();
    if output.exists() {
        return Err("HTTP receipt already exists".into());
    }
    if !python.is_absolute() || !python.is_file() {
        return Err("Python must be an absolute executable path".into());
    }
    let fixture: Value = serde_json::from_str(CASE).map_err(|_| "HTTP fixture invalid")?;
    if fs::read(root.join("xtask/tests/fixtures/capture/http-v1.json"))
        .map_err(|_| "HTTP fixture missing")?
        != CASE.as_bytes()
    {
        return Err("HTTP fixture changed".into());
    }
    let sources = shared::sources(root)?;
    let binary = python
        .canonicalize()
        .map_err(|_| "Python executable missing")?;
    let identity = json!({"sha256":shared::digest(&fs::read(&binary).map_err(|_| "cannot hash Python")?),"version":shared::version(python)?});
    let first = observe(
        root,
        python,
        &fixture,
        &sources,
        fault,
        &output.with_extension("pid"),
    )?;
    let second = observe(
        root,
        python,
        &fixture,
        &sources,
        fault,
        &output.with_extension("pid"),
    )?;
    if first["catalog_reference_matches"] != true || second["catalog_reference_matches"] != true {
        let completed=[&first,&second].map(|run|json!({"pid":run["pid"],"exit":run["exit"],"dispatches":run["requests"].as_array().unwrap().len(),"responses":run["responses"].as_array().unwrap().len()}));
        return Err(format!(
            "HTTP action reference differs; completed HTTP executions: {}",
            json!(completed)
        ));
    }
    if projection(&first) != projection(&second) {
        return Err("fresh HTTP observations differ".into());
    }
    if shared::digest(HISTORICAL_REFERENCE.as_bytes()) != HISTORICAL_REFERENCE_SHA256
        || fs::read(root.join("xtask/tests/fixtures/capture/http-reference-v1.json"))
            .map_err(|_| "HTTP reference missing")?
            != HISTORICAL_REFERENCE.as_bytes()
        || shared::digest(REFERENCE.as_bytes()) != REFERENCE_SHA256
        || fs::read(root.join("xtask/tests/fixtures/capture/http-reference-rules-promise-v3.json"))
            .map_err(|_| "current HTTP reference missing")?
            != REFERENCE.as_bytes()
    {
        return Err("HTTP reference changed".into());
    }
    let reference: Value = serde_json::from_str(REFERENCE).map_err(|_| "HTTP reference invalid")?;
    if projection(&first) != reference || projection(&second) != reference {
        return Err("HTTP frozen reference differs".into());
    }
    for field in [
        "external_modules",
        "site_inputs",
        "sys_path",
        "distributions",
        "prefix",
        "base_prefix",
    ] {
        if first["observer"][field] != second["observer"][field] {
            return Err("fresh HTTP dependency evidence differs".into());
        }
    }
    if sources != shared::sources(root)?
        || identity["sha256"]
            != shared::digest(&fs::read(binary).map_err(|_| "cannot rehash Python")?)
        || identity["version"] != shared::version(python)?
    {
        return Err("HTTP source or interpreter changed".into());
    }
    let mut revision = Command::new("/usr/bin/git");
    revision
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .env_clear();
    let mut source_revision = String::new();
    let (_, code, out, err) = shared::execute_during(revision, |_, _| Ok(()))?;
    if code != Some(0) || !err.is_empty() {
        return Err("HTTP revision unavailable".into());
    }
    source_revision.push_str(out.trim());
    let executable = std::env::current_exe().map_err(|_| "HTTP capture executable missing")?;
    let receipt = json!({"schema":fixture["schema"],"scope":"Python make_server handler oracle; no CLI startup or warmer qualification","qualification":"bounded-handler-contract" ,"reference_sha256":REFERENCE_SHA256,"reference_revision":"rules-promise-v3","historical_reference_sha256":HISTORICAL_REFERENCE_SHA256,"source_revision":source_revision,"source_revision_scope":"HEAD identity only; actual implementation files are bound by sources hashes","sources":sources,"python":identity,"capture_executable_sha256":shared::digest(&fs::read(executable).map_err(|_| "cannot hash HTTP capture executable")?),"fixture_sha256":shared::digest(CASE.as_bytes()),"driver_sha256":shared::digest(DRIVER.as_bytes()),"observer_sha256":shared::digest(OBSERVER.as_bytes()),"semantic_projection":projection(&first),"executions":[first,second]});
    shared::write_receipt(&receipt, output)
}
