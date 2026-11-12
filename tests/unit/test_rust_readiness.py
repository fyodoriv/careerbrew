"""Installed-tool fixtures exercise the real bootstrap and health entrypoints."""

import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]


def executable(path, body):
    path.write_text("#!/bin/bash\nset -eu\n" + body + "\n")
    path.chmod(0o700)


@pytest.fixture
def installed(tmp_path):
    repo = tmp_path / "repo"
    scripts = repo / "scripts"
    scripts.mkdir(parents=True)
    for name in ("rust-readiness.sh", "setup.sh", "doctor.sh"):
        source = ROOT / "scripts" / name
        if source.exists():
            shutil.copy2(source, scripts / name)
    shutil.copy2(ROOT / "rust-toolchain.toml", repo / "rust-toolchain.toml")
    (repo / "Cargo.toml").write_text("[workspace]\n")
    tools = tmp_path / "installed/bin"
    tools.mkdir(parents=True)
    log = tmp_path / "calls"
    executable(tools / "curl", "exit 1")
    for name, answer in {
        "rustc": "rustc 1.99.1\nrelease: 1.99.1",
        "cargo": "cargo 1.99.1 (fixture)",
        "rustfmt": "rustfmt 1.10.0",
        "clippy-driver": "clippy 0.1.99",
    }.items():
        executable(tools / name, f'echo "{name} $*" >> "$CALL_LOG"\nprintf "%s\\n" "{answer}"')
    env = {"PATH": f"{tools}:/usr/bin:/bin", "HOME": str(tmp_path), "CALL_LOG": str(log)}
    return repo, tools, log, env


def check(installed, **overrides):
    repo, _, _, env = installed
    return subprocess.run(["/bin/bash", "scripts/rust-readiness.sh"], cwd=repo, env={**env, **overrides}, capture_output=True, text=True, timeout=10)


def test_matching_installation(installed):
    result = check(installed)
    assert result.returncode == 0, result.stderr + result.stdout
    assert "1.99.1" in result.stdout


def test_wrong_compiler(installed):
    executable(installed[1] / "rustc", 'printf "rustc 1.98.0\\nrelease: 1.98.0\\n"')
    result = check(installed)
    assert result.returncode == 1 and "release" in result.stdout


def test_wrong_cargo(installed):
    executable(installed[1] / "cargo", 'echo "cargo 1.98.0 (fixture)"')
    result = check(installed)
    assert result.returncode == 1 and "Cargo" in result.stdout


def test_missing_formatter(installed):
    (installed[1] / "rustfmt").unlink()
    result = check(installed)
    assert result.returncode == 1 and "rustfmt" in result.stdout


def test_missing_clippy(installed):
    (installed[1] / "clippy-driver").unlink()
    result = check(installed)
    assert result.returncode == 1 and "clippy" in result.stdout


def test_missing_toolchain_never_installs(installed):
    repo, tools, log, env = installed
    (tools / "rustc").unlink()
    for name in ("curl", "wget", "rustup"):
        executable(tools / name, 'echo forbidden >> "$CALL_LOG"; exit 99')
    result = check(installed)
    assert result.returncode == 1 and "compiler" in result.stdout
    assert not log.exists()


def rustup_proxy(installed, available):
    repo, tools, log, env = installed
    proxy = repo.parent / "proxy"
    proxy.mkdir()
    body = 'echo "rustup $* auto=$RUSTUP_AUTO_INSTALL" >> "$CALL_LOG"\n'
    body += 'test "${0##*/}" = rustup\n'
    body += 'test "$*" = "which --toolchain 1.99.1 rustc"\ntest "$RUSTUP_AUTO_INSTALL" = 0\n'
    body += f'echo "{tools / "rustc"}"' if available else "exit 1"
    executable(proxy / "rustup", body)
    if available:
        (proxy / "rustc").hardlink_to(proxy / "rustup")
    else:
        (proxy / "rustc").symlink_to(proxy / "rustup")
    env["PATH"] = f"{proxy}:{env['PATH']}"


def test_installed_rustup_uses_local_direct_tools(installed):
    rustup_proxy(installed, True)
    result = check(installed)
    assert result.returncode == 0, result.stdout + result.stderr
    calls = installed[2].read_text()
    assert "which --toolchain 1.99.1 rustc auto=0" in calls
    assert "rustc -Vv" in calls


