"""rust-02 pure value vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>, or python -B driver.py --all.
A set run prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes. `--all`
prints {"sets": [...]}, the registered set names, for baseline and measure.

It has no sets yet: the setting, path, clock and hash sets come one per pull
request. To add a set: a function below and one line in SETS. A set must not
write outside the synthetic home verify gives it.
"""

import json
import sys

# The registry: one line per vector set.
SETS = {}


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
