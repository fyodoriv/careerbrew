#!/usr/bin/env python3
"""Public-readiness checks for one git ref. Run it through scripts/public-readiness.sh.

It reads the tracked content of the ref, never the working tree. It prints one line per
exposure surface of the open-source release plan, then one summary line. The plan is in the
project journal, which is not part of the public tree.
A line holds an id, a kind, a status, a count and a fixed label. It never holds matched text.

Every check reads the release tree: the files of the ref minus the paths that
config/public-release-exclude.txt (read from the ref) leaves out. The summary line names the
number of files left out. The list must work, or every surface that reads the tree is `error`
and the tree actions stop with status 2. It does not work when it is missing, when no line
leaves a path out, when a line matches no file, when it leaves out no file or every file, when
a path differs from a line only by letter case, or when the tree holds a submodule.
With --released the ref is the release commit itself: the list must be there, and the commit
must hold no file that the list leaves out.

S21 counts the files that point to a path the release tree leaves out. A Markdown link (read
only from a Markdown file) counts when it leads to a left-out file, or to a folder with no file left in the release tree. A plain
path counts when it starts at the repo root with a line of the list and names a left-out file.
The list itself is not read. --list-dangling prints `<paths>` TAB `<file>` per counted file.
With --released a path counts when the list covers it and the commit does not hold it; a plain
path must then end with a file extension, so a path pattern in prose does not count.

S7 reads the inventory docs/proof/public-readiness-binaries.tsv: one row per non-text file,
`<path>` TAB `<provenance>` TAB `<sha256>` TAB `<generator>`. The provenance is generated, synthetic,
own-recording or third-party-licensed; `pending` lists a file that is not cleared yet and counts.
The generator of a `generated` row is a file of the release tree; any other row takes `-`.
A file is non-text when it holds a NUL byte, when it starts with a UTF-16 or UTF-32 byte order mark
(the text checks read bytes, so they cannot read it), or when its name or its first bytes say that
it is a picture, a sound, a PDF, a font or an archive. A file counts when it has no row, a row with another
word, a hash that differs from its content, or a generator that is missing. A row for no file counts.
A PNG counts when it holds a chunk outside IHDR PLTE IDAT IEND tRNS gAMA cHRM sRGB sBIT bKGD pHYs,
or bytes after IEND: those chunks hold picture data only, and every other chunk can hold text, a
time or a profile. It also counts when a chunk has a wrong checksum or a size that its kind does
not allow (IEND holds nothing), or when the IDAT data is not one zlib stream of exactly the size
that the header gives. So no chunk has room for other bytes. The pixels themselves are not read.
A WAV file counts when it holds a chunk outside `fmt `, `fact` and `data`. The samples of `data`
are not read: a sound can say anything, so a `synthetic` WAV row rests on the reviewer's S7 signature.
The script cannot read the inside of any other non-text file: a known format (JPEG, GIF, WebP, PDF,
a font, an archive) or an unknown one (a database, a raw blob, UTF-16 text). Such a file counts
unless its row says own-recording or third-party-licensed: a person looked at it, and the S7
signature covers that. `generated` and `synthetic` clear a clean PNG or WAV file only.
--list-binaries prints `<path>` TAB `<sha256>` per non-text file.

S10 reads uv.lock, Cargo.lock and web/package-lock.json from the ref. The licence of a package
comes from this machine, offline: installed Python metadata, the Cargo registry sources, and the
`license` field of the web lock. A licence that cannot be read is `unknown` and counts. A lock
that names no package is an `error`. Rust and web packages are named `<name>@<version>`.
S14, S16 and S20 run in owner mode only and read this machine. S14 needs the gh text guard, the
project in the public push list, and a pre-push hook that holds the leak gate.
S16 searches each plugin name, and each word of a name that is not generic and has 4 letters or more.
S20 searches every file and file name for the owner's time zone, the city of the zone, the country,
other forms of the country from a table in this script, and the words in PUBLIC_READINESS_PLACE_TERMS
(a comma list, read in owner mode, never printed). These match in any letter case, non-ASCII letters
included, and a term of two words also matches joined or with `_` or `-`. The note says `zone=env`
when PUBLIC_READINESS_ZONE gave the zone. A form of three letters matches in capitals only.
The country code of two letters matches only after `<Word>, `, as the value of a key such as country
or location, or in a locale tag such as en_XX.
S16 and S20 ask the checkout's CLI, which reads the data folder. Their note says `home=default` or
`home=overridden`. With CAREERBREW_HOME set they are `unmeasured` unless --allow-home-override is given.
S17 writes the release tree to a scratch folder and runs the cheap self-checks there; --first-run
also runs setup, doctor and the fast gate. The steps get a scratch HOME and scratch XDG folders.
A step that runs longer than its limit fails, and its whole process group is killed
(PUBLIC_READINESS_STEP_SECONDS sets the limit; default 300, with --first-run 3600).
S22 counts machine and account identifiers by pattern, in the text and in the name of every file: a
host name of a private network (also in a URL, after `@`, before a port, a path or a final dot),
private, carrier-grade NAT and link-local IPv4 addresses (also with zero-padded octets), IPv6
addresses (also with an IPv4 tail), MAC addresses (pairs, the dotted form, and twelve bare hex
digits after a word such as mac or ether), UUIDs, reverse-domain job labels (any first word on
a line that speaks of launchd), account ids, analytics keys, phone numbers and private document
links. The allowlist is searched too, without the `<rule>:<text>` word of its S22 lines. Rules clear the reserved example names, the documentation ranges, the loopback address and
filler values. An `S22 <path> <rule>:<text> <reason>` line clears one text in one file.
The line may hold `sha256:<hash of the text>` in place of the text.
config/public-readiness-allow.txt holds the S3 paths and the `S10` and `S22` rule lines. A line
that starts with a surface id and does not have the shape of a rule line, or holds two spaces
or a tab anywhere, is an `error` for S3, S10 and S22.
The summary says `result=ready` only in owner mode with nothing open. A run with no failure and
with unmeasured, unsigned or owner-only surfaces says `result=no-failures` (status 0).
Any failure of the script itself ends with one `result=error` line and status 2, never a traceback.

Statuses: pass, fail, error (a tool or an input is missing; blocks in both modes), unmeasured
(no check yet), unsigned (a review surface whose machine part is clean), owner-only, release-day.

Known limits:
- S1 searches text with `git grep -I`, which skips binary files. The S7 review covers them.
- S11 writes the files of the ref to a scratch folder and scans it with the default gitleaks
  rules. It ignores `gitleaks:allow` comments and the caller's gitleaks settings. An in-tree
  gitleaks config or ignore file would hide content, so S11 is `error` when the tree holds one.
- S3 compares in any letter case, non-ASCII letters included.
- S3 knows the macOS account name only from PUBLIC_READINESS_ACCOUNT, or in owner mode from
  the current user. It does not check the author of the release commit.
- S22 does not count a host name directly before `.` and a file ending from a list in this script
  (`name.<ending>.json`), before `_word(` (a method call), or after a word such as threading or env.
  These have the shape of code or of a settings file for one machine. Any other text after the
  host name counts: `-2`, `_key`, `(1)`, `.1`.
- S22 counts `app.<name>.<name>(` as a job label: a call in code with that shape needs an allow line.
- The pattern file's minimum version is not checked here. The dotfiles gates check it.
- S10 counts every package of Cargo.lock, build and test tools included, and the runtime
  packages of the Python and web locks. It reads no licence text: bundled third-party text
  without a notice stays with the S9 review.
- S14 checks that the guards are in place. It does not send a planted term through them: the
  guards' own tests in dotfiles do that. It knows the hook by the name of the variable that the
  leak gate loads.
- S16 with an empty plugin list passes only when the data folder holds a profile, which marks the
  owner's data. In a checkout with no profile it is `unmeasured`. A CLI that fails is an `error`.
- S16 drops generic words by a short list. A company word that is also a common word gives many
  hits; a company word of 3 letters is searched only inside the full name.
- S20 derives the city and the country from the time zone. The city of a zone is the reference
  city of the zone, which is not always the owner's city. The script does not know a region, a
  province or a nickname unless PUBLIC_READINESS_PLACE_TERMS names it. The S5 review covers the rest.
  The table of country forms holds 27 countries; for any other country only the name and the code are
  searched. No word for the people of a country is searched (it is often a language name too).
- S22 does not flag a bare long number: a test vector and an account id look the same.
  It reads an IPv6 address only with a digit in it, and a job label with a first word outside
  com, org, net, io, dev and me only on a line that names launchd, launchctl or a plist.
- S17 keeps CARGO_HOME and RUSTUP_HOME of the caller, so the toolchain is not downloaded again.
  The caller's network and the tools on PATH are not scratch. The full run has not run on the real tree yet.
- S2 scans the checkout, not the ref. On a checkout that still holds excluded folders it
  counts them too. It measures the release tree only on a checkout of the release commit.
"""

from __future__ import annotations

import argparse
import datetime
import getpass
import hashlib
import json
import os
import posixpath
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import tomllib
import unicodedata
import urllib.parse
import zlib

# The private pattern arrives through a pipe, never through the environment or the argument
# list. Child processes do not inherit the pipe.
PATTERN = ""
_pattern_fd = os.environ.pop("PUBLIC_READINESS_PATTERN_FD", "")
if _pattern_fd.isdigit():
    with os.fdopen(int(_pattern_fd), encoding="utf-8", errors="replace") as _pipe:
        PATTERN = _pipe.read()
PATTERN_FILE = os.environ.pop("PUBLIC_READINESS_PATTERN_FILE", "")
for _name in ("PUBLIC_READINESS_PATTERN", "OSS_READINESS_INTERNAL_PATTERN"):
    os.environ.pop(_name, None)

