"""Repository privacy scans bind staged findings to index bytes."""

import errno
import os
import struct
import subprocess
import zlib
from pathlib import Path

import pytest

from careerbrew import cli, privacy

TERM = "SyntheticIndexCanary"
GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false"]


def git(repo, *args, env=None, input=None):
    return subprocess.run([*GIT, "-C", str(repo), *args], input=input, capture_output=True, check=True, env=env)


@pytest.fixture
def repo(tmp_path):
    path = tmp_path / "repo"
    path.mkdir()
    git(path, "init", "-q")
    (path / "document.txt").write_text("clean\n")
    git(path, "add", "--", "document.txt")
    git(path, "commit", "-qm", "synthetic base")
    return path


def test_staged_leak_cannot_be_hidden_by_clean_or_missing_worktree(repo):
    path = repo / "document.txt"
    path.write_text(TERM + "\n")
    git(repo, "add", "--", "document.txt")
    for working in ("clean\n", None):
        if working is None:
            path.unlink()
        else:
            path.write_text(working)
        for staged_only in (True, False):
            report = privacy.scan_repo(repo, staged_only=staged_only, terms=[TERM])
            assert not report.ok
            assert any(name == "document.txt" for name, _ in report.findings)
            assert any("staged" in how for spans in report.spans.values() for _, how, _ in spans)


def test_staged_only_does_not_substitute_working_content(repo):
    path = repo / "document.txt"
    path.write_text("new clean\n")
    git(repo, "add", "--", "document.txt")
    path.write_text(TERM + "\n")
    assert privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok
    report = privacy.scan_repo(repo, terms=[TERM])
    assert not report.ok
    assert any("working" in how for spans in report.spans.values() for _, how, _ in spans)


def test_unusual_staged_paths_symlinks_private_and_alternate_index(repo, tmp_path, monkeypatch):
    alternate = tmp_path / "alternate-index"
    monkeypatch.setenv("GIT_INDEX_FILE", str(alternate))
    env = os.environ.copy()
    git(repo, "read-tree", "HEAD", env=env)
    name = "-odd\nname.txt"
    (repo / name).write_text(TERM + "\n")
    (repo / "private").mkdir()
    (repo / "private" / "record.txt").write_text("clean\n")
    (repo / "link").symlink_to(TERM)
    git(repo, "add", "--", name, "private/record.txt", "link", env=env)
    (repo / name).write_text("clean\n")
    report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
    assert not report.ok
    assert {path for path, _ in report.findings} >= {name, "link", "private/record.txt"}
    assert any(reason == "path-under-private" for _, reason in report.findings)


def test_git_read_failure_and_changed_index_fail_closed(repo, monkeypatch):
    path = repo / "document.txt"
    path.write_text("staged clean\n")
    git(repo, "add", "--", "document.txt")
    original = subprocess.run

    def fail_object(argv, *args, **kwargs):
        if "cat-file" in argv:
            return subprocess.CompletedProcess(argv, 1, b"", b"SYNTHETIC-SECRET")
        return original(argv, *args, **kwargs)

    monkeypatch.setattr(subprocess, "run", fail_object)
    report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
    assert not report.ok
    assert "SYNTHETIC-SECRET" not in str(report.findings)
    monkeypatch.setattr(subprocess, "run", original)
    changed = False

    def change_index(argv, *args, **kwargs):
        nonlocal changed
        result = original(argv, *args, **kwargs)
        if not changed and "cat-file" in argv:
            changed = True
            (repo / "other.txt").write_text("another staged entry\n")
            git(repo, "add", "--", "other.txt")
        return result

    monkeypatch.setattr(subprocess, "run", change_index)
    assert not privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok


def test_staged_gitlink_cannot_be_hidden_by_git_configuration(repo):
    commit = git(repo, "rev-parse", "HEAD").stdout.decode().strip()
    git(repo, "config", "diff.ignoreSubmodules", "all")
    git(repo, "update-index", "--add", "--cacheinfo", f"160000,{commit},submodule")
    assert not privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok
    assert not privacy.scan_repo(repo, terms=[TERM]).ok


def test_undecodable_staged_bytes_fail_closed(repo):
    (repo / "document.txt").write_bytes(TERM.encode("utf-16"))
    git(repo, "add", "--", "document.txt")
    for staged_only in (True, False):
        report = privacy.scan_repo(repo, staged_only=staged_only, terms=[TERM])
        assert not report.ok
        assert TERM not in str(report.findings)


def test_index_changed_and_restored_during_scan_fails_closed(repo, monkeypatch):
    (repo / "document.txt").write_text("staged clean\n")
    git(repo, "add", "--", "document.txt")
    original = subprocess.run
    changed = False

    def change_and_restore(argv, *args, **kwargs):
        nonlocal changed
        result = original(argv, *args, **kwargs)
        if not changed and "cat-file" in argv:
            changed = True
            (repo / "other.txt").write_text("synthetic\n")
            git(repo, "add", "--", "other.txt")
            git(repo, "update-index", "--force-remove", "--", "other.txt")
        return result

    monkeypatch.setattr(subprocess, "run", change_and_restore)
    assert not privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok


def png_chunk(kind, payload):
    body = kind + payload
    return struct.pack(">I", len(payload)) + body + struct.pack(">I", zlib.crc32(body))


def synthetic_png(width=1, height=1, pixels=None, extra=b""):
    header = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    raw = pixels if pixels is not None else b"\0\0\0\0"
    return b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", header) + extra + png_chunk(b"IDAT", zlib.compress(raw)) + png_chunk(b"IEND", b"")


