"""Leak prevention for personal and business data.

Three layers:
1. Sensitive terms: the user's own list (`private/sensitive-terms.txt`), terms
   from the profile (name, employer), and each installed plugin's term file.
   The terms themselves live only in the data root.
2. Guards: `assert_clean` refuses any text that contains a term. Model routing
   (models.py) and every exporter call it before data leaves the data root.
3. Repo scan: `scan_repo` checks staged blobs and tracked working text (wired
   into scripts/leak-check.sh and lefthook). Supported visual snapshots get a
   bounded byte scan, not OCR; pushed history is a separate boundary.
   `scan_files` checks drafts (an issue an agent files) before they are posted.

Reports never print a term. They print a short hash so the user can find it.
"""

from __future__ import annotations

import hashlib
import os
import re
import struct
import subprocess
import unicodedata
import zlib
from dataclasses import dataclass, field
from pathlib import Path

from . import paths, store

PUBLIC = "public"
PERSONAL = "personal"
BUSINESS = "business"
SENSITIVITIES = (PUBLIC, PERSONAL, BUSINESS)


class PrivacyError(RuntimeError):
    """Raised when data would leave its allowed boundary."""


def term_id(term: str) -> str:
    return "t-" + hashlib.sha256(term.lower().encode()).hexdigest()[:8]


def _read_terms(path: Path) -> list[str]:
    if not path.exists():
        return []
    out = []
    for line in (store.read_text(path) or "").splitlines():
        line = line.strip()
        if line and not line.startswith("#") and len(line) >= 3:
            out.append(line)
    return out


TEMPLATE_PLACEHOLDERS = {"Your Name", "Your Employer"}  # unedited example values; the repo itself contains them


def sensitive_terms() -> list[str]:
    root = paths.data_root()
    terms = _read_terms(root / "sensitive-terms.txt")
    profile = store.read_yaml(root / "profile.yaml", {}) or {}
    for key in ("name", "employer", "email"):
        value = profile.get(key)
        if isinstance(value, str) and len(value) >= 3 and value not in TEMPLATE_PLACEHOLDERS:
            terms.append(value)
    for extra in profile.get("sensitive", []) or []:
        if isinstance(extra, str) and len(extra) >= 3:
            terms.append(extra)
    from . import plugins  # local import: plugins imports privacy

    for plugin in plugins.installed():
        terms_file = plugin.get("sensitive_terms_path")
        if terms_file:
            terms.extend(_read_terms(Path(terms_file)))
    # Longest first so redaction removes whole phrases before their parts.
    return sorted(set(terms), key=len, reverse=True)


def find_leaks(text: str, terms: list[str] | None = None) -> list[str]:
    """Return the ids (never the values) of terms found in text."""
    terms = sensitive_terms() if terms is None else terms
    norm = normalize(text)
    return [term_id(t) for t in terms if term_pattern(t).search(norm)]


def leak_spans(text: str, terms: list[str] | None = None) -> list[tuple[str, int, str, str]]:
    """Each match as (term id, line, how it matched, matched text). The text is for the owner's eyes only (`--show`)."""
    terms = sensitive_terms() if terms is None else terms
    norm = normalize(text)
    out = []
    for t in terms:
        for m in term_pattern(t).finditer(norm):
            out.append((term_id(t), norm.count("\n", 0, m.start()) + 1, _match_kind(norm, m, t), m.group(0)))
    return sorted(out, key=lambda x: (x[1], x[0]))


def _match_kind(text: str, m: re.Match[str], term: str) -> str:
    got, core = m.group(0), term.strip()
    if "\n" in got:
        return f"letters spread over {got.count(chr(10)) + 1} lines, likely a false match"
    words = len(got.split())
    if words > len(core.split()):
        return f"letters spread across {words} words, likely a false match"
    if (m.start() > 0 and text[m.start() - 1].isalnum()) or (m.end() < len(text) and text[m.end()].isalnum()):
        return "inside a longer word, likely a false match"
    return "whole word" if got.lower() == core.lower() else "look-alike spelling"


def assert_clean(text: str, where: str, terms: list[str] | None = None) -> None:
    leaks = find_leaks(text, terms)
    if leaks:
        raise PrivacyError(f"blocked: {len(leaks)} sensitive term(s) would leave the data root via {where}: {', '.join(leaks)}")


