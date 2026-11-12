"""Small file store and content-hash cache under the data root.

Owner and lifecycle: every function reads and writes files under
`paths.data_root()`; nothing is kept in process memory between calls.
Data records under the data root are encrypted at rest by default (`vault`
names the plaintext folders); read and write them
through `read_text` / `write_text` here, never with `Path.read_text`.
"""

from __future__ import annotations

import datetime
import fcntl
import hashlib
import json
import os
import re
import stat
import time
import uuid
from collections.abc import Callable, Iterable
from pathlib import Path
from typing import Any

import yaml

from . import paths, vault


def _tmp_for(path: Path) -> Path:
    """A temp file unique to this writer, so two processes writing the same file never clash."""
    return path.with_name(f".{path.name}.{os.getpid()}.{uuid.uuid4().hex[:8]}.tmp")


_SAFE_LOADER = getattr(yaml, "CSafeLoader", yaml.SafeLoader)


def read_text(path: Path, default: str | None = None, *, create_key: bool = True) -> str | None:
    if not path.exists():
        return default
    return vault.decode(path.read_bytes(), create_key=create_key)


def read_text_nofollow(path: Path, *, create_key: bool = True) -> str:
    """Read an existing regular data file without following a final symlink."""
    nofollow = getattr(os, "O_NOFOLLOW", 0)
    if not nofollow:
        raise ValueError("safe data reads are unavailable on this platform")
    fd = os.open(path, os.O_RDONLY | os.O_NONBLOCK | nofollow)
    with os.fdopen(fd, "rb") as stream:
        if not stat.S_ISREG(os.fstat(stream.fileno()).st_mode):
            raise ValueError("data file is not a regular file")
        return vault.decode(stream.read(), create_key=create_key)


