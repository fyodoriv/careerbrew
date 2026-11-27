//! The native localhost front (rust-25).
//!
//! Native here: the host check, the token and its sign-in cookie
//! (`/api/session`), the corporate job-path guard, the response headers (CSP
//! and the rest), the error envelopes, `/api/changes`, `/api/tab-key`, unknown
//! GET routes and the built web files. Every other route is forwarded to the
//! Python handler (`careerbrew serve` on a private port) through `ROUTES`,
//! which names the migration step that owns each one. The Python handler keeps
//! its response cache, its warmer and its POST cache clears for the routes it
//! answers, so cached views behave exactly as before.

use crate::http::{self, Request, Response};
use crate::json::{self, J};
use crate::token::{self, TokenSource};
use crate::version;
use careerbrew_vault::canon::Canon;
use careerbrew_vault::transfer::Context;
use chrono::{DateTime, FixedOffset};
use std::io::{BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::FromRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// The listen backlog. Python's stdlib default of 5 reset the extra
/// connections of a Dashboard burst (AGENTS.md Gotcha, issue #142).
pub const LISTEN_BACKLOG: i32 = 128;
pub const SIGN_IN: &str = "fix: run `careerbrew open` in a terminal";
pub const CSP: &str = "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'none'; form-action 'self'";
pub const JOB_PATHS: &[&str] = &[
    "/api/jobs",
    "/api/interviews",
    "/api/cards",
    "/api/library",
    "/api/explanations",
    "/api/exercises",
    "/api/drill",
];
pub const VOICE_MAX_BYTES: i64 = 12_000_000;
pub const POST_MAX_BYTES: i64 = 64_000;

/// A session answer (body and extra headers) or an API error, or a bad-request text.
type SessionResult = Result<(J, Vec<(String, String)>), Result<ApiError, String>>;

/// Who answers a route: this server, or the Python handler until the named
/// step ports the route's domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Native,
    Python(&'static str),
}

pub struct Route {
    pub method: &'static str,
    pub path: &'static str,
    /// A prefix route (`/api/focus/<block>`); otherwise the path is exact.
    pub prefix: bool,
    pub owner: Owner,
}

const fn get(path: &'static str, owner: Owner) -> Route {
    Route {
        method: "GET",
        path,
        prefix: false,
        owner,
    }
}
const fn get_prefix(path: &'static str, owner: Owner) -> Route {
    Route {
        method: "GET",
        path,
        prefix: true,
        owner,
    }
}
const fn post(path: &'static str, owner: Owner) -> Route {
    Route {
        method: "POST",
        path,
        prefix: false,
        owner,
    }
}
const fn post_prefix(path: &'static str, owner: Owner) -> Route {
    Route {
        method: "POST",
        path,
        prefix: true,
        owner,
    }
}

use Owner::{Native, Python};

/// Every API route of `server.py`, with its owner. `/api/session*` and the
/// static files are handled before this table, natively.
pub const ROUTES: &[Route] = &[
    get("/api/changes", Native),
    get("/api/tab-key", Native),
    // The warm state lives in the Python response cache that holds the cached views.
    get("/api/ping", Python("rust-25")),
    get("/api/overview", Python("rust-17")),
    get("/api/today", Python("rust-17")),
    get("/api/week", Python("rust-17")),
    get("/api/daylog", Python("rust-17")),
    get_prefix("/api/daylog/", Python("rust-17")),
    get("/api/inbox", Python("rust-17")),
    get("/api/jobs", Python("rust-19")),
    get("/api/interviews", Python("rust-19")),
    get_prefix("/api/interviews/", Python("rust-19")),
    get("/api/work", Python("rust-18")),
    get("/api/standup", Python("rust-18")),
    get("/api/priority", Python("rust-09")),
    get("/api/focus", Python("rust-09")),
    get_prefix("/api/focus/", Python("rust-09")),
    get("/api/promises", Python("rust-09")),
    get("/api/agents", Python("rust-10")),
    get("/api/feed/asks", Python("rust-13")),
    get("/api/feed/health", Python("rust-14")),
    get("/api/notifications", Python("rust-14")),
    get("/api/checklist", Python("rust-24")),
    get("/api/review", Python("rust-24")),
    get("/api/reports", Python("rust-24")),
    get_prefix("/api/reports/", Python("rust-24")),
    get("/api/time-saved", Python("rust-24")),
    get("/api/config", Python("rust-02")),
    get("/api/actions", Python("rust-26")),
    get("/api/setup", Python("rust-07")),
    get("/api/cards", Python("rust-20")),
    get("/api/library", Python("rust-20")),
    get_prefix("/api/library/", Python("rust-20")),
    get("/api/teach", Python("rust-21")),
    get("/api/teach/improvements", Python("rust-21")),
    get("/api/drill", Python("rust-21")),
    get("/api/goals", Python("rust-21")),
    get("/api/mistakes", Python("rust-21")),
    get("/api/exercises", Python("rust-22")),
    post_prefix("/api/inbox/", Python("rust-17")),
    post_prefix("/api/inbox-group/", Python("rust-17")),
    post("/api/pm/draft", Python("rust-17")),
    post("/api/pm/send", Python("rust-17")),
    post_prefix("/api/watch/", Python("rust-17")),
    post_prefix("/api/notifications/", Python("rust-14")),
    post_prefix("/api/promises/", Python("rust-09")),
    post("/api/tasks", Python("rust-09")),
    post_prefix("/api/tasks/", Python("rust-09")),
    post_prefix("/api/actions/", Python("rust-26")),
    post("/api/timing", Python("rust-24")),
    post("/api/config", Python("rust-02")),
    post("/api/drill/voice", Python("rust-23")),
    post("/api/teach/complete", Python("rust-21")),
    post("/api/teach/draft", Python("rust-21")),
    post("/api/teach/feedback", Python("rust-21")),
    post("/api/goals", Python("rust-21")),
    post("/api/jobs/choose", Python("rust-19")),
    post("/api/jobs/unchoose", Python("rust-19")),
    post_prefix("/api/approvals/", Python("rust-04")),
    post_prefix("/api/cards/", Python("rust-20")),
    post_prefix("/api/explanations/", Python("rust-20")),
];

/// The owner of `method path`: an exact route first, then the longest prefix.
/// An unknown POST still goes to Python, which checks the body before its 404.
pub fn owner(method: &str, path: &str) -> Option<Owner> {
    let mut best: Option<&Route> = None;
    for route in ROUTES.iter().filter(|r| r.method == method) {
        if !route.prefix && route.path == path {
            return Some(route.owner);
        }
        if route.prefix
            && path.starts_with(route.path)
            && best.is_none_or(|b| route.path.len() > b.path.len())
        {
            best = Some(route);
        }
    }
    best.map(|r| r.owner)
        .or_else(|| (method == "POST").then_some(Python("rust-25")))
}

struct ApiError {
    status: u16,
    message: String,
}

fn api_error(status: u16, message: impl Into<String>) -> ApiError {
    ApiError {
        status,
        message: message.into(),
    }
}

/// Server-instance state. Lifecycle: one per `Server`; it goes away with it.
pub struct State {
    pub ctx: Context,
    pub port: u16,
    pub tokens: TokenSource,
    /// The key the page encrypts its per-tab view copies with: random per
    /// server, held only in this process's memory.
    pub tab_key: String,
    pub dist: PathBuf,
    /// The Python handler's private port; 0 while none answers.
    pub backend: AtomicU16,
}

fn random_bytes(n: usize) -> Vec<u8> {
    let mut buf = vec![0u8; n];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("the system random source is readable");
    buf
}

pub fn token_urlsafe() -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(random_bytes(32))
}