def redact(text: str, terms: list[str] | None = None) -> str:
    terms = sensitive_terms() if terms is None else terms
    if any(term_pattern(t).search(normalize(text)) for t in terms):
        text = normalize(text)
        for t in terms:
            text = term_pattern(t).sub("[REDACTED]", text)
    return text


_ZERO_WIDTH = re.compile("[\u200b-\u200d\u2060\ufeff]")
_SEP = r"[\s_\-.]*"


def normalize(text: str) -> str:
    """NFKC (fullwidth -> ASCII) and no zero-width characters, so look-alike codenames still match."""
    return _ZERO_WIDTH.sub("", unicodedata.normalize("NFKC", text))


_LEET = {"o": "[o0]", "i": "[i1l!]", "l": "[l1i]", "e": "[e3]", "a": "[a4@]", "s": "[s5$]", "t": "[t7]", "b": "[b8]", "g": "[g9]"}


def _char(ch: str) -> str:
    return _LEET.get(ch.lower(), re.escape(ch))


def term_pattern(term: str) -> re.Pattern[str]:
    """A term also matches with separators between letters and leetspeak digits (5+ chars)."""
    core = normalize(term)
    body = _SEP.join(_char(ch) for ch in core if not ch.isspace()) if len(core) >= 5 else re.escape(core)
    return re.compile(body, re.IGNORECASE)


PUBLIC_PATTERNS = [
    re.compile(r"(?:https?|hxxps?|ftp)s?:/+\S*|mailto:\S*", re.IGNORECASE),  # URLs and remnants
    re.compile(r"[\w.+-]+@[\w-]+(\.[\w-]+)+"),  # email addresses
    re.compile(r"\b\w+\s*(?:\[at\]|\(at\)|\sat\s)\s*\w+(?:\s*(?:\.|\[dot\]|\(dot\)|\sdot\s)\s*\w+)+", re.IGNORECASE),  # obfuscated emails
    re.compile(r"\b[\w-]+/[\w.-]+#\d+\b"),  # owner/repo#45
    re.compile(r"\(?#[\w-]+\)?"),  # #123, (#123), #channel
    re.compile(r"(?<![\w])!\d+\b"),  # GitLab MR !42
    re.compile(r"(?<![\w])@[\w][\w.-]*"),  # @mentions
    re.compile(r"\bAKIA[0-9A-Z]{16}\b|\b[A-Za-z0-9_\-]{32,}\b"),  # cloud keys and long tokens
    re.compile(r"\b(?=[0-9a-f]*\d)(?=[0-9a-f]*[a-f])[0-9a-f]{7,40}\b"),  # commit SHAs
    re.compile(r"\[?[0-9a-f]{0,4}(?::[0-9a-f]{0,4}){2,7}\]?(?::\d+)?", re.IGNORECASE),  # IPv6
    re.compile(r"\b\d{1,3}(?:\.\d{1,3}){3}(?::\d+)?\b"),  # IPv4 with optional port
    re.compile(r"\b[A-Za-z]:\\\S*|\\\\\S+"),  # Windows and UNC paths
    re.compile(r"(?:(?<=\s)|^)(?:~|\.{1,2})?/[\w.-]+(?:/[\w.-]+)*"),  # POSIX paths and URL paths starting a word
    re.compile(r"\bgo/[\w.-]+", re.IGNORECASE),  # go/ short links
    re.compile(r"\b(?:[a-z0-9-]+\.){2,}[a-z]{2,}\b", re.IGNORECASE),  # hostnames with 2+ dots
    re.compile(r"\b[a-z0-9-]+\.(?:com|net|org|io|dev|internal|corp|intra|local|lan|home|test|example|cloud|app|ai|co)\b", re.IGNORECASE),  # domains
    re.compile(r"\b[a-z]+-\d+\b"),  # lowercase ticket keys (zebra-42)
    re.compile(r"\b[A-Z][A-Z0-9]{1,9}-\d+\b"),  # ticket keys (ZBR-7)
    re.compile(r"(?:\+\d{1,3}[\s.-]?)?\(?\d{3}\)?[\s.-]\d{3}[\s.-]\d{4}\b"),  # phone numbers
    re.compile(r"\b\d{1,6}\s+(?:[A-Z][a-z]+\s+){1,3}(?:St|Street|Ave|Avenue|Rd|Road|Blvd|Dr|Drive|Ln|Lane|Way)\b\.?"),  # street addresses
    re.compile(r"\b(?:room|rm|bldg|building|floor|desk)\s+\w+(?:\s+(?:bldg|building|floor)\s+\w+)?", re.IGNORECASE),  # office locations
    re.compile(r"\b(?:employee|badge|staff|personnel)\s+(?:id|number|no\.?|#)\s*\w+", re.IGNORECASE),  # employee ids
]
TECH_WORDS = {"OAuth2", "IPv4", "IPv6", "HTTP2", "HTTP3", "Python3", "ES2015", "ES2020", "UTF-8", "SHA256", "S3", "EC2", "ARM64", "x86_64", "AV1", "H264", "WCAG2"}
_OK_SLASH = re.compile(r"^(?:[A-Za-z]{1,12}(?:/[A-Za-z]{1,12})+|\d+/\d+)$")
_VERSION = re.compile(r"^v?\d+(?:\.\d+)+$")


