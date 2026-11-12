"""Encryption warnings and replay exclusions use synthetic data only."""

import json

import pytest

from careerbrew import checkup, report, store, vault
from careerbrew.cli import main
from careerbrew.work import projects


@pytest.fixture(autouse=True)
def encryption_on(monkeypatch):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "on")


def test_new_project_research_log_inside_data_root_is_sealed(home):
    folder = home / "work" / "synthetic-notes"
    folder.mkdir(parents=True)
    path = projects.memory_path("synthetic", str(folder) + "/")
    log = folder / "synthetic-research-log.md"
    assert path == str(log)
    assert vault.is_sealed(log.read_bytes())
    assert store.read_text(log).startswith("# synthetic research log")
    assert vault.status()["plaintext"] == 0


def test_existing_and_external_research_logs_are_preserved(home, tmp_path):
    inside = home / "work" / "notes"
    inside.mkdir(parents=True)
    old = inside / "synthetic-research-log.md"
    store.write_text(old, "existing synthetic note")
    before = old.read_bytes()
    projects.memory_path("synthetic", str(inside) + "/")
    assert old.read_bytes() == before
    outside = tmp_path / "external"
    outside.mkdir()
    projects.memory_path("synthetic", str(outside) + "/")
    assert (outside / "synthetic-research-log.md").read_text().startswith("# synthetic research log")


def test_research_log_stays_plain_when_encryption_is_off(home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "off")
    folder = home / "work" / "notes"
    folder.mkdir(parents=True)
    projects.memory_path("synthetic", str(folder) + "/")
    assert (folder / "synthetic-research-log.md").read_text().startswith("# synthetic research log")


def test_replay_checkout_is_not_counted_or_rewritten(home):
    checkout = home / "learn" / "synthetic" / "replay" / "7"
    (checkout / ".git").mkdir(parents=True)
    code = checkout / "example.py"
    config = checkout / ".git" / "config"
    code.write_text("value = 1\n")
    config.write_text("[core]\n")
    (checkout / ".git" / "HEAD").write_text("ref: refs/heads/main\n")
    before = {p: p.read_bytes() for p in (code, config, checkout / ".git" / "HEAD")}
    metadata = checkout.parent / "7.json"
    store.write_json(metadata, {"state": "synthetic"})
    assert vault.status()["plaintext"] == 0
    assert vault.encrypt_all() == 0
    assert {p: p.read_bytes() for p in before} == before
    assert vault.is_sealed(metadata.read_bytes())
    vault.decrypt_all()
    assert {p: p.read_bytes() for p in before} == before
    assert json.loads(metadata.read_text()) == {"state": "synthetic"}


def test_explicit_decrypt_recovers_a_previously_sealed_replay(home):
    checkout = home / "learn" / "synthetic" / "replay" / "7"
    (checkout / ".git").mkdir(parents=True)
    files = {checkout / "example.py": b"value = 1\n", checkout / ".git" / "HEAD": b"ref: refs/heads/main\n"}
    for path, raw in files.items():
        path.write_bytes(vault.seal_bytes(raw))
    vault.decrypt_all()
    assert {path: path.read_bytes() for path in files} == files


def test_missing_key_for_sealed_replay_is_not_replaced(home):
    from careerbrew import backup

    checkout = home / "learn" / "synthetic" / "replay" / "7"
    (checkout / ".git").mkdir(parents=True)
    head = checkout / ".git" / "HEAD"
    head.write_bytes(vault.seal_bytes(b"ref: refs/heads/main\n"))
    key = backup.key_path()
    key.unlink()
    vault._fernet.cache_clear()
    with pytest.raises(vault.VaultError, match="missing"):
        vault.decrypt_all()
    assert not key.exists()
    assert vault.is_sealed(head.read_bytes())


@pytest.mark.parametrize("parts", [("learn", "synthetic", "replay", "not-a-pr"), ("learn", "synthetic", "replay", "7"), ("learn", "synthetic", "other", "7"), ("work", "synthetic", "replay", "7")])
def test_near_miss_paths_remain_sealable(home, parts):
    path = home.joinpath(*parts, "record.txt")
    store.write_text(path, "synthetic record")
    assert vault.sealable(path)
    assert vault.is_sealed(path.read_bytes())


@pytest.mark.parametrize("state", ["plaintext", "sealed", "off"])
def test_actual_checkup_reports_only_enabled_plaintext(home, monkeypatch, capsys, state):
    secret_name = "SYNTHETIC-PRIVATE-NAME"
    path = home / "work" / secret_name / "record.txt"
    path.parent.mkdir(parents=True)
    if state == "sealed":
        store.write_text(path, "synthetic value")
    else:
        path.write_text("synthetic value")
    if state == "off":
        monkeypatch.setenv("CAREERBREW_ENCRYPT", "off")
    monkeypatch.setattr(checkup, "STEPS", [checkup.encryption_step])
    monkeypatch.setattr(checkup, "plugin_steps", lambda: [])
    rc = main(["checkup", "--offline"])
    summary = report.latest("checkup")
    output = capsys.readouterr().out + json.dumps(summary)
    assert secret_name not in output
    assert rc == (1 if state == "plaintext" else 0)
    assert any(item.get("command") == "careerbrew data encrypt" for item in summary["blockers"]) == (state == "plaintext")
