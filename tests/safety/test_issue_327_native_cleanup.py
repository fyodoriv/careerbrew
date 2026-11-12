"""Four synthetic cleanup primitives; no native module or real child is run.

Each case owns its process table, clock, capability namespace and evidence.
The actual selected method executes unchanged against those boundaries.
"""

from __future__ import annotations
import __future__

import ast
import builtins
import hashlib
import json
import os
import signal
import socket
import stat
import subprocess
import sys
import tempfile
import types
import webbrowser
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
NATIVE = ROOT / "tests/safety/test_issue_327_native.py"
PID, PARENT = 731327, 731000
ARGV = ["synthetic-git", "fixture-only"]
OUT = b"synthetic-native-cleanup\n"
ERR = b"synthetic-native-cleanup-stderr\n"
OUTSIDE_SHA = "dec72c7c1426534f046cfe8ae725845e5cc009e08ea0a1c291a5308d77d295b6"
FUNCTION_SHA = {"_sha": "d982ec2aec5266e58ceac1397f5b187e6ea1b7ffdd83715d8927341cdd90bbc5", "_json": "ce6c65513abdca48a800bf42a735d1a44f2d81ad8352fe6b272c860f81ff995f"}


class DriverBlock(RuntimeError):
    """An unavailable primitive is never an expected safety assertion failure."""


def require(condition: bool, reason: str) -> None:
    if not condition:
        raise DriverBlock(reason)


def digest(raw: bytes) -> str:
    return hashlib.sha256(raw).hexdigest()


def retain(path: Path, value: object) -> None:
    raw = (json.dumps(value, sort_keys=True, indent=2) + "\n").encode()
    with path.open("xb") as stream:
        stream.write(raw)
        stream.flush()
        os.fsync(stream.fileno())
    require(path.read_bytes() == raw, "driver-evidence-readback")


def snapshot(root: Path) -> dict:
    return {str(p.relative_to(root)): {"sha256": digest(p.read_bytes()), "mode": stat.S_IMODE(p.stat().st_mode)} for p in sorted(root.rglob("*")) if p.is_file()}


class OwnedPath:
    """Only one case's candidate files can reach the selected writer/reader."""

    def __init__(self, world, path: Path):
        self.world, self.path = world, path
        world.check(path.resolve().is_relative_to(world.candidate), "candidate-path-escape")

    def __truediv__(self, name):
        return OwnedPath(self.world, self.path / name)

    def __str__(self):
        return str(self.path)

    def with_suffix(self, suffix):
        return OwnedPath(self.world, self.path.with_suffix(suffix))

    def open(self, mode):
        self.world.check(mode in {"xb", "x"} and not self.path.is_symlink(), "candidate-open-mode")
        stream = self.path.open(mode)
        info = os.fstat(stream.fileno())
        self.world.streams[stream.fileno()] = (info.st_dev, info.st_ino)
        return stream

    def read_bytes(self):
        self.world.check(not self.path.is_symlink(), "candidate-read-symlink")
        return self.path.read_bytes()

    def read_text(self):
        self.world.check(not self.path.is_symlink(), "candidate-read-symlink")
        return self.path.read_text()


class Clock:
    def __init__(self, world):
        self.world, self.seconds = world, 0.0

    def monotonic(self):
        value = self.seconds
        self.world.event("monotonic")
        self.seconds += 0.25
        self.world.check(self.seconds <= 100, "virtual-time-cap")
        return value

    def sleep(self, seconds):
        self.world.event("sleep", seconds=seconds)
        self.world.check(isinstance(seconds, (int, float)) and 0 <= seconds <= 100 and self.seconds + seconds <= 100, "virtual-sleep-cap")
        self.seconds += seconds

    def monotonic_ns(self):
        return int(self.monotonic() * 1_000_000_000)

    def time_ns(self):
        return 1_700_000_000_000_000_000 + self.monotonic_ns()


