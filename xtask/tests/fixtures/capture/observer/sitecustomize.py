"""Observe this synthetic Python oracle invocation at its normal module entry."""

import atexit
import hashlib
import importlib.metadata
import json
import os
import subprocess
import sys
from pathlib import Path

control = json.loads(Path(os.environ["CAREERBREW_CAPTURE_INPUT"]).read_text())
fault = control["fault"]
canary = control["canary"]
report_path = Path(os.environ["CAREERBREW_CAPTURE_OBSERVER"])
calls = 0
descendant = None

if fault == "skip":
    os._exit(0)
if fault == "nonzero":
    os._exit(17)

if fault == "wrong-root":
    import shutil

    shadow = Path(os.environ["CAREERBREW_CAPTURE_INPUT"]).parent / "shadow"
    source = Path(os.environ["CAREERBREW_CAPTURE_ROOT"]) / "src" / "careerbrew"
    shutil.copytree(source, shadow / "careerbrew", ignore=shutil.ignore_patterns("__pycache__"))
    sys.path.insert(0, str(shadow))

from careerbrew import actions  # noqa: E402 (observe the skip control before importing the app)

original_catalog = actions.catalog


def observed_catalog():
    global calls
    calls += 1
    rows = original_catalog()
    if fault == "remove-action":
        rows.pop()
    elif fault in {"argument", "terminal-only", "confirmation"}:
        row = next(row for row in rows if row["name"] == "config.set")
        if fault == "argument":
            row["args"][0]["help"] += " synthetic alteration"
        elif fault == "terminal-only":
            row["terminal_only"] = "synthetic changed terminal policy"
        else:
            row["confirm"] = "synthetic changed confirmation"
    elif fault == "action-type":
        rows[0]["ui"] = "true"
    elif fault == "argument-type":
        row = next(row for row in rows if row["args"])
        row["args"][0]["required"] = "true"
    return rows


actions.catalog = observed_catalog

if fault in {"retained-canary", "extra-write"}:
    data = Path(os.environ["CAREERBREW_HOME"])
    (data / "capture-negative.txt").write_text(canary if fault == "retained-canary" else "synthetic extra write")
elif fault == "descendant":
    process = subprocess.Popen(
        [sys.executable, "-B", "-S", "-c", "import time; time.sleep(60)"],
        stdin=subprocess.DEVNULL,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    descendant = process.pid
    Path(os.environ["CAREERBREW_CAPTURE_DESCENDANT"]).write_text(str(descendant))


def file_record(path):
    resolved = Path(path).resolve(strict=True)
    return {"path": str(resolved), "sha256": hashlib.sha256(resolved.read_bytes()).hexdigest()}


def observe_exit():
    if fault == "stdout-canary":
        print(canary)
    elif fault == "stderr-canary":
        print(canary, file=sys.stderr)
    modules = {}
    external = {}
    for name, module in list(sys.modules.items()):
        path = getattr(module, "__file__", None)
        if not path or not Path(path).is_file():
            continue
        record = file_record(path)
        spec = getattr(module, "__spec__", None)
        origin = getattr(spec, "origin", None)
        if name == "careerbrew" or name.startswith("careerbrew.") or (name == "__main__" and getattr(spec, "name", None) == "careerbrew.__main__"):
            record["loader_origin"] = str(Path(origin).resolve(strict=True)) if origin else None
            modules[name] = record
        else:
            external[name] = record
    sites = {}
    for value in sys.path:
        path = Path(value or os.getcwd()).resolve()
        if path.is_dir():
            for pattern in ("*.pth", "*.egg-link", "*.dist-info/METADATA", "*.dist-info/RECORD"):
                for candidate in path.glob(pattern):
                    sites[str(candidate.resolve())] = file_record(candidate)["sha256"]
    config = Path(sys.prefix) / "pyvenv.cfg"
    if config.is_file():
        sites[str(config.resolve())] = file_record(config)["sha256"]
    distributions = sorted(
        ({"name": distribution.metadata.get("Name"), "version": distribution.version} for distribution in importlib.metadata.distributions()),
        key=lambda item: (item["name"] or "", item["version"]),
    )
    report = {
        "catalog_calls": calls,
        "modules": modules,
        "external_modules": external,
        "sys_path": sys.path,
        "prefix": sys.prefix,
        "base_prefix": sys.base_prefix,
        "site_inputs": sites,
        "distributions": distributions,
        "descendant": descendant,
        "dependency_scope": "application-loaded modules at exit-hook entry and installed distribution/site metadata",
        "unresolved": ["unloaded dependency file content", "transient writes", "writes outside retained owned roots"],
    }
    report_path.write_text(json.dumps(report, sort_keys=True))


atexit.register(observe_exit)
