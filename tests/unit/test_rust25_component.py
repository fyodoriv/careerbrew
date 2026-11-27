"""rust-25: the native web front answers like the Python server, and stays opt-in."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust25.py"
FIXTURE = json.loads((ROOT / "tests" / "fixtures" / "web" / "rust25-cases.json").read_text())
NATIVE = {c["id"] for c in FIXTURE["cases"] if c["owner"] == "native"}
FORWARDED = {c["id"] for c in FIXTURE["cases"] if c["owner"] == "python"}


@pytest.fixture(scope="module")
def candidate() -> Path:
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the component checks need the Rust toolchain")
    subprocess.run(["cargo", "build", "--locked", "--offline", "-p", "careerbrew-web", "--bin", "web-component"], cwd=ROOT, check=True, capture_output=True)
    return ROOT / "target" / "debug" / "web-component"


def synthetic_env(root: Path) -> dict[str, str]:
    for folder in ("data", "home", "keys", "tmp"):
        (root / folder).mkdir(parents=True, exist_ok=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(root / "data"),
        "TMPDIR": str(root / "tmp"),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "off",
        "CAREERBREW_LLM": "off",
        "CAREERBREW_NOTIFY": "off",
        "CAREERBREW_POSTHOG_KEY": "",
        "CAREERBREW_NATIVE": "off",
        "CAREERBREW_NOW": "2026-11-20T10:00:00+00:00",
        "PYTHONHASHSEED": "0",
        "TZ": "UTC",
        "LC_ALL": "C",
        "PATH": "/usr/bin:/bin",
    }


def run(args: list[str], env: dict[str, str]) -> str:
    done = subprocess.run(args, cwd=ROOT, env=env, capture_output=True, text=True, timeout=240, check=False)
    assert done.returncode == 0, done.stderr[-2000:]
    return done.stdout


def by_case(text: str) -> dict[str, str]:
    return {json.loads(line)["case"]: line for line in text.splitlines()}


def test_every_native_case_answers_byte_for_byte_like_the_python_server(candidate: Path, tmp_path: Path) -> None:
    env = synthetic_env(tmp_path)
    expected = by_case(run([sys.executable, "-I", "-B", str(ORACLE), "--all"], env))
    actual = by_case(run([str(candidate), "--all"], env))
    assert set(expected) == NATIVE
    assert set(actual) == NATIVE
    differing = sorted(case for case in NATIVE if expected[case] != actual[case])
    assert differing == []
    # The checks are real: a sign-in sets the cookie, a wrong token and a bad host are refused.
    assert json.loads(actual["session-set-ok"])["status"] == 200
    assert any(h[0] == "set-cookie" and "HttpOnly; SameSite=Strict" in h[1] for h in json.loads(actual["session-set-ok"])["headers"])
    assert json.loads(actual["changes-wrong-token"])["status"] == 401
    assert json.loads(actual["changes-bad-host"])["status"] == 403
    assert json.loads(actual["changes-expired"])["status"] == 401


def test_forwarded_routes_reach_the_python_handler_unchanged(candidate: Path, tmp_path: Path) -> None:
    env = synthetic_env(tmp_path)
    rows = [json.loads(line) for line in run([sys.executable, "-I", "-B", str(ORACLE), "--forward", str(candidate)], env).splitlines()]
    assert {r["case"] for r in rows} == NATIVE | FORWARDED
    for row in rows:
        if row["case"] == "forward-ping":  # the warmer's progress is a race between two servers, not the front
            for side in ("python", "native"):
                row[side]["body"].pop("warm", None)
                row[side]["headers"] = [h for h in row[side]["headers"] if h[0] != "content-length"]
        assert row["python"] == row["native"], row["case"]
    statuses = {r["case"]: r["native"]["status"] for r in rows}
    assert statuses["forward-config"] == 200
    assert statuses["forward-post-not-object"] == 400
    assert statuses["forward-put"] == 501


def test_the_default_server_stays_python_and_native_is_opt_in(monkeypatch: pytest.MonkeyPatch, tmp_path: Path) -> None:
    from careerbrew import config, server

    default, check, _help = config.DEFAULTS["web_server"]
    assert default == "python"
    assert check("native") == "native"
    with pytest.raises(ValueError):
        check("go")
    monkeypatch.setattr(config, "get", lambda key: "python" if key == "web_server" else config.DEFAULTS[key][0])
    command, extra = server.serve_command(8999)
    assert command[1:] == ["-m", "careerbrew", "serve", "--port", "8999"]
    assert extra == {}
    monkeypatch.setattr(config, "get", lambda key: "native" if key == "web_server" else config.DEFAULTS[key][0])
    monkeypatch.delenv("CAREERBREW_WEB_BIN", raising=False)
    with pytest.raises(RuntimeError, match="CAREERBREW_WEB_BIN"):
        server.serve_command(8999)
    monkeypatch.setenv("CAREERBREW_WEB_BIN", str(tmp_path / "missing"))
    with pytest.raises(RuntimeError, match="CAREERBREW_WEB_BIN"):
        server.serve_command(8999)
    fake = tmp_path / "web-component"
    fake.write_text("#!/bin/sh\nexit 0\n")
    fake.chmod(0o755)
    monkeypatch.setenv("CAREERBREW_WEB_BIN", str(fake))
    monkeypatch.setenv("CAREERBREW_HOME", str(tmp_path / "data"))
    command, extra = server.serve_command(8999)
    assert command[:4] == [str(fake), "serve", "--port", "8999"]
    assert command[command.index("--python") + 1] == sys.executable
    assert extra["CAREERBREW_HOME"] == str(tmp_path / "data")
    assert "CAREERBREW_SERVE_TOKEN" not in extra


def test_a_tracked_restart_recognizes_the_native_server_and_nothing_else() -> None:
    from careerbrew import server

    assert server._SERVE_CMD.search("/repo/target/release/web-component serve --port 8765 --python /v/bin/python")
    assert server._SERVE_CMD.search("/v/bin/python -m careerbrew serve --port 8765")
    assert not server._SERVE_CMD.search("/repo/target/release/web-component --all")
    assert not server._SERVE_CMD.search("node web-server serve")


def test_the_native_server_never_prints_the_token(candidate: Path, tmp_path: Path) -> None:
    env = synthetic_env(tmp_path) | {"CAREERBREW_SERVE_TOKEN": "synthetic-pinned-token-never-printed"}
    proc = subprocess.Popen([str(candidate), "serve", "--port", "0", "--dist", str(tmp_path / "dist")], cwd=ROOT, env=env, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)  # noqa: S603
    try:
        line = proc.stdout.readline() if proc.stdout else ""
    finally:
        proc.terminate()
        out, err = proc.communicate(timeout=10)
    assert "careerbrew web app (native): http://127.0.0.1:" in line
    assert "synthetic-pinned-token" not in line + out + err
