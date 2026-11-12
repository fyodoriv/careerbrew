"""scripts/public-readiness.sh: one planted defect per machine check fails its surface; a clean tree passes.

Each test runs the real script in a tiny scratch repo. Every name, pattern, marker and secret here is
synthetic, and the proof files are built inside the scratch repo.

The S11 tests need gitleaks. Without it they fail, so a runner without gitleaks cannot look green.
Set PUBLIC_READINESS_TESTS_ALLOW_NO_GITLEAKS=1 to skip them on purpose.
"""

import hashlib
import importlib.util
import json
import os
import shutil
import subprocess
import sys
import time
import zlib
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
_spec = importlib.util.spec_from_file_location("public_readiness", ROOT / "scripts" / "public-readiness.py")
assert _spec and _spec.loader
script = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(script)

GIT = ["git", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "-c", "user.name=Test", "-c", "user.email=test@example.com"]
ALLOW, BINARIES, SIGNATURES, REVIEW = script.ALLOW, script.BINARIES, script.SIGNATURES, script.REVIEW
GIVEN, FAMILY = "Zedrick", "Quillfeather"
OWNER = f"{GIVEN} {FAMILY}"
EMAIL = "zq@quill.example"
HANDLE = "zedquill"
ACCOUNT = "qzaccount"
SECRET_WORD = "plumbago"
# A fake token in a shape that the default gitleaks rules know. It is joined at run time, so no scanner reads it here.
FAKE_TOKEN = "gh" + "p_" + "x7Kq2mVn9RtB4wLz8PcJ5hYd3FgS6aEu1NiO"
HOME_DIRS = ["/" + "Users/someone/apps", "/" + "home/someone/apps"]
PYPROJECT = f'[project]\nname = "samplebrew"\nlicense = "MIT"\nauthors = [{{ name = "{OWNER}", email = "{EMAIL}" }}]\n'
CLEAN_SCAN = "#!/bin/sh\necho 'privacy scan: 4 file(s), 0 finding(s)'\n"
NO_MATCH = "OSS_READINESS_INTERNAL_PATTERN=zzznothing\n"
SURFACE_IDS = [f"S{n}" for n in range(1, 23)]
UV_ROOT = '[[package]]\nname = "samplebrew"\nversion = "0.1.0"\nsource = { editable = "." }\ndependencies = [{ name = "samplelib" }]\n'
UV_LOCK = UV_ROOT + '\n[[package]]\nname = "samplelib"\nversion = "1.0"\n'
PASSING = "#!/bin/sh\nexit 0\n"
ZONE, CITY, COUNTRY = "Pacific/Auckland", "Auckland", "New Zealand"
CODE2, CODE3 = "N" + "Z", "NZ" + "L"
PLUGIN = "samplebrew-zorgcorp"


def chunk(kind: bytes, body: bytes = b"", check: bytes | None = None) -> bytes:
    return len(body).to_bytes(4, "big") + kind + body + (zlib.crc32(kind + body).to_bytes(4, "big") if check is None else check)


def ihdr(width: int = 1, height: int = 1, colour: int = 0, interlace: int = 0) -> bytes:
    return chunk(b"IHDR", width.to_bytes(4, "big") + height.to_bytes(4, "big") + bytes([8, colour, 0, 0, interlace]))


def png(*extra: bytes, head: bytes | None = None, pixels: bytes = b"\0\0", idat: bytes | None = None, end: bytes | None = None) -> bytes:
    """A real picture of one grey pixel, with the given chunks before the pixel data."""
    data = chunk(b"IDAT", zlib.compress(pixels)) if idat is None else idat
    return b"\x89PNG\r\n\x1a\n" + (ihdr() if head is None else head) + b"".join(extra) + data + (chunk(b"IEND") if end is None else end)


PNG = png()


class Repo:
    def __init__(self, root: Path, tmp: Path):
        self.root, self.tmp = root, tmp
        # Shell startup files can replace fixture tools before the child script runs.
        self.env = {k: v for k, v in os.environ.items() if not k.startswith(("GIT_", "OSS_READINESS", "PUBLIC_READINESS")) and k not in {"BASH_ENV", "ENV", "CAREERBREW_HOME"}}
        self.env["PATH"] = f"{Path(sys.executable).parent}{os.pathsep}{self.env['PATH']}"
        self.env["HOME"] = str(tmp / "home")
        self.env["PUBLIC_READINESS_ACCOUNT"] = ACCOUNT
        self.env["PYTHONPATH"] = str(tmp / "site")
        # Owner mode reads the machine. These keep it away from the real zone, push list and Cargo registry.
        self.env["PUBLIC_READINESS_ZONE"] = ZONE
        self.env["DOTFILES_PUBLIC_PUSH_ALLOWLIST"] = str(tmp / "no-push-list")
        self.env["CARGO_HOME"] = str(tmp / "cargo")
        self.planted: list[str] = [SECRET_WORD, FAKE_TOKEN]

    def script(self, rel: str, content: str = PASSING) -> None:
        self.write(rel, content)
        (self.root / rel).chmod(0o755)

    def installed(self, name: str, licence: str, version: str = "1.0") -> None:
        """Python package metadata as an installer leaves it, outside the repo."""
        meta = self.tmp / "site" / f"{name}-{version}.dist-info" / "METADATA"
        meta.parent.mkdir(parents=True, exist_ok=True)
        meta.write_text(f"Metadata-Version: 2.4\nName: {name}\nVersion: {version}\n{licence}\n")

    def allow(self, *lines: str) -> None:
        self.write(ALLOW, "# comment\npyproject.toml\nREADME.md\n" + "".join(f"{line}\n" for line in lines))
        self.commit()

    def sha(self, rel: str) -> str:
        return hashlib.sha256((self.root / rel).read_bytes()).hexdigest()

    def inventory(self, *rows: tuple[str, str, str]) -> None:
        """Rows of (path, provenance, generator) with the hash of the file as it is now."""
        self.write(BINARIES, script.BINARIES_HEADER + "".join(f"{path}\t{word}\t{self.sha(path)}\t{generator}\n" for path, word, generator in rows))
        self.commit()

    def owner_machine(self) -> None:
        """What owner mode reads from the machine: the guards, the CLI and the time zone. All synthetic, outside the tree."""
        tools = self.tmp / "dotfiles"
        (tools / "bin").mkdir(parents=True, exist_ok=True)
        (tools / "config").mkdir(exist_ok=True)
        (tools / "bin" / "gh").write_text("#!/bin/sh\n# gh_public_leak_guard\nexit 0\n")
        (tools / "bin" / "gh").chmod(0o755)
        (tools / "config" / "public-push-remotes.txt").write_text(f"# list\ngithub.com/{HANDLE}/samplebrew  # ours\n")
        hooks = self.tmp / "hooks"
        hooks.mkdir(exist_ok=True)
        (hooks / "pre-push").write_text(PASSING + f"# {script.HOOK_MARK.decode()}\n")
        (hooks / "pre-push").chmod(0o755)
        self.git("config", "core.hooksPath", str(hooks))
        self.env["PATH"] = f"{tools / 'bin'}{os.pathsep}{self.env['PATH']}"
        del self.env["DOTFILES_PUBLIC_PUSH_ALLOWLIST"]
        self.cli(f"echo '{PLUGIN}   1.0   provides=work exec=False'\necho 'effective policy: {{}}'\n")
        self.planted += [ZONE, CITY, COUNTRY, PLUGIN]

    def cli(self, body: str | None) -> None:
        """The checkout's own CLI, never tracked. None removes it."""
        path = self.root / ".venv" / "bin" / "careerbrew"
        if body is None:
            path.unlink()
            return
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text("#!/bin/sh\n" + body)
        path.chmod(0o755)

    def write(self, rel: str, content: str | bytes) -> None:
        path = self.root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content if isinstance(content, bytes) else content.encode())

    def git(self, *args: str) -> None:
        subprocess.run([*GIT, *args], cwd=self.root, env=self.env, check=True, capture_output=True)

    def commit(self, message: str = "fixture") -> None:
        self.git("add", "-A")
        self.git("commit", "-q", "--allow-empty", "-m", message)

    def pattern_file(self, text: str) -> None:
        path = self.tmp / "pattern.env"
        path.write_text(text)
        self.env["OSS_READINESS_ENV"] = str(path)

    def raw(self, *args: str) -> subprocess.CompletedProcess[str]:
        r = subprocess.run(["bash", "scripts/public-readiness.sh", *args], cwd=self.root, env=self.env, capture_output=True, text=True, check=False)
        for text in self.planted:
            assert text.lower() not in (r.stdout + r.stderr).lower(), "the output holds planted text"
        return r

    def run(self, *args: str) -> tuple[int, dict[str, tuple[str, int]], str]:
        """Exit status, {surface: (status, count)}, and the summary line."""
        r = self.raw(*args)
        lines = r.stdout.splitlines()
        assert [ln.split()[0] for ln in lines[:-1]] == SURFACE_IDS, r.stdout + r.stderr
        return r.returncode, {ln.split()[0]: (ln.split()[2], int(ln.split()[3])) for ln in lines[:-1]}, lines[-1]

    def sign(self, *surfaces: str, extra: str = "", findings: int = 0, tree_hash: str | None = None) -> None:
        """Write one entry per surface. An entry for the same surface is replaced, never added twice."""
        tree_hash = tree_hash or self.raw("--tree-hash").stdout.strip()
        old = (self.root / SIGNATURES).read_text()[len(script.SIGNATURES_HEADER) :].splitlines()
        surfaces = surfaces or REVIEW
        kept = [line for line in old if line.split(" ")[0] not in surfaces]
        new = [f"{sid} judge-1 2026-11-10 {findings} {tree_hash}" for sid in surfaces]
        self.write(SIGNATURES, script.SIGNATURES_HEADER + "".join(f"{line}\n" for line in [*kept, *new]) + extra)
        self.commit()


@pytest.fixture
def repo(tmp_path):
    """A clean tree: owner identity only in allowlisted files, no binary, no old link, one journal file left out."""
    root = tmp_path / "repo"
    (root / "scripts").mkdir(parents=True)
    r = Repo(root, tmp_path)
    for name in ("public-readiness.sh", "public-readiness.py"):
        shutil.copy(ROOT / "scripts" / name, root / "scripts" / name)
    r.write("scripts/leak-check.sh", CLEAN_SCAN)
    (root / "scripts" / "leak-check.sh").chmod(0o755)
    r.write("pyproject.toml", PYPROJECT)
    r.write("README.md", f"Install from https://github.com/{HANDLE}/samplebrew. By {OWNER} <{EMAIL}>.\n")
    r.write(ALLOW, "# comment\npyproject.toml\nREADME.md\n")
    r.write(BINARIES, script.BINARIES_HEADER)
    r.write(SIGNATURES, script.SIGNATURES_HEADER)
    r.write("src/app.py", "print('hello')\n")
    r.write("docs/journal/seed.md", "seed\n")
    r.write(script.EXCLUDE, "docs/journal/\n")
    r.write("LICENSE", "MIT License\n")
    r.write("uv.lock", UV_LOCK)
    r.installed("samplelib", "License-Expression: MIT")
    r.script("scripts/check-docs.sh")
    subprocess.run([*GIT, "init", "-q"], cwd=root, env=r.env, check=True, capture_output=True)
    (root / ".git" / "info" / "exclude").write_text(".venv/\n")
    r.commit()
    r.planted += [OWNER, GIVEN, FAMILY, EMAIL, HANDLE, ACCOUNT]
    return r


def test_a_picture_with_any_chunk_outside_the_allowed_set_or_bytes_after_its_end_fails_s7(repo):
    repo.write("web/shot.spec.ts", "test()\n")
    extra = {
        "time": png(chunk(b"tIME", bytes(7))),
        "profile": png(chunk(b"iCCP", b"name")),
        "private": png(chunk(b"prVt", b"hi")),
        "after-end": PNG + b"a note after the end",
        "cut": PNG[:-6],
    }
    for name, data in extra.items():
        repo.write(f"web/{name}.png", data)
    # Chunks that hold picture data only stay clean.
    repo.write("web/clean.png", png(chunk(b"sRGB", b"\0"), chunk(b"pHYs", bytes(9))))
    repo.inventory(*((f"web/{name}.png", "generated", "web/shot.spec.ts") for name in [*extra, "clean"]))
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", len(extra))