def test_missing_rustup_toolchain_never_runs_proxy(installed):
    rustup_proxy(installed, False)
    result = check(installed)
    assert result.returncode == 1
    assert installed[2].read_text().splitlines() == ["rustup which --toolchain 1.99.1 rustc auto=0"]


def test_setup_checks_before_fetch_and_keeps_skip(installed):
    repo, tools, log, env = installed
    executable(repo / "scripts/doctor.sh", "exit 0")
    executable(repo / "scripts/install-launcher.sh", "exit 0")
    for skip in ("0", "1"):
        log.unlink(missing_ok=True)
        result = subprocess.run(["/bin/bash", "scripts/setup.sh"], cwd=repo, env={**env, "SETUP_SKIP_DEPS": skip}, capture_output=True, text=True, timeout=10)
        assert result.returncode == 0, result.stdout + result.stderr
        calls = log.read_text().splitlines()
        assert calls.index("rustc -Vv") < calls.index("cargo fetch") if skip == "0" else "cargo fetch" not in calls
    executable(tools / "rustc", 'echo "release: 1.98.0"')
    log.unlink()
    result = subprocess.run(["/bin/bash", "scripts/setup.sh"], cwd=repo, env=env, capture_output=True, text=True, timeout=10)
    assert result.returncode == 1
    assert "cargo fetch" not in (log.read_text() if log.exists() else "")


@pytest.mark.parametrize("ready", [False, True], ids=["refused", "verified"])
def test_setup_fetch_uses_only_verified_cargo(installed, ready):
    repo, tools, log, env = installed
    executable(repo / "scripts/doctor.sh", "exit 0")
    executable(repo / "scripts/install-launcher.sh", "exit 0")
    decoy = repo.parent / "decoy"
    decoy.mkdir()
    executable(decoy / "cargo", 'echo "decoy $*" >> "$CALL_LOG"; exit 99')
    executable(tools / "cargo", 'echo "verified $* auto=${RUSTUP_AUTO_INSTALL-unset}" >> "$CALL_LOG"\nif [ "$*" = "--version" ]; then echo "cargo 1.99.1 (fixture)"; else test "$*" = fetch && test "$RUSTUP_AUTO_INSTALL" = 0; fi')
    env["PATH"] = f"{decoy}:{env['PATH']}"
    if not ready:
        executable(tools / "rustc", 'echo "release: 1.98.0"')
    result = subprocess.run(["/bin/bash", "scripts/setup.sh"], cwd=repo, env=env, capture_output=True, text=True, timeout=10)
    calls = log.read_text().splitlines() if log.exists() else []
    assert result.returncode == (0 if ready else 1), result.stdout + result.stderr
    assert not any(line.startswith("decoy ") for line in calls)
    assert ("verified --version auto=0" in calls) is ready
    assert [line for line in calls if line.startswith("verified fetch")] == (["verified fetch auto=0"] if ready else [])


def test_doctor_uses_same_readiness(installed):
    repo, tools, log, env = installed
    # Other health checks fail safely in this tiny repository.
    executable(tools / "rustc", 'echo "release: 1.98.0"')
    result = subprocess.run(["/bin/bash", "scripts/doctor.sh"], cwd=repo, env=env, capture_output=True, text=True, timeout=15)
    assert result.returncode == 1
    assert "Rust compiler release" in result.stdout


def test_ambiguous_or_malformed_pin_refuses(installed):
    repo, _, log, _ = installed
    pin = (ROOT / "rust-toolchain.toml").read_text()
    for content in ('[toolchain]\nchannel = "stable"\n', pin + '\nchannel = "1.98.0"\n', pin.replace("[toolchain]\n", "") + "\n[toolchain]\n"):
        (repo / "rust-toolchain.toml").write_text(content)
        result = check(installed)
        assert result.returncode == 1 and "pin" in result.stdout
        assert not log.exists()


