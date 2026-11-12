// Fixed synthetic measurement checks assigned to rust-27; no actors or servers.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { dirname, join } from 'node:path';
import { spawnSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const directory = dirname(fileURLToPath(import.meta.url));
const projector = join(directory, 'project-visible-v2.mjs');
const ORIGINAL_SOURCE = '749ba33c4e3a9435184ba2378a250d60e1476c85d51cd0ce4408407897061e41';
const ORIGINAL_CASES = '692f0bf3c4dbb3ef8a1fedc4cd4000415eb0b3977acc21116fd82c381a1444c1';
const NEW_CASES = '3324bef5c471b1980d3bb47fe10b8d6d0e71ff5f2960a6649052cd2945059b62';
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
if (phase === 'red') assert.equal(codeHash, ORIGINAL_SOURCE, 'RED requires the unchanged V2 projector');
assert.equal(hash(await readFile(join(directory, 'projector-v2-cases.json'))), ORIGINAL_CASES);
const caseBytes = await readFile(join(directory, 'projector-v21-cases.json'));
assert.equal(hash(caseBytes), NEW_CASES);
const groups = JSON.parse(caseBytes).groups;
assert.deepEqual(groups.map((group) => group.id), [8, 9, 10, 11]);

async function execute(argv, destination) {
  assert.equal(hash(await readFile(projector)), codeHash, 'Projector changed during checks');
  const result = spawnSync(process.execPath, [projector, ...argv], { encoding: 'utf8', maxBuffer: 10 * 1024 * 1024 });
  await writeFile(join(destination, 'stdout.log'), result.stdout || '');
  await writeFile(join(destination, 'stderr.log'), result.stderr || '');
  assert.equal(hash(await readFile(projector)), codeHash, 'Projector changed during checks');
  return { status: result.status, signal: result.signal, error: result.error?.message || null };
}

const originalDirectory = join(output, 'original-seven');
const original = await execute(['--self-test', '--output', originalDirectory], output);
let originalRuns = [];
try {
  originalRuns = await Promise.all([1, 2].map(async (iteration) => JSON.parse(await readFile(join(originalDirectory, `run-${iteration}.json`), 'utf8'))));
} catch { /* The failed process log remains the evidence; no missing test is a pass. */ }
const originalPass = original.status === 0 && originalRuns.length === 2 && originalRuns.every((run) => run.results.length === 7 && run.results.every((item) => item.pass));
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
      const processResult = await execute(['--outputs-dir', finished, '--artifact-sha256', hash(artifact), '--output', projected], destination);
      let receipt = null;
      let text = null;
      try {
        receipt = JSON.parse(await readFile(join(projected, 'projection-receipt.json'), 'utf8'));
        if (receipt.eligible) text = await readFile(join(projected, 'projected-input.txt'), 'utf8');
      } catch { /* Preserve missing/invalid receipt as failure rather than inventing an output. */ }
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
const versions = [...new Set([...originalRuns.map((run) => run.browser), ...results.flatMap((group) => group.subcases.map((entry) => entry.browserVersion))])];
const identity = groups.every((group) => {
  const first = results.find((entry) => entry.iteration === 1 && entry.group === group.id).subcases;
  const second = results.find((entry) => entry.iteration === 2 && entry.group === group.id).subcases;
  return first.every((entry, index) => entry.eligible === second[index].eligible && entry.actual === second[index].actual && entry.status === second[index].status);
});
const pass = originalPass && results.every((entry) => entry.pass) && identity && versions.length === 1 && versions[0] !== null;
const receipt = {
  method: 'V2.1-checks', phase, registrationCommit: '5d7361d1', inputOrigin: 'registered synthetic self-tests only',
  codeSha256: codeHash, driverSha256: hash(await readFile(fileURLToPath(import.meta.url))), originalCasesSha256: ORIGINAL_CASES, newCasesSha256: NEW_CASES,
  originalPass, originalRuns, groupsPerRun: 11, freshRuns: 2, outputIdentity: identity, browserVersions: versions,
  results, pass, actualActorArtifactReads: 0, readerCalls: 0,
};
await writeFile(join(output, 'combined-receipt.json'), JSON.stringify(receipt, null, 2) + '\n');
if (!pass) process.exitCode = 1;