NOTE = b"a note in a place for picture data"
HIDING = {
    "end-with-bytes": png(end=chunk(b"IEND", NOTE)),
    "wrong-checksum": png(end=chunk(b"IEND", check=b"note")),
    "long-colour-space": png(chunk(b"sRGB", NOTE)),
    "long-gamma": png(chunk(b"gAMA", NOTE)),
    "long-size": png(chunk(b"pHYs", NOTE)),
    "long-transparency": png(chunk(b"tRNS", NOTE)),
    "long-background": png(chunk(b"bKGD", NOTE)),
    "palette-in-grey-transparency": png(chunk(b"PLTE", bytes(6)), chunk(b"tRNS", bytes(3))),
    "odd-palette": png(chunk(b"PLTE", NOTE[:4])),
    "bytes-after-the-pixels": png(idat=chunk(b"IDAT", zlib.compress(b"\0\0") + NOTE)),
    "more-pixels-than-the-header": png(pixels=b"\0\0" + NOTE),
    "fewer-pixels-than-the-header": png(pixels=b"\0"),
    "not-a-zlib-stream": png(idat=chunk(b"IDAT", NOTE)),
    "no-pixels": png(idat=b""),
    "two-headers": png(ihdr()),
    "no-width": png(head=ihdr(width=0)),
    "header-of-another-size": png(head=chunk(b"IHDR", bytes(4) + NOTE)),
}
CLEAN = {
    "palette": png(chunk(b"PLTE", bytes(6)), chunk(b"tRNS", bytes(2)), chunk(b"bKGD", b"\0"), head=ihdr(colour=3)),
    "colour": png(chunk(b"tRNS", bytes(6)), chunk(b"sBIT", bytes(3)), head=ihdr(width=2, height=2, colour=2), pixels=bytes(14)),
    "interlaced": png(head=ihdr(width=3, height=3, interlace=1), pixels=bytes(15)),
    "two-data-chunks": png(idat=chunk(b"IDAT", zlib.compress(b"\0\0")[:3]) + chunk(b"IDAT", zlib.compress(b"\0\0")[3:])),
}


@pytest.mark.parametrize("name", HIDING)
def test_a_picture_with_room_for_other_bytes_in_an_allowed_chunk_fails_s7(repo, name):
    repo.write("web/shot.spec.ts", "test()\n")
    repo.write("web/shot.png", HIDING[name])
    repo.inventory(("web/shot.png", "generated", "web/shot.spec.ts"))
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", 1)
    assert "metadata=1" in repo.raw().stdout


def test_pictures_of_each_colour_type_with_exact_chunks_are_clean_for_s7(repo):
    repo.write("web/shot.spec.ts", "test()\n")
    for name, data in CLEAN.items():
        repo.write(f"web/{name}.png", data)
    repo.inventory(*((f"web/{name}.png", "generated", "web/shot.spec.ts") for name in CLEAN))
    assert repo.run()[1]["S7"] == ("unsigned", 0)


def wav(*chunks: tuple[bytes, bytes]) -> bytes:
    body = b"WAVE" + b"".join(kind + len(data).to_bytes(4, "little") + data for kind, data in chunks)
    return b"RIFF" + len(body).to_bytes(4, "little") + body


def test_a_sound_file_with_a_text_chunk_fails_s7_and_one_with_samples_only_is_clean(repo):
    samples = (b"fmt ", bytes(16)), (b"data", bytes(8))
    repo.write("tests/tone.wav", wav(*samples))
    repo.inventory(("tests/tone.wav", "synthetic", "-"))
    assert repo.run()[1]["S7"] == ("unsigned", 0)
    repo.write("tests/tone.wav", wav(*samples, (b"LIST", b"INFOINAMname")))
    repo.inventory(("tests/tone.wav", "synthetic", "-"))
    assert repo.run()[1]["S7"] == ("fail", 1)
    repo.write("tests/tone.wav", wav(*samples) + b"tail")
    repo.inventory(("tests/tone.wav", "synthetic", "-"))
    assert repo.run()[1]["S7"] == ("fail", 1)


def test_a_media_file_without_a_nul_byte_is_a_binary_that_only_a_reviewed_provenance_clears(repo):
    files = {
        "docs/photo.jpg": b"\xff\xd8\xff plain bytes",  # by name and by first bytes
        "docs/renamed.txt": b"GIF89a plain bytes",  # by first bytes only
        "docs/paper.pdf": b"%PDF-1.4 plain bytes",
        "web/font.woff2": b"plain bytes",  # by name only
        "docs/bundle.zip": b"PK\x03\x04plain bytes",
        "docs/anim.webp": b"RIFF\x04\x00\x00\x00WEBP",
    }
    for path, data in files.items():
        assert b"\0" not in data or path.endswith(".webp")
        repo.write(path, data)
    repo.write("web/make.ts", "draw()\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", len(files))
    assert [line.split("\t")[0] for line in repo.raw("--list-binaries").stdout.splitlines()] == sorted(files)
    # This script cannot read their metadata, so "generated" and "synthetic" prove nothing for them.
    repo.inventory(*((path, "generated", "web/make.ts") for path in files))
    assert repo.run()[1]["S7"] == ("fail", len(files))
    repo.inventory(*((path, "synthetic", "-") for path in files))
    assert repo.run()[1]["S7"] == ("fail", len(files))
    repo.inventory(*((path, "third-party-licensed" if n % 2 else "own-recording", "-") for n, path in enumerate(files)))
    assert repo.run()[1]["S7"] == ("unsigned", 0)


@pytest.fixture
def gitleaks():
    """S11 tests need gitleaks. Its absence fails them unless the runner opts out by name."""
    if shutil.which("gitleaks"):
        return
    if os.environ.get("PUBLIC_READINESS_TESTS_ALLOW_NO_GITLEAKS") == "1":
        pytest.skip("gitleaks is not installed and PUBLIC_READINESS_TESTS_ALLOW_NO_GITLEAKS=1 is set")
    pytest.fail("gitleaks is not installed, so S11 is untested. Install it, or set PUBLIC_READINESS_TESTS_ALLOW_NO_GITLEAKS=1 to skip on purpose.")


def test_a_clean_tree_passes_in_public_mode_and_never_claims_an_owner_or_review_check(repo, gitleaks):
    code, got, summary = repo.run()
    assert code == 0, summary
    assert {got[s] for s in ("S3", "S10", "S11", "S13", "S21", "S22")} == {("pass", 0)}
    assert got["S7"] == ("unsigned", 0)
    assert {got[s][0] for s in ("S1", "S2", "S14", "S16", "S20")} == {"owner-only"}
    assert {got[s][0] for s in ("S12", "S15", "S19")} == {"release-day"}
    # The cheap self-checks can fail S17. Only the full first run passes it.
    assert {got[s][0] for s in ("S4", "S5", "S6", "S8", "S9", "S17", "S18")} == {"unmeasured"}
    assert "mode=public" in summary and " pass=6 " in summary and " unsigned=1 " in summary
    # Public mode runs no owner check and reads no signature, so it never says "ready".
    assert "result=no-failures" in summary and "result=ready" not in summary


@pytest.mark.parametrize("marker", [OWNER, OWNER.upper(), GIVEN, FAMILY.lower(), EMAIL, f"github.com/{HANDLE}/other", f"user {ACCOUNT} ran it", *HOME_DIRS])
def test_owner_identity_outside_the_allowlist_fails_s3(repo, marker):
    repo.write("tests/fixture.txt", f"written by {marker}\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S3"] == ("fail", 1)


def test_s3_matches_whole_words_and_file_names_and_skips_the_allowlist(repo):
    repo.write("tests/fixture.txt", f"{GIVEN}son and {FAMILY}s and x{ACCOUNT} are other words\n")
    repo.write("README.md", f"By {OWNER}, also known as {ACCOUNT}. See github.com/{HANDLE}/samplebrew.\n")
    repo.commit()
    assert repo.run()[1]["S3"] == ("pass", 0)
    repo.write(f"docs/{GIVEN.lower()}_notes.md", "clean\n")
    repo.commit()
    assert repo.run()[1]["S3"] == ("fail", 1)


def test_a_tree_with_no_author_fails_s3_instead_of_passing_empty(repo):
    repo.write("pyproject.toml", '[project]\nname = "samplebrew"\n')
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S3"] == ("fail", 0)


def test_a_link_to_an_old_pull_request_or_issue_fails_s13(repo):
    repo.write("docs/a.md", "see https://github.com/acme/samplebrew/pull/12\n")
    repo.write("docs/b.md", "see samplebrew/issues/7\n")
    repo.write("docs/c.md", "see samplebrew/pull/ and otherbrew/pull/3\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S13"] == ("fail", 2)


def test_a_tree_with_no_project_name_is_an_error_for_s13_in_both_modes(repo):
    repo.write("pyproject.toml", f'[project]\nauthors = [{{ name = "{OWNER}", email = "{EMAIL}" }}]\n')
    repo.commit()
    code, got, summary = repo.run()
    assert code == 1 and got["S13"] == ("error", 0) and "result=not-ready" in summary
    assert repo.run("--release")[1]["S13"] == ("error", 0)


def test_an_unlisted_binary_fails_s7_and_only_a_known_provenance_with_its_hash_clears_it(repo):
    repo.write("web/shot.png", PNG)
    repo.write("web/shot.spec.ts", "test()\n")
    repo.write("web/other.bin", b"\0\1\2")
    repo.write("web/voice.wav", b"RIFF\0\0")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", 3)
    listed = repo.raw("--list-binaries").stdout.splitlines()
    assert listed == [f"{path}\t{repo.sha(path)}" for path in ("web/other.bin", "web/shot.png", "web/voice.wav")]
    shot = ("web/shot.png", "generated", "web/shot.spec.ts")
    repo.inventory(shot, ("web/other.bin", "", "-"), ("web/voice.wav", "someone said it is fine", "-"))
    assert repo.run()[1]["S7"] == ("fail", 2)
    # A file that is listed but not cleared still counts.
    repo.inventory(shot, ("web/other.bin", "third-party-licensed", "-"), ("web/voice.wav", "pending", "-"))
    assert repo.run()[1]["S7"] == ("fail", 1)
    repo.inventory(shot, ("web/other.bin", "third-party-licensed", "-"), ("web/voice.wav", "own-recording", "-"))
    assert repo.run()[1]["S7"] == ("unsigned", 0)


@pytest.mark.parametrize(
    ("path", "data"),
    [
        ("docs/.DS_Store", b"\0\0\0\1Bud1\0\0\x10\0"),
        ("data/cache.db", b"SQLite format 3\0" + bytes(20)),
        ("docs/blob", b"\0\1\2\3"),
        ("docs/icon.icns", b"icns\0\0\0\x10" + bytes(8)),
        ("docs/clip.dat", b"\0\0\0\x18ftypmp42" + bytes(12)),  # a film under another name
        ("docs/scan.dat", b"II*\0" + bytes(8)),
        ("docs/archive", b"notes.txt" + bytes(248) + b"ustar\0"),  # a tar file without an extension
        ("docs/state.pkl", b"\x80\x04\x95\x05\0\0\0\0\0\0\0"),
        ("docs/notes16.txt", "plain words".encode("utf-16")),  # with a byte order mark
        ("docs/notes16le.txt", "plain words".encode("utf-16-le")),  # without one
        ("docs/notes32.txt", "plain words".encode("utf-32")),
        # No NUL byte in the first 8000 bytes: the whole content decides.
        ("docs/cjk.txt", ("\u4e2d" * 4100 + "plain words").encode("utf-16-le")),
        ("docs/late.txt", b"a" * 8001 + "plain words".encode("utf-16-le")),
    ],
)
def test_a_binary_of_a_format_that_the_script_cannot_read_fails_s7_until_a_person_reviews_it(repo, path, data):
    repo.write(path, data)
    repo.write("web/make.ts", "draw()\n")
    repo.commit()
    assert repo.raw("--list-binaries").stdout.split("\t")[0] == path
    for row in ((path, "synthetic", "-"), (path, "generated", "web/make.ts")):
        repo.inventory(row)
        code, got, _ = repo.run()
        assert code == 1 and got["S7"] == ("fail", 1)
        assert "unverified=1" in repo.raw().stdout
    repo.inventory((path, "own-recording", "-"))
    assert repo.run()[1]["S7"] == ("unsigned", 0)