class Child:
    """One synthetic handle; its status changes only at a recorded wait."""

    def __init__(self, world):
        self.world, self.pid, self.returncode, self.pending_signal = world, PID, None, None

    def wait(self, timeout):
        self.world.event("wait", timeout=timeout)
        if self.returncode is not None:
            return self.returncode
        if timeout == 30:
            if self.world.case == "P02":
                self.world.event("injected-error", boundary="wait30", kind="OSError")
                raise OSError("synthetic-initial-wait-denied")
            self.returncode = 7 if self.world.case == "C02" else 0
        else:
            self.world.check(isinstance(timeout, (int, float)) and 0 <= timeout <= 3, "cleanup-wait-bound")
            if self.pending_signal is None:
                self.world.event("injected-error", boundary="cleanup-wait", kind="TimeoutExpired")
                raise subprocess.TimeoutExpired(ARGV, timeout)
            self.returncode = -self.pending_signal
        self.world.event("reaped", code=self.returncode)
        return self.returncode

    def poll(self):
        self.world.event("poll")
        if self.world.case == "P02":
            self.world.event("injected-error", boundary="poll", kind="OSError")
            raise OSError("synthetic-poll-denied")
        return self.returncode

    def direct(self, name, kind):
        self.world.event("direct-signal", method=name, signal=int(kind))
        self.world.check(int(kind) in (int(signal.SIGTERM), int(signal.SIGKILL)), "direct-signal-kind")
        if self.returncode is None:
            self.pending_signal = int(kind)

    def terminate(self):
        self.direct("terminate", signal.SIGTERM)

    def kill(self):
        self.direct("kill", signal.SIGKILL)

    def send_signal(self, kind):
        self.direct("send_signal", kind)


class Kernel:
    """Process operations touch this case's table, never the real OS."""

    def __init__(self, world):
        self.world = world

    def killpg(self, target, kind):
        self.world.event("group-operation", target=target, signal=int(kind))
        self.world.check(target == PID and int(kind) in (0, int(signal.SIGTERM), int(signal.SIGKILL)), "synthetic-group-target")
        if kind == 0:
            if self.world.case != "P01" and self.world.child.returncode is not None:
                self.world.event("absent-group")
                raise ProcessLookupError("synthetic-group-absent")
            return
        if self.world.case == "P02":
            self.world.event("injected-error", boundary="group-signal", kind="PermissionError")
            raise PermissionError("synthetic-group-signal-denied")

    def getpgid(self, target):
        self.world.event("getpgid", target=target)
        self.world.check(target == PID, "synthetic-identity-target")
        return PID

    def getsid(self, target):
        self.world.event("getsid", target=target)
        self.world.check(target in (0, PID), "synthetic-identity-target")
        return PARENT if target == 0 else PID

    def getpgrp(self):
        self.world.event("getpgrp")
        return PARENT

    def getpid(self):
        self.world.event("getpid")
        return PARENT

    def fsync(self, descriptor):
        self.world.check(descriptor in self.world.streams, "unknown-stream-descriptor")
        info = os.fstat(descriptor)
        self.world.check(stat.S_ISREG(info.st_mode) and (info.st_dev, info.st_ino) == self.world.streams[descriptor], "owned-stream-identity")
        os.fsync(descriptor)


