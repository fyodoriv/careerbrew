//! A small HTTP/1.0 request reader and response writer with the semantics of
//! Python's `BaseHTTPRequestHandler`: one request per connection, headers read
//! as latin-1, the first header of a name wins, and the connection closes after
//! the answer.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;

/// One header line may hold at most this many bytes (Python's `_MAXLINE`).
pub const MAX_LINE: usize = 65_536;
/// At most this many headers (Python's `_MAXHEADERS`).
pub const MAX_HEADERS: usize = 100;

pub struct Request {
    pub method: String,
    /// The raw request target, exactly as the client sent it.
    pub target: String,
    pub headers: Vec<(String, String)>,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    /// `urlsplit(target).path`.
    pub fn path(&self) -> &str {
        let t = self.target.as_str();
        let t = t.split_once('#').map_or(t, |(a, _)| a);
        t.split_once('?').map_or(t, |(a, _)| a)
    }
}

fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

fn line(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<Vec<u8>>> {
    let mut buf = Vec::new();
    let n = reader
        .by_ref()
        .take(MAX_LINE as u64 + 1)
        .read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    if buf.len() > MAX_LINE {
        return Err(std::io::Error::other("line too long"));
    }
    while buf.last().is_some_and(|b| *b == b'\n' || *b == b'\r') {
        buf.pop();
    }
    Ok(Some(buf))
}

/// Read the request line and headers. `Ok(None)`: the client sent nothing.
pub fn read_request(reader: &mut BufReader<TcpStream>) -> std::io::Result<Option<Request>> {
    let Some(first) = line(reader)? else {
        return Ok(None);
    };
    let first = latin1(&first);
    let words = first.split_whitespace().collect::<Vec<_>>();
    if words.len() != 3 || !words[2].starts_with("HTTP/") {
        return Err(std::io::Error::other("bad request line"));
    }
    let mut headers = Vec::new();
    while let Some(raw) = line(reader)? {
        if raw.is_empty() {
            break;
        }
        if headers.len() >= MAX_HEADERS {
            return Err(std::io::Error::other("too many headers"));
        }
        let text = latin1(&raw);
        if let Some((name, value)) = text.split_once(':') {
            headers.push((
                name.to_owned(),
                value.trim_start_matches([' ', '\t']).to_owned(),
            ));
        }
    }
    Ok(Some(Request {
        method: words[0].to_owned(),
        target: words[1].to_owned(),
        headers,
    }))
}

pub fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        413 => "Request Entity Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        _ => "",
    }
}

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

pub fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let mut out = format!(
        "HTTP/1.0 {} {}\r\nServer: careerbrew-web\r\nDate: {}\r\n",
        response.status,
        reason(response.status),
        chrono::Utc::now().format("%a, %d %b %Y %H:%M:%S GMT")
    );
    for (name, value) in &response.headers {
        out.push_str(name);
        out.push_str(": ");
        out.push_str(value);
        out.push_str("\r\n");
    }
    out.push_str("\r\n");
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(&response.body);
    stream.write_all(&bytes)?;
    stream.flush()
}
