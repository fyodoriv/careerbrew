"""rust-01 dependency vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>. Prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes.

To add a set: a function below and one line in SETS.
"""

import base64
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
from zoneinfo import ZoneInfo

import fsrs
import yaml
from cryptography.fernet import Fernet

from careerbrew import backup, calendar, connectors, store, vault
from careerbrew.tutor import cards, exercises


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


def calendar_set(op, data):
    tz = ZoneInfo(data["tz"])
    if op == "day":
        events = calendar.parse_ics(data["ics"], datetime.date.fromisoformat(data["day"]), tz)
        return [
            {"start": e.start.astimezone(tz).isoformat(), "end": e.end.astimezone(tz).isoformat(), "title": e.title, "busy": e.busy, "personal": e.personal, "recurring": e.recurring}
            for e in sorted(events, key=lambda e: (e.start, e.end, e.title))
        ]
    if op == "local-day":
        return datetime.datetime.fromisoformat(data["at"]).astimezone(tz).date().isoformat()
    raise ValueError("unknown op")


ROOT = Path(__file__).resolve().parents[4]


def _esbuild():
    # The native esbuild binary `npm ci` puts in web/node_modules: the same engine
    # scripts/check-card-code.py calls through node. A missing binary stops the run
    # (verify is refused), never a silent skip.
    found = sorted(ROOT.glob("web/node_modules/@esbuild/*/bin/esbuild"))
    if not found:
        raise SystemExit("esbuild is missing: run scripts/setup.sh")
    return found[0]


def code_parse(op, data):
    # The checks of scripts/check-card-code.py: compile() for Python, esbuild's
    # transform (format esm) for the JS and TS loaders.
    if op == "python-parse":
        compile(data, "example", "exec")
        return "parsed"
    if op == "js-parse":
        if data["loader"] not in ("js", "jsx", "ts", "tsx"):
            raise ValueError("unknown loader")
        args = [str(_esbuild()), f"--loader={data['loader']}", "--format=esm", "--log-level=silent"]
        done = subprocess.run(args, input=data["code"].encode(), capture_output=True, check=False)  # noqa: S603
        if done.returncode != 0:
            raise ValueError("does not parse")
        return "parsed"
    raise ValueError("unknown op")


@contextlib.contextmanager
def _key_file(text):
    # A scratch key file and data root, so vault.py loads the key its own way.
    with tempfile.TemporaryDirectory() as scratch:
        key = os.path.join(scratch, "backup.key")
        with open(key, "w", encoding="utf-8") as fh:
            fh.write(text)
        old = {name: os.environ.get(name) for name in ("CAREERBREW_KEY_FILE", "CAREERBREW_HOME")}
        os.environ["CAREERBREW_KEY_FILE"] = key
        os.environ["CAREERBREW_HOME"] = os.path.join(scratch, "data")
        try:
            yield
        finally:
            for name, value in old.items():
                if value is None:
                    os.environ.pop(name, None)
                else:
                    os.environ[name] = value


def fernet(op, data):
    with _key_file(data["cipher_file"]):
        if op == "open":
            return vault.open_bytes(data["data"].encode(), create_key=False).hex()
        if op == "open-strict":
            # The checks backup.verify makes before it decrypts a backup.
            token = data["data"].encode()
            if len(token) % 4 or not token:
                raise ValueError("not strict base64")
            base64.b64decode(token, altchars=b"-_", validate=True)
            return Fernet(backup.load_key(create=False)).decrypt(token).hex()
        if op == "seal":
            sealed = vault.seal_bytes(bytes.fromhex(data["plaintext_hex"]))
            raw = base64.urlsafe_b64decode(sealed[len(vault.MAGIC) :])
            return {
                "magic": vault.is_sealed(sealed),
                "version": raw[0],
                "length": len(raw),
                "opens": vault.open_bytes(sealed, create_key=False).hex(),
            }
    raise ValueError("unknown op")


# A fixed synthetic start; review times are minutes after it.
FSRS_START = datetime.datetime(2026, 1, 5, 9, 0, tzinfo=datetime.UTC)


def _scheduler():
    # cards.review calls fsrs.Scheduler() with its defaults; fuzzing is pinned
    # off, since py-fsrs draws it from random() and a vector must be fixed.
    return fsrs.Scheduler(enable_fuzzing=False)


def fsrs_schedule(op, data):
    if op == "parameters":
        if data is not None:
            raise ValueError("parameters takes no input")
        scheduler = _scheduler()
        return {
            "parameters": [f"{w:.4f}" for w in scheduler.parameters],
            "desired_retention": f"{scheduler.desired_retention:.2f}",
            "learning_steps_seconds": [int(s.total_seconds()) for s in scheduler.learning_steps],
            "relearning_steps_seconds": [int(s.total_seconds()) for s in scheduler.relearning_steps],
            "maximum_interval_days": scheduler.maximum_interval,
        }
    if op == "schedule":
        scheduler = _scheduler()
        card = fsrs.Card()
        out = []
        for review in data["reviews"]:
            at = FSRS_START + datetime.timedelta(minutes=review["at_minutes"])
            card, _log = scheduler.review_card(card, cards.RATINGS[review["rating"]], review_datetime=at)
            out.append(
                {
                    "state": card.state.name.lower(),
                    "step": card.step,
                    "due_seconds": int((card.due - FSRS_START).total_seconds()),
                    "stability": f"{card.stability:.2f}",
                    "difficulty": f"{card.difficulty:.2f}",
                }
            )
        return out
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
SETS = {
    "calendar": calendar_set,
    "code-parse": code_parse,
    "fernet": fernet,
    "fsrs": fsrs_schedule,
    "json-yaml": json_yaml,
    "runtime": runtime,
}


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
