"""Browser fixture cleanup owns its launcher and every child it starts."""

import json
import os
import shutil
import subprocess
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[2]
CHECK = r"""
import assert from 'node:assert/strict';
import { existsSync, readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
const [source, repo, mode, compiler] = process.argv.slice(1);
const ts = createRequire(import.meta.url)(compiler);
const module = join(repo, 'fixture-under-test.mjs');
writeFileSync(module, ts.transpileModule(readFileSync(source, 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText);
const { repairServer } = await import(pathToFileURL(module));
const alive = (pid) => {
  try { process.kill(pid, 0); return true; }
  catch (error) { if (error.code === 'ESRCH') return false; throw error; }
};
const gone = async (pid) => {
  const until = Date.now() + 2000;
  while (alive(pid) && Date.now() < until) await new Promise((resolve) => setTimeout(resolve, 20));
  return !alive(pid);
};
let fixture, child;
try {
  try { fixture = await repairServer(repo); }
  catch (error) { if (mode !== 'exit' || !error.message.startsWith('synthetic repair server exited 1:')) throw error; }
  assert.equal(Boolean(fixture), mode !== 'exit');
  child = Number(readFileSync(join(repo, 'child.pid'), 'utf8'));
  if (fixture) await fixture.stop();
  assert.equal(await gone(child), true, `${mode}: owned child survived fixture cleanup`);
  const home = readFileSync(join(repo, 'fixture-home'), 'utf8').trim();
  assert.equal(existsSync(dirname(home)), false, `${mode}: fixture scratch directory survived cleanup`);
} finally {
  if (!child && existsSync(join(repo, 'child.pid'))) child = Number(readFileSync(join(repo, 'child.pid'), 'utf8'));
  if (child && alive(child)) { process.kill(child, 'SIGKILL'); await gone(child); }
  await fixture?.stop();
}
"""


@pytest.mark.parametrize("mode", ["ready", "exit"])
def test_repair_fixture_stops_owned_descendants(tmp_path, mode):
    check_fixture(tmp_path, mode)


def test_repair_fixture_escalates_for_resistant_owned_group(tmp_path):
    check_fixture(tmp_path, "resist")


def check_fixture(tmp_path, mode, script=CHECK):
    node = shutil.which("node")
    assert node, "install Node.js for the existing web test tools"
    launcher = """#!/bin/bash
if [ "$FIXTURE_START_MODE" = resist ]; then
  trap '' TERM
  node -e 'process.on("SIGTERM", () => {}); setInterval(() => {}, 1000)' &
else
  node -e 'setInterval(() => {}, 1000)' &
fi
printf '%s\\n' "$!" > child.pid
printf '%s\\n' "$HOME" > fixture-home
if [ "$FIXTURE_START_MODE" = exit ]; then exit 1; fi
for server_port do :; done
printf 'careerbrew web app: http://127.0.0.1:%s/\\n' "$server_port"
wait
"""
    for relative in ("tests/e2e/web-seed-and-serve.sh", ".venv/bin/python"):
        path = tmp_path / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(launcher)
        path.chmod(0o755)
    result = subprocess.run(
        [node, "--input-type=module", "-e", script, str(ROOT / "web/e2e/mcp-repair-fixture.ts"), str(tmp_path), mode, str(ROOT / "web/node_modules/typescript")],
        env={**os.environ, "FIXTURE_START_MODE": mode},
        capture_output=True,
        text=True,
        timeout=25,
    )
    assert result.returncode == 0, result.stdout + result.stderr
    return result