def test_text_in_utf_16_cannot_hide_an_identifier_because_s7_counts_the_file(repo):
    text = f"ssh zed-macbook-pro.{LOCAL} as {OWNER}\n"
    repo.planted.append(text)
    repo.write("docs/notes.txt", text.encode("utf-16"))
    repo.inventory(("docs/notes.txt", "synthetic", "-"))
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", 1)


def test_a_binary_whose_content_changed_after_its_inventory_row_fails_s7(repo):
    repo.write("web/shot.png", PNG)
    repo.write("web/shot.spec.ts", "test()\n")
    repo.inventory(("web/shot.png", "generated", "web/shot.spec.ts"))
    assert repo.run()[1]["S7"] == ("unsigned", 0)
    repo.write("web/shot.png", png(pixels=b"\0\1"))  # the picture was drawn again
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S7"] == ("fail", 1)
    repo.inventory(("web/shot.png", "generated", "web/shot.spec.ts"))
    assert repo.run()[1]["S7"] == ("unsigned", 0)


@pytest.mark.parametrize(
    "row",
    [
        "web/shot.png\tgenerated\t{sha}\tweb/gone.spec.ts",  # the generator is not in the tree
        "web/shot.png\tgenerated\t{sha}\t-",
        "web/shot.png\tsynthetic\t{sha}\tsomeone said so",  # only a generated row names a generator
        "web/shot.png\tgenerated\t{sha}",  # a row of the old two-column shape plus a hash
        "web/shot.png\tgenerated\t{sha}\tweb/shot.spec.ts\nweb/gone.png\tgenerated\t{sha}\tweb/shot.spec.ts",  # a row for no file
        "web/shot.png\tgenerated\t{sha}\tweb/shot.spec.ts\nweb/shot.png\tsynthetic\t{sha}\t-",  # two rows for one file
    ],
)
def test_an_inventory_row_that_proves_nothing_fails_s7(repo, row):
    repo.write("web/shot.png", PNG)
    repo.write("web/shot.spec.ts", "test()\n")
    repo.write(BINARIES, script.BINARIES_HEADER + row.format(sha=hashlib.sha256(PNG).hexdigest()) + "\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S7"][0] == "fail" and got["S7"][1] >= 1


def test_an_inventory_with_the_old_header_and_a_picture_with_a_text_chunk_fail_s7(repo):
    repo.write(BINARIES, "path\tprovenance\n")
    repo.commit()
    assert repo.run()[1]["S7"] == ("fail", 1)
    noted = png(chunk(b"tEXt", b"note"))
    repo.write("web/shot.png", noted)
    repo.write("web/shot.spec.ts", "test()\n")
    repo.inventory(("web/shot.png", "generated", "web/shot.spec.ts"))
    assert repo.run()[1]["S7"] == ("fail", 1)


def test_a_planted_secret_fails_s11(repo, gitleaks):
    repo.write("config/settings.txt", f"token = {FAKE_TOKEN}\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S11"][0] == "fail" and got["S11"][1] >= 1


def test_an_allow_comment_does_not_hide_a_secret_from_s11(repo, gitleaks):
    repo.write("config/settings.txt", f"token = {FAKE_TOKEN} # gitleaks:allow\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S11"][0] == "fail" and got["S11"][1] >= 1


def test_the_callers_gitleaks_settings_do_not_hide_a_secret_from_s11(repo, gitleaks, tmp_path):
    allow_all = tmp_path / "allow-all.toml"
    allow_all.write_text("[extend]\nuseDefault = true\n[allowlist]\npaths = ['.*']\n")
    repo.env["GITLEAKS_CONFIG"] = str(allow_all)
    repo.env["GITLEAKS_CONFIG_TOML"] = allow_all.read_text()
    repo.write("config/settings.txt", f"token = {FAKE_TOKEN}\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S11"][0] == "fail" and got["S11"][1] >= 1


def test_an_untracked_export_ignore_rule_does_not_hide_a_secret_from_s11(repo, gitleaks):
    repo.write("config/settings.txt", f"token = {FAKE_TOKEN}\n")
    repo.write(".gitattributes", "docs/** export-ignore\n")
    repo.commit()
    info = repo.root / ".git" / "info"
    info.mkdir(exist_ok=True)
    (info / "attributes").write_text("config/settings.txt export-ignore\nconfig export-ignore\n")
    code, got, _ = repo.run()
    assert code == 1 and got["S11"][0] == "fail" and got["S11"][1] >= 1


@pytest.mark.parametrize(
    "pyproject, readme",
    [
        (f'[project]\nname = "samplebrew"\nauthors = [{{ name = "Al {FAMILY}", email = "{EMAIL}" }}]\n', None),
        (None, f"By {OWNER}. No project link here.\n"),
    ],
)
def test_an_owner_marker_that_cannot_be_checked_is_an_error_for_s3(repo, pyproject, readme):
    if pyproject:
        repo.write("pyproject.toml", pyproject)
    if readme:
        repo.write("README.md", readme)
    repo.commit()
    code, got, summary = repo.run()
    assert code == 1 and got["S3"] == ("error", 0) and "result=not-ready" in summary


def test_an_owner_name_with_other_letters_fails_s3_in_any_letter_case(repo):
    family = "\u00c5ngstra\u00dfe"
    repo.planted.append(family)
    repo.write("pyproject.toml", PYPROJECT.replace(FAMILY, family))
    repo.write("docs/a.md", f"written by {family.upper()}\n")
    repo.write("docs/b.md", f"written by {family.lower()}\n")
    repo.write(f"docs/{family.upper()}/c.md", "clean\n")
    repo.write("docs/d.md", f"the {family.lower()}n list\n")  # another word
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S3"] == ("fail", 3)


def test_a_short_account_name_is_an_error_for_s3(repo):
    repo.env["PUBLIC_READINESS_ACCOUNT"] = "qz"
    assert repo.run()[1]["S3"] == ("error", 0)


def test_a_missing_gitleaks_is_an_error_that_blocks_both_modes(repo, tmp_path):
    if shutil.which("gitleaks", path="/usr/bin:/bin"):
        if os.environ.get("PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GITLEAKS") == "1":
            pytest.skip("gitleaks is in a system folder and PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GITLEAKS=1 is set")
        pytest.fail("gitleaks is in a system folder, so no PATH here lacks it and the missing-gitleaks case is untested. Set PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GITLEAKS=1 to skip on purpose.")
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "python3").symlink_to(sys.executable)
    repo.env["PATH"] = f"{tools}:/usr/bin:/bin"
    repo.pattern_file(NO_MATCH)
    for mode in ([], ["--release"]):
        code, got, summary = repo.run(*mode)
        assert code == 1 and got["S11"] == ("error", 0) and "result=not-ready" in summary


@pytest.mark.parametrize("path, content", [(".gitleaks.toml", "[allowlist]\n"), ("docs/.gitleaksignore", "x\n")])
def test_a_file_that_could_hide_content_from_the_secret_scan_is_an_error(repo, path, content):
    repo.write(path, content)
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S11"] == ("error", 1)


def test_the_script_reads_the_ref_not_the_working_tree(repo):
    repo.write("tests/fixture.txt", f"written by {OWNER}\n")
    assert repo.run()[1]["S3"] == ("pass", 0)
    repo.commit()
    assert repo.run("--tree", "HEAD~1")[1]["S3"] == ("pass", 0)
    assert repo.run("--tree", "HEAD")[1]["S3"] == ("fail", 1)


@pytest.mark.parametrize("ref", ["--output=x", "no-such-ref", "HEAD:README.md"])
def test_a_ref_that_is_not_a_commit_is_a_usage_error(repo, ref):
    r = repo.raw(f"--tree={ref}")
    assert r.returncode == 2 and r.stdout == ""
    assert not (repo.root / "x").exists()


def test_release_mode_counts_private_pattern_hits_in_text_and_file_names(repo):
    repo.pattern_file(f"export OSS_READINESS_INTERNAL_PATTERN='{SECRET_WORD}|zzznothing'\n")
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S1"] == ("pass", 0)
    repo.write("docs/notes.md", f"one {SECRET_WORD.upper()} line\nclean line\nanother {SECRET_WORD}\n")
    repo.write(f"docs/{SECRET_WORD}.md", "clean\n")
    repo.commit()
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S1"] == ("fail", 3)
    assert repo.run()[1]["S1"] == ("owner-only", 0)


def test_release_mode_counts_a_private_pattern_hit_in_the_commit_message(repo):
    repo.pattern_file(f"OSS_READINESS_INTERNAL_PATTERN='{SECRET_WORD}'\n")
    repo.commit(message=f"chore: a note about {SECRET_WORD}")
    assert repo.run("--release")[1]["S1"] == ("fail", 1)


def test_an_invalid_private_pattern_is_an_error(repo):
    repo.pattern_file("OSS_READINESS_INTERNAL_PATTERN='(unclosed'\n")
    code, got, summary = repo.run("--release")
    assert code == 1 and got["S1"] == ("error", 0) and "result=not-ready" in summary


@pytest.mark.parametrize("content", [None, "", "OSS_READINESS_INTERNAL_PATTERN=''\n", "OTHER=1\n", "exit 3\n"])
def test_release_mode_fails_s1_when_the_pattern_file_is_missing_or_empty(repo, content):
    if content is None:
        repo.env["OSS_READINESS_ENV"] = str(repo.tmp / "absent.env")
    else:
        repo.pattern_file(content)
    repo.env["OSS_READINESS_INTERNAL_PATTERN"] = SECRET_WORD  # a value from the caller's shell must not stand in for the file
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S1"] == ("fail", 0)


def test_the_private_pattern_never_reaches_a_child_process_or_a_trace(repo):
    seen = repo.tmp / "seen-by-child"
    probe = f'[ -z "${{PUBLIC_READINESS_PATTERN:-}}${{OSS_READINESS_INTERNAL_PATTERN:-}}${{EXTRA_FROM_FILE:-}}" ] || echo leaked > "{seen}"\n'
    repo.write("scripts/leak-check.sh", CLEAN_SCAN + probe)
    repo.commit()
    repo.pattern_file(f"export OSS_READINESS_INTERNAL_PATTERN='{SECRET_WORD}'\nexport EXTRA_FROM_FILE=1\n")
    repo.env["OSS_READINESS_INTERNAL_PATTERN"] = SECRET_WORD
    repo.env["SHELLOPTS"] = "xtrace"  # a caller that traces every bash script
    code, got, _ = repo.run("--release")
    assert got["S1"] == ("pass", 0) and got["S2"] == ("pass", 0)
    assert not seen.exists()


def test_release_mode_uses_the_privacy_scan_status_and_count_and_rejects_an_empty_scan(repo):
    repo.pattern_file(NO_MATCH)
    assert repo.run("--release")[1]["S2"] == ("pass", 0)
    scan = repo.root / "scripts" / "leak-check.sh"
    scan.write_text(f"#!/bin/sh\necho 'LEAK  a.md:1: {SECRET_WORD}'\necho 'privacy scan: 4 file(s), 2 finding(s)'\nexit 1\n")
    repo.commit()
    assert repo.run("--release")[1]["S2"] == ("fail", 2)
    scan.write_text(CLEAN_SCAN + "exit 1\n")
    repo.commit()
    assert repo.run("--release")[1]["S2"] == ("fail", 0)
    scan.write_text("#!/bin/sh\necho 'privacy scan: 0 file(s), 0 finding(s)'\n")
    repo.commit()
    assert repo.run("--release")[1]["S2"] == ("fail", 0)
    scan.write_text("#!/bin/sh\nexit 0\n")
    repo.commit()
    assert repo.run("--release")[1]["S2"] == ("error", 0)


