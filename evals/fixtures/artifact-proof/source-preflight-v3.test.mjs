// Registered 57 source rows + 70 recipes + 6 injected owner decisions. No native imports.
import { preflightSource, semanticSourceReceipt, decideSourceOwner } from './source-preflight-v3.mjs';
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync, mkdirSync, writeFileSync, existsSync, lstatSync } from 'node:fs';
import { resolve, relative, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import test, { after } from 'node:test';

const here = fileURLToPath(new URL('.', import.meta.url));
const directory = resolve(process.env.PREFLIGHT_REGISTRATION ?? `${here}/source-preflight-v3-registration`);
assert.equal(directory, resolve(here, 'source-preflight-v3-registration'));
const read = (name) => JSON.parse(readFileSync(resolve(directory, name), 'utf8'));
const corpusBytes = readFileSync(resolve(here, 'visual-v3-cases.json'));
const corpus = JSON.parse(corpusBytes);
const originalCases = corpus.groups.flatMap((group) => group.cases);
const originals = new Map(originalCases.map((item) => [item.id, item]));
const classification = read('frozen57-source-classification.json');
const recipes = read('source-cases.json').cases;
const faults = read('source-owner-faults.json').cases;
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const canonical = (value) => JSON.stringify(value, (_key, item) => item && typeof item === 'object' && !Array.isArray(item)
  ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, item[key]])) : item);
assert.equal(hash(corpusBytes), classification.corpusSha256);
assert.deepEqual([classification.rows.length, recipes.length, faults.length], [57, 70, 6]);
const records = [];
const bodyKeys = ['role', 'identity', 'mime', 'bytes', 'sha256'].sort();

// Root creates this ignored capability only after independent source review.
// Its hashes bind this exact positive branch; absence stops before any parser call.
const releasePath = resolve('.metrics/v3-source-development/parser-execution-release.json');
assert.equal(existsSync(releasePath), true, 'independent source review and root execution release required');
const release = JSON.parse(readFileSync(releasePath, 'utf8'));
assert.equal(release.allow133SourceOnly, true);
const sourceFiles = [
  'source-preflight-v3.mjs', 'source-preflight-v3.test.mjs', 'capture-visual-v3-admission.mjs', 'capture-visual-v3-source.test.mjs',
  'visual-v3-cases.json', 'source-preflight-v3-tools/package.json', 'source-preflight-v3-tools/package-lock.json',
  ...['policy.json', 'source-cases.json', 'source-owner-faults.json', 'frozen57-source-classification.json'].map((name) => `source-preflight-v3-registration/${name}`),
];
for (const name of sourceFiles) assert.equal(hash(readFileSync(resolve(here, name))), release.sourcePins[name], `unreleased source: ${name}`);
assert.equal(process.version, 'v24.21.0');
const censusPath = resolve('.metrics/v3-pure-development/installed-source-census.json');
assert.equal(hash(readFileSync(censusPath)), release.installedCensusSha256);
const installed = JSON.parse(readFileSync(censusPath, 'utf8'));
for (const [name, pin] of Object.entries(installed)) {
  const path = resolve(here, 'source-preflight-v3-tools', name);
  const info = lstatSync(path);
  assert.equal(info.isSymbolicLink(), false);
  assert.equal(`0o${(info.mode & 0o777).toString(8)}`, pin.mode);
  if (pin.kind === 'file') {
    assert.equal(hash(readFileSync(path)), pin.sha256);
    assert.equal(info.size, pin.bytes);
  } else assert.equal(info.isDirectory(), true);
}

