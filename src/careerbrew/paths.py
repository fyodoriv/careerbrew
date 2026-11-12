"""Where careerbrew keeps data.

Personal and business data records live under one data root (the few files
elsewhere, such as a project's memory file, are in docs/privacy.md). The default is
`private/` in the repo, which git never tracks. `CAREERBREW_HOME` moves it
(tests point it at a temporary folder).
"""

from __future__ import annotations

import os
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]


def data_root(*, create: bool = True) -> Path:
    env = os.environ.get("CAREERBREW_HOME")
    if env is not None and not env.strip():
        # A failed `$(mktemp -d)` leaves it empty; falling back would write test data into real data.
        raise RuntimeError("CAREERBREW_HOME is set but empty; unset it to use private/, or point it at a folder")
    root = Path(env).expanduser() if env else REPO_ROOT / "private"
    if not create:
        return root
    root.mkdir(parents=True, exist_ok=True)
    try:
        root.chmod(0o700)
    except OSError:
        pass
    return root


def sub(*parts: str) -> Path:
    path = data_root().joinpath(*parts)
    path.parent.mkdir(parents=True, exist_ok=True)
    return path


def require_outside_data_root(path: Path, description: str) -> None:
    if path.expanduser().resolve().is_relative_to(data_root(create=False).resolve()):
        raise ValueError(f"{description} must be outside the data folder")


def config_dir() -> Path:
    return REPO_ROOT / "config"


def extra_knowledge() -> bool:
    """False when CAREERBREW_EXTRA_KNOWLEDGE=0: read only the base knowledge set (the sample companies).

    Tests and pictures set it, so they give the same result with or without the optional batch files.
    """
    return os.environ.get("CAREERBREW_EXTRA_KNOWLEDGE", "1") != "0"