class World:
    def __init__(self, case: str, root: Path, forbidden: list):
        self.case, self.root, self.forbidden = case, root, forbidden
        self.events, self.streams, self.child, self.violations = [], {}, None, []
        self.candidate, self.inputs = root / "candidate", root / "inputs"
        self.candidate.mkdir()
        self.inputs.mkdir()
        self.repo, self.home, self.bin = root / "repo", root / "home", root / "bin"
        for path in (self.repo, self.home, self.bin):
            path.mkdir()
        self.env = {"HOME": str(self.home), "PATH": str(self.bin), "LANG": "C", "TZ": "UTC"}
        self.clock, self.kernel = Clock(self), Kernel(self)
        retain(
            self.inputs / "case.json",
            {
                "case": case,
                "synthetic_pid": PID,
                "synthetic_parent": PARENT,
                "argv": ARGV,
                "clock_start": 0,
                "monotonic_step": 0.25,
                "event_cap": 512,
                "virtual_seconds_cap": 100,
                "fault_script": "fixed wait30/poll OSError and nonzero group PermissionError" if case == "P02" else "lingering group after reaping" if case == "P01" else "normal reaped status7" if case == "C02" else "normal reaped status0",
            },
        )

    def event(self, operation, **fields):
        self.check(len(self.events) < 512, "boundary-event-cap")
        captured = json.loads(json.dumps(fields, sort_keys=True))
        self.events.append({"ordinal": len(self.events), "operation": operation, "synthetic_reaped": self.child is not None and self.child.returncode is not None, "virtual_seconds": self.clock.seconds, **captured})

    def check(self, condition, reason):
        if not condition:
            self.violations.append(reason)
            raise DriverBlock(reason)

    def popen(self, argv, **kwargs):
        self.event("Popen", argv=argv)
        self.check(self.child is None and argv == ARGV, "synthetic-constructor-count-or-argv")
        self.check(set(kwargs) == {"cwd", "env", "stdin", "stdout", "stderr", "start_new_session"}, "synthetic-constructor-keys")
        self.check(kwargs["cwd"] == self.repo and kwargs["env"] == self.env and kwargs["stdin"] == subprocess.DEVNULL and kwargs["start_new_session"] is True, "synthetic-constructor-context")
        for stream in (kwargs["stdout"], kwargs["stderr"]):
            self.check(stream.fileno() in self.streams, "constructor-stream-not-owned")
        kwargs["stdout"].write(OUT)
        kwargs["stderr"].write(ERR)
        self.child = Child(self)
        return self.child


def selected_code():
    raw = NATIVE.read_bytes()
    lines, tree = raw.splitlines(keepends=True), ast.parse(raw)
    classes = [n for n in tree.body if isinstance(n, ast.ClassDef) and n.name == "Mini"]
    require(len(classes) == 1, "selected-Mini-class")
    runs = [n for n in classes[0].body if isinstance(n, ast.FunctionDef) and n.name == "run"]
    require(len(runs) == 1 and not runs[0].decorator_list, "selected-run")
    run = runs[0]
    outside = b"".join(lines[: run.lineno - 1]) + b"<Mini.run omitted>\n" + b"".join(lines[run.end_lineno :])
    require(digest(outside) == OUTSIDE_SHA, "outside-run-source-drift")
    functions = {}
    for name, expected in FUNCTION_SHA.items():
        items = [n for n in tree.body if isinstance(n, ast.FunctionDef) and n.name == name]
        require(len(items) == 1 and digest(b"".join(lines[items[0].lineno - 1 : items[0].end_lineno])) == expected, "selected-pure-function-drift")
        functions[name] = items[0]
    nodes = [functions["_sha"], functions["_json"], run]
    for node in nodes:
        for part in ast.walk(node):
            require(not isinstance(part, (ast.Import, ast.ImportFrom, ast.Global, ast.Nonlocal)), "selected-source-capability")
            if isinstance(part, ast.Name):
                require(part.id not in {"eval", "exec", "compile", "open", "__import__", "globals", "locals", "vars", "getattr", "setattr", "__builtins__"}, "selected-source-name")
            if isinstance(part, ast.Attribute):
                require(not part.attr.startswith("__"), "selected-source-dunder")
    code = compile(ast.Module(body=nodes, type_ignores=[]), str(NATIVE) + "#selected-primitives", "exec", flags=__future__.annotations.compiler_flag, dont_inherit=True)
    return code, {"native_sha256": digest(raw), "outside_run_sha256": digest(outside), "selected_AST_sha256": digest(ast.dump(ast.Module(body=nodes, type_ignores=[]), include_attributes=False).encode())}


