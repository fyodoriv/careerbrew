"""rust-06 Python oracle for encrypted archives, sync and CLI adapters.

Every operation uses the invocation-owned synthetic CAREERBREW_HOME. The report
contains typed results, error classes, modes and content digests, never keys,
archive bytes or synthetic record text. The native candidate emits this schema.
"""

from __future__ import annotations

import base64
import contextlib
import hashlib
import io
import json
import os
import re
import stat
import sys
import tarfile
import types
from pathlib import Path
from typing import Any

from careerbrew import backup, cli, machine, store, sync, vault

CANARY = "SYNTHETIC-RUST06-CANARY-7Q9"


def _typed(value: Any) -> Any:
    if value is None or isinstance(value, bool | str):
        return value
    if isinstance(value, int):
        return {"int": str(value)}
    if isinstance(value, list | tuple):
        return [_typed(item) for item in value]
    if isinstance(value, dict):
        return {str(key): _typed(value[key]) for key in sorted(value, key=str)}
    return {"type": type(value).__name__}


def _sha(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def _call(cases: list[dict[str, Any]], name: str, fn: Any) -> Any:
    try:
        value = fn()
    except Exception as exc:
        cases.append({"id": name, "result": {"error": type(exc).__name__}})
        return None
    cases.append({"id": name, "result": _typed(value)})
    return value


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


class Workspace:
    def __init__(self, base: Path, name: str):
        self.base = base / name
        self.root = self.base / "data"
        self.home = self.base / "home"
        self.backups = self.base / "backups"
        self.key = self.base / "keys" / "backup.key"
        self.pair = self.base / "keys" / "pair.key"
        self.root.mkdir(parents=True)
        self.home.mkdir(parents=True)
        os.environ.update(
            {
                "HOME": str(self.home),
                "CAREERBREW_HOME": str(self.root),
                "CAREERBREW_KEY_FILE": str(self.key),
                "CAREERBREW_PAIR_KEY_FILE": str(self.pair),
                "CAREERBREW_BACKUP_DIRS": str(self.backups),
                "CAREERBREW_EVENTS": "off",
                "CAREERBREW_ENCRYPT": "on",
                "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
                "TZ": "UTC",
            }
        )
        vault._fernet.cache_clear()

    def rel(self, path: Path) -> str:
        return path.resolve().relative_to(self.base.resolve()).as_posix()

    def evidence(self, root: Path | None = None) -> list[dict[str, Any]]:
        root = self.root if root is None else root
        if not root.exists():
            return []
        rows = []
        for path in sorted(root.rglob("*")):
            if path.is_symlink() or not path.is_file():
                continue
            rel = path.relative_to(root).as_posix()
            raw = path.read_bytes()
            row = {"path": rel, "mode": f"{stat.S_IMODE(path.stat().st_mode):03o}", "sealed": raw.startswith(vault.MAGIC)}
            try:
                text = store.read_text(path, create_key=False)
            except Exception as exc:
                row["read_error"] = type(exc).__name__
            else:
                row["text_sha256"] = None if text is None else _sha(text.encode())
            rows.append(row)
        return rows


def _archive_with_members(members: list[tuple[str, str, bytes | None]]) -> bytes:
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz") as tar:
        for name, kind, data in members:
            info = tarfile.TarInfo(name)
            if kind == "symlink":
                info.type = tarfile.SYMTYPE
                info.linkname = "."
                tar.addfile(info)
            else:
                info.size = len(data or b"")
                tar.addfile(info, io.BytesIO(data or b""))
    return buf.getvalue()


def _archive_roundtrip(base: Path) -> dict[str, Any]:
    w = Workspace(base, "archive-roundtrip")
    store.write_text(w.root / "profile.yaml", f"name: {CANARY}\n")
    store.write_text(w.root / "work" / "note.md", "synthetic work\n")
    store.write_text(w.root / "cache" / "ignored.json", "{}")
    store.write_text(w.root / ".staging" / "ignored.txt", "ignored")
    os.link(w.root / "work" / "note.md", w.root / "work" / "twin.md")
    (w.root / "link.md").symlink_to(w.root / "profile.yaml")
    store.write_yaml(w.root / "machine.yaml", {"role": "personal", "label": "source"})
    written = backup.create()
    archive = written[0]
    verified = backup.verify(archive)
    dest = w.base / "restored"
    store.write_text(dest / "work" / "note.md", "old conflict\n")
    restored = backup.restore(archive, dest)
    return {
        "archives": len(written),
        "verified": verified,
        "restored": restored,
        "archive_mode": f"{stat.S_IMODE(archive.stat().st_mode):03o}",
        "state": w.evidence(dest),
        "source_link_kept_out": not (dest / "link.md").exists(),
        "cache_kept_out": not (dest / "cache").exists(),
        "staging_kept_out": not (dest / ".staging").exists(),
        "machine_kept_out": not (dest / "machine.yaml").exists(),
        "hardlink_is_file": (dest / "work" / "twin.md").is_file() and not (dest / "work" / "twin.md").is_symlink(),
    }


def _archive_rejections(base: Path) -> dict[str, Any]:
    w = Workspace(base, "archive-rejections")
    store.write_text(w.root / "note.txt", "synthetic")
    archive = backup.create()[0]
    variants = {
        "trailing-junk": archive.read_bytes() + b"x",
        "trailing-padding": archive.read_bytes() + b"=",
        "truncated": archive.read_bytes()[:-1],
        "invalid-base64": b"!" + archive.read_bytes()[1:],
        "empty": b"",
    }
    tampered = bytearray(archive.read_bytes())
    tampered[50] ^= 1
    variants["tampered"] = bytes(tampered)
    results = {}
    for name, raw in variants.items():
        path = w.base / "variants" / archive.name
        path.parent.mkdir(exist_ok=True)
        path.write_bytes(raw)
        try:
            backup.verify(path)
        except Exception as exc:
            results[name] = type(exc).__name__
        else:
            results[name] = "accepted"
    wrong_name = w.base / "variants" / "careerbrew-20260301T093000000000Z-000000000000.cbk"
    wrong_name.write_bytes(archive.read_bytes())
    for name, fn in {
        "wrong-name": lambda: backup.verify(wrong_name),
        "expected-sha": lambda: backup.verify(archive, expect_sha="0" * 64),
    }.items():
        try:
            fn()
        except Exception as exc:
            results[name] = type(exc).__name__
        else:
            results[name] = "accepted"
    return results


def _archive_paths(base: Path) -> dict[str, Any]:
    w = Workspace(base, "archive-paths")
    results = {}
    odd = ["./jobs/tracker.yaml", "./machine.yaml", "work/../machine.yaml", "/profile.yaml", "work\\note.md"]
    for index, name in enumerate(odd):
        with _patched(backup, _archive=lambda n=name: _archive_with_members([(n, "file", b"x\n")])):
            archive = backup.create()[0]
        dest = w.base / f"odd-{index}"
        try:
            backup.restore(archive, dest)
        except Exception as exc:
            results[f"odd-{index}"] = {"error": type(exc).__name__, "created": dest.exists()}
    with _patched(backup, _archive=lambda: _archive_with_members([("alias", "symlink", None), ("alias/profile.yaml", "file", b"x\n")])):
        archive = backup.create()[0]
    dest = w.base / "link-member"
    try:
        backup.restore(archive, dest)
    except Exception as exc:
        results["link-member"] = {"error": type(exc).__name__, "created": dest.exists()}
    store.write_text(w.root / "work" / "note.md", "synthetic")
    archive = backup.create()[0]
    trapped = w.base / "trapped"
    trapped.mkdir()
    (trapped / "work").symlink_to(".")
    try:
        backup.restore(archive, trapped)
    except Exception as exc:
        results["destination-link"] = {"error": type(exc).__name__, "entries": sorted(p.name for p in trapped.iterdir())}
    return results


def _archive_corporate(base: Path) -> dict[str, Any]:
    scenarios = {
        "work": ("out/standup.html", "<p>synthetic</p>\n"),
        "personal-dir": ("Jobs/tracker.yaml", "jobs: []\n"),
        "personal-output": ("out/reports/run-1.html", "<p>synthetic</p>\n"),
        "profile-field": ("profile.yaml", "name: Example\ntarget_salary: 1\n"),
        "profile-malformed": ("profile.yaml", "name: [unclosed\n"),
        "goals": ("goals.yaml", "queue:\n- {kind: interview, title: Synthetic}\n"),
    }
    results = {}
    for name, (rel, text) in scenarios.items():
        source = Workspace(base, f"corporate-source-{name}")
        store.write_text(source.root / rel, text)
        archive = backup.create()[0]
        corporate = Workspace(base, f"corporate-dest-{name}")
        corporate.key.parent.mkdir(parents=True, exist_ok=True)
        corporate.key.write_bytes(source.key.read_bytes())
        corporate.key.chmod(0o600)
        machine.set_role("corporate", "work")
        dest = corporate.base / "restore"
        before = corporate.evidence(corporate.root)
        try:
            count = backup.restore(archive, dest)
        except Exception as exc:
            results[name] = {"error": type(exc).__name__, "unchanged": before == corporate.evidence(corporate.root), "created": dest.exists()}
        else:
            results[name] = {"restored": count, "role": store.read_yaml(dest / "machine.yaml", {}).get("role"), "state": corporate.evidence(dest)}
    return results


def _archive_key_guards(base: Path) -> dict[str, Any]:
    results = {}
    w = Workspace(base, "archive-missing-key")
    (w.root / "work").mkdir()
    (w.root / "work" / "sealed.txt").write_bytes(vault.seal_bytes(b"synthetic"))
    before = (w.root / "work" / "sealed.txt").read_bytes()
    backup.load_key()
    w.key.unlink()
    vault._fernet.cache_clear()
    try:
        backup.create()
    except Exception as exc:
        results["missing-key"] = {"error": type(exc).__name__, "key_created": w.key.exists(), "unchanged": before == (w.root / "work" / "sealed.txt").read_bytes(), "archives": len(list(w.backups.glob("*.cbk"))) if w.backups.exists() else 0}
    inside = Workspace(base, "archive-key-inside")
    os.environ["CAREERBREW_KEY_FILE"] = str(inside.root / "keys" / "backup.key")
    try:
        backup.create()
    except Exception as exc:
        results["inside-key"] = {"error": type(exc).__name__, "created": (inside.root / "keys" / "backup.key").exists()}
    pair = Workspace(base, "pair-key-inside")
    os.environ["CAREERBREW_PAIR_KEY_FILE"] = str(pair.root / "pair.key")
    try:
        sync.pair_init()
    except Exception as exc:
        results["pair-inside"] = {"error": type(exc).__name__, "created": (pair.root / "pair.key").exists()}
    valid = Workspace(base, "pair-valid")
    key = sync.pair_init()
    results["pair-roundtrip"] = {"key_length": len(key), "mode": f"{stat.S_IMODE(valid.pair.stat().st_mode):03o}", "roundtrip": sync._fernet().decrypt(sync._fernet().encrypt(b"synthetic")) == b"synthetic"}
    try:
        sync.pair_join("not-a-key")
    except Exception as exc:
        results["pair-invalid"] = type(exc).__name__
    return results


def _archive_targets(base: Path) -> dict[str, Any]:
    w = Workspace(base, "archive-targets")
    results = {}
    store.write_yaml(w.root / "settings.yaml", {"backup_icloud": True})
    os.environ["CAREERBREW_ICLOUD_DIR"] = str(w.base / "icloud")
    written = backup.create()
    results["icloud"] = {"targets": sorted(w.rel(path.parent) for path in written), "verified": [backup.verify(path) for path in written]}
    allowed = w.base / "approved"
    os.environ["CAREERBREW_BACKUP_DIRS"] = os.pathsep.join([str(w.backups), str(allowed)])
    os.environ["CAREERBREW_RUST06_ALLOWED"] = str(allowed)
    with _patched(backup.plugins, effective_policy=lambda: {"backup_targets": [str(allowed)]}):
        results["policy"] = [w.rel(path.parent) for path in backup.targets()]
    os.environ["CAREERBREW_RUST06_ALLOWED"] = ""
    with _patched(backup.plugins, effective_policy=lambda: {"backup_targets": []}):
        try:
            backup.create()
        except Exception as exc:
            results["policy-denied"] = type(exc).__name__
    corporate = Workspace(base, "archive-cloud")
    machine.set_role("corporate")
    os.environ["CAREERBREW_BACKUP_DIRS"] = str(corporate.home / "Dropbox" / "cb")
    try:
        backup.create()
    except Exception as exc:
        results["cloud-create"] = {"error": type(exc).__name__, "created": (corporate.home / "Dropbox" / "cb").exists()}
    os.environ["CAREERBREW_BACKUP_DIRS"] = str(corporate.backups)
    archive = backup.create()[0]
    for service in ("pCloud Drive", "Sync"):
        target = corporate.home / service / "cb"
        os.environ["CAREERBREW_BACKUP_DIRS"] = str(target)
        try:
            backup.create()
        except Exception as exc:
            results[f"cloud-create-{service}"] = {"error": type(exc).__name__, "created": target.exists()}
        dest = corporate.home / service / "restored"
        try:
            backup.restore(archive, dest)
        except Exception as exc:
            results[f"cloud-restore-{service}"] = {"error": type(exc).__name__, "created": dest.exists()}
    return results


def _sync_roundtrip(base: Path) -> dict[str, Any]:
    personal = Workspace(base, "sync-personal")
    store.write_yaml(personal.root / "settings.yaml", {"ladder": "generic-swe", "target_level": "staff", "backup_targets": ["/synthetic/refused"]})
    key = sync.pair_init()
    bundle = personal.base / "bundle.cbsync"
    exported = sync.export(bundle)
    corporate = Workspace(base, "sync-corporate")
    machine.set_role("corporate")
    sync.pair_join(key)
    imported = sync.import_bundle(bundle)
    settings = store.read_yaml(corporate.root / "settings.yaml", {})
    return {"exported": exported, "imported": imported, "bundle_mode": f"{stat.S_IMODE(bundle.stat().st_mode):03o}", "settings": settings, "state": corporate.evidence()}


def _sync_rejections(base: Path) -> dict[str, Any]:
    Workspace(base, "sync-rejections")
    sync.pair_init()
    results = {}
    corporate = Workspace(base, "sync-export-corporate")
    machine.set_role("corporate")
    sync.pair_init()
    try:
        sync.export(corporate.base / "refused.cbsync")
    except Exception as exc:
        results["corporate-export"] = {"error": type(exc).__name__, "created": (corporate.base / "refused.cbsync").exists()}
    personal = Workspace(base, "sync-personal-refusal")
    key = corporate.pair.read_text().strip()
    sync.pair_join(key)
    from cryptography.fernet import Fernet

    cipher = Fernet(key.encode())
    bundle = personal.base / "corporate.cbsync"
    bundle.write_bytes(cipher.encrypt(json.dumps({"from": "corporate", "at": "x", "payload": {"evidence": []}}).encode()))
    try:
        sync.import_bundle(bundle)
    except Exception as exc:
        results["corporate-import"] = type(exc).__name__
    same = personal.base / "same.cbsync"
    same.write_bytes(cipher.encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"settings": {}}}).encode()))
    try:
        sync.import_bundle(same)
    except Exception as exc:
        results["same-role"] = type(exc).__name__
    wrong = Fernet(base64.urlsafe_b64encode(bytes(range(32))))
    wrong_bundle = personal.base / "wrong.cbsync"
    wrong_bundle.write_bytes(wrong.encrypt(b"{}"))
    try:
        sync.import_bundle(wrong_bundle)
    except Exception as exc:
        results["wrong-key"] = type(exc).__name__
    corp = Workspace(base, "sync-forbidden")
    machine.set_role("corporate")
    sync.pair_join(key)
    forbidden = corp.base / "forbidden.cbsync"
    forbidden.write_bytes(cipher.encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"jobs": []}}).encode()))
    before = corp.evidence()
    try:
        sync.import_bundle(forbidden)
    except Exception as exc:
        results["forbidden-payload"] = {"error": type(exc).__name__, "unchanged": before == corp.evidence()}
    return results


