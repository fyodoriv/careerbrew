"""Every Rust workspace crate either ships nothing or is rebuilt after a pull.

A merged Rust change must reach the machine. Today no crate runs in
production: the launcher runs Python through uv. This test fails when a crate
is added, or its "Production use" cell in docs/rust-migration.md stops being
"none", while scripts/rust-shipped-crates.txt does not list it. setup.sh
rebuilds every crate on that list. A scan also fails when app code reaches a
Rust binary or a target/ path for a crate that is not on the list.
"""

import re
import tomllib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
NOT_SHIPPED = re.compile(r"^none(:|$)")
# Developer tooling that builds or checks Rust on purpose; it ships nothing.
SCAN_SKIP = {"scripts/check.sh", "scripts/doctor.sh", "scripts/rust-readiness.sh", "scripts/setup.sh"}


def members(root: Path = ROOT) -> list[str]:
    return tomllib.loads((root / "Cargo.toml").read_text())["workspace"]["members"]


def production_use(doc: str) -> dict[str, str]:
    heads = [m.start() for m in re.finditer(r"^### Crates\b", doc, re.M)]
    assert len(heads) == 1, "docs/rust-migration.md needs exactly one '### Crates' heading"
    table = doc[heads[0] :].split("\n### ", 1)[0]
    rows: dict[str, str] = {}
    for line in table.splitlines():
        cells = [c.strip() for c in line.strip().strip("|").split("|")]
        if len(cells) == 4 and cells[0].startswith("`"):
            rows[cells[0].strip("`").rstrip("/")] = cells[3]
    return rows


def shipped_list(text: str) -> set[str]:
    return {ln.strip() for ln in text.splitlines() if ln.strip() and not ln.startswith("#")}


def unlisted_shipped(rows: dict[str, str], shipped: set[str]) -> list[str]:
    return sorted(m for m, use in rows.items() if not NOT_SHIPPED.match(use) and m not in shipped)


def _rows() -> dict[str, str]:
    return production_use((ROOT / "docs/rust-migration.md").read_text())


def _shipped() -> set[str]:
    return shipped_list((ROOT / "scripts/rust-shipped-crates.txt").read_text())


def test_every_crate_has_a_production_use_row():
    assert sorted(_rows()) == sorted(members())


def test_a_crate_that_ships_is_on_the_rebuild_list():
    assert unlisted_shipped(_rows(), _shipped()) == []


def test_a_planted_shipped_crate_off_the_list_fails():
    rows = {"crates/vault": "native vault for every read", "xtask": "none"}
    assert unlisted_shipped(rows, set()) == ["crates/vault"]
    assert unlisted_shipped(rows, {"crates/vault"}) == []
    assert unlisted_shipped({"x": "none yet, ships soon"}, set()) == ["x"]


def test_the_rebuild_list_names_only_workspace_crates():
    assert _shipped() <= set(members())


def test_setup_rebuilds_every_listed_crate():
    setup = (ROOT / "scripts/setup.sh").read_text()
    assert "done < scripts/rust-shipped-crates.txt" in setup
    assert re.search(r'cargo" build --release --locked --manifest-path "\$crate/Cargo.toml"', setup)


def test_two_crates_headings_are_refused():
    with pytest.raises(AssertionError):
        production_use("### Crates\n\n### Crates\n")


def _package_names() -> dict[str, str]:
    names = {}
    for m in members():
        data = tomllib.loads((ROOT / m / "Cargo.toml").read_text())
        names[data["package"]["name"]] = m
    return names


def test_app_code_reaches_no_rust_binary_off_the_list():
    shipped = _shipped()
    names = {n: m for n, m in _package_names().items() if m not in shipped}
    words = re.compile(r"target/(debug|release)|\b(" + "|".join(map(re.escape, names)) + r")\b")
    hits = []
    for top in ("src", "web/src", "commands", "scripts"):
        for path in sorted((ROOT / top).rglob("*")):
            rel = path.relative_to(ROOT).as_posix()
            if not path.is_file() or rel in SCAN_SKIP or path.suffix in {".png", ".txt"}:
                continue
            text = path.read_text(errors="ignore")
            hits += [f"{rel}:{n}" for n, ln in enumerate(text.splitlines(), 1) if words.search(ln)]
    assert hits == [], f"app code reaches a Rust binary that setup never rebuilds: {hits}"
