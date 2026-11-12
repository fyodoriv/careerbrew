"""Synthetic cross-language checks for the disconnected Rust vault codec."""

import base64
import hashlib
import hmac
import json
import subprocess
import sys
from pathlib import Path

import cryptography
import pytest
from cryptography.fernet import Fernet

from careerbrew import backup, vault

ROOT = Path(__file__).resolve().parents[2]
SOURCE_SHA = "9d23e54fa94472ffd60950649f880d80a37132d7ea875b7e053ea647cc44096c"
KEY = base64.urlsafe_b64encode(bytes(range(32)))
WRONG_KEY = base64.urlsafe_b64encode(bytes(range(1, 33)))
PAYLOADS = [b"", b"CBV1", b"CBV1X", "é😀".encode(), bytes(range(256)), *[b"x" * n for n in (15, 16, 17, 31, 32, 33)], b"z" * 4096]


@pytest.fixture(scope="module")
def candidate():
    source = ROOT / "src/careerbrew/vault.py"
    assert hashlib.sha256(source.read_bytes()).hexdigest() == SOURCE_SHA
    assert Path(vault.__file__).resolve() == source.resolve()
    assert cryptography.__version__ == "50.0.2"
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-vault"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target/debug/careerbrew-vault"


def invoke(candidate, op, data, key=KEY):
    request = json.dumps({"op": op, "data": base64.b64encode(data).decode(), "key": key.decode() if key is not None else None}).encode()
    if len(request) > 32768:
        pytest.fail("synthetic request bound", pytrace=False)
    try:
        result = subprocess.run([candidate], input=request, capture_output=True, cwd=ROOT, timeout=10)
    except (OSError, subprocess.TimeoutExpired):
        pytest.fail("synthetic transport failed", pytrace=False)
    if len(result.stdout) > 32768:
        pytest.fail("synthetic response bound", pytrace=False)
    return result


def output(result, case):
    if result.returncode != 0:
        pytest.fail(case, pytrace=False)
    try:
        return base64.b64decode(json.loads(result.stdout)["data"], validate=True)
    except (KeyError, ValueError, TypeError):
        pytest.fail(case, pytrace=False)


def same(actual, expected, case):
    if not hmac.compare_digest(actual, expected):
        pytest.fail(case, pytrace=False)


def refused(result, case):
    if result.returncode == 0 or result.stdout:
        pytest.fail(case, pytrace=False)


@pytest.fixture
def python_vault(home, monkeypatch, tmp_path):
    monkeypatch.setattr(vault, "_key", lambda: Fernet(KEY))
    monkeypatch.setattr(backup, "key_path", lambda: tmp_path / "synthetic-key")
    return monkeypatch


def test_synthetic_round_trips(candidate, python_vault):
    for index, payload in enumerate(PAYLOADS):
        py_token = vault.seal_bytes(payload)
        rust_plain = output(invoke(candidate, "open", py_token), f"Python to Rust {index}")
        same(rust_plain, payload, f"Python to Rust {index}")
        rust_token = output(invoke(candidate, "seal", payload), f"Rust to Python {index}")
        same(vault.open_bytes(rust_token), payload, f"Rust to Python {index}")
        with python_vault.context() as patch:
            patch.setattr(vault, "_key", lambda: pytest.fail("plaintext requested a key"))
            same(vault.open_bytes(payload), payload, f"Python plaintext {index}")
        plain = output(invoke(candidate, "open", payload, None), f"Rust plaintext {index}")
        same(plain, payload, f"Rust plaintext {index}")
    first = output(invoke(candidate, "seal", b"x"), "first random token")
    second = output(invoke(candidate, "seal", b"x"), "second random token")
    if hmac.compare_digest(first, second):
        pytest.fail("token randomness", pytrace=False)
    same(vault.open_bytes(first), b"x", "first token")
    same(vault.open_bytes(second), b"x", "second token")


def test_invalid_and_noncanonical_tokens(candidate, python_vault):
    token = vault.seal_bytes(b"x" * 33)[len(vault.MAGIC) :]
    decoded = base64.urlsafe_b64decode(token)
    variants = [b"", b"!", token[:1], token[:5], token[:16]]
    for offset in (0, 1, 9, 25, len(decoded) - 1):
        changed = bytearray(decoded)
        changed[offset] ^= 1
        variants.append(base64.urlsafe_b64encode(changed))
    variants.extend([token, token[:-1]])
    empty_token = vault.seal_bytes(b"")[len(vault.MAGIC) :]
    variants.append(empty_token.rstrip(b"="))
    assert len(variants) == 13
    for index, variant in enumerate(variants):
        key = WRONG_KEY if index == 10 else KEY
        with python_vault.context() as patch:
            patch.setattr(vault, "_key", lambda: Fernet(key))
            with pytest.raises(vault.VaultError):
                vault.open_bytes(vault.MAGIC + variant)
        result = invoke(candidate, "open", vault.MAGIC + variant, key)
        refused(result, f"invalid token {index}")
    noisy = vault.MAGIC + token[:8] + b"!" + token[8:]
    expected = vault.open_bytes(noisy)
    actual = output(invoke(candidate, "open", noisy), "noncanonical token")
    same(actual, expected, "noncanonical token")


def test_extra_trailing_padding_matches_python(candidate, python_vault):
    token = vault.seal_bytes(b"") + b"="
    expected = vault.open_bytes(token)
    actual = output(invoke(candidate, "open", token), "extra padding")
    same(actual, expected, "extra padding")


def test_oversized_request_refuses_without_output(candidate):
    result = subprocess.run([candidate], input=b"x" * 32769, capture_output=True, cwd=ROOT, timeout=10)
    refused(result, "oversized request")


def test_failure_reporting_does_not_expose_canary(tmp_path):
    probe = tmp_path / "test_vault_failure_probe.py"
    source = f"""import runpy
import subprocess
import pytest

m = runpy.run_path({str(Path(__file__).resolve())!r})
marker = bytes([86, 65, 85, 76, 84, 45, 67, 65, 78, 65, 82, 89, 45, 55, 57, 51])

@pytest.mark.parametrize("case", range(5))
def test_failure(case, monkeypatch):
    if case in (0, 1):
        fake = subprocess.CompletedProcess([], 0, stdout=marker, stderr=b"")
        monkeypatch.setattr(subprocess, "run", lambda *args, **kwargs: fake)
        data = marker if case == 1 else b"data"
        key = marker if case == 0 else b"key"
        m["output"](m["invoke"]("unused", "open", data, key), "sanitized")
    elif case == 2:
        m["same"](marker, b"different", "sanitized")
    elif case == 3:
        m["output"](subprocess.CompletedProcess([], 0, stdout=marker, stderr=b""), "sanitized")
    else:
        m["refused"](subprocess.CompletedProcess([], 1, stdout=marker, stderr=b""), "sanitized")
"""
    probe.write_text(source)
    # A hang guard only: the nested run needs under 1 s alone (0.5 s warm, 0.8 s with no bytecode cache, in any
    # folder), but a new interpreter got no CPU for 10 s in a full unit run on a busy machine.
    result = subprocess.run([sys.executable, "-m", "pytest", "-q", str(probe)], capture_output=True, cwd=ROOT, timeout=120)
    marker = bytes([86, 65, 85, 76, 84, 45, 67, 65, 78, 65, 82, 89, 45, 55, 57, 51])
    if result.returncode != 1 or marker in result.stdout or marker in result.stderr or b"5 failed" not in result.stdout:
        pytest.fail("failure-report canary control", pytrace=False)