pub fn new_tab_key() -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(random_bytes(32))
}

struct EventClock;
impl careerbrew_vault::events::Clock for EventClock {
    fn stamp(&self) -> String {
        // Python events.stamp() reads the wall clock, not CAREERBREW_NOW.
        chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
    }
    fn version(&self) -> String {
        std::env::var("CAREERBREW_VERSION").unwrap_or_else(|_| "0.1.0".into())
    }
}

/// `events.emit`: content-free fields only; a log line never fails a request.
pub fn emit(root: &Path, event: &str, fields: Vec<(&str, Canon)>) {
    let fields = fields
        .into_iter()
        .map(|(k, v)| (Canon::Str(k.into()), v))
        .collect::<Vec<_>>();
    let _ = careerbrew_vault::events::emit(
        root,
        std::env::var("CAREERBREW_EVENTS").ok().as_deref(),
        event,
        &Canon::Map(fields),
        &EventClock,
    );
}

fn int(v: i64) -> Canon {
    Canon::Int(v.into())
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

fn headers_for(ctype: &str, len: usize, extra: &[(String, String)]) -> Vec<(String, String)> {
    let mut h = vec![
        ("Content-Type".to_owned(), ctype.to_owned()),
        ("Content-Length".to_owned(), len.to_string()),
    ];
    h.extend(extra.iter().cloned());
    h.extend(
        [
            ("X-Content-Type-Options", "nosniff"),
            ("Referrer-Policy", "no-referrer"),
            ("Content-Security-Policy", CSP),
            ("Cross-Origin-Resource-Policy", "same-origin"),
            ("Cache-Control", "no-store"),
        ]
        .map(|(k, v)| (k.to_owned(), v.to_owned())),
    );
    h
}

/// `Handler._send`: every answer carries the same security headers.
pub fn send(status: u16, body: Vec<u8>, ctype: &str, extra: &[(String, String)]) -> Response {
    Response {
        status,
        headers: headers_for(ctype, body.len(), extra),
        body,
    }
}

pub fn send_json(status: u16, data: &J, extra: &[(String, String)]) -> Response {
    send(
        status,
        json::dumps(data).into_bytes(),
        "application/json",
        extra,
    )
}

/// `_route`: the event-log name; a path parameter is content and never logged.
pub fn route_name(path: &str) -> String {
    for (prefix, name) in [
        ("/api/library/", "library.page"),
        ("/api/daylog/", "daylog.day"),
        ("/api/interviews/", "interviews.company"),
        ("/api/reports/", "reports.page"),
    ] {
        if path.starts_with(prefix) {
            return name.into();
        }
    }
    let name = path.strip_prefix("/api/").unwrap_or(path).replace('/', ".");
    let name = name.chars().take(48).collect::<String>();
    if name.is_empty() {
        "api".into()
    } else {
        name
    }
}

/// Python `repr(str)` for the short header values an int() error quotes.
fn py_repr(text: &str) -> String {
    let quote = if text.contains('\'') && !text.contains('"') {
        '"'
    } else {
        '\''
    };
    let mut out = String::from(quote);
    for ch in text.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c == quote => {
                out.push('\\');
                out.push(c);
            }
            c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                out.push_str(&format!("\\x{:02x}", c as u32))
            }
            c => out.push(c),
        }
    }
    out.push(quote);
    out
}

