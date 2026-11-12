#!/usr/bin/env python3
"""Select Python test modules for local iteration from Git changes."""

from __future__ import annotations

import argparse
import ast
import os
import shlex
import signal
import stat
import subprocess
import sys
import time
from fnmatch import fnmatchcase
from pathlib import Path, PurePosixPath


def git(cwd: Path, *args: str) -> bytes:
    env = {key: value for key, value in os.environ.items() if not key.startswith("GIT_")}
    env.update(GIT_CONFIG_NOSYSTEM="1", GIT_OPTIONAL_LOCKS="0", GIT_TERMINAL_PROMPT="0")
    return subprocess.run(
        ["git", "-c", "core.fsmonitor=false", "-c", "core.hooksPath=/dev/null", *args],
        cwd=cwd,
        env=env,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        check=True,
    ).stdout


def paths(raw: bytes) -> set[str]:
    if not raw:
        return set()
    if not raw.endswith(b"\0"):
        raise ValueError("invalid Git pathname output")
    return {os.fsdecode(path) for path in raw[:-1].split(b"\0")}


def changed(root: Path, ref: str | None) -> set[str]:
    diff = ("diff", "--name-only", "-z", "--no-renames", "--no-ext-diff", "--no-textconv")
    result = paths(git(root, *diff, "--cached", "--"))
    result |= paths(git(root, *diff, "--"))
    result |= paths(git(root, "ls-files", "-z", "--others", "--exclude-standard"))
    if ref is not None:
        commit = git(root, "rev-parse", "--verify", "--end-of-options", ref + "^{commit}").strip().decode("ascii")
        result |= paths(git(root, *diff, commit, "HEAD", "--"))
    return result


def public_parts(path: str) -> list[str]:
    parts = path.split("/")
    if any(part in ("", ".", "..", ".git", "private") for part in parts):
        raise ValueError("path is outside public source")
    return parts


def file_info(root: Path, path: str, *, read: bool = False) -> bytes | None:
    """Open only regular files below a chain of real public directories."""
    *parents, name = public_parts(path)
    flags = os.O_RDONLY | os.O_DIRECTORY | os.O_NOFOLLOW
    folder = os.open(root, flags)
    try:
        for part in parents:
            child = os.open(part, flags, dir_fd=folder)
            os.close(folder)
            folder = child
        info = os.stat(name, dir_fd=folder, follow_symlinks=False)
        if not stat.S_ISREG(info.st_mode):
            raise ValueError("path is not a regular file")
        if not read:
            return None
        fd = os.open(name, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK, dir_fd=folder)
        with os.fdopen(fd, "rb") as source:
            if not stat.S_ISREG(os.fstat(source.fileno()).st_mode):
                raise ValueError("path is not a regular file")
            return source.read()
    finally:
        os.close(folder)


def is_test(path: str) -> bool:
    name = PurePosixPath(path).name
    return path.startswith("tests/") and name.startswith("test_") and name.endswith(".py")


def joined_parts(node: ast.expr) -> list[str] | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return [node.value]
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
        left, right = joined_parts(node.left), joined_parts(node.right)
        if right is not None and (left is not None or isinstance(node.left, (ast.Name, ast.Attribute))):
            return (left or []) + right
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id in {"Path", "PurePath", "PurePosixPath"} and not node.keywords:
        parts = [joined_parts(arg) for arg in node.args]
        if all(part is not None for part in parts):
            return [piece for part in parts if part is not None for piece in part]
    return None


def references(source: bytes, filename: str) -> tuple[set[str], set[str]]:
    literals: set[str] = set()
    imports: set[str] = set()
    if filename.endswith(".sh"):
        tokens = shlex.shlex(source.decode("utf-8"), posix=True, punctuation_chars=True)
        tokens.whitespace_split = True
        for token in tokens:
            explicit_root = token.startswith(("./", "$ROOT/", "${ROOT}/"))
            for prefix in ("./", "$ROOT/", "${ROOT}/"):
                if token.startswith(prefix):
                    token = token.removeprefix(prefix)
                    break
            if token and (explicit_root or "/" in token) and not any(char in token for char in "$`"):
                literals.add(token.rstrip("/"))
        return literals, imports
    tree = ast.parse(source, filename=filename)
    nodes = list(ast.walk(tree))
    glob_args = set()
    for node in nodes:
        if not (isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute) and node.func.attr in {"glob", "rglob", "iglob"} and node.args):
            continue
        arg = node.args[0]
        if not isinstance(arg, ast.Constant) or not isinstance(arg.value, str):
            continue
        glob_args.add(arg)
        receiver = node.func.value
        base = joined_parts(receiver)
        if base is None:
            base = [] if isinstance(receiver, ast.Name) and receiver.id in {"ROOT", "glob"} else ["**"]
        if node.func.attr == "rglob":
            base = [*base, "**"]
        literals.add("/".join([*base, arg.value]).removeprefix("./").rstrip("/"))
    for node in nodes:
        if isinstance(node, ast.Import):
            imports.update(alias.name for alias in node.names)
        elif isinstance(node, ast.ImportFrom) and node.level == 0 and node.module:
            imports.add(node.module)
            imports.update(f"{node.module}.{alias.name}" for alias in node.names)
        if isinstance(node, (ast.Constant, ast.BinOp, ast.Call)) and node not in glob_args:
            parts = joined_parts(node)
            if parts is not None:
                literals.add("/".join(parts).removeprefix("./").rstrip("/"))
    return literals, imports


def glob_matches(path: str, pattern: str) -> bool:
    parts = path.split("/")
    positions = {0}
    for segment in pattern.split("/"):
        if segment == "**":
            positions = set(range(min(positions), len(parts) + 1))
        else:
            positions = {index + 1 for index in positions if index < len(parts) and fnmatchcase(parts[index], segment)}
        if not positions:
            return False
    return len(parts) in positions