@pytest.fixture
def no_real_processes(monkeypatch):
    require(not any(n == "careerbrew" or n.startswith("careerbrew.") for n in sys.modules), "app-import-before-primitive")
    attempts = []

    def deny(name):
        def blocked(*args, **kwargs):
            attempts.append(name)
            raise DriverBlock("real-capability-denied:" + name)

        return blocked

    for name in ("Popen", "run", "call", "check_call", "check_output"):
        monkeypatch.setattr(subprocess, name, deny("subprocess." + name))
    for name in dir(os):
        if name in {"kill", "killpg", "fork", "forkpty", "posix_spawn", "posix_spawnp", "system", "popen"} or name.startswith(("exec", "spawn")):
            monkeypatch.setattr(os, name, deny("os." + name))
    for name in ("socket", "create_connection"):
        monkeypatch.setattr(socket, name, deny("socket." + name))
    for name in ("open", "open_new", "open_new_tab", "get"):
        monkeypatch.setattr(webbrowser, name, deny("browser." + name))
    return attempts


def observe(case: str, forbidden: list):
    retained = ROOT / ".metrics/issue327-native-cleanup/retained"
    for path in (ROOT / ".metrics", retained.parent, retained):
        require(not path.is_symlink(), "retained-symlink")
        path.mkdir(exist_ok=True)
    require(retained.resolve().is_relative_to(ROOT), "retained-path")
    root = Path(tempfile.mkdtemp(prefix=case + "-", dir=retained)).resolve()
    root.chmod(0o700)
    (root / ".owned-native-cleanup").write_text(case)
    world = World(case, root, forbidden)
    before, native_before = snapshot(world.inputs), digest(NATIVE.read_bytes())
    outcome, source, setup_error = None, None, None
    try:
        code, source = selected_code()
        names = (
            "BaseException",
            "Exception",
            "OSError",
            "ValueError",
            "ProcessLookupError",
            "PermissionError",
            "RuntimeError",
            "KeyboardInterrupt",
            "SystemExit",
            "AssertionError",
            "isinstance",
            "type",
            "str",
            "int",
            "float",
            "bool",
            "list",
            "tuple",
            "dict",
            "set",
            "len",
            "min",
            "max",
            "object",
        )
        namespace = {
            "__builtins__": {name: getattr(builtins, name) for name in names},
            "hashlib": hashlib,
            "json": json,
            "os": world.kernel,
            "time": world.clock,
            "signal": types.SimpleNamespace(SIGTERM=signal.SIGTERM, SIGKILL=signal.SIGKILL),
            "subprocess": types.SimpleNamespace(Popen=world.popen, DEVNULL=subprocess.DEVNULL, CompletedProcess=subprocess.CompletedProcess, TimeoutExpired=subprocess.TimeoutExpired),
        }
        exec(code, namespace)
        context = types.SimpleNamespace(sequence=0, root=OwnedPath(world, world.candidate), repo=world.repo, env=world.env)
        try:
            value = namespace["run"](context, "primitive", list(ARGV))
            require(isinstance(value, subprocess.CompletedProcess), "candidate-return-type")
            outcome = {"kind": "returned", "code": value.returncode, "stdout": value.stdout, "stderr": value.stderr}
        except DriverBlock:
            raise
        except BaseException as error:
            require(type(error) in (OSError, RuntimeError), "unexpected-candidate-exception")
            if type(error) is OSError:
                require(str(error) == "synthetic-poll-denied" and case == "P02", "unexpected-candidate-OSError")
            else:
                require(str(error).startswith("BLOCKED: owned command lifecycle"), "unexpected-candidate-RuntimeError")
            outcome = {"kind": "exception", "type": type(error).__name__}
    except BaseException as error:
        setup_error = type(error).__name__
    finally:
        terminal = world.candidate / "01-primitive.json"
        record = {
            "schema": 1,
            "case": case,
            "source": source,
            "outcome": outcome,
            "driver_block": setup_error,
            "events": world.events,
            "synthetic_child_returncode": world.child.returncode if world.child else None,
            "real_capability_attempts": list(forbidden),
            "facade_violations": list(world.violations),
            "before_inputs": before,
            "native_before_sha256": native_before,
            "retention_errors": [],
        }
        for name, capture in (
            ("after_inputs", lambda: snapshot(world.inputs)),
            ("native_after_sha256", lambda: digest(NATIVE.read_bytes())),
            ("candidate_terminal", lambda: json.loads(terminal.read_bytes()) if terminal.exists() else None),
            ("candidate_artifacts", lambda: snapshot(world.candidate)),
        ):
            try:
                record[name] = capture()
            except BaseException as error:
                record[name] = None
                record["retention_errors"].append({"stage": name, "type": type(error).__name__})
        retain(root / "observation.json", record)
    require(setup_error is None and not forbidden and not world.violations and not record["retention_errors"], "driver-block-or-real-capability-or-retention")
    require(record["before_inputs"] == record["after_inputs"] and record["native_before_sha256"] == record["native_after_sha256"], "primitive-input-drift")
    require(world.child is not None and len([e for e in world.events if e["operation"] == "Popen"]) == 1, "synthetic-child-count")
    require((world.candidate / "01-primitive.stdout").read_bytes() == OUT and (world.candidate / "01-primitive.stderr").read_bytes() == ERR, "actual-captured-bytes")
    return record


