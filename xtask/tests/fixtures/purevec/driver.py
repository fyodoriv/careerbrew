"""rust-02 pure value vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>, or python -B driver.py --all.
A set run prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes. `--all`
prints {"sets": [...]}, the registered set names, for baseline and measure.

The setting, path, clock and hash sets come one per pull request. To add a set: a function below and one line in SETS. A set must not
write outside the synthetic home verify gives it.
"""

import datetime
import json
import os
import sys
import zoneinfo


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

        saved = calendar.local_tz, calendar.datetime
        calendar.local_tz, calendar.datetime = (lambda: tz), Frozen
        old = _clock_env("CAREERBREW_NOW", data["pinned"])
        try:
            return enc(calendar.now() if op == "now" else calendar.now_utc())
        finally:
            calendar.local_tz, calendar.datetime = saved
            _clock_env("CAREERBREW_NOW", old)
    raise ValueError("unknown op")


# The registry: one line per vector set.
SETS = {
    "clock": clock,
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