/// `int(headers.get("Content-Length", "0") or 0)`; Err holds Python's message.
fn content_length(req: &Request) -> Result<i64, String> {
    let raw = req.header("Content-Length").unwrap_or("0");
    if raw.is_empty() {
        return Ok(0);
    }
    token::py_int(raw)
        .ok_or_else(|| format!("invalid literal for int() with base 10: {}", py_repr(raw)))
}

fn bad_request(message: &str) -> Response {
    let text = format!("bad request: {message}");
    send_json(
        400,
        &json::error(text.chars().take(300).collect::<String>()),
        &[],
    )
}

impl State {
    pub fn new(ctx: Context, port: u16, pinned: Option<String>, dist: PathBuf) -> Self {
        Self {
            ctx,
            port,
            tokens: TokenSource::new(pinned),
            tab_key: new_tab_key(),
            dist,
            backend: AtomicU16::new(0),
        }
    }

    fn root(&self) -> &Path {
        &self.ctx.root
    }

    fn host_ok(&self, req: &Request) -> bool {
        let host = req.header("Host").unwrap_or("");
        host == format!("127.0.0.1:{}", self.port) || host == format!("localhost:{}", self.port)
    }

    fn check_token(&self, given: &str) -> Result<Option<DateTime<FixedOffset>>, ApiError> {
        let (tok, expires) = self.tokens.current(&self.ctx);
        if tok.is_empty() || !ct_eq(given.as_bytes(), tok.as_bytes()) {
            return Err(api_error(
                401,
                format!("missing or wrong token ({SIGN_IN})"),
            ));
        }
        if let Some(at) = expires {
            if token::now(&self.ctx) >= at {
                return Err(api_error(
                    401,
                    format!("the sign-in expired after web_token_max_days ({SIGN_IN})"),
                ));
            }
        }
        Ok(expires)
    }

