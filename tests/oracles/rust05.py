"""rust-05 Python oracle: real storage, receipt and CLI providers, synthetic adapters only.

`--all` runs one stateful workload in the invocation's synthetic CAREERBREW_HOME.
The output contains case ids, exit codes, typed values and digests, never stored
text, a key or a secret. The native `vault-store --all` candidate emits the same
schema. No terminal, Keychain, `op`, network or owner data is read.
"""

from __future__ import annotations

import contextlib
import datetime
import hashlib
import io
import json
import os
import pwd
import socket
import stat
import subprocess
import sys
import types
from pathlib import Path
from typing import Any

from careerbrew import approvals, cli, config_io, events, keyfile, paths, secrets, store, vault

CANARY = "SYNTHETIC-RUST05-CANARY-7Q9"
SECRET = "SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE"


def _sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, float):
        return {"float": value.hex()}
    if isinstance(value, datetime.datetime):
        return {"datetime": value.isoformat()}
    if isinstance(value, datetime.date):
        return {"date": value.isoformat()}
    if isinstance(value, tuple):
        return {"tuple": [_typed(item) for item in value]}
    if isinstance(value, list):
        return [_typed(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _typed(item) for key, item in sorted(value.items(), key=lambda pair: str(pair[0]))}
    return {"type": type(value).__name__}


@contextlib.contextmanager
def _patched(module: Any, **attrs: Any):
    old = {name: getattr(module, name) for name in attrs}
    for name, value in attrs.items():
        setattr(module, name, value)
    try:
        yield
    finally:
        for name, value in old.items():
            setattr(module, name, value)


def _error(exc: BaseException) -> dict[str, str]:
    return {"error": type(exc).__name__}


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> Any:
    try:
        value = fn()
    except Exception as exc:
        cases.append({"id": name, "result": _error(exc)})
        return None
    cases.append({"id": name, "result": _typed(value)})
    return value


def _identity(path: Path) -> dict[str, Any]:
    info = path.stat()
    return {
        "sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
        "inode": info.st_ino,
        "mtime_ns": info.st_mtime_ns,
        "mode": stat.S_IMODE(info.st_mode),
    }


def _file_evidence(root: Path, key_file: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for path in sorted((p for p in root.rglob("*") if p.is_file() and not p.is_symlink()), key=lambda p: p.relative_to(root).as_posix()):
        rel = path.relative_to(root).as_posix()
        if rel.endswith((".lock", ".tmp")):
            continue
        raw = path.read_bytes()
        row: dict[str, Any] = {
            "path": rel,
            "mode": f"{stat.S_IMODE(path.stat().st_mode):03o}",
            "sealed": raw.startswith(vault.MAGIC),
        }
        if path == key_file:
            row["key"] = "present"
        else:
            try:
                text = store.read_text(path, create_key=False) or ""
            except UnicodeDecodeError:
                row["raw_sha256"] = hashlib.sha256(raw).hexdigest()
                row["text"] = "non-utf8"
            else:
                row["text_sha256"] = _sha(text)
                row["utf8_bytes"] = len(text.encode("utf-8"))
        rows.append(row)
    return rows


def _cli_case(cases: list[dict[str, Any]], name: str, args: types.SimpleNamespace, fn: Any, *, tty: bool) -> None:
    output = io.StringIO()
    stdin = types.SimpleNamespace(isatty=lambda: tty)
    with _patched(cli.sys, stdin=stdin), contextlib.redirect_stdout(output):
        code = fn(args)
    text = output.getvalue()
    if CANARY in text or SECRET in text:
        raise RuntimeError("synthetic canary reached CLI output")
    cases.append({"id": name, "exit": code, "stdout_sha256": _sha(text)})


def _nonfinite_numbers() -> dict[str, Any]:
    return {
        "nan": float("nan"),
        "infinity": float("inf"),
        "negative_infinity": float("-inf"),
        "negative_zero": -0.0,
        "exponent": 1e-7,
        "high_scientific": 1e16,
        "literal": "NaN Infinity -Infinity",
        "big": 12089258196146291747061791208925819614629174706179,
    }


def _cached_result(pair: tuple[Any, bool]) -> dict[str, Any]:
    value, hit = pair
    return {"hit": hit, "sha256": _sha(json.dumps(value, sort_keys=True)), "numbers": _typed(value["numbers"])}


def _outline_and_cache_cases(cases: list[dict[str, Any]], root: Path) -> None:
    projects = root / "work" / "projects.yaml"
    store.write_text(
        projects,
        f"""projects:
  synthetic-project:
    created_at: '2026-10-01T09:00:00Z'
    stale_after_days: 7
    sources:
    - kind: doc
      ref: r1
  synthetic-project-two:
    created_at: '2026-10-02T09:00:00Z'
    owner: me
people:
  {CANARY}: 3
""",
    )
    _call(cases, "outline-projects", lambda: store.outline(projects, strict=True))

    state = root / "tutor" / "state.json"
    store.write_text(
        state,
        json.dumps(
            {
                "cards": {
                    "What is a heap? (explain it to a junior)": {"due": "2026-10-23", "reps": 2},
                    CANARY: {"due": "2026-10-24", "reps": 1, "lapses": 0},
                },
                "by_id": {f"t{i:03d}": i for i in range(40)},
                "rows": [[1, "a"], [2, None]],
            }
        ),
    )
    _call(cases, "outline-json", lambda: store.outline(state))

    evidence = root / "work" / "evidence.jsonl"
    store.write_text(
        evidence,
        "".join(json.dumps(row) + "\n" for row in ({"title": CANARY, "tags": ["delivery"]}, {"title": "second", "at": 3})),
    )
    _call(cases, "outline-jsonl", lambda: store.outline(evidence, strict=True))

    day = root / "day" / "log" / "2026-10-20.md"
    store.write_text(day, f"# Day\n\nShipped {CANARY}\n")
    _call(cases, "outline-text", lambda: store.outline(day, strict=True))

    bad_json = root / "work" / "bad-outline.json"
    store.write_text(bad_json, '{"x":\n')
    _call(cases, "outline-malformed-json", lambda: store.outline(bad_json, strict=True))
    bad_jsonl = root / "work" / "bad-outline.jsonl"
    store.write_text(bad_jsonl, '{"x": 1}\nnot-json\n')
    _call(cases, "outline-malformed-jsonl", lambda: store.outline(bad_jsonl, strict=True))
    bad_yaml = root / "work" / "bad-outline.yaml"
    store.write_text(bad_yaml, f"rules:\n  - match: {CANARY}\n    bad: [unclosed\n")
    _call(cases, "outline-malformed-yaml", lambda: store.outline(bad_yaml, strict=True))

    nonfinite_path = root / "work" / "nonfinite.json"
    nonfinite_value = _nonfinite_numbers()
    _call(cases, "json-nonfinite-write", lambda: store.write_json(nonfinite_path, nonfinite_value))
    _call(cases, "json-nonfinite-read", lambda: store.read_json(nonfinite_path))
    nonfinite_before = _identity(nonfinite_path)
    _call(
        cases,
        "json-nonfinite-noop",
        lambda: store.write_json_if_changed(nonfinite_path, store.read_json(nonfinite_path)),
    )
    cases.append({"id": "json-nonfinite-noop-identity", "result": nonfinite_before == _identity(nonfinite_path)})
    _call(cases, "outline-nonfinite", lambda: store.outline(nonfinite_path, strict=True))

    key_48 = "k" * 48
    for case_id, key in [
        ("outline-key-48", key_48),
        ("outline-key-48-newline", key_48 + "\n"),
        ("outline-key-48-two-newlines", key_48 + "\n\n"),
        ("outline-key-49", key_48 + "k"),
        ("outline-key-49-newline", key_48 + "k\n"),
    ]:
        key_path = root / "work" / f"{case_id}.json"
        store.write_text(key_path, json.dumps({key: 1}))
        _call(cases, case_id, lambda path=key_path: store.outline(path)["keys"])

    _call(
        cases,
        "digest-values",
        lambda: store.digest(
            "synthetic",
            {"b": 2, "a": "café", "nan": float("nan"), "infinity": float("inf")},
            [1, None, True, 1e-7, -0.0],
            datetime.date(2026, 3, 1),
            datetime.datetime(2026, 3, 1, 9, 30, tzinfo=datetime.UTC),
        ),
    )
    _call(cases, "digest-empty", store.digest)

    key_parts = ["rust05h", {"b": 2, "a": "café"}, 3]
    cache_path = root / "cache" / f"{store.digest(*key_parts)}.json"

    def unexpected_compute() -> Any:
        raise AssertionError("a fresh cached value recomputed")

    with _patched(store.time, time=lambda: 100.0):
        missing = store.cached(key_parts, lambda: {"title": CANARY, "count": 1, "numbers": _nonfinite_numbers()})
    _call(cases, "cached-miss", lambda: _cached_result(missing))
    cache_before = _identity(cache_path)
    with _patched(store.time, time=lambda: 100.0):
        hit = store.cached(key_parts, unexpected_compute)
    _call(cases, "cached-hit", lambda: _cached_result(hit))
    cases.append({"id": "cached-hit-identity", "result": cache_before == _identity(cache_path)})
    with _patched(store.time, time=lambda: 105.0):
        fresh = store.cached(key_parts, unexpected_compute, ttl_seconds=10)
    _call(cases, "cached-fresh", lambda: _cached_result(fresh))
    with _patched(store.time, time=lambda: 111.5):
        expired = store.cached(key_parts, lambda: {"title": CANARY, "count": 2, "numbers": _nonfinite_numbers()}, ttl_seconds=10)
    _call(cases, "cached-expired", lambda: _cached_result(expired))


def _key_path_cases(cases: list[dict[str, Any]]) -> None:
    original = os.environ.get("CAREERBREW_KEY_FILE")
    user = pwd.getpwuid(os.getuid())
    expected_home = Path.home() / ".config" / "careerbrew" / "backup.key"
    rows = [
        ("key-path-home", "~", Path.home()),
        ("key-path-home-file", "~/backup.key", Path.home() / "backup.key"),
        ("key-path-home-repeated-slash", "~//backup.key", Path.home() / "backup.key"),
        ("key-path-home-repeated-slashes", "~////nested//backup.key", Path.home() / "nested" / "backup.key"),
        ("key-path-home-traversal-lexical", "~//../backup.key", Path.home() / ".." / "backup.key"),
        ("key-path-empty-default", "", expected_home),
        ("key-path-current-user", f"~{user.pw_name}", Path(user.pw_dir)),
        ("key-path-current-user-file", f"~{user.pw_name}/backup.key", Path(user.pw_dir) / "backup.key"),
        ("key-path-current-user-repeated-slash", f"~{user.pw_name}//backup.key", Path(user.pw_dir) / "backup.key"),
        (
            "key-path-current-user-repeated-slashes",
            f"~{user.pw_name}///nested//backup.key",
            Path(user.pw_dir) / "nested" / "backup.key",
        ),
        (
            "key-path-current-user-traversal-lexical",
            f"~{user.pw_name}//../backup.key",
            Path(user.pw_dir) / ".." / "backup.key",
        ),
        (
            "key-path-missing-user",
            "~careerbrew-rust05h-no-such-user-7q9/backup.key",
            Path("~careerbrew-rust05h-no-such-user-7q9/backup.key"),
        ),
    ]
    try:
        for name, value, expected in rows:
            os.environ["CAREERBREW_KEY_FILE"] = value
            _call(cases, name, lambda expected=expected: keyfile.key_path() == expected)
    finally:
        if original is None:
            os.environ.pop("CAREERBREW_KEY_FILE", None)
        else:
            os.environ["CAREERBREW_KEY_FILE"] = original


def _snapshot(root: Path) -> dict[str, tuple[bytes, int]]:
    return {str(path.relative_to(root)): (path.read_bytes(), path.stat().st_mtime_ns) for path in root.rglob("*") if path.is_file()}


def _data_show_cases(cases: list[dict[str, Any]], root: Path) -> None:
    store.write_yaml(root / "profile.yaml", {"current_level": 3})
    store.write_yaml(root / "cache" / "evidence" / "index.yaml", {"count": 7, "note": "synthetic-outline-value-366"})
    store.write_yaml(root / "private" / "nested.yaml", {"nested_only": 11})
    store.write_yaml(root / "privateish" / "literal.yaml", {"literal_only": 13})
    outside = root.parent / "outside.yaml"
    outside.write_text("outside: synthetic-outline-value-366\n", encoding="utf-8")
    (root / "escape.yaml").symlink_to(outside)

    def show(name: Any, *, keys: bool = False) -> dict[str, Any]:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = cli.main(["data", "show", str(name), *(["--keys"] if keys else [])])
        text = output.getvalue().replace(str(root), "<data>").replace(str(outside), "<outside>")
        if any(term in text for term in (CANARY, SECRET, "synthetic-outline-value-366")):
            raise RuntimeError("synthetic value reached data-show output")
        return {"exit": code, "stdout": text}

    def result(reference: Any, alias: Any, passed: bool) -> dict[str, Any]:
        return {"reference": reference, "alias": alias, "passed": passed}

    attempts = []

    def deny(channel: str):
        def denied(*_args: Any, **_kwargs: Any) -> None:
            attempts.append(channel)
            raise AssertionError("a host transport or secret resolver was called")

        return denied

    was_events = os.environ.get("CAREERBREW_EVENTS")
    os.environ["CAREERBREW_EVENTS"] = "off"
    try:
        with (
            _patched(subprocess, run=deny("process"), Popen=deny("process")),
            _patched(socket, create_connection=deny("socket")),
            _patched(socket.socket, connect=deny("socket"), connect_ex=deny("socket"), sendto=deny("socket")),
            _patched(secrets, resolve=deny("secret")),
        ):
            before = _snapshot(root.parent)
            reference = show("profile.yaml")
            alias = show("private/profile.yaml")
            cases.append(
                {
                    "id": "data-show-P01-personal",
                    "result": result(reference, alias, reference == {"exit": 0, "stdout": "current_level: 3\n"} and alias == reference),
                }
            )
            reference = show("cache/evidence/index.yaml", keys=True)
            alias = show("private/cache/evidence/index.yaml", keys=True)
            cases.append(
                {
                    "id": "data-show-P02-cache-keys",
                    "result": result(
                        reference,
                        alias,
                        reference["exit"] == 0 and reference["stdout"].splitlines()[1:] == ["count: int", "note: str"] and alias == reference,
                    ),
                }
            )
            reference = show("cache/evidence/index.yaml")
            alias = show("private/cache/evidence/index.yaml")
            cases.append(
                {
                    "id": "data-show-P03-cache-refusal",
                    "result": result(reference, alias, reference["exit"] == 3 and "--keys" in reference["stdout"] and alias == reference),
                }
            )
            alias = show("private/private/nested.yaml", keys=True)
            lines = alias["stdout"].splitlines()
            cases.append(
                {
                    "id": "data-show-P04-exact-once",
                    "result": result(
                        None,
                        alias,
                        alias["exit"] == 0 and lines[0].startswith("private/nested.yaml: yaml, encrypted on disk") and lines[1:] == ["nested_only: int"] and "11" not in alias["stdout"],
                    ),
                }
            )

            controls = {
                "C01-personal": show("profile.yaml"),
                "C02-cache-keys": show("cache/evidence/index.yaml", keys=True),
                "C03-cache-refusal": show("cache/evidence/index.yaml"),
                "C04-missing": show("private/missing.yaml"),
                "C05-directory": show("private/cache"),
                "C06-traversal": show("private/../outside.yaml"),
                "C07-symlink": show("private/escape.yaml"),
                "C08-absolute-inside": show(root / "profile.yaml"),
                "C09-absolute-outside": show(outside),
                "C12-root-directory": show("private/"),
            }
            passed = {
                "C01-personal": controls["C01-personal"] == {"exit": 0, "stdout": "current_level: 3\n"},
                "C02-cache-keys": controls["C02-cache-keys"]["exit"] == 0 and controls["C02-cache-keys"]["stdout"].splitlines()[1:] == ["count: int", "note: str"],
                "C03-cache-refusal": controls["C03-cache-refusal"]["exit"] == 3 and "--keys" in controls["C03-cache-refusal"]["stdout"],
                "C08-absolute-inside": controls["C08-absolute-inside"] == {"exit": 0, "stdout": "current_level: 3\n"},
            }
            for name in ("C04-missing", "C05-directory", "C06-traversal", "C07-symlink", "C09-absolute-outside", "C12-root-directory"):
                passed[name] = controls[name]["exit"] == 1 and "no such data file" in controls[name]["stdout"]
            for name, actual in controls.items():
                cases.append({"id": f"data-show-{name}", "result": {"actual": actual, "passed": passed[name]}})

            keys = show("privateish/literal.yaml", keys=True)
            hidden = show("privateish/literal.yaml")
            lookalike = keys["exit"] == 0 and keys["stdout"].splitlines()[1:] == ["literal_only: int"] and "13" not in keys["stdout"] and hidden["exit"] == 3 and "--keys" in hidden["stdout"] and "literal_only" not in hidden["stdout"]
            cases.append({"id": "data-show-C11-lookalike", "result": {"keys": keys, "hidden": hidden, "passed": lookalike}})
            cases.append({"id": "data-show-read-only", "result": before == _snapshot(root.parent)})
            read_only_attempts = len(attempts)
            if read_only_attempts:
                raise AssertionError("data show reached a denied host adapter")

            def planted(channel: str, operation: Any) -> dict[str, Any]:
                before_attempts = attempts.count(channel)
                try:
                    operation()
                except AssertionError:
                    refused = True
                else:
                    refused = False
                return {"attempts": attempts.count(channel) - before_attempts, "refused": refused}

            cases.append(
                {
                    "id": "data-show-host-denial",
                    "result": {
                        "read_only_attempts": read_only_attempts,
                        "process": planted("process", lambda: subprocess.run(["synthetic-command"])),
                        "socket": planted("socket", lambda: socket.create_connection(("127.0.0.1", 9))),
                        "secret": planted("secret", lambda: secrets.resolve("env:SYNTHETIC_HOST_DENIAL")),
                    },
                }
            )
    finally:
        if was_events is None:
            os.environ.pop("CAREERBREW_EVENTS", None)
        else:
            os.environ["CAREERBREW_EVENTS"] = was_events


@contextlib.contextmanager
def _bulk_home(base: Path, name: str, *, encrypt: str = "on"):
    root = base / name / "data"
    root.mkdir(parents=True, exist_ok=True)
    key_file = base / name / "keys" / "backup.key"
    names = ("CAREERBREW_HOME", "CAREERBREW_KEY_FILE", "CAREERBREW_ENCRYPT")
    old = {name: os.environ.get(name) for name in names}
    os.environ["CAREERBREW_HOME"] = str(root)
    os.environ["CAREERBREW_KEY_FILE"] = str(key_file)
    os.environ["CAREERBREW_ENCRYPT"] = encrypt
    vault._fernet.cache_clear()
    try:
        yield root, key_file
    finally:
        for name, value in old.items():
            if value is None:
                os.environ.pop(name, None)
            else:
                os.environ[name] = value
        vault._fernet.cache_clear()


def _bulk_snapshot(root: Path) -> list[dict[str, Any]]:
    rows = []
    for path in sorted(root.rglob("*"), key=lambda item: item.relative_to(root).as_posix()):
        relative = path.relative_to(root).as_posix()
        if path.is_symlink():
            rows.append({"path": relative, "symlink": True})
            continue
        if not path.is_file():
            continue
        raw = path.read_bytes()
        row: dict[str, Any] = {
            "path": relative,
            "mode": f"{stat.S_IMODE(path.stat().st_mode):03o}",
            "sealed": vault.is_sealed(raw),
        }
        try:
            plain = vault.open_bytes(raw, create_key=False)
        except Exception as exc:
            row["read_error"] = type(exc).__name__
        else:
            if not row["sealed"] and vault._is_fernet_token(path):
                row["bundle"] = "fernet"
            else:
                row["sha256"] = hashlib.sha256(plain).hexdigest()
        rows.append(row)
    return rows


def _tree_identities(root: Path) -> dict[str, Any]:
    return {path.relative_to(root).as_posix(): _identity(path) for path in root.rglob("*") if path.is_file() and not path.is_symlink()}


def _record_bulk(cases: list[dict[str, Any]], name: str, root: Path, key_file: Path, operation: Any, *, before: dict[str, Any] | None = None) -> None:
    try:
        result = _typed(operation())
    except Exception as exc:
        result = _error(exc)
    row: dict[str, Any] = {
        "id": name,
        "result": result,
        "state": _bulk_snapshot(root),
        "key": {
            "present": key_file.exists(),
            "mode": f"{stat.S_IMODE(key_file.stat().st_mode):03o}" if key_file.exists() else None,
        },
    }
    if before is not None:
        row["mutation_free"] = before == _tree_identities(root)
    cases.append(row)


def _plain_file(path: Path, text: str = "synthetic bulk record\n", *, mode: int = 0o640) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    path.chmod(mode)
    return path


def _sealed_file(path: Path, raw: bytes = b"synthetic bulk record\n", *, mode: int = 0o640) -> Path:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(vault.seal_bytes(raw))
    path.chmod(mode)
    return path


def _bulk_cases(cases: list[dict[str, Any]], data_root: Path) -> None:
    base = data_root.parent / "bulk"
    with _bulk_home(base, "enumeration") as (root, key_file):
        plain = _plain_file(root / "work/plain.txt")
        sealed = _sealed_file(root / "work/sealed.txt")
        near = _plain_file(root / "learn/synthetic/replay/07/near.txt")
        checkout = root / "learn/synthetic/replay/7"
        _plain_file(checkout / ".git/HEAD", "ref: refs/heads/main\n")
        _plain_file(checkout / "code.py", "value = 1\n")
        for folder in ("logs", "out", ".edit", "plugins"):
            _plain_file(root / folder / "plain.txt")
        _plain_file(root / "work/ignored.lock")
        _plain_file(root / "work/ignored.tmp")
        (root / "work/link.txt").symlink_to(plain)
        expected_paths = sorted(path.relative_to(root).as_posix() for path in (near, plain, sealed))
        _record_bulk(
            cases,
            "bulk-enumeration-status",
            root,
            key_file,
            lambda: {"paths": sorted(path.relative_to(root).as_posix() for path in vault.files()), "status": vault.status()},
        )
        os.environ["CAREERBREW_ENCRYPT"] = "off"
        _record_bulk(cases, "bulk-status-environment-off", root, key_file, vault.status)
        os.environ["CAREERBREW_ENCRYPT"] = "on"
        _plain_file(root / vault.MARKER, "synthetic existing marker\n")
        plain_before = _identity(plain)
        _record_bulk(
            cases,
            "bulk-marker-disables-seal-files",
            root,
            key_file,
            lambda: {"count": vault.seal_files([plain]), "unchanged": plain_before == _identity(plain), "status": vault.status()},
        )
        assert expected_paths == sorted(path.relative_to(root).as_posix() for path in vault.files())

    with _bulk_home(base, "seal-files") as (root, key_file):
        first = _plain_file(root / "work/first.txt")
        sealed = _sealed_file(root / "work/already.txt")
        log = _plain_file(root / "logs/plain.txt")
        missing = root / "work/missing.txt"
        outside = _plain_file(base / "seal-files/outside.txt")
        link = root / "work/link.txt"
        link.symlink_to(first)
        sealed_before = _identity(sealed)
        _record_bulk(
            cases,
            "bulk-seal-files",
            root,
            key_file,
            lambda: {
                "count": vault.seal_files([first, sealed, log, missing, root / "work", outside, link]),
                "sealed_noop": sealed_before == _identity(sealed),
            },
        )

    with _bulk_home(base, "encrypt-all") as (root, key_file):
        first = _plain_file(root / "work/first.txt")
        _plain_file(root / "work/second.txt")
        sealed = _sealed_file(root / "work/already.txt")
        log = _plain_file(root / "logs/plain.txt")
        replay = _plain_file(root / "learn/synthetic/replay/7/code.py")
        _plain_file(root / "learn/synthetic/replay/7/.git/HEAD", "ref: refs/heads/main\n")
        _plain_file(root / vault.MARKER, "synthetic existing marker\n")
        sealed_before = _identity(sealed)
        _record_bulk(
            cases,
            "bulk-encrypt-all",
            root,
            key_file,
            lambda: {"count": vault.encrypt_all(), "sealed_noop": sealed_before == _identity(sealed)},
        )
        identities = _tree_identities(root)
        _record_bulk(
            cases,
            "bulk-encrypt-all-noop",
            root,
            key_file,
            lambda: {"count": vault.encrypt_all(), "unchanged": identities == _tree_identities(root)},
        )
        assert not log.read_bytes().startswith(vault.MAGIC) and not replay.read_bytes().startswith(vault.MAGIC)

    for kind in ("backup-suffix", "sync-suffix", "raw-fernet"):
        with _bulk_home(base, f"bundle-{kind}") as (root, key_file):
            _plain_file(root / "record.txt")
            _plain_file(root / vault.MARKER, "synthetic existing marker\n")
            if kind == "raw-fernet":
                bundle = root / "bundle-without-extension"
                bundle.write_bytes(vault.seal_bytes(b"synthetic bundle")[len(vault.MAGIC) :])
            else:
                bundle = root / ("archive.CBK" if kind == "backup-suffix" else "archive.CbSyNc")
                bundle.write_text("synthetic named bundle\n", encoding="utf-8")
            before = _tree_identities(root)
            _record_bulk(cases, f"bulk-encrypt-refuses-{kind}", root, key_file, vault.encrypt_all, before=before)

    with _bulk_home(base, "encrypt-sealed-missing-key") as (root, key_file):
        _sealed_file(root / "old.txt")
        _plain_file(root / "new.txt")
        _plain_file(root / vault.MARKER, "synthetic existing marker\n")
        key_file.unlink()
        vault._fernet.cache_clear()
        _record_bulk(cases, "bulk-encrypt-sealed-missing-key", root, key_file, vault.encrypt_all)

    with _bulk_home(base, "encrypt-invalid-key") as (root, key_file):
        _plain_file(root / "record.txt")
        _plain_file(root / vault.MARKER, "synthetic existing marker\n")
        key_file.parent.mkdir(parents=True, exist_ok=True)
        key_file.write_bytes(b"not-a-valid-fernet-key")
        vault._fernet.cache_clear()
        _record_bulk(cases, "bulk-encrypt-invalid-key", root, key_file, vault.encrypt_all)

    with _bulk_home(base, "decrypt-all") as (root, key_file):
        _sealed_file(root / "work/sealed.txt")
        _plain_file(root / "work/plain.txt")
        _sealed_file(root / "learn/synthetic/replay/7/code.py", b"value = 1\n")
        _sealed_file(root / "learn/synthetic/replay/7/.git/HEAD", b"ref: refs/heads/main\n")
        _plain_file(root / "logs/plain.txt")
        _record_bulk(cases, "bulk-decrypt-all", root, key_file, vault.decrypt_all)
        identities = _tree_identities(root)
        _record_bulk(
            cases,
            "bulk-decrypt-all-noop",
            root,
            key_file,
            lambda: {"count": vault.decrypt_all(), "data_unchanged": all(identities[path] == _tree_identities(root)[path] for path in identities if path != vault.MARKER)},
        )

    for kind in ("missing-key", "invalid-key", "wrong-key"):
        with _bulk_home(base, f"decrypt-{kind}") as (root, key_file):
            _sealed_file(root / "record.txt")
            if kind == "missing-key":
                key_file.unlink()
            elif kind == "invalid-key":
                key_file.write_bytes(b"not-a-valid-fernet-key")
            else:
                key_file.write_bytes(b"AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=")
            vault._fernet.cache_clear()
            before = _tree_identities(root)
            _record_bulk(cases, f"bulk-decrypt-{kind}", root, key_file, vault.decrypt_all, before=before)

    for operation_name in ("seal-files", "encrypt-all", "decrypt-all"):
        with _bulk_home(base, f"failure-{operation_name}") as (root, key_file):
            targets = [root / f"work/{name}.txt" for name in ("a", "b", "c")]
            for target in targets:
                if operation_name == "decrypt-all":
                    _sealed_file(target)
                else:
                    _plain_file(target)
            if operation_name == "encrypt-all":
                _plain_file(root / vault.MARKER, "synthetic existing marker\n")
            real_rewrite = vault._rewrite

            def failing_rewrite(path: Path, raw: bytes) -> None:
                if path == targets[1]:
                    raise OSError("planted bulk rewrite failure")
                real_rewrite(path, raw)

            if operation_name == "seal-files":

                def operation():
                    return vault.seal_files(targets)

                patches = _patched(vault, _rewrite=failing_rewrite)
            else:
                operation = vault.encrypt_all if operation_name == "encrypt-all" else vault.decrypt_all
                selected = (lambda: list(targets)) if operation_name == "encrypt-all" else (lambda *, include_replay=False: list(targets))
                patches = contextlib.ExitStack()
                patches.enter_context(_patched(vault, files=selected) if operation_name == "encrypt-all" else _patched(vault, _files=selected))
                patches.enter_context(_patched(vault, _rewrite=failing_rewrite))
            with patches:
                _record_bulk(cases, f"bulk-{operation_name}-failure-atomicity", root, key_file, operation)


def _run_all() -> dict[str, Any]:
    root = paths.data_root()
    key_file = root.parent / "keys" / "backup.key"
    os.environ["CAREERBREW_KEY_FILE"] = str(key_file)
    os.environ["CAREERBREW_NOW"] = "2026-03-01T09:30:00+00:00"
    os.environ["CAREERBREW_EVENTS"] = "on"
    os.environ["CAREERBREW_ENCRYPT"] = "on"
    cases: list[dict[str, Any]] = []

    missing = root / "missing.json"
    _call(cases, "json-missing-default", lambda: store.read_json(missing, {"count": 0, "ok": True}))
    _call(cases, "yaml-missing-default", lambda: store.read_yaml(root / "missing.yaml", [1, "yes"]))
    _call(cases, "jsonl-missing-empty", lambda: store.read_jsonl(root / "missing.jsonl"))

    text_path = root / "notes" / "synthetic.txt"
    original = f" café / 東京\n{CANARY}\n"
    _call(cases, "text-seal", lambda: store.write_text(text_path, original))
    _call(cases, "text-read-digest", lambda: {"sha256": _sha(store.read_text(text_path) or "")})
    before = _identity(text_path)
    _call(cases, "text-noop", lambda: store.write_text_if_changed(text_path, original))
    after = _identity(text_path)
    cases.append({"id": "text-noop-identity", "result": before == after})

    json_path = root / "record.json"
    json_value = {
        "big": 12089258196146291747061791208925819614629174706179,
        "date": datetime.date(2026, 3, 1),
        "at": datetime.datetime(2026, 3, 1, 9, 30, tzinfo=datetime.UTC),
        "nested": {"yes": "yes", "nothing": None},
    }
    _call(cases, "json-write", lambda: store.write_json(json_path, json_value))
    _call(cases, "json-read", lambda: store.read_json(json_path))
    json_before = _identity(json_path)
    _call(cases, "json-noop", lambda: store.write_json_if_changed(json_path, store.read_json(json_path)))
    cases.append({"id": "json-noop-identity", "result": json_before == _identity(json_path)})

    yaml_path = root / "record.yaml"
    yaml_value = {"at": json_value["at"], "date": json_value["date"], "literal": "yes", "count": 8}
    _call(cases, "yaml-write", lambda: store.write_yaml(yaml_path, yaml_value))
    _call(cases, "yaml-read", lambda: store.read_yaml(yaml_path))
    _call(cases, "jsonl-malformed", lambda: store.read_jsonl(_plaintext(root, "bad.jsonl", "{not-json}\n")))
    _call(cases, "utf8-noop-refusal", lambda: store.write_text_if_changed(_raw(root, "bad-utf8.txt", b"\xff"), "replacement"))

    rows_path = root / "rows.jsonl"
    _call(cases, "jsonl-write", lambda: store.write_jsonl(rows_path, [{"id": "a", "v": 1}, {"id": "b", "v": 2}]))
    _call(cases, "jsonl-append", lambda: store.append_jsonl(rows_path, [{"id": "c", "v": 3}]))
    _call(
        cases,
        "jsonl-upsert-last-copy",
        lambda: store.upsert_jsonl(rows_path, [{"id": "b", "v": 20}, {"id": "d", "v": 4}, {"id": "d", "v": 5}], ["v"]),
    )
    _call(cases, "jsonl-read", lambda: store.read_jsonl(rows_path))
    upsert_before = _identity(rows_path)
    _call(cases, "jsonl-upsert-noop", lambda: store.upsert_jsonl(rows_path, [{"id": "b", "v": 20}], ["v"]))
    cases.append({"id": "jsonl-noop-identity", "result": upsert_before == _identity(rows_path)})

    # Exactly one request needs an id. A fixed value avoids an iterator's
    # StopIteration crossing into the real decision providers.
    new_id = "a1b2c3d4" + "0" * 24
    with _patched(approvals.uuid, uuid4=lambda: types.SimpleNamespace(hex=new_id)), _patched(events, stamp=lambda: "2026-03-01T09:30:00Z", _version=lambda: "0.0.0"):
        requested = _call(cases, "receipt-request", lambda: approvals.request("  Synthetic   approval  ", " synthetic detail ", "agent"))
        _call(cases, "receipt-pending", approvals.pending)
        _call(cases, "receipt-decide", lambda: approvals.decide(requested["id"], "approved", " synthetic note "))
        _call(cases, "receipt-get", lambda: approvals.get(requested["id"]))
        _call(cases, "receipt-guidance", approvals.guidance)
        _call(cases, "receipt-already-decided", lambda: approvals.decide(requested["id"], "rejected"))
        _call(cases, "receipt-unknown", lambda: approvals.get("ffffffff"))

    # Real CLI handlers with invocation-scoped terminal, editor and Keychain adapters.
    _cli_case(cases, "data-edit-missing", types.SimpleNamespace(file="absent.yaml"), cli.cmd_data_edit, tty=True)
    _cli_case(cases, "data-edit-nonterminal", types.SimpleNamespace(file="record.yaml"), cli.cmd_data_edit, tty=False)

    def editor(args: list[str], check: bool) -> types.SimpleNamespace:
        assert check
        Path(args[-1]).write_text("at: 2026-03-01 10:00:00+00:00\ncount: 9\n", encoding="utf-8")
        return types.SimpleNamespace(returncode=0)

    edit_before = _identity(yaml_path)
    with _patched(subprocess, run=editor):
        _cli_case(cases, "data-edit-success", types.SimpleNamespace(file="record.yaml"), cli.cmd_data_edit, tty=True)
    edit_after = _identity(yaml_path)
    cases.append({"id": "data-edit-rewrites", "result": edit_before["inode"] != edit_after["inode"] and not (root / ".edit" / yaml_path.name).exists()})

    _cli_case(cases, "secret-set-nonterminal", types.SimpleNamespace(service="synthetic.service"), cli.cmd_secret_set, tty=False)
    with _patched(cli.getpass, getpass=lambda prompt: ""):
        _cli_case(cases, "secret-set-empty", types.SimpleNamespace(service="synthetic.service"), cli.cmd_secret_set, tty=True)
    with _patched(cli.getpass, getpass=lambda prompt: SECRET), _patched(cli.secrets, store_keychain=lambda service, value: f"keychain:{service}"):
        _cli_case(cases, "secret-set-success", types.SimpleNamespace(service="synthetic.service"), cli.cmd_secret_set, tty=True)

    _call(cases, "secret-backend-keychain", lambda: secrets.backend_of("keychain:synthetic"))
    _call(cases, "secret-backend-op", lambda: secrets.backend_of("op://synthetic/item/field"))
    _call(cases, "secret-backend-env", lambda: secrets.backend_of("env:SYNTHETIC_SECRET"))
    _call(cases, "secret-backend-literal", lambda: secrets.backend_of(SECRET))
    os.environ["SYNTHETIC_SECRET"] = SECRET
    with _patched(secrets.plugins, effective_policy=lambda: {"secret_backends": ["env"]}):
        _call(cases, "secret-resolve-env", lambda: secrets.resolve("env:SYNTHETIC_SECRET") == SECRET)
    os.environ.pop("SYNTHETIC_SECRET", None)

    event_rows = [json.loads(line) for line in (root / "logs" / "events.jsonl").read_text(encoding="utf-8").splitlines()]
    cases.append({"id": "receipt-events", "result": [{"event": row["event"], "status": row["status"]} for row in event_rows]})

    settings = config_io.settings_path()
    cases.append({"id": "settings-path", "result": settings.relative_to(root).as_posix()})
    _call(cases, "settings-missing", config_io.read_settings)
    settings.write_text("", encoding="utf-8")
    _call(cases, "settings-empty", config_io.read_settings)
    settings_value = {
        "questions_per_day": 3,
        "work_hours": 7.5,
        "notify_events": True,
        "nested": {"yes": "yes", "none": None},
    }
    _call(cases, "settings-write", lambda: config_io.write_settings(settings_value))
    _call(cases, "settings-read", config_io.read_settings)
    settings_before = _identity(settings)
    _call(cases, "settings-rewrite", lambda: config_io.write_settings(settings_value))
    cases.append({"id": "settings-write-rewrites", "result": settings_before["inode"] != _identity(settings)["inode"]})

    _call(
        cases,
        "event-clean",
        lambda: events.clean(
            {
                "event": "config.set",
                "status": "a" * 48,
                "command": "a" * 49,
                "route": "ok\n",
                "action": "ok\n\n",
                "error": "ValueError",
                "source_kind": "HTTP",
                "count": 1208925819614629174706179,
                "ratio": 1.5,
                "enabled": True,
                "note": CANARY,
                "nothing": None,
                "items": ["dropped"],
                "nested": {"dropped": True},
            }
        ),
    )
    with _patched(events, stamp=lambda: "2026-03-01T09:30:00Z", _version=lambda: "0.0.0"):
        _call(cases, "event-record", lambda: events.record("config.set", status="ok", note=CANARY))
    _call(cases, "event-write-row", lambda: events.write_row({"b": 1, "a": "x"}))
    log = root / "logs" / "events.jsonl"
    _call(cases, "event-write-row-line", lambda: log.read_text(encoding="utf-8").splitlines()[-1])
    with _patched(events, stamp=lambda: "2026-03-01T09:30:00Z", _version=lambda: "0.0.0"):
        _call(cases, "event-emit-config", lambda: events.emit("config.set", status="ok", note=CANARY, count=2))
        _call(cases, "event-emit-error", lambda: events.emit("cli.error", command="config", error="ValueError"))
    _call(cases, "event-lines", lambda: log.read_text(encoding="utf-8").splitlines()[-3:])
    float_row = {
        "nan": float("nan"),
        "infinity": float("inf"),
        "negative_infinity": float("-inf"),
        "negative_zero": -0.0,
        "exponent": 1e-7,
        "low_fixed": 1e-4,
        "low_scientific": 1e-5,
        "high_fixed": 1e15,
        "high_scientific": 1e16,
    }
    _call(cases, "event-write-float-row", lambda: events.write_row(float_row))
    _call(cases, "event-write-float-line", lambda: log.read_text(encoding="utf-8").splitlines()[-1])

    off_before = _identity(log)
    os.environ["CAREERBREW_EVENTS"] = "off"

    def unexpected_event_work(*args: Any, **kwargs: Any) -> None:
        raise AssertionError("disabled events reached a clock, version or writer adapter")

    with _patched(events, stamp=unexpected_event_work, _version=unexpected_event_work, write_row=unexpected_event_work):
        _call(cases, "event-off", lambda: events.emit("config.set", status="blocked"))
    os.environ["CAREERBREW_EVENTS"] = "on"
    cases.append({"id": "event-off-identity", "result": off_before == _identity(log)})

    _outline_and_cache_cases(cases, root)
    _key_path_cases(cases)
    _data_show_cases(cases, root)
    _bulk_cases(cases, root)

    cases.append({"id": "files", "result": _file_evidence(root, key_file)})
    return {"schema": "careerbrew-rust05-component-v1", "cases": cases}


def _plaintext(root: Path, name: str, text: str) -> Path:
    path = root / "logs" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text, encoding="utf-8")
    return path


def _raw(root: Path, name: str, raw: bytes) -> Path:
    path = root / "logs" / name
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(raw)
    return path


def _crash_write(stage: str) -> None:
    root = paths.data_root()
    os.environ["CAREERBREW_KEY_FILE"] = str(root.parent / "keys" / "backup.key")
    target = root / "crash.txt"

    # Keep a direct reference before replacing the method for this invocation.
    real_replace = Path.replace

    def replace(self: Path, other: Path) -> Path:
        if stage in ("after-temp", "before-rename"):
            os._exit(86)
        result = real_replace(self, other)
        if stage == "after-rename":
            os._exit(86)
        return result

    if stage == "before-temp":
        with _patched(vault, encode=lambda path, text: os._exit(86)):
            store.write_text(target, "synthetic crash")
    elif stage in ("after-temp", "before-rename", "after-rename"):
        with _patched(Path, replace=replace):
            store.write_text(target, "synthetic crash")
    else:
        raise ValueError("unknown crash point")


def _crash_receipt() -> None:
    root = paths.data_root()
    os.environ["CAREERBREW_KEY_FILE"] = str(root.parent / "keys" / "backup.key")
    with _patched(approvals.uuid, uuid4=lambda: types.SimpleNamespace(hex="a1b2c3d4" + "0" * 24)), _patched(events, emit=lambda *args, **kwargs: os._exit(86)):
        approvals.request("Synthetic crash")


def main(argv: list[str]) -> int:
    if len(argv) == 2 and argv[0] == "--crash":
        _crash_write(argv[1])
        return 0
    if argv == ["--crash-receipt"]:
        _crash_receipt()
        return 0
    if argv != ["--all"]:
        print("usage: rust05.py --all | --crash <point> | --crash-receipt", file=sys.stderr)
        return 2
    result = _run_all()
    raw = json.dumps(result, ensure_ascii=True, sort_keys=True, separators=(",", ":"))
    if CANARY in raw or SECRET in raw:
        print("synthetic value reached the oracle report", file=sys.stderr)
        return 1
    print(raw)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