def _sync_allowlist(base: Path) -> list[dict[str, Any]]:
    w = Workspace(base, "sync-allowlist")
    machine.set_role("corporate")
    key = sync.pair_init()
    from cryptography.fernet import Fernet

    cipher = Fernet(key.encode())
    inputs = [
        {"ladder": "no-such-ladder", "target_level": "DROP"},
        {"ladder": "generic-swe", "target_level": "staff"},
        {"target_level": "wizard"},
        {"ladder": {"not": "text"}, "target_level": "staff", "extra": 1},
        {"ladder": "x" * 65, "target_level": "staff"},
    ]
    results = []
    for index, settings in enumerate(inputs):
        path = w.base / f"case-{index}.cbsync"
        path.write_bytes(cipher.encrypt(json.dumps({"from": "personal", "at": "x", "payload": {"settings": settings}}).encode()))
        outcome = sync.import_bundle(path)
        current = store.read_yaml(w.root / "settings.yaml", {})
        results.append({"result": outcome, "settings": current})
    return results


def _sync_plugin_ladder(base: Path) -> dict[str, Any]:
    personal = Workspace(base, "sync-plugin-personal")
    store.write_yaml(personal.root / "settings.yaml", {"ladder": "synthetic-eng", "target_level": "e5"})
    key = sync.pair_init()
    bundle = personal.base / "plugin.cbsync"
    sync.export(bundle)
    corporate = Workspace(base, "sync-plugin-corporate")
    machine.set_role("corporate")
    sync.pair_join(key)
    plugin = corporate.root / "plugins" / "synthetic"
    manifest = {"name": "synthetic", "version": "1.0.0", "provides": {"ladders": "ladders/*.yaml"}}
    store.write_yaml(plugin / "careerbrew-plugin.yaml", manifest)
    ladder = {
        "id": "synthetic-eng",
        "name": "Synthetic engineering",
        "levels": ["e4", "e5"],
        "competencies": [{"id": "delivery", "name": "Delivery", "weight": 1, "signals": {}, "expect": {"e4": 1, "e5": 2}}],
    }
    store.write_yaml(plugin / "ladders" / "custom.yaml", ladder)
    store.write_json(corporate.root / "plugins.lock.json", {"plugins": {"synthetic": {"origin": "plugin:synthetic"}}})
    imported = sync.import_bundle(bundle)
    from careerbrew.work import promo

    loaded = promo.ladders()["synthetic-eng"]
    result = {
        "imported": imported,
        "settings": store.read_yaml(corporate.root / "settings.yaml", {}),
        "origin": loaded["origin"],
        "source": (plugin / "ladders" / "custom.yaml").relative_to(corporate.root).as_posix(),
    }
    store.write_yaml(plugin / "careerbrew-plugin.yaml", {"name": "synthetic", "version": "1.0.0", "provides": {"ladders": "../outside.yaml"}})
    escape_before = corporate.evidence()
    try:
        sync.import_bundle(bundle)
    except Exception as exc:
        result["escape"] = {"error": type(exc).__name__, "unchanged": escape_before == corporate.evidence()}
    store.write_yaml(plugin / "careerbrew-plugin.yaml", manifest)
    (plugin / "ladders" / "link.yaml").symlink_to(plugin / "ladders" / "custom.yaml")
    symlink_before = corporate.evidence()
    try:
        sync.import_bundle(bundle)
    except Exception as exc:
        result["symlink"] = {"error": type(exc).__name__, "unchanged": symlink_before == corporate.evidence()}
    return result


