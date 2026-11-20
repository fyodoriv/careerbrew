"""rust-04 Python oracle: run privacy, policy and approval vectors with careerbrew's own code.

Usage:
  python -I tests/oracles/rust04.py unicode-version
  python -I tests/oracles/rust04.py <set> <vectors-file>
  python -I tests/oracles/rust04.py --all [<vectors-file>]

`unicode-version` prints `unicodedata.unidata_version` alone (the
vector-driver probe; the Rust driver must print the same).
A set run prints one JSON line,
{"set": ..., "unidata_version": ..., "results": [{"id": ..., "output": {"ok": value} | {"error": {...}}}]}.
`--all` runs every set (from the file, or from the generator in memory when
no file is given) and prints one content-free summary line: the set names,
the case count, the Unicode table version and a SHA-256 of all results.
`cargo xtask migration baseline --step rust-04` runs `--all`.

Rules (docs/rust-migration.md, rust-04 "Vector contract"):
- Inputs come only from the vectors file; no clock, randomness, data folder,
  network or git. Store, event, clock and id hooks are replaced in-process
  for the duration of one case, so the real functions run on supplied data.
- No output holds term text. Matched text is reported by length only, term
  patterns by hash only, filtered terms by `privacy.term_id`. Every output is
  also checked against every term pattern; a hit is exit 3 with the case id.
- Output uses `json.dumps` defaults (the CLI's form), so both drivers can
  compare bytes.
"""

from __future__ import annotations

import contextlib
import datetime
import hashlib
import importlib.util
import json
import sys
import types
import unicodedata
from collections.abc import Callable, Iterator
from pathlib import Path
from typing import Any

from careerbrew import approvals, privacy
from careerbrew.work import access

ECHO_EXIT = 3


def _generator() -> types.ModuleType:
    """The sibling generator module, loaded by path (`-I` leaves the script folder off sys.path)."""
    path = Path(__file__).resolve().with_name("rust04_vectors.py")
    spec = importlib.util.spec_from_file_location("careerbrew_rust04_vectors", path)
    if spec is None or spec.loader is None:
        raise SystemExit("cannot load the vector generator")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


def _sha(text: str) -> str:
    return hashlib.sha256(text.encode("utf-8")).hexdigest()


@contextlib.contextmanager
def _patched(module: Any, **attrs: Any) -> Iterator[None]:
    old = {name: getattr(module, name) for name in attrs}
    for name, value in attrs.items():
        setattr(module, name, value)
    try:
        yield
    finally:
        for name, value in old.items():
            setattr(module, name, value)


def _error(exc: BaseException) -> dict[str, Any]:
    """A refusal: the exception class and its fixed message (messages hold ids, never term text)."""
    return {"class": type(exc).__name__, "message": str(exc)}


# --- canary sets -----------------------------------------------------------


def _canary_core(case: dict[str, Any]) -> Any:
    op = case["op"]
    if op == "sensitivities":
        return list(privacy.SENSITIVITIES)
    if op == "term-id":
        return privacy.term_id(case["term"])
    if op == "normalize":
        return privacy.normalize(case["text"])
    if op == "term-pattern":
        pattern = privacy.term_pattern(case["term"])
        return {"sha256": _sha(pattern.pattern), "flags": int(pattern.flags)}
    raise ValueError(f"unknown op {op}")


def _canary_match(case: dict[str, Any]) -> Any:
    return privacy.find_leaks(case["text"], case["terms"])


def _leak_spans(case: dict[str, Any]) -> Any:
    # The matched text is for the owner's eyes only: report its length.
    return [[tid, line, how, len(got)] for tid, line, how, got in privacy.leak_spans(case["text"], case["terms"])]


def _assert_redact(case: dict[str, Any]) -> Any:
    if case["op"] == "assert-clean":
        privacy.assert_clean(case["text"], case["where"], case["terms"])
        return "clean"
    return privacy.redact(case["text"], case["terms"])


class _FakePath:
    """A data-root stand-in: `root / name` and `.exists()` with supplied text, no file system."""

    def __init__(self, files: dict[str, str], name: str = "") -> None:
        self.files, self.name = files, name

    def __truediv__(self, name: str) -> _FakePath:
        return _FakePath(self.files, name)

    def exists(self) -> bool:
        return self.name in self.files

    def __str__(self) -> str:
        return self.name

    def __fspath__(self) -> str:
        return self.name


