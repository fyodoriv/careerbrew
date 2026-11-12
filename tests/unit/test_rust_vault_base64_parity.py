"""Bounded synthetic comparison against Python's vault token decoder."""

import base64
import hashlib
import hmac
import json
import subprocess
from pathlib import Path

import cryptography
import pytest
from cryptography.fernet import Fernet

from careerbrew import backup, vault
from tests.unit.test_rust_vault_interop import candidate as candidate
from tests.unit.test_rust_vault_interop import invoke, output, same
from tests.unit.test_rust_vault_interop import python_vault as python_vault

ROOT = Path(__file__).resolve().parents[2]
KEY = base64.urlsafe_b64encode(bytes(range(32)))
SOURCE_SHA = "9d23e54fa94472ffd60950649f880d80a37132d7ea875b7e053ea647cc44096c"


def fail(case):
    pytest.fail(case, pytrace=False)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        if key in result:
            raise ValueError("duplicate key")
        result[key] = value
    return result


def responses(data, expected_ids):
    try:
        body = json.loads(data, object_pairs_hook=unique_object)
        rows = body["responses"]
        if not isinstance(body, dict) or set(body) != {"responses"} or not isinstance(rows, list):
            raise ValueError
        result = {}
        for row in rows:
            if not isinstance(row, dict) or type(row.get("id")) is not int:
                raise ValueError
            ident = row["id"]
            if ident in result or ident not in expected_ids:
                raise ValueError
            if row.get("status") == "refused" and set(row) == {"id", "status"}:
                result[ident] = None
            elif row.get("status") == "ok" and set(row) == {"id", "status", "data"}:
                result[ident] = base64.b64decode(row["data"], validate=True)
            else:
                raise ValueError
        if set(result) != expected_ids:
            raise ValueError
        return result
    except (KeyError, TypeError, ValueError):
        fail("invalid batch response")


def test_batch_response_validation():
    bad = [
        b"{}",
        b'{"responses":[]}',
        b'{"responses":[{"id":0,"status":"refused"},{"id":0,"status":"refused"}]}',
        b'{"responses":[{"id":1,"status":"refused"}]}',
        b'{"responses":[{"id":0,"status":"ok","data":"!"}]}',
        b'{"responses":',
        b'{"responses":[],"responses":[{"id":0,"status":"refused"}]}',
    ]
    for item in bad:
        with pytest.raises(pytest.fail.Exception):
            responses(item, {0})


def test_trailing_data_single_request(candidate, python_vault):
    token = vault.seal_bytes(b"") + b"A"
    expected = vault.open_bytes(token)
    actual = output(invoke(candidate, "open", token), "trailing data")
    same(actual, expected, "trailing data")


def test_batch_rejects_malformed_requests(candidate):
    valid = {"id": 0, "op": "open", "key": None, "data": ""}
    duplicate_keys = [
        b'{"requests":[{"id":0,"id":1,"op":"open","key":null,"data":""}]}',
        b'{"requests":[{"id":0,"op":"open","op":"seal","key":null,"data":""}]}',
        b'{"requests":[{"id":0,"op":"open","key":null,"key":null,"data":""}]}',
        b'{"requests":[{"id":0,"op":"open","key":null,"data":"","data":""}]}',
        b'{"requests":[],"requests":[]}',
    ]
    bad = duplicate_keys + [
        json.dumps({"requests": []}).encode(),
        json.dumps({"requests": [valid] * 33}).encode(),
        json.dumps({"requests": [valid, valid]}).encode(),
        json.dumps({"requests": [{**valid, "extra": 1}]}).encode(),
        json.dumps({"requests": [{**valid, "id": "0"}]}).encode(),
        json.dumps({"requests": [{"id": 0, "op": "open"}]}).encode(),
        json.dumps({"requests": [{"id": 0, "op": "open", "data": ""}]}).encode(),
        json.dumps({"requests": [valid], "extra": 1}).encode(),
        b" " * 32769,
    ]
    for index, wire in enumerate(bad):
        result = subprocess.run([candidate], input=wire, capture_output=True, cwd=ROOT, timeout=10)
        if result.returncode == 0 or result.stdout or result.stderr:
            fail(f"malformed request {index} accepted or leaked")


def test_batch_rejects_oversized_output(candidate):
    items = [{"id": i, "op": "seal", "key": KEY.decode(), "data": base64.b64encode(b"x" * 600).decode()} for i in range(32)]
    wire = json.dumps({"requests": items}).encode()
    if len(wire) > 32768:
        fail("output control input bound")
    result = subprocess.run([candidate], input=wire, capture_output=True, cwd=ROOT, timeout=10)
    if result.returncode == 0 or result.stdout or result.stderr:
        fail("oversized output accepted or leaked")


def test_registered_decoder_matrix(candidate, home, tmp_path, monkeypatch):
    source = ROOT / "src/careerbrew/vault.py"
    if hashlib.sha256(source.read_bytes()).hexdigest() != SOURCE_SHA or Path(vault.__file__).resolve() != source.resolve() or cryptography.__version__ != "50.0.2":
        fail("oracle identity")
    monkeypatch.setattr(vault, "_key", lambda: Fernet(KEY))
    monkeypatch.setattr(backup, "key_path", lambda: tmp_path / "synthetic-key")
    binary = candidate
    bases = [vault.seal_bytes(data)[len(vault.MAGIC) :] for data in (b"", b"x" * 33)]
    cases = [base[:offset] + bytes([byte]) + base[offset:] for base in bases for offset in (0, 8) for byte in range(256)]
    first = bases[0]
    cases.extend([first, first + b"=", first + b"===", b"=" + first, first[:-1] + b"!" + first[-1:], first + b"A", first.rstrip(b"="), first[:8] + b"=" + first[8:]])
    changed = bytearray(base64.urlsafe_b64decode(first))
    changed[-1] ^= 1
    tampered = base64.urlsafe_b64encode(changed)
    cases.extend([tampered, tampered + b"=", tampered[:8] + b"!" + tampered[8:], b"=" + tampered])
    if len(cases) != 1036:
        fail("case count")
    expected = []
    for index, token in enumerate(cases):
        try:
            expected.append(vault.open_bytes(vault.MAGIC + token))
        except vault.VaultError:
            expected.append(None)
        except Exception:
            fail(f"unexpected oracle error {index}")
    if any(value is not None for value in expected[-4:]):
        fail("tampered oracle control")
    for start in range(0, len(cases), 32):
        requests = [{"id": i, "op": "open", "key": KEY.decode(), "data": base64.b64encode(vault.MAGIC + cases[i]).decode()} for i in range(start, min(start + 32, len(cases)))]
        wire = json.dumps({"requests": requests}).encode()
        if len(wire) > 32768:
            fail("batch input bound")
        try:
            result = subprocess.run([binary], input=wire, capture_output=True, cwd=ROOT, timeout=10)
        except (OSError, subprocess.TimeoutExpired):
            fail("batch transport")
        if result.returncode != 0 or len(result.stdout) > 32768:
            fail("batch execution")
        got = responses(result.stdout, {row["id"] for row in requests})
        for index, actual in got.items():
            want = expected[index]
            if (want is None) != (actual is None) or (want is not None and not hmac.compare_digest(want, actual)):
                fail(f"decoder mismatch case {index}")
