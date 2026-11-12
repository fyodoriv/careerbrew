"""Every careerbrew action runs three ways: CLI, agents, and the web app (goal 2026-10-20).

The registry is generated from the CLI parser. This file fails when a command misses a path:
not in the registry, not in the agent catalog, not runnable from the web app without a written
terminal-only reason, or the agent command does not point at the catalog.
"""

import argparse
import json
from pathlib import Path

import pytest

from careerbrew import actions
from careerbrew.cli import main

ROOT = Path(__file__).resolve().parents[2]


def _cli_leaves() -> set[str]:
    from careerbrew.cli import build_parser

    return {".".join(p) for p, _ in actions._leaves(build_parser()) if p}


def test_every_cli_command_is_an_action_with_a_ui_path_or_a_reason():
    cat = {a["name"]: a for a in actions.catalog()}
    leaves = _cli_leaves()
    assert leaves == set(cat), "registry = CLI leaves, nothing missing, nothing extra"
    for name, a in cat.items():
        assert a["ui"] or a["terminal_only"], f"{name}: runs in the web app or names why it cannot"
    assert set(actions.TERMINAL_ONLY) <= leaves, "no stale terminal-only entries"
    assert set(actions.CONFIRM) <= leaves, "no stale confirm entries"
    assert len(leaves) >= 110


def test_agents_get_the_same_catalog(capsys):
    assert main(["actions", "--json"]) == 0
    agent = {a["name"] for a in json.loads(capsys.readouterr().out)}
    assert agent == _cli_leaves()
    cmd = (ROOT / "commands" / "careerbrew.md").read_text()
    assert "careerbrew actions" in cmd, "the /careerbrew agent command points agents at the catalog"


def test_web_app_lists_every_action(home):
    from careerbrew import server

    assert {a["name"] for a in server.actions_view()["actions"]} == _cli_leaves()


def test_the_catalog_and_a_web_run_build_the_cli_parser_once_each(home, monkeypatch):
    # The catalog looked up each action's help in a new parser: 133 builds, about 1 s of CPU
    # for the Actions view and for each run from the web app, over 5 s at load ~85 (us-35).
    from careerbrew import cli

    real, built = cli.build_parser, []
    monkeypatch.setattr(cli, "build_parser", lambda: built.append(1) or real())
    cat = {a["name"]: a for a in actions.catalog()}
    assert len(built) == 1
    assert cat["config.show"]["help"] == "every setting, its value, and where it comes from", "help still comes from the subcommand"
    built.clear()
    assert actions.run("config.show", {})["exit"] == 0
    assert len(built) == 2, "one for the catalog, one for the command"


SAFE_SKIP = {"day.watch"}  # needs an argument; covered by test_day.py


def test_confirmation_supplies_each_confirmed_yes_flag():
    names = {a["name"] for a in actions.catalog() if a["confirm"] and any(arg["name"] == "yes" and arg["flag"] == "--yes" and arg["kind"] == "flag" for arg in a["args"])}
    assert len(names) == 9
    assert names == actions.YES_FROM_CONFIRM


def test_decrypt_confirmation_supplies_yes_without_touching_data(monkeypatch):
    from careerbrew import cli

    calls = []
    monkeypatch.setattr(cli, "main", lambda argv: calls.append(argv) or 0)
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("data.decrypt", {}, confirm=False)
    assert calls == []
    assert actions.run("data.decrypt", {}, confirm=True)["exit"] == 0
    assert calls == [["data", "decrypt", "--yes"]]


def test_confirmed_decrypt_changes_only_synthetic_home(home):
    from careerbrew import store, vault

    path = home / "profile.yaml"
    store.write_yaml(path, {"name": "Synthetic test"})
    assert vault.is_sealed(path.read_bytes())
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("data.decrypt", {}, confirm=False)
    assert vault.is_sealed(path.read_bytes())
    result = actions.run("data.decrypt", {}, confirm=True)
    assert result["exit"] == 0
    assert not vault.is_sealed(path.read_bytes())
    assert store.read_yaml(path)["name"] == "Synthetic test"


@pytest.mark.parametrize("name", sorted(a["name"] for a in actions.catalog() if a["ui"] and not a["confirm"] and not any(x["required"] for x in a["args"])))
def test_every_argument_free_action_runs_from_the_web_app(name, home, monkeypatch, tmp_path):
    monkeypatch.setenv("CAREERBREW_HTTP_FIXTURES", str(ROOT / "tests" / "fixtures" / "http"))
    monkeypatch.setenv("CAREERBREW_NOTIFY", "off")
    monkeypatch.setenv("CAREERBREW_COMMANDS_DIR", str(tmp_path / "commands"))
    from careerbrew.day import watch

    monkeypatch.setattr(watch, "start", lambda interval=None: {"state": "on"})
    r = actions.run(name, {})
    assert isinstance(r["exit"], int) and "Traceback" not in r["output"], (name, r["output"][-500:])


