"""rust-01 dependency vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>. Prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes.

To add a set: a function below and one line in SETS.
"""

import base64
import contextlib
import datetime
import json
import os
import struct
import subprocess
import sys
from zoneinfo import ZoneInfo
from pathlib import Path
import tempfile

import yaml
from cryptography.fernet import Fernet

from careerbrew import calendar, store
from careerbrew import backup, store, vault


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


# The registry: one line per vector set.
SETS = {"calendar": calendar_set, "json-yaml": json_yaml}
SETS = {"json-yaml": json_yaml, "code-parse": code_parse}
SETS = {"fernet": fernet, "json-yaml": json_yaml}


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