DIAGNOSTIC_CHECK = r"""
import assert from 'node:assert/strict';
import { existsSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import { dirname, join } from 'node:path';
import { pathToFileURL } from 'node:url';
const [source, repo, mode, compiler] = process.argv.slice(1);
const variant = VARIANT;
const ts = createRequire(import.meta.url)(compiler);
const file = join(repo, 'diagnostic-fixture.mjs');
writeFileSync(file, ts.transpileModule(readFileSync(source, 'utf8'), {
  compilerOptions: { module: ts.ModuleKind.ESNext, target: ts.ScriptTarget.ES2022 },
}).outputText);
const primary = Object.assign(new Error('synthetic cleanup error'), { code: variant === 'other-error' ? 'EACCES' : variant === 'absent' ? 'ESRCH' : 'EPERM' });
const realKill = process.kill.bind(process);
const childProcess = createRequire(import.meta.url)('node:child_process');
const realSnapshot = childProcess.spawnSync;
const realSpawn = childProcess.spawn;
const filesystem = createRequire(import.meta.url)('node:fs');
const realWrite = filesystem.writeSync;
const realStat = filesystem.statSync;
const executableStat = realStat(process.execPath);
const expectedWorker = { node: process.versions.node, executable: { device: executableStat.dev, inode: executableStat.ino } };
filesystem.statSync = (...args) => {
  if (armed && variant === 'identity-error' && args[0] === process.execPath) throw new Error('synthetic-identity-canary-4827');
  if (armed && variant === 'identity-malformed' && args[0] === process.execPath) return { ...realStat(...args), ino: NaN };
  return realStat(...args);
};
const sinkError = Object.assign(new Error('synthetic diagnostic sink failure'), { code: 'EIO' });
let armed = false, group, snapshots = 0, sinkAttempts = 0, attemptedDiagnostic, launcherExit, termSent = false;
process.kill = (pid, signal) => {
  if (armed && pid < 0 && signal === 0) throw primary;
  if (armed && pid < 0 && signal === 'SIGTERM') group = -pid;
  const result = realKill(pid, signal);
  if (armed && pid < 0 && signal === 'SIGTERM') termSent = true;
  return result;
};
filesystem.writeSync = (fd, ...args) => {
  if (armed && variant === 'sink-error' && fd === 2 && typeof args[0] === 'string' && args[0].startsWith('repair-fixture-cleanup: ')) {
    sinkAttempts++;
    attemptedDiagnostic = JSON.parse(args[0].slice('repair-fixture-cleanup: '.length));
    throw sinkError;
  }
  return realWrite(fd, ...args);
};
childProcess.spawn = (...args) => {
  const child = realSpawn(...args);
  if (variant === 'sink-error' && args[0] === join(repo, '.venv/bin/python')) {
    child.once('exit', (code, signal) => { launcherExit = { pid: child.pid, code, signal }; });
  }
  return child;
};
childProcess.spawnSync = (...args) => {
  if (!armed || args[0] !== 'ps') return realSnapshot(...args);
  assert.deepEqual(args[1], ['-axo', 'pid=,pgid=,uid=,stat=']);
  snapshots++;
  const uid = process.getuid();
  const foreign = `${group} ${group} ${uid + 1} Z\n`;
  let stdout = foreign;
  if (variant === 'malformed' || variant === 'canary') stdout = 'synthetic-rejection-canary-4827\n' + foreign;
  if (variant === 'canary') stdout += `${group} ${group} ${uid} synthetic-rejection-canary-4827\n`;
  if (variant === 'live') stdout = `${group} ${group} ${uid} S\n`;
  if (variant.startsWith('codec:')) stdout = `${group} ${group} ${uid + 1} ${variant.slice('codec:'.length)}\n`;
  if (variant === 'oversize') stdout = Array.from({ length: 2000 }, (_, i) => `${900000 + i} ${900000 + i} ${uid} S\n`).join('') + foreign;
  return { status: variant === 'ps-error' ? null : variant === 'ps-status' ? 3 : 0,
    error: variant === 'ps-error' ? Object.assign(new Error('synthetic-rejection-canary-4827'), { code: 'ETIMEDOUT' }) : undefined,
    signal: null, stdout, stderr: 'synthetic-rejection-canary-4827' };
};
syncBuiltinESMExports();
const { repairServer } = await import(pathToFileURL(file));
const fixture = await repairServer(repo);
const child = Number(readFileSync(join(repo, 'child.pid'), 'utf8'));
const home = readFileSync(join(repo, 'fixture-home'), 'utf8').trim();
let caught;
try {
  armed = true;
  try { await fixture.stop(); } catch (error) { caught = error; }
} finally {
  armed = false;
  process.kill = realKill;
  childProcess.spawnSync = realSnapshot;
  childProcess.spawn = realSpawn;
  filesystem.writeSync = realWrite;
  filesystem.statSync = realStat;
  syncBuiltinESMExports();
}
assert.equal(caught, variant === 'absent' ? undefined : primary, 'primary error identity changed');
assert.equal(snapshots, ['absent', 'other-error'].includes(variant) ? 0 : 1);
if (variant === 'sink-error') {
  assert.equal(sinkAttempts, 1);
  assert.equal(attemptedDiagnostic.reason, 'foreign-member');
  assert.equal(termSent, true);
  assert.equal(launcherExit.pid, group);
  assert.equal(launcherExit.signal, 'SIGTERM');
  assert.equal(launcherExit.code, null);
}
const absenceChecks = [];
for (const pid of [child, group]) {
  const until = Date.now() + 2000;
  let alive, lastErrno;
  do {
    try { realKill(pid, 0); alive = true; }
    catch (error) { if (error.code !== 'ESRCH') throw error; alive = false; lastErrno = error.code; }
    if (alive) await new Promise((resolve) => setTimeout(resolve, 20));
  } while (alive && Date.now() < until);
  assert.equal(alive, false, 'actual fixture process survived its real SIGTERM');
  absenceChecks.push({ pid, errno: lastErrno });
}
if (existsSync(dirname(home))) rmSync(dirname(home), { recursive: true });
console.log(JSON.stringify({ expected_worker: expectedWorker, same_primary: variant === 'absent' ? caught === undefined : caught === primary, primary_code: caught?.code, sink_error_escaped: caught === sinkError, snapshots, sink_attempts: sinkAttempts, term_sent: termSent, launcher_exit: launcherExit, absence_checks: absenceChecks, owned_child_absent: absenceChecks[0].errno === 'ESRCH', launcher_absent: absenceChecks[1].errno === 'ESRCH' }));
"""