def test_the_privacy_scan_is_trusted_only_when_checkout_and_index_equal_the_ref(repo):
    repo.pattern_file(NO_MATCH)
    scan = repo.root / "scripts" / "leak-check.sh"
    scan.write_text(CLEAN_SCAN + "# edited after the commit\n")
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S2"] == ("unmeasured", 0)
    repo.git("add", "scripts/leak-check.sh")
    scan.write_text(CLEAN_SCAN)  # the checkout equals the ref again; only the index differs
    assert repo.run("--release")[1]["S2"] == ("unmeasured", 0)
    repo.git("reset", "-q")
    assert repo.run("--release")[1]["S2"] == ("pass", 0)


def test_review_surfaces_need_a_signature_for_this_tree_with_no_findings_left(repo):
    repo.pattern_file(NO_MATCH)
    code, got, summary = repo.run("--release")
    assert code == 1 and {got[s] for s in REVIEW} == {("fail", 1)}
    repo.sign()
    code, got, summary = repo.run("--release")
    assert {got[s] for s in REVIEW} == {("pass", 0)}
    assert {got[s] for s in ("S1", "S2")} == {("pass", 0)}
    # This machine has no guard, no CLI and no first run yet, so owner mode is not ready.
    assert code == 1 and got["S14"][0] in ("fail", "error") and got["S16"][0] == "error" and got["S17"][0] == "unmeasured" and "result=not-ready" in summary
    # Public mode never reads a signature: S7 stays unsigned there.
    assert repo.run()[1]["S7"] == ("unsigned", 0)


def test_a_signed_s7_still_fails_while_a_binary_has_no_provenance(repo):
    repo.pattern_file(NO_MATCH)
    repo.write("web/shot.png", PNG)
    repo.commit()
    repo.sign()
    got = repo.run("--release")[1]
    assert got["S7"] == ("fail", 1) and got["S4"] == ("pass", 0)


def test_a_new_signature_replaces_the_entry_for_its_surface(repo):
    repo.pattern_file(NO_MATCH)
    repo.sign()
    repo.sign("S4", findings=2)
    lines = (repo.root / SIGNATURES).read_text()[len(script.SIGNATURES_HEADER) :].splitlines()
    assert sorted(line.split(" ")[0] for line in lines) == sorted(REVIEW)
    got = repo.run("--release")[1]
    assert got["S4"] == ("fail", 2) and got["S5"] == ("pass", 0)
    repo.sign("S4")
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("pass", 0)}


def test_a_signature_does_not_change_the_hash_it_signs_but_any_other_change_does(repo):
    repo.pattern_file(NO_MATCH)
    repo.sign()
    repo.sign()
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("pass", 0)}
    repo.write("src/app.py", "print('changed after the review')\n")
    repo.commit()
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("fail", 1)}


def test_a_signature_with_another_hash_or_with_findings_left_fails(repo):
    repo.pattern_file(NO_MATCH)
    repo.sign(tree_hash="0" * 64)
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("fail", 1)}
    repo.sign(findings=3)
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("fail", 3)}


@pytest.mark.parametrize(
    "extra",
    [f"S4 found a note about {SECRET_WORD}\n", "\n", f"S4 {OWNER} 2026-11-10 0 {'0' * 64}\n", f"S3 judge-1 2026-11-10 0 {'0' * 64}\n", f"S4 judge-2 2026-13-40 0 {'0' * 64}\n", "trailing"],
)
def test_any_other_text_or_a_second_entry_in_the_signature_file_fails_every_review_surface(repo, extra):
    repo.pattern_file(NO_MATCH)
    repo.sign(extra=extra)
    assert {repo.run("--release")[1][s] for s in REVIEW} == {("fail", 1)}


EXCLUDE = script.EXCLUDE
JOURNAL = "docs/journal/"


def exclude(repo, *lines: str) -> None:
    repo.write(EXCLUDE, "# reason\n  # a comment after spaces\n" + "".join(f"{line}\n" for line in lines))
    repo.commit()


def test_a_hit_inside_an_excluded_folder_does_not_count_and_the_same_hit_outside_does(repo, gitleaks):
    repo.pattern_file(f"OSS_READINESS_INTERNAL_PATTERN='{SECRET_WORD}'\n")
    defect = f"by {OWNER}, see samplebrew/pull/12, token = {FAKE_TOKEN}, {SECRET_WORD}\n"
    repo.write(f"{JOURNAL}old.md", defect)
    repo.write(f"{JOURNAL}shot.png", b"\x89PNG\0")
    exclude(repo, JOURNAL)
    code, got, summary = repo.run()
    assert code == 0 and got["S3"] == ("pass", 0) and got["S11"] == ("pass", 0) and got["S13"] == ("pass", 0) and got["S7"] == ("unsigned", 0)
    assert " excluded=3 " in summary
    assert repo.run("--release")[1]["S1"] == ("pass", 0)
    repo.write("docs/kept.md", defect)
    repo.write("docs/shot.png", b"\x89PNG\0")
    repo.commit()
    code, got, summary = repo.run()
    assert code == 1 and got["S3"] == ("fail", 1) and got["S11"][0] == "fail" and got["S13"] == ("fail", 1) and got["S7"] == ("fail", 1)
    assert " excluded=3 " in summary
    assert repo.run("--release")[1]["S1"] == ("fail", 1)


def test_an_excluded_file_name_does_not_count_for_s1_or_s3(repo):
    repo.pattern_file(f"OSS_READINESS_INTERNAL_PATTERN='{SECRET_WORD}'\n")
    repo.write(f"{JOURNAL}{SECRET_WORD}-{GIVEN.lower()}.md", "clean\n")
    exclude(repo, JOURNAL)
    got = repo.run("--release")[1]
    assert got["S1"] == ("pass", 0) and got["S3"] == ("pass", 0)


def test_a_folder_line_needs_its_slash_and_a_keep_line_puts_one_file_back(repo):
    repo.write(f"{JOURNAL}old.md", f"by {OWNER}\n")
    repo.write(f"{JOURNAL}data.csv", f"by {OWNER}\n")
    repo.write("docs/journal-notes.md", f"by {OWNER}\n")
    exclude(repo, JOURNAL, f"!{JOURNAL}data.csv")
    code, got, summary = repo.run()
    # The folder line leaves out old.md only: data.csv is kept, and journal-notes.md is another path.
    assert code == 1 and got["S3"] == ("fail", 2) and " excluded=2 " in summary


@pytest.mark.parametrize("lines", [[JOURNAL, "docs/gone/"], [JOURNAL, "docs/journal"], [JOURNAL, "!docs/gone.md"], [JOURNAL, "!src/app.py"]])
def test_a_stale_line_in_the_exclusion_list_is_an_error_that_blocks_both_modes(repo, lines):
    repo.pattern_file(NO_MATCH)
    repo.write(f"{JOURNAL}old.md", "clean\n")
    exclude(repo, *lines)
    for mode in ([], ["--release"]):
        code, got, summary = repo.run(*mode)
        assert code == 1 and "result=not-ready" in summary
        assert {got[s] for s in ("S3", "S7", "S11", "S13")} == {("error", 1)}
    assert repo.run()[1]["S1"] == ("owner-only", 0) and repo.run("--release")[1]["S1"] == ("error", 1)
    for action in (["--tree-hash"], ["--release-tree-id"], ["--write-release-tree", str(repo.tmp / "out")]):
        r = repo.raw(*action)
        assert r.returncode == 2 and r.stdout == ""
    assert not (repo.tmp / "out").exists()


def test_the_tree_hash_covers_the_release_tree_only(repo):
    repo.write(f"{JOURNAL}old.md", "one\n")
    exclude(repo, JOURNAL)
    before = repo.raw("--tree-hash").stdout
    repo.write(f"{JOURNAL}old.md", "two\n")
    repo.write(f"{JOURNAL}new.md", "three\n")
    repo.commit()
    assert repo.raw("--tree-hash").stdout == before
    repo.write("src/app.py", "print('changed')\n")
    repo.commit()
    assert repo.raw("--tree-hash").stdout != before


def test_the_written_release_tree_is_the_tree_the_checks_read_and_a_run_inside_it_gives_the_same_hash(repo):
    """The list stays in the release tree, so a --released run on a commit of that tree signs the same hash."""
    repo.write(f"{JOURNAL}old.md", f"by {OWNER}\n")
    repo.write("scripts/tool.sh", "#!/bin/sh\n")
    (repo.root / "scripts" / "tool.sh").chmod(0o755)
    (repo.root / "LINK.md").symlink_to("README.md")
    exclude(repo, JOURNAL)
    out = repo.tmp / "out"
    r = repo.raw("--write-release-tree", str(out))
    assert r.returncode == 0 and r.stdout.strip() == "public-readiness: wrote 15 file(s), left out 2"
    assert not (out / "docs" / "journal").exists() and (out / EXCLUDE).exists()
    assert (out / "LINK.md").is_symlink() and os.access(out / "scripts" / "tool.sh", os.X_OK) and not os.access(out / "src" / "app.py", os.X_OK)
    assert repo.raw("--write-release-tree", str(out)).returncode == 2  # never into a folder that holds files
    released = Repo(out, repo.tmp)
    subprocess.run([*GIT, "init", "-q"], cwd=out, env=released.env, check=True, capture_output=True)
    released.commit()
    written = subprocess.run([*GIT, "rev-parse", "HEAD^{tree}"], cwd=out, env=released.env, check=True, capture_output=True, text=True).stdout
    assert repo.raw("--release-tree-id").stdout == written
    assert released.raw("--released", "--tree-hash").stdout == repo.raw("--tree-hash").stdout
    code, got, summary = released.run("--released")
    assert code == 0 and got["S3"] == ("pass", 0) and got["S21"] == ("pass", 0) and " excluded=0 " in summary
    # Without --released the list leaves out no file there, and that blocks.
    code, got, _ = released.run()
    assert code == 1 and got["S3"][0] == "error" and released.raw("--tree-hash").returncode == 2
    # A left-out file that comes back into the release commit blocks a --released run.
    released.write(f"{JOURNAL}back.md", "clean\n")
    released.commit()
    code, got, _ = released.run("--released")
    assert code == 1 and {got[s] for s in ("S3", "S7", "S11", "S13", "S21")} == {("error", 1)}
    assert released.raw("--released", "--release-tree-id").returncode == 2


ACTIONS = (["--tree-hash"], ["--release-tree-id"], ["--list-dangling"], ["--write-release-tree", "{out}"])
EVERYTHING = ["config/", "docs/", "scripts/", "src/", "pyproject.toml", "README.md", "LICENSE", "uv.lock"]


def blocked(repo) -> set[tuple[str, int]]:
    """The tree-reading surfaces of a blocked run. Both modes are not ready, and no tree action runs."""
    out = repo.tmp / "blocked-out"
    for action in ACTIONS:
        r = repo.raw(*(arg.format(out=out) for arg in action))
        assert r.returncode == 2 and r.stdout == "", action
    assert not out.exists()
    for mode in ([], ["--release"]):
        code, got, summary = repo.run(*mode)
        assert code == 1 and "result=not-ready" in summary
    return {got[s] for s in ("S1", "S3", "S7", "S11", "S13", "S21")}


@pytest.mark.parametrize(
    ("content", "count"),
    [
        (None, 0),  # the list is missing
        ("", 0),  # the list is empty
        ("# only a comment\n\n   \n", 0),
        ("!docs/journal/seed.md\n", 0),  # no line leaves a path out
        ("docs/journal/\n!docs/journal/seed.md\n", 0),  # every left-out file is kept again: 0 files left out
        ("docs/journal/\n!docs/journal/\n", 0),
        ("".join(f"{line}\n" for line in EVERYTHING), 14),  # the list leaves every file out
        (f"docs/journal/\n{script.EXCLUDE}\n", 1),  # the list leaves itself out
    ],
)
def test_a_list_that_does_not_work_is_an_error_that_blocks_both_modes_and_every_tree_action(repo, content, count):
    repo.pattern_file(NO_MATCH)
    if content is None:
        (repo.root / EXCLUDE).unlink()
    else:
        repo.write(EXCLUDE, content)
    repo.commit()
    assert blocked(repo) == {("error", count)}


def plant(repo, mode: str, oid: str, path: str) -> None:
    """Add one tree entry without a file in the checkout, then commit only the index."""
    repo.git("update-index", "--add", "--cacheinfo", f"{mode},{oid},{path}")
    repo.git("commit", "-q", "-m", "planted entry")