def _identifier_shaped(token: str) -> bool:
    """Tokens that look like ids, keys, hosts, paths, or resource names, never prose."""
    t = token.strip(".,;:!?()[]{}\"'")
    if not t:
        return False
    if any(x in t for x in ("://", ":/", "@", "\\")) or t.startswith(("/", "~")):
        return True
    if "/" in t and not _OK_SLASH.match(t):
        return True
    if "_" in t:
        return True
    has_digit, has_alpha = any(c.isdigit() for c in t), any(c.isalpha() for c in t)
    if "-" in t and has_digit and has_alpha:
        return True  # k8s-prod-useast1, xoxb-123-abc, sk-ant-abc123
    if has_digit and has_alpha and len(t) >= 6 and not _VERSION.match(t) and t not in TECH_WORDS:
        return True  # U012ABCDEF, abc123def
    return False


COMMON_COMPOUNDS = {
    "front-end",
    "back-end",
    "full-stack",
    "cross-team",
    "open-source",
    "high-impact",
    "low-latency",
    "self-serve",
    "self-service",
    "multi-region",
    "data-driven",
    "large-scale",
    "long-term",
    "short-term",
    "year-over-year",
    "follow-up",
    "user-facing",
    "customer-facing",
}
COMMON_CAMEL = {
    "JavaScript",
    "TypeScript",
    "GitHub",
    "GitLab",
    "PostgreSQL",
    "MySQL",
    "GraphQL",
    "DevOps",
    "OpenTelemetry",
    "LinkedIn",
    "PagerDuty",
    "BigQuery",
    "DynamoDB",
    "MongoDB",
    "CloudFormation",
    "OpenAI",
    "macOS",
    "iOS",
    "iPadOS",
    "NoSQL",
    "FastAPI",
    "PyTorch",
    "TensorFlow",
    "CircleCI",
    "SwiftUI",
    "WebSocket",
    "OAuth",
    "YouTube",
}


def public_suspects(text: str) -> list[str]:
    """Words that may be internal names no rule can detect; the user reviews them before posting."""
    out = set()
    for raw in text.split():
        t = raw.strip(".,;:!?()[]{}\"'")
        parts = t.split("-")
        if len(parts) > 1 and t.lower() not in COMMON_COMPOUNDS and all(p.isalpha() for p in parts) and any(len(p) >= 5 for p in parts):
            out.add(t)
        elif re.search(r"[a-z][A-Z]", t) and t not in COMMON_CAMEL:
            out.add(t)
    return sorted(out)


_DANGLING = re.compile(
    r"(?:\s+|^)(?:and|or|see|to|in|on|at|from|via|with|for|use|ping|mail|thanks|host|server|key|commit|by|ipv6|ran|saved|posted|merged|closed|plus|hit|email|pr|mr|ticket|bucket|cluster|tracked|copied|pushed|deployed|renamed|bot|dm'd|old)\s*(?=[;,.:]|$)",
    re.IGNORECASE,
)