@pytest.mark.parametrize(
    ("variant", "reason"),
    [
        ("identity-malformed", "foreign-member"),
        ("identity-error", "foreign-member"),
        ("ps-error", "ps-error"),
        ("ps-status", "ps-nonzero"),
        ("malformed", "malformed-row"),
        ("foreign", "foreign-member"),
        ("live", "live-member"),
        ("oversize", "foreign-member"),
        ("canary", "malformed-row"),
    ],
)
def test_repair_fixture_retains_eperm_diagnostic(tmp_path, variant, reason):
    result = check_fixture(tmp_path, "ready", DIAGNOSTIC_CHECK.replace("VARIANT", json.dumps(variant)))
    lines = [line for line in result.stderr.splitlines() if line.startswith("repair-fixture-cleanup: ")]
    assert len(lines) == 1, result.stdout + result.stderr
    assert len(lines[0].encode()) <= 32768
    assert "synthetic-rejection-canary-4827" not in result.stderr
    assert str(tmp_path) not in result.stderr
    assert shutil.which("node") not in result.stderr
    record = json.loads(lines[0].removeprefix("repair-fixture-cleanup: "))
    assert record["reason"] == reason
    assert record["scope"] == "repair-fixture-invocation"
    expected_worker = json.loads(result.stdout)["expected_worker"]
    if variant in ("identity-error", "identity-malformed"):
        assert record["worker"] is None
        assert "synthetic-identity-canary-4827" not in result.stderr
    else:
        assert record["worker"] == expected_worker
    assert record["group"] > 0 and record["expected_uid"] == os.getuid()
    assert len(record["ps"]["rows"]) <= 128
    assert len(record["ps"]["group_members"]) <= 128
    assert record["ps"]["truncated"] is (variant == "oversize")
    if variant == "ps-error":
        assert record["ps"]["error_code"] == "ETIMEDOUT"
    if variant == "ps-status":
        assert record["ps"]["status"] == 3
    if variant == "canary":
        assert record["ps"]["group_members"][-1][-1] == "unknown"
    if variant == "oversize":
        assert record["ps"]["total_rows"] == 2001
        assert record["ps"]["group_members"] == [[record["group"], record["group"], os.getuid() + 1, "Z"]]


@pytest.mark.parametrize("variant", ["other-error", "absent"])
def test_repair_fixture_keeps_non_eperm_decisions(tmp_path, variant):
    result = check_fixture(tmp_path, "ready", DIAGNOSTIC_CHECK.replace("VARIANT", json.dumps(variant)))
    assert "repair-fixture-cleanup: " not in result.stderr


def test_repair_fixture_keeps_primary_when_diagnostic_sink_fails(tmp_path):
    result = check_fixture(tmp_path, "ready", DIAGNOSTIC_CHECK.replace("VARIANT", json.dumps("sink-error")))
    evidence = json.loads(result.stdout.strip())
    assert evidence["same_primary"] is True
    assert evidence["primary_code"] == "EPERM"
    assert evidence["sink_error_escaped"] is False
    assert evidence["snapshots"] == 1
    assert evidence["sink_attempts"] == 1
    assert evidence["term_sent"] is True
    assert evidence["launcher_exit"]["pid"] > 0
    assert evidence["launcher_exit"]["code"] is None
    assert evidence["launcher_exit"]["signal"] == "SIGTERM"
    assert evidence["owned_child_absent"] is True
    assert evidence["launcher_absent"] is True
    assert "repair-fixture-cleanup: " not in result.stderr
    evidence["successful_diagnostic_lines"] = len([line for line in result.stderr.splitlines() if line.startswith("repair-fixture-cleanup: ")])
    (tmp_path / "sink-control-evidence.json").write_text(json.dumps(evidence, indent=2) + "\n")


@pytest.mark.parametrize(
    ("state", "expected"),
    [("S" + flag, "S" + flag) for flag in "+<>AELNSsVWX"]
    + [("RWNA", "RWNA"), ("R+<>AELNSsVWX", "R+<>AELNSsVWX"), ("S" + "X" * 13, "unknown"), ("Ssynthetic-codec-canary-4827", "unknown")]
    + [(state, state) for state in ("D", "Dl", "Xl", "E-", "Z+", "SNNNNNNN")]
    + [pytest.param("HNE", "HNE", id="native-halted"), pytest.param("?NE", "?NE", id="native-unknown")],
)
def test_repair_fixture_preserves_state_codec(tmp_path, state, expected):
    result = check_fixture(tmp_path, "ready", DIAGNOSTIC_CHECK.replace("VARIANT", json.dumps("codec:" + state)))
    lines = [line for line in result.stderr.splitlines() if line.startswith("repair-fixture-cleanup: ")]
    assert len(lines) == 1, result.stdout + result.stderr
    record = json.loads(lines[0].removeprefix("repair-fixture-cleanup: "))
    assert record["reason"] == "foreign-member"
    assert record["ps"]["group_members"] == [[record["group"], record["group"], os.getuid() + 1, expected]]
    assert len(lines[0].encode()) <= 32768
    assert "synthetic-codec-canary-4827" not in result.stderr