def test_terminal_only_confirm_and_validation(home):
    with pytest.raises(actions.ActionError, match="terminal only"):
        actions.run("secret.set", {"service": "x"})
    with pytest.raises(actions.ActionError, match="confirm"):
        actions.run("machine.set", {"role": "corporate"})
    with pytest.raises(actions.ActionError, match="required"):
        actions.run("config.set", {})
    with pytest.raises(actions.ActionError, match="one of"):
        actions.run("machine.set", {"role": "boss"}, confirm=True)
    with pytest.raises(actions.ActionError, match="unknown field"):
        actions.run("config.show", {"nope": 1})
    r = actions.run("config.set", {"key": "work_end", "value": "18:00"})
    assert r["exit"] == 0 and "saved to settings.yaml" in r["output"] and r["argv"] == ["careerbrew", "config", "set", "--", "work_end", "18:00"]
    assert actions.run("machine.set", {"role": "corporate"}, confirm=True)["exit"] == 0


def test_owner_ui_sees_business_text_an_agent_does_not(home, monkeypatch, capsys):
    monkeypatch.setenv("CAREERBREW_NOW", "2026-10-16T10:00")
    from careerbrew.work import priorities

    priorities.add("Ship the invoice export", 4, 4, 1, [])
    assert "Ship the invoice export" in actions.run("work.focus", {})["output"], "the owner's web app shows titles"
    main(["work", "focus"])
    assert "Ship the invoice export" not in capsys.readouterr().out, "an agent without a terminal does not"


def test_a_repeatable_option_takes_a_list_in_the_catalog():
    from careerbrew.cli import build_parser

    for path, sp in actions._leaves(build_parser()):
        for a in sp._actions:
            if isinstance(a, argparse._AppendAction):
                arg = actions._arg(a)
                assert arg and arg["many"], f"{'.'.join(path)} {a.dest}: a repeatable option runs with several values from agents and the web app"


def test_hard_to_undo_actions_always_ask_to_confirm():
    must = {"plugin.add", "plugin.remove", "data.decrypt", "data.encrypt", "backup.restore", "sync.export", "sync.import", "machine.set", "day.send", "approvals.decide", "exercises.submit", "research.page"}
    assert must <= set(actions.CONFIRM), "this list may grow, never shrink"


def test_values_cannot_inject_flags(home):
    with pytest.raises(actions.ActionError, match="one value"):
        actions.run("plugin.add", {"source": ["./evil", "--allow-exec"]}, confirm=True)
    with pytest.raises(actions.ActionError, match="cannot start with"):
        actions.run("config.set", {"key": "--purge-data", "value": "x"})
    with pytest.raises(actions.ActionError, match="on or off"):
        actions.run("day.log", {"show": "yes"})
    with pytest.raises(actions.ActionError, match="whole number"):
        actions.run("interview", {"company": "Stripe", "level": "3; rm -rf"})
    action = next(x for x in actions.catalog() if x["name"] == "day.send")
    assert actions.argv_for(action, {"text": "- shipped the export"}) == ["day", "send", "--text=- shipped the export"], "a value starting with '-' stays a value"


def test_a_slow_action_does_not_block_or_mix_with_another(home, monkeypatch):
    import threading

    from careerbrew import cli

    # No wall-clock limit: the gate runs on a loaded machine. The fast action must
    # finish while the slow one is still held, which proves it did not wait.
    gate = threading.Event()
    started = threading.Event()
    real_main = cli.main

    def fake_main(argv):
        if argv[:2] == ["config", "show"]:
            print("slow started")
            started.set()
            gate.wait(60)
            print("slow done")
            return 0
        return real_main(argv)

    monkeypatch.setattr(cli, "main", fake_main)
    out: dict = {}
    t = threading.Thread(target=lambda: out.setdefault("slow", actions.run("config.show", {})))
    t.start()
    assert started.wait(30), "the slow action never started"
    fast = actions.run("config.set", {"key": "work_end", "value": "18:00"})
    assert t.is_alive() and "slow" not in out, "the fast action waited for the slow one"
    gate.set()
    t.join(30)
    assert "slow" not in fast["output"] and "saved to settings.yaml" in fast["output"]
    assert out["slow"]["output"] == "slow started\nslow done\n"