ALLOW = "config/public-readiness-allow.txt"
EXCLUDE = "config/public-release-exclude.txt"
TREE_READERS = ("S1", "S3", "S7", "S10", "S11", "S13", "S14", "S16", "S17", "S20", "S21", "S22")
BINARIES = "docs/proof/public-readiness-binaries.tsv"
BINARIES_HEADER = "path\tprovenance\tsha256\tgenerator\n"
PROVENANCE = ("generated", "synthetic", "own-recording", "third-party-licensed")
PENDING = "pending"
PNG_MAGIC = b"\x89PNG\r\n\x1a\n"
# The PNG chunks that hold picture data only: no free text, no time, no embedded profile.
# Any other chunk counts (tEXt, iTXt, zTXt, eXIf, tIME, iCCP, sPLT, private chunks), and so do bytes after IEND.
PNG_CHUNKS = frozenset((b"IHDR", b"PLTE", b"IDAT", b"IEND", b"tRNS", b"gAMA", b"cHRM", b"sRGB", b"sBIT", b"bKGD", b"pHYs"))
# The same for a WAV file: the sample format, the sample count and the samples.
WAV_CHUNKS = frozenset((b"fmt ", b"fact", b"data"))
# Formats that can carry metadata this script cannot read. A file is one of them by its name or by its first bytes.
MEDIA_NAMES = (".jpg", ".jpeg", ".gif", ".webp", ".pdf", ".ico", ".bmp", ".tif", ".tiff", ".heic", ".avif", ".mp3", ".mp4", ".m4a", ".mov", ".ogg", ".webm", ".flac")
MEDIA_NAMES += (".woff", ".woff2", ".ttf", ".otf", ".eot", ".zip", ".gz", ".tgz", ".bz2", ".xz", ".7z", ".tar", ".jar", ".whl", ".dmg", ".docx", ".xlsx", ".pptx")
MEDIA_MAGIC = (b"\xff\xd8\xff", b"GIF8", b"%PDF-", b"PK\x03\x04", b"\x1f\x8b", b"wOFF", b"wOF2", b"OTTO", b"BZh", b"\xfd7zXZ", b"7z\xbc\xaf", b"ID3", b"OggS", b"fLaC")
BINARY_NAMES = (".png", ".wav", *MEDIA_NAMES)
REVIEWED = ("own-recording", "third-party-licensed")
RULE_LINE = re.compile(r"(S\d+) (\S+)(?: (.*))?")
RULE_START = re.compile(r"S\d+(?:\s|$)")
BAD_ALLOW = "an allow line is malformed or has no reason"
# Text in an encoding that the text checks cannot read as bytes: UTF-16 and UTF-32 with a byte order mark.
WIDE_TEXT = (b"\xff\xfe", b"\xfe\xff", b"\x00\x00\xfe\xff")
SIGNATURES = "docs/proof/public-readiness.md"
SIGNATURES_HEADER = """# Public readiness review signatures

One line per review surface: `<surface> <reviewer> <date> <findings-left> <tree-hash>`.
`scripts/public-readiness.sh --tree-hash` prints the hash. No other text may follow this header.

"""
REVIEW = ("S4", "S5", "S6", "S7", "S8", "S9", "S18")
ENTRY = re.compile(r"(S\d+) (judge-\d+) (\d{4}-\d{2}-\d{2}) (\d+) ([0-9a-f]{64})")
# The home folder prefixes are split so that this file does not match its own check.
HOME_PATH = re.compile(rb"/" + rb"(?:Users|home)/[A-Za-z0-9._-]")
IDENTITY_FILES = ("pyproject.toml", "README.md", "INSTALL.md", "CONTRIBUTING.md", "scripts/install.sh")
SCAN_OVERRIDES = (".gitleaks.toml", ".gitleaksignore")

# id, kind, label. The order is the output order.
SURFACES = [
    ("S1", "machine", "private-reference pattern hits in text, names and commit message"),
    ("S2", "machine", "findings of the repo's own privacy scan"),
    ("S3", "machine", "files with owner identity outside the allowlist"),
    ("S4", "review", "other real people"),
    ("S5", "review", "personal facts by inference"),
    ("S6", "review", "work-derived stories"),
    ("S7", "review", "non-text files without a provenance line"),
    ("S8", "review", "fixtures and captured traffic"),
    ("S9", "review", "third-party content"),
    ("S10", "machine", "dependency licences"),
    ("S11", "machine", "gitleaks findings in the exported tree"),
    ("S12", "release-day", "git history"),
    ("S13", "release-day", "files that link to old pull request or issue numbers"),
    ("S14", "machine", "text the tools write to GitHub"),
    ("S15", "release-day", "copies outside the repository"),
    ("S16", "machine", "corporate plugin names"),
    ("S17", "machine", "first run for a stranger"),
    ("S18", "review", "the corporate-laptop story"),
    ("S19", "release-day", "settings of the new repository and the archive"),
    ("S20", "machine", "the owner's time zone, the zone's city (may not be the owner's city) and the country, in any file"),
    ("S21", "machine", "files that point to a path the release tree leaves out"),
    ("S22", "machine", "machine and account identifiers"),
]
OWNER_ONLY = ("S1", "S2", "S14", "S16", "S20")
RELEASE_DAY = ("S12", "S15", "S19")
STATUSES = ("pass", "fail", "error", "unmeasured", "unsigned", "owner-only", "release-day")
Result = tuple[str, int, str]


def stop(message: str) -> SystemExit:
    print(f"public-readiness: {message}", file=sys.stderr)
    return SystemExit(2)


def git(*args: str, ok: tuple[int, ...] = (0,)) -> bytes:
    r = subprocess.run(["git", *args], capture_output=True, check=False)
    if r.returncode not in ok:
        raise stop(f"git {args[0]} failed with status {r.returncode}")
    return r.stdout


def covers(rule: str, path: str) -> bool:
    """A rule that ends with "/" names a folder. Any other rule names one file."""
    return path.startswith(rule) if rule.endswith("/") else path == rule


class Tree:
    """The release tree of one ref: path -> bytes, without the paths the exclusion list leaves out."""

    def __init__(self, ref: str, released: bool = False):
        self.released = released
        self.commit = git("rev-parse", "--verify", "--quiet", "--end-of-options", f"{ref}^{{commit}}", ok=(0, 1, 128)).decode().strip()
        if not re.fullmatch(r"[0-9a-f]{40,64}", self.commit):
            raise stop("the ref is not a commit")
        self.files: dict[str, bytes] = {}
        self.modes: dict[str, str] = {}
        entries = []
        for raw in git("ls-tree", "-r", "-z", self.commit).split(b"\0"):
            if not raw:
                continue
            meta, path = raw.split(b"\t", 1)
            mode, kind, oid = meta.decode().split(" ")
            entries.append((raw, mode, kind, oid, path.decode("utf-8", errors="surrogateescape")))
        with subprocess.Popen(["git", "cat-file", "--batch"], stdin=subprocess.PIPE, stdout=subprocess.PIPE) as p:
            assert p.stdin and p.stdout
            for _raw, mode, kind, oid, path in entries:
                if kind != "blob":
                    continue
                p.stdin.write(oid.encode() + b"\n")
                p.stdin.flush()
                size = int(p.stdout.readline().split()[2])
                self.files[path] = p.stdout.read(size)
                self.modes[path] = mode
                p.stdout.read(1)
            p.stdin.close()
        # The exclusion list. A "!" line keeps a path that a folder line would leave out.
        listed = self.text(EXCLUDE)
        rules = [ln for ln in (raw.strip() for raw in (listed or "").splitlines()) if ln and not ln.startswith("#")]
        self.drop = [r for r in rules if not r.startswith("!")]
        self.keep = [r[1:].strip() for r in rules if r.startswith("!")]
        paths = [e[4] for e in entries]
        dropped = {path for path in paths if any(covers(r, path) for r in self.drop)}
        kept = {path for path in dropped if any(covers(r, path) for r in self.keep)}
        self.left_out = dropped - kept
        self.excluded = len(self.left_out)
        self.listing = [e[0] for e in entries if e[4] not in self.left_out]
        self.problem = self._problem(listed, entries, dropped)
        for path in self.left_out:
            self.files.pop(path, None)
            self.modes.pop(path, None)

    def _problem(self, listed: str | None, entries: list, dropped: set[str]) -> tuple[int, str] | None:
        """Why the exclusion list cannot be trusted for this ref: a count and a fixed reason. None when it works."""
        paths = [e[4] for e in entries]
        submodules = sum(1 for e in entries if e[2] == "commit")
        if submodules:
            # A submodule has no content in this repository: no check can read it, and no action can write it.
            return submodules, "the tree holds a submodule"
        if listed is None:
            return 0, "the exclusion list is missing"
        if not self.drop:
            return 0, "no line of the exclusion list leaves a path out"
        if not self.listing:
            return self.excluded, "the exclusion list leaves every file out"
        if EXCLUDE in self.left_out:
            return 1, "the exclusion list leaves itself out"
        lower = [r.lower() for r in self.drop]
        variants = sum(1 for path in paths if path not in dropped and any(covers(r, path.lower()) for r in lower))
        if variants:
            return variants, "a path differs from a line of the exclusion list only by letter case"
        if self.released:
            if self.left_out:
                return self.excluded, "the release commit holds files that the exclusion list leaves out"
            stale = sum(1 for r in self.keep if not any(covers(r, path) for path in paths))
        else:
            # A stale line matches no file. A "!" line must match a file that a folder line leaves out.
            stale = sum(1 for r in self.drop if not any(covers(r, path) for path in paths)) + sum(1 for r in self.keep if not any(covers(r, path) for path in dropped))
        if stale:
            return stale, "a line of the exclusion list matches no file"
        if not self.left_out and not self.released:
            return 0, "the exclusion list leaves no file out"
        return None

    def gone(self, path: str, folder: bool = False) -> bool:
        """True when the release tree leaves this path out. With `folder`, also for a folder that it leaves empty."""
        inside = path + "/"
        if self.released:
            # The left-out files are not in the release commit. The list tells what it would have left out.
            if path in self.files:
                return False
            if any(covers(r, path) for r in self.drop) and not any(covers(r, path) for r in self.keep):
                return True
            return folder and any(inside.startswith(r) for r in self.drop if r.endswith("/")) and not any(f.startswith(inside) for f in self.files)
        if path in self.left_out:
            return True
        return folder and any(f.startswith(inside) for f in self.left_out) and not any(f.startswith(inside) for f in self.files)

    def text(self, path: str) -> str | None:
        data = self.files.get(path)
        return None if data is None else data.decode("utf-8", errors="replace")

    def project(self) -> dict:
        try:
            return tomllib.loads(self.text("pyproject.toml") or "").get("project", {})
        except tomllib.TOMLDecodeError:
            return {}

    def hash(self) -> str:
        """A hash of every file of the release tree except the signature file, so a signature does not change what it signs."""
        skip = b"\t" + SIGNATURES.encode()
        kept = [e for e in self.listing if not e.endswith(skip)]
        return hashlib.sha256(b"\0".join(kept)).hexdigest()

    def write(self, target: str) -> int:
        """Write the release tree into a new or empty folder, with file modes and symbolic links. Return the file count."""
        if os.path.lexists(target) and (not os.path.isdir(target) or os.listdir(target)):
            raise stop("the release tree folder must be new or empty")
        for path, data in self.files.items():
            out = os.path.join(target, path)
            os.makedirs(os.path.dirname(out), exist_ok=True)
            if self.modes[path] == "120000":
                os.symlink(os.fsdecode(data), out)
                continue
            with open(out, "wb") as f:
                f.write(data)
            os.chmod(out, 0o755 if self.modes[path] == "100755" else 0o644)
        return len(self.files)

    def git_tree(self) -> str:
        """The id of the git tree object that holds exactly the release tree. `git commit-tree` takes it."""
        with tempfile.TemporaryDirectory() as tmp:
            env = {**os.environ, "GIT_INDEX_FILE": os.path.join(tmp, "index")}
            names = b"".join(os.fsencode(path) + b"\0" for path in sorted(self.left_out))
            steps = (["read-tree", self.commit], ["update-index", "--force-remove", "-z", "--stdin"], ["write-tree"])
            for args in steps:
                r = subprocess.run(["git", *args], input=names if args[0] == "update-index" else None, capture_output=True, check=False, env=env)
                if r.returncode != 0:
                    raise stop(f"git {args[0]} failed with status {r.returncode}")
        return r.stdout.decode().strip()