    fn corporate(&self) -> bool {
        // Fail closed: an unreadable machine record keeps job-hunt views shut.
        !matches!(self.ctx.role(), Ok(careerbrew_vault::sync::Role::Personal))
    }

    fn guard(&self, req: &Request, path: &str) -> Result<(), ApiError> {
        if !self.host_ok(req) {
            return Err(api_error(403, "bad host"));
        }
        self.check_token(req.header("X-Careerbrew-Token").unwrap_or(""))?;
        if JOB_PATHS.iter().any(|p| path.starts_with(p)) && self.corporate() {
            return Err(api_error(
                403,
                "job-hunt views are personal-only; this is a corporate machine",
            ));
        }
        Ok(())
    }

    fn own_page(req: &Request, required: bool) -> bool {
        match req.header("Sec-Fetch-Site") {
            Some(site) => site == "same-origin",
            None => !required,
        }
    }

    fn cookie_name(&self) -> String {
        format!("careerbrew-{}", self.port)
    }

    fn cookie_token(&self, req: &Request) -> String {
        let name = self.cookie_name();
        for part in req.header("Cookie").unwrap_or("").split(';') {
            let part = part.trim();
            let (k, v) = part.split_once('=').unwrap_or((part, ""));
            if k == name {
                return v.trim().to_owned();
            }
        }
        String::new()
    }

    fn set_cookie(
        &self,
        req: &Request,
        expires: Option<DateTime<FixedOffset>>,
    ) -> (J, Vec<(String, String)>) {
        let left = match expires {
            Some(at) => (at - token::now(&self.ctx)).num_seconds(),
            None => token::token_days(&self.ctx) * 86_400,
        };
        let days_left = left.div_euclid(86_400);
        let body = |cookie: bool| {
            json::obj(vec![
                ("signed_in", J::Bool(true)),
                ("days_left", J::Int(days_left)),
                ("cookie", J::Bool(cookie)),
            ])
        };
        if req.header("Host") != Some(format!("127.0.0.1:{}", self.port).as_str()) {
            return (body(false), Vec::new());
        }
        let cookie = format!(
            "{}={}; Max-Age={}; Path=/; HttpOnly; SameSite=Strict",
            self.cookie_name(),
            self.tokens.current(&self.ctx).0,
            left.max(0)
        );
        (body(true), vec![("Set-Cookie".to_owned(), cookie)])
    }

