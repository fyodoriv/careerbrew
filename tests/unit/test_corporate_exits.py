"""Corporate data stays on the corporate machine: every off-machine exit refuses (goal 2026-10-20).

One test per exit. Already-guarded exits (git, logs, web server, notifications, restore)
have their own tests listed in docs/privacy.md.
"""

import pytest

from careerbrew import backup, machine, models, privacy, sync
from careerbrew.work import evidence


@pytest.fixture
def corporate(home, monkeypatch, tmp_path):
    fake_home = tmp_path / "userhome"
    (fake_home / "Library" / "CloudStorage" / "OneDrive-Personal").mkdir(parents=True)
    (fake_home / "Library" / "Mobile Documents" / "com~apple~CloudDocs" / "Documents").mkdir(parents=True)
    monkeypatch.setenv("HOME", str(fake_home))
    machine.set_role("corporate", "")
    return fake_home


def test_exit_sync_export_refuses_on_corporate(corporate, tmp_path):
    sync.pair_init()
    evidence.note("feat: shipped the billing export", ["delivery"])
    with pytest.raises(sync.SyncError, match="corporate"):
        sync.export(tmp_path / "bundle.cbs")
    assert not (tmp_path / "bundle.cbs").exists()


@pytest.mark.parametrize("sub", ["Library/CloudStorage/OneDrive-Personal/cb", "Library/Mobile Documents/com~apple~CloudDocs/cb", "Dropbox/cb", "Documents/cb"])
def test_exit_backup_to_a_cloud_synced_folder_refuses(corporate, monkeypatch, sub):
    monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(corporate / sub))
    with pytest.raises(RuntimeError, match="cloud-synced"):
        backup.create()


def test_exit_restore_into_a_cloud_synced_folder_refuses_and_writes_nothing(corporate, monkeypatch, tmp_path):
    monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(tmp_path / "local-backups"))
    made = backup.create()[0]
    dest = corporate / "Dropbox" / "restored"
    with pytest.raises(RuntimeError, match="cloud-synced"):
        backup.restore(made, dest)
    assert not dest.exists()
    backup.restore(made, tmp_path / "restored")  # a local folder is still fine
    assert (tmp_path / "restored").is_dir()


def test_exit_data_root_in_a_cloud_synced_folder_fails_readiness(corporate, monkeypatch):
    monkeypatch.setenv("CAREERBREW_HOME", str(corporate / "Documents" / "careerbrew"))
    machine.set_role("corporate", "")
    row = next(r for r in machine.readiness() if "data folder" in r["check"])
    assert not row["ok"] and "iCloud" in row["check"]


def test_exit_hosted_model_marked_private_is_not_used_on_corporate(corporate, monkeypatch):
    monkeypatch.setattr(
        models,
        "routes",
        lambda: [
            models.Route(name="paid", provider="anthropic", base_url="https://api.example.com/v1", model="m", api_key_env="", private_data=True),
            models.Route(name="local", provider="ollama", base_url="http://localhost:11434/v1", model="m", api_key_env="", private_data=True),
        ],
    )
    assert [r.name for r in models.allowed_routes(privacy.BUSINESS)] == ["local"]


def test_personal_machine_keeps_its_flows(home, monkeypatch, tmp_path):
    monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(tmp_path / "Dropbox" / "cb"))
    assert backup.create(), "personal backups may go anywhere the user chooses"


def test_personal_machine_refuses_a_corporate_bundle(home, tmp_path):
    import json

    from cryptography.fernet import Fernet

    sync.pair_init()
    f = Fernet(sync.key_path().read_text().strip().encode())
    (tmp_path / "b.cbs").write_bytes(f.encrypt(json.dumps({"from": "corporate", "at": "x", "payload": {"evidence": [{"id": "e1", "title": "secret"}]}}).encode()))
    with pytest.raises(sync.SyncError, match="corporate bundle"):
        sync.import_bundle(tmp_path / "b.cbs")
    assert evidence.load() == []


def test_personal_to_corporate_moves_settings_only_and_a_wrong_key_fails(tmp_path, monkeypatch):
    monkeypatch.setenv("CAREERBREW_PAIR_KEY_FILE", str(tmp_path / "pair.key"))
    pers, corp = tmp_path / "pers", tmp_path / "corp"
    monkeypatch.setenv("CAREERBREW_HOME", str(pers))
    sync.pair_init()
    pers.mkdir(parents=True)
    (pers / "settings.yaml").write_text("ladder: generic-swe\ntarget_level: staff\n")
    (pers / "jobs").mkdir()
    (pers / "jobs" / "ratings.jsonl").write_text('{"id": "x", "rating": 5}\n')
    sync.export(tmp_path / "b.cbs")
    monkeypatch.setenv("CAREERBREW_HOME", str(corp))
    machine.set_role("corporate", "")
    assert sync.import_bundle(tmp_path / "b.cbs")["added"] == 2, "ladder and target_level"
    assert not (corp / "jobs").exists(), "job-hunt data never reaches a corporate machine"
    sync.pair_init()  # a new key: the old bundle must not decrypt
    with pytest.raises(sync.SyncError, match="does not decrypt"):
        sync.import_bundle(tmp_path / "b.cbs")


def test_checkup_fails_on_a_cloud_synced_backup_target(corporate, monkeypatch):
    from careerbrew import checkup

    monkeypatch.setenv("CAREERBREW_BACKUP_DIRS", str(corporate / "Dropbox" / "cb"))
    row = checkup.cloud_step()
    assert not row["ok"] and "Dropbox" in row["step"]


def test_corporate_plugin_new_writes_none_and_pair_init_says_nothing_leaves(corporate, tmp_path, capsys):
    import yaml

    from careerbrew.cli import main

    assert main(["plugin", "new", "Globex", "--dir", str(tmp_path / "g"), "--no-prompt"]) == 0
    assert yaml.safe_load((tmp_path / "g" / "careerbrew-plugin.yaml").read_text())["policy"]["export_to_personal"] == "none"
    capsys.readouterr()
    assert main(["pair", "init"]) == 0
    assert "export level: none" in capsys.readouterr().out