function repeated(spec) {
  assert.equal(spec.encoding, 'repeat-utf8');
  assert.ok(Number.isSafeInteger(spec.repeat) && spec.repeat >= 0);
  const bytes = Buffer.from(spec.prefix + spec.unit.repeat(spec.repeat) + spec.suffix);
  assert.equal(bytes.length, spec.bytes);
  assert.equal(hash(bytes), spec.fixtureBodySha256 ?? spec.sha256);
  return bytes;
}
function corpusAsset(id) {
  const asset = corpus.assets[id];
  assert.ok(asset);
  let bytes;
  if (asset.encoding === 'file') {
    assert.ok(asset.path.startsWith('v3-assets/') && !asset.path.split('/').includes('..'));
    let path = here;
    for (const part of asset.path.split('/')) {
      path = resolve(path, part);
      assert.equal(lstatSync(path).isSymbolicLink(), false);
    }
    bytes = readFileSync(path);
  } else bytes = repeated(asset);
  assert.equal(bytes.length, asset.bytes);
  assert.equal(hash(bytes), asset.sha256);
  return bytes;
}
function originalDelivery(raw) {
  if (raw === undefined || raw === null) return raw;
  const result = { ...raw };
  if (Object.hasOwn(raw, 'htmlAssetId')) result.html = corpusAsset(raw.htmlAssetId);
  if (Object.hasOwn(raw, 'assets')) result.assets = raw.assets.map((binding) => {
    const asset = corpus.assets[binding.assetId];
    return { path: binding.path, mime: asset.mime, sha256: binding.sha256Override ?? asset.sha256, bytes: corpusAsset(binding.assetId) };
  });
  return result;
}
function recipeDelivery(item) {
  if (Object.hasOwn(item.input, 'frozenCorpusCase')) {
    const original = originals.get(item.input.frozenCorpusCase);
    assert.equal(hash(canonical(original)), item.input.caseSha256);
    return originalDelivery(original.delivery);
  }
  const raw = { ...item.input };
  if (raw.htmlRecipe) raw.html = repeated(raw.htmlRecipe);
  raw.assets = raw.assets.map((asset) => {
    const bytes = asset.encoding === 'base64' ? Buffer.from(asset.body, 'base64') : repeated(asset.recipe);
    assert.equal(bytes.length, asset.bytes);
    assert.equal(hash(bytes), asset.fixtureBodySha256 ?? asset.recipe.sha256);
    return { path: asset.path, mime: asset.mime, sha256: asset.sha256, bytes };
  });
  return raw;
}
function snapshot(raw) {
  const digest = createHash('sha256');
  function walk(value) {
    if (value instanceof Uint8Array) { digest.update('bytes:'); digest.update(value); }
    else if (Array.isArray(value)) { digest.update('array:'); for (const item of value) walk(item); }
    else if (value && typeof value === 'object') {
      for (const key of Object.keys(value).sort()) { digest.update(`key:${key};`); walk(value[key]); }
    } else digest.update(`${typeof value}:${JSON.stringify(value) ?? 'undefined'};`);
  }
  walk(raw);
  return digest.digest('hex');
}
function exactSet(actual, expected) {
  assert.ok(Array.isArray(actual) && Array.isArray(expected));
  for (const set of [actual, expected]) {
    assert.equal(new Set(set.map((row) => row.identity)).size, set.length);
    for (const row of set) assert.deepEqual(Object.keys(row).sort(), bodyKeys);
  }
  const order = (set) => [...set].sort((a, b) => a.identity < b.identity ? -1 : a.identity > b.identity ? 1 : 0);
  assert.deepEqual(order(actual), order(expected));
}
function verify(actual, expected, matchers = {}) {
  for (const [name, value] of Object.entries(matchers)) {
    assert.equal(name, 'boundResourceSet');
    assert.equal(value, 'exact-canonical-set');
  }
  for (const [key, value] of Object.entries(expected)) {
    assert.ok(Object.hasOwn(actual, key), `missing observed field: ${key}`);
    if (key === 'boundResourceSet') exactSet(actual[key], value);
    else assert.deepEqual(actual[key], value, `observed field mismatch: ${key}`);
  }
  for (const field of ['browserLaunches', 'artifactContextsCreated', 'metadataContextsCreated', 'headerChildrenCreated', 'codecCalls']) assert.equal(actual[field], 0);
}
function retainedGraphChecks(result, raw) {
  if (!result.sourceEligible) return;
  const graph = result.graph;
  assert.ok(Object.isFrozen(graph) && Object.isFrozen(graph.resources));
  assert.equal(hash(graph.copyPayload(graph.main.identity)), graph.main.sha256);
  for (const row of [graph.main, ...graph.declaredAssets, ...graph.resources]) {
    const first = graph.copyPayload(row.identity);
    assert.equal(hash(first), row.sha256);
    if (first.length) first[0] ^= 255;
    assert.equal(hash(graph.copyPayload(row.identity)), row.sha256);
  }
  // Caller-owned buffers cannot change retained graph payloads.
  for (const asset of raw?.assets ?? []) if (asset.bytes instanceof Uint8Array && asset.bytes.length) asset.bytes[0] ^= 255;
  for (const row of [...graph.declaredAssets, ...graph.resources]) assert.equal(hash(graph.copyPayload(row.identity)), row.sha256);
}

