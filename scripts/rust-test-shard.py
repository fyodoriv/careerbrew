"""Run one share of the Rust workspace tests (the hosted gate's `rust-test` step).

    RUST_TEST_SHARD=i/n python scripts/rust-test-shard.py          run share i of n
    RUST_TEST_SHARD=i/n python scripts/rust-test-shard.py --plan   print every share as JSON, run nothing

`cargo test --all` runs its test binaries one after another, and they take
most of the hosted `native` part (docs/ci.md, "Parallel parts"). This script
splits the workspace's integration-test targets into n shares by their usual
time in `config/rust-test-weights.txt`: the heaviest target first, each to the
share with the least weight so far, like the e2e shares. Targets with one name
in several crates (`component`, `cli`) are one entry and always run together.
A target with no weight line weighs DEFAULT_WEIGHT, so a new test file always
runs in exactly one share. Share 1 also runs every lib and bin unit test and
the doc tests, so together the shares run what `cargo test --all` runs.
"""

from __future__ import annotations

import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
WEIGHTS = ROOT / "config/rust-test-weights.txt"
DEFAULT_WEIGHT = 60
EXTRA = "@share-1-extra"


def shard() -> tuple[int, int]:
    value = os.environ.get("RUST_TEST_SHARD", "")
    m = re.fullmatch(r"([1-9][0-9]*)/([1-9][0-9]*)", value)
    if not m or int(m[1]) > int(m[2]):
        print(f"rust-test-shard: RUST_TEST_SHARD must be i/n with 1 <= i <= n (got: {value!r})", file=sys.stderr)
        sys.exit(2)
    return int(m[1]), int(m[2])


def weights() -> dict[str, int]:
    out: dict[str, int] = {}
    for number, line in enumerate(WEIGHTS.read_text().splitlines(), 1):
        line = line.split("#", 1)[0].strip()
        if not line:
            continue
        parts = line.split()
        if len(parts) != 2 or not parts[1].isdigit():
            sys.exit(f"rust-test-shard: {WEIGHTS.name}:{number}: want '<target> <seconds>', got {line!r}")
        out[parts[0]] = int(parts[1])
    return out


def targets() -> list[str]:
    """Every integration-test target name of the workspace members, once each."""
    meta = subprocess.run(["cargo", "metadata", "--no-deps", "--format-version", "1"], cwd=ROOT, capture_output=True, text=True, check=True).stdout
    data = json.loads(meta)
    members = set(data["workspace_members"])
    return sorted({t["name"] for p in data["packages"] if p["id"] in members for t in p["targets"] if "test" in t["kind"]})


def split(names: list[str], n: int, w: dict[str, int]) -> list[list[str]]:
    load = [0] * n
    load[0] = w.get(EXTRA, 0)
    shares: list[list[str]] = [[] for _ in range(n)]
    for name in sorted(names, key=lambda t: (-w.get(t, DEFAULT_WEIGHT), t)):
        best = min(range(n), key=lambda s: (load[s], s))
        shares[best].append(name)
        load[best] += w.get(name, DEFAULT_WEIGHT)
    return [sorted(s) for s in shares]


def commands(i: int, share: list[str]) -> list[list[str]]:
    tests = [f"--test={t}" for t in share]
    if i == 1:
        # cargo refuses --doc beside other target options, so the doc tests run on their own.
        return [["cargo", "test", "--workspace", "--lib", "--bins", *tests], ["cargo", "test", "--workspace", "--doc"]]
    return [["cargo", "test", "--workspace", *tests]] if share else []


def main() -> int:
    i, n = shard()
    w = weights()
    shares = split(targets(), n, w)
    if "--plan" in sys.argv[1:]:
        print(json.dumps({"shares": shares, "commands": [commands(s, shares[s - 1]) for s in range(1, n + 1)]}))
        return 0
    mine = shares[i - 1]
    weight = sum(w.get(t, DEFAULT_WEIGHT) for t in mine)
    print(f"rust-test-shard: share {i} of {n}: {len(mine)} test target(s), weight {weight} s{', plus lib, bin and doc tests' if i == 1 else ''}")
    # A test that runs the gate in a scratch copy must not inherit the share.
    env = {k: v for k, v in os.environ.items() if k != "RUST_TEST_SHARD"}
    for argv in commands(i, mine):
        print("rust-test-shard: " + " ".join(argv), flush=True)
        result = subprocess.run(argv, cwd=ROOT, env=env, check=False)
        if result.returncode != 0:
            return result.returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