def scrub_public(text: str, terms: list[str] | None = None, replacement: str = "an internal project") -> str:
    """Make text safe to publish: drop URLs, emails, hosts, IPs, paths, refs, ids, and secrets; replace terms."""
    terms = sensitive_terms() if terms is None else terms
    text = normalize(text)
    for t in terms:
        text = re.sub(rf"(?:\b(?:the|a|an)\s+)?\w*(?:{term_pattern(t).pattern})\w*", replacement, text, flags=re.IGNORECASE)
    for pat in PUBLIC_PATTERNS:
        text = pat.sub("", text)
    text = " ".join(tok for tok in text.split() if not _identifier_shaped(tok))
    text = re.sub(r"\(\s*\)|\[\s*\]", "", text)
    text = re.sub(r"\s+", " ", text)
    for _ in range(6):
        before = text
        text = _DANGLING.sub("", text)
        text = re.sub(r"\s*([;,:])\s*(?=[;,.:]|$)", "", text)
        text = re.sub(r"\b(and|or)\s+(and|or)\b", r"\1", text, flags=re.IGNORECASE)
        text = re.sub(r"\s+([.,;:])", r"\1", re.sub(r"\s+", " ", text)).strip(" ;,-:")
        text = re.sub(r"^[.]+|(?<=[\s;,:])[.]+$", "", text).strip()
        if text == before:
            break
    return text


@dataclass
class ScanReport:
    files_scanned: int = 0
    findings: list[tuple[str, str]] = field(default_factory=list)  # (path, term id or reason)
    spans: dict[tuple[str, str], list[tuple[int, str, str]]] = field(default_factory=dict)  # (path, term id) -> (line, how, matched text)
    notes: list[tuple[str, str]] = field(default_factory=list)

    def add_terms(self, path: str, text: str, terms: list[str], source: str = "") -> None:
        for tid, line, how, got in leak_spans(text, terms):
            if (path, tid) not in self.spans:
                self.findings.append((path, tid))
            span = (line, f"{source}: {how}" if source else how, got)
            if span not in self.spans.setdefault((path, tid), []):
                self.spans[(path, tid)].append(span)

    @property
    def ok(self) -> bool:
        return not self.findings


_INDEX_LIMIT = 32 * 1024 * 1024
_STAGED_DIFF = ("diff", "--cached", "--no-ext-diff", "--no-textconv", "--no-renames", "--ignore-submodules=none", "--raw", "--abbrev=64", "--diff-filter=ACMRT", "-z")


def _git(repo: Path, *args: str) -> bytes:
    """Read Git metadata without echoing its output on failure."""
    env = os.environ.copy()
    env["GIT_NO_REPLACE_OBJECTS"] = "1"
    env["GIT_OPTIONAL_LOCKS"] = "0"
    try:
        res = subprocess.run(["git", "-C", str(repo), *args], capture_output=True, check=False, timeout=30, env=env)
    except (OSError, subprocess.TimeoutExpired) as exc:
        raise PrivacyError("could not inspect the Git index") from exc
    if res.returncode != 0:
        raise PrivacyError("could not inspect the Git index")
    return res.stdout


def _index_state(repo: Path) -> tuple[str, tuple[int, int, int, int, int] | None]:
    raw = _git(repo, "rev-parse", "--path-format=absolute", "--git-path", "index")
    if not raw.endswith(b"\n") or b"\0" in raw:
        raise PrivacyError("could not identify the Git index")
    path = os.fsdecode(raw[:-1])
    if not os.path.isabs(path):
        raise PrivacyError("could not identify the Git index")
    try:
        info = os.stat(path)
    except FileNotFoundError:
        return path, None
    return path, (info.st_dev, info.st_ino, info.st_size, info.st_mtime_ns, info.st_ctime_ns)


