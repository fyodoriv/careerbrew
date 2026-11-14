#!/usr/bin/env python3
"""Validate one Git pre-push context, then run the existing strict leak scan."""

from __future__ import annotations

import os
import re
import subprocess
import sys

MAX_CONTEXT = 1024 * 1024
MAX_REFS = 1000
# The first commit of the public history. A commit that does not descend from it
# comes from a checkout made before the public release (docs/migrate-to-public-history.md).
PUBLIC_ROOT = "22e3e591b70dd8a5f8a1111355201b85c8f68ed5"


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


def old_history(data: bytes) -> int:
    """Count pushed refs in a validated context whose history is not only the public history."""
    root = os.environ.get("CAREERBREW_PUBLIC_ROOT") or PUBLIC_ROOT  # the override is for tests with scratch repositories
    if not re.fullmatch(r"[0-9a-f]{40,64}", root):
        raise InvalidContext("public root is invalid")
    payload = data.split(b"\0", 2)[2]
    found = 0
    for line in payload.splitlines():
        new = os.fsdecode(line.split(b" ")[1])
        if set(new) == {"0"}:
            continue  # a deletion pushes no commit
        if _git("cat-file", "-e", new).returncode != 0:
            continue  # not a local object: Git itself refuses to push it
        if _git("cat-file", "-e", f"{new}^{{commit}}").returncode != 0:
            found += 1  # a tag of a tree or a file: its history cannot be checked
            continue
        # Every first commit under the tip must be the public one: a merge of both histories descends from it too.
        roots = _git("rev-list", "--max-parents=0", new)
        if roots.returncode != 0 or set(os.fsdecode(roots.stdout).split()) != {root}:
            found += 1
    return found


def pushed_ranges(data: bytes) -> list[str]:
    """The `git log` range of each pushed ref in a validated context: the commits this push adds."""
    remote, _, payload = data.split(b"\0", 2)
    name = os.fsdecode(remote)
    # Commits that a branch of this remote already holds were scanned when they were pushed.
    known = f" --not --remotes={name}" if re.fullmatch(r"[A-Za-z0-9._-]+", name) else ""
    ranges = []
    for line in payload.splitlines():
        _, new, _, old = (os.fsdecode(part) for part in line.split(b" "))
        if set(new) == {"0"} or _git("cat-file", "-e", new).returncode != 0:
            continue  # a deletion, or an object Git itself refuses to push
        if set(old) != {"0"} and _git("cat-file", "-e", f"{old}^{{commit}}").returncode == 0:
            ranges.append(f"{old}..{new}")
        else:
            ranges.append(f"{new}{known}")  # a new ref, or a remote tip this checkout does not hold
    return ranges


def secret_scan(ranges: list[str]) -> int:
    """Run gitleaks on every pushed commit, not only the newest ones. Findings are redacted."""
    for commits in ranges:
        # gitleaks exits 0 when `git log` fails, so prove first that Git can read the range.
        if _git("rev-list", "--quiet", *commits.split(" ")).returncode != 0:
            print("pre-push secret scan unavailable: Git cannot read a pushed range; repair the repository", file=sys.stderr)
            return 1
        try:
            done = subprocess.run(
                # -m: `git log -p` prints no diff for a merge commit without it, and a merge can add a secret.
                ["gitleaks", "git", "--no-banner", "--redact", "-l", "warn", f"--log-opts=-m {commits}"],
                stdin=subprocess.DEVNULL,
                check=False,
                timeout=300,
            )
        except FileNotFoundError:
            print("gitleaks missing (fix - brew install gitleaks)", file=sys.stderr)
            return 1
        except (OSError, subprocess.TimeoutExpired):
            print("pre-push secret scan unavailable; repair the repository setup", file=sys.stderr)
            return 1
        if done.returncode != 0:
            print("pre-push: gitleaks found a secret in a pushed commit, or could not scan it. Remove the commit from the branch; never push it", file=sys.stderr)
            return 1
    return 0


def main() -> int:
    try:
        data = sys.stdin.buffer.read(MAX_CONTEXT + 1)
        count = validate(data)
        old = old_history(data)
    except InvalidContext:
        print("pre-push context invalid or unavailable; retry through Git", file=sys.stderr)
        return 1
    if old:
        print(
            f"pre-push: {old} pushed ref(s) do not descend from the public first commit. This checkout holds the history from before the public release (or is a shallow clone). Do not push it. Fix: follow docs/migrate-to-public-history.md",
            file=sys.stderr,
        )
        return 1
    print(f"pre-push context: {count} ref record(s) checked; target not verified")
    try:
        leaks = subprocess.run(["scripts/leak-check.sh"], stdin=subprocess.DEVNULL, check=False).returncode
    except OSError:
        print("pre-push privacy scan unavailable; repair the repository setup", file=sys.stderr)
        return 1
    if leaks:
        return leaks
    try:
        return secret_scan(pushed_ranges(data))
    except InvalidContext:
        print("pre-push context invalid or unavailable; retry through Git", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
