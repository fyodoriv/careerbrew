"""rust-04a harness control: a synthetic vector driver.

Usage: python -I driver.py unicode-version, or python -I driver.py <set> <file>.
The tests run it as both the Python and the Rust side, so a clean run agrees
and each planted defect fails by itself.
"""

import json
import sys
import unicodedata

OPS = {"casefold": str.casefold, "split": str.split}


def main() -> None:
    if sys.argv[1:] == ["unicode-version"]:
        print(unicodedata.unidata_version)
        return
    name, path = sys.argv[1], sys.argv[2]
    with open(path, encoding="utf-8") as vectors:
        sets = json.load(vectors)["sets"]
    results = [
        {"id": vector["id"], "output": {"ok": OPS[vector["op"]](vector["input"])}}
        for vector in sets[name]
    ]
    print(json.dumps({"set": name, "results": results}))


if __name__ == "__main__":
    main()
