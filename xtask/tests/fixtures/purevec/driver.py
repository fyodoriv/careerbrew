"""rust-02 pure value vector oracle: run one vector set with careerbrew's own code.

Usage: python -B driver.py <set> <vectors-file>, or python -B driver.py --all.
A set run prints one JSON line,
{"set": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": true}}]},
in the typed value form `crates/depvec/src/canon.rs` describes. `--all`
prints {"sets": [...]}, the registered set names, for baseline and measure.

Sets: paths; the setting, clock and hash sets come one per pull request.
To add a set: a function below and one line in SETS. A set must not
write outside the synthetic home verify gives it.
"""

import json
import os
import sys
from unittest import mock

from careerbrew import paths


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


# The registry: one line per vector set, sorted by name.
SETS = {
    "paths": paths_set,
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