def is_text(data: bytes) -> bool:
    # The whole content: a NUL byte after a long clean start still means text that the byte checks cannot read.
    return b"\0" not in data


def allow_lines(tree: Tree) -> list[str]:
    lines = [ln.strip() for ln in (tree.text(ALLOW) or "").splitlines()]
    return [ln for ln in lines if ln and not ln.startswith("#")]


def rule_lines(tree: Tree, sid: str) -> list[tuple[str, str]] | None:
    """The `<sid> <key> <reason>` lines of the allowlist as (key, reason). None when a line has no reason,
    or when any line starts like a rule line and does not have its shape (two spaces, a tab)."""
    out = []
    for line in allow_lines(tree):
        m = RULE_LINE.fullmatch(line)
        if RULE_START.match(line) and (not m or "  " in line or "\t" in line):
            return None
        if m and m[1] == sid:
            if not (m[3] or "").strip():
                return None
            out.append((m[2], m[3]))
    return out


def project_key(tree: Tree) -> str:
    """`github.com/<user>/<name>` from the links to this project, lower case. Empty when no link names it."""
    name = str(tree.project().get("name", ""))
    if not name:
        return ""
    link = re.compile(r"github\.com[/:]([A-Za-z0-9-]+)/" + re.escape(name) + r"\b")
    for path in IDENTITY_FILES:
        found = link.search(tree.text(path) or "")
        if found:
            return f"github.com/{found[1]}/{name}".lower()
    return ""


def whole_words(terms: list[str]) -> re.Pattern[bytes]:
    """Match any term as a whole word in lower-case bytes. A space in a term also matches `_` and `-`."""
    parts = [re.escape(t.lower()).replace(r"\ ", "[ _-]").encode() for t in terms]
    return re.compile(rb"(?<![a-z0-9])(?:" + b"|".join(parts) + rb")(?![a-z0-9])")


def owner_markers(tree: Tree, release: bool) -> tuple[list[str], str]:
    """Owner markers, lower case, and a problem. No marker when pyproject.toml names no author.

    A marker that cannot be checked is a problem, never a silent drop.

    The full name, each part of it and the email come from pyproject.toml authors. The GitHub user
    comes from links to this project. The macOS account name comes from PUBLIC_READINESS_ACCOUNT,
    or in owner mode from the current user.
    """
    project = tree.project()
    found: list[str] = []
    for author in project.get("authors", []) or []:
        name, email = str(author.get("name", "")), str(author.get("email", ""))
        found += [name, email, *name.split()]
    if not any(found):
        return [], ""
    handles: list[str] = []
    if project.get("name"):
        link = re.compile(r"github\.com[/:]([A-Za-z0-9-]+)/" + re.escape(str(project["name"])) + r"\b")
        for path in IDENTITY_FILES:
            handles += link.findall(tree.text(path) or "")
    if not handles:
        return [], "no link names the owner's GitHub user"
    found += handles
    account = os.environ.get("PUBLIC_READINESS_ACCOUNT", "")
    if not account and release:
        try:
            account = getpass.getuser()
        except (KeyError, OSError):
            account = ""
    found.append(account)
    markers = sorted({m.strip().lower() for m in found if m.strip()})
    if any(len(m) < 3 for m in markers):
        return [], "an owner marker is shorter than 3 characters"
    return markers, ""


def s3(tree: Tree, release: bool) -> Result:
    markers, problem = owner_markers(tree, release)
    if problem:
        return "error", 0, problem
    if not markers:
        return "fail", 0, "no author in pyproject.toml"
    if rule_lines(tree, "S3") is None:
        return "error", 0, BAD_ALLOW
    allow = {ln for ln in allow_lines(tree) if not RULE_LINE.fullmatch(ln)}
    # Whole words only: a marker inside a longer word is another word.
    # Any letter case, non-ASCII letters included: the text and the markers are compared folded.
    words = re.compile("(?<![a-z0-9])(?:" + "|".join(re.escape(folded(m)) for m in markers) + ")(?![a-z0-9])")
    hits = 0
    for path, data in tree.files.items():
        if path in allow:
            continue
        text = is_text(data)
        if words.search(folded(path)) or (text and (words.search(folded(data.decode("utf-8", errors="replace"))) or HOME_PATH.search(data))):
            hits += 1
    return ("fail" if hits else "pass"), hits, ""


# Chunk -> its only size. tRNS, sBIT and bKGD depend on the colour type; PLTE and IDAT vary.
PNG_SIZES = {b"IHDR": 13, b"IEND": 0, b"gAMA": 4, b"cHRM": 32, b"sRGB": 1, b"pHYs": 9}
PNG_CHANNELS = {0: 1, 2: 3, 3: 1, 4: 2, 6: 4}
PNG_BY_COLOUR = {b"tRNS": {0: 2, 2: 6}, b"sBIT": {0: 1, 2: 3, 3: 3, 4: 2, 6: 4}, b"bKGD": {0: 2, 4: 2, 2: 6, 6: 6, 3: 1}}
ADAM7 = ((0, 0, 8, 8), (4, 0, 8, 8), (0, 4, 4, 8), (2, 0, 4, 4), (0, 2, 2, 4), (1, 0, 2, 2), (0, 1, 1, 2))


