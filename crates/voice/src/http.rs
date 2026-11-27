//! A bounded plain-HTTP JSON POST client and a scripted fixture server.
//!
//! The client is the native counterpart of `urllib.request.urlopen` for the
//! model request boundary. It speaks HTTP/1.1 over plain TCP only; `https`
//! routes are refused here (the hosted TLS transport stays Python).

use crate::models::TransportError;
use serde_json::Value;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// The response size the client reads before it stops.
pub const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

struct Target {
    host: String,
    port: u16,
    path: String,
}

fn target(url: &str) -> Result<Target, TransportError> {
    let Some(rest) = url.strip_prefix("http://") else {
        return Err(TransportError::Url {
            reason: "unsupported URL scheme for the native transport".into(),
            timeout: false,
        });
    };
    let split = rest.find('/').unwrap_or(rest.len());
    let authority = &rest[..split];
    let path = if split == rest.len() {
        "/".to_owned()
    } else {
        rest[split..].to_owned()
    };
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let close = inner
            .find(']')
            .ok_or_else(|| TransportError::Value("Invalid IPv6 URL".into()))?;
        let port = inner[close + 1..]
            .strip_prefix(':')
            .map(|port| port.parse::<u16>().map_err(|_| bad_port()))
            .transpose()?
            .unwrap_or(80);
        (inner[..close].to_owned(), port)
    } else if let Some((host, port)) = authority.rsplit_once(':') {
        (
            host.to_owned(),
            port.parse::<u16>().map_err(|_| bad_port())?,
        )
    } else {
        (authority.to_owned(), 80)
    };
    Ok(Target { host, port, path })
}

fn bad_port() -> TransportError {
    TransportError::Value("Port could not be cast to integer value".into())
}

fn io_error(error: &std::io::Error) -> TransportError {
    let timeout = matches!(
        error.kind(),
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
    );
    let reason = match error.kind() {
        std::io::ErrorKind::ConnectionRefused => "Connection refused".to_owned(),
        _ => error.kind().to_string(),
    };
    TransportError::Url { reason, timeout }
}

fn read_error(error: &std::io::Error) -> TransportError {
    match error.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => {
            TransportError::Timeout("timed out".into())
        }
        _ => TransportError::Os(error.kind().to_string()),
    }
}

