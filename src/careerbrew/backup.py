"""Encrypted, verified backups of the data root.

Each backup is a tar.gz of the data root (minus the rebuildable cache), encrypted
with Fernet (AES-128-CBC + HMAC-SHA256, from the `cryptography` library), written
to every configured target, then read back and verified. The key lives outside
the data root (default `~/.config/careerbrew/backup.key`, mode 600), so a copied
backup alone cannot be opened.
"""

from __future__ import annotations

import base64
import binascii
import fnmatch
import hashlib
import io
import os
import posixpath
import tarfile
from datetime import UTC, datetime
from pathlib import Path

from cryptography.fernet import Fernet, InvalidToken

from . import events, paths, plugins, store

EXCLUDE_DIRS = {"cache", ".staging"}
KEEP = 10


def key_path() -> Path:
    env = os.environ.get("CAREERBREW_KEY_FILE")
    return Path(env).expanduser() if env else Path.home() / ".config" / "careerbrew" / "backup.key"


def load_key(create: bool = True) -> bytes:
    path = key_path()
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
    path.write_bytes(key)
    path.chmod(0o600)
    return key


def icloud_dir() -> Path:
    """The opt-in iCloud Drive target; macOS uploads this folder, careerbrew only writes files to it."""
    env = os.environ.get("CAREERBREW_ICLOUD_DIR")
    if env:
        return Path(env).expanduser()
    return Path.home() / "Library" / "Mobile Documents" / "com~apple~CloudDocs" / "careerbrew"


def configured_targets() -> list[Path]:
    settings = store.read_yaml(paths.data_root() / "settings.yaml", {}) or {}
    configured = settings.get("backup_targets") or [str(Path.home() / "Backups" / "careerbrew")]
    env = os.environ.get("CAREERBREW_BACKUP_DIRS")
    if env:
        configured = env.split(os.pathsep)
    out = [Path(os.path.expanduser(t)) for t in configured]
    if settings.get("backup_icloud") is True and icloud_dir() not in out:
        out.append(icloud_dir())
    return out


def targets() -> list[Path]:
    from . import machine

    allowed = plugins.effective_policy().get("backup_targets")
    corporate = machine.role() == "corporate"
    out = []
    for p in configured_targets():
        cloud = machine.cloud_synced(p) if corporate else ""
        if cloud:
            # A corporate backup does not go to a known cloud-synced folder, not even encrypted (docs/privacy.md).
            raise RuntimeError(f"backup target is in a cloud-synced folder ({cloud}); corporate data stays on this machine (fix: set backup_targets with `careerbrew data edit settings.yaml` to a local folder)")
        if allowed is not None and str(p) not in allowed:
            continue
        paths.require_outside_data_root(p, "backup target")
        out.append(p)
    if not out:
        raise RuntimeError("no backup target allowed by plugin policy (fix: add a target the policy lists with `careerbrew data edit settings.yaml`)")
    return out


def _archive() -> bytes:
    root = paths.data_root()
    buf = io.BytesIO()
    with tarfile.open(fileobj=buf, mode="w:gz", dereference=True) as tar:  # dereference: two hard-linked files go in as two files
        for p in sorted(root.rglob("*")):
            rel = p.relative_to(root)
            if p.is_file() and not p.is_symlink() and not (set(rel.parts) & EXCLUDE_DIRS):  # a restore refuses links, so none goes in
                tar.add(p, arcname=rel.as_posix())
    return buf.getvalue()


def create() -> list[Path]:
    destinations = targets()
    raw = _archive()
    sha = hashlib.sha256(raw).hexdigest()
    token = Fernet(load_key()).encrypt(raw)
    stamp = datetime.now(UTC).strftime("%Y%m%dT%H%M%S%fZ")  # wall clock: names sort by real write time for retention
    written = []
    for target in destinations:
        target.mkdir(parents=True, exist_ok=True)
        path = target / f"careerbrew-{stamp}-{sha[:12]}.cbk"
        path.write_bytes(token)
        path.chmod(0o600)
        verify(path, expect_sha=sha)
        written.append(path)
        backups = sorted(target.glob("careerbrew-*.cbk"))
        for old in backups[:-KEEP]:
            old.unlink()
    events.emit("backup.create", status="ok", targets=len(written))
    return written


