"""rust-01 dependency vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>. Prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes.

To add a set: a function below and one line in SETS.
"""

import contextlib
import datetime
import fcntl
import json
import os
import struct
import subprocess
import sys
import tempfile
from pathlib import Path

import yaml

from careerbrew import connectors, store
from careerbrew.tutor import exercises


def encode(value):
    if value is None:
        return {"t": "null"}
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, float):
        return {"t": "float", "v": struct.pack(">d", value).hex()}
    if isinstance(value, str):
        return {"t": "str", "v": value}
    if isinstance(value, datetime.datetime):
        return {"t": "datetime", "v": value.isoformat()}
    if isinstance(value, datetime.date):
        return {"t": "date", "v": value.isoformat()}
    if isinstance(value, list):
        return {"t": "list", "v": [encode(item) for item in value]}
    if isinstance(value, dict):
        return {"t": "map", "v": [[encode(k), encode(v)] for k, v in value.items()]}
    raise TypeError("unsupported value type")


def decode(value):
    kind, v = value["t"], value.get("v")
    if kind == "null":
        return None
    if kind in ("bool", "str"):
        return v
    if kind == "int":
        return int(v)
    if kind == "float":
        return struct.unpack(">d", bytes.fromhex(v))[0]
    if kind == "date":
        return datetime.date.fromisoformat(v)
    if kind == "datetime":
        return datetime.datetime.fromisoformat(v)
    if kind == "list":
        return [decode(item) for item in v]
    if kind == "map":
        return {decode(k): decode(item) for k, item in v}
    raise ValueError("unknown value type")


def _yaml_load(text):
    # store.read_yaml without the file and the default.
    return yaml.load(text, Loader=store._SAFE_LOADER)  # noqa: S506 (the safe loader store.py uses)


def json_yaml(op, data):
    if op == "json-read":
        return encode(json.loads(data))
    if op == "json-write":
        return store._json_text(decode(data))
    if op == "yaml-read":
        return encode(_yaml_load(data))
    if op == "yaml-roundtrip":
        # The text store.write_yaml writes.
        return encode(_yaml_load(yaml.safe_dump(decode(data), sort_keys=False, allow_unicode=True)))
    raise ValueError("unknown op")


@contextlib.contextmanager
def _fixtures(root):
    old = os.environ.get("CAREERBREW_HTTP_FIXTURES")
    os.environ["CAREERBREW_HTTP_FIXTURES"] = root
    try:
        yield
    finally:
        if old is None:
            os.environ.pop("CAREERBREW_HTTP_FIXTURES")
        else:
            os.environ["CAREERBREW_HTTP_FIXTURES"] = old


# The lock peer: the same script the Rust side starts (crates/depvec/src/runtime.rs).
PEER = """import fcntl, sys
path, mode, action = sys.argv[1:]
flag = {"exclusive": fcntl.LOCK_EX, "shared": fcntl.LOCK_SH}[mode]
with open(path, "a") as fh:
    if action == "probe":
        try:
            fcntl.flock(fh, flag | fcntl.LOCK_NB)
            print("1")
        except BlockingIOError:
            print("0")
    else:
        fcntl.flock(fh, flag)
        print("ready", flush=True)
        sys.stdin.readline()
        fcntl.flock(fh, fcntl.LOCK_UN)
        print("released", flush=True)
        sys.stdin.readline()
"""
LOCK_FLAGS = {"exclusive": fcntl.LOCK_EX, "shared": fcntl.LOCK_SH}


def _peer(path, mode, action):
    return subprocess.Popen(  # noqa: S603 (synthetic lock peer)
        [sys.executable, "-I", "-c", PEER, str(path), mode, action],
        stdin=subprocess.PIPE,
        stdout=subprocess.PIPE,
        stderr=subprocess.DEVNULL,
        text=True,
    )