    /// `web_token()`: the stable token, made when missing or expired, under `day/web.lock`.
    fn web_token(&self) -> String {
        let root = self.root();
        let _ = std::fs::create_dir_all(root.join("day"));
        let lock = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("day").join("web.lock"));
        let fd = lock.as_ref().map(std::os::fd::AsRawFd::as_raw_fd).ok();
        if let Some(fd) = fd {
            // SAFETY: flock on an open descriptor this function owns.
            unsafe { libc::flock(fd, libc::LOCK_EX) };
        }
        let reason = match token::stored_token(&self.ctx) {
            Some((tok, created)) if token::now(&self.ctx) < token::expires(&self.ctx, created) => {
                return tok
            }
            Some(_) => "age",
            None => "first",
        };
        let tok = token_urlsafe();
        let created = token::now(&self.ctx)
            .format("%Y-%m-%dT%H:%M:%S%:z")
            .to_string();
        let record = Canon::Map(vec![
            (Canon::Str("token".into()), Canon::Str(tok.clone())),
            (Canon::Str("created".into()), Canon::Str(created)),
        ]);
        let _ = self
            .ctx
            .store()
            .write_json(&token::web_token_path(root), &record);
        emit(
            root,
            "web.token",
            vec![("status", Canon::Str(reason.into()))],
        );
        tok
    }

    fn open_tab(&self, req: &Request, raw_body: &[u8]) -> Result<u16, Result<ApiError, String>> {
        let length = content_length(req).map_err(Err)?;
        if !(0..=1000).contains(&length) {
            return Err(Ok(api_error(400, "Content-Length must be 0 to 1000")));
        }
        let body: serde_json::Value = if raw_body.is_empty() {
            serde_json::Value::Object(Default::default())
        } else {
            serde_json::from_slice(raw_body)
                .map_err(|_| Err("the body is not valid JSON".into()))?
        };
        let view = body
            .get("view")
            .and_then(serde_json::Value::as_str)
            .filter(|v| (1..=20).contains(&v.len()) && v.bytes().all(|b| b.is_ascii_lowercase()))
            .unwrap_or("today");
        let token = if self.tokens.pinned.is_empty() {
            self.web_token()
        } else {
            self.tokens.pinned.clone()
        };
        if !open_browser(&format!(
            "http://127.0.0.1:{}/#/{view}&token={token}",
            self.port
        )) {
            return Err(Ok(api_error(
                409,
                format!("could not open a browser from the web app here ({SIGN_IN})"),
            )));
        }
        Ok(200)
    }

    /// `Handler._session`.
    fn session(&self, method: &str, path: &str, req: &Request, body: &[u8]) -> Response {
        let action = match (method, path) {
            ("POST", "/api/session") => "set",
            ("GET", "/api/session") => "restore",
            ("POST", "/api/session/open") => "open",
            _ => return send_json(404, &json::error("no such endpoint"), &[]),
        };
        let had_cookie = !self.cookie_token(req).is_empty();
        let result: SessionResult = (|| {
            if !self.host_ok(req) {
                return Err(Ok(api_error(403, "bad host")));
            }
            if !Self::own_page(req, action == "open") {
                return Err(Ok(api_error(
                    403,
                    format!("only careerbrew's own pages may sign in ({SIGN_IN})"),
                )));
            }
            match action {
                "set" => {
                    let expires = self
                        .check_token(req.header("X-Careerbrew-Token").unwrap_or(""))
                        .map_err(Ok)?;
                    Ok(self.set_cookie(req, expires))
                }
                "restore" => {
                    self.check_token(&self.cookie_token(req)).map_err(Ok)?;
                    Ok((
                        json::obj(vec![("token", json::s(self.tokens.current(&self.ctx).0))]),
                        Vec::new(),
                    ))
                }
                _ => {
                    self.open_tab(req, body)?;
                    Ok((json::obj(vec![("opened", J::Bool(true))]), Vec::new()))
                }
            }
        })();
        match result {
            Ok((data, extra)) => {
                emit(
                    self.root(),
                    "web.session",
                    vec![
                        ("route", Canon::Str(action.into())),
                        ("status", Canon::Str("ok".into())),
                        ("cookie", Canon::Bool(had_cookie)),
                    ],
                );
                send_json(200, &data, &extra)
            }
            Err(Ok(e)) => {
                emit(
                    self.root(),
                    "web.session",
                    vec![
                        ("route", Canon::Str(action.into())),
                        ("status", Canon::Str("refused".into())),
                        ("code", int(i64::from(e.status))),
                        ("cookie", Canon::Bool(had_cookie)),
                    ],
                );
                send_json(e.status, &json::error(e.message), &[])
            }
            Err(Err(message)) => bad_request(&message),
        }
    }

    /// `Handler._static`: built files from the dist folder, else index.html.
    fn static_file(&self, path: &str) -> Response {
        let rel = if path.is_empty() || path == "/" {
            "index.html"
        } else {
            path.trim_start_matches('/')
        };
        let dist = std::fs::canonicalize(&self.dist).unwrap_or_else(|_| self.dist.clone());
        let file = std::fs::canonicalize(self.dist.join(rel))
            .ok()
            .filter(|f| f.starts_with(&dist) && f.is_file())
            .unwrap_or_else(|| self.dist.join("index.html"));
        match std::fs::read(&file) {
            Ok(bytes) => {
                let ctype = crate::mime::guess(&file.to_string_lossy())
                    .unwrap_or_else(|| "application/octet-stream".into());
                send(200, bytes, &ctype, &[])
            }
            Err(_) => send(
                404,
                b"web app not built: cd web && npm ci && npm run build".to_vec(),
                "text/plain",
                &[],
            ),
        }
    }

    fn native_get(&self, path: &str) -> Option<J> {
        match path {
            "/api/changes" => Some(json::obj(vec![(
                "version",
                json::s(version::files_version(self.root(), version::CHANGE_DIRS)),
            )])),
            "/api/tab-key" => Some(json::obj(vec![(
                "key",
                if token::tab_cache_on(&self.ctx) {
                    json::s(self.tab_key.clone())
                } else {
                    J::Null
                },
            )])),
            _ => None,
        }
    }

    /// One request. `Forward` asks the caller to relay it to the Python handler.
    pub fn handle(&self, req: &Request, body: &[u8]) -> Outcome {
        let path = req.path().to_owned();
        match req.method.as_str() {
            "GET" if path.starts_with("/api/session") => {
                Outcome::Answer(self.session("GET", &path, req, body))
            }
            "GET" if !path.starts_with("/api/") => Outcome::Answer(self.static_file(&path)),
            "GET" => {
                let t0 = Instant::now();
                if let Err(e) = self.guard(req, &path) {
                    return Outcome::Answer(send_json(e.status, &json::error(e.message), &[]));
                }
                match owner("GET", &path) {
                    Some(Native) => {
                        let data = self.native_get(&path).expect("native GET route");
                        emit(
                            self.root(),
                            "web.api",
                            vec![
                                ("status", Canon::Str("ok".into())),
                                ("route", Canon::Str(route_name(&path))),
                                ("cached", Canon::Bool(false)),
                                ("ms", int(t0.elapsed().as_millis() as i64)),
                            ],
                        );
                        Outcome::Answer(send_json(200, &data, &[]))
                    }
                    Some(Python(step)) => Outcome::Forward(step),
                    None => Outcome::Answer(send_json(404, &json::error("no such endpoint"), &[])),
                }
            }
            "POST" if path.starts_with("/api/session") => {
                Outcome::Answer(self.session("POST", &path, req, body))
            }
            "POST" => {
                if let Err(e) = self.guard(req, &path) {
                    return Outcome::Answer(send_json(e.status, &json::error(e.message), &[]));
                }
                match owner("POST", &path) {
                    Some(Python(step)) => Outcome::Forward(step),
                    _ => Outcome::Answer(send_json(404, &json::error("no such endpoint"), &[])),
                }
            }
            // Other methods: the Python handler's own 501 page.
            _ => Outcome::Forward("rust-25"),
        }
    }
}

