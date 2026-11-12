#!/usr/bin/env python3
"""Opt-in manifest readiness; invocation scope, no parser imports or owner data."""

import hashlib
import json
import re
import shutil
import subprocess
from pathlib import Path

PACKAGES = {"parse5": "8.1.0", "css-tree": "3.2.1", "data-urls": "8.0.0"}
LOCK_SHA256 = "de942aeed26ffd7b98780192cf89bc2c36addd1588533bbd1284c8e2041d501a"


def check(root: Path, version: str | None) -> str | None:
    match = re.fullmatch(r"v(\d+)\.(\d+)\.(\d+)", version or "")
    if not match or tuple(map(int, match.groups())) < (24, 15, 0) or int(match[1]) != 24:
        return "node-version"
    tools = root / "evals/fixtures/artifact-proof/source-preflight-v3-tools"
    try:
        for path in (tools, tools / "package.json", tools / "package-lock.json", tools / "node_modules"):
            if path.is_symlink():
                return "shared-tools"
        manifest = json.loads((tools / "package.json").read_text())
        if not isinstance(manifest, dict):
            return "manifest"
        raw_lock = (tools / "package-lock.json").read_bytes()
        if hashlib.sha256(raw_lock).hexdigest() != LOCK_SHA256:
            return "lock-identity"
        lock = json.loads(raw_lock)
        if manifest.get("private") is not True or manifest.get("type") != "module" or manifest.get("devDependencies") != PACKAGES:
            return "manifest"
        if manifest.get("dependencies") or manifest.get("scripts") or manifest.get("packageManager") != "npm@12.2.1":
            return "manifest"
        if manifest.get("engines") != {"node": ">=24.15.0 <25", "npm": "12.2.1"}:
            return "toolchain-policy"
        if lock["packages"][""]["devDependencies"] != PACKAGES:
            return "lock-manifest"
        for name, record in lock["packages"].items():
            if not name:
                continue
            path = Path(name)
            if path.is_absolute() or ".." in path.parts or path.parts[0] != "node_modules":
                return "lock-path"
            current = tools
            for part in path.parts:
                current = current / part
                if current.is_symlink():
                    return "shared-tools"
            package = current / "package.json"
            if package.is_symlink():
                return "shared-tools"
            package_metadata = json.loads(package.read_text())
            if not isinstance(package_metadata, dict) or package_metadata.get("version") != record["version"]:
                return "package-version"
    except (OSError, ValueError, KeyError, TypeError):
        return "tooling-metadata"
    return None


def main() -> int:
    node = shutil.which("node")
    version = None
    if node:
        try:
            result = subprocess.run([node, "--version"], capture_output=True, text=True, timeout=5, check=False)
            if result.returncode == 0:
                version = result.stdout.strip()
        except (OSError, subprocess.SubprocessError):
            pass
    reason = check(Path(__file__).resolve().parent.parent, version)
    if reason:
        print(f"FAIL artifact preflight tooling ({reason}) -> fix: select Node 24.15+ below 25 and install the reviewed isolated tools lock with npm ci --ignore-scripts")
        return 1
    print("ok artifact preflight tooling manifests (no parser import or source qualification)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
