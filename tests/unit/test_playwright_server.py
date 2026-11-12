"""A Playwright run tests only the server it started (task open-asks-mark-flake).

With `webServer.url`, a run whose server lost the port to another run still saw "a server answers" and tested
that run's data: two runs on one port failed each other's open asks counts. The config now waits for its own
server's address line, which `careerbrew serve` prints only after it holds the port.
"""

import os
import queue
import re
import socket
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

CONFIG = Path(__file__).resolve().parents[2] / "web" / "playwright.config.ts"


def _web_server_block() -> str:
    text = CONFIG.read_text()
    first = re.search(r"webServer: \[?\{(.*?)\n  \},", text, re.S)
    assert first, "the first webServer config owns the ordinary suite"
    return first.group(1)


def _ready_line(port: int) -> re.Pattern[str]:
    """The config's `wait.stdout` pattern for `port`, read from the config itself."""
    m = re.search(r"wait: \{ stdout: new RegExp\(`(.+?)`\) \}", _web_server_block())
    assert m, "webServer waits for the server's own address line (wait.stdout)"
    return re.compile(m.group(1).replace("${PORT}", str(port)).replace("\\\\", "\\"))


# `careerbrew serve` with the web build folder set to `dist`, so a test never depends on whether this checkout has
# built web/dist (the gate's unit step runs before its web-build step, so a fresh checkout has none).
_SERVE_WITH_DIST = "import sys; from pathlib import Path; from careerbrew import cli, server; server.DIST = Path(sys.argv[1]); sys.exit(cli.main(sys.argv[2:]))"


def _serve(port: int, dist: Path) -> subprocess.Popen[str]:
    env = {**os.environ, "CAREERBREW_SERVE_TOKEN": "unit-test-token"}
    cmd = [sys.executable, "-c", _SERVE_WITH_DIST, str(dist), "serve", "--port", str(port)]
    return subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)


def _lines_until_ready(proc: subprocess.Popen[str], port: int, timeout: float = 60.0) -> list[str]:
    """The server's stdout lines up to and including the ready line. Like Playwright's `wait.stdout`, scan every line:
    the server may print notes first (for example "the web app is not built yet")."""
    stdout = proc.stdout
    assert stdout is not None
    lines: queue.Queue[str] = queue.Queue()

    def pump() -> None:
        for line in stdout:
            lines.put(line)
        lines.put("")  # end of output

    threading.Thread(target=pump, daemon=True).start()
    seen: list[str] = []
    deadline = time.monotonic() + timeout
    while (left := deadline - time.monotonic()) > 0:
        try:
            line = lines.get(timeout=left)
        except queue.Empty:
            break
        if not line:
            break
        seen.append(line)
        if _ready_line(port).search(line):
            return seen
    raise AssertionError(f"the server printed no ready line within {timeout:.0f}s: {seen!r}")


def _dist(tmp_path: Path, built: bool) -> Path:
    dist = tmp_path / "dist"
    dist.mkdir()
    if built:
        (dist / "index.html").write_text("<!doctype html>", encoding="utf-8")
    return dist


def test_the_run_is_ready_only_on_its_own_servers_line():
    block = _web_server_block()
    assert "wait: { stdout:" in block
    assert "url:" not in block and "port:" not in block, "a url or port check also passes on another run's server on that port"


@pytest.mark.parametrize("built", [True, False], ids=["web-app-built", "web-app-not-built"])
def test_the_server_prints_the_ready_line_once_it_holds_the_port(home, tmp_path, built):
    free = socket.socket()
    free.bind(("127.0.0.1", 0))
    port = free.getsockname()[1]
    free.close()
    proc = _serve(port, _dist(tmp_path, built))
    try:
        lines = _lines_until_ready(proc, port)
        if not built:
            assert "not built yet" in lines[0], "the case a fresh checkout hits: a note comes before the ready line"
        with socket.create_connection(("127.0.0.1", port), timeout=10):
            pass  # it listens by the time it prints the line
    finally:
        proc.terminate()
        proc.wait(30)


def test_a_server_that_finds_its_port_taken_never_prints_the_ready_line(home, tmp_path):
    holder = socket.socket()
    holder.bind(("127.0.0.1", 0))
    holder.listen()
    port = holder.getsockname()[1]
    try:
        proc = _serve(port, _dist(tmp_path, built=False))
        out, err = proc.communicate(timeout=60)
    finally:
        holder.close()
    assert proc.returncode != 0 and "Address already in use" in err
    assert not _ready_line(port).search(out), "so Playwright stops instead of testing whatever answers on the port"