def test_compiler_overrides_refuse_before_fetch(installed):
    for name in ("RUSTC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "RUSTUP_TOOLCHAIN"):
        result = check(installed, **{name: "different-tool"})
        assert result.returncode == 1 and name in result.stdout
    assert not installed[2].exists()


def doctor_metadata_cargo(path, label):
    import json
    import sys

    dependency = {"name": "serde", "req": "=1.0.229", "features": ["derive"], "kind": None}
    metadata = json.dumps({"packages": [{"name": "careerbrew-xtask", "dependencies": [dependency]}]})
    script = f"""#!{sys.executable}
import json
import os
import sys

argv = sys.argv[1:]
expected = ["metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"]
if argv == ["--version"]:
    event = "VERSION_{label}"
    output = "cargo 1.99.1 (fixture)"
    status = 0
elif argv == expected:
    event = "META_{label}"
    output = {metadata!r}
    status = 0
else:
    event = "FORBIDDEN_{label}"
    output = ""
    status = 99
with open(os.environ["CALL_LOG"], "a") as log:
    log.write(json.dumps({{"event": event, "argv": argv, "env_keys": sorted(os.environ), "auto_install": os.environ.get("RUSTUP_AUTO_INSTALL")}}, sort_keys=True) + "\\n")
print(output)
sys.exit(status)
"""
    path.write_text(script)
    path.chmod(0o700)


def doctor_routing_observation(installed):
    import json

    repo, _, log, env = installed
    result = subprocess.run(
        ["/bin/bash", "scripts/doctor.sh"],
        cwd=repo,
        env=env,
        capture_output=True,
        text=True,
        timeout=15,
    )
    lines = log.read_text().splitlines() if log.exists() else []
    events = [json.loads(line) for line in lines if line.startswith("{")]
    observed = {
        "scope": "controlled synthetic Cargo dispatch only; no real installer or network",
        "meta_a": sum(event["event"] == "META_A" for event in events),
        "meta_b": sum(event["event"] == "META_B" for event in events),
        "forbidden": [event for event in events if event["event"].startswith("FORBIDDEN_")],
        "rustup_discoveries": lines.count("rustup which --toolchain 1.99.1 rustc auto=0"),
        "readiness_refused": "Rust 1.99.1 toolchain is not installed" in result.stdout,
        "readiness_ready": "Rust 1.99.1 compiler, Cargo, rustfmt and Clippy installed" in result.stdout,
        "serde_metadata_validated": "ok    native Serde derive dependency (locked, offline)" in result.stdout,
        "events": events,
        "doctor_exit": result.returncode,
    }
    receipt = json.dumps(observed, sort_keys=True)
    (repo.parent / "doctor-cargo-observation.json").write_text(receipt + "\n")
    print("doctor-cargo-observation: " + receipt, flush=True)
    return observed


def test_doctor_refusal_never_dispatches_metadata_cargo(installed):
    repo, tools, _, _ = installed
    doctor_metadata_cargo(tools / "cargo", "B")
    rustup_proxy(installed, False)
    doctor_metadata_cargo(repo.parent / "proxy/cargo", "A")
    observed = doctor_routing_observation(installed)
    assert observed["meta_a"] + observed["meta_b"] == 0, observed
    assert observed["rustup_discoveries"] == 1
    assert observed["readiness_refused"]
    assert not observed["readiness_ready"]
    assert observed["events"] == []
    assert observed["forbidden"] == []


def test_doctor_uses_verified_metadata_cargo(installed):
    repo, tools, _, env = installed
    doctor_metadata_cargo(tools / "cargo", "B")
    decoy = repo.parent / "decoy"
    decoy.mkdir()
    doctor_metadata_cargo(decoy / "cargo", "A")
    env["PATH"] = f"{decoy}:{env['PATH']}"
    observed = doctor_routing_observation(installed)
    assert observed["meta_a"] == 0, observed
    assert observed["meta_b"] == 1
    assert observed["readiness_ready"]
    assert not observed["readiness_refused"]
    assert observed["serde_metadata_validated"]
    assert not any(event["event"].endswith("_A") for event in observed["events"])
    metadata = [event for event in observed["events"] if event["event"] == "META_B"]
    assert metadata[0]["argv"] == ["metadata", "--locked", "--offline", "--no-deps", "--format-version", "1"]
    assert metadata[0]["auto_install"] == "0"
    assert observed["forbidden"] == []
