"""rust-02 pure value vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>, or python -B driver.py --all.
A set run prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes. `--all`
prints {"sets": [...]}, the registered set names, for baseline and measure.

Sets: paths and settings. To add a set: a function below and
one line in SETS. A set must not write outside the synthetic home verify
gives it.
"""

import json
import os
import struct
import sys
import types
from pathlib import PurePosixPath
from unittest import mock

from careerbrew import config, paths


def _str(value):
    return {"t": "str", "v": value}


def _repo(path):
    """A path under the repo root reads `<repo>`, so no machine path is printed."""
    text, repo = str(path), str(paths.REPO_ROOT)
    if text == repo or text.startswith(repo + "/"):
        return "<repo>" + text[len(repo) :]
    return text


def paths_set(op, data):
    """`careerbrew.paths` with os.environ replaced by the vector's env map; never creates a folder."""
    if set(data) != {"env"} or not all(isinstance(v, str) for v in data["env"].values()):
        raise ValueError("input must be {env: {name: text}}")
    with mock.patch.dict(os.environ, data["env"], clear=True):
        if op == "data-root":
            return _str(_repo(paths.data_root(create=False)))
        if op == "data-root-refusal":
            try:
                paths.data_root(create=False)
            except RuntimeError as exc:
                if not str(exc).startswith("CAREERBREW_HOME"):
                    raise
                return _str(str(exc))
            return {"t": "null"}
        if op == "extra-knowledge":
            return {"t": "bool", "v": paths.extra_knowledge()}
        if op == "config-dir":
            return _str(_repo(paths.config_dir()))
    raise ValueError("unknown op")


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
    if kind == "list":
        return [decode(item) for item in v]
    if kind == "map":
        return {decode(k): decode(item) for k, item in v}
    raise ValueError("unknown value type")


# Validators that call workday modules with I/O, and `_tz` (tzdata): the
# candidate refuses them, so the oracle does not run them either.
DELEGATED = {"work_steps", "work_checkouts", "work_signin_probe", "standup_tz"}


def settings(op, data):
    """config's pure value logic over an in-memory settings map: the file
    read, the write and the event are replaced, so nothing touches a home."""
    stored = decode(data.get("settings", {"t": "map", "v": []}))
    written = []
    config._file = lambda: dict(stored)
    config.store = types.SimpleNamespace(write_yaml=lambda _path, value: written.append(value))
    config.events = types.SimpleNamespace(emit=lambda *_a, **_k: None)
    config.paths = types.SimpleNamespace(data_root=lambda: PurePosixPath("synthetic"))
    key = data.get("key")
    try:
        if op == "effective":
            return [encode(row) for row in config.effective()]
        if op == "get":
            return {"value": encode(config.get(key))}
        if op == "set":
            if key in DELEGATED:
                return {"refused": "delegated"}
            value = config.set_value(key, data["raw"])
            return {"value": encode(value), "written": encode(written[-1])}
        if op == "unset":
            config.unset(key)
            return {"written": encode(written[-1]) if written else None}
    except (KeyError, ValueError) as e:
        return {"raised": type(e).__name__, "message": e.args[0]}
    raise ValueError("unknown op")


# The registry: one line per vector set, sorted by name.
SETS = {
    "paths": paths_set,
    "settings": settings,
}


def main():
    if sys.argv[1:] == ["--all"]:
        print(json.dumps({"sets": sorted(SETS)}))
        return 0
    name, path = sys.argv[1:]
    run = SETS.get(name)
    if run is None:
        print("unknown vector set", file=sys.stderr)
        return 2
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
    return 0


if __name__ == "__main__":
    sys.exit(main())
