"""rust-03 per-action native route: the `_native_route` prelude at the top of `cli.main` (docs/rust-migration.md).

Every case uses a fake `careerbrew-native` in tmp_path and a scratch route file; nothing here builds or runs the real
binary (the e2e story US-111 does). rust-26 deletes this file with the shim.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
from pathlib import Path

import pytest

from careerbrew import actions, cli

ROOT = Path(__file__).resolve().parents[2]


@pytest.fixture
def route(home, tmp_path, monkeypatch):
    """A scratch route file and a fake native binary that records its argv and exits with $FAKE_RC."""
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    for key in ("FAKE_OUT", "FAKE_SIGNAL", "FAKE_RC", "CAREERBREW_NATIVE", "CAREERBREW_NATIVE_BIN", "CAREERBREW_ROUTE_ACTIONS", "CAREERBREW_ROUTE_CONFIG_SHOW", "COLUMNS"):
        monkeypatch.delenv(key, raising=False)
    routes = tmp_path / "native-routes.txt"
    routes.write_text("# header\nactions python\nconfig.show python\n", encoding="utf-8")
    monkeypatch.setattr(cli, "NATIVE_ROUTES", routes)
    marker = tmp_path / "native-ran.txt"
    fake = tmp_path / "careerbrew-native"
    fake.write_text(f'#!/bin/sh\nprintf "%s\\n" "$@" > "{marker}"\necho "fake stderr" >&2\n[ -n "$FAKE_OUT" ] && echo "$FAKE_OUT"\n[ -n "$FAKE_SIGNAL" ] && kill -"$FAKE_SIGNAL" $$\nexit "${{FAKE_RC:-0}}"\n', encoding="utf-8")
    fake.chmod(0o755)
    monkeypatch.setattr(cli, "NATIVE_BIN", fake)

    class Route:
        def __init__(self):
            self.routes, self.marker, self.fake = routes, marker, fake

        def set(self, text: str) -> None:
            routes.write_text("# header\n" + text, encoding="utf-8")

        def ran(self) -> list[str] | None:
            return marker.read_text(encoding="utf-8").splitlines() if marker.exists() else None

        def events(self) -> list[dict]:
            log = home / "logs" / "events.jsonl"
            return [json.loads(line) for line in log.read_text().splitlines()] if log.exists() else []

    return Route()


def test_the_committed_route_file_routes_every_action_to_python():
    """Default python: with the committed table, behavior is the same as before the prelude."""
    routes = cli._native_routes()
    assert routes and set(routes.values()) == {"python"}
    assert cli.NATIVE_ROUTES == ROOT / "config" / "native-routes.txt"


def test_a_python_route_does_not_run_the_binary(route):
    assert cli._native_route(["actions", "--json"]) is None
    assert route.ran() is None and route.events() == []


def test_a_native_route_runs_the_binary_with_the_full_argv_and_returns_its_code(route, monkeypatch):
    route.set("actions native\n")
    monkeypatch.setenv("FAKE_RC", "3")
    assert cli._native_route(["actions", "--json"]) == 3
    assert route.ran() == ["actions", "--json"]


def test_a_two_word_action_is_routed_by_its_dotted_name(route):
    route.set("config.show native\n")
    assert cli._native_route(["config", "show"]) == 0
    assert route.ran() == ["config", "show"]


def test_a_missing_binary_falls_back_to_python_with_a_content_free_event(route, monkeypatch, tmp_path):
    route.set("actions native\n")
    monkeypatch.setenv("CAREERBREW_NATIVE_BIN", str(tmp_path / "gone"))
    assert cli._native_route(["actions", "--json"]) is None
    (event,) = route.events()
    assert event["event"] == "cli.native_fallback" and event["command"] == "actions"
    assert str(tmp_path) not in json.dumps(event)


def test_a_file_that_is_not_executable_falls_back_too(route):
    route.set("actions native\n")
    route.fake.chmod(0o644)
    assert cli._native_route(["actions"]) is None
    assert [e["event"] for e in route.events()] == ["cli.native_fallback"]


def test_exit_64_falls_back_to_python_and_logs_unsupported(route, monkeypatch, capsys):
    route.set("actions native\n")
    monkeypatch.setenv("FAKE_RC", "64")
    assert cli._native_route(["actions", "--json"]) is None
    assert route.ran() == ["actions", "--json"]
    assert [e["event"] for e in route.events()] == ["cli.native_unsupported"]
    assert "fake stderr" not in capsys.readouterr().err  # the native refusal is not shown before Python runs


def test_the_kill_switch_keeps_python(route, monkeypatch):
    route.set("actions native\n")
    monkeypatch.setenv("CAREERBREW_NATIVE", "off")
    assert cli._native_route(["actions", "--json"]) is None
    assert route.ran() is None


def test_the_env_override_wins_both_ways(route, monkeypatch):
    route.set("actions native\nconfig.show python\n")
    monkeypatch.setenv("CAREERBREW_ROUTE_ACTIONS", "python")
    assert cli._native_route(["actions"]) is None
    monkeypatch.setenv("CAREERBREW_ROUTE_CONFIG_SHOW", "native")
    assert cli._native_route(["config", "show"]) == 0
    assert route.ran() == ["config", "show"]


def test_an_unknown_override_value_keeps_the_file_route(route, monkeypatch):
    route.set("actions python\n")
    monkeypatch.setenv("CAREERBREW_ROUTE_ACTIONS", "yes")
    assert cli._native_route(["actions"]) is None


@pytest.mark.parametrize("argv", [["--version"], ["-h"], ["--bogus", "actions"], [], ["--no-watch"], ["--no-web", "actions"], ["nosuch"]])
def test_a_leading_flag_a_bare_run_or_an_unknown_command_goes_to_python(route, argv):
    route.set("actions native\n")
    assert cli._native_route(argv) is None
    assert route.ran() is None


def test_help_routes_native_only_at_80_columns(route, monkeypatch):
    route.set("actions native\n")
    assert cli._native_route(["actions", "--help"]) is None
    monkeypatch.setenv("COLUMNS", "120")
    assert cli._native_route(["actions", "-h"]) is None
    monkeypatch.setenv("COLUMNS", "80")
    assert cli._native_route(["actions", "-h"]) == 0


def test_a_missing_route_file_routes_nothing(route, monkeypatch, tmp_path):
    monkeypatch.setattr(cli, "NATIVE_ROUTES", tmp_path / "absent.txt")
    assert cli._native_route(["actions"]) is None


def test_main_with_argv_never_routes(route, monkeypatch):
    """`cli.main(argv)` (tests, `actions.run`) skips the prelude; only the console script's `main()` routes."""
    route.set("actions native\n")
    monkeypatch.setattr(sys, "argv", ["careerbrew", "actions", "--json"])
    assert cli.main(["actions", "--json"]) == 0
    assert route.ran() is None
    assert cli.main() == 0
    assert route.ran() == ["actions", "--json"]