def test_a_path_that_differs_from_a_list_line_only_by_letter_case_is_an_error(repo):
    """A folder line is exact. A case variant of it would stay in the release tree without a sign."""
    repo.pattern_file(NO_MATCH)
    blob = subprocess.run([*GIT, "hash-object", "-w", "--stdin"], cwd=repo.root, env=repo.env, input="clean\n", check=True, capture_output=True, text=True).stdout.strip()
    plant(repo, "100644", blob, "Docs/Journal/variant.md")
    assert blocked(repo) == {("error", 1)}


def test_a_submodule_is_an_error_for_the_checks_the_hash_and_the_written_tree_alike(repo):
    repo.pattern_file(NO_MATCH)
    head = subprocess.run([*GIT, "rev-parse", "HEAD"], cwd=repo.root, env=repo.env, check=True, capture_output=True, text=True).stdout.strip()
    plant(repo, "160000", head, "vendor/lib")
    assert blocked(repo) == {("error", 1)}


def test_a_keep_line_for_a_folder_and_paths_with_spaces(repo):
    repo.write(f"{JOURNAL}data/kept.csv", f"by {OWNER}\n")
    repo.write(f"{JOURNAL}data/deep/also kept.csv", f"by {OWNER}\n")
    repo.write("docs/old notes/a b.md", f"by {OWNER}\n")
    repo.write("docs/old notes/keep me.csv", f"by {OWNER}\n")
    repo.write("docs/old notes.md", f"by {OWNER}\n")
    exclude(repo, JOURNAL, f"!{JOURNAL}data/", "docs/old notes/", "! docs/old notes/keep me.csv")
    code, got, summary = repo.run()
    # Left out: seed.md and "a b.md". Kept: the data folder (2 files), "keep me.csv", and "old notes.md" (another path).
    assert code == 1 and got["S3"] == ("fail", 4) and " excluded=2 " in summary
    out = repo.tmp / "out"
    assert repo.raw("--write-release-tree", str(out)).returncode == 0
    assert (out / "docs/old notes/keep me.csv").is_file() and (out / JOURNAL / "data/deep/also kept.csv").is_file()
    assert not (out / "docs/old notes/a b.md").exists() and not (out / JOURNAL / "seed.md").exists()


def dangling(repo) -> dict[str, int]:
    r = repo.raw("--list-dangling")
    assert r.returncode == 0, r.stderr
    return {path: int(count) for count, path in (line.split("\t") for line in r.stdout.splitlines())}


def test_a_link_into_an_excluded_folder_fails_s21_and_a_link_to_a_kept_file_passes(repo):
    repo.write(f"{JOURNAL}old.md", "clean\n")
    repo.write(f"{JOURNAL}data.csv", "clean\n")
    exclude(repo, JOURNAL, f"!{JOURNAL}data.csv")
    # Links to a kept file, to a folder that still holds a file, to the web, to a path pattern and to a file outside the list.
    repo.write("docs/guide.md", f"[data](journal/data.csv) [folder](journal/) [web](https://example.com/{JOURNAL}old.md) [app](../src/app.py)\n")
    repo.write("notes.md", f"See `{JOURNAL}data.csv`, a new `{JOURNAL}YYYY-MM-DD-<slug>.md`, the folder `{JOURNAL}` and [top](#top).\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 0 and got["S21"] == ("pass", 0) and dangling(repo) == {}
    repo.write("docs/guide.md", "[old](journal/old.md#part) and again [old](./journal/old.md)\n")  # one path, two links
    repo.write("docs/deep/page.md", "[seed](../journal/seed.md) [old](/docs/journal/old.md)\n")
    repo.write("src/tool.py", f"# the plan is {JOURNAL}old.md.\n")  # a plain path in a file that is not Markdown
    repo.write("other.md", f"not a path: x{JOURNAL}old.md\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S21"] == ("fail", 3)
    assert dangling(repo) == {"docs/guide.md": 1, "docs/deep/page.md": 2, "src/tool.py": 1}


def test_s21_counts_a_link_to_a_folder_with_no_file_left_and_a_symbolic_link_and_never_the_list_itself(repo):
    repo.write("docs/index.md", "[journal](journal/)\n")
    (repo.root / "latest.md").symlink_to("docs/journal/seed.md")
    repo.commit()
    assert repo.run()[1]["S21"] == ("fail", 2) and dangling(repo) == {"docs/index.md": 1, "latest.md": 1}
    # A plain mention of the folder is not a link, and the list names its own lines.
    repo.write("docs/index.md", "The release tree leaves out `docs/journal/`.\n")
    (repo.root / "latest.md").unlink()
    repo.commit()
    assert repo.run()[1]["S21"] == ("pass", 0)


def test_s21_on_the_release_commit_counts_a_path_that_the_list_covers_and_the_commit_does_not_hold(repo):
    repo.write(f"{JOURNAL}data.csv", "clean\n")
    exclude(repo, JOURNAL, f"!{JOURNAL}data.csv")
    out = repo.tmp / "out"
    assert repo.raw("--write-release-tree", str(out)).returncode == 0
    released = Repo(out, repo.tmp)
    subprocess.run([*GIT, "init", "-q"], cwd=out, env=released.env, check=True, capture_output=True)
    released.write("docs/ok.md", f"[data](journal/data.csv) and a new `{JOURNAL}YYYY-MM-DD-<slug>.md` in `{JOURNAL}`\n")
    released.commit()
    assert released.run("--released")[1]["S21"] == ("pass", 0)
    released.write("docs/bad.md", "[seed](journal/seed.md)\n")
    released.write("src/tool.py", f"# see {JOURNAL}seed.md\n")
    released.commit()
    assert released.run("--released")[1]["S21"] == ("fail", 2)


def test_s21_on_the_release_commit_does_not_read_code_text_in_a_kept_data_file_as_a_link(repo):
    """A release rehearsal failed on a kept data table: `rows[i](raw)` read as a link to a left-out `raw`."""
    repo.write(f"{JOURNAL}data.csv", "call,rows[i](raw)\n")
    exclude(repo, JOURNAL, f"!{JOURNAL}data.csv")
    repo.commit()
    assert repo.run()[1]["S21"] == ("pass", 0)
    out = repo.tmp / "out"
    assert repo.raw("--write-release-tree", str(out)).returncode == 0
    released = Repo(out, repo.tmp)
    subprocess.run([*GIT, "init", "-q"], cwd=out, env=released.env, check=True, capture_output=True)
    released.commit()
    assert released.run("--released")[1]["S21"] == ("pass", 0)
    # A real link from a Markdown file in the same folder still counts.
    released.write(f"{JOURNAL}data.md", "[seed](seed.md)\n")
    exclude(released, JOURNAL, f"!{JOURNAL}data.csv", f"!{JOURNAL}data.md")
    released.commit()
    assert released.run("--released")[1]["S21"] == ("fail", 1)


def test_owner_mode_is_ready_when_every_machine_check_passes_and_every_review_is_signed(repo, gitleaks):
    """The one way to exit 0 in owner mode: guards, CLI, zone, the full first run and the signatures."""
    for name in ("setup.sh", "doctor.sh", "check.sh"):
        repo.script(f"scripts/{name}")
    repo.commit()
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    repo.sign()
    code, got, summary = repo.run("--release")
    assert code == 1 and got["S17"] == ("unmeasured", 0)
    assert {got[s] for s in ("S10", "S14", "S16", "S20", "S22")} == {("pass", 0)}
    code, got, summary = repo.run("--release", "--first-run")
    assert code == 0 and got["S17"] == ("pass", 0), summary
    assert " fail=0 error=0 unmeasured=0 unsigned=0 owner-only=0 release-day=3 " in summary and "result=ready" in summary


def lock(repo, *packages: tuple[str, str]) -> None:
    deps = ", ".join(f'{{ name = "{name}" }}' for name, _version in packages)
    root = UV_ROOT.replace('[{ name = "samplelib" }]', f"[{deps}]")
    repo.write("uv.lock", root + "".join(f'\n[[package]]\nname = "{name}"\nversion = "{version}"\n' for name, version in packages))
    repo.commit()


@pytest.mark.parametrize(
    ("licence", "count"),
    [
        ("License-Expression: GPL-3.0-only", 1),
        ("License-Expression: MIT AND GPL-3.0-only", 1),
        ("License-Expression: MIT OR GPL-3.0-only", 0),
        ("License-Expression: Apache-2.0 WITH LLVM-exception", 0),
        ("Classifier: License :: OSI Approved :: MIT License", 0),
        ("License: BSD", 1),  # which BSD licence? unknown until an allow line names it
        ("Classifier: License :: OSI Approved :: BSD License", 1),
        ("License: read the file named COPYING", 1),  # not a licence name: unknown
        ("Summary: no licence field at all", 1),
    ],
)
def test_a_python_dependency_fails_s10_unless_its_licence_is_known_and_accepted(repo, licence, count):
    repo.installed("otherlib", licence, "2.0")
    lock(repo, ("samplelib", "1.0"), ("otherlib", "2.0"))
    code, got, _ = repo.run()
    assert got["S10"] == (("fail", count) if count else ("pass", 0)) and code == (1 if count else 0)


def test_s10_reads_runtime_packages_only_and_needs_the_installed_version_of_the_lock(repo):
    # A package that only a development tool needs is in the lock but not under the project.
    repo.write("uv.lock", UV_LOCK + '\n[[package]]\nname = "devtool"\nversion = "9.0"\n')
    repo.commit()
    assert repo.run()[1]["S10"] == ("pass", 0)
    # A dependency of a dependency counts.
    repo.write("uv.lock", UV_LOCK + 'dependencies = [{ name = "Deep_Lib" }]\n\n[[package]]\nname = "deep-lib"\nversion = "3.0"\n')
    repo.commit()
    assert repo.run()[1]["S10"] == ("fail", 1)  # not installed: unknown
    repo.installed("deep.lib", "License-Expression: ISC", "3.1")
    assert repo.run()[1]["S10"] == ("fail", 1)  # another version is installed: still unknown
    repo.installed("deep.lib", "License-Expression: ISC", "3.0")
    assert repo.run()[1]["S10"] == ("pass", 0)


def test_an_s10_allow_line_needs_a_licence_a_reason_and_a_dependency_that_needs_it(repo):
    repo.installed("otherlib", "License-Expression: MPL-2.0", "2.0")
    lock(repo, ("samplelib", "1.0"), ("otherlib", "2.0"))
    assert repo.run()[1]["S10"] == ("fail", 1)
    repo.allow("S10 python:otherlib MPL-2.0 used as an unchanged dependency")
    code, got, _ = repo.run()
    assert code == 0 and got["S10"] == ("pass", 0) and got["S3"] == ("pass", 0)
    for line in ("S10 python:otherlib", "S10 python:otherlib MPL-2.0"):
        repo.allow(line)
        code, got, _ = repo.run()
        assert code == 1 and got["S10"] == ("error", 0)
    repo.allow("S10 python:otherlib MPL-2.0 a reason", "S10 python:samplelib MIT its licence is accepted, so this line is stale")
    assert repo.run()[1]["S10"] == ("error", 1)


def test_an_allowed_dependency_fails_s10_again_when_its_licence_is_not_the_recorded_one(repo):
    repo.installed("otherlib", "Classifier: License :: OSI Approved :: Mozilla Public License 2.0 (MPL 2.0)", "2.0")
    lock(repo, ("samplelib", "1.0"), ("otherlib", "2.0"))
    repo.allow("S10 python:otherlib MPL-2.0 used as an unchanged dependency")
    assert repo.run()[1]["S10"] == ("pass", 0)
    repo.installed("otherlib", "License-Expression: AGPL-3.0-only", "2.0")  # the same package under another licence
    code, got, _ = repo.run()
    assert code == 1 and got["S10"] == ("fail", 1)
    repo.installed("otherlib", "Summary: the licence field is gone", "2.0")
    assert repo.run()[1]["S10"] == ("fail", 1)
    repo.allow("S10 python:otherlib unknown the owner read its licence file")
    assert repo.run()[1]["S10"] == ("pass", 0)


def test_s10_fails_closed_on_a_missing_or_empty_lock_and_on_a_missing_own_licence(repo):
    (repo.root / "LICENSE").unlink()
    repo.write("pyproject.toml", PYPROJECT.replace('license = "MIT"\n', ""))
    repo.commit()
    assert repo.run()[1]["S10"] == ("fail", 2)
    repo.write("uv.lock", UV_ROOT.replace('[{ name = "samplelib" }]', "[]"))
    repo.commit()
    assert repo.run()[1]["S10"] == ("error", 0)  # no dependency read: an empty input never passes
    repo.write("uv.lock", "not a lock file [")
    repo.commit()
    assert repo.run()[1]["S10"] == ("error", 0)
    (repo.root / "uv.lock").unlink()
    repo.commit()
    code, got, summary = repo.run()
    assert code == 1 and got["S10"] == ("error", 1) and "result=not-ready" in summary


def test_s10_reads_the_rust_lock_from_the_local_registry_and_the_web_lock_without_its_dev_packages(repo):
    repo.write("Cargo.toml", '[workspace]\nmembers = ["tool"]\n')
    repo.write("tool/Cargo.toml", '[package]\nname = "tool"\nversion = "0.1.0"\nlicense = "MIT"\n')
    crates = '[[package]]\nname = "tool"\nversion = "0.1.0"\n\n[[package]]\nname = "okcrate"\nversion = "1.2.3"\nsource = "registry+https://example.invalid/index"\n'
    repo.write("Cargo.lock", crates)
    repo.write("web/package.json", '{"name": "web", "license": "MIT"}')
    packages = {"": {"name": "web"}, "node_modules/oklib": {"version": "1.0.0", "license": "ISC"}, "node_modules/devlib": {"version": "1.0.0", "dev": True, "license": "GPL-3.0"}}
    repo.write("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": packages}))
    repo.commit()
    assert repo.run()[1]["S10"] == ("fail", 1)  # the crate is not in the registry: unknown
    manifest = repo.tmp / "cargo" / "registry" / "src" / "index-1" / "okcrate-1.2.3" / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text('[package]\nname = "okcrate"\nversion = "1.2.3"\nlicense = "MIT/Apache-2.0"\n')
    assert repo.run()[1]["S10"] == ("pass", 0)
    manifest.write_text('[package]\nname = "okcrate"\nversion = "1.2.3"\nlicense = "AGPL-3.0-only"\n')
    assert repo.run()[1]["S10"] == ("fail", 1)
    manifest.write_text('[package]\nname = "okcrate"\nversion = "1.2.3"\nlicense = "MIT"\n')
    packages["node_modules/@scope/badlib"] = {"version": "1.0.0", "license": "GPL-3.0"}
    packages["node_modules/nolicence"] = {"version": "1.0.0"}
    repo.write("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": packages}))
    repo.write("tool/Cargo.toml", '[package]\nname = "tool"\nversion = "0.1.0"\n')  # a crate that names no licence
    repo.commit()
    assert repo.run()[1]["S10"] == ("fail", 3)
    # A web allow line names the version: the same name at another version counts again.
    repo.allow("S10 web:@scope/badlib@1.0.0 GPL-3.0 a reason", "S10 web:nolicence@1.0.0 unknown a reason")
    assert repo.run()[1]["S10"] == ("fail", 1)
    packages["node_modules/@scope/badlib"]["version"] = "1.0.1"
    repo.write("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": packages}))
    repo.commit()
    assert repo.run()[1]["S10"] == ("error", 1)  # the line for 1.0.0 covers nothing now


