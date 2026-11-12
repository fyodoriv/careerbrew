import json
import os

import pytest

from careerbrew import backup, notion_import, store, vault
from careerbrew.cli import main

from .conftest import CANARIES, FIXTURES


def _raw(path):
    return path.read_bytes()


def test_data_files_are_ciphertext_on_disk_and_read_back(home):
    store.write_yaml(home / "profile.yaml", {"name": CANARIES[0]})
    store.append_jsonl(home / "work" / "evidence.jsonl", [{"title": CANARIES[2]}])
    store.append_jsonl(home / "work" / "evidence.jsonl", [{"title": "second"}])
    for f in (home / "profile.yaml", home / "work" / "evidence.jsonl"):
        assert vault.is_sealed(_raw(f)) and CANARIES[0].encode() not in _raw(f) and CANARIES[2].encode() not in _raw(f)
    assert store.read_yaml(home / "profile.yaml")["name"] == CANARIES[0]
    assert [r["title"] for r in store.read_jsonl(home / "work" / "evidence.jsonl")] == [CANARIES[2], "second"], "appends keep every row"


def test_plaintext_files_still_read_and_encrypt_seals_them(home, capsys):
    home.mkdir(parents=True, exist_ok=True)
    (home / "settings.yaml").write_text("ladder: generic-swe\n")
    assert store.read_yaml(home / "settings.yaml") == {"ladder": "generic-swe"}, "hand-written plaintext still works"
    assert main(["data", "status"]) == 0 and "still plaintext" in capsys.readouterr().out
    assert main(["data", "encrypt"]) == 0
    assert vault.is_sealed(_raw(home / "settings.yaml"))
    assert vault.status()["plaintext"] == 0


def test_logs_and_reports_stay_plaintext_by_design(home):
    store.write_text(home / "out" / "promo.md", "report")
    store.write_text(home / "logs" / "x.jsonl", "{}\n")
    assert _raw(home / "out" / "promo.md") == b"report" and _raw(home / "logs" / "x.jsonl") == b"{}\n"


def test_decrypt_turns_encryption_off_and_encrypt_turns_it_back_on(home, capsys):
    store.write_json(home / "jobs" / "a.json", {"k": 1})
    assert main(["data", "decrypt"]) == 1, "decrypt needs --yes"
    assert main(["data", "decrypt", "--yes"]) == 0
    assert json.loads(_raw(home / "jobs" / "a.json")) == {"k": 1} and not vault.enabled()
    store.write_json(home / "jobs" / "b.json", {"k": 2})
    assert not vault.is_sealed(_raw(home / "jobs" / "b.json")), "with encryption off, writes stay plaintext"
    assert main(["data", "encrypt"]) == 0 and vault.enabled()
    assert vault.is_sealed(_raw(home / "jobs" / "a.json")) and vault.is_sealed(_raw(home / "jobs" / "b.json"))


def test_a_wrong_key_is_a_clear_error_not_garbage(home, tmp_path, monkeypatch):
    store.write_json(home / "jobs" / "a.json", {"k": 1})
    monkeypatch.setenv("CAREERBREW_KEY_FILE", str(tmp_path / "other.key"))
    with pytest.raises(vault.VaultError, match="key"):
        store.read_json(home / "jobs" / "a.json")


def test_notion_bundle_and_library_are_sealed_after_import(home):
    import shutil

    bundle = home / "imports" / "notion"
    shutil.copytree(FIXTURES / "notion-bundle", bundle)
    notion_import.run(bundle)
    assert all(vault.is_sealed(_raw(f)) for f in bundle.glob("*.json")), "the fetched bundle is encrypted after import"
    lib = list((home / "library" / "notion").glob("*.md"))
    assert lib and all(vault.is_sealed(_raw(f)) for f in lib)
    assert "sample step" in notion_import.page("st1")["markdown"]
    notion_import.check(bundle)  # the sealed bundle still loads for the N/N check


def test_the_key_lives_outside_the_data_root_with_owner_only_access(home):
    store.write_json(home / "jobs" / "a.json", {})
    key = backup.key_path()
    assert home not in key.parents and oct(os.stat(key).st_mode)[-3:] == "600"


def test_a_lost_key_is_never_silently_replaced(home):
    store.write_json(home / "jobs" / "a.json", {"k": 1})
    backup.key_path().unlink()
    vault._fernet.cache_clear()
    with pytest.raises(vault.VaultError, match="missing"):
        store.write_json(home / "jobs" / "b.json", {"k": 2})
    assert not backup.key_path().exists(), "no new key was made"