def test_the_web_path_never_runs_the_native_binary(route, monkeypatch):
    route.set("actions native\n")
    monkeypatch.setattr(sys, "argv", ["careerbrew", "actions", "--json"])
    actions.run("actions", {})
    assert route.ran() is None


def test_exit_64_after_stdout_shows_none_of_it(route, monkeypatch, capsys):
    """Review: a refusal that printed first would duplicate output once Python runs; native output is held until exit."""
    route.set("actions native\n")
    monkeypatch.setenv("FAKE_RC", "64")
    monkeypatch.setenv("FAKE_OUT", "partial native line")
    assert cli._native_route(["actions", "--json"]) is None
    assert "partial native line" not in capsys.readouterr().out


def test_native_stdout_and_stderr_reach_the_caller(route, monkeypatch, capsys):
    route.set("actions native\n")
    monkeypatch.setenv("FAKE_OUT", "native out")
    assert cli._native_route(["actions", "--json"]) == 0
    seen = capsys.readouterr()
    assert seen.out == "native out\n" and seen.err == "fake stderr\n"


def test_a_signal_killed_child_exits_128_plus_the_signal(route, monkeypatch):
    route.set("actions native\n")
    monkeypatch.setenv("FAKE_SIGNAL", "9")
    assert cli._native_route(["actions", "--json"]) == 137


def test_a_binary_that_cannot_start_falls_back_to_python(route, monkeypatch):
    route.set("actions native\n")
    route.fake.write_text("#!/nonexistent/interpreter\n", encoding="utf-8")
    assert cli._native_route(["actions", "--json"]) is None
    assert [e["event"] for e in route.events()] == ["cli.native_fallback"]


def test_ctrl_c_during_a_native_run_exits_130_with_no_traceback(route, monkeypatch):
    route.set("actions native\n")

    def interrupted(*_a, **_k):
        raise KeyboardInterrupt

    monkeypatch.setattr(subprocess, "run", interrupted)
    assert cli._native_route(["actions", "--json"]) == 130


def test_python_dash_m_careerbrew_cli_runs(home):
    """Review: the route helpers sat after the `__main__` guard, so `python -m careerbrew.cli` hit NameError."""
    done = subprocess.run([sys.executable, "-m", "careerbrew.cli", "--help"], capture_output=True, text=True, env={**os.environ, "CAREERBREW_HOME": str(home)}, check=False, timeout=120)
    assert done.returncode == 0, done.stderr
    assert "NameError" not in done.stderr
