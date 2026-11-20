"""Settings file IO: read and write `<data root>/settings.yaml` (rust-05a).

`config` keeps the schema, the validators and the settings lock; this module
only moves the file in and out of encrypted storage, so the native storage port
can own it without the schema.
"""

from __future__ import annotations

from pathlib import Path
from typing import Any

from . import paths, store


def settings_path() -> Path:
    return paths.data_root() / "settings.yaml"


def read_settings() -> dict[str, Any]:
    """The saved overrides; an empty or missing file reads as no overrides."""
    return store.read_yaml(settings_path(), {}) or {}


def write_settings(data: dict[str, Any]) -> None:
    store.write_yaml(settings_path(), data)