def png_problem(data: bytes) -> bool:
    """True when a PNG holds anything but a picture: a chunk outside the allowed set, a chunk of a wrong size
    or with a wrong checksum, bytes after the end, or pixel data that is not one zlib stream of the size
    that the header gives. So no chunk has room for other bytes."""
    at, head, palette, pixels = len(PNG_MAGIC), None, None, bytearray()
    while True:
        size, kind = int.from_bytes(data[at : at + 4], "big"), data[at + 4 : at + 8]
        body, check = data[at + 8 : at + 8 + size], data[at + 8 + size : at + 12 + size]
        at += 12 + size
        if kind not in PNG_CHUNKS or len(check) != 4 or zlib.crc32(kind + body) != int.from_bytes(check, "big"):
            return True
        if size != PNG_SIZES.get(kind, size) or (head is None) != (kind == b"IHDR"):
            return True
        if kind == b"IHDR":
            width, height, depth, colour, packing, filtering, interlace = (int.from_bytes(body[:4], "big"), int.from_bytes(body[4:8], "big"), *body[8:])
            if not width or not height or colour not in PNG_CHANNELS or depth not in (1, 2, 4, 8, 16) or packing or filtering or interlace > 1:
                return True
            head = (width, height, depth * PNG_CHANNELS[colour], colour)
        elif kind == b"PLTE":
            if palette is not None or size % 3 or not 3 <= size <= 768:
                return True
            palette = size // 3
        elif kind in PNG_BY_COLOUR:
            colour = head[3]
            fixed = PNG_BY_COLOUR[kind].get(colour)
            if kind == b"tRNS" and colour == 3:
                if palette is None or not 1 <= size <= palette:
                    return True
            elif size != fixed:
                return True
        elif kind == b"IDAT":
            pixels += body
        elif kind == b"IEND":
            break
    if at != len(data) or head is None:
        return True
    width, height, bits, _colour = head
    grids = [(width, height)] if not interlace else [((width - x + dx - 1) // dx, (height - y + dy - 1) // dy) for x, y, dx, dy in ADAM7]
    expected = sum(rows * (1 + (cols * bits + 7) // 8) for cols, rows in grids if cols > 0 and rows > 0)
    try:
        stream = zlib.decompressobj()
        raw = stream.decompress(bytes(pixels), expected + 1)
    except zlib.error:
        return True
    return len(raw) != expected or not stream.eof or bool(stream.unused_data or stream.unconsumed_tail)


def chunks_problem(data: bytes) -> bool:
    """True when a PNG or a WAV file holds a chunk outside the allowed set, bytes after its end, or cannot be read."""
    if data.startswith(PNG_MAGIC):
        return png_problem(data)
    if data[:4] == b"RIFF" and data[8:12] == b"WAVE" and int.from_bytes(data[4:8], "little") + 8 == len(data):
        at = 12
        while at + 8 <= len(data):
            if data[at : at + 4] not in WAV_CHUNKS:
                return True
            at += 8 + int.from_bytes(data[at + 4 : at + 8], "little")
            at += at % 2
        return at != len(data)
    return True


def media(path: str, data: bytes) -> bool:
    """True for a format that can carry metadata this script cannot read."""
    return path.lower().endswith(MEDIA_NAMES) or data.startswith(MEDIA_MAGIC) or (data[:4] == b"RIFF" and data[8:12] != b"WAVE")


def binaries(tree: Tree) -> dict[str, str]:
    """Non-text file of the release tree -> the sha256 of its content. A known binary format counts by name or first bytes too."""
    return {
        path: hashlib.sha256(data).hexdigest() for path, data in sorted(tree.files.items()) if tree.modes[path] != "120000" and (not is_text(data) or data.startswith(WIDE_TEXT) or path.lower().endswith(BINARY_NAMES) or media(path, data))
    }


def s7_inventory(tree: Tree) -> tuple[int, str]:
    """The number of inventory problems and a fixed note with one count per kind."""
    text = tree.text(BINARIES) or ""
    rows: dict[str, list[str]] = {}
    bad = 0 if text.startswith(BINARIES_HEADER) else 1
    for line in text.splitlines()[1:]:
        cells = line.split("\t")
        if len(cells) != 4 or cells[0] in rows:
            bad += 1
            continue
        rows[cells[0]] = cells[1:]
    found = binaries(tree)
    tally = {"missing": 0, "pending": 0, "changed": 0, "metadata": 0, "unverified": 0, "stale": sum(1 for path in rows if path not in found), "malformed": bad}
    for path, digest in found.items():
        provenance, recorded, generator = rows.get(path, ["", "", ""])
        data = tree.files[path]
        if provenance == PENDING:
            tally["pending"] += 1
        elif provenance not in PROVENANCE or (generator not in tree.files if provenance == "generated" else generator != "-"):
            tally["missing"] += 1
        elif recorded != digest:
            tally["changed"] += 1
        elif not media(path, data) and (data.startswith(PNG_MAGIC) or data[:4] == b"RIFF" or path.lower().endswith((".png", ".wav"))):
            tally["metadata"] += chunks_problem(data)
        else:
            # Default deny: this script reads the inside of a PNG and of a WAV file only. Any other binary
            # (a known media format or an unknown one, UTF-16 text included) needs a person: a reviewed provenance.
            tally["unverified"] += provenance not in REVIEWED
    return sum(tally.values()), "inventory: " + " ".join(f"{k}={v}" for k, v in tally.items())


def s13(tree: Tree) -> Result:
    name = str(tree.project().get("name", ""))
    if not name:
        return "error", 0, "no project name in pyproject.toml"
    link = re.compile(re.escape(name).encode() + rb"/(pull|issues)/[0-9]+")
    hits = sum(1 for data in tree.files.values() if is_text(data) and link.search(data))
    return ("fail" if hits else "pass"), hits, ""


MD_LINK = re.compile(r"\]\(\s*<?([^)\s>]+)")
URL_SCHEME = re.compile(r"[A-Za-z][A-Za-z0-9+.-]*:")
PATH_CHARS = "A-Za-z0-9_./-"
HAS_EXTENSION = re.compile(r"\.[A-Za-z0-9]+$")
MARKDOWN = (".md", ".mdx", ".markdown")


def dangling(tree: Tree) -> dict[str, int]:
    """Release-tree file -> the number of distinct left-out paths that it points to."""
    if not tree.drop:
        return {}
    plain = re.compile(f"(?<![{PATH_CHARS}])(?:" + "|".join(re.escape(r) for r in tree.drop) + f")[{PATH_CHARS}]*")
    out: dict[str, int] = {}
    for path, data in tree.files.items():
        if path == EXCLUDE or not is_text(data):
            continue
        text = data.decode("utf-8", errors="replace")
        here = posixpath.dirname(path)
        # The content of a symbolic link is its target.
        # Only a Markdown file holds Markdown links: `rows[i](raw)` in a data table or in code is not one.
        links = [text] if tree.modes[path] == "120000" else MD_LINK.findall(text) if path.lower().endswith(MARKDOWN) else []
        found = set()
        for link in links:
            if URL_SCHEME.match(link) or link.startswith("#"):
                continue
            link = urllib.parse.unquote(re.split(r"[#?]", link)[0])
            target = posixpath.normpath(link.lstrip("/") if link.startswith("/") else posixpath.join(here, link))
            if tree.gone(target, folder=True):
                found.add(target)
        if tree.modes[path] != "120000":
            for name in plain.findall(text):
                name = name.rstrip(".")
                if tree.gone(name) and (not tree.released or HAS_EXTENSION.search(name)):
                    found.add(name)
        if found:
            out[path] = len(found)
    return out


def s21(tree: Tree) -> Result:
    hits = len(dangling(tree))
    return ("fail" if hits else "pass"), hits, ""


def s11(tree: Tree) -> Result:
    hidden = sum(1 for path in tree.files if os.path.basename(path) in SCAN_OVERRIDES)
    if hidden:
        return "error", hidden, "the tree holds a gitleaks config or ignore file"
    if not shutil.which("gitleaks"):
        return "error", 0, "gitleaks is not installed"
    with tempfile.TemporaryDirectory() as tmp:
        export, report, no_ignores = os.path.join(tmp, "tree"), os.path.join(tmp, "report.json"), os.path.join(tmp, "empty")
        os.mkdir(no_ignores)
        # Write the blobs read from the ref. `git archive` would obey export-ignore attributes.
        for path, data in tree.files.items():
            target = os.path.join(export, path)
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, "wb") as f:
                f.write(data)
        cmd = ["gitleaks", "detect", "--no-git", "--no-banner", "--redact", "--exit-code", "0", "--source", export]
        cmd += ["--ignore-gitleaks-allow", "--gitleaks-ignore-path", no_ignores, "--report-format", "json", "--report-path", report]
        env = {k: v for k, v in os.environ.items() if k not in ("GITLEAKS_CONFIG", "GITLEAKS_CONFIG_TOML")}
        scan: subprocess.CompletedProcess[bytes] | None = subprocess.run(cmd, capture_output=True, check=False, env=env)
        try:
            with open(report, encoding="utf-8") as f:
                hits = len(json.load(f))
        except (OSError, ValueError):
            scan = None
    if scan is None or scan.returncode != 0:
        return "error", 0, "gitleaks did not finish"
    return ("fail" if hits else "pass"), hits, ""


def count_pattern(pattern: str, args: list[str], data: bytes | None = None, tree: Tree | None = None) -> int:
    """Count matches with grep -E. The pattern goes through a pipe, never through the argument list.

    With a tree, git grep prints `<commit>:<path>` NUL `<count>` per file; only files of the release tree count.
    """
    ref = tree.commit if tree else None
    r, w = os.pipe()
    os.write(w, pattern.encode() + b"\n")
    os.close(w)
    try:
        out = subprocess.run([*args, "-f", f"/dev/fd/{r}", *([ref] if ref else [])], input=data, capture_output=True, pass_fds=(r,), check=False)
    finally:
        os.close(r)
    if out.returncode not in (0, 1):
        raise ValueError
    if tree is None:
        return sum(int(line.rsplit(b":", 1)[-1]) for line in out.stdout.splitlines() if line)
    hits = 0
    for name, count in re.findall(rb"([^\0]+)\0(\d+)\n", out.stdout):
        if name[len(tree.commit) + 1 :].decode("utf-8", errors="surrogateescape") in tree.files:
            hits += int(count)
    return hits


def s1(tree: Tree) -> Result:
    pattern = PATTERN.strip()
    if not PATTERN_FILE or not pattern:
        return "fail", 0, "the private pattern file is missing or its pattern is empty"
    if "\n" in pattern:
        return "error", 0, "the private pattern is not a valid expression"
    names = "\n".join(tree.files).encode("utf-8", errors="surrogateescape")
    message = git("log", "-1", "--format=%B", tree.commit)
    try:
        hits = count_pattern(pattern, ["git", "grep", "-I", "-i", "-E", "-c", "-z"], tree=tree)
        hits += count_pattern(pattern, ["grep", "-i", "-E", "-c"], names)
        hits += count_pattern(pattern, ["grep", "-i", "-E", "-c"], message)
    except ValueError:
        return "error", 0, "the private pattern is not a valid expression"
    return ("fail" if hits else "pass"), hits, ""


def s2(tree: Tree) -> Result:
    """Run scripts/leak-check.sh. It scans the checkout, so the checkout and the index must equal the ref."""
    for cached in ([], ["--cached"]):
        if subprocess.run(["git", "diff", *cached, "--quiet", tree.commit, "--"], check=False).returncode != 0:
            return "unmeasured", 0, "the checkout or the index differs from the ref"
    if not os.access("scripts/leak-check.sh", os.X_OK):
        return "error", 0, "scripts/leak-check.sh is missing"
    scan = subprocess.run(["scripts/leak-check.sh"], capture_output=True, text=True, check=False)
    summary = re.search(r"^privacy scan: (\d+) file\(s\), (\d+) finding\(s\)$", scan.stdout, re.M)
    if not summary:
        return "error", 0, "the privacy scan printed no summary"
    files, hits = int(summary[1]), int(summary[2])
    if not files:
        return "fail", hits, "the privacy scan read no file"
    return ("fail" if scan.returncode or hits else "pass"), hits, ""


# Licences that allow distribution of this project under MIT when the package is used as a
# dependency. Any other licence needs an `S10 <ecosystem>:<name> <licence> <reason>` line in the
# allowlist. The line holds the licence as S10 reads it, so a package that changes its licence counts again.
LICENCE_OK = frozenset(s.lower() for s in ("MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "0BSD", "Unlicense", "Zlib", "CC0-1.0", "BSL-1.0", "PSF-2.0", "Python-2.0", "Unicode-3.0", "Unicode-DFS-2016", "BlueOak-1.0.0"))
# Free-text licence names of old Python metadata, lower case -> SPDX id. Only names that mean one
# licence: "BSD License" does not say which BSD licence, so it stays unknown until an allow line names it.
LICENCE_NAMES = {
    "mit license": "MIT",
    "apache software license": "Apache-2.0",
    "apache 2.0": "Apache-2.0",
    "apache license 2.0": "Apache-2.0",
    "apache license, version 2.0": "Apache-2.0",
    "isc license (iscl)": "ISC",
    "isc license": "ISC",
    "python software foundation license": "PSF-2.0",
    "the unlicense (unlicense)": "Unlicense",
    "mozilla public license 2.0 (mpl 2.0)": "MPL-2.0",
    "gnu lesser general public license v3 or later (lgplv3+)": "LGPL-3.0-or-later",
}
LOCKS = (("pyproject.toml", "uv.lock"), ("Cargo.toml", "Cargo.lock"), ("web/package.json", "web/package-lock.json"))


def licence_verdict(text: str) -> str:
    """`ok`, `denied` or `unknown` for an SPDX expression or a known licence name."""
    text = LICENCE_NAMES.get(text.strip().lower(), text.strip())
    tokens = re.findall(r"\(|\)|[A-Za-z0-9.+-]+", text.replace("/", " OR "))
    if not tokens or "".join(tokens) != re.sub(r"\s+", "", text.replace("/", " OR ")):
        return "unknown"
    at = 0

    def atom() -> bool:
        nonlocal at
        word = tokens[at]
        at += 1
        if word == "(":
            value = either()
            if tokens[at] != ")":
                raise IndexError
            at += 1
            return value
        if word == ")" or word.upper() in ("AND", "OR", "WITH"):
            raise IndexError
        if at < len(tokens) and tokens[at].upper() == "WITH":
            at += 2  # an exception widens the licence; it never narrows it
        return word.lower() in LICENCE_OK

    def both() -> bool:
        nonlocal at
        value = atom()
        while at < len(tokens) and tokens[at].upper() == "AND":
            at += 1
            value = atom() and value
        return value

    def either() -> bool:
        nonlocal at
        value = both()
        while at < len(tokens) and tokens[at].upper() == "OR":
            at += 1
            value = both() or value
        return value

    try:
        value = either()
    except IndexError:
        return "unknown"
    if at != len(tokens):
        return "unknown"
    return "ok" if value else "denied"


def licence_id(text: str) -> str:
    """A licence as one word for an allow line: the SPDX name without spaces or outer brackets, or `unknown`."""
    word = re.sub(r"\s+", "", LICENCE_NAMES.get(text.strip().lower(), text.strip()))
    while word.startswith("(") and word.endswith(")") and "(" not in word[1:-1]:
        word = word[1:-1]
    return word or "unknown"


def dist_name(name: str) -> str:
    return re.sub(r"[-_.]+", "-", name).lower()


def python_licences(tree: Tree) -> dict[str, str] | None:
    """Runtime package of uv.lock -> its licence text from the installed metadata ("" when not readable)."""
    import importlib.metadata

    lock = tomllib.loads(tree.text("uv.lock") or "")
    packages = {dist_name(p["name"]): p for p in lock.get("package", [])}
    roots = [n for n, p in packages.items() if "editable" in p.get("source", {}) or "virtual" in p.get("source", {})]
    if not roots:
        return None
    # Walk (package, extra): a dependency that asks for an extra also pulls the packages of that extra.
    todo: list[tuple[str, str]] = [(root, "") for root in roots]
    walked, seen = set(todo), set(roots)
    while todo:
        name, extra = todo.pop()
        package = packages.get(name, {})
        wanted = package.get("optional-dependencies", {}).get(extra, []) if extra else package.get("dependencies", [])
        for dep in wanted:
            seen.add(dist_name(dep["name"]))
            for step in [(dist_name(dep["name"]), ""), *((dist_name(dep["name"]), e) for e in dep.get("extra", []))]:
                if step not in walked:
                    walked.add(step)
                    todo.append(step)
    installed = {dist_name(d.metadata["Name"] or ""): d for d in importlib.metadata.distributions()}
    out = {}
    for name in sorted(seen - set(roots)):
        dist = installed.get(name)
        out[name] = ""
        if dist is None or dist.version != str(packages.get(name, {}).get("version")):
            continue
        meta = dist.metadata
        classifiers = [c.rsplit("::", 1)[-1].strip() for c in meta.get_all("Classifier") or [] if c.startswith("License ::")]
        out[name] = meta.get("License-Expression") or " AND ".join(f"({LICENCE_NAMES.get(c.lower(), c)})" for c in classifiers) or meta.get("License") or ""
    return out


def rust_licences(tree: Tree) -> dict[str, str]:
    """Registry package of Cargo.lock -> the `license` of its manifest in the local Cargo registry."""
    home = os.environ.get("CARGO_HOME") or os.path.join(os.path.expanduser("~"), ".cargo")
    sources = os.path.join(home, "registry", "src")
    indexes = sorted(os.listdir(sources)) if os.path.isdir(sources) else []
    out = {}
    for package in tomllib.loads(tree.text("Cargo.lock") or "").get("package", []):
        if "source" not in package:
            continue  # a crate of this workspace
        key = f"{package['name']}@{package['version']}"
        out[key] = ""
        for index in indexes:
            try:
                with open(os.path.join(sources, index, f"{package['name']}-{package['version']}", "Cargo.toml"), "rb") as f:
                    out[key] = str(tomllib.load(f).get("package", {}).get("license", ""))
                break
            except (OSError, tomllib.TOMLDecodeError):
                continue
    return out


def web_licences(tree: Tree) -> dict[str, str]:
    """Runtime package of the web lock, `<name>@<version>` -> its `license` field. Two copies that differ give ""."""
    out: dict[str, str] = {}
    for path, entry in json.loads(tree.text("web/package-lock.json") or "{}")["packages"].items():
        if path and not entry.get("dev") and not entry.get("link"):
            licence = entry.get("license", "")
            key = f"{path.rsplit('node_modules/', 1)[-1]}@{entry['version']}"
            licence = licence if isinstance(licence, str) else ""
            out[key] = licence if out.get(key, licence) == licence else ""
    return out


def own_licence_gaps(tree: Tree) -> int:
    """The number of places that must name this project's licence and do not."""
    gaps = 0 if (tree.text("LICENSE") or "").strip() else 1
    try:
        if "pyproject.toml" in tree.files and not tree.project().get("license"):
            gaps += 1
        if "Cargo.toml" in tree.files:
            workspace = tomllib.loads(tree.text("Cargo.toml") or "").get("workspace", {})
            shared = workspace.get("package", {}).get("license")
            # Each crate names the licence, or the workspace does it for all of them.
            for manifest in ["Cargo.toml", *(f"{member}/Cargo.toml" for member in workspace.get("members", []))]:
                package = tomllib.loads(tree.text(manifest) or "").get("package")
                if package is not None and not (package.get("license") or shared) or (package is None and manifest != "Cargo.toml"):
                    gaps += 1
        if "web/package.json" in tree.files and not json.loads(tree.text("web/package.json") or "{}").get("license"):
            gaps += 1
    except (ValueError, AttributeError):
        gaps += 1
    return gaps


def s10(tree: Tree) -> Result:
    allowed = rule_lines(tree, "S10")
    if allowed is None:
        return "error", 0, BAD_ALLOW
    missing = sum(1 for manifest, lock in LOCKS if manifest in tree.files and lock not in tree.files)
    if missing:
        return "error", missing, "a manifest has no lock file"
    readers = (("python", "uv.lock", python_licences), ("rust", "Cargo.lock", rust_licences), ("web", "web/package-lock.json", web_licences))
    found: dict[str, str] = {}
    totals = []
    for ecosystem, lock, reader in readers:
        try:
            licences = reader(tree) if lock in tree.files else {}
        except (ValueError, KeyError, TypeError, AttributeError):
            licences = None
        if licences is None:
            return "error", 0, "a lock file cannot be read"
        if lock in tree.files and not licences:
            return "error", 0, "a lock file names no dependency"
        totals.append(f"{ecosystem}={len(licences)}")
        found.update({f"{ecosystem}:{name}": licence for name, licence in licences.items()})
    if not found:
        return "error", 0, "the lock files name no dependency"
    verdicts = {key: licence_verdict(licence) for key, licence in found.items()}
    recorded = {key: rest.split(None, 1) for key, rest in allowed}
    if any(len(cells) < 2 for cells in recorded.values()):
        return "error", 0, "an allow line has no licence or no reason"
    stale = sum(1 for key in recorded if verdicts.get(key, "ok") == "ok")
    if stale:
        return "error", stale, "an allow line matches no dependency that needs it"
    # An allow line covers the package only while its licence is the recorded one.
    keys = {key for key, cells in recorded.items() if licence_id(found[key]) == cells[0]}
    tally = {v: sum(1 for key, got in verdicts.items() if got == v and key not in keys) for v in ("unknown", "denied")}
    own = own_licence_gaps(tree)
    hits = tally["unknown"] + tally["denied"] + own
    return ("fail" if hits else "pass"), hits, f"{' '.join(totals)} unknown={tally['unknown']} denied={tally['denied']} own={own} allowed={len(keys)}"


GUARD_MARK = b"gh_public_leak_guard"
# The name of the variable that the leak gate of the dotfiles pre-push hook loads. A hook without it is another hook.
HOOK_MARK = b"OSS_READINESS_" + b"INTERNAL_PATTERN"


def s14(tree: Tree) -> Result:
    """Three guards for text that tools post later: the gh text guard, the public push list, the pre-push hook with the leak gate."""
    key = project_key(tree)
    if not key:
        return "error", 0, "no link names the project's GitHub repository"
    gh = shutil.which("gh")
    if not gh:
        return "error", 0, "gh is not installed"
    try:
        with open(gh, "rb") as f:
            guarded = GUARD_MARK in f.read(200_000)
    except OSError:
        guarded = False
    push_list = os.environ.get("DOTFILES_PUBLIC_PUSH_ALLOWLIST") or os.path.join(os.path.dirname(os.path.realpath(gh)), "..", "config", "public-push-remotes.txt")
    try:
        with open(push_list, encoding="utf-8") as f:
            listed = key in {re.sub(r"\s", "", line.split("#")[0]).lower() for line in f}
    except OSError:
        listed = False
    hooks = git("config", "--get", "core.hooksPath", ok=(0, 1)).decode().strip() or os.path.join(git("rev-parse", "--git-common-dir").decode().strip(), "hooks")
    hook = os.path.join(os.path.expanduser(hooks), "pre-push")
    try:
        with open(hook, "rb") as f:
            hooked = os.access(hook, os.X_OK) and HOOK_MARK in f.read(1_000_000)
    except OSError:
        hooked = False
    gaps = [guarded, listed, hooked].count(False)
    return ("fail" if gaps else "pass"), gaps, f"gh-guard={int(guarded)} push-list={int(listed)} pre-push-hook={int(hooked)}"


CLI = ".venv/bin/careerbrew"


def cli(*args: str) -> str | None:
    """The output of the checkout's own CLI. None when it is missing or fails. The caller never prints it."""
    if not os.access(CLI, os.X_OK):
        return None
    r = subprocess.run([CLI, *args], capture_output=True, text=True, check=False)
    return r.stdout if r.returncode == 0 else None


HOME_OVERRIDDEN = "the data home is overridden: run with the default home, or pass --allow-home-override when that is intended"
# Words of a plugin name that say nothing about the company.
GENERIC_WORDS = frozenset("careerbrew plugin plugins corporate corp company work internal private tool tools data team test example sample".split())


def home_kind() -> str:
    """A fixed label for the data folder that the CLI reads: the checkout's own, or one that CAREERBREW_HOME names."""
    return "overridden" if "CAREERBREW_HOME" in os.environ else "default"


def named(words: re.Pattern[bytes], tree: Tree) -> int:
    """The number of files whose path or text holds a word."""
    return sum(1 for path, data in tree.files.items() if words.search(path.lower().encode("utf-8", errors="surrogateescape")) or (is_text(data) and words.search(data.lower())))


def s16(tree: Tree) -> Result:
    listed = cli("plugin", "list")
    if listed is None:
        return "error", 0, "the careerbrew CLI is missing or failed"
    names = []
    for line in listed.splitlines():
        if line.startswith("effective policy:"):
            break
        if line.strip() and line.strip() != "no plugins installed":
            names.append(line.split()[0])
    if not names:
        # An empty list means something only in the owner's data folder, which holds a profile.
        if cli("data", "show", "profile.yaml", "--keys") is None:
            return "unmeasured", 0, "no profile in this data folder: run from the owner's checkout"
        return "pass", 0, f"names=0: the owner's data folder holds no plugin; home={home_kind()}"
    if any(len(name) < 3 for name in names):
        return "error", 0, "a plugin name is shorter than 3 characters"
    # A name is also searched word by word: the company word of a name can stand alone in a file.
    generic = GENERIC_WORDS | {str(tree.project().get("name", "")).lower()}
    words = sorted({w for name in names for w in re.split(r"[-_.]+", name.lower()) if len(w) >= 4 and w not in generic})
    hits = named(whole_words([*names, *words]), tree)
    return ("fail" if hits else "pass"), hits, f"names={len(names)} words={len(words)} home={home_kind()}"


ZONE = re.compile(r"[A-Za-z_]+(?:/[A-Za-z0-9_+-]+)+")
ZONE_TABLES = "/usr/share/zoneinfo"
# Country code of the tz tables -> other forms that people write and that are no common word.
# A form of three letters is searched in capitals only. A form of two letters is searched like the code.
# Each form is written backwards here, so this file never matches its own check.
COUNTRY_FORMS = {
    "US": (
        "ASU",
        ".S.U",
        ".A.S.U",
        "aciremA fo setatS detinU",
        "aciremA",
    ),
    "GB": (
        "RBG",
        "KU",
        ".K.U",
        "modgniK detinU",
        "niatirB taerG",
        "dnalgnE",
        "dnaltocS",
        "selaW",
    ),
    "AU": ("SUA",),
    "NZ": (
        "LZN",
        "aoraetoA",
    ),
    "DE": (
        "UED",
        "dnalhcstueD",
    ),
    "FR": ("ARF",),
    "ES": ("PSE",),
    "IT": ("ATI",),
    "NL": (
        "DLN",
        "dnalloH",
    ),
    "JP": (
        "NPJ",
        "noppiN",
    ),
    "CN": (
        "NHC",
        "CRP",
    ),
    "KR": (
        "ROK",
        "aeroK htuoS",
        "aeroK fo cilbupeR",
    ),
    "RU": (
        "SUR",
        "noitaredeF naissuR",
    ),
    "UA": ("RKU",),
    "PL": ("LOP",),
    "SE": ("EWS",),
    "CH": (
        "EHC",
        "ziewhcS",
        "essiuS",
    ),
    "AT": ("TUA",),
    "BR": (
        "ARB",
        "lisarB",
    ),
    "MX": ("XEM",),
    "AE": (
        "EAU",
        ".E.A.U",
        "setarimE",
    ),
    "IL": ("RSI",),
    "SG": ("PGS",),
    "TR": ("RUT",),
    "PT": ("TRP",),
    "CZ": (
        "EZC",
        "aihcezC",
    ),
    "IN": ("DNI",),
}
COUNTRY_FORMS = {code: tuple(form[::-1] for form in forms) for code, forms in COUNTRY_FORMS.items()}


def owner_place() -> tuple[list[str], str]:
    """The owner's time zone, its city, the country and the country code, and a problem. The zone comes from
    PUBLIC_READINESS_ZONE, then from the `timezone` setting, then from the system. The tz tables give the country."""
    zone = os.environ.get("PUBLIC_READINESS_ZONE", "")
    if not zone:
        for line in (cli("config", "show") or "").splitlines():
            cells = re.split(r"\s{2,}", line.strip())
            if cells[0] == "timezone" and len(cells) > 1 and ZONE.fullmatch(cells[1]):
                zone = cells[1]
    if not zone:
        zone = os.path.realpath("/etc/localtime").partition("zoneinfo/")[2]
    if not ZONE.fullmatch(zone):
        return [], "the time zone names no place"
    try:
        with open(os.path.join(ZONE_TABLES, "zone.tab"), encoding="utf-8") as f:
            codes = [line.split("\t")[0] for line in f if not line.startswith("#") and line.rstrip("\n").split("\t")[2:3] == [zone]]
        with open(os.path.join(ZONE_TABLES, "iso3166.tab"), encoding="utf-8") as f:
            countries = dict(line.rstrip("\n").split("\t")[:2] for line in f if not line.startswith("#") and "\t" in line)
    except OSError:
        return [], "the time zone tables are missing"
    if not codes or codes[0] not in countries:
        return [], "the time zone tables do not name the country of the zone"
    city = zone.rsplit("/", 1)[1].replace("_", " ")
    country = re.sub(r"\s*\(.*?\)", "", countries[codes[0]]).strip()
    if len(city) < 3 or len(country) < 3:
        return [], "the city or the country name is shorter than 3 characters"
    return [zone, city, country, codes[0]], ""


def exact_patterns(words: list[str], codes: list[str]) -> list[re.Pattern[bytes]]:
    """Patterns that keep the letter case. A word of three letters matches in capitals only. A code of two
    letters matches only where the text speaks of a place: after `<Word>, `, as the value of a key such as
    country or location, or in a locale tag such as en_XX."""
    out = [re.compile(rb"(?<![A-Za-z0-9])" + re.escape(word.upper().encode()) + rb"(?![A-Za-z0-9])") for word in words]
    for code in codes:
        c = re.escape(code.upper().encode()) + rb"(?![A-Za-z0-9])"
        key = rb"(?i:country|region|location|locale|nationality|based[ _]in)[A-Za-z_ -]{0,12}[\"'=:\s]{1,4}"
        out.append(re.compile(rb"[A-Za-z]{2,}, " + c + rb"|" + key + c + rb"|(?<![A-Za-z0-9])[a-z]{2}[-_]" + c))
    return out


def folded(text: str) -> str:
    """Text for a comparison in any letter case, with non-ASCII letters too."""
    return unicodedata.normalize("NFC", text).casefold()


def any_case(terms: list[str]) -> re.Pattern[str]:
    """Match any term as a whole word in folded text. A space in a term also matches `_`, `-` and nothing (the joined form)."""
    parts = [re.escape(folded(t)).replace(r"\ ", "[ _-]?") for t in terms]
    return re.compile(r"(?<![^\W_])(?:" + "|".join(parts) + r")(?![^\W_])")


def s20(tree: Tree) -> Result:
    source = "env" if os.environ.get("PUBLIC_READINESS_ZONE") else "machine"
    place, problem = owner_place()
    if problem:
        return "error", 0, problem
    zone, city, country, code = place
    # More words of the owner's place, for this run only: a real city, a region, a nickname. Never printed.
    extra = [w.strip() for w in os.environ.get("PUBLIC_READINESS_PLACE_TERMS", "").split(",") if w.strip()]
    if any(len(w) < 3 for w in extra):
        return "error", 0, "a place term is shorter than 3 characters"
    forms = COUNTRY_FORMS.get(code, ())
    # The owner's own terms and every long word match in any letter case. A short form from the table keeps its case.
    wide = any_case([zone, city, country, *(f for f in forms if len(f) > 3), *extra])
    exact = exact_patterns([f for f in forms if len(f) == 3], [code, *(f for f in forms if len(f) == 2)])
    hits = 0
    for path, data in tree.files.items():
        name = path.encode("utf-8", errors="surrogateescape")
        text = data if is_text(data) else b""
        hits += bool(wide.search(folded(path)) or wide.search(folded(text.decode("utf-8", errors="replace"))) or any(p.search(name) or p.search(text) for p in exact))
    return ("fail" if hits else "pass"), hits, f"zone={source} extra={len(extra)} home={home_kind()}"


# Rule name -> pattern. A match counts unless `identifier_safe` clears it by rule, or an
# `S22 <path> <rule>:<match> <reason>` line of the allowlist covers that exact text in that file.
# The parts are split so this file does not match itself.
HEX = rb"[0-9A-Fa-f]"
HOST_END = rb"(?:loc" + rb"al|l" + rb"an|inter" + rb"nal|ts\.n" + rb"et|home\.ar" + rb"pa)"
LABEL_FIRST = ("com", "org", "net", "io", "dev", "me", "app", "co", "ai", "uk")
MAC_WORD = rb"(?<![A-Za-z])(?i:mac|ether|ethernet|hwaddr|hw[ _-]addr|bssid|lladdr)"
# A file ending. After a host ending it means a file such as settings.<ending>.json; after a label it is a log or a list file.
FILE_LAST = frozenset(b"py sh json md yaml yml toml log txt ts tsx js jsx mjs cjs rs lock html css out err bak example sample template env conf cfg ini xml".split())
DOMAIN_LAST = frozenset(b"com org net io dev".split())
# The word before a host ending in code and in file names: threading.<ending>(), .env.<ending>, storage.<ending>.
HOST_CODE = frozenset(b"threading env storage settings config self this cls".split())
# A pattern with a group `t` counts the text of that group: the rest is the context that makes it an identifier.
IDENTIFIERS = {
    # A host name of a private network: bare, inside a URL, after `@`, before a port, a path or a final dot.
    "host": re.compile(rb"(?i)(?<![A-Za-z0-9])[A-Za-z0-9][A-Za-z0-9-]*(?:\.[A-Za-z0-9-]+)*\." + HOST_END + rb"(?![A-Za-z0-9]|_\w*\(|\.(?:" + b"|".join(sorted(FILE_LAST)) + rb")(?![A-Za-z0-9]))"),
    # Private, carrier-grade NAT (100.64/10) and link-local (169.254/16) addresses, also with zero-padded octets.
    "ip": re.compile(rb"(?<![\d.])0*(?:10\.\d{1,3}|192\.0*168|169\.0*254|172\.0*(?:1[6-9]|2\d|3[01])|100\.0*(?:6[4-9]|[7-9]\d|1[01]\d|12[0-7]))\.\d{1,3}\.\d{1,3}(?!\d|\.\d)"),
    # Also after `:` (inet6:<address>), before a final dot, `_` or `(`, and with an IPv4 tail.
    "ip6": re.compile(rb"(?<![\w.])(?<!::)(?:" + HEX + rb"{1,4})?(?::" + HEX + rb"{0,4}){2,7}(?:(?<=:)\d{1,3}(?:\.\d{1,3}){3}|(?:\.\d{1,3}){3})?(?![0-9A-Za-z:]|\.\w)"),
    # Six pairs with `:` or `-` (also after `ether:`), six groups of one or two digits with `:`, the dotted form
    # of three groups, and twelve bare hex digits or a dotted form after a word such as mac or ether.
    "mac": re.compile(
        rb"(?<![0-9A-Fa-f-])(?<!\b[0-9A-Fa-f]{2}[:-])" + HEX + rb"{2}(?P<sep>[:-])" + HEX + rb"{2}(?:(?P=sep)" + HEX + rb"{2}){4}(?![0-9A-Fa-f:-])"
        rb"|(?<![0-9A-Fa-f.-])(?<![0-9A-Fa-f]:)(?<!::)(?:" + HEX + rb"{1,2}:){5}" + HEX + rb"{1,2}(?![0-9A-Fa-f:-])"
        rb"|(?<![\w.])" + HEX + rb"{4}\." + HEX + rb"{4}\." + HEX + rb"{4}(?!\w|\.\w)"
        rb"|" + MAC_WORD + rb"[A-Za-z_ -]{0,12}(?: ?\([^)\n]{0,16}\))?[\"'=:\s]{1,4}[\"']?(?P<t>" + HEX + rb"{12}|\d{4}\.\d{4}\.\d{4})(?![0-9A-Fa-f])"
    ),
    "uuid": re.compile(rb"(?<![0-9A-Fa-f-])" + HEX + rb"{8}-" + HEX + rb"{4}-" + HEX + rb"{4}-" + HEX + rb"{4}-" + HEX + rb"{12}(?![0-9A-Fa-f-])"),
    "label": re.compile(rb"(?<![\w.-])(?:" + "|".join(LABEL_FIRST).encode() + rb")\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+(?:\.[A-Za-z0-9_-]+)*(?![\w-])"),
    "id": re.compile(rb"(?i)(?:project|dashboard|account|workspace|tenant|organi[sz]ation|org|property)(?:[ _-]?id)?[\"'=:/# ]{1,4}\d{5,}"),
    "key": re.compile(rb"\bph[cx]_[A-Za-z0-9]{16,}|\bG-[A-Z0-9]{8,}\b|\bUA-\d{4,}-\d+\b"),
    "phone": re.compile(rb"(?<![\w.+-])(?:\+\d{1,3}[ .-]?)?(?:\(\d{3}\) ?|\d{3}[.-])\d{3}[.-]\d{4}(?![\w-])|(?<![\w.+-])\+\d{1,3}[ .-]?\(?\d{3}\)?[ .-]?\d{3}[ .-]?\d{4}(?!\w)"),
    "notion": re.compile(rb"notion\.(?:so|site)/[^\s)\"'>]*[0-9a-f]{32}"),
    "gdoc": re.compile(rb"(?:docs|drive)\.google\.com/[^\s)\"'>]*/d/[A-Za-z0-9_-]{25,}"),
}
# A job label of any first word counts on a line that speaks of launchd: `<word>.<name>.<name>`.
LAUNCHD_LINE = re.compile(rb"(?i)launchctl|launchd|launchagent|launchdaemon|\.plist|bundle[ _-]?id|<key>Label</key>")
LAUNCHD_LABEL = re.compile(rb"(?<![\w.-])[a-z][a-z0-9-]*(?:\.[A-Za-z0-9_-]+){2,}(?![\w-])")
# The first word of a dotted name in code, and the last word of a file or a public host name. Neither is a job label.
CODE_FIRST = frozenset(b"pytest os sys self this re path json subprocess shutil process console window document std crate super cfg config settings args env www docs api".split())
# A reserved example host: the whole label `example` stands directly before the ending.
SAFE_HOST = re.compile(rb"(?i)(?:[a-z0-9-]+\.)*example\." + HOST_END)
SAFE_NETS = ("2001:db8::/32", "3fff::/20", "::1/128", "::/128")
SAFE_NETS4 = ("127.0.0.0/8", "192.0.2.0/24", "198.51.100.0/24", "203.0.113.0/24", "0.0.0.0/32")
SAFE_UUIDS = frozenset(("123e4567-e89b-12d3-a456-426614174000", "123e4567-e89b-12d3-a456-426655440000"))


def identifier_safe(rule: str, found: bytes, context: bool = False) -> bool:
    """True for a match that the rule itself clears: a reserved example name, a documentation range, an obvious filler.
    `context` says that the text around the match names its kind: a launchd line for a label, a word such as mac for a card."""
    text = found.decode("ascii", errors="replace").lower()
    if rule == "host":
        if SAFE_HOST.fullmatch(found):
            return True
        # `threading.<ending>` and `.env.<ending>` are code and file names: the word before the ending decides.
        labels = HOST_END.join([rb"(.*)\.", rb""])
        before = re.fullmatch(b"(?i)" + labels, found)
        return bool(before) and before[1].rsplit(b".", 1)[-1].lower() in HOST_CODE
    if rule == "ip6":
        import ipaddress

        try:
            address = ipaddress.IPv6Address(text)
        except ValueError:
            return True  # a time, a ratio or a code path: not an address
        if address.ipv4_mapped is not None:
            return any(address.ipv4_mapped in ipaddress.ip_network(net) for net in SAFE_NETS4)
        # Without a digit it is a pair of names in code, such as `add::dec`. So is a float type of Rust with one name.
        return not any(ch.isdigit() for ch in text) or bool(re.fullmatch(r"f(?:32|64)::[a-f]+", text)) or any(address in ipaddress.ip_network(net) for net in SAFE_NETS)
    if rule == "mac":
        groups = re.split("[:-]", text)
        digits = "".join(g.zfill(2) for g in groups) if len(groups) == 6 else text.replace(".", "")
        pairs = [digits[n : n + 2] for n in range(0, 12, 2)]
        if len(set(pairs)) == 1 or pairs[:5] == ["00", "00", "5e", "00", "53"]:
            return True  # filler, or the RFC 7042 documentation range
        if any(len(g) == 1 for g in groups) and len(groups) == 6:
            return digits.isdigit()  # a time or a ratio
        # The dotted form with no digit or no letter is words or a version, unless a word such as mac stands before it.
        return "." in text and not context and (digits.isdigit() or digits.isalpha())
    if rule == "uuid":
        return text in SAFE_UUIDS or len(set(text) - {"-"}) <= 2 or all(len(set(group)) == 1 for group in text.split("-"))
    if rule == "label":
        parts = text.removesuffix(".plist").split(".")
        if "example" in parts:
            return True
        if parts[0] not in LABEL_FIRST and (not context or parts[0].encode() in CODE_FIRST or parts[-1].encode() in DOMAIN_LAST):
            return True  # a dotted name in code, or a public host name
        # A file ending is not a part of the label: `<label>.log` is the label and a log file.
        return len(parts[:-1] if parts[-1].encode() in FILE_LAST else parts) < 3
    if rule == "phone":
        return bool(re.fullmatch(r".*555[ .-]?01\d\d", text))
    return False


def identifiers(data: bytes):
    """Each (rule, matched text) of one file, or of one file name, that no rule clears."""
    for rule, pattern in IDENTIFIERS.items():
        for found in pattern.finditer(data):
            text = found.groupdict().get("t") or found[0]
            if not identifier_safe(rule, text, context=bool(found.groupdict().get("t"))):
                yield rule, text
    known = IDENTIFIERS["label"]
    for line in data.splitlines():
        if LAUNCHD_LINE.search(line):
            for found in LAUNCHD_LABEL.finditer(line):
                if not known.fullmatch(found[0]) and not identifier_safe("label", found[0], context=True):
                    yield "label", found[0]


def s22(tree: Tree) -> Result:
    allowed = rule_lines(tree, "S22")
    if allowed is None:
        return "error", 0, BAD_ALLOW
    # An allow line names one file, so the same text in another file still counts.
    keys = {(path, rest.split(None, 1)[0]) for path, rest in allowed}
    if any(len(rest.split(None, 1)) < 2 or ":" not in rest.split(None, 1)[0] for _path, rest in allowed):
        return "error", 0, "an allow line has no file, no `<rule>:<text>` or no reason"
    used: set[tuple[str, str]] = set()
    tally = dict.fromkeys(IDENTIFIERS, 0)
    for path, data in tree.files.items():
        name = path.encode("utf-8", errors="surrogateescape")
        if path == ALLOW:
            # The allowlist is read like any file, without the one `<rule>:<text>` word of each S22 line.
            data = b"\n".join(b" ".join(w for n, w in enumerate(line.split(b" ")) if n != 2 or not line.startswith(b"S22 ")) for line in data.splitlines())
        for rule, found in [*identifiers(name), *(identifiers(data) if is_text(data) else ())]:
            # A line holds the text itself, or `sha256:` and the hash of the text when a secret scanner would flag the text there.
            forms = {(path, f"{rule}:{found.decode('utf-8', errors='replace')}"), (path, f"{rule}:sha256:{hashlib.sha256(found).hexdigest()}")} & keys
            used |= forms
            tally[rule] += not forms
    if keys - used:
        return "error", len(keys - used), "an allow line matches nothing in its file"
    hits = sum(tally.values())
    return ("fail" if hits else "pass"), hits, " ".join(f"{k}={v}" for k, v in tally.items()) + f" allowed={len(keys)}"


FIRST_RUN_CHECKS = (["scripts/check-docs.sh"],)
FIRST_RUN_FULL = (["scripts/setup.sh"], ["scripts/doctor.sh"], ["scripts/check.sh", "fast"])


def ran(command: list[str], cwd: str, env: dict[str, str], seconds: int) -> bool:
    """True when the command ends with status 0 in time. On a timeout its whole process group is killed."""
    try:
        with subprocess.Popen(command, cwd=cwd, env=env, stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True) as p:
            try:
                return p.wait(timeout=seconds) == 0
            except subprocess.TimeoutExpired:
                return False
            finally:
                # The group outlives its leader when a step leaves a background job. Nothing of a step may stay.
                try:
                    os.killpg(p.pid, signal.SIGKILL)
                except (ProcessLookupError, PermissionError):
                    pass
    except OSError:
        return False


def s17(tree: Tree, full: bool) -> Result:
    """Run the self-checks in a scratch copy of the release tree with a scratch HOME. Only the full run can pass."""
    limit = os.environ.get("PUBLIC_READINESS_STEP_SECONDS", "")
    if limit and not (limit.isdigit() and int(limit) > 0):
        return "error", 0, "PUBLIC_READINESS_STEP_SECONDS is not a positive whole number"
    limit = int(limit or 0)
    commands = [*FIRST_RUN_CHECKS, *(FIRST_RUN_FULL if full else ())]
    absent = sum(1 for command in commands if command[0] not in tree.files)
    if absent:
        return "error", absent, "a first-run command is not in the tree"
    with tempfile.TemporaryDirectory() as tmp:
        copy, home = os.path.join(tmp, "tree"), os.path.join(tmp, "home")
        tree.write(copy)
        os.mkdir(home)
        env = {k: v for k, v in os.environ.items() if not k.startswith(("GIT_", "CAREERBREW_", "PUBLIC_READINESS", "OSS_READINESS", "XDG_")) and k not in ("BASH_ENV", "ENV", "VIRTUAL_ENV")}
        # The only folders of the caller that the steps still see: the Rust toolchain and its registry,
        # so a run does not download them again. Settings, caches and state come from the scratch HOME.
        for name, folder in (("CARGO_HOME", ".cargo"), ("RUSTUP_HOME", ".rustup")):
            kept = os.environ.get(name) or os.path.join(os.path.expanduser("~"), folder)
            if os.path.isdir(kept):
                env[name] = kept
        env.update(HOME=home, CAREERBREW_BIN_DIR=os.path.join(tmp, "bin"))
        env.update({f"XDG_{name}_HOME": os.path.join(home, folder) for name, folder in (("CONFIG", ".config"), ("CACHE", ".cache"), ("DATA", ".local/share"), ("STATE", ".local/state"))})
        # One commit by a scratch identity, with no hook: a real clone always has a commit, and the leak scan needs one.
        commit = ["git", "-c", "user.name=release-check", "-c", "user.email=release-check@example.com", "-c", "core.hooksPath=/dev/null", "-c", "commit.gpgsign=false", "commit", "-q", "-m", "release tree"]
        setup = (["git", "init", "-q"], ["git", "add", "-A"], commit)
        if any(subprocess.run(args, cwd=copy, env=env, capture_output=True, check=False).returncode for args in setup):
            return "error", 0, "the scratch copy could not become a git repository"
        # Each failing command by its place in the run and its script name. Never a word a command printed.
        failed = [f"{place}:{os.path.basename(command[0])}" for place, command in enumerate(commands, 1) if not ran(command, copy, env, limit or (3600 if full else 300))]
    if failed:
        return "fail", len(failed), f"commands={len(commands)} failed={','.join(failed)}"
    if full:
        return "pass", 0, f"commands={len(commands)}"
    return "unmeasured", 0, "self-checks pass; setup, doctor and the fast gate run only with --first-run"


def signatures(tree: Tree) -> dict[str, Result]:
    """Status, findings left and a reason for each review surface."""
    text = tree.text(SIGNATURES)
    body = text[len(SIGNATURES_HEADER) :] if text is not None and text.startswith(SIGNATURES_HEADER) else None
    lines = body.splitlines() if body is not None and (not body or body.endswith("\n")) else None
    entries: dict[str, tuple[int, str]] = {}
    for line in lines or []:
        m = ENTRY.fullmatch(line)
        try:
            datetime.date.fromisoformat(m[3]) if m else None
        except ValueError:
            m = None
        if not m or m[1] not in REVIEW or m[1] in entries:
            lines = None
            break
        entries[m[1]] = (int(m[4]), m[5])
    if lines is None:
        return dict.fromkeys(REVIEW, ("fail", 1, "the signature file is missing or holds other text"))
    want = tree.hash()
    out = {}
    for sid in REVIEW:
        if sid not in entries:
            out[sid] = ("fail", 1, "no signature")
        elif entries[sid][1] != want:
            out[sid] = ("fail", 1, "the signature is for another tree")
        elif entries[sid][0]:
            out[sid] = ("fail", entries[sid][0], "findings left")
        else:
            out[sid] = ("pass", 0, "signed")
    return out


def main() -> int:
    ap = argparse.ArgumentParser(prog="scripts/public-readiness.sh", description="Public-readiness checks for one git ref.")
    ap.add_argument("--release", action="store_true", help="owner mode: also S1, S2 and the review signatures")
    ap.add_argument("--tree", default="HEAD", metavar="<git-ref>", help="the ref to read (default HEAD)")
    ap.add_argument("--tree-hash", action="store_true", help="print the hash of the release tree that a review signature must carry, then stop")
    ap.add_argument("--release-tree-id", action="store_true", help="print the id of the git tree object of the release tree, then stop")
    ap.add_argument("--write-release-tree", metavar="<dir>", help="write the release tree into a new or empty folder, then stop")
    ap.add_argument("--list-dangling", action="store_true", help="print the S21 files, one `<paths>` TAB `<file>` line each, then stop")
    ap.add_argument("--list-binaries", action="store_true", help="print `<path>` TAB `<sha256>` per non-text file of the release tree, then stop")
    ap.add_argument("--first-run", action="store_true", help="S17 also runs setup, doctor and the fast gate in a scratch copy with a scratch HOME (slow)")
    ap.add_argument("--allow-home-override", action="store_true", help="owner mode: CAREERBREW_HOME is set on purpose, so S16 and S20 may read that data folder")
    ap.add_argument("--released", action="store_true", help="the ref is the release commit: it must hold the exclusion list and no file that the list leaves out")
    a = ap.parse_args()
    tree = Tree(a.tree, a.released)
    if a.tree_hash or a.release_tree_id or a.write_release_tree or a.list_dangling or a.list_binaries:
        if tree.problem:
            raise stop(f"{EXCLUDE}: {tree.problem[1]} ({tree.problem[0]})")
        if a.list_dangling:
            for path, count in sorted(dangling(tree).items()):
                print(f"{count}\t{path}")
        if a.list_binaries:
            for path, digest in binaries(tree).items():
                print(f"{path}\t{digest}")
        if a.tree_hash:
            print(tree.hash())
        if a.release_tree_id:
            print(tree.git_tree())
        if a.write_release_tree:
            print(f"public-readiness: wrote {tree.write(a.write_release_tree)} file(s), left out {tree.excluded}")
        return 0

    results: dict[str, Result] = {}
    for sid, _kind, _label in SURFACES:
        if sid in RELEASE_DAY:
            results[sid] = ("release-day", 0, "")
        elif sid in OWNER_ONLY and not a.release:
            results[sid] = ("owner-only", 0, "not run")
        else:
            results[sid] = ("unmeasured", 0, "no check yet")
    results["S3"] = s3(tree, a.release)
    results["S11"] = s11(tree)
    results["S13"] = s13(tree)
    results["S21"] = s21(tree)
    unlisted, inventory = s7_inventory(tree)
    # The inventory is the machine part of a review surface. It can fail S7; it cannot pass it.
    results["S7"] = ("fail", unlisted, inventory) if unlisted else ("unsigned", 0, "inventory complete, review not signed")
    if a.release:
        results["S1"] = s1(tree)
        results["S2"] = s2(tree)
        signed = signatures(tree)
        for sid in REVIEW:
            results[sid] = signed[sid]
        if unlisted:
            results["S7"] = ("fail", unlisted, inventory)
        results["S14"] = s14(tree)
        # S16 and S20 ask the CLI, which reads the data folder. A folder that the caller's shell picked is not trusted silently.
        trusted = home_kind() == "default" or a.allow_home_override
        results["S16"] = s16(tree) if trusted else ("unmeasured", 0, HOME_OVERRIDDEN)
        results["S20"] = s20(tree) if trusted else ("unmeasured", 0, HOME_OVERRIDDEN)
    if not tree.problem:
        # These checks parse or run files of the tree, so they wait for a list that works.
        results["S10"] = s10(tree)
        results["S17"] = s17(tree, a.first_run)
        results["S22"] = s22(tree)
    if tree.problem:
        # An exclusion list that does not work blocks: the tree the checks read is not the release tree.
        for sid in TREE_READERS:
            if results[sid][0] != "owner-only":
                results[sid] = ("error", *tree.problem)

    tally = dict.fromkeys(STATUSES, 0)
    for sid, kind, label in SURFACES:
        status, count, note = results[sid]
        tally[status] += 1
        print(f"{sid} {kind} {status} {count} {label}" + (f" ({note})" if note else ""))
    blocking = ("fail", "error", "unmeasured", "unsigned", "owner-only") if a.release else ("fail", "error")
    ready = not any(tally[s] for s in blocking)
    counts = " ".join(f"{s}={tally[s]}" for s in STATUSES)
    # Public mode cannot say "ready": it does not run the owner checks or read the signatures.
    open_work = any(tally[s] for s in ("unmeasured", "unsigned", "owner-only"))
    word = "not-ready" if not ready else "no-failures" if open_work else "ready"
    print(f"public-readiness: mode={'release' if a.release else 'public'} tree={tree.commit[:12]} {counts} excluded={tree.excluded} result={word}")
    return 0 if ready else 1


if __name__ == "__main__":
    try:
        sys.exit(main())
    except Exception as problem:  # noqa: BLE001
        # No failure ends in a traceback, which could hold a line of a file. The name of the error class is a fixed label.
        print(f"public-readiness: result=error ({type(problem).__name__}: an input could not be read or the script failed)")
        sys.exit(2)