pub enum Outcome {
    Answer(Response),
    Forward(&'static str),
}

/// Python `webbrowser.open`: each `BROWSER` entry is one program that gets the
/// link; then the platform opener. True when one exits 0.
pub fn open_browser(url: &str) -> bool {
    let mut programs = std::env::var("BROWSER")
        .unwrap_or_default()
        .split(':')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    programs.push(if cfg!(target_os = "macos") {
        "open".into()
    } else {
        "xdg-open".into()
    });
    programs.into_iter().any(|program| {
        std::process::Command::new(program)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

/// How many request body bytes the front reads before it answers or forwards:
/// a POST with a valid, allowed Content-Length; nothing otherwise, so the
/// Python handler refuses a bad length before any body arrives.
fn body_len(req: &Request) -> usize {
    if req.method != "POST" {
        return 0;
    }
    let limit = if req.path() == "/api/session/open" {
        1000
    } else if req.path() == "/api/drill/voice" {
        VOICE_MAX_BYTES
    } else {
        POST_MAX_BYTES
    };
    match content_length(req) {
        Ok(n) if (0..=limit).contains(&n) => n as usize,
        _ => 0,
    }
}

const HOP: &[&str] = &["host", "connection", "keep-alive", "proxy-connection"];

/// Relay one request to the Python handler on `port` and return its raw answer.
pub fn forward(port: u16, req: &Request, body: &[u8]) -> std::io::Result<Vec<u8>> {
    let mut up = TcpStream::connect(("127.0.0.1", port))?;
    up.set_read_timeout(Some(Duration::from_secs(600)))?;
    let mut head = format!("{} {} HTTP/1.0\r\n", req.method, req.target);
    for (name, value) in &req.headers {
        if !HOP.contains(&name.to_ascii_lowercase().as_str()) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str(&format!("Host: 127.0.0.1:{port}\r\n\r\n"));
    let mut bytes: Vec<u8> = head.chars().map(|c| c as u32 as u8).collect();
    bytes.extend_from_slice(body);
    up.write_all(&bytes)?;
    let mut out = Vec::new();
    up.read_to_end(&mut out)?;
    Ok(out)
}

fn serve_one(state: &State, stream: TcpStream) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(60)));
    let Ok(read_half) = stream.try_clone() else {
        return;
    };
    let mut reader = BufReader::new(read_half);
    let mut stream = stream;
    let req = match http::read_request(&mut reader) {
        Ok(Some(req)) => req,
        Ok(None) => return,
        Err(_) => {
            let _ = http::write_response(
                &mut stream,
                &send(400, b"bad request".to_vec(), "text/plain", &[]),
            );
            return;
        }
    };
    let mut body = vec![0u8; body_len(&req)];
    if reader.read_exact(&mut body).is_err() {
        return;
    }
    match state.handle(&req, &body) {
        Outcome::Answer(response) => {
            let _ = http::write_response(&mut stream, &response);
        }
        Outcome::Forward(_step) => {
            let port = state.backend.load(Ordering::SeqCst);
            let answer = if port == 0 {
                Err(std::io::Error::other("no backend"))
            } else {
                forward(port, &req, &body)
            };
            match answer {
                Ok(raw) => {
                    let _ = stream.write_all(&raw);
                }
                Err(_) => {
                    let _ = http::write_response(
                        &mut stream,
                        &send_json(
                            502,
                            &json::error(format!(
                                "the Python handler behind the native web server did not answer ({SIGN_IN})"
                            )),
                            &[],
                        ),
                    );
                }
            }
        }
    }
    let _ = stream.shutdown(Shutdown::Both);
}

/// A listening socket on 127.0.0.1:`port` with `SO_REUSEADDR` (like Python's
/// `HTTPServer`) and a listen backlog of `LISTEN_BACKLOG`.
pub fn bind(port: u16) -> std::io::Result<TcpListener> {
    // SAFETY: plain socket calls on a descriptor created and owned here; on
    // success the descriptor moves into the TcpListener.
    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        let one: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            std::ptr::addr_of!(one).cast(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr.s_addr = u32::from_be_bytes([127, 0, 0, 1]).to_be();
        #[cfg(target_os = "macos")]
        {
            addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        }
        if libc::bind(
            fd,
            std::ptr::addr_of!(addr).cast(),
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ) != 0
            || libc::listen(fd, LISTEN_BACKLOG) != 0
        {
            let err = std::io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }
        Ok(TcpListener::from_raw_fd(fd))
    }
}

/// Accept forever, one thread per connection (like `ThreadingHTTPServer`).
pub fn run(listener: TcpListener, state: Arc<State>) {
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        let state = Arc::clone(&state);
        std::thread::spawn(move || serve_one(&state, stream));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_route_has_one_owner_and_unknown_get_stays_native() {
        assert_eq!(owner("GET", "/api/changes"), Some(Native));
        assert_eq!(owner("GET", "/api/focus/now"), Some(Python("rust-09")));
        assert_eq!(owner("GET", "/api/focus"), Some(Python("rust-09")));
        assert_eq!(
            owner("GET", "/api/teach/improvements"),
            Some(Python("rust-21"))
        );
        assert_eq!(owner("GET", "/api/nope"), None);
        assert_eq!(owner("POST", "/api/nope"), Some(Python("rust-25")));
        assert_eq!(owner("POST", "/api/tasks/t1/done"), Some(Python("rust-09")));
        let mut seen = std::collections::HashSet::new();
        for r in ROUTES {
            assert!(
                seen.insert((r.method, r.path)),
                "duplicate route {}",
                r.path
            );
        }
    }

    #[test]
    fn route_names_never_hold_a_path_parameter() {
        assert_eq!(route_name("/api/library/secret-page"), "library.page");
        assert_eq!(route_name("/api/feed/asks"), "feed.asks");
        assert_eq!(route_name("/api/"), "api");
    }

    #[test]
    fn repr_and_lengths_follow_python() {
        assert_eq!(py_repr("abc"), "'abc'");
        assert_eq!(py_repr("a'b"), "\"a'b\"");
        assert_eq!(py_repr("a\tb"), "'a\\tb'");
    }

    #[test]
    fn the_listener_uses_the_large_backlog() {
        let listener = bind(0).unwrap();
        assert!(listener.local_addr().unwrap().port() > 0);
        assert_eq!(LISTEN_BACKLOG, 128);
    }
}