def _lock_probe(data):
    holder, probe, released = data["holder"], data["probe"], data["released"]
    if probe not in LOCK_FLAGS or not isinstance(released, bool):
        raise ValueError("bad lock vector")
    with tempfile.TemporaryDirectory() as tmp:
        path = Path(tmp) / "probe.lock"

        def try_here():
            with path.open("a") as fh:
                try:
                    fcntl.flock(fh, LOCK_FLAGS[probe] | fcntl.LOCK_NB)
                    return True
                except BlockingIOError:
                    return False

        if holder == "none":
            return {"acquired": try_here()}
        if holder == "self":
            # The lock store.append_jsonl takes: fcntl.flock on a file opened for append.
            with path.open("a") as held:
                fcntl.flock(held, LOCK_FLAGS[data["hold"]])
                if released:
                    fcntl.flock(held, fcntl.LOCK_UN)
                out, _ = _peer(path, probe, "probe").communicate()
            return {"acquired": {"1": True, "0": False}[out.strip()]}
        if holder == "peer":
            child = _peer(path, data["hold"], "hold")
            try:
                if child.stdout.readline().strip() != "ready":
                    raise RuntimeError("peer not ready")
                if released:
                    child.stdin.write("\n")
                    child.stdin.flush()
                    if child.stdout.readline().strip() != "released":
                        raise RuntimeError("peer did not release")
                return {"acquired": try_here()}
            finally:
                child.stdin.close()
                child.wait()
        raise ValueError("unknown holder")


PROGRAMS = {"ok": "exit 0", "exit3": "exit 3", "spin": "while :; do :; done", "sleep": "exec /bin/sleep 30"}


def _run_limited(data):
    script, cpu, wall = PROGRAMS[data["program"]], data["cpu"], data["wall"]
    if not isinstance(cpu, int) or isinstance(cpu, bool) or cpu <= 0 or not wall > 0:
        raise ValueError("bad limits")
    # The limits (exercises._limits) and outcome rules of exercises._child, on a fixed shell program.
    try:
        proc = subprocess.run(  # noqa: S603 (fixed synthetic shell programs)
            ["/bin/sh", "-c", script],
            stdin=subprocess.DEVNULL,
            capture_output=True,
            timeout=wall,
            env={},
            preexec_fn=lambda: exercises._limits(cpu),  # noqa: PLW1509 (the exercises runner does the same)
        )
    except subprocess.TimeoutExpired:
        return {"category": "wall_timeout", "exit_code": None}
    code = proc.returncode
    if code in (-9, -24, 152):
        return {"category": "cpu_limit", "exit_code": code if code >= 0 else None}
    if code < 0:
        return {"category": "signal_exit", "exit_code": None}
    return {"category": "ok" if code == 0 else "nonzero_exit", "exit_code": code}


def runtime(op, data):
    if op == "http-fixture-path":
        if not isinstance(data, str):
            raise TypeError("input is not text")
        with _fixtures(""):
            return str(connectors._fixture_path(data))
    if op == "http-fixture-read":
        with tempfile.TemporaryDirectory() as tmp:
            for name, body in data["files"].items():
                path = Path(tmp) / name
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(body, encoding="utf-8")
            with _fixtures(tmp):
                return connectors.http_get(data["url"]).decode("utf-8")
    if op == "lock-probe":
        return _lock_probe(data)
    if op == "run-limited":
        return _run_limited(data)
    raise ValueError("unknown op")


# The registry: one line per vector set.
SETS = {"json-yaml": json_yaml, "runtime": runtime}


def main():
    name, path = sys.argv[1:]
    run = SETS[name]
    with open(path, encoding="utf-8") as fh:
        fixture = json.load(fh)
    results = []
    for vector in fixture["vectors"]:
        try:
            output = {"ok": run(vector["op"], vector["input"])}
        except Exception:  # noqa: BLE001 (any failure is the outcome "error")
            output = {"error": True}
        results.append({"id": vector["id"], "output": output})
    print(json.dumps({"set": name, "results": results}))


if __name__ == "__main__":
    main()