def mentions(refs: tuple[set[str], set[str]], path: str) -> bool:
    literals, imports = refs
    targets = {path}
    parts = path.split("/")
    if len(parts) > 2 and parts[0] == "skills":
        targets.add("/".join(parts[:2]))
    if path.startswith("scripts/") and path.endswith(".py"):
        module = path[:-3].replace("/", ".")
        if module in imports or module.removeprefix("scripts.") in imports:
            return True
    return any(target in literals or any(glob_matches(target, pattern) for pattern in literals if any(c in pattern for c in "*?[")) for target in targets)


def select(root: Path, edits: set[str]) -> list[str]:
    if not edits:
        return []
    listed = paths(git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard", "--", "tests", "scripts"))
    modules: set[str] = set()
    unsafe_test = False
    for path in sorted(listed):
        if is_test(path):
            try:
                file_info(root, path)
            except (OSError, ValueError):
                unsafe_test = True
            else:
                modules.add(path)
    all_tests = sorted(modules)
    selected = edits & modules
    remaining = edits - modules
    if not remaining:
        return sorted(selected)
    if unsafe_test or any(path.startswith("tests/") for path in remaining):
        return all_tests
    try:
        for path in remaining:
            public_parts(path)
            try:
                file_info(root, path)
            except FileNotFoundError:
                pass  # A removed non-test path can still have literal readers.
        scripts = {path for path in listed if path.startswith("scripts/") and path.endswith((".py", ".sh"))}
        lookup = {path: references(file_info(root, path, read=True) or b"", path) for path in sorted(modules | scripts)}
    except (OSError, ValueError, SyntaxError, RecursionError):
        return all_tests
    for path in remaining:
        dependencies = {path}
        while True:
            readers = {script for script in scripts - dependencies if any(mentions(lookup[script], dep) for dep in dependencies)}
            if not readers:
                break
            dependencies.update(readers)
        covering = {test for test in modules if any(mentions(lookup[test], dep) for dep in dependencies)}
        if not covering and PurePosixPath(path).suffix.lower() not in (".md", ".markdown"):
            return all_tests
        selected.update(covering)
    return sorted(selected)


def run_tests(root: Path, modules: list[str]) -> int:
    if not modules:
        return 0
    startup = ("conftest.py", "pytest.ini", ".pytest.ini", "pytest.toml", ".pytest.toml", "pyproject.toml", "tox.ini", "setup.cfg")
    for module in modules:
        file_info(root, module)
        for parent in (root / module).parents:
            for name in startup:
                if (parent / name).is_symlink():
                    raise ValueError("pytest startup path is a symlink")
    pending = iter(modules)
    active: dict[subprocess.Popen, int | None] = {}
    cancelled = False
    failed = False

    def cancel(_number: int, _frame: object) -> None:
        nonlocal cancelled
        cancelled = True

    handlers = {kind: signal.signal(kind, cancel) for kind in (signal.SIGINT, signal.SIGTERM)}
    try:
        exhausted = False
        while not cancelled and (active or not exhausted):
            while not cancelled and len(active) < 4 and not exhausted:
                module = next(pending, None)
                if module is None:
                    exhausted = True
                    break
                child = subprocess.Popen(
                    [sys.executable, "-I", "-B", "-m", "pytest", "-q", "--confcutdir", str(root), module],
                    cwd=root,
                    start_new_session=True,
                )
                active[child] = None
                try:
                    group = os.getpgid(child.pid)
                    if group == child.pid and group != os.getpgrp():
                        active[child] = group
                except ProcessLookupError:
                    pass
            for child in tuple(active):
                if cancelled:
                    break
                code = child.poll()
                if code is not None:
                    failed |= code != 0
                    del active[child]
            if active and not cancelled:
                time.sleep(0.02)
    finally:
        try:
            if active:
                stop_children(active)
        finally:
            for kind, handler in handlers.items():
                signal.signal(kind, handler)
    return 130 if cancelled else int(failed)


def stop_children(active: dict[subprocess.Popen, int | None]) -> None:
    # Keep each leader unreaped through group signalling, so its PID cannot
    # be reused as another group's target during cancellation.
    failed = False
    for kind in (signal.SIGTERM, signal.SIGKILL):
        for child, group in active.items():
            if child.returncode is not None:
                continue
            try:
                if group is None:
                    os.kill(child.pid, kind)
                else:
                    os.killpg(group, kind)
            except ProcessLookupError:
                pass
            except OSError:
                try:
                    os.kill(child.pid, kind)
                except ProcessLookupError:
                    pass
                except OSError:
                    failed = True
        if kind == signal.SIGTERM:
            time.sleep(0.5)
    for child in active:
        try:
            child.wait(timeout=3)
        except (OSError, subprocess.TimeoutExpired):
            failed = True
    if failed:
        raise OSError("could not clean up all selected test children")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("ref", nargs="?", help="also include committed changes since this ref")
    parser.add_argument("--run", action="store_true", help="run separate pytest processes in a trusted checkout")
    options = parser.parse_args(argv)
    try:
        raw_root = git(Path.cwd(), "rev-parse", "--show-toplevel").removesuffix(b"\n")
        root = Path(os.fsdecode(raw_root)).resolve(strict=True)
        modules = select(root, changed(root, options.ref))
    except (OSError, ValueError, subprocess.CalledProcessError):
        print("tests_affected: could not read repository changes or resolve the requested ref", file=sys.stderr)
        return 2
    if options.run:
        try:
            return run_tests(root, modules)
        except (OSError, ValueError, subprocess.SubprocessError):
            print("tests_affected: could not start or clean up selected test processes", file=sys.stderr)
            return 2
    for module in modules:
        print(module)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
