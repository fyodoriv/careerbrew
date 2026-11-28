"""rust-25 oracle: the Python web server's answers to the fixed synthetic HTTP cases.

`--all` seeds the synthetic data root (`CAREERBREW_HOME`) from
`tests/fixtures/web/rust25-cases.json` unless it is seeded already, starts
`server.make_server` on a free OS port in this process, sends every case owned
by the native front, and prints one normalized JSON line per case. The native
`web-component --all` prints the same lines for the Rust server; the xtask
`rust25` record compares the two. It never touches port 8765 or owner data.

`--forward <web-component>` instead starts the Python server and the native
server (with its Python handler) as separate processes on free ports in the
same data root, sends every case to both, and prints one JSON line per case
with both answers.
"""

from __future__ import annotations

import json
import os
import socket
import stat
import subprocess
import sys
import time
from pathlib import Path
from typing import Any

ROOT = Path(__file__).resolve().parents[2]
FIXTURE = ROOT / "tests" / "fixtures" / "web" / "rust25-cases.json"
SEEDED, DIST, BROWSER = ".rust25-seeded", ".rust25-dist", ".rust25-browser"


def fixture() -> dict[str, Any]:
    return json.loads(FIXTURE.read_text())


def home() -> Path:
    value = os.environ.get("CAREERBREW_HOME", "")
    if not value:
        raise SystemExit("rust25 oracle: CAREERBREW_HOME is required (a synthetic data root)")
    return Path(value)