def test_supported_staged_visual_png_is_admitted_with_pixel_limit_notice(repo, home, capsys):
    folder = repo / "web/visual/__screenshots__/fixture"
    folder.mkdir(parents=True)
    (folder / "one.png").write_bytes(synthetic_png())
    root = Path(__file__).resolve().parents[2]
    source = next((root / "web/visual/__screenshots__").rglob("*.png"))
    (folder / "real.png").write_bytes(source.read_bytes())
    git(repo, "add", "--", "web")
    report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
    assert report.ok
    assert len(report.notes) == 2
    assert all("pixels" in note and "not" in note for _, note in report.notes)
    assert cli.main(["privacy", "scan", "--repo", str(repo), "--staged"]) == 0
    assert "image pixels are not OCR-scanned" in capsys.readouterr().out


def test_visual_png_refuses_malformed_metadata_and_unsupported_content(repo):
    folder = repo / "web/visual/__screenshots__/fixture"
    folder.mkdir(parents=True)
    path = folder / "candidate.png"
    valid = synthetic_png()
    cases = [
        b"not an image",
        TERM.encode(),
        TERM.encode("utf-16"),
        b"\x89PNG\r\n\x1a\nnot a PNG",
        synthetic_png(extra=png_chunk(b"tEXt", b"note\0" + TERM.encode())),
        synthetic_png(extra=png_chunk(b"zTXt", b"note\0\0" + zlib.compress(TERM.encode()))),
        synthetic_png(extra=png_chunk(b"abcd", b"hidden")),
        valid[:-1],
        valid + b"trailing",
        valid[:29] + bytes([valid[29] ^ 1]) + valid[30:],
        synthetic_png(width=65536, height=65536),
        synthetic_png(pixels=b"\0\0"),
        synthetic_png(pixels=b"\5\0\0\0"),
    ]
    for payload in cases:
        path.write_bytes(payload)
        git(repo, "add", "--", str(path.relative_to(repo)))
        report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
        assert not report.ok
        assert TERM not in str(report.findings)


def test_binary_png_outside_visual_snapshot_folder_is_refused(repo):
    (repo / "arbitrary.png").write_bytes(synthetic_png())
    git(repo, "add", "--", "arbitrary.png")
    assert not privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok


def test_visual_png_detects_literal_terms_in_raw_and_decompressed_pixels(repo):
    folder = repo / "web/visual/__screenshots__/fixture"
    folder.mkdir(parents=True)
    path = folder / "candidate.png"
    for encoding in ("utf-8", "utf-16-le", "utf-16-be"):
        term = TERM.encode(encoding)
        width = (len(term) + 2) // 3
        pixels = b"\0" + term.ljust(width * 3, b"X")
        header = struct.pack(">IIBBBBB", width, 1, 8, 2, 0, 0, 0)
        for level in (0, 9):
            data = b"\x89PNG\r\n\x1a\n" + png_chunk(b"IHDR", header) + png_chunk(b"IDAT", zlib.compress(pixels, level)) + png_chunk(b"IEND", b"")
            path.write_bytes(data)
            git(repo, "add", "--", str(path.relative_to(repo)))
            report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
            assert not report.ok, (encoding, level)
            assert TERM not in str(report.findings)


def test_all_committed_visual_snapshots_match_supported_png_shape():
    root = Path(__file__).resolve().parents[2]
    folder = root / "web/visual/__screenshots__"
    paths = list(folder.rglob("*.png"))
    assert paths
    assert all(privacy._visual_snapshot_png(path.relative_to(root).as_posix(), path.read_bytes()) is not None for path in paths)


def test_unmerged_index_and_failed_git_enumeration_fail_closed(repo, monkeypatch):
    oid = git(repo, "rev-parse", "HEAD:document.txt").stdout.decode().strip()
    zeros = "0" * len(oid)
    entries = f"0 {zeros}\tdocument.txt\n100644 {oid} 1\tdocument.txt\n100644 {oid} 2\tdocument.txt\n".encode()
    git(repo, "update-index", "--index-info", input=entries)
    assert not privacy.scan_repo(repo, staged_only=True, terms=[TERM]).ok
    original = subprocess.run

    def fail_git(argv, *args, **kwargs):
        if "diff" in argv:
            return subprocess.CompletedProcess(argv, 1, b"", b"SYNTHETIC-SECRET")
        return original(argv, *args, **kwargs)

    monkeypatch.setattr(subprocess, "run", fail_git)
    report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
    assert not report.ok and "SYNTHETIC-SECRET" not in str(report.findings)


def test_non_utf8_filename_is_scanned_from_index(repo):
    raw_path = os.fsencode(repo) + b"/odd-\xff.txt"
    try:
        fd = os.open(raw_path, os.O_WRONLY | os.O_CREAT, 0o600)
    except OSError as exc:
        if exc.errno == errno.EILSEQ:
            pytest.skip("filesystem rejects non-UTF-8 filenames")
        raise
    with os.fdopen(fd, "wb") as stream:
        stream.write(TERM.encode())
    name = os.fsdecode(b"odd-\xff.txt")
    git(repo, "add", "--", name)
    os.unlink(raw_path)
    report = privacy.scan_repo(repo, staged_only=True, terms=[TERM])
    assert not report.ok
    assert any(path == name for path, _ in report.findings)


def test_non_utf8_path_metadata_is_not_split_or_lost():
    oid = b"a" * 40
    raw = b":000000 100644 " + b"0" * 40 + b" " + oid + b" A\0odd-\xff\nname.txt\0"
    entries = privacy._staged_entries(raw)
    assert len(entries) == 1
    assert os.fsencode(entries[0][0]) == b"odd-\xff\nname.txt"
