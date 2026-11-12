"""Encrypted sync bundles between a corporate and a personal machine.

No server. One machine writes an encrypted `.cbsync` file; you move it
(AirDrop, USB, a personal cloud folder); the other machine imports it.

Pairing: `careerbrew pair init` creates a pairing key (Fernet) on one machine
and prints it once; store it in your password manager and run
`careerbrew pair join <key>` on the other machine. Key file:
`~/.config/careerbrew/pair.key` (mode 600), outside the data root.

What moves (the filter runs before encryption):
- corporate → personal: nothing. Corporate data stays on the corporate
  machine; `export` refuses there and a personal machine refuses a corporate
  bundle (docs/privacy.md).
- personal → corporate: only the promotion ladder choice and tutor-free
  settings. Job-hunt data (postings, ratings, applications, drills, profile)
  never moves to a corporate machine.
"""

from __future__ import annotations

import json
import os
from pathlib import Path
from typing import Any

from cryptography.fernet import Fernet, InvalidToken

from . import calendar, events, machine, paths, store

SETTINGS_KEYS = ("ladder", "target_level")


class SyncError(RuntimeError):
    pass


def key_path() -> Path:
    env = os.environ.get("CAREERBREW_PAIR_KEY_FILE")
    return Path(env).expanduser() if env else Path.home() / ".config" / "careerbrew" / "pair.key"


def pair_init() -> str:
    key = Fernet.generate_key().decode()
    _write_key(key)
    return key


def pair_join(key: str) -> None:
    try:
        Fernet(key.encode())
    except (ValueError, TypeError) as exc:
        raise SyncError("not a valid pairing key (fix: copy it exactly from `pair init`)") from exc
    _write_key(key)


def _write_key(key: str) -> None:
    p = key_path()
    paths.require_outside_data_root(p, "pairing key")
    p.parent.mkdir(parents=True, exist_ok=True)
    p.write_text(key, encoding="utf-8")
    p.chmod(0o600)


def _fernet() -> Fernet:
    paths.require_outside_data_root(key_path(), "pairing key")
    if not key_path().exists():
        raise SyncError("this machine is not paired (fix: `careerbrew pair init` on one machine, `pair join <key>` on the other)")
    return Fernet(key_path().read_text(encoding="utf-8").strip().encode())


def _personal_payload() -> dict[str, Any]:
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    return {"settings": {k: settings[k] for k in SETTINGS_KEYS if k in settings}}


def export(dest: Path) -> dict[str, Any]:
    src = machine.role()
    if src == "corporate":
        raise SyncError("corporate data stays on the corporate machine: sync export is personal → corporate only")
    paths.require_outside_data_root(dest, "sync bundle")
    payload = _personal_payload()
    bundle = {"from": src, "at": calendar.now_utc().isoformat(timespec="seconds"), "payload": payload}
    token = _fernet().encrypt(json.dumps(bundle).encode())
    dest.write_bytes(token)
    dest.chmod(0o600)
    events.emit("sync.export", status="ok", source_kind=src)
    return {"from": src, "level": "settings", "items": len(payload["settings"])}


def import_bundle(src: Path) -> dict[str, Any]:
    try:
        bundle = json.loads(_fernet().decrypt(src.read_bytes()))
    except InvalidToken as exc:
        raise SyncError("bundle does not decrypt (wrong pairing key, or the file changed)") from exc
    here = machine.role()
    if bundle["from"] == here:
        raise SyncError(f"bundle comes from another {here} machine; sync runs between a corporate and a personal machine")
    payload = bundle["payload"]
    if here == "personal":
        raise SyncError("refusing a corporate bundle: corporate data stays on the corporate machine")
    else:
        forbidden = set(payload) - {"settings"}
        if forbidden:
            raise SyncError("refusing personal data on a corporate machine")
        settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
        from .work import promo

        incoming = {k: v for k, v in (payload.get("settings") or {}).items() if k in SETTINGS_KEYS and isinstance(v, str) and len(v) <= 64}
        ladders = promo.ladders()
        applied = {}
        if incoming.get("ladder") in ladders:
            applied["ladder"] = incoming["ladder"]
        ladder_id = applied.get("ladder") or settings.get("ladder", "generic-swe")
        if ladder_id in ladders and incoming.get("target_level") in ladders[ladder_id]["levels"]:
            applied["target_level"] = incoming["target_level"]
        settings.update(applied)
        store.write_yaml(paths.data_root() / "settings.yaml", settings)
        added = len(applied)
        ignored = len(payload.get("settings") or {}) - added
    events.emit("sync.import", status="ok", items=added)
    return {"from": bundle["from"], "added": added, "ignored": ignored}
