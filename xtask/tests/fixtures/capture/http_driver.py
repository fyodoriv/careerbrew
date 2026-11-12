"""Run the real handler in one invocation-owned synthetic oracle process."""

import hashlib
import json
import os
import threading
import time
from pathlib import Path

control = json.loads(Path(os.environ["CAREERBREW_CAPTURE_INPUT"]).read_text())
protocol = Path(control["protocol"])
requests = []
fault = control["fault"]
root = Path(os.environ["CAREERBREW_CAPTURE_ROOT"])
if fault == "served-source":
    if not (root / ".synthetic-http-source-copy").is_file():
        raise RuntimeError("source mutation requires an owned synthetic copy")
    source = root / "src/careerbrew/server.py"
    source.write_bytes(source.read_bytes() + b"\n# synthetic served-source alteration\n")

from careerbrew import server  # noqa: E402 (mutate only the owned source copy before import)

if "CAREERBREW_HTTP_TEST_PID" in os.environ:
    Path(os.environ["CAREERBREW_HTTP_TEST_PID"]).write_text(str(os.getpid()))

if fault == "auth-bypass":
    server.Handler._guard = lambda self, path: None
elif fault in {"weak-header", "invalid-date", "invalid-weekday"}:
    original_header = server.Handler.send_header

    def changed_header(self, name, value):
        if fault == "weak-header" and name == "Cache-Control":
            value = "public"
        if fault == "invalid-date" and name == "Date":
            value = "Mon, 30 Feb 2026 09:00:00 GMT"
        if fault == "invalid-weekday" and name == "Date":
            value = "Mon, 05 Nov 2026 09:00:00 GMT"
        return original_header(self, name, value)

    server.Handler.send_header = changed_header
elif fault in {"bad-status", "malformed-header", "duplicate-length", "transfer-encoding", "truncated-body", "oversized-response", "hang"}:

    def wrong_send(self, *args, **kwargs):
        if fault == "hang":
            time.sleep(60)
            return
        status = "+200" if fault == "bad-status" else "200"
        headers = "Content-Length: 2\r\n"
        if fault == "malformed-header":
            headers = "BrokenHeader\r\n" + headers
        elif fault == "duplicate-length":
            headers += headers
        elif fault == "transfer-encoding":
            headers += "Transfer-Encoding: chunked\r\n"
        elif fault == "truncated-body":
            headers = "Content-Length: 5\r\n"
        elif fault == "oversized-response":
            headers = "Content-Length: 5000000\r\n"
        self.close_connection = True
        self.wfile.write(f"HTTP/1.0 {status} OK\r\n{headers}\r\n".encode())
        if fault == "oversized-response":
            for _ in range(80):
                self.wfile.write(b"x" * 65536)
        else:
            self.wfile.write(b"{}")

    server.Handler._send = wrong_send


def traced(original):
    def dispatch(self):
        requests.append({"method": self.command, "path": self.path, "pid": os.getpid(), "source": original.__code__.co_filename})
        return original(self)

    return dispatch


server.Handler.do_GET = traced(server.Handler.do_GET)
server.Handler.do_POST = traced(server.Handler.do_POST)
srv = server.make_server(0, control["token"])
worker = threading.Thread(target=srv.serve_forever)
worker.start()
source = Path(server.__file__).resolve()


def publication_marker(name, value):
    temporary = protocol / (name + ".tmp")
    with temporary.open("x", encoding="utf-8") as stream:
        json.dump(value, stream, sort_keys=True)
    os.replace(temporary, protocol / name)


class PausedReadinessWrite:
    def __init__(self, stream, path):
        self.stream = stream
        self.path = path

    def __enter__(self):
        self.stream.__enter__()
        return self

    def write(self, text):
        count = self.stream.write(text[:1])
        self.stream.flush()
        os.fsync(self.stream.fileno())
        publication_marker("publication-paused.json", {"pid": os.getpid(), "opened_path": str(self.path)})
        while not (protocol / "publication-ack").exists():
            time.sleep(0.01)
        return count + self.stream.write(text[1:])

    def __exit__(self, *arguments):
        return self.stream.__exit__(*arguments)


original_path_open = Path.open


def paused_readiness_open(path, *arguments, **keywords):
    stream = original_path_open(path, *arguments, **keywords)
    mode = arguments[0] if arguments else keywords.get("mode", "r")
    if path in {protocol / "ready.json", protocol / ".ready.json.tmp"} and mode in {"w", "x"}:
        return PausedReadinessWrite(stream, path)
    return stream


readiness = json.dumps({"pid": os.getpid(), "port": srv.server_address[1], "host": srv.server_address[0], "server_path": str(source), "server_sha256": hashlib.sha256(source.read_bytes()).hexdigest()})
if fault == "malformed-readiness":
    readiness = '{"pid":'
if fault == "paused-readiness":
    Path.open = paused_readiness_open
try:
    temporary = protocol / ".ready.json.tmp"
    with temporary.open("x", encoding="utf-8") as stream:
        stream.write(readiness)
    os.replace(temporary, protocol / "ready.json")
finally:
    Path.open = original_path_open
if fault == "malformed-readiness":
    publication_marker("publication-closed.json", {"pid": os.getpid(), "final_path": str(protocol / "ready.json")})
try:
    while not (protocol / "stop").exists():
        time.sleep(0.01)
finally:
    srv.shutdown()
    worker.join()
    srv.server_close()
    (protocol / "requests.json").write_text(json.dumps(requests))