def verify(path: Path, expect_sha: str | None = None) -> int:
    """Decrypt and check integrity. Returns the number of files inside."""
    token = path.read_bytes()
    try:
        # Strict decode first: Fernet's own decoder ignores appended junk, which would hide tampering.
        if len(token) % 4 or not token:
            raise InvalidToken
        base64.b64decode(token, altchars=b"-_", validate=True)
        raw = Fernet(load_key(create=False)).decrypt(token)
    except (InvalidToken, binascii.Error) as exc:
        raise RuntimeError(f"backup {path.name} failed verification: wrong key or corrupted file") from exc
    sha = hashlib.sha256(raw).hexdigest()
    if expect_sha and sha != expect_sha:
        raise RuntimeError(f"backup {path.name} hash mismatch")
    if not path.name.split("-")[-1].startswith(sha[:12]):
        raise RuntimeError(f"backup {path.name} content does not match its name")
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tar:
        return len(tar.getmembers())


def latest() -> Path | None:
    found = []
    for t in targets():
        found.extend(t.glob("careerbrew-*.cbk"))
    return max(found, key=lambda p: p.name) if found else None


def _job_hunt_member(tar: tarfile.TarFile, member: tarfile.TarInfo) -> bool:
    """Whether an archived file is job-hunt data: the same folders, reports, and profile fields the corporate role refuses."""
    from . import machine, vault

    name = member.name.lower()  # the default macOS file system ignores case
    if name.split("/", 1)[0] in machine.PERSONAL_DIRS:
        return True
    if any(fnmatch.fnmatchcase(name, f"out/{pattern}") for pattern in machine.PERSONAL_OUTPUTS):
        return True
    if name not in ("profile.yaml", "goals.yaml"):
        return False
    stream = tar.extractfile(member)
    if stream is None:
        return True  # a profile or goals file that is not a regular file: refuse, do not guess
    try:
        text = vault.decode(stream.read(), create_key=False)
        if name == "goals.yaml":
            import yaml

            from . import goals

            return goals.job_hunt_goals(yaml.safe_load(text)) > 0
    except Exception:  # noqa: BLE001 (a file this machine cannot read is refused, never restored unchecked)
        return True
    return machine.has_job_hunt_fields(text)


def restore(path: Path, dest: Path) -> int:
    from . import machine

    dest = dest.expanduser().resolve()  # the real folder: every check below, and the write, use the same path with no link in it
    cloud = machine.cloud_synced(dest) if machine.role() == "corporate" else ""
    if cloud:
        raise RuntimeError(f"restore destination is in a cloud-synced folder ({cloud}); choose a local folder on a corporate machine")
    verify(path)
    raw = Fernet(load_key(create=False)).decrypt(path.read_bytes())
    with tarfile.open(fileobj=io.BytesIO(raw), mode="r:gz") as tar:
        # A restore never carries a machine role; the target machine keeps its own.
        # careerbrew writes plain relative names. Another spelling (`./jobs/x`, `a/../machine.yaml`) could slip past the checks below.
        # A link member (`alias -> .`, then `alias/profile.yaml`) would do the same, so only regular files pass.
        odd = [m for m in tar.getmembers() if not m.isreg() or m.name != posixpath.normpath(m.name) or m.name.startswith("/") or ".." in m.name.split("/") or "\\" in m.name]
        if odd:
            raise RuntimeError(f"backup {path.name} holds {len(odd)} file name(s) careerbrew never writes; nothing restored")
        members = [m for m in tar.getmembers() if m.name.lower() != "machine.yaml"]
        if machine.role() == "corporate":
            if any(_job_hunt_member(tar, m) for m in members):
                raise machine.RoleError("this backup holds job-hunt data; restore it on your personal machine, never on a corporate one")
        # A link already in the destination (`dest/alias -> .`) would send `alias/machine.yaml` past the name checks.
        for m in members:
            parts = m.name.split("/")
            if any((dest.joinpath(*parts[:i])).is_symlink() for i in range(1, len(parts) + 1)):
                raise RuntimeError(f"restore destination holds a link on the path of {len(parts)}-part file name(s); remove the link or choose an empty folder; nothing restored")
        dest.mkdir(parents=True, exist_ok=True)
        tar.extractall(dest, members=members, filter="data")
        n = len(members)
    # The role lives in each data folder. A fresh folder would run as personal, so a corporate machine marks it.
    if machine.role() == "corporate" and not (dest / "machine.yaml").exists():
        store.write_yaml(dest / "machine.yaml", machine.info())
    events.emit("backup.restore", status="ok", files=n)
    return n