def _staged_entries(raw: bytes) -> list[tuple[str, str, str]]:
    if not raw:
        return []
    parts = raw.split(b"\0")
    if parts[-1] or len(parts) % 2 != 1:
        raise PrivacyError("could not parse the Git index")
    entries = []
    for metadata, name in zip(parts[0:-1:2], parts[1:-1:2], strict=True):
        fields = metadata.split()
        if len(fields) != 5 or not re.fullmatch(rb":[0-7]{6}", fields[0]) or not re.fullmatch(rb"[0-7]{6}", fields[1]) or not re.fullmatch(rb"[0-9a-f]{40}|[0-9a-f]{64}", fields[3]) or fields[4] not in (b"A", b"C", b"M", b"T") or not name:
            raise PrivacyError("could not parse the Git index")
        rel = os.fsdecode(name)
        if rel.startswith("/") or any(part in ("", ".", "..") for part in rel.split("/")):
            raise PrivacyError("could not parse the Git index")
        entries.append((rel, fields[1].decode("ascii"), fields[3].decode("ascii")))
    return entries


def _staged_blob(repo: Path, mode: str, oid: str) -> bytes:
    if mode not in ("100644", "100755", "120000") or _git(repo, "cat-file", "-t", oid).strip() != b"blob":
        raise PrivacyError("staged entry is not a readable blob")
    size_raw = _git(repo, "cat-file", "-s", oid).strip()
    if not size_raw.isdigit() or int(size_raw) > _INDEX_LIMIT:
        raise PrivacyError("staged blob exceeds the scan limit or has an invalid size")
    data = _git(repo, "cat-file", "blob", oid)
    if len(data) != int(size_raw):
        raise PrivacyError("staged blob changed or could not be read")
    return data


def _visual_snapshot_png(rel: str, data: bytes) -> bytes | None:
    """Admit only the bounded, metadata-free RGB PNG form used by visual tests."""
    if not rel.startswith("web/visual/__screenshots__/") or not rel.endswith(".png") or not data.startswith(b"\x89PNG\r\n\x1a\n"):
        return None
    offset, state, expected = 8, "header", 0
    compressed = bytearray()
    try:
        while offset + 12 <= len(data):
            size = int.from_bytes(data[offset : offset + 4], "big")
            end = offset + 12 + size
            if end > len(data):
                return None
            kind = data[offset + 4 : offset + 8]
            payload = data[offset + 8 : offset + 8 + size]
            checksum = int.from_bytes(data[offset + 8 + size : end], "big")
            if zlib.crc32(kind + payload) != checksum:
                return None
            if state == "header" and kind == b"IHDR" and size == 13:
                width, height, depth, color, compression, filtering, interlace = struct.unpack(">IIBBBBB", payload)
                expected = (width * 3 + 1) * height
                if not width or not height or expected > _INDEX_LIMIT or (depth, color, compression, filtering, interlace) != (8, 2, 0, 0, 0):
                    return None
                state = "image"
            elif state == "image" and kind == b"IDAT":
                compressed.extend(payload)
            elif state == "image" and kind == b"IEND" and not payload and compressed and end == len(data):
                decoder = zlib.decompressobj()
                pixels = decoder.decompress(compressed, expected + 1)
                if len(pixels) != expected or not decoder.eof or decoder.unused_data or decoder.unconsumed_tail:
                    return None
                stride = width * 3 + 1
                return pixels if all(pixels[row * stride] <= 4 for row in range(height)) else None
            else:
                return None
            offset = end
    except (struct.error, zlib.error, OverflowError):
        return None
    return None


