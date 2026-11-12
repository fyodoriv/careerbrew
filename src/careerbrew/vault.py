"""Encryption at rest for the data root.

Every data record careerbrew writes under the data root goes through `seal` (in `store`)
and, with encryption on (the default), is stored as `MAGIC + Fernet token` (AES-128-CBC + HMAC-SHA256). The key is
the backup key (`backup.key_path()`, outside the data root, mode 600), so a
copied `private/` folder, a stray sync, or a tool that greps the disk sees ciphertext
for those records (the plaintext folders are listed below), and one key in your password manager opens both data and backups.

Reads accept plaintext too (`open_`), so hand-written files and older data keep
working; `careerbrew data encrypt` seals what is still plaintext.

Not sealed, by design: `logs/` (content-free events), `out/` (reports you open
in a browser or editor; delete them when done), `plugins/` (installed copies
mirror the plaintext plugin source folder; the lock hashes the plaintext and the
term scan reads it, issue #25), and replay Git checkouts. `careerbrew data decrypt` turns
encryption off for the data root and writes the `.plaintext` marker.
"""

from __future__ import annotations

import base64
import binascii
import os
from functools import lru_cache
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

from . import paths

MAGIC = b"CBV1\n"
PLAIN_DIRS = {"logs", "out", ".edit", "plugins"}  # .edit: the short-lived plaintext copy `data edit` opens; plugins: see below
MARKER = ".plaintext"


class VaultError(RuntimeError):
    pass


@lru_cache(maxsize=4)
def _fernet(key_file: str) -> Fernet:
    from . import backup

    return Fernet(backup.load_key(create=True))


def _key() -> Fernet:
    from . import backup

    path = backup.key_path()
    paths.require_outside_data_root(path, "backup key")
    if not path.exists() and _has_sealed_data():
        # A new key would split the data in two: old files under the lost key, new ones under the new key.
        raise VaultError(f"the key at {path} is missing but data files are encrypted (fix: restore the key from your password manager)")
    return _fernet(str(path))


def enabled() -> bool:
    if os.environ.get("CAREERBREW_ENCRYPT", "on") == "off":
        return False
    return not (paths.data_root() / MARKER).exists()


def sealable(path: Path) -> bool:
    """True for files under the data root that are not in a plaintext-by-design folder."""
    try:
        rel = path.resolve().relative_to(paths.data_root().resolve())
    except ValueError:
        return False
    return bool(rel.parts) and rel.parts[0] not in PLAIN_DIRS and rel.name != MARKER and not _replay_checkout(rel)


def _replay_checkout(rel: Path) -> bool:
    parts = rel.parts
    if len(parts) < 5 or parts[0] != "learn" or parts[2] != "replay" or not parts[3].isascii() or not parts[3].isdigit() or parts[3].startswith("0"):
        return False
    git = paths.data_root().joinpath(*parts[:4], ".git")
    return not git.is_symlink() and git.is_dir() and not (git / "HEAD").is_symlink() and (git / "HEAD").is_file()


def is_sealed(raw: bytes) -> bool:
    return raw.startswith(MAGIC)


def seal_bytes(raw: bytes) -> bytes:
    return MAGIC + _key().encrypt(raw)


def open_bytes(raw: bytes, *, create_key: bool = True) -> bytes:
    if not is_sealed(raw):
        return raw
    try:
        if create_key:
            cipher = _key()
        else:
            from . import backup

            cipher = Fernet(backup.load_key(create=False))
        return cipher.decrypt(raw[len(MAGIC) :])
    except InvalidToken as e:
        from . import backup

        raise VaultError(f"cannot decrypt a data file: the key at {backup.key_path()} is not the one it was written with (fix: restore the key from your password manager)") from e


def encode(path: Path, text: str) -> bytes:
    raw = text.encode("utf-8")
    return seal_bytes(raw) if enabled() and sealable(path) else raw


def decode(raw: bytes, *, create_key: bool = True) -> str:
    return open_bytes(raw, create_key=create_key).decode("utf-8")


def _files(*, include_replay: bool = False) -> list[Path]:
    root = paths.data_root()
    result = []
    for p in root.rglob("*"):
        if not p.is_file() or p.is_symlink() or p.name.endswith((".lock", ".tmp")):
            continue
        try:
            rel = p.resolve().relative_to(root.resolve())
        except ValueError:
            continue
        if sealable(p) or (include_replay and _replay_checkout(rel)):
            result.append(p)
    return result


def files() -> list[Path]:
    return _files()


def _has_sealed_data() -> bool:
    root = paths.data_root(create=False)
    if not root.exists():
        return False
    for path in root.rglob("*"):
        if not path.is_file() or path.is_symlink() or path.name.endswith((".lock", ".tmp")):
            continue
        try:
            path.resolve().relative_to(root.resolve())
        except ValueError:
            continue
        with path.open("rb") as stream:
            if is_sealed(stream.read(len(MAGIC))):
                return True
    return False


def status() -> dict[str, int | bool]:
    fs = files()
    sealed = sum(1 for p in fs if is_sealed(p.read_bytes()[: len(MAGIC)]))
    return {"enabled": enabled(), "files": len(fs), "sealed": sealed, "plaintext": len(fs) - sealed}


def _rewrite(path: Path, raw: bytes) -> None:
    tmp = path.with_name(f".{path.name}.{os.getpid()}.vault.tmp")
    tmp.write_bytes(raw)
    os.chmod(tmp, 0o600)
    tmp.replace(path)


def seal_files(targets: list[Path]) -> int:
    """Seal the given plaintext files when encryption is on (for files other tools wrote, like an import bundle)."""
    if not enabled():
        return 0
    n = 0
    for p in targets:
        if not (p.is_file() and sealable(p)):
            continue
        raw = p.read_bytes()
        if not is_sealed(raw):
            _rewrite(p, seal_bytes(raw))
            n += 1
    return n


def encrypt_all() -> int:
    """Seal every plaintext file and turn encryption on. Returns the number of files sealed."""
    targets = files()
    if any(p.suffix.lower() in {".cbk", ".cbsync"} or _is_fernet_token(p) for p in targets):
        raise VaultError("move backup and sync bundles outside the data folder before encryption")
    (paths.data_root() / MARKER).unlink(missing_ok=True)
    n = 0
    for p in targets:
        raw = p.read_bytes()
        if not is_sealed(raw):
            _rewrite(p, seal_bytes(raw))
            n += 1
    return n


def _is_fernet_token(path: Path) -> bool:
    with path.open("rb") as stream:
        first = stream.read(1)
        if first != b"g":
            return False
        raw = first + stream.read()
    try:
        decoded = base64.b64decode(raw, altchars=b"-_", validate=True)
    except (ValueError, binascii.Error):
        return False
    return len(decoded) >= 73 and decoded[0] == 0x80


def decrypt_all() -> int:
    """Open every sealed file and turn encryption off. Returns the number of files opened."""
    n = 0
    for p in _files(include_replay=True):
        raw = p.read_bytes()
        if is_sealed(raw):
            _rewrite(p, open_bytes(raw))
            n += 1
    (paths.data_root() / MARKER).write_text("encryption at rest is off for this data root; `careerbrew data encrypt` turns it on\n", encoding="utf-8")
    return n
