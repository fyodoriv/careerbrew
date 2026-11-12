import pytest

from careerbrew import cli
from careerbrew.workday import plan, steps


@pytest.mark.parametrize("style", [">", ">-", ">+"])
def test_folded_description_shows_first_sentence(tmp_path, style):
    command = tmp_path / "commands" / "synthetic.md"
    command.parent.mkdir()
    command.write_text(f"---\ndescription: {style}\n  Check the connector and\n  list the result. Then continue.\n---\nBody.\n")
    assert steps.resolve("synthetic", [tmp_path])["description"] == "Check the connector and list the result."


@pytest.mark.parametrize(
    ("front", "expected", "user_only"),
    [
        ("description: Plain first. Later.", "Plain first.", False),
        ('description: "Quoted first. Later."', "Quoted first.", False),
        ("description: Plain first.\ndisable-model-invocation: true", "Plain first.", True),
        ("name: synthetic", "", False),
        ("description: >-\n  Broken text.\ninvalid: [", ">-", False),
    ],
)
def test_existing_description_controls_are_preserved(tmp_path, front, expected, user_only):
    command = tmp_path / "commands" / "synthetic.md"
    command.parent.mkdir()
    command.write_text(f"---\n{front}\n---\nBody.\n")
    result = steps.resolve("synthetic", [tmp_path])
    assert result["description"] == expected
    assert result["user_only"] is user_only


def test_deep_malformed_front_matter_keeps_the_existing_fallback(tmp_path):
    command = tmp_path / "commands" / "synthetic.md"
    command.parent.mkdir()
    nested = "[" * 700 + "x" + "]" * 700
    command.write_text(f"---\ndescription: >-\n  Synthetic first.\nnested: {nested}\n---\n")
    assert steps.resolve("synthetic", [tmp_path])["description"] == ">-"


def test_restricted_plan_hides_nonbuiltin_descriptions_but_allowed_plan_keeps_them(monkeypatch, capsys):
    cli.main(["init"])
    built = plan.build()
    base = built["steps"][0]
    built["steps"] = [
        {**base, "step": "/careerbrew", "from": "careerbrew", "about": "Public built-in."},
        {**base, "step": "/settings-step", "from": "settings", "about": "CANARY-SETTING-7Q"},
        {**base, "step": "/plugin-step", "from": "plugin:synthetic", "about": "CANARY-PLUGIN-3X"},
    ]
    monkeypatch.setattr(plan, "build", lambda **_kwargs: built)
    capsys.readouterr()
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    assert cli.main(["day", "run", "--json"]) == 0
    restricted = capsys.readouterr().out
    assert "Public built-in." in restricted
    assert "CANARY-SETTING-7Q" not in restricted
    assert "CANARY-PLUGIN-3X" not in restricted
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "run", "--json"]) == 0
    allowed = capsys.readouterr().out
    assert "CANARY-SETTING-7Q" in allowed
    assert "CANARY-PLUGIN-3X" in allowed


def test_restricted_builtin_fallback_does_not_expose_user_description(tmp_path, monkeypatch, capsys):
    command = tmp_path / "commands" / "careerbrew.md"
    command.parent.mkdir()
    command.write_text("---\ndescription: >-\n  CANARY-FALLBACK-9Z.\n---\n")
    resolved = steps.resolve("/careerbrew", [tmp_path])
    assert resolved["description"] == "CANARY-FALLBACK-9Z."
    cli.main(["init"])
    built = plan.build()
    built["steps"][0].update(path=resolved["path"], about=resolved["description"])
    monkeypatch.setattr(plan, "build", lambda **_kwargs: built)
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    assert "CANARY-FALLBACK-9Z" not in capsys.readouterr().out
    monkeypatch.setattr(cli, "may_show_business", lambda: True)
    assert cli.main(["day", "run", "--json"]) == 0
    assert "CANARY-FALLBACK-9Z" in capsys.readouterr().out


def test_restricted_builtin_plugin_fallback_inside_repo_is_not_public(tmp_path, monkeypatch, capsys):
    cli.main(["init"])
    built = plan.build()
    repo = tmp_path / "synthetic-repo"
    command = repo / "private" / "plugins" / "synthetic" / "commands" / "careerbrew.md"
    command.parent.mkdir(parents=True)
    command.write_text("---\ndescription: >-\n  CANARY-PLUGIN-FALLBACK-2Y.\n---\n")
    resolved = steps.resolve("/careerbrew", [command.parent.parent])
    assert resolved["description"] == "CANARY-PLUGIN-FALLBACK-2Y."
    built["steps"][0].update(path=resolved["path"], about=resolved["description"])
    monkeypatch.setattr(steps.paths, "REPO_ROOT", repo)
    monkeypatch.setattr(plan, "build", lambda **_kwargs: built)
    monkeypatch.setattr(cli, "may_show_business", lambda: False)
    capsys.readouterr()
    assert cli.main(["day", "run", "--json"]) == 0
    assert "CANARY-PLUGIN-FALLBACK-2Y" not in capsys.readouterr().out
