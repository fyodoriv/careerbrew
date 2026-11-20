"""The data key provider: where the key file lives and how it is read or created (rust-05a).

The key lives outside the data root (default `~/.config/careerbrew/backup.key`,
mode 600). Storage (`vault`) and backups both use it. `backup.key_path` and
`backup.load_key` stay as thin names for existing callers, so the native port
of storage never needs the backup module.
"""

from __future__ import annotations

import os
import time
from pathlib import Path

from cryptography.fernet import Fernet

from . import paths


def key_path() -> Path:
    env = os.environ.get("CAREERBREW_KEY_FILE")
    return Path(env).expanduser() if env else Path.home() / ".config" / "careerbrew" / "backup.key"


def load_key(create: bool = True, path: Path | None = None) -> bytes:
    """Read the key at `path` (default `key_path()`); with `create`, make a new one when it is missing."""
    path = key_path() if path is None else path
    paths.require_outside_data_root(path, "backup key")
    if path.exists():
        return path.read_bytes().strip()
    if not create:
        raise FileNotFoundError(f"backup key missing at {path} (fix: restore it from your password manager)")
    from . import vault

    if vault._has_sealed_data():
        raise vault.VaultError("backup key missing but encrypted data exists (fix: restore the key from your password manager)")
    path.parent.mkdir(parents=True, exist_ok=True)
    key = Fernet.generate_key()
    # rust-05b: create the file only if absent, at mode 0600 from the start; a first run that loses the race reads the winner's key.
    try:
        fd = os.open(path, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
    except FileExistsError:
        return _read_existing_key(path)
    with os.fdopen(fd, "wb") as fh:
        fh.write(key)
    os.chmod(path, 0o600)
    return key


def _read_existing_key(path: Path) -> bytes:
    """Read a key another process just created; wait briefly while its writer finishes."""
    for _ in range(50):
        key = path.read_bytes().strip()
        if key:
            return key
        time.sleep(0.01)
    raise OSError(f"backup key at {path} is empty (fix: restore it from your password manager)")
