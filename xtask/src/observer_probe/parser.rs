use std::collections::BTreeMap;

#[derive(Debug)]
pub struct Verdict {
    pub events: usize,
}
#[derive(Debug)]
struct Call {
    name: String,
    args: Vec<String>,
    result: String,
}

fn split_args(text: &str) -> Result<Vec<String>, String> {
    let mut fields = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote = false;
    let mut escape = false;
    for (i, c) in text.char_indices() {
        if quote {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                quote = false;
            }
            continue;
        }
        match c {
            '"' => quote = true,
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return Err("unbalanced argument".into());
                }
            }
            ',' if depth == 0 => {
                fields.push(text[start..i].trim().into());
                start = i + 1;
            }
            _ => {}
        }
    }
    if quote || depth != 0 {
        return Err("truncated arguments".into());
    }
    if !text.is_empty() {
        fields.push(text[start..].trim().into());
    }
    Ok(fields)
}
fn rows(trace: &str) -> Result<Vec<Call>, String> {
    if !trace.ends_with('\n') {
        return Err("missing trace EOF newline".into());
    }
    let mut calls = Vec::new();
    let mut terminal = false;
    let mut pending: Option<String> = None;
    for raw in trace.lines() {
        if terminal {
            return Err("record after terminal".into());
        }
        if raw == "+++ exited with 0 +++" {
            if pending.is_some() {
                return Err("unfinished at exit".into());
            }
            terminal = true;
            continue;
        }
        if raw.starts_with("--- SIGCHLD ") && raw.ends_with(" ---") {
            continue;
        }
        let line = if let Some(prefix) = pending.take() {
            let name = prefix.split('(').next().ok_or("missing pending syscall")?;
            let rest = raw
                .strip_prefix(&format!("<... {name} resumed>"))
                .ok_or("unmatched resumed syscall")?;
            prefix + rest.trim_start()
        } else {
            raw.to_owned()
        };
        if let Some(prefix) = line.strip_suffix("<unfinished ...>") {
            pending = Some(prefix.trim_end().into());
            continue;
        }
        if line.contains("...") {
            return Err("truncated or unsupported trace record".into());
        }
        let begin = line.find('(').ok_or("missing syscall opening")?;
        let name = &line[..begin];
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
            return Err("invalid syscall name".into());
        }
        let (arguments, result) = line[begin + 1..]
            .rsplit_once(" = ")
            .ok_or("missing syscall result")?;
        let arguments = arguments
            .trim_end()
            .strip_suffix(')')
            .ok_or("missing syscall closing")?;
        calls.push(Call {
            name: name.into(),
            args: split_args(arguments)?,
            result: result.into(),
        });
    }
    if !terminal || pending.is_some() {
        return Err("missing successful terminal record".into());
    }
    Ok(calls)
}
fn hex(text: &str) -> String {
    format!(
        "\"{}\"",
        text.bytes()
            .map(|b| format!("\\x{b:02x}"))
            .collect::<String>()
    )
}
fn integer(call: &Call) -> Option<i64> {
    call.result
        .split_whitespace()
        .next()?
        .split('<')
        .next()?
        .parse()
        .ok()
}
fn exec(call: &Call, path: &str) -> bool {
    call.name == "execve"
        && call.args.len() == 3
        && call.args[0] == hex("/usr/local/bin/python")
        && call.args[1].starts_with(&format!("[{}, {}", hex("/usr/local/bin/python"), hex(path)))
        && call.result == "0"
}
fn marker_cwd(value: &str) -> bool {
    matches!(value, "AT_FDCWD" | r"AT_FDCWD<\x2f\x77\x6f\x72\x6b>")
}
fn fd(value: &str, path: &str) -> Option<u32> {
    let (number, suffix) = value.split_once('<')?;
    if suffix != format!("{}>", hex(path).trim_matches('"')) {
        return None;
    }
    number.parse().ok()
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Binding {
    Other,
    Stream(u32),
    Marker(usize),
    Socket(usize),
}
struct EffectExpectation {
    exec: usize,
    creation: Option<usize>,
    wait: Option<usize>,
    marker: Option<(usize, u32, usize, usize)>,
    socket: Option<(usize, u32, usize)>,
}
fn number(value: &str) -> Result<u32, String> {
    value
        .split('<')
        .next()
        .ok_or("missing descriptor")?
        .parse()
        .map_err(|_| "invalid descriptor".into())
}
fn hex_length(value: &str) -> Option<usize> {
    let bytes = value.strip_prefix('"')?.strip_suffix('"')?.as_bytes();
    let (chunks, remainder) = bytes.as_chunks::<4>();
    if !remainder.is_empty()
        || !chunks.iter().all(|chunk| {
            chunk[0] == b'\\' && chunk[1] == b'x' && chunk[2..].iter().all(u8::is_ascii_hexdigit)
        })
    {
        return None;
    }
    Some(bytes.len() / 4)
}
fn effects(calls: &[Call], expected: &EffectExpectation) -> Result<(), String> {
    // Descriptor bindings have one traced-PID scan scope. Only constructors grant them.
    let mut bindings: BTreeMap<u32, (Binding, bool)> = [
        (0, (Binding::Other, false)),
        (1, (Binding::Stream(1), false)),
        (2, (Binding::Stream(2), false)),
    ]
    .into();
    for (index, call) in calls.iter().enumerate() {
        match call.name.as_str() {
            "execve" => {
                if index != expected.exec {
                    return Err("unexpected execution effect".into());
                }
                bindings.retain(|_, (_, cloexec)| !*cloexec);
            }
            "openat" => {
                let marker = expected
                    .marker
                    .is_some_and(|(opened, _, _, _)| opened == index);
                let flags = call.args.get(2).ok_or("missing open flags")?;
                if !marker
                    && !flags.split('|').all(|flag| {
                        matches!(
                            flag,
                            "O_RDONLY"
                                | "O_CLOEXEC"
                                | "O_DIRECTORY"
                                | "O_NOFOLLOW"
                                | "O_NONBLOCK"
                                | "O_LARGEFILE"
                                | "O_PATH"
                                | "O_NOATIME"
                        )
                    })
                {
                    return Err("unexpected or unsupported file mutation".into());
                }
                if integer(call).is_some_and(|fd| fd >= 0) {
                    let fd = number(&call.result)?;
                    bindings.insert(
                        fd,
                        (
                            if marker {
                                Binding::Marker(index)
                            } else {
                                Binding::Other
                            },
                            flags.contains("O_CLOEXEC"),
                        ),
                    );
                }
            }
            "write" => {
                if call.args.len() != 3 {
                    return Err("malformed write".into());
                }
                let descriptor = number(&call.args[0])?;
                if let Some((opened, fd, written, _)) = expected
                    .marker
                    .filter(|(_, _, written, _)| *written == index)
                {
                    if descriptor != fd
                        || bindings.get(&fd).map(|(kind, _)| *kind) != Some(Binding::Marker(opened))
                    {
                        return Err("marker descriptor was closed or overwritten".into());
                    }
                    let _ = written;
                } else {
                    if ![1, 2].contains(&descriptor)
                        || bindings.get(&descriptor).map(|(kind, _)| *kind)
                            != Some(Binding::Stream(descriptor))
                    {
                        return Err("unexpected file write".into());
                    }
                    let path = if descriptor == 1 {
                        "/work/stdout"
                    } else {
                        "/work/stderr"
                    };
                    let length = hex_length(&call.args[1]).ok_or("unsupported stream encoding")?;
                    if fd(&call.args[0], path) != Some(descriptor)
                        || call.args[2] != length.to_string()
                        || call.result != length.to_string()
                    {
                        return Err("stream write was not a complete declared output".into());
                    }
                }
            }
            "unlinkat" => {
                if !expected
                    .marker
                    .is_some_and(|(_, _, _, unlinked)| unlinked == index)
                {
                    return Err("unexpected unlink effect".into());
                }
            }
            "socket" => {
                if !expected
                    .socket
                    .is_some_and(|(created, _, _)| created == index)
                    || !integer(call).is_some_and(|fd| fd >= 0)
                {
                    return Err("unexpected or failed socket constructor".into());
                }
                bindings.insert(
                    number(&call.result)?,
                    (
                        Binding::Socket(index),
                        call.args[1].contains("SOCK_CLOEXEC"),
                    ),
                );
            }
            "connect" => {
                let (created, fd, connected) =
                    expected.socket.ok_or("unexpected network attempt")?;
                if connected != index
                    || bindings.get(&fd).map(|(kind, _)| *kind) != Some(Binding::Socket(created))
                {
                    return Err("unexpected network attempt or dead socket descriptor".into());
                }
            }
            "close" => {
                if call.args.len() != 1 || !matches!(integer(call), Some(0) | Some(-1)) {
                    return Err("malformed close transition".into());
                }
                let descriptor = number(call.args.first().ok_or("missing close descriptor")?)?;
                bindings.remove(&descriptor);
            }
            "close_range" => {
                if call.args.len() != 3 {
                    return Err("malformed close_range".into());
                }
                let first = number(&call.args[0])?;
                let last = if call.args[1] == "~0U" {
                    u32::MAX
                } else {
                    number(&call.args[1])?
                };
                let flags = &call.args[2];
                if !flags
                    .split('|')
                    .all(|flag| matches!(flag, "0" | "CLOSE_RANGE_UNSHARE" | "CLOSE_RANGE_CLOEXEC"))
                {
                    return Err("unsupported close_range transition".into());
                }
                if flags.contains("CLOSE_RANGE_CLOEXEC") {
                    for (fd, (_, cloexec)) in &mut bindings {
                        if *fd >= first && *fd <= last {
                            *cloexec = true;
                        }
                    }
                } else {
                    bindings.retain(|fd, _| *fd < first || *fd > last);
                }
            }
            "dup" | "dup2" | "dup3" => {
                let arity = match call.name.as_str() {
                    "dup" => 1,
                    "dup2" => 2,
                    _ => 3,
                };
                if call.args.len() != arity {
                    return Err("malformed descriptor duplication".into());
                }
                if call.name == "dup3" && !matches!(call.args[2].as_str(), "0" | "O_CLOEXEC") {
                    return Err("unsupported dup3 flags".into());
                }
                let from = number(&call.args[0])?;
                let target = if arity > 1 {
                    Some(number(&call.args[1])?)
                } else {
                    None
                };
                let result = integer(call).ok_or("unsupported duplication result")?;
                if result >= 0 {
                    let to = number(&call.result)?;
                    if target.is_some_and(|target| to != target)
                        || ((call.name == "dup" || call.name == "dup3") && to == from)
                        || (call.name == "dup" && bindings.contains_key(&to))
                    {
                        return Err(
                            "duplication result differs from a valid requested target".into()
                        );
                    }
                    let (kind, _) = bindings
                        .get(&from)
                        .copied()
                        .unwrap_or((Binding::Other, false));
                    bindings.insert(
                        to,
                        (kind, call.name == "dup3" && call.args[2] == "O_CLOEXEC"),
                    );
                } else if let Some(target) = target {
                    bindings.remove(&target);
                }
            }
            "fcntl" => {
                let from = number(call.args.first().ok_or("missing fcntl descriptor")?)?;
                let command = call.args.get(1).ok_or("missing fcntl command")?.as_str();
                let arity = if matches!(command, "F_GETFD" | "F_GETFL") {
                    2
                } else {
                    3
                };
                if call.args.len() != arity {
                    return Err("malformed fcntl transition".into());
                }
                match command {
                    "F_DUPFD" | "F_DUPFD_CLOEXEC" => {
                        if integer(call).is_some_and(|fd| fd >= 0) {
                            let target = number(&call.result)?;
                            let minimum = number(&call.args[2])?;
                            if target < minimum || target == from || bindings.contains_key(&target)
                            {
                                return Err("fcntl duplication returned an invalid target".into());
                            }
                            let kind = bindings
                                .get(&from)
                                .map(|(kind, _)| *kind)
                                .unwrap_or(Binding::Other);
                            bindings.insert(
                                number(&call.result)?,
                                (kind, call.args[1] == "F_DUPFD_CLOEXEC"),
                            );
                        }
                    }
                    "F_GETFD" | "F_GETFL" | "F_SETFL" => {}
                    "F_SETFD" => {
                        let flags = call.args.get(2).ok_or("missing descriptor flags")?;
                        if flags != "0" && flags != "FD_CLOEXEC" {
                            return Err("unsupported descriptor flag transition".into());
                        }
                        if let Some((_, cloexec)) = bindings.get_mut(&from) {
                            *cloexec = flags == "FD_CLOEXEC";
                        }
                    }
                    _ => return Err("unmodeled fcntl transition".into()),
                }
            }
            "clone" | "clone3" | "fork" | "vfork" => {
                let created = expected.creation.ok_or("unexpected child creation")?;
                if integer(call).is_some_and(|pid| pid > 0) && created != index {
                    return Err("extra successful child creation".into());
                }
            }
            "wait4" => {
                let created = expected.creation.ok_or("unexpected wait")?;
                if index <= created {
                    return Err("wait before child creation".into());
                }
                if integer(call).is_some_and(|pid| pid > 0) && expected.wait != Some(index) {
                    return Err("unexpected child reap".into());
                }
            }
            "exit" | "exit_group" => {
                if call.args != ["0"] || call.result != "?" {
                    return Err("unexpected exit effect".into());
                }
            }
            "getsockname" | "getpeername" | "getsockopt" => {
                let descriptor =
                    number(call.args.first().ok_or("missing socket query descriptor")?)?;
                if !matches!(bindings.get(&descriptor), Some((Binding::Socket(_), _))) {
                    return Err("socket query has no live constructor".into());
                }
            }
            "access" | "faccessat" | "faccessat2" | "stat" | "stat64" | "lstat" | "lstat64"
            | "newfstatat" | "fstat" | "fstat64" | "statx" | "readlink" | "readlinkat"
            | "getcwd" | "statfs" | "statfs64" | "fstatfs" | "fstatfs64" | "getxattr"
            | "lgetxattr" | "listxattr" | "llistxattr" | "getpid" | "getppid" | "gettid"
            | "getuid" | "geteuid" | "getgid" | "getegid" => {}
            _ => {
                return Err(format!(
                    "unexpected or unsupported selected effect: {}",
                    call.name
                ))
            }
        }
    }
    Ok(())
}

pub enum Observation {
    Complete(Verdict),
    ExpectedMissingChild { parent_pid: u32, child_pid: u32 },
}

#[cfg(test)]
pub fn verify(traces: &BTreeMap<u32, String>) -> Result<Verdict, String> {
    match observe(traces)? {
        Observation::Complete(verdict) => Ok(verdict),
        Observation::ExpectedMissingChild {
            parent_pid,
            child_pid,
        } => Err(format!(
            "missing child trace {child_pid} from parent {parent_pid}"
        )),
    }
}

pub fn observe(traces: &BTreeMap<u32, String>) -> Result<Observation, String> {
    if traces.is_empty()
        || traces.contains_key(&0)
        || traces.values().map(String::len).sum::<usize>() > 8 * 1024 * 1024
    {
        return Err("missing or oversized trace".into());
    }
    let parsed = traces
        .iter()
        .map(|(pid, text)| Ok((*pid, rows(text)?)))
        .collect::<Result<BTreeMap<_, _>, String>>()?;
    let parents = parsed
        .iter()
        .filter(|(_, calls)| calls.iter().any(|c| exec(c, "/probe/parent.py")))
        .collect::<Vec<_>>();
    if parents.len() != 1 {
        return Err("parent execution not unique".into());
    }
    let (parent_pid, parent) = parents[0];
    let parent_exec = parent
        .iter()
        .position(|call| exec(call, "/probe/parent.py"))
        .ok_or("missing parent exec")?;
    let children = parent
        .iter()
        .filter(|c| matches!(c.name.as_str(), "clone" | "clone3" | "fork" | "vfork"))
        .filter_map(integer)
        .filter(|p| *p > 0)
        .collect::<Vec<_>>();
    if children.len() != 1 {
        return Err("child relation not unique".into());
    }
    let child_pid = u32::try_from(children[0]).map_err(|_| "invalid child PID")?;
    let creation = parent
        .iter()
        .position(|call| {
            matches!(call.name.as_str(), "clone" | "clone3" | "fork" | "vfork")
                && call.result == child_pid.to_string()
        })
        .ok_or("missing exact child creation result")?;
    if child_pid == *parent_pid {
        return Err("unexpected process tree".into());
    }
    let waited = parent
        .iter()
        .position(|c| {
            c.name == "wait4"
                && c.args
                    == [
                        child_pid.to_string(),
                        "[{WIFEXITED(s) && WEXITSTATUS(s) == 0}]".into(),
                        "0".into(),
                        "NULL".into(),
                    ]
                && c.result == child_pid.to_string()
        })
        .ok_or("missing actual child wait zero")?;
    if !(parent_exec < creation && creation < waited) {
        return Err("parent execution, creation and wait are out of causal order".into());
    }
    effects(
        parent,
        &EffectExpectation {
            exec: parent_exec,
            creation: Some(creation),
            wait: Some(waited),
            marker: None,
            socket: None,
        },
    )?;
    if !parsed.contains_key(&child_pid) {
        if parsed.len() != 1 {
            return Err("unexpected process tree".into());
        }
        return Ok(Observation::ExpectedMissingChild {
            parent_pid: *parent_pid,
            child_pid,
        });
    }
    let child = parsed.get(&child_pid).ok_or("missing child trace")?;
    if parsed.len() != 2 {
        return Err("unexpected process tree".into());
    }
    let exec_index = child
        .iter()
        .position(|c| exec(c, "/probe/child.py"))
        .ok_or("missing successful child exec")?;
    let opened = child
        .iter()
        .enumerate()
        .find_map(|(i, c)| {
            if i <= exec_index
                || c.name != "openat"
                || c.args.len() != 4
                || !marker_cwd(&c.args[0])
                || c.args[1] != hex("/work/transient")
                || c.args[2] != "O_WRONLY|O_CREAT|O_EXCL|O_CLOEXEC"
                || c.args[3] != "0600"
            {
                return None;
            }
            Some((i, fd(&c.result, "/work/transient")?))
        })
        .ok_or("missing child open")?;
    let marker = "CAREERBREW-TRANSIENT-V1\n";
    let write = child
        .iter()
        .enumerate()
        .find(|(i, c)| {
            *i > opened.0
                && c.name == "write"
                && c.args.len() == 3
                && fd(&c.args[0], "/work/transient") == Some(opened.1)
                && c.args[1] == hex(marker)
                && c.args[2] == marker.len().to_string()
                && c.result == marker.len().to_string()
        })
        .ok_or("missing exact child write")?;
    let unlink = child
        .iter()
        .enumerate()
        .find(|(i, c)| {
            *i > write.0
                && c.name == "unlinkat"
                && c.args.len() == 3
                && marker_cwd(&c.args[0])
                && c.args[1] == hex("/work/transient")
                && c.args[2] == "0"
                && c.result == "0"
        })
        .ok_or("missing same-child unlink")?;
    let socket = child
        .iter()
        .enumerate()
        .find_map(|(i, c)| {
            if i <= unlink.0
                || c.name != "socket"
                || c.args != ["AF_INET", "SOCK_STREAM|SOCK_CLOEXEC", "IPPROTO_IP"]
            {
                return None;
            }
            let descriptor = integer(c)?;
            if descriptor < 0 {
                return None;
            }
            Some((i, descriptor))
        })
        .ok_or("missing typed child socket")?;
    let address = format!(
        "{{sa_family=AF_INET, sin_port=htons(9), sin_addr=inet_addr({})}}",
        hex("127.0.0.1")
    );
    let connected = child
        .iter()
        .enumerate()
        .find(|(i, c)| {
            *i > socket.0
                && c.name == "connect"
                && c.args.len() == 3
                && c.args[0].split('<').next() == Some(socket.1.to_string().as_str())
                && c.args[0].ends_with('>')
                && c.args[1] == address
                && c.args[2] == "16"
                && c.result == "-1 ECONNREFUSED (Connection refused)"
        })
        .ok_or("missing typed loopback refusal")?
        .0;

    effects(
        child,
        &EffectExpectation {
            exec: exec_index,
            creation: None,
            wait: None,
            marker: Some((opened.0, opened.1, write.0, unlink.0)),
            socket: Some((
                socket.0,
                u32::try_from(socket.1).map_err(|_| "invalid socket descriptor")?,
                connected,
            )),
        },
    )?;

    Ok(Observation::Complete(Verdict { events: 5 }))
}