@pytest.mark.parametrize(
    ("path", "content"),
    [
        ("web/package-lock.json", json.dumps({"lockfileVersion": 1, "dependencies": {"oklib": {"version": "1.0.0"}}})),  # no `packages`
        ("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": {"": {"name": "web"}}})),
        ("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": {"node_modules/oklib": {"license": "ISC"}}})),  # no version
        ("Cargo.lock", '[[package]]\nname = "tool"\nversion = "0.1.0"\n'),  # the workspace crate only
    ],
)
def test_a_lock_that_names_no_package_is_an_error_for_s10_even_when_another_lock_is_fine(repo, path, content):
    repo.write("web/package.json", '{"name": "web", "license": "MIT"}')
    repo.write("web/package-lock.json", json.dumps({"lockfileVersion": 3, "packages": {"node_modules/oklib": {"version": "1.0.0", "license": "ISC"}}}))
    repo.write("Cargo.toml", '[package]\nname = "tool"\nversion = "0.1.0"\nlicense = "MIT"\n')
    repo.write("Cargo.lock", '[[package]]\nname = "okcrate"\nversion = "1.2.3"\nsource = "registry+https://example.invalid/index"\n')
    manifest = repo.tmp / "cargo" / "registry" / "src" / "index-1" / "okcrate-1.2.3" / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text('[package]\nname = "okcrate"\nversion = "1.2.3"\nlicense = "MIT"\n')
    repo.commit()
    assert repo.run()[1]["S10"] == ("pass", 0)
    repo.write(path, content)
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S10"] == ("error", 0)


def test_s10_follows_the_extras_that_a_dependency_asks_for(repo):
    extras = '\n[package.optional-dependencies]\nfast = [{ name = "speedlib" }]\nunused = [{ name = "otherlib" }]\n'
    packages = '\n[[package]]\nname = "speedlib"\nversion = "4.0"\n\n[[package]]\nname = "otherlib"\nversion = "5.0"\n'
    repo.write("uv.lock", UV_LOCK.replace('[{ name = "samplelib" }]', '[{ name = "samplelib", extra = ["fast"] }]') + extras + packages)
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S10"] == ("fail", 1)  # speedlib is not installed: unknown. otherlib is not asked for.
    repo.installed("speedlib", "License-Expression: GPL-3.0-only", "4.0")
    assert repo.run()[1]["S10"] == ("fail", 1)
    repo.installed("speedlib", "License-Expression: MIT", "4.0")
    assert repo.run()[1]["S10"] == ("pass", 0)