@pytest.mark.parametrize("case", ("P01", "P02"))
def test_native_cleanup_primary(case, no_real_processes):
    row = observe(case, no_real_processes)
    events, terminal = row["events"], row["candidate_terminal"]
    group_signals = [e for e in events if e["operation"] == "group-operation" and e["signal"] != 0]
    direct = [e for e in events if e["operation"] == "direct-signal"]
    if case == "P01":
        require(row["outcome"] == {"kind": "exception", "type": "RuntimeError"} and terminal is not None, "P01-expected-underlying-block")
        require(terminal["code"] == 0 and terminal["leader_reaped"] and terminal["owned_group_gone"] is False and terminal["blocked"], "P01-lingering-reaped-precondition")
        assert not group_signals and not direct, "PRIMARY-P01: nonzero group/direct call after the synthetic leader was reaped"
    else:
        require(len([e for e in events if e["operation"] == "injected-error" and e["boundary"] == "wait30"]) == 1, "P02-initial-fault-not-observed")
        if row["outcome"] == {"kind": "exception", "type": "OSError"}:
            require(any(e["operation"] == "injected-error" and e["boundary"] == "poll" for e in events), "P02-old-poll-cause")
        else:
            require(row["outcome"] == {"kind": "exception", "type": "RuntimeError"} and terminal is not None, "P02-expected-blocked-terminal")
        retained_error = json.dumps(terminal["blocked"]) if terminal is not None else ""
        safe = bool(direct) and all(not e["synthetic_reaped"] for e in direct + group_signals)
        safe = safe and row["synthetic_child_returncode"] in (-int(signal.SIGTERM), -int(signal.SIGKILL))
        safe = safe and terminal is not None and terminal["leader_reaped"] and terminal["owned_group_gone"] and ("OSError" in retained_error or "synthetic-initial-wait-denied" in retained_error)
        assert safe, "PRIMARY-P02: observation exceptions skipped direct owned cleanup/reaping or lost its error record"


@pytest.mark.parametrize("case, expected", (("C01", 0), ("C02", 7)))
def test_native_cleanup_compatibility(case, expected, no_real_processes):
    row = observe(case, no_real_processes)
    assert row["outcome"] == {"kind": "returned", "code": expected, "stdout": OUT.decode(), "stderr": ERR.decode()}
    assert row["synthetic_child_returncode"] == expected
    terminal = row["candidate_terminal"]
    assert terminal is not None and terminal["code"] == expected and terminal["leader_reaped"] and terminal["owned_group_gone"] and not terminal["blocked"]
    assert not any(e["operation"] == "direct-signal" or e["operation"] == "group-operation" and e["signal"] != 0 for e in row["events"])
