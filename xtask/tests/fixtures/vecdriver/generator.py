"""rust-04a harness control: write one synthetic scratch vector file.

Usage: python -I generator.py <vector-file>. Writes
{"sets": {"<set>": [{"id", "op", "input"}]}}. Synthetic canaries only.
"""

import json
import sys

SETS = {
    "fold": [
        {"id": "ascii", "op": "casefold", "input": "Canary-Alpha"},
        {"id": "sharp-s", "op": "casefold", "input": "STRASSE ß"},
        {"id": "dotted-i", "op": "casefold", "input": "İstanbul-canary"},
    ],
    "split": [
        {"id": "spaces", "op": "split", "input": "one  two\tthree"},
        {"id": "nbsp", "op": "split", "input": "one two three"},
    ],
}


def main() -> None:
    with open(sys.argv[1], "x", encoding="utf-8") as out:
        json.dump({"sets": SETS}, out, sort_keys=True)


if __name__ == "__main__":
    main()
