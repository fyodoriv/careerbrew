"""Non-interactive profile updates use synthetic values only."""

from concurrent.futures import ThreadPoolExecutor

import pytest

from careerbrew import actions, store, vault
from careerbrew.cli import main


@pytest.mark.parametrize("field", ["name", "employer"])
def test_profile_update_is_noninteractive_encrypted_and_redacted(home, capsys, field):
    path = home / "profile.yaml"
    store.write_text(path, "# Keep this comment\nname: Old Name # name note\nemployer: Old Employer\nother: {nested: kept}\n")
    secret = f"SYNTHETIC-{field.upper()}-CANARY"
    assert main(["data", "set", "profile.yaml", f"--{field}", secret]) == 0
    out = capsys.readouterr()
    assert secret not in out.out + out.err
    assert vault.is_sealed(path.read_bytes())
    assert secret.encode() not in path.read_bytes()
    text = store.read_text(path)
    assert text is not None and "# Keep this comment" in text and "# name note" in text
    data = store.read_yaml(path)
    assert data[field] == secret
    assert data["other"] == {"nested": "kept"}


def test_web_action_redacts_supplied_values(home):
    store.write_text(home / "profile.yaml", "name: Old Name\nemployer: Old Employer\n")
    secret = "SYNTHETIC-PRIVATE-NAME"
    result = actions.run("data.set", {"file": "profile.yaml", "name": secret})
    assert result["exit"] == 0
    assert secret not in str(result)


@pytest.mark.parametrize("args", [["other.yaml", "--name", "CANARY-INVALID"], ["profile.yaml", "--name", ""], ["profile.yaml", "--name", "line\nbreak"], ["profile.yaml"]])
def test_invalid_update_keeps_existing_bytes(home, capsys, args):
    path = home / "profile.yaml"
    store.write_text(path, "name: Original\nemployer: Original\n")
    before = path.read_bytes()
    assert main(["data", "set", *args]) != 0
    assert path.read_bytes() == before
    assert "CANARY-INVALID" not in capsys.readouterr().out


def test_symlinked_profile_is_not_read_or_replaced(home, tmp_path):
    home.mkdir(parents=True)
    outside = tmp_path / "outside.yaml"
    outside.write_text("name: outside\n")
    path = home / "profile.yaml"
    path.symlink_to(outside)
    assert main(["data", "set", "profile.yaml", "--name", "synthetic"]) != 0
    assert path.is_symlink() and outside.read_text() == "name: outside\n"


def test_invalid_web_update_does_not_echo_values(home):
    store.write_text(home / "profile.yaml", "name: Original\n")
    secret = "SYNTHETIC-INVALID-CANARY"
    result = actions.run("data.set", {"file": "wrong.yaml", "name": secret})
    assert result["exit"] != 0
    assert secret not in str(result)


def test_unknown_option_does_not_echo_its_value(home, capsys):
    store.write_text(home / "profile.yaml", "name: Original\n")
    with pytest.raises(SystemExit):
        main(["data", "set", "profile.yaml", "--unknown=SYNTHETIC-PRIVATE-CANARY"])
    assert "SYNTHETIC-PRIVATE-CANARY" not in capsys.readouterr().err


def test_parallel_profile_updates_keep_both_fields(home):
    from careerbrew import machine

    path = home / "profile.yaml"
    store.write_text(path, "name: Old Name\nemployer: Old Employer\nother: kept\n")
    with ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(machine.set_profile_fields, [{"name": "Synthetic Name"}, {"employer": "Synthetic Employer"}]))
    assert results == [["name"], ["employer"]]
    assert store.read_yaml(path) == {"name": "Synthetic Name", "employer": "Synthetic Employer", "other": "kept"}


def test_profile_update_events_do_not_include_values(home, monkeypatch):
    from careerbrew import events

    store.write_text(home / "profile.yaml", "name: Old Name\nemployer: Old Employer\n")
    monkeypatch.setenv("CAREERBREW_EVENTS", "on")
    secret = "SYNTHETIC-EVENT-CANARY"
    assert main(["data", "set", "profile.yaml", "--name", secret]) == 0
    event_text = events.log_path().read_text()
    assert '"event": "data.set"' in event_text
    assert secret not in event_text


@pytest.mark.parametrize("text", ["name: [broken\n", "name: One\nname: Two\n", "name: &shared One\nemployer: *shared\n", "name: |\n  multiline\n"])
def test_unsupported_profile_is_preserved(home, text):
    path = home / "profile.yaml"
    store.write_text(path, text)
    before = path.read_bytes()
    assert main(["data", "set", "profile.yaml", "--name", "Synthetic"]) != 0
    assert path.read_bytes() == before


def test_two_field_update_validates_all_before_writing(home):
    path = home / "profile.yaml"
    store.write_text(path, "name: Original\nemployer: Original\n")
    before = path.read_bytes()
    assert main(["data", "set", "profile.yaml", "--name", "Valid", "--employer", ""]) != 0
    assert path.read_bytes() == before


def test_missing_key_does_not_create_a_replacement(home):
    from careerbrew import backup

    path = home / "profile.yaml"
    store.write_text(path, "name: Original\n")
    before = path.read_bytes()
    key = backup.key_path()
    key.unlink()
    vault._fernet.cache_clear()
    assert main(["data", "set", "profile.yaml", "--name", "Synthetic"]) != 0
    assert path.read_bytes() == before and not key.exists()


def test_values_starting_with_dash_work_in_cli_and_web(home):
    path = home / "profile.yaml"
    store.write_text(path, "name: Original\nemployer: Original\n")
    assert main(["data", "set", "profile.yaml", "--name=-Synthetic"]) == 0
    result = actions.run("data.set", {"file": "profile.yaml", "employer": "-Synthetic Employer"})
    assert result["exit"] == 0 and "-Synthetic Employer" not in str(result)
    assert store.read_yaml(path)["name"] == "-Synthetic"
    assert store.read_yaml(path)["employer"] == "-Synthetic Employer"


def test_empty_yaml_fields_can_be_filled_without_losing_comments(home):
    path = home / "profile.yaml"
    store.write_text(path, "name: # keep name comment\nemployer: null # keep employer comment\n")
    assert main(["data", "set", "profile.yaml", "--name", "Synthetic Name", "--employer", "Synthetic Employer"]) == 0
    text = store.read_text(path)
    assert text is not None and "# keep name comment" in text and "# keep employer comment" in text
    assert store.read_yaml(path) == {"name": "Synthetic Name", "employer": "Synthetic Employer"}


def test_unrelated_invalid_redactor_field_does_not_hide_success(home, capsys):
    path = home / "profile.yaml"
    store.write_text(path, "name: Old\nemployer: Old\nsensitive: 1\n")
    assert main(["data", "set", "profile.yaml", "--name", "Synthetic Name"]) == 0
    assert store.read_yaml(path) == {"name": "Synthetic Name", "employer": "Old", "sensitive": 1}
    assert "saved profile.yaml fields: name" in capsys.readouterr().out


def test_profile_update_works_on_corporate_role(home):
    store.write_yaml(home / "machine.yaml", {"role": "corporate"})
    store.write_text(home / "profile.yaml", "name: Old\nemployer: Old\n")
    assert main(["data", "set", "profile.yaml", "--employer", "Synthetic Employer"]) == 0
    assert store.read_yaml(home / "profile.yaml")["employer"] == "Synthetic Employer"
