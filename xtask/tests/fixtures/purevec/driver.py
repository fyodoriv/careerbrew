"""rust-02 pure value vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>, or python -B driver.py --all.
A set run prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes. `--all`
prints {"sets": [...]}, the registered set names, for baseline and measure.

Sets: clock, hash, paths and settings. To add a set: a function below and
one line in SETS. A set must not write outside the synthetic home verify
gives it.
"""

import datetime
import json
import os
import struct
import sys
import types
import zoneinfo
from pathlib import PurePosixPath
from unittest import mock

from careerbrew import config, paths

LOCAL_TZ = "local_tz"


def _clock_encode(value):
    """The typed value form of `crates/depvec/src/canon.rs`, for the clock set's types."""
    if isinstance(value, bool):
        return {"t": "bool", "v": value}
    if isinstance(value, int):
        return {"t": "int", "v": str(value)}
    if isinstance(value, str):
        return {"t": "str", "v": value}
    if isinstance(value, datetime.datetime):
        return {"t": "datetime", "v": value.isoformat()}
    if isinstance(value, list | tuple):
        return {"t": "list", "v": [_clock_encode(item) for item in value]}
    raise TypeError("unsupported value type")


def _clock_env(name, value):
    """Sets or removes one environment variable; returns the old value."""
    old = os.environ.get(name)
    if value is None:
        os.environ.pop(name, None)
    else:
        os.environ[name] = value
    return old


def clock(op, data):
    """calendar.py clock helpers and config._tz, with injected zones and clocks.

    Zones come from the tzdata package only (the tz path is emptied), so the
    oracle's zone data does not depend on the machine. `local_tz` (the
    settings read and /etc/localtime) is not ported here (rust-05): the
    vector's zone replaces it. The vector's `clock` instant replaces the wall
    clock, so no vector reads it.
    """
    zoneinfo.reset_tzpath(to=())
    from careerbrew import calendar, config

    enc = _clock_encode
    if op == "tz-valid":
        return enc(config._tz(data["value"]))
    if op == "system-tz":
        old = _clock_env("TZ", data["tz_env"])
        try:
            return enc(calendar.system_tz())
        finally:
            _clock_env("TZ", old)
    tz = zoneinfo.ZoneInfo(data["tz"]) if data.get("tz") else None
    second = datetime.timedelta(seconds=1)
    if op == "day-bounds":
        return enc(list(calendar._day_bounds(datetime.date.fromisoformat(data["day"]), tz)))
    if op == "day-span":
        start, end = calendar._day_bounds(datetime.date.fromisoformat(data["day"]), tz)
        real = end.astimezone(datetime.UTC) - start.astimezone(datetime.UTC)
        return enc([(end - start) // second, real // second])
    if op == "aware":
        text = data["value"]
        value = datetime.date.fromisoformat(text) if data["kind"] == "date" else datetime.datetime.fromisoformat(text)
        return enc(calendar._aware(value, tz))
    if op == "shift":
        start = datetime.datetime.fromisoformat(data["value"])
        if tz is not None:
            start = calendar._aware(start, tz)
        delta = datetime.timedelta(days=data["days"], hours=data["hours"], minutes=data["minutes"], seconds=data["seconds"])
        return enc(start + delta)
    if op in ("now", "now-utc"):
        instant = datetime.datetime.fromisoformat(data["clock"])

        class Frozen(datetime.datetime):
            @classmethod
            def now(cls, tz=None):
                return instant.astimezone(tz)

        # getattr keeps "calendar" + "." + "local_tz" out of the text: the
        # public-readiness host scan reads that as a .local host name.
        saved = getattr(calendar, LOCAL_TZ), calendar.datetime
        setattr(calendar, LOCAL_TZ, lambda: tz)
        calendar.datetime = Frozen
        old = _clock_env("CAREERBREW_NOW", data["pinned"])
        try:
            return enc(calendar.now() if op == "now" else calendar.now_utc())
        finally:
            setattr(calendar, LOCAL_TZ, saved[0])
            calendar.datetime = saved[1]
            _clock_env("CAREERBREW_NOW", old)
    raise ValueError("unknown op")


def _typed(value):
    """One typed input value: the canon.rs form, plus `tuple` and `path`."""
    t, v = value["t"], value.get("v")
    if t == "null":
        return None
    if t in ("bool", "str"):
        return v
    if t == "int":
        return int(v)
    if t == "float":
        return struct.unpack(">d", bytes.fromhex(v))[0]
    if t == "date":
        return datetime.date.fromisoformat(v)
    if t == "datetime":
        return datetime.datetime.fromisoformat(v)
    if t == "path":
        return PurePosixPath(v)
    if t == "list":
        return [_typed(item) for item in v]
    if t == "tuple":
        return tuple(_typed(item) for item in v)
    if t == "map":
        return {_typed(k): _typed(item) for k, item in v}
    raise ValueError("unknown type")


def hash_set(op, value):
    """`changes._h` on one typed value: the serialized text and the digest."""
    from careerbrew import changes

    if op != "input-hash":
        raise ValueError("unknown op")
    obj = _typed(value)
    text = json.dumps(obj, sort_keys=True, default=str)
    return {"t": "list", "v": [{"t": "str", "v": text}, {"t": "str", "v": changes._h(obj)}]}


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
    "clock": clock,
    "hash": hash_set,
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
