"""careerbrew: a private, local-first career assistant for programmers."""

from importlib import metadata as _metadata

# pyproject.toml holds the one version; uv writes it into the installed package's metadata.
# scripts/release.sh changes it (tests/unit/test_version.py).
try:
    __version__ = _metadata.version("careerbrew")
except _metadata.PackageNotFoundError:  # a copy of the source that was never installed
    __version__ = "0.0.0"

# The project's own addresses. Bug report commands and User-Agent headers read them here.
REPO = "fyodoriv/careerbrew"
CONTACT = "github.com/fyodoriv"