def seed(root: Path) -> None:
    from careerbrew import store

    if (root / SEEDED).exists():
        return
    fx = fixture()["seed"]
    (root / "day").mkdir(parents=True, exist_ok=True)
    store.write_json(root / "day" / "web-token.json", fx["token"])
    for f in fx["files"]:
        target = root / f["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        if f["sealed"]:
            store.write_text(target, f["text"])
        else:
            target.write_text(f["text"])
    for f in fx["dist"]:
        target = root / DIST / f["path"]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(f["text"])
    browser = root / BROWSER
    browser.write_text('#!/bin/sh\nprintf \'%s\\n\' "$1" >> "$0.log"\nexit 0\n')
    browser.chmod(browser.stat().st_mode | stat.S_IXUSR)
    (root / SEEDED).write_text("")


def apply_setup(root: Path, setup: dict[str, Any] | None) -> None:
    from careerbrew import store

    if not setup:
        return
    if "settings" in setup:
        store.write_yaml(root / "settings.yaml", setup["settings"])
    if "role" in setup:
        store.write_yaml(root / "machine.yaml", {"role": setup["role"]})


def clear_setup(root: Path, setup: dict[str, Any] | None) -> None:
    if not setup:
        return
    if "settings" in setup:
        (root / "settings.yaml").unlink(missing_ok=True)
    if "role" in setup:
        (root / "machine.yaml").unlink(missing_ok=True)


def request_bytes(case: dict[str, Any], port: int, token: str) -> bytes:
    body = case.get("body", "")
    head = f"{case['method']} {case['path']} HTTP/1.0\r\n"
    has_length = False
    for name, value in case.get("headers", []):
        has_length |= name.lower() == "content-length"
        head += f"{name}: {value.replace('{port}', str(port)).replace('{token}', token)}\r\n"
    if body and not has_length:
        head += f"Content-Length: {len(body.encode())}\r\n"
    return (head + "\r\n").encode("latin-1") + body.encode()


def send_raw(port: int, data: bytes) -> bytes:
    with socket.create_connection(("127.0.0.1", port), timeout=30) as s:
        s.sendall(data)
        chunks = []
        while chunk := s.recv(65536):
            chunks.append(chunk)
    return b"".join(chunks)


def normalize(case_id: str, raw: bytes, port: int) -> dict[str, Any]:
    split = raw.find(b"\r\n\r\n")
    split = len(raw) if split < 0 else split
    head, body = raw[:split].decode("utf-8", "replace"), raw[split + 4 :]
    lines = head.split("\r\n")
    parts = lines[0].split(" ", 2)
    status = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 0
    reason = parts[2] if len(parts) > 2 else ""
    headers, ctype = [], ""
    for line in lines[1:]:
        if ":" not in line:
            continue
        name, value = line.split(":", 1)
        name, value = name.strip().lower(), value.strip().replace(f"careerbrew-{port}=", "careerbrew-<port>=")
        if name in ("server", "date"):
            continue
        if name == "content-type":
            ctype = value
        headers.append([name, value])
    parsed: Any
    if ctype.startswith("application/json"):
        try:
            parsed = json.loads(body)
        except ValueError:
            parsed = {"text": body.decode("utf-8", "replace")}
    else:
        parsed = {"text": body.decode("utf-8", "replace")}
    if isinstance(parsed, dict) and isinstance(parsed.get("key"), str) and len(parsed["key"]) == 44:
        parsed["key"] = "<tab-key>"
    return {"case": case_id, "status": status, "reason": reason, "headers": headers, "body": parsed}


def line(value: dict[str, Any]) -> str:
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def run_all() -> str:
    root = home()
    seed(root)
    os.environ["BROWSER"] = str(root / BROWSER)
    from careerbrew import server

    server.DIST = root / DIST
    fx = fixture()
    token = fx["seed"]["token"]["token"]
    srv, _thread = server.serve_in_thread(0, os.environ.get("CAREERBREW_SERVE_TOKEN") or None)
    port = srv.server_address[1]
    out = []
    try:
        for case in fx["cases"]:
            if case.get("owner") != "native":
                continue
            apply_setup(root, case.get("setup"))
            try:
                raw = send_raw(port, request_bytes(case, port, token))
            finally:
                clear_setup(root, case.get("setup"))
            out.append(line(normalize(case["id"], raw, port)))
    finally:
        srv.shutdown()
        srv.server_close()
    return "".join(f"{x}\n" for x in out)


def _free_port() -> int:
    with socket.socket() as s:
        s.bind(("127.0.0.1", 0))
        return int(s.getsockname()[1])


def _wait(port: int, token: str, proc: subprocess.Popen[bytes], seconds: float = 60) -> None:
    deadline = time.monotonic() + seconds
    probe = request_bytes({"method": "GET", "path": "/api/ping", "headers": [["Host", "127.0.0.1:{port}"], ["X-Careerbrew-Token", "{token}"]]}, port, token)
    while time.monotonic() < deadline:
        if proc.poll() is not None:
            raise SystemExit(f"rust25 oracle: a server exited early ({proc.returncode})")
        try:
            if b" 200 " in send_raw(port, probe).split(b"\r\n", 1)[0] + b" ":
                return
        except OSError:
            pass
        time.sleep(0.1)
    raise SystemExit("rust25 oracle: a server did not answer its ping in time")


# The change version hashes the data root, and either server's cache warmer may
# write there between the two reads. Ask again until both reads see one state.
RACY_CASES = {"changes-ok"}
RACY_TRIES = 10


def _ask_both(case: dict[str, Any], token: str, py_port: int, rs_port: int) -> dict[str, Any]:
    tries = RACY_TRIES if case["id"] in RACY_CASES else 1
    answers: dict[str, Any] = {}
    for attempt in range(tries):
        answers = {name: normalize(case["id"], send_raw(port, request_bytes(case, port, token)), port) for name, port in (("python", py_port), ("native", rs_port))}
        if answers["python"] == answers["native"] or attempt == tries - 1:
            break
        time.sleep(0.2)
    return answers


def run_forward(candidate: str) -> str:
    """Both servers as processes in one data root; every case goes to both."""
    root = home()
    seed(root)
    env = {**os.environ, "BROWSER": str(root / BROWSER), "CAREERBREW_EVENTS": os.environ.get("CAREERBREW_EVENTS", "off")}
    fx = fixture()
    token = fx["seed"]["token"]["token"]
    py_port, rs_port = _free_port(), _free_port()
    # The Python server reads web/dist from the checkout; this run pins the synthetic one for both.
    py = subprocess.Popen(  # noqa: S603
        [sys.executable, "-I", "-B", str(Path(__file__)), "--serve-python", str(py_port)], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE
    )
    rs = subprocess.Popen(  # noqa: S603
        [candidate, "serve", "--port", str(rs_port), "--python", sys.executable, "--dist", str(root / DIST)], env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE
    )
    out = []
    try:
        _wait(py_port, token, py)
        _wait(rs_port, token, rs)
        for case in fx["cases"]:
            apply_setup(root, case.get("setup"))
            try:
                answers = _ask_both(case, token, py_port, rs_port)
            finally:
                clear_setup(root, case.get("setup"))
            out.append(line({"case": case["id"], "owner": case["owner"], **answers}))
    finally:
        for proc in (rs, py):
            proc.terminate()
            try:
                proc.wait(timeout=10)
            except subprocess.TimeoutExpired:
                proc.kill()
    return "".join(f"{x}\n" for x in out)


def serve_python(port: int) -> None:
    from careerbrew import server

    # The same entry point as `careerbrew serve` (with its cache warmer), over the synthetic dist.
    server.DIST = home() / DIST
    server.run(port)


def main(argv: list[str]) -> int:
    if argv == ["--all"]:
        sys.stdout.write(run_all())
        return 0
    if len(argv) == 2 and argv[0] == "--forward":
        sys.stdout.write(run_forward(argv[1]))
        return 0
    if len(argv) == 2 and argv[0] == "--serve-python":
        serve_python(int(argv[1]))
        return 0
    sys.stderr.write("usage: rust25.py --all | --forward <web-component>\n")
    return 2


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