def _filter_terms(case: dict[str, Any]) -> list[str]:
    """`privacy.sensitive_terms` over supplied raw inputs: its line, placeholder, length and order rules."""
    files = {"sensitive-terms.txt": case["file"]} if case["file"] else {}
    plugins = []
    for i, text in enumerate(case["plugin_files"]):
        files[f"plugin-{i}.txt"] = text
        plugins.append({"sensitive_terms_path": f"plugin-{i}.txt"})
    root = _FakePath(files)
    fake_store = types.SimpleNamespace(
        read_text=lambda path: files[str(path)],
        read_yaml=lambda path, default=None: case["profile"],
    )
    fake_paths = types.SimpleNamespace(data_root=lambda: root)
    fake_plugins = types.SimpleNamespace(installed=lambda: plugins)
    real_path = privacy.Path

    def path_of(name: Any) -> Any:
        return _FakePath(files, str(name)) if str(name) in files else real_path(name)

    import careerbrew

    # `sensitive_terms` imports plugins locally: replace both the package
    # attribute and the module cache entry for this case only.
    saved = sys.modules.get("careerbrew.plugins")
    with _patched(privacy, store=fake_store, paths=fake_paths, Path=path_of), _patched(careerbrew, plugins=fake_plugins):
        sys.modules["careerbrew.plugins"] = fake_plugins  # type: ignore[assignment]
        try:
            terms = privacy.sensitive_terms()
        finally:
            if saved is None:
                sys.modules.pop("careerbrew.plugins", None)
            else:
                sys.modules["careerbrew.plugins"] = saved
    return [privacy.term_id(t) for t in terms]


def _allowlists(case: dict[str, Any]) -> Any:
    op = case["op"]
    if op == "constants":
        return {
            "template_placeholders": sorted(privacy.TEMPLATE_PLACEHOLDERS),
            "tech_words": sorted(privacy.TECH_WORDS),
            "common_compounds": sorted(privacy.COMMON_COMPOUNDS),
            "common_camel": sorted(privacy.COMMON_CAMEL),
        }
    if op == "filter-terms":
        return _filter_terms(case)
    if op == "identifier-shaped":
        return privacy._identifier_shaped(case["token"])
    if op == "public-suspects":
        return privacy.public_suspects(case["text"])
    raise ValueError(f"unknown op {op}")


def _public_scrub(case: dict[str, Any]) -> Any:
    if "replacement" in case:
        return privacy.scrub_public(case["text"], case["terms"], case["replacement"])
    return privacy.scrub_public(case["text"], case["terms"])


def _private_path(case: dict[str, Any]) -> Any:
    return privacy._names_private_path(case["text"])


# --- approvals -------------------------------------------------------------


def _approvals_call(case: dict[str, Any]) -> Any:
    """Run one approvals function over supplied records, with the store, events, clock and id injected."""
    records = json.loads(json.dumps(case["records"]))
    written: list[Any] = []
    now = datetime.datetime.fromisoformat(case.get("now", "2026-01-01T00:00:00+00:00"))
    new_id = case.get("new_id", "00000000")
    hooks = {
        "load": lambda: records,
        "_path": lambda: "approvals.json",
        # rust-05b2 lock: the driver touches no file, so the lock is a no-op here.
        "_locked": contextlib.nullcontext,
        "store": types.SimpleNamespace(write_json=lambda path, value: written.append(json.loads(json.dumps(value)))),
        "events": types.SimpleNamespace(emit=lambda *args, **kwargs: None),
        "calendar": types.SimpleNamespace(now_utc=lambda: now),
        "uuid": types.SimpleNamespace(uuid4=lambda: types.SimpleNamespace(hex=new_id + "0" * 24)),
    }
    op = case["op"]
    with _patched(approvals, **hooks):
        if op == "request":
            value: Any = approvals.request(case["title"], case["detail"], case["by"])
        elif op == "decide":
            value = approvals.decide(case["item"], case["decision"], case["note"])
        elif op == "pending":
            value = approvals.pending()
        elif op == "guidance":
            value = approvals.guidance(case["limit"]) if "limit" in case else approvals.guidance()
        elif op == "get":
            value = approvals.get(case["item"])
        elif op == "validate-records":
            validate: Callable[[Any], Any] | None = getattr(approvals, "validate_records", None)
            if validate is None:
                # rust-04b adds the validator; until then the vector records its absence.
                return {"validator": "absent"}
            value = validate(records)
        else:
            raise ValueError(f"unknown op {op}")
    return {"value": value, "written": written}


# --- access ----------------------------------------------------------------


