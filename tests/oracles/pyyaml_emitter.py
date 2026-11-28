"""Oracle for the native PyYAML emitter (`crates/cbcli/src/pyyaml.rs`).

Reads a JSON file of typed values (the `{"t", "v"}` form of
`careerbrew_vault::canon::Canon`) and prints
`yaml.safe_dump(value, sort_keys=False, allow_unicode=True)` of each, the
call `store.write_yaml` makes, as its UTF-8 byte length, a newline, and the
text. Synthetic values only.
"""

from __future__ import annotations

import datetime
import json
import struct
import sys
from typing import Any

import yaml


def _value(typed: dict[str, Any]) -> Any:
    kind, v = typed["t"], typed.get("v")
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
        value = datetime.datetime.fromisoformat(v)
        if value.isoformat() != v:
            raise ValueError("datetime text is not in isoformat() shape")
        return value
    if kind == "list":
        return [_value(item) for item in v]
    if kind == "map":
        return {_value(k): _value(item) for k, item in v}
    raise ValueError(f"unknown type {kind}")


def main() -> None:
    with open(sys.argv[1], encoding="utf-8") as handle:
        cases = json.load(handle)
    for case in cases:
        text = yaml.safe_dump(_value(case), sort_keys=False, allow_unicode=True).encode()
        sys.stdout.buffer.write(b"%d\n" % len(text) + text)


if __name__ == "__main__":
    main()
