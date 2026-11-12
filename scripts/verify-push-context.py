#!/usr/bin/env python3
"""Validate one Git pre-push context, then run the existing strict leak scan."""

from __future__ import annotations

import os
import re
import subprocess
import sys

MAX_CONTEXT = 1024 * 1024
MAX_REFS = 1000


class InvalidContext(ValueError):
    """The hook did not receive a valid bounded Git context."""


def _git(*args: str) -> subprocess.CompletedProcess[bytes]:
    try:
        return subprocess.run(["git", *args], stdin=subprocess.DEVNULL, capture_output=True, check=False, timeout=30)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise InvalidContext("Git context unavailable") from exc


def _ref(value: bytes) -> bool:
    if not value.startswith(b"refs/"):
        return False
    return _git("check-ref-format", os.fsdecode(value)).returncode == 0


def validate(data: bytes) -> int:
    if len(data) > MAX_CONTEXT:
        raise InvalidContext("context exceeds the limit")
    fields = data.split(b"\0", 2)
    if len(fields) != 3 or not fields[0] or not fields[1] or b"\0" in fields[2]:
        raise InvalidContext("context framing is invalid")
    if any(byte < 32 or byte == 127 for field in fields[:2] for byte in field):
        raise InvalidContext("remote context is invalid")
    payload = fields[2]
    if payload and not payload.endswith(b"\n"):
        raise InvalidContext("ref records are missing or incomplete")
    fmt = _git("rev-parse", "--show-object-format")
    width = {b"sha1\n": 40, b"sha256\n": 64}.get(fmt.stdout) if fmt.returncode == 0 else None
    if width is None:
        raise InvalidContext("object format is unavailable")
    rows = payload[:-1].split(b"\n") if payload else []
    if len(rows) > MAX_REFS:
        raise InvalidContext("too many ref records")
    seen: set[bytes] = set()
    for line in rows:
        parts = line.split(b" ")
        if len(parts) != 4:
            raise InvalidContext("ref record is invalid")
        local, new, remote, old = parts
        if not re.fullmatch(rb"[0-9a-f]{%d}" % width, new) or not re.fullmatch(rb"[0-9a-f]{%d}" % width, old):
            raise InvalidContext("object ID is invalid")
        zero = b"0" * width
        if not _ref(remote) or remote in seen:
            raise InvalidContext("destination ref is invalid or repeated")
        seen.add(remote)
        if new == zero:
            if local != b"(delete)" or old == zero:
                raise InvalidContext("deletion record is invalid")
        elif not local or len(local) > 1024 or local == b"(delete)" or any(byte <= 32 or byte == 127 for byte in local):
            raise InvalidContext("source ref is invalid")
    return len(rows)


def main() -> int:
    try:
        count = validate(sys.stdin.buffer.read(MAX_CONTEXT + 1))
    except InvalidContext:
        print("pre-push context invalid or unavailable; retry through Git", file=sys.stderr)
        return 1
    print(f"pre-push context: {count} ref record(s) checked; target not verified")
    try:
        return subprocess.run(["scripts/leak-check.sh"], stdin=subprocess.DEVNULL, check=False).returncode
    except OSError:
        print("pre-push privacy scan unavailable; repair the repository setup", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