def _cli_handlers(base: Path) -> dict[str, Any]:
    w = Workspace(base, "cli-handlers")
    store.write_text(w.root / "profile.yaml", "name: Example\n")
    outputs = {}

    def invoke(name: str, fn: Any, args: types.SimpleNamespace) -> None:
        output = io.StringIO()
        with contextlib.redirect_stdout(output):
            code = fn(args)
        text = output.getvalue().replace(str(w.base), "<work>")
        text = re.sub(r"careerbrew-[^ \n]+\.cbk", "careerbrew-<archive>.cbk", text)
        if CANARY in text or "pairing key" in text and "paired" not in text:
            raise RuntimeError("private output")
        outputs[name] = {"exit": code, "stdout": text}

    invoke("data-status", cli.cmd_data_status, types.SimpleNamespace())
    invoke("backup-create", cli.cmd_backup_create, types.SimpleNamespace())
    archive = backup.latest()
    invoke("backup-verify", cli.cmd_backup_verify, types.SimpleNamespace(file=str(archive)))
    invoke("backup-restore", cli.cmd_backup_restore, types.SimpleNamespace(file=str(archive), dest=str(w.base / "restored")))
    key = sync.pair_init()
    invoke("pair-join", cli.cmd_pair_join, types.SimpleNamespace(key=key))
    bundle = w.base / "handler.cbsync"
    store.write_yaml(w.root / "settings.yaml", {"ladder": "generic-swe", "target_level": "staff"})
    invoke("sync-export", cli.cmd_sync_export, types.SimpleNamespace(file=str(bundle)))
    machine.set_role("corporate")
    invoke("sync-import", cli.cmd_sync_import, types.SimpleNamespace(file=str(bundle)))
    return outputs


def run_all() -> dict[str, Any]:
    base = Path(os.environ["CAREERBREW_HOME"]) / "rust06-workload"
    base.mkdir(parents=True)
    cases: list[dict[str, Any]] = []
    for name, fn in [
        ("archive-roundtrip", _archive_roundtrip),
        ("archive-rejections", _archive_rejections),
        ("archive-paths", _archive_paths),
        ("archive-corporate", _archive_corporate),
        ("archive-key-guards", _archive_key_guards),
        ("archive-targets", _archive_targets),
        ("sync-roundtrip", _sync_roundtrip),
        ("sync-rejections", _sync_rejections),
        ("sync-allowlist", _sync_allowlist),
        ("sync-plugin-ladder", _sync_plugin_ladder),
        ("cli-handlers", _cli_handlers),
    ]:
        _call(cases, name, lambda fn=fn: fn(base))
    text = json.dumps({"cases": cases}, sort_keys=True, separators=(",", ":"))
    if CANARY in text:
        raise RuntimeError("synthetic canary reached the oracle report")
    return {"text": text + "\n"}


def main() -> int:
    if sys.argv[1:] != ["--all"]:
        return 2
    sys.stdout.write(run_all()["text"])
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
