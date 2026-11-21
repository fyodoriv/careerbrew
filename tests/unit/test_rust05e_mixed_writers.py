"""rust-05e: Python and Rust writers share one advisory lock, synthetic data only.

Four Python processes run `store.append_jsonl` and four loops run the Rust
`vault-store append` against ONE JSONL file. Both take `flock` on the sibling
`.{name}.lock`, so no append is lost: the file opens and holds every id once.
"""

import json
import os
import shutil
import subprocess
import sys
from pathlib import Path

import pytest

from careerbrew import keyfile, store, vault

ROOT = Path(__file__).resolve().parents[2]
PER_WRITER = 125
WRITERS = 4

PY_WORKER = """
import sys
from pathlib import Path
from careerbrew import store
path, tag, count = Path(sys.argv[1]), sys.argv[2], int(sys.argv[3])
for i in range(count):
    store.append_jsonl(path, [{"id": f"{tag}-{i}", "n": i}])
"""


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
    return ROOT / "target/debug/vault-store"


@pytest.fixture(autouse=True)
def data_root(home):
    home.mkdir(parents=True, exist_ok=True)
    return home


def rust_loop(store_bin, key_file, home, data, mode, tag, count):
    # One shell loop per Rust writer: each append is its own process, like a CLI call.
    script = 'i=0; while [ "$i" -lt "$6" ]; do printf \'{"id": "%s-%s", "n": %s}\\n\' "$5" "$i" "$i" | "$1" append "$2" "$3" "$4" "$7" || exit 1; i=$((i+1)); done'
    return subprocess.Popen(["sh", "-c", script, "sh", store_bin, key_file, home, data, tag, str(count), mode], stderr=subprocess.PIPE)


def py_worker(data, tag, count):
    return subprocess.Popen([sys.executable, "-c", PY_WORKER, str(data), tag, str(count)], env=os.environ.copy(), stderr=subprocess.PIPE)


def run_mixed(store_bin, key_file, home, data, mode, count):
    procs = [py_worker(data, f"py{w}", count) for w in range(WRITERS)]
    procs += [rust_loop(store_bin, key_file, home, data, mode, f"rs{w}", count) for w in range(WRITERS)]
    for proc in procs:
        _, err = proc.communicate(timeout=300)
        assert proc.returncode == 0, err.decode(errors="replace")[-2000:]


def check_ids(text, total):
    rows = [json.loads(line) for line in text.splitlines() if line.strip()]
    ids = [row["id"] for row in rows]
    assert len(ids) == total
    assert len(set(ids)) == total


def test_mixed_python_and_rust_appends_to_one_sealed_file_keep_every_row(store_bin, home):
    key_file = Path(os.environ["CAREERBREW_KEY_FILE"])
    keyfile.load_key(create=True, path=key_file)
    data = home / "work" / "mixed.jsonl"
    data.parent.mkdir(parents=True, exist_ok=True)
    assert vault.enabled() and vault.sealable(data)
    run_mixed(store_bin, key_file, home, data, "seal", PER_WRITER)
    assert data.read_bytes().startswith(vault.MAGIC)
    check_ids(store.read_text(data), 2 * WRITERS * PER_WRITER)


def test_mixed_plain_appends_keep_every_row(store_bin, home, monkeypatch):
    monkeypatch.setenv("CAREERBREW_ENCRYPT", "off")
    key_file = Path(os.environ["CAREERBREW_KEY_FILE"])
    data = home / "work" / "plain.jsonl"
    data.parent.mkdir(parents=True, exist_ok=True)
    run_mixed(store_bin, key_file, home, data, "plain", 25)
    assert not data.read_bytes().startswith(vault.MAGIC)
    check_ids(data.read_text(), 2 * WRITERS * 25)
