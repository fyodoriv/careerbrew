use careerbrew_vault::transfer::Context;
use careerbrew_web::{cases, server};
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, ExitCode, Stdio};
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Arc;

/// The Python handler's pid, for the signal handler. Lifecycle: process scope;
/// set once the handler starts, read only when this process is told to stop.
static BACKEND_PID: AtomicI32 = AtomicI32::new(0);

extern "C" fn stop(_signal: libc::c_int) {
    let pid = BACKEND_PID.load(Ordering::SeqCst);
    // SAFETY: kill and _exit are async-signal-safe.
    unsafe {
        if pid > 0 {
            libc::kill(pid, libc::SIGTERM);
        }
        libc::_exit(0);
    }
}

fn usage() -> ExitCode {
    eprintln!("usage: web-component --all | serve [--port N] [--python <python>] [--dist <dir>]");
    ExitCode::from(2)
}

/// Start `careerbrew serve` on a private free port and record its port.
fn start_backend(python: PathBuf, state: Arc<server::State>) {
    let child = Command::new(python)
        .args(["-m", "careerbrew", "serve", "--port", "0"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn();
    let Ok(mut child) = child else {
        eprintln!("web-component: the Python handler did not start; forwarded views answer 502");
        return;
    };
    BACKEND_PID.store(child.id() as i32, Ordering::SeqCst);
    let Some(stdout) = child.stdout.take() else {
        return;
    };
    let marker = "careerbrew web app: http://127.0.0.1:";
    for line in BufReader::new(stdout).lines() {
        let Ok(line) = line else { break };
        if let Some(rest) = line.find(marker).map(|i| &line[i + marker.len()..]) {
            let digits = rest
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>();
            if let Ok(port) = digits.parse::<u16>() {
                state.backend.store(port, Ordering::SeqCst);
            }
        }
    }
    let _ = child.wait();
    state.backend.store(0, Ordering::SeqCst);
    eprintln!("web-component: the Python handler stopped; forwarded views answer 502");
}

fn serve(args: &[String]) -> ExitCode {
    let mut port: u16 = 8765;
    let mut python: Option<PathBuf> = None;
    let mut dist: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        let Some(value) = it.next() else {
            return usage();
        };
        match flag.as_str() {
            "--port" => match value.parse() {
                Ok(p) => port = p,
                Err(_) => return usage(),
            },
            "--python" => python = Some(PathBuf::from(value)),
            "--dist" => dist = Some(PathBuf::from(value)),
            _ => return usage(),
        }
    }
    let Ok(ctx) = Context::from_env() else {
        eprintln!("web-component: CAREERBREW_HOME is required");
        return ExitCode::from(2);
    };
    let listener = match server::bind(port) {
        Ok(listener) => listener,
        Err(_) => {
            eprintln!(
                "web-component: cannot listen on 127.0.0.1:{port} (another program may use it)"
            );
            return ExitCode::from(1);
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let dist = dist.unwrap_or_else(|| PathBuf::from("web/dist"));
    let pinned = std::env::var("CAREERBREW_SERVE_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());
    let state = Arc::new(server::State::new(ctx, port, pinned, dist));
    // SAFETY: installs a handler that only calls async-signal-safe functions.
    unsafe {
        for signal in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP] {
            libc::signal(
                signal,
                stop as extern "C" fn(libc::c_int) as libc::sighandler_t,
            );
        }
    }
    if let Some(python) = python {
        let state = Arc::clone(&state);
        std::thread::spawn(move || start_backend(python, state));
    }
    println!(
        "careerbrew web app (native): http://127.0.0.1:{port}/ (sign in with `careerbrew open`; the token never prints)"
    );
    server::run(listener, state);
    ExitCode::SUCCESS
}

fn main() -> ExitCode {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    match args.first().map(String::as_str) {
        Some("--all") if args.len() == 1 => match cases::run_all() {
            Ok(text) => {
                print!("{text}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                eprintln!("web-component: {error}");
                ExitCode::from(2)
            }
        },
        Some("serve") => serve(&args[1..]),
        _ => usage(),
    }
}