def write_text(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = _tmp_for(path)
    tmp.write_bytes(vault.encode(path, text))
    tmp.replace(path)


def write_text_if_changed(path: Path, text: str) -> bool:
    """Write only when the text differs from the stored text. Returns True when it wrote.

    A rebuild that a read triggers (the day log, today's plan) then leaves the file and its mtime alone,
    so it does not move the web app's change version (`server.changes_view`) when nothing changed (issue #117).
    """
    try:
        if read_text(path) == text:
            return False
    except vault.VaultError:
        pass  # unreadable with this key: write it again, as before
    write_text(path, text)
    return True


def read_yaml(path: Path, default: Any = None, *, create_key: bool = True) -> Any:
    text = read_text(path) if create_key else read_text(path, create_key=False)
    if text is None:
        return default
    data = yaml.load(text, Loader=_SAFE_LOADER)  # noqa: S506 (a safe loader: C speed when built, else pure Python)
    return default if data is None else data


def write_yaml(path: Path, data: Any) -> None:
    write_text(path, yaml.safe_dump(data, sort_keys=False, allow_unicode=True))


def read_json(path: Path, default: Any = None, *, create_key: bool = True) -> Any:
    text = read_text(path) if create_key else read_text(path, create_key=False)
    return default if text is None else json.loads(text)


def _json_text(data: Any) -> str:
    return json.dumps(data, indent=2, sort_keys=True, default=str)


def write_json(path: Path, data: Any) -> None:
    write_text(path, _json_text(data))


def write_json_if_changed(path: Path, data: Any) -> bool:
    return write_text_if_changed(path, _json_text(data))


def read_jsonl(path: Path, *, create_key: bool = True) -> list[dict[str, Any]]:
    text = (read_text(path) if create_key else read_text(path, create_key=False)) or ""
    return [json.loads(line) for line in text.splitlines() if line.strip()]


def write_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> None:
    write_text(path, "".join(json.dumps(r, sort_keys=True, default=str) + "\n" for r in rows))


def append_jsonl(path: Path, rows: Iterable[dict[str, Any]]) -> int:
    path.parent.mkdir(parents=True, exist_ok=True)
    new = [json.dumps(row, sort_keys=True, default=str) + "\n" for row in rows]
    if not (vault.enabled() and vault.sealable(path)):
        with path.open("a", encoding="utf-8") as fh:
            fh.writelines(new)
        return len(new)
    # A sealed file is one ciphertext: append is read, add, rewrite, under a lock so two writers never lose rows.
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        write_text(path, (read_text(path) or "") + "".join(new))
    return len(new)


def upsert_jsonl(path: Path, rows: Iterable[dict[str, Any]], fields: Iterable[str]) -> tuple[int, int]:
    """Store rows by their `id`: a new id is appended, and a stored id whose `fields` changed is replaced
    by the newer copy (issue #73). An unchanged row is not written. Returns (added, replaced).

    One row per id: the last copy wins, both in the batch and in the file. An id stored twice by older code
    (two writers raced before this lock) collapses to its last stored copy and counts as replaced.

    Runs under the file's lock (the one `append_jsonl` takes for sealed files), so two writers never lose a row.
    """
    keys = tuple(fields)
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.with_name(f".{path.name}.lock").open("a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        old = read_jsonl(path)
        out: dict[str, dict[str, Any]] = {}  # id -> row, in first-seen order
        replaced: set[str] = set()
        for r in old:
            if r["id"] in out:
                replaced.add(r["id"])
            out[r["id"]] = r
        batch: dict[str, dict[str, Any]] = {}
        for row in rows:
            batch[row["id"]] = row
        added = [row for rid, row in batch.items() if rid not in out]
        for rid, row in batch.items():
            if rid in out and any(out[rid].get(k) != row.get(k) for k in keys):
                out[rid] = row
                replaced.add(rid)
        if replaced or (added and vault.enabled() and vault.sealable(path)):
            write_jsonl(path, [*out.values(), *added])
        elif added:
            with path.open("a", encoding="utf-8") as fh:
                fh.writelines(json.dumps(r, sort_keys=True, default=str) + "\n" for r in added)
    return len(added), len(replaced)


_OUTLINE_KEY = re.compile(r"^[A-Za-z_][A-Za-z0-9_-]{0,47}$")
_OUTLINE_MAX_KEYS = 30
_OUTLINE_KINDS = {".yaml": "yaml", ".yml": "yaml", ".json": "json", ".jsonl": "jsonl"}
_SCALARS = {bool: "bool", int: "int", float: "float", str: "str", type(None): "null", datetime.date: "date", datetime.datetime: "datetime"}


def _outline_walk(value: Any, path: str, types: dict[str, dict[str, None]], strict: bool = False, record: bool = True) -> None:
    """Record each key path's value types. Inside a list (`[]`) or a data-keyed map (`*`), sizes merge away.

    `record`: the map is the top of the file, a list item, or a value in a data-keyed map, so its keys are
    likely field names. With `strict`, any other map (one under a named key) prints as `*`: short,
    identifier-shaped keys there are often names (projects, usernames), and nothing tells them from fields.
    """
    merged = "[]" in path or "*" in path
    if isinstance(value, dict):
        # Keys that are not short identifiers, or very many keys, are data (questions, dates, ids), not schema.
        data_keys = (strict and not record) or len(value) > _OUTLINE_MAX_KEYS or not all(isinstance(k, str) and _OUTLINE_KEY.match(k) for k in value)
        if path:
            types.setdefault(path, {})[f"map[{len(value)}]" if data_keys and not merged else "map"] = None
        for k, v in value.items():
            key = "*" if data_keys else str(k)
            _outline_walk(v, f"{path}.{key}" if path else key, types, strict, record=data_keys)
    elif isinstance(value, list):
        types.setdefault(path or "(top)", {})["list" if merged else f"list[{len(value)}]"] = None
        for v in value:
            _outline_walk(v, f"{path}[]", types, strict, record=True)
    else:
        types.setdefault(path or "(top)", {})[_SCALARS.get(type(value), type(value).__name__)] = None


def outline(path: Path, strict: bool = False) -> dict[str, Any]:
    """A data file's structure without one value, for `careerbrew data show --keys`: key paths and value types.

    Reads only. A map keyed by data (free text, dates, more than 30 keys) prints as `*`. Short,
    identifier-shaped names (a project, a username) look like field names, so `strict` (a business
    file shown to an agent) also prints every map under a named key as `*`; key names then come only
    from the top of the file, list items, and records in a data-keyed map. A file that does not parse
    raises ValueError that names the line, never the text.
    """
    text = read_text(path) or ""
    kind = _OUTLINE_KINDS.get(path.suffix.lower(), "text")
    types: dict[str, dict[str, None]] = {}
    count = len(text.splitlines())
    if kind == "jsonl":
        rows = []
        for n, line in enumerate(text.splitlines(), 1):
            if line.strip():
                try:
                    rows.append(json.loads(line))
                except json.JSONDecodeError:
                    raise ValueError(f"cannot parse as jsonl (line {n})") from None
        for row in rows:
            _outline_walk(row, "[]", types, strict)
        count = len(rows)
    elif kind == "json":
        try:
            _outline_walk(json.loads(text), "", types, strict)
        except json.JSONDecodeError as e:
            raise ValueError(f"cannot parse as json (line {e.lineno})") from None
    elif kind == "yaml":
        try:
            data = yaml.load(text, Loader=_SAFE_LOADER)  # noqa: S506 (a safe loader, as in read_yaml)
        except yaml.YAMLError as e:
            mark = getattr(e, "problem_mark", None)
            raise ValueError(f"cannot parse as yaml (line {mark.line + 1})" if mark else "cannot parse as yaml") from None
        if data is not None:
            _outline_walk(data, "", types, strict)
    return {"kind": kind, "sealed": vault.is_sealed(path.read_bytes()[: len(vault.MAGIC)]), "count": count, "keys": [f"{p}: {'|'.join(t)}" for p, t in types.items()]}


def digest(*parts: Any) -> str:
    h = hashlib.sha256()
    for part in parts:
        h.update(json.dumps(part, sort_keys=True, default=str).encode())
        h.update(b"\0")
    return h.hexdigest()[:24]


def cached(key_parts: list[Any], compute: Callable[[], Any], ttl_seconds: float | None = None) -> tuple[Any, bool]:
    """Return (value, hit). The key is a hash of every input; ttl bounds network data."""
    path = paths.sub("cache", digest(*key_parts) + ".json")
    if path.exists():
        entry = read_json(path)
        if ttl_seconds is None or time.time() - entry["at"] < ttl_seconds:
            return entry["value"], True
    value = compute()
    write_json(path, {"at": time.time(), "value": value})
    return value, False