def scan_repo(repo: Path | None = None, staged_only: bool = False, terms: list[str] | None = None) -> ScanReport:
    repo = repo or paths.REPO_ROOT
    terms = sensitive_terms() if terms is None else terms
    report = ScanReport()
    scanned: set[str] = set()

    def private_path(rel: str) -> bool:
        if rel.startswith("private/") or "/private/" in rel:
            finding = (rel, "path-under-private")
            if finding not in report.findings:
                report.findings.append(finding)
            return True
        return False

    try:
        index_state = _index_state(repo)
        staged_raw = _git(repo, *_STAGED_DIFF)
        if _git(repo, "ls-files", "--unmerged", "-z"):
            raise PrivacyError("unmerged staged entries cannot be scanned")
        entries = _staged_entries(staged_raw)
        for rel, mode, oid in entries:
            if private_path(rel):
                continue
            data = _staged_blob(repo, mode, oid)
            if rel.startswith("web/visual/__screenshots__/") and rel.endswith(".png"):
                pixels = _visual_snapshot_png(rel, data) if mode == "100644" else None
                if pixels is None:
                    raise PrivacyError("visual snapshot is not a supported PNG")
                for source, content in (("staged snapshot bytes", data), ("staged snapshot pixels", pixels)):
                    report.add_terms(rel, content.decode("utf-8", errors="replace"), terms, source=source)
                    lowered = content.lower()
                    for term in terms:
                        variants = {normalize(term), normalize(term).lower(), normalize(term).upper()}
                        if any(variant.encode(encoding).lower() in lowered for variant in variants for encoding in ("utf-16-le", "utf-16-be")):
                            tid = term_id(term)
                            if (rel, tid) not in report.spans:
                                report.findings.append((rel, tid))
                            span = (1, f"{source}: UTF-16 literal", term)
                            if span not in report.spans.setdefault((rel, tid), []):
                                report.spans[(rel, tid)].append(span)
                scanned.add(rel)
                report.notes.append((rel, "validated visual snapshot; image pixels are not OCR-scanned"))
                continue
            text = data.decode("utf-8")
            scanned.add(rel)
            report.add_terms(rel, text, terms, source="staged")
        if not staged_only:
            raw_files = _git(repo, "ls-files", "-z")
            if raw_files and not raw_files.endswith(b"\0"):
                raise PrivacyError("could not parse tracked files")
            for rel in sorted(os.fsdecode(name) for name in raw_files.split(b"\0") if name):
                if private_path(rel):
                    continue
                path = repo / rel
                if not path.parent.resolve().is_relative_to(repo.resolve()):
                    raise PrivacyError("tracked working path leaves the repository")
                if path.is_symlink():
                    text = os.readlink(path)
                elif path.is_file():
                    try:
                        text = path.read_text(encoding="utf-8")
                    except UnicodeDecodeError:
                        continue
                else:
                    continue
                scanned.add(rel)
                report.add_terms(rel, text, terms, source="working")
        if _git(repo, *_STAGED_DIFF) != staged_raw or _git(repo, "ls-files", "--unmerged", "-z") or _index_state(repo) != index_state:
            raise PrivacyError("the Git index changed during the scan")
    except (OSError, PrivacyError, UnicodeDecodeError):
        report.findings.append(("Git index or working tree", "scan-incomplete"))
    report.files_scanned = len(scanned)
    return report


_PRIVATE_PATH = re.compile(r"(?<![\w.-])private/([\w.-]+)")
_PATH_START = re.compile(r"[\s(\[{<\"'`=:,]")  # a char that can open a path; `>` and `}` close a placeholder such as `<repo>`
_SYSTEM_DIRS = {"tmp", "var", "etc"}  # the macOS system roots under /private


def _names_private_path(text: str) -> bool:
    """A path through a `private/` folder. The bare word `private/` and macOS system paths (`/private/tmp`) do not count."""
    for m in _PRIVATE_PATH.finditer(text):
        i = m.start()
        path_start = i == 1 or (i >= 2 and bool(_PATH_START.match(text[i - 2])))
        system_root = i >= 1 and text[i - 1] == "/" and path_start and m.group(1) in _SYSTEM_DIRS
        if not system_root:
            return True
    return False


def scan_files(files: list[Path], terms: list[str] | None = None) -> ScanReport:
    """Check drafts before they leave the machine: terms, `private/` paths, the data root. Findings hold term ids, never terms."""
    terms = sensitive_terms() if terms is None else terms
    root = paths.data_root().resolve()
    home = str(Path.home())
    root_names = {str(root), str(paths.data_root())}
    root_names |= {"~" + n[len(home) :] for n in root_names if n.startswith(home + "/")}
    report = ScanReport()
    for path in files:
        name = str(path)
        if not path.is_file():
            raise FileNotFoundError(f"privacy scan: no such draft file: {name}")
        if path.resolve().is_relative_to(root):
            report.findings.append((name, "path-under-private"))
            continue
        try:
            text = normalize(path.read_text(encoding="utf-8"))
        except (UnicodeDecodeError, OSError):
            report.findings.append((name, "not-text"))
            continue
        report.files_scanned += 1
        report.add_terms(name, text, terms)
        if _names_private_path(text):
            report.findings.append((name, "mentions-private-path"))
        if any(n in text for n in root_names):
            report.findings.append((name, "mentions-data-root"))
    return report
