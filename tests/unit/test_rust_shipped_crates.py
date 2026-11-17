"""Every Rust workspace crate either ships nothing or is rebuilt after a pull.

A merged Rust change must reach the machine. Today no crate runs in
production: the launcher runs Python through uv. This test fails when a crate
is added, or its "Production use" cell in docs/rust-migration.md stops being
"none", while scripts/rust-shipped-crates.txt does not list it. That list is
the rebuild trigger that setup must act on once a crate ships.
"""

import re
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def _members() -> list[str]:
    data = tomllib.loads((ROOT / "Cargo.toml").read_text())
    return data["workspace"]["members"]


def _production_use() -> dict[str, str]:
    text = (ROOT / "docs/rust-migration.md").read_text()
    table = text.split("### Crates", 1)[1].split("\n### ", 1)[0]
    rows: dict[str, str] = {}
    for line in table.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) == 4 and cells[0].startswith("`"):
            rows[cells[0].strip("`").rstrip("/")] = cells[3]
    return rows


def _shipped_list() -> set[str]:
    path = ROOT / "scripts/rust-shipped-crates.txt"
    lines = path.read_text().splitlines()
    return {ln.strip() for ln in lines if ln.strip() and not ln.startswith("#")}


def test_every_crate_has_a_production_use_row():
    rows = _production_use()
    assert sorted(rows) == sorted(_members())


def test_a_crate_that_ships_is_on_the_rebuild_list():
    shipped = _shipped_list()
    for member, use in _production_use().items():
        if not re.match(r"none\b", use):
            assert member in shipped, f"{member} ships but setup never rebuilds it"


def test_the_rebuild_list_names_only_workspace_crates():
    assert _shipped_list() <= set(_members())


def test_setup_reads_the_rebuild_list():
    setup = (ROOT / "scripts/setup.sh").read_text()
    shipped = _shipped_list()
    if shipped:
        assert "rust-shipped-crates.txt" in setup