def test_s14_needs_the_text_guard_the_push_list_entry_and_a_pre_push_hook(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    assert repo.run("--release")[1]["S14"] == ("pass", 0)
    assert repo.run()[1]["S14"] == ("owner-only", 0)
    tools = repo.tmp / "dotfiles"
    (tools / "config" / "public-push-remotes.txt").write_text(f"github.com/{HANDLE}/another\n# github.com/{HANDLE}/samplebrew\n")
    assert repo.run("--release")[1]["S14"] == ("fail", 1)
    (tools / "bin" / "gh").write_text(PASSING)  # a plain gh with no guard
    assert repo.run("--release")[1]["S14"] == ("fail", 2)
    hook = repo.tmp / "hooks" / "pre-push"
    hook.write_text(PASSING)  # another pre-push hook, without the leak gate
    assert repo.run("--release")[1]["S14"] == ("fail", 3)
    hook.write_text(PASSING + f"# {script.HOOK_MARK.decode()}\n")
    assert repo.run("--release")[1]["S14"] == ("fail", 2)
    hook.chmod(0o644)
    assert repo.run("--release")[1]["S14"] == ("fail", 3)
    hook.unlink()
    assert repo.run("--release")[1]["S14"] == ("fail", 3)


def test_s14_is_an_error_without_gh_or_without_a_link_to_the_repository(repo, tmp_path):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    repo.write("README.md", f"By {OWNER}.\n")
    repo.commit()
    assert repo.run("--release")[1]["S14"] == ("error", 0)
    if shutil.which("gh", path="/usr/bin:/bin"):
        if os.environ.get("PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GH") == "1":
            pytest.skip("gh is in a system folder and PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GH=1 is set")
        pytest.fail("gh is in a system folder, so no PATH here lacks it and the missing-gh case is untested. Set PUBLIC_READINESS_TESTS_ALLOW_SYSTEM_GH=1 to skip on purpose.")
    repo.git("checkout", "-q", "HEAD~1", "--", "README.md")
    repo.commit()
    tools = tmp_path / "tools"
    tools.mkdir()
    (tools / "python3").symlink_to(sys.executable)
    for tool in ("gitleaks",):
        if shutil.which(tool):
            (tools / tool).symlink_to(shutil.which(tool))
    repo.env["PATH"] = f"{tools}:/usr/bin:/bin"
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S14"] == ("error", 0)


def test_a_plugin_name_in_text_or_in_a_file_name_fails_s16(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    assert repo.run("--release")[1]["S16"] == ("pass", 0)
    repo.write("docs/setup.md", f"Install {PLUGIN.upper()} first.\n")
    repo.write(f"tests/{PLUGIN}/case.txt", "clean\n")
    repo.write("docs/other.md", f"{PLUGIN}x is another word\n")
    repo.commit()
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S16"] == ("fail", 2)
    assert repo.run()[1]["S16"] == ("owner-only", 0)


def test_the_company_word_of_a_plugin_name_fails_s16_alone_and_a_generic_word_does_not(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    company = PLUGIN.split("-")[1]
    repo.planted.append(company)
    repo.cli(f"echo '{PLUGIN}   1.0   provides=work exec=False'\necho 'careerbrew_corporate-plugin-abc 1.0'\necho 'effective policy: {{}}'\n")
    # The project name, generic words and short words of a name are not searched alone.
    repo.write("docs/setup.md", "samplebrew has a plugin for corporate work. abc.\n")
    repo.commit()
    assert repo.run("--release")[1]["S16"] == ("pass", 0)
    repo.write("docs/setup.md", f"Ask the {company.title()} help desk.\n")
    repo.write(f"tests/{company}_case.txt", "clean\n")
    repo.write("docs/other.md", f"{company}s is another word\n")
    repo.commit()
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S16"] == ("fail", 2)


def test_s16_and_s20_do_not_trust_a_data_home_from_the_callers_shell_without_the_flag(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    default = repo.raw("--release").stdout
    assert default.count("home=default") == 2 and "home=overridden" not in default
    repo.env["CAREERBREW_HOME"] = str(repo.tmp / "other-data")
    code, got, summary = repo.run("--release")
    assert code == 1 and got["S16"] == ("unmeasured", 0) and got["S20"] == ("unmeasured", 0)
    code, got, _ = repo.run("--release", "--allow-home-override")
    assert got["S16"] == ("pass", 0) and got["S20"] == ("pass", 0)
    assert repo.raw("--release", "--allow-home-override").stdout.count("home=overridden") == 2
    assert str(repo.tmp) not in repo.raw("--release").stdout


EMPTY_LIST = "echo 'no plugins installed'; echo 'effective policy: {}'"


def test_an_empty_plugin_list_passes_s16_only_in_a_data_folder_with_a_profile(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    # The owner's data folder: the list is empty and the profile is there.
    repo.cli(f'[ "$1" = plugin ] && {{ {EMPTY_LIST}; exit 0; }}\n[ "$1 $2 $3 $4" = "data show profile.yaml --keys" ] && echo "name: str"\n')
    assert repo.run("--release")[1]["S16"] == ("pass", 0)
    # A worktree with no data: the list is empty because nothing is there. That blocks owner mode.
    repo.cli(f'[ "$1" = plugin ] && {{ {EMPTY_LIST}; exit 0; }}\necho "no such data file" >&2\nexit 1\n')
    code, got, summary = repo.run("--release")
    assert code == 1 and got["S16"] == ("unmeasured", 0) and "result=not-ready" in summary


def test_s16_is_an_error_when_the_plugin_list_cannot_be_read(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    repo.cli("echo 'ab 1.0 provides= exec=False'\n")
    assert repo.run("--release")[1]["S16"] == ("error", 0)
    repo.cli("exit 3\n")
    assert repo.run("--release")[1]["S16"] == ("error", 0)
    repo.cli(None)
    assert repo.run("--release")[1]["S16"] == ("error", 0)


@pytest.mark.parametrize(
    ("path", "text", "count"),
    [
        ("src/clock.py", f'ZONE = "{ZONE}"\n', 1),
        ("scripts/jobs.py", f"where = '{CITY.lower()}'\n", 1),
        ("profile/examples/criteria.yaml", f"locations: [Remote, {COUNTRY}]\n", 1),
        ("web/src/view.tsx", f"const key = '{COUNTRY.lower().replace(' ', '_')}_presence';\n", 1),
        ("README.md", f"Install from https://github.com/{HANDLE}/samplebrew. By {OWNER} <{EMAIL}>. Made in {CITY}.\n", 1),
        ("tests/unit/test_zone.py", f'ZONE = "{ZONE}"\n', 1),  # every folder counts
        ("docs/notes.md", f"met in {CITY}\n", 1),
        (f"tests/fixtures/{CITY.lower()}-office/case.txt", "clean\n", 1),  # a file name
        ("docs/notes.md", "also called Aotearoa\n", 1),  # another form of the country
        ("docs/notes.md", f"team {CODE3} plays\n", 1),  # three letters: capitals only
        ("docs/notes.md", f"the {CODE3.lower()} tool and the {CODE3.title()}x tool\n", 0),
        ("docs/notes.md", f"based in Wellington, {CODE2}\n", 1),  # two letters: only where the text speaks of a place
        ("profile/examples/criteria.yaml", f'country: "{CODE2}"\n', 1),
        ("web/src/view.tsx", f"const tag = 'en-{CODE2}';\n", 1),
        ("src/words.py", f"{CODE2} = 1  # the {CODE2} flag, also {CODE2.lower()}\n", 0),
        ("src/words.py", f"x = '{CITY}er'\n", 0),  # another word
        ("docs/notes.md", f"MET IN {CITY.upper()}\n", 1),  # any letter case
        ("docs/notes.md", f"zone {ZONE.swapcase()}\n", 1),
        ("src/words.py", f"tag = '{COUNTRY.lower().replace(' ', '')}'\n", 1),  # two words joined
        ("src/words.py", f"TAG = '{COUNTRY.upper().replace(' ', '-')}'\n", 1),
        (f"docs/{COUNTRY.replace(' ', '')}/notes.md", "clean\n", 1),
    ],
)
def test_the_owners_zone_city_or_country_in_any_file_fails_s20(repo, path, text, count):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    repo.write(path, text)
    repo.commit()
    assert repo.run("--release")[1]["S20"] == (("fail", count) if count else ("pass", 0))
    assert repo.run()[1]["S20"] == ("owner-only", 0)


@pytest.mark.parametrize("zone", ["UTC", "Nowhere/Atlantis", "not a zone"])
def test_a_zone_that_names_no_known_place_is_an_error_for_s20(repo, zone):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    repo.env["PUBLIC_READINESS_ZONE"] = zone
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S20"] == ("error", 0)


def test_s20_also_searches_the_place_terms_that_the_owner_gives_for_this_run(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    town, region = "Zorgville", "Quux Valley"
    repo.planted += [town, region]
    repo.write("docs/notes.md", f"the office in {town.upper()}\n")
    repo.write("src/app.py", f"AREA = '{region.lower().replace(' ', '_')}'\n")
    repo.commit()
    assert repo.run("--release")[1]["S20"] == ("pass", 0)
    repo.env["PUBLIC_READINESS_PLACE_TERMS"] = f"{town}, {region}"
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S20"] == ("fail", 2)
    assert repo.run()[1]["S20"] == ("owner-only", 0)
    repo.env["PUBLIC_READINESS_PLACE_TERMS"] = f"{town},ab"
    assert repo.run("--release")[1]["S20"] == ("error", 0)


def test_an_owner_place_term_matches_in_any_letter_case_with_other_alphabets_and_joined(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    town, region, short = "Z\u00fcrgstra\u00dfe", "\u00c5ker Valley", "Zog"
    repo.planted += [town, region]
    repo.write("docs/a.md", f"the office in {town.upper()}\n")  # the capital form of the last letters is two letters
    repo.write("docs/b.md", f"area {region.lower().replace(' ', '')}\n")
    repo.write("docs/c.md", f"near {short.lower()} and {short.upper()}\n")  # the owner's own short term, in any case
    repo.write("docs/d.md", f"the {town}n list and the {short}s\n")  # other words
    repo.commit()
    repo.env["PUBLIC_READINESS_PLACE_TERMS"] = f"{town}, {region}, {short}"
    code, got, _ = repo.run("--release")
    assert code == 1 and got["S20"] == ("fail", 3)


def test_the_s20_note_says_where_the_zone_came_from(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    line = next(ln for ln in repo.raw("--release").stdout.splitlines() if ln.startswith("S20 "))
    assert "zone=env" in line and ZONE not in line
    del repo.env["PUBLIC_READINESS_ZONE"]
    repo.cli(f"echo 'timezone      {ZONE}      file      IANA time zone'\n")
    line = next(ln for ln in repo.raw("--release").stdout.splitlines() if ln.startswith("S20 "))
    assert "zone=machine" in line and ZONE not in line


def test_s20_reads_the_zone_from_the_timezone_setting_of_the_cli(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    del repo.env["PUBLIC_READINESS_ZONE"]
    repo.cli(f"echo 'timezone      {ZONE}      file      IANA time zone'\n")
    repo.write("src/clock.py", f"city = '{CITY}'\n")
    repo.commit()
    assert repo.run("--release")[1]["S20"] == ("fail", 1)


def test_a_failing_self_check_fails_s17_and_a_missing_one_is_an_error(repo):
    # The check runs in a copy of the release tree with a scratch HOME, not in the checkout.
    probe = f'[ ! -e docs/journal ] && [ -e src/app.py ] && [ "$HOME" != "{repo.env["HOME"]}" ] && [ -d "$HOME" ] && git rev-parse --git-dir >/dev/null\n'
    repo.script("scripts/check-docs.sh", "#!/bin/sh\n" + probe)
    repo.commit()
    code, got, _ = repo.run()
    assert code == 0 and got["S17"] == ("unmeasured", 0)
    repo.script("scripts/check-docs.sh", "#!/bin/sh\nexit 1\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S17"] == ("fail", 1)
    (repo.root / "scripts" / "check-docs.sh").unlink()
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S17"] == ("error", 1)


def test_s17_gives_its_steps_scratch_settings_folders_and_kills_every_process_of_a_step_that_runs_too_long(repo):
    repo.env["XDG_CONFIG_HOME"] = str(repo.tmp / "callers-config")
    repo.env["XDG_RUNTIME_DIR"] = str(repo.tmp / "callers-runtime")
    probe = '[ "$XDG_CONFIG_HOME" = "$HOME/.config" ] && [ "$XDG_CACHE_HOME" = "$HOME/.cache" ] && [ "$XDG_DATA_HOME" = "$HOME/.local/share" ] && [ "$XDG_STATE_HOME" = "$HOME/.local/state" ] && [ -z "${XDG_RUNTIME_DIR:-}" ]\n'
    repo.script("scripts/check-docs.sh", "#!/bin/sh\n" + probe)
    repo.commit()
    assert repo.run()[1]["S17"] == ("unmeasured", 0)
    # A step that leaves a job behind and never ends: the job must die with the step.
    mark = repo.tmp / "left-behind"
    repo.script("scripts/check-docs.sh", f"#!/bin/sh\n( sleep 3; touch '{mark}' ) &\nsleep 60\n")
    repo.commit()
    repo.env["PUBLIC_READINESS_STEP_SECONDS"] = "1"
    code, got, _ = repo.run()
    assert code == 1 and got["S17"] == ("fail", 1)
    time.sleep(4)
    assert not mark.exists()
    # A step that ends in time but leaves a job behind: the job dies too.
    repo.script("scripts/check-docs.sh", f"#!/bin/sh\n( sleep 2; touch '{mark}' ) &\nexit 0\n")
    repo.commit()
    assert repo.run()[1]["S17"] == ("unmeasured", 0)
    time.sleep(3)
    assert not mark.exists()
    repo.env["PUBLIC_READINESS_STEP_SECONDS"] = "soon"
    assert repo.run()[1]["S17"] == ("error", 0)


def test_the_full_first_run_counts_each_failing_command_and_needs_all_of_them_in_the_tree(repo):
    assert repo.run("--first-run")[1]["S17"] == ("error", 3)
    repo.script("scripts/setup.sh")
    repo.script("scripts/doctor.sh", "#!/bin/sh\nexit 1\n")
    repo.script("scripts/check.sh", '#!/bin/sh\n[ "$1" = fast ]\n')
    repo.commit()
    assert repo.run("--first-run")[1]["S17"] == ("fail", 1)
    repo.script("scripts/doctor.sh")
    repo.commit()
    code, got, _ = repo.run("--first-run")
    assert code == 0 and got["S17"] == ("pass", 0)


def test_the_s17_scratch_copy_has_one_commit_by_a_scratch_identity_like_a_real_clone(repo):
    # A real clone always has a commit, and the fast gate's leak scan needs one.
    probe = 'git rev-parse --verify -q HEAD >/dev/null && [ "$(git rev-list --count HEAD)" = 1 ] && [ "$(git log -1 --format=%an/%ae)" = release-check/release-check@example.com ] && [ -z "$(git status --porcelain)" ]\n'
    repo.script("scripts/check-docs.sh", "#!/bin/sh\n" + probe)
    repo.commit()
    code, got, _ = repo.run()
    assert code == 0 and got["S17"] == ("unmeasured", 0)


def test_the_s17_note_names_each_failing_command_by_place_and_script_and_never_its_output(repo):
    said = "words-a-step-printed"
    repo.script("scripts/setup.sh")
    repo.script("scripts/doctor.sh", f"#!/bin/sh\necho {said}\necho {said} >&2\nexit 1\n")
    repo.script("scripts/check.sh", "#!/bin/sh\nexit 1\n")
    repo.commit()
    r = repo.raw("--first-run")
    line = next(ln for ln in r.stdout.splitlines() if ln.startswith("S17 "))
    assert line.split()[2:4] == ["fail", "2"] and line.endswith("(commands=4 failed=3:doctor.sh,4:check.sh)"), line
    assert said not in r.stdout + r.stderr
    repo.script("scripts/doctor.sh")
    repo.script("scripts/check.sh")
    repo.script("scripts/check-docs.sh", "#!/bin/sh\nexit 1\n")
    repo.commit()
    line = next(ln for ln in repo.raw().stdout.splitlines() if ln.startswith("S17 "))
    assert line.split()[2:4] == ["fail", "1"] and line.endswith("(commands=1 failed=1:check-docs.sh)"), line


LOCAL = "loc" + "al"
LOADER = "launch" + "ctl"
DOT = "."


def ip(*parts: int) -> str:
    """An address joined at run time, so S22 does not count this file on the real tree."""
    return ".".join(str(part) for part in parts)


def joined(mark: str, *parts: str) -> str:
    return mark.join(parts)


# Each text is joined at run time for the same reason.
IDENTIFIERS = [
    f"ssh zed-macbook-pro.{LOCAL}",
    f"open http://zed-macbook-pro.{LOCAL}:8765/today",  # inside a URL, before a port
    f"scp notes.txt zed@zed-macbook-pro.{LOCAL}:",  # after `@`
    f"The server is zed-macbook-pro.{LOCAL}.",  # before the final dot of a sentence
    f"curl zed-macbook-pro.{LOCAL}/api/today",  # before a path
    f"ZED-MACBOOK-PRO.{LOCAL.upper()} in capitals",
    "printer at " + joined(DOT, "printer", "l" + "an"),
    "db at " + joined(DOT, "db", "corp", "inter" + "nal"),
    "node " + joined(DOT, "zedbox", "tail9f3a2", "ts", "net"),
    "router " + joined(DOT, "router", "home", "arpa"),
    f"host {ip(192, 168, 4, 27)} is the printer",
    f"peer {ip(10, 20, 30, 40)}",
    f"gateway {ip(172, 20, 1, 9)}",
    f"tailnet {ip(100, 101, 102, 103)}",  # carrier-grade NAT, 100.64/10
    f"self-assigned {ip(169, 254, 17, 3)}",  # link-local
    "link " + joined(":", "fe80", "", "1c2d", "3e4f"),
    "home " + joined(":", "2607", "f8b0", "4005", "80a", "", "200e"),
    "unique " + joined(":", "fd12", "3456", "789a", "1", "", "1"),
    "card " + joined(":", "a4", "83", "e7", "1b", "2c", "3d"),
    "card " + joined("-", "A4", "83", "E7", "1B", "2C", "3D"),
    "device " + joined("-", "0f8fad5b", "d9cb", "469f", "a165", "70867728950e"),
    f"{LOADER} load com." + f"{HANDLE[:3]}corp.sample.watch",
    f"{LOADER} load " + joined(DOT, "homebrew", HANDLE[:3], "watch"),  # a label with another first word
    "see ~/Library/LaunchAgents/" + joined(DOT, "zq", "sample", "watch", "plist"),
    "posthog project " + "482913",
    "https://eu.example.invalid/dashboard/" + "7719024",
    "account_id: " + '"30881207"',
    "key ph" + "c_" + "Zk3mQ9xT7vB2nL5wR8yD",
    "tag G-" + "ZK3MQ9XT7V",
    "call +1 416 " + "867 5309",
    "call (416) " + "867-5309",
    "see https://www.notion.so/Plan-" + "0a1b2c3d4e5f60718293a4b5c6d7e8f9",
    "The link is " + joined(":", "fe80", "", "1c2d", "3e4f") + ".",  # before the final dot of a sentence
    "inet6:" + joined(":", "fe80", "", "1c2d", "3e4f") + " scope",  # after a colon
    "nat " + joined(":", "64", "ff9b", "", "8.8.4.4"),  # with an IPv4 tail
    "ether:" + joined(":", "a4", "83", "e7", "1b", "2c", "3d"),
    "card " + joined(DOT, "a483", "e71b", "2c3d"),  # the dotted form
    "hwaddr=" + "a483" + "e71b2c3d",  # bare, after a word for it
    'mac_address: "' + "A483" + 'E71B2C3D"',
    "peer " + joined(DOT, "010", "20", "30", "40"),  # zero-padded octets
    "gateway " + joined(DOT, "172", "016", "1", "9"),
    "tailnet " + joined(DOT, "100", "064", "1", "1"),
    "job com." + "Zed_Corp.Watch_1",  # capitals and underscores
    *("job " + joined(DOT, first, "zedcorp", "watch") for first in ("app", "co", "ai", "uk")),
    f"ssh my-example.{LOCAL}",  # `example` is only a part of the label
    f"ssh notexample.{LOCAL}",
    *("job com." + joined(DOT, "zzname", "sync", last) for last in ("log", "err", "out")),  # a log file of a job
    "job com." + "zzname.dev",  # the last word is a domain ending
    "job io." + "zzname.net",
    "run com." + "zzname.sync(now)",  # before a bracket
    f"{LOADER} load " + joined(DOT, "homebrew", "zz", "watch", "out"),
    f"cert for *.zzbox.{LOCAL}",  # after a dot
    "cert for " + joined(DOT, "*", "zznet-1234", "ts", "net"),
    "flag -zzbox." + "l" + "an",  # after a hyphen
    *(f"see zzbox.{LOCAL}{after}" for after in ("-2", "_key", "(1)", ".1")),
    "card " + joined(":", "0", "1c", "b3", "9", "85", "15"),  # without leading zeros
    "MAC (en0): " + "a483" + "e71b2c3d",
    "mac " + joined(DOT, "1234", "5678", "9012"),  # digits only, after the word
    "link " + joined(":", "fe80", "", "1c2d", "3e4f") + "_tag",
    "link " + joined(":", "fe80", "", "1c2d", "3e4f") + "(en0)",
    "host " + joined(DOT, "0192", "168", "4", "27"),
    "see https://docs.google.com/document/d/" + "1aZ9_kQ7mX2vB5nL8wR3yD6tG0hJ4pS-uEcFi" + "/edit",
]
NOT_IDENTIFIERS = [
    f"lock = threading.{LOCAL}()",
    f"ssh mac.example.{LOCAL} and http://app.example.{LOCAL}:8765/",
    f"see settings.{LOCAL}.json and ~/.{LOCAL}/bin and .env.{LOCAL}",
    "listen on 127.0.0.1 or 192.0.2.7 or 203.0.113.9",
    f"public {ip(100, 63, 255, 1)} and {ip(100, 128, 0, 1)} and {ip(169, 253, 1, 1)}",  # next to the ranges, not in them
    "version 10.2.3 and 1.10.0.12.5",
    "listen on [" + joined(":", "", "", "1") + "] or " + joined(":", "2001", "db8", "", "1") + " or " + joined(":", "3fff", "", "7"),  # loopback and the documentation ranges
    "at 12:30:45 the ratio 16:9:1 and std::fmt and f64::add(x) and dead::beef",
    "card " + joined(":", *["00"] * 6) + " or " + joined(":", *["ff"] * 6) + " or " + joined(":", "00", "00", "5e", "00", "53", "2a"),
    "id " + joined("-", "0" * 8, "0" * 4, "0" * 4, "0" * 4, "0" * 11 + "1") + " or " + joined("-", "a" * 8, "b" * 4, "c" * 4, "d" * 4, "e" * 12),
    "id " + joined("-", "123e4567", "e89b", "12d3", "a456", "426614174000"),  # the example of the UUID standard
    "bundle org.example.com and com.example.app.watch",
    f"{LOADER} list | grep x; os.path.join(a, b); see docs.github.com and settings.{LOCAL}.json and scripts/job.sample.sh",
    "a.b.c is no label away from a launch line",
    "call 415-555-0100 or +1 415 555 0142",
    "sizes 375 768 1280",
    "see https://docs.google.com/document/d/doc-prd-1/edit and https://docs.google.com/spreadsheets/d/<id>/edit",
    "limit 2147483647 and stamp 1792407600",
    'uid "User":"65534"',
    "commit " + "a483" + "e71b2c3d and digest " + "A483" + "E71B2C3D",  # twelve hex digits with no word for a card
    "words " + joined(DOT, "dead", "beef", "cafe") + " and version " + joined(DOT, "1234", "5678", "9012"),
    "open app.spec.ts and co.example.watch and com.zz.log",
    f"zone = calendar.{LOCAL}_tz() and settings.{LOCAL}.json and .env.{LOCAL}.example",
    "ratio 1:2:3:4:5:6 and f64::add(x)",
    f"{LOADER} list; pytest.mark.parametrize(x)",
    "mapped " + joined(":", "", "", "ffff", "192.0.2.9") + " and the range " + joined(":", "2001", "db8", "", "1") + ".",
    "version 010.2.3 and 172.015.1.9",
]


@pytest.mark.parametrize("text", IDENTIFIERS)
def test_a_machine_or_account_identifier_fails_s22(repo, text):
    repo.planted.append(text)
    repo.write("docs/notes.md", f"{text}\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S22"] == ("fail", 1)


def test_text_that_only_looks_like_an_identifier_passes_s22(repo):
    repo.write("docs/notes.md", "".join(f"{text}\n" for text in NOT_IDENTIFIERS))
    repo.commit()
    code, got, _ = repo.run()
    assert code == 0 and got["S22"] == ("pass", 0)


def test_an_s22_allow_line_covers_one_exact_text_in_one_file_and_must_match_something(repo):
    base, host, absent = ip(10, 0, 0, 0), ip(10, 9, 8, 7), ip(10, 5, 5, 5)
    repo.write("docs/cards.md", f"A private range starts at {base}, for example {host}.\n")
    repo.commit()
    assert repo.run()[1]["S22"] == ("fail", 2)
    repo.allow(f"S22 docs/cards.md ip:{base} the base address of a private range")
    assert repo.run()[1]["S22"] == ("fail", 1)
    # A line with no reason, and a line of the old shape with no file.
    for line in (f"S22 docs/cards.md ip:{host}", f"S22 ip:{host} a reason"):
        repo.allow(f"S22 docs/cards.md ip:{base} the base address of a private range", line)
        assert repo.run()[1]["S22"] == ("error", 0)
    both = (f"S22 docs/cards.md ip:{base} a reason", f"S22 docs/cards.md ip:{host} a reason")
    repo.allow(*both, f"S22 docs/cards.md ip:{absent} a reason")
    code, got, _ = repo.run()
    assert code == 1 and got["S22"] == ("error", 1)
    repo.allow(*both, f"S22 docs/other.md ip:{host} a reason")  # the text is there, but not in that file
    assert repo.run()[1]["S22"] == ("error", 1)
    repo.allow(*both)
    code, got, _ = repo.run()
    assert code == 0 and got["S22"] == ("pass", 0) and got["S3"] == ("pass", 0)
    # The same text in another file is not covered.
    repo.write("src/config.py", f"HOST = '{host}'\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S22"] == ("fail", 1)
    # The hash of the text stands for the text, for a text that a secret scanner would flag in the allowlist.
    repo.allow(*both, f"S22 src/config.py ip:sha256:{hashlib.sha256(host.encode()).hexdigest()} a reason")
    assert repo.run()[1]["S22"] == ("pass", 0)
    repo.allow(*both, f"S22 src/config.py ip:sha256:{hashlib.sha256(absent.encode()).hexdigest()} a reason")
    assert repo.run()[1]["S22"] == ("error", 1)


@pytest.mark.parametrize("name", [f"docs/hosts/zed-macbook-pro.{LOCAL}/notes.md", "docs/" + ip(192, 168, 4, 27) + "/notes.md", "docs/" + joined("-", "0f8fad5b", "d9cb", "469f", "a165", "70867728950e") + ".json"])
def test_an_identifier_in_a_file_name_fails_s22(repo, name):
    repo.planted.append(name)
    repo.write(name, "clean\n")
    repo.commit()
    code, got, _ = repo.run()
    assert code == 1 and got["S22"] == ("fail", 1)


def test_s22_reads_the_allowlist_except_the_one_allowed_word_of_each_s22_line(repo):
    base, host = ip(10, 0, 0, 0), ip(10, 9, 8, 7)
    name = f"zed-macbook-pro.{LOCAL}"
    repo.planted += [host, name]
    repo.write("docs/cards.md", f"A private range starts at {base}.\n")
    good = f"S22 docs/cards.md ip:{base} the base address of a private range"
    repo.allow(good)
    assert repo.run()[1]["S22"] == ("pass", 0)
    # A comment, a reason and an S3 path line are read like any other text.
    for line in (f"# my box is {name}", f"S22 docs/cards.md ip:{base} the same as {host}", f"docs/{host}.md"):
        repo.allow(good if line.startswith("#") or not line.startswith("S22") else "# nothing", line)
        code, got, _ = repo.run()
        assert code == 1 and got["S22"] == ("fail", 1), line


@pytest.mark.parametrize(
    "line",
    [
        "S22 docs/cards.md  ip:{base} a reason",
        "S22 docs/cards.md ip:{base}  a reason",
        "S22 docs/cards.md ip:{base} a\treason",
        "S22  docs/cards.md ip:{base} a reason",
        "S22\tdocs/cards.md ip:{base} a reason",
        "S10  python:samplelib MIT a reason",
        "S22",
    ],
)
def test_a_rule_line_with_two_spaces_or_a_tab_is_an_error_and_never_an_s3_path(repo, line):
    base = ip(10, 0, 0, 0)
    repo.write("docs/cards.md", f"A private range starts at {base}.\n")
    repo.allow(line.format(base=base))
    code, got, _ = repo.run()
    assert code == 1 and {got[s] for s in ("S3", "S22")} == {("error", 0)}


def test_an_unexpected_failure_of_the_script_ends_with_status_2_and_one_summary_line(repo):
    repo.write("pyproject.toml", 'project = "not a table"\n')
    repo.commit()
    for mode in ([], ["--release"]):
        r = repo.raw(*mode)
        assert r.returncode == 2 and "Traceback" not in r.stderr + r.stdout
        assert r.stdout.splitlines()[-1].startswith("public-readiness: result=error (")


class OneFile:
    def __init__(self, text: str):
        self.content = text

    def text(self, _path: str) -> str:
        return self.content


def test_an_indented_comment_in_the_allowlist_is_a_comment():
    assert script.allow_lines(OneFile("  # a note\n\t#another\nREADME.md  \n\n  S10 python:x MIT why\n")) == ["README.md", "S10 python:x MIT why"]


def test_an_input_that_cannot_be_read_ends_with_status_2_and_a_summary_line_not_a_traceback(repo):
    repo.owner_machine()
    repo.pattern_file(NO_MATCH)
    (repo.tmp / "dotfiles" / "config" / "public-push-remotes.txt").write_bytes(b"github.com/\xff\xfe broken\n")
    r = repo.raw("--release")
    assert r.returncode == 2 and r.stdout.splitlines()[-1].startswith("public-readiness: result=error (") and "Traceback" not in r.stderr
