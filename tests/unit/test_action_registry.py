"""The generated action registry (rust-03): one file that the CLI, the web app, and the native pilot read.

scripts/gen-registry.py writes crates/cbcli/registry/registry.json and config/native-routes.txt from
the CLI parser. These tests give the "0 registry discrepancies" number: the committed files equal a
fresh export, the rows equal the parser walk and the frozen capture reference byte for byte, and
every action has its help text and a route line.
"""

import importlib.util
import json
import re
import sys
from pathlib import Path

from careerbrew import actions

ROOT = Path(__file__).resolve().parents[2]
FIX = "fix: uv run python scripts/gen-registry.py"


def _gen():
    spec = importlib.util.spec_from_file_location("gen_registry", ROOT / "scripts" / "gen-registry.py")
    assert spec and spec.loader
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def _committed() -> dict:
    return json.loads(actions.REGISTRY.read_text(encoding="utf-8"))


def _reference() -> Path:
    """The actions reference the Rust capture tests point at (REFERENCE in capture_actions.rs)."""
    src = (ROOT / "xtask" / "src" / "capture_actions.rs").read_text(encoding="utf-8")
    m = re.search(r'const REFERENCE: &str =\s*include_str!\("([^"]+)"\)', src)
    assert m, "capture_actions.rs names its REFERENCE"
    name = Path(m.group(1)).name
    return ROOT / "xtask" / "tests" / "fixtures" / "capture" / name


def test_the_committed_registry_and_routes_equal_a_fresh_export():
    gen = _gen()
    registry = actions.REGISTRY.read_text(encoding="utf-8")
    routes = gen.ROUTES.read_text(encoding="utf-8")
    found = gen.stale(registry, routes)
    assert not found, f"the action registry differs from the CLI parser at: {', '.join(found[:20])}\n{FIX}"
    assert registry == gen.render(gen.export()), FIX


def test_the_registry_rows_equal_the_parser_walk():
    walked = json.loads(json.dumps(actions.catalog_from_parser()))
    rows = actions.catalog()
    assert [r["name"] for r in rows] == [r["name"] for r in walked], FIX
    differ = [a["name"] for a, b in zip(rows, walked, strict=True) if a != b]
    assert not differ, f"{len(differ)} registry discrepancies: {differ[:10]}\n{FIX}"
    assert len(rows) == _committed()["meta"]["rows"]


def test_the_rows_print_byte_equal_to_the_frozen_actions_reference():
    reference = _reference()
    assert reference.is_file(), reference
    assert json.dumps(actions.catalog(), indent=1) + "\n" == reference.read_text(encoding="utf-8"), (
        f"`careerbrew actions --json` moved from {reference.name}: a catalog change needs a successor reference pair (AGENTS.md Gotchas, 2026-11-15)"
    )


def test_every_action_has_its_help_and_a_route_line():
    gen = _gen()
    reg = _committed()
    routes = gen.read_routes(gen.ROUTES.read_text(encoding="utf-8"))
    names = [r["name"] for r in reg["rows"]]
    assert list(routes) == names, "one route line per action, in registry order"
    assert set(routes.values()) <= set(gen.ROUTE_VALUES)
    for name in names:
        assert name.replace(".", " ") in reg["help"], f"{name}: no help text in the registry"
    assert "" in reg["help"] and reg["help"][""].startswith("usage: careerbrew"), "the root help is stored"
    assert reg["notes"]["terminal_only"] == actions.TERMINAL_ONLY
    assert reg["notes"]["confirm"] == actions.CONFIRM
    assert set(reg["notes"]["yes_from_confirm"]) == actions.YES_FROM_CONFIRM
    assert reg["notes"]["agent_notes"] == actions.AGENT_NOTES


def test_the_help_was_rendered_by_the_pinned_python_at_80_columns():
    meta = _committed()["meta"]
    pinned = (ROOT / ".python-version").read_text(encoding="utf-8").strip()
    assert meta["columns"] == 80
    assert meta["python"] == ".".join(pinned.split(".")[:2]), "regenerate with the pinned Python"
    assert meta["python"] == f"{sys.version_info.major}.{sys.version_info.minor}", "argparse help changes between Python minor versions: run the tests on the pinned Python"


def test_the_help_width_ignores_the_terminal(monkeypatch):
    gen = _gen()
    monkeypatch.setenv("COLUMNS", "200")
    assert gen.export()["help"] == _committed()["help"], "the export pins COLUMNS=80 whatever the terminal is"
    import os

    assert os.environ["COLUMNS"] == "200", "the export restores the caller's COLUMNS"


def test_a_stale_registry_names_what_differs_and_the_fix(tmp_path, monkeypatch, capsys):
    gen = _gen()
    reg = _committed()
    reg["rows"][0]["help"] = "changed by hand"
    del reg["help"]["config set"]
    bad = tmp_path / "registry.json"
    bad.write_text(gen.render(reg), encoding="utf-8")
    monkeypatch.setattr(gen, "REGISTRY", bad)
    monkeypatch.setattr(gen, "ROOT", tmp_path)
    monkeypatch.setattr(gen, "ROUTES", tmp_path / "native-routes.txt")
    (tmp_path / "native-routes.txt").write_text(gen.ROUTES_HEADER + "init native\n", encoding="utf-8")
    assert gen.main(["--check"]) == 1
    err = capsys.readouterr().err
    assert f"rows.{reg['rows'][0]['name']}" in err and "help['config set']" in err and "native-routes.txt" in err
    assert FIX.removeprefix("fix: ") in err


def test_a_regeneration_keeps_a_native_route(tmp_path, monkeypatch):
    gen = _gen()
    monkeypatch.setattr(gen, "REGISTRY", tmp_path / "registry.json")
    monkeypatch.setattr(gen, "ROUTES", tmp_path / "native-routes.txt")
    (tmp_path / "native-routes.txt").write_text(gen.ROUTES_HEADER + "actions native\ngone.away native\n", encoding="utf-8")
    assert gen.main([]) == 0
    routes = gen.read_routes((tmp_path / "native-routes.txt").read_text(encoding="utf-8"))
    assert routes["actions"] == "native" and routes["init"] == "python"
    assert "gone.away" not in routes, "a removed action loses its line"
    assert (tmp_path / "registry.json").read_text(encoding="utf-8") == actions.REGISTRY.read_text(encoding="utf-8")


def test_a_missing_registry_fails_with_the_fix(tmp_path, monkeypatch):
    import pytest

    monkeypatch.setattr(actions, "REGISTRY", tmp_path / "missing.json")
    with pytest.raises(actions.ActionError, match="gen-registry.py"):
        actions.catalog()