/// POST one JSON body and return the reply body. A status of 400 or more is
/// `Http(code, reason)`, like urllib's `HTTPError`.
pub fn post(
    url: &str,
    body: &Value,
    authorization: Option<&str>,
    timeout: Duration,
) -> Result<Vec<u8>, TransportError> {
    let target = target(url)?;
    let address = (target.host.as_str(), target.port)
        .to_socket_addrs()
        .map_err(|error| io_error(&error))?
        .next()
        .ok_or(TransportError::Url {
            reason: "no address".into(),
            timeout: false,
        })?;
    let mut stream =
        TcpStream::connect_timeout(&address, timeout).map_err(|error| io_error(&error))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|error| io_error(&error))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|error| io_error(&error))?;
    let payload = serde_json::to_vec(body)
        .map_err(|_| TransportError::Value("payload is not JSON".into()))?;
    let host = if target.host.contains(':') {
        format!("[{}]:{}", target.host, target.port)
    } else {
        format!("{}:{}", target.host, target.port)
    };
    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        target.path,
        payload.len()
    );
    if let Some(value) = authorization {
        request.push_str(&format!("Authorization: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream
        .write_all(request.as_bytes())
        .and_then(|_| stream.write_all(&payload))
        .map_err(|error| io_error(&error))?;
    let mut raw = Vec::new();
    stream
        .take(MAX_RESPONSE_BYTES as u64)
        .read_to_end(&mut raw)
        .map_err(|error| read_error(&error))?;
    let end = raw
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .ok_or(TransportError::Os("malformed HTTP response".into()))?;
    let head = String::from_utf8_lossy(&raw[..end]).into_owned();
    let mut body = raw[end + 4..].to_vec();
    let first = head.lines().next().unwrap_or("");
    let mut parts = first.splitn(3, ' ');
    let status = parts
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
        .ok_or(TransportError::Os("malformed HTTP status".into()))?;
    let phrase = parts.next().unwrap_or("").to_owned();
    let length = head.lines().skip(1).find_map(|line| {
        let (name, value) = line.split_once(':')?;
        name.trim()
            .eq_ignore_ascii_case("content-length")
            .then(|| value.trim().parse::<usize>().ok())
            .flatten()
    });
    if let Some(length) = length {
        body.truncate(length);
    }
    if status >= 400 {
        return Err(TransportError::Http(status, phrase));
    }
    Ok(body)
}

/// One request the fixture server saw.
#[derive(Clone, Debug)]
pub struct Seen {
    pub path: String,
    pub authorization: Option<String>,
    pub content_type: Option<String>,
    pub body: Option<Value>,
}

struct Shared {
    replies: Vec<(u16, Vec<u8>)>,
    seen: Vec<Seen>,
    stop: bool,
}

/// A scripted chat-completion server on 127.0.0.1 and a free OS port.
pub struct Fixture {
    pub port: u16,
    shared: Arc<Mutex<Shared>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        404 => "Not Found",
        500 => "Internal Server Error",
        503 => "Service Unavailable",
        _ => "Status",
    }
}

fn serve_one(mut stream: TcpStream, shared: &Arc<Mutex<Shared>>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut raw = Vec::new();
    let mut buffer = [0u8; 4096];
    let header_end = loop {
        if let Some(at) = raw.windows(4).position(|window| window == b"\r\n\r\n") {
            break at;
        }
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => return,
            Ok(count) => raw.extend_from_slice(&buffer[..count]),
        }
    };
    let head = String::from_utf8_lossy(&raw[..header_end]).into_owned();
    let mut headers = Vec::new();
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    let header = |name: &str| {
        headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.clone())
    };
    let length = header("content-length")
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = raw[header_end + 4..].to_vec();
    while body.len() < length {
        match stream.read(&mut buffer) {
            Ok(0) | Err(_) => break,
            Ok(count) => body.extend_from_slice(&buffer[..count]),
        }
    }
    body.truncate(length);
    let path = head
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .unwrap_or("")
        .to_owned();
    let (status, reply) = {
        let mut state = shared.lock().expect("fixture lock");
        state.seen.push(Seen {
            path,
            authorization: header("authorization"),
            content_type: header("content-type"),
            body: serde_json::from_slice(&body).ok(),
        });
        if state.replies.is_empty() {
            (500, b"{}".to_vec())
        } else {
            state.replies.remove(0)
        }
    };
    let response = format!(
        "HTTP/1.0 {status} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        reason(status),
        reply.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.write_all(&reply);
    let _ = stream.shutdown(Shutdown::Both);
}

impl Fixture {
    pub fn start(replies: Vec<(u16, Vec<u8>)>) -> std::io::Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        let shared = Arc::new(Mutex::new(Shared {
            replies,
            seen: Vec::new(),
            stop: false,
        }));
        let owner = Arc::clone(&shared);
        let thread = std::thread::spawn(move || {
            for stream in listener.incoming() {
                if owner.lock().expect("fixture lock").stop {
                    break;
                }
                if let Ok(stream) = stream {
                    serve_one(stream, &owner);
                }
            }
        });
        Ok(Self {
            port,
            shared,
            thread: Some(thread),
        })
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/v1", self.port)
    }

    pub fn seen(&self) -> Vec<Seen> {
        self.shared.lock().expect("fixture lock").seen.clone()
    }

    pub fn close(&mut self) {
        self.shared.lock().expect("fixture lock").stop = true;
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.close();
    }
}

/// A port that was free a moment ago and has nothing listening now.
pub fn closed_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}