for (const row of classification.rows) test(`corpus:${row.caseId}`, () => {
  const item = originals.get(row.caseId);
  assert.equal(hash(canonical(item)), row.originalCaseSha256);
  const arms = [];
  const checks = [];
  for (const arm of row.inputArms) {
    const [field, index] = arm.split('/');
    const originalsForArm = [index === undefined ? item[field] : item[field][Number(index)]];
    assert.ok(Array.isArray(originalsForArm));
    const results = [];
    for (const rawOriginal of originalsForArm) {
      const raw = originalDelivery(rawOriginal);
      const before = snapshot(raw);
      const result = preflightSource(raw);
      const observed = semanticSourceReceipt(result);
      checks.push({ raw, result, observed, inputUnchanged: snapshot(raw) === before });
      results.push(observed);
    }
    arms.push({ arm, results });
  }
  assert.equal(hash(canonical(item)), row.originalCaseSha256);
  records.push({ kind: 'corpus', id: row.caseId, inputSha256: row.originalCaseSha256, expected: { sourceDisposition: row.sourceDisposition, reason: row.sourceReason }, arms });
  // Collect every declared arm before judging; a first bad arm cannot hide another.
  for (const check of checks) {
    assert.equal(check.inputUnchanged, true);
    assert.equal(check.observed.sourceDisposition, row.sourceDisposition);
    assert.equal(check.observed.reason, row.sourceReason);
    assert.equal(check.observed.artifactContextsCreated, row.artifactContextsCreated);
    verify(check.observed, { browserLaunches: 0, metadataContextsCreated: 0, headerChildrenCreated: 0, codecCalls: 0 });
    retainedGraphChecks(check.result, check.raw);
  }
});
for (const item of recipes) test(`source:${item.id}`, () => {
  const raw = recipeDelivery(item);
  const before = snapshot(raw);
  const result = preflightSource(raw);
  const observed = semanticSourceReceipt(result);
  records.push({ kind: 'source', id: item.id, inputSha256: hash(canonical(item.input)), expected: item.expected, observed });
  assert.equal(snapshot(raw), before);
  verify(observed, item.expected, item.matchers);
  if (item.expected.boundResourceSet) {
    const expected = item.expected.boundResourceSet;
    assert.throws(() => exactSet([...observed.boundResourceSet, { role: 'image', identity: 'extra:synthetic', mime: 'image/png', bytes: 0, sha256: '0'.repeat(64) }], expected));
    if (expected.length) {
      assert.throws(() => exactSet(observed.boundResourceSet.slice(1), expected));
      assert.throws(() => exactSet([...observed.boundResourceSet, observed.boundResourceSet[0]], expected));
    }
  }
  retainedGraphChecks(result, raw);
});
function injectedPorts(fault) {
  let thunk;
  return {
    create(build) { if (fault === 'child-create-reject') throw new Error('synthetic create'); thunk = build; return {}; },
    result() {
      if (fault === 'child-exit-nonzero') return { kind: 'exit', exitCode: 17, elapsedMs: 0 };
      if (fault === 'child-never-settle' || fault === 'child-kill-reap-unconfirmed') return { kind: 'deadline', elapsedMs: 30000 };
      if (fault === 'child-output-overflow') return { kind: 'result', elapsedMs: 0, outputBytes: 33554433 };
      return { kind: 'result', elapsedMs: 30001, outputBytes: 0, value: thunk() };
    },
    close() { return { terminal: fault === 'child-kill-reap-unconfirmed' ? 'unknown' : 'reaped' }; },
  };
}
for (const item of faults) test(`owner:${item.id}`, async () => {
  const source = recipes.find((recipe) => recipe.id === item.input.deliveryReference);
  assert.ok(source);
  const raw = recipeDelivery(source);
  const before = snapshot(raw);
  const observed = semanticSourceReceipt(await decideSourceOwner(raw, injectedPorts(item.input.fault)));
  records.push({ kind: 'owner-port', id: item.id, inputSha256: hash(canonical(item.input)), expected: item.expected, observed });
  assert.equal(snapshot(raw), before);
  verify(observed, item.expected);
});
after(() => {
  if (records.length !== 133) return;
  const output = process.env.PREFLIGHT_RECEIPT_DIR;
  assert.ok(output, 'PREFLIGHT_RECEIPT_DIR is required');
  const directory = resolve(output);
  const within = relative(resolve('.metrics'), directory);
  assert.ok(within && within !== '..' && !within.startsWith(`..${sep}`) && !within.startsWith(sep));
  let parent = resolve('.metrics');
  assert.equal(lstatSync(parent).isSymbolicLink(), false);
  for (const part of within.split(sep)) {
    parent = resolve(parent, part);
    if (existsSync(parent)) assert.equal(lstatSync(parent).isSymbolicLink(), false);
  }
  mkdirSync(directory, { recursive: true, mode: 0o700 });
  const file = resolve(directory, 'semantic.json');
  assert.equal(existsSync(file), false, 'semantic receipt must be fresh');
  writeFileSync(file, `${JSON.stringify(records, null, 2)}\n`, { flag: 'wx', mode: 0o600 });
});
