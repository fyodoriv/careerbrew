"""Synthetic comparison of the disconnected Rust hash experiment."""

import ast
import hashlib
import json
import subprocess
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
SOURCE_SHA = "11d63c3f2e00c44bf64c9674ca54c98ab23e521c5eadd5953dd56d785bbfcf64"


@pytest.fixture(scope="module")
def candidate():
    subprocess.run(
        ["cargo", "build", "--locked", "--offline", "-p", "careerbrew-hash-experiment"],
        cwd=ROOT,
        check=True,
        capture_output=True,
    )
    return ROOT / "target/debug/careerbrew-hash-experiment"


def oracle():
    source = (ROOT / "src/careerbrew/changes.py").read_bytes()
    assert hashlib.sha256(source).hexdigest() == SOURCE_SHA
    module = ast.parse(source)
    function = next(node for node in module.body if isinstance(node, ast.FunctionDef) and node.name == "_h")
    function.args.args[0].annotation = None
    function.returns = None
    namespace = {"json": json, "hashlib": hashlib}
    exec(compile(ast.fix_missing_locations(ast.Module(body=[function], type_ignores=[])), "synthetic-oracle", "exec"), namespace)
    return namespace["_h"]


def run(binary, data):
    return subprocess.run([binary], input=data, capture_output=True, cwd=ROOT, timeout=5)


def compare(wire, encoded, digest):
    assert bytes.fromhex(wire["serialized_hex"]) == encoded
    assert wire["digest"] == digest


def test_frozen_positive_cases(candidate):
    values = [None, False, True, 0, -1, 9007199254740991, -9007199254740991, "", 'a"\\\n', "é😀"]
    wrappers = [lambda x: x, lambda x: [x], lambda x: {"z": x, "a": 1}, lambda x: {"é": x, "a": [None]}]
    expected_hash = oracle()
    print(f"synthetic oracle Python {sys.version_info.major}.{sys.version_info.minor}.{sys.version_info.micro}")
    for wrapper in wrappers:
        for value in values:
            obj = wrapper(value)
            encoded = json.dumps(obj, sort_keys=True).encode()
            result = run(candidate, json.dumps(obj, ensure_ascii=True).encode())
            assert result.returncode == 0, result.stderr
            wire = json.loads(result.stdout)
            compare(wire, encoded, expected_hash(obj))


@pytest.mark.parametrize("data", [b"1.5", b"NaN", b"Infinity", b"9007199254740992", rb'"\ud800"', b'{"a":1,"a":2}', b"[[[[[null]]]]]"])
def test_unsupported_inputs_are_rejected(candidate, data):
    result = run(candidate, data)
    assert result.returncode != 0
    assert not result.stdout


def test_wrong_spacing_is_detected():
    obj = {"z": None, "a": 1}
    wrong = json.dumps(obj, sort_keys=True, separators=(",", ":")).encode()
    with pytest.raises(AssertionError):
        compare({"serialized_hex": wrong.hex(), "digest": oracle()(obj)}, json.dumps(obj, sort_keys=True).encode(), oracle()(obj))


def test_output_limit_includes_newline(candidate):
    overhead = len(json.dumps({"digest": "0" * 16, "serialized_hex": ""}, separators=(",", ":"))) + 1
    max_chars = (131072 - overhead) // 2 - 2
    accepted = run(candidate, json.dumps("a" * max_chars).encode())
    rejected = run(candidate, json.dumps("a" * (max_chars + 1)).encode())
    assert accepted.returncode == 0
    assert len(accepted.stdout) <= 131072
    assert rejected.returncode != 0
    assert rejected.stdout == b""
