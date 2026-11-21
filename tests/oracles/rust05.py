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
import stat
import subprocess
import sys
import types
from pathlib import Path
from typing import Any

from careerbrew import approvals, cli, events, paths, secrets, store, vault

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

    ids = iter(["a1b2c3d4" + "0" * 24, "e5f60718" + "0" * 24])
    with _patched(approvals.uuid, uuid4=lambda: types.SimpleNamespace(hex=next(ids))), _patched(events, stamp=lambda: "2026-03-01T09:30:00Z", _version=lambda: "0.0.0"):
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