def _access(case: dict[str, Any]) -> Any:
    op = case["op"]
    if op == "blocker":
        return access.blocker(case["text"])
    if op == "owner":
        channel, topic = access.owner(case["text"], case["directory"])
        return {"channel": channel["id"] if channel else None, "topic": topic}
    if op == "route":
        return access.route(case["text"], case["directory"], case["teammate"])
    if op == "topic":
        return access._topic(case["text"], case["channel"])
    if op == "existing":
        return access.existing(case["channel"], case["topic"], case["directory"])
    if op == "mentions":
        return access.mentions(case["text"])
    if op == "judge":
        return access.judge(case["draft"], case["channel"], case["directory"], case["new_thread"])
    if op == "validate-directory":
        validate: Callable[[Any], Any] | None = getattr(access, "validate_directory", None)
        if validate is None:
            return {"validator": "absent"}
        return validate(case["directory"])
    raise ValueError(f"unknown op {op}")


SetFn = Callable[[dict[str, Any]], Any]
SETS: dict[str, SetFn] = {
    "canary-core": _canary_core,
    "canary-match": _canary_match,
    "leak-spans": _leak_spans,
    "assert-redact": _assert_redact,
    "allowlists": _allowlists,
    "public-scrub": _public_scrub,
    "private-path": _private_path,
    "approvals": _approvals_call,
    "receipt-validate": _approvals_call,
    "access": _access,
}


def _run_case(fn: SetFn, case: dict[str, Any]) -> dict[str, Any]:
    try:
        return {"id": case["id"], "output": {"ok": fn(case)}}
    except Exception as exc:  # a refusal is an output; both drivers must match it
        return {"id": case["id"], "output": {"error": _error(exc)}}


class EchoError(RuntimeError):
    pass


def _check_no_echo(set_name: str, results: list[dict[str, Any]], terms: list[str]) -> None:
    """Fail on any output that holds a term, in any form a term pattern matches. Names the case, never the term."""
    patterns = [privacy.term_pattern(t) for t in terms]
    for row in results:
        text = privacy.normalize(json.dumps(row["output"], ensure_ascii=False))
        if any(p.search(text) for p in patterns):
            raise EchoError(f"{set_name}/{row['id']}: output holds term text")


def run_set(name: str, cases: list[dict[str, Any]], terms: list[str]) -> list[dict[str, Any]]:
    if name == "assert-no-echo":
        return [{"id": case["id"], "output": {"ok": {"terms": len(case["terms"]), "checked_by": "every-other-set"}}} for case in cases]
    fn = SETS[name]
    results = [_run_case(fn, case) for case in cases]
    _check_no_echo(name, results, terms)
    return results


def _load(path: str) -> dict[str, Any]:
    data = json.loads(Path(path).read_text(encoding="utf-8"))
    if not isinstance(data, dict) or not isinstance(data.get("sets"), dict) or not isinstance(data.get("terms"), list):
        raise SystemExit("malformed vectors file")
    return data


def main(argv: list[str]) -> int:
    if argv == ["unicode-version"]:
        print(unicodedata.unidata_version)
        return 0
    if argv and argv[0] == "--all" and len(argv) <= 2:
        data = _load(argv[1]) if len(argv) == 2 else _generator().vectors()
        results = {}
        try:
            for name, cases in data["sets"].items():
                if name != "assert-no-echo" and name not in SETS:
                    print(f"unknown set: {name}", file=sys.stderr)
                    return 2
                results[name] = run_set(name, cases, data["terms"])
        except EchoError as exc:
            print(str(exc), file=sys.stderr)
            return ECHO_EXIT
        print(
            json.dumps(
                {
                    "sets": sorted(results),
                    "cases": sum(len(rows) for rows in results.values()),
                    "unidata_version": unicodedata.unidata_version,
                    # Key order differs between the in-memory and file forms; the digest does not.
                    "results_sha256": _sha(json.dumps(results, sort_keys=True)),
                }
            )
        )
        return 0
    if len(argv) != 2:
        print("usage: rust04.py unicode-version | <set> <vectors-file> | --all [<vectors-file>]", file=sys.stderr)
        return 2
    name, path = argv
    data = _load(path)
    if name not in data["sets"] or (name != "assert-no-echo" and name not in SETS):
        print(f"unknown set: {name}", file=sys.stderr)
        return 2
    try:
        rows = run_set(name, data["sets"][name], data["terms"])
    except EchoError as exc:
        print(str(exc), file=sys.stderr)
        return ECHO_EXIT
    print(json.dumps({"set": name, "unidata_version": unicodedata.unidata_version, "results": rows}))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
