"""rust-05i bulk vault/status parity and dynamic native host denial."""

from __future__ import annotations

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
ORACLE = ROOT / "tests" / "oracles" / "rust05.py"
CANARY = b"SYNTHETIC-RUST05-CANARY-7Q9"
SECRET = b"SYNTHETIC-RUST05-SECRET-NOT-AN-AUTHENTICATION-VALUE"
BULK_CASES = {
    "bulk-enumeration-status",
    "bulk-status-environment-off",
    "bulk-marker-disables-seal-files",
    "bulk-seal-files",
    "bulk-encrypt-all",
    "bulk-encrypt-all-noop",
    "bulk-encrypt-refuses-backup-suffix",
    "bulk-encrypt-refuses-sync-suffix",
    "bulk-encrypt-refuses-raw-fernet",
    "bulk-encrypt-sealed-missing-key",
    "bulk-encrypt-invalid-key",
    "bulk-decrypt-all",
    "bulk-decrypt-all-noop",
    "bulk-decrypt-missing-key",
    "bulk-decrypt-invalid-key",
    "bulk-decrypt-wrong-key",
    "bulk-seal-files-failure-atomicity",
    "bulk-encrypt-all-failure-atomicity",
    "bulk-decrypt-all-failure-atomicity",
}


@pytest.fixture(scope="module")
def store_bin():
    if shutil.which("cargo") is None:
        pytest.skip("cargo is not installed: the cross-engine checks need the Rust toolchain")
    subprocess.run(
        ["cargo", "build", "--locked", "-p", "careerbrew-vault", "--bin", "vault-store"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target" / "debug" / "vault-store"


def synthetic_env(root: Path) -> dict[str, str]:
    home = root / "data"
    home.mkdir(parents=True)
    return {
        "HOME": str(root / "home"),
        "CAREERBREW_HOME": str(home),
        "CAREERBREW_KEY_FILE": str(root / "keys" / "backup.key"),
        "CAREERBREW_EVENTS": "on",
        "CAREERBREW_ENCRYPT": "on",
        "CAREERBREW_NOW": "2026-03-01T09:30:00+00:00",
        "TZ": "UTC",
        "USER": "synthetic",
        "LOGNAME": "synthetic",
        "PATH": os.environ.get("PATH", ""),
    }


def case(report: dict, case_id: str) -> dict:
    return next(row for row in report["cases"] if row["id"] == case_id)


def state(case_row: dict, relative: str) -> dict:
    return next(row for row in case_row["state"] if row["path"] == relative)


def test_bulk_vault_status_and_native_host_denial_match_real_python(store_bin, tmp_path):
    answers = []
    for engine in ("python", "rust"):
        root = tmp_path / engine
        command = [sys.executable, "-I", "-B", str(ORACLE), "--all"] if engine == "python" else [str(store_bin), "--all"]
        done = subprocess.run(command, cwd=ROOT, env=synthetic_env(root), capture_output=True, timeout=120)
        assert done.returncode == 0, done.stderr.decode(errors="replace")
        assert CANARY not in done.stdout + done.stderr
        assert SECRET not in done.stdout + done.stderr
        report = json.loads(done.stdout)
        ids = [row["id"] for row in report["cases"]]
        assert len(ids) == len(set(ids))
        assert BULK_CASES | {"data-show-host-denial"} <= set(ids)

        assert case(report, "data-show-host-denial")["result"] == {
            "read_only_attempts": 0,
            "process": {"attempts": 1, "refused": True},
            "socket": {"attempts": 1, "refused": True},
            "secret": {"attempts": 1, "refused": True},
        }
        enumeration = case(report, "bulk-enumeration-status")["result"]
        assert enumeration == {
            "paths": ["learn/synthetic/replay/07/near.txt", "work/plain.txt", "work/sealed.txt"],
            "status": {"enabled": True, "files": {"int": "3"}, "sealed": {"int": "1"}, "plaintext": {"int": "2"}},
        }
        assert case(report, "bulk-status-environment-off")["result"] == {
            "enabled": False,
            "files": {"int": "3"},
            "sealed": {"int": "1"},
            "plaintext": {"int": "2"},
        }
        marker = case(report, "bulk-marker-disables-seal-files")
        assert marker["result"] == {
            "count": {"int": "0"},
            "unchanged": True,
            "status": {"enabled": False, "files": {"int": "3"}, "sealed": {"int": "1"}, "plaintext": {"int": "2"}},
        }
        assert case(report, "bulk-seal-files")["result"] == {"count": {"int": "1"}, "sealed_noop": True}
        assert state(case(report, "bulk-seal-files"), "work/first.txt")["mode"] == "600"
        assert state(case(report, "bulk-seal-files"), "work/already.txt")["mode"] == "640"
        assert case(report, "bulk-encrypt-all")["result"] == {"count": {"int": "2"}, "sealed_noop": True}
        assert case(report, "bulk-encrypt-all-noop")["result"] == {"count": {"int": "0"}, "unchanged": True}
        assert all(row["key"] == {"present": True, "mode": "600"} for row in report["cases"] if row["id"] in {"bulk-encrypt-all", "bulk-decrypt-all"})

        for kind in ("backup-suffix", "sync-suffix", "raw-fernet"):
            refused = case(report, f"bulk-encrypt-refuses-{kind}")
            assert refused["result"] == {"error": "VaultError"}
            assert refused["mutation_free"] is True
            assert state(refused, ".plaintext")["sealed"] is False
            assert state(refused, "record.txt")["sealed"] is False

        missing_sealed = case(report, "bulk-encrypt-sealed-missing-key")
        assert missing_sealed["result"] == {"error": "VaultError"}
        assert missing_sealed["key"] == {"present": False, "mode": None}
        assert state(missing_sealed, "old.txt")["read_error"] == "FileNotFoundError"
        assert state(missing_sealed, "new.txt")["sealed"] is False
        invalid_encrypt = case(report, "bulk-encrypt-invalid-key")
        assert invalid_encrypt["result"] == {"error": "ValueError"}
        assert state(invalid_encrypt, "record.txt")["sealed"] is False

        decrypted = case(report, "bulk-decrypt-all")
        assert decrypted["result"] == {"int": "3"}
        assert all(row["sealed"] is False for row in decrypted["state"])
        assert state(decrypted, ".plaintext")["sealed"] is False
        assert case(report, "bulk-decrypt-all-noop")["result"] == {"count": {"int": "0"}, "data_unchanged": True}
        for kind, error in (("missing-key", "VaultError"), ("invalid-key", "ValueError"), ("wrong-key", "VaultError")):
            refused = case(report, f"bulk-decrypt-{kind}")
            assert refused["result"] == {"error": error}
            assert refused["mutation_free"] is True
            assert state(refused, "record.txt")["sealed"] is True
            assert ".plaintext" not in {row["path"] for row in refused["state"]}

        for operation in ("seal-files", "encrypt-all", "decrypt-all"):
            failed = case(report, f"bulk-{operation}-failure-atomicity")
            assert failed["result"] == {"error": "OSError"}
            assert not any(row["path"].endswith(".tmp") for row in failed["state"])
            assert state(failed, "work/a.txt")["mode"] == "600"
            assert state(failed, "work/a.txt")["sealed"] is (operation != "decrypt-all")
            assert state(failed, "work/b.txt")["sealed"] is (operation == "decrypt-all")
            assert state(failed, "work/c.txt")["sealed"] is (operation == "decrypt-all")
            assert (".plaintext" in {row["path"] for row in failed["state"]}) is False

        answers.append(done.stdout)
    assert answers[0] == answers[1]
