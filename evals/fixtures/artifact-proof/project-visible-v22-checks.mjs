// Fixed synthetic checks assigned to rust-27. No actor input or server is used.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
const projector = join(directory, 'project-visible-v2.mjs');
const previousDriver = join(directory, 'project-visible-v21-checks.mjs');
const RED_SOURCE = '1e2404527edc5b8f34a623d1ada8c76ca6df5e93b4a1fd4279675656de4bfbf7';
const PREVIOUS_DRIVER = '173b64b85177a1b2019b171892c11d31d6d5410e6a064971b0ad29296e595d9b';
const NEW_CASES = '5fc69da4f85b4f61e832319270253f5cb9f8d14954ddfd81e25d1d8f2d18471f';
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const args = process.argv.slice(2);
assert.equal(args.length, 4);
assert.equal(args[0], '--phase');
assert.ok(['red', 'green'].includes(args[1]));
assert.equal(args[2], '--output');
const phase = args[1];
const output = args[3];
await mkdir(output, { recursive: true });
assert.equal((await readdir(output)).length, 0, 'Use a fresh result directory');
const codeHash = hash(await readFile(projector));
if (phase === 'red') assert.equal(codeHash, RED_SOURCE, 'RED requires unchanged V2.1');
assert.equal(hash(await readFile(previousDriver)), PREVIOUS_DRIVER);
const caseBytes = await readFile(join(directory, 'projector-v22-cases.json'));
assert.equal(hash(caseBytes), NEW_CASES);
const groups = JSON.parse(caseBytes).groups;
assert.deepEqual(groups.map((group) => group.id), [12, 13]);

async function execute(script, argv, destination) {
  assert.equal(hash(await readFile(projector)), codeHash, 'Projector changed during checks');
  const result = spawnSync(process.execPath, [script, ...argv], { encoding: 'utf8', maxBuffer: 10 * 1024 * 1024 });
  await writeFile(join(destination, 'stdout.log'), result.stdout || '');
  await writeFile(join(destination, 'stderr.log'), result.stderr || '');
  assert.equal(hash(await readFile(projector)), codeHash, 'Projector changed during checks');
  return { status: result.status, signal: result.signal, error: result.error?.message || null };
}

const compatibilityDirectory = join(output, 'previous-eleven');
const compatibilityProcess = await execute(previousDriver, ['--phase', 'green', '--output', compatibilityDirectory], output);
let compatibility = null;
try { compatibility = JSON.parse(await readFile(join(compatibilityDirectory, 'combined-receipt.json'), 'utf8')); }
catch { /* Missing compatibility evidence is not success. */ }
const previousPass = compatibilityProcess.status === 0 && compatibility?.pass === true && compatibility.codeSha256 === codeHash;
const results = [];
for (let iteration = 1; iteration <= 2; iteration++) {
  for (const group of groups) {
    const subcases = [];
    for (const [index, entry] of group.cases.entries()) {
      const destination = join(output, `run-${iteration}`, `group-${group.id}`, `case-${index + 1}`);
      const finished = join(destination, 'finished');
      await mkdir(finished, { recursive: true });
      const artifact = Buffer.from(entry.html, 'utf8');
      await writeFile(join(finished, 'result.html'), artifact, { flag: 'wx' });
      const projected = join(destination, 'projection');
      const processResult = await execute(projector, ['--outputs-dir', finished, '--artifact-sha256', hash(artifact), '--output', projected], destination);
      let receipt = null;
      let text = null;
      try {
        receipt = JSON.parse(await readFile(join(projected, 'projection-receipt.json'), 'utf8'));
        if (receipt.eligible) text = await readFile(join(projected, 'projected-input.txt'), 'utf8');
      } catch { /* Preserve failure without inventing a projected input. */ }
      const bound = receipt?.codeSha256 === codeHash && receipt?.artifactSha256 === hash(artifact);
      const pass = bound && (entry.expected === 'unavailable'
        ? processResult.status === 2 && receipt.eligible === false && text === null
        : processResult.status === 0 && receipt.eligible === true && text === entry.expected);
      const record = { subcase: index + 1, inputSha256: hash(artifact), expected: entry.expected, actual: text, eligible: receipt?.eligible ?? null, reason: receipt?.reason ?? null, browserVersion: receipt?.browserVersion ?? null, projectedInputSha256: receipt?.projectedInputSha256 ?? null, ...processResult, pass };
      await writeFile(join(destination, 'check.json'), JSON.stringify(record, null, 2) + '\n');
      subcases.push(record);
    }
    const record = { iteration, group: group.id, pass: subcases.every((item) => item.pass), subcases };
    results.push(record);
    console.log(JSON.stringify({ iteration, group: group.id, pass: record.pass, failures: subcases.filter((item) => !item.pass).map((item) => item.subcase) }));
  }
}
const identity = groups.every((group) => {
  const first = results.find((entry) => entry.iteration === 1 && entry.group === group.id).subcases;
  const second = results.find((entry) => entry.iteration === 2 && entry.group === group.id).subcases;
  return first.every((entry, index) => entry.eligible === second[index].eligible && entry.actual === second[index].actual && entry.status === second[index].status);
});
const versions = [...new Set([...(compatibility?.browserVersions || []), ...results.flatMap((group) => group.subcases.map((entry) => entry.browserVersion))])];
const pass = previousPass && results.every((entry) => entry.pass) && identity && compatibility?.outputIdentity === true && versions.length === 1 && versions[0] !== null;
await writeFile(join(output, 'combined-receipt.json'), JSON.stringify({
  method: 'V2.2-checks', phase, registrationCommit: '4f2cd198', codeSha256: codeHash,
  driverSha256: hash(await readFile(fileURLToPath(import.meta.url))), previousDriverSha256: PREVIOUS_DRIVER, newCasesSha256: NEW_CASES,
  compatibilityDirectory, previousPass, groupsPerRun: 13, freshRuns: 2, outputIdentity: identity && compatibility?.outputIdentity === true,
  browserVersions: versions, results, pass, actualActorArtifactReads: 0, readerCalls: 0,
}, null, 2) + '\n');
if (!pass) process.exitCode = 1;
