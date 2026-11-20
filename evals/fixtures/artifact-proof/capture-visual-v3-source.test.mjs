// Pure admission checks only. No browser module or image decoder is imported.
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import {
  admitDelivery, entryUrl, normalizeBundlePath, resolveLocalReference, tilePositions,
} from './capture-visual-v3-admission.mjs';

const corpus = JSON.parse(await readFile(new URL('./visual-v3-cases.json', import.meta.url), 'utf8'));
const cases = corpus.groups.flatMap((group) => group.cases);
const byId = (id) => cases.find((item) => item.id === id);

async function assetBytes(asset) {
  if (asset.encoding === 'file') return readFile(new URL(asset.path, import.meta.url));
  return Buffer.from(asset.prefix + asset.unit.repeat(asset.repeat) + asset.suffix);
}

async function fixtureDelivery(raw) {
  if (raw === null || raw === undefined) return raw;
  const result = { ...raw };
  if (raw.htmlAssetId) result.html = await assetBytes(corpus.assets[raw.htmlAssetId]);
  if (Object.hasOwn(raw, 'assets')) {
    result.assets = await Promise.all(raw.assets.map(async (binding) => {
      const asset = corpus.assets[binding.assetId];
      return { path: binding.path, mime: asset.mime, sha256: binding.sha256Override ?? asset.sha256, bytes: await assetBytes(asset) };
    }));
  }
  return result;
}

for (const item of cases.filter((item) => item.kind === 'admission-negative')) {
  test(item.id, async () => {
    const present = Object.hasOwn(item, 'delivery');
    const raw = present ? item.delivery : undefined;
    assert.equal(present, item.expected.rawDeliveryFieldPresent);
    const delivery = await fixtureDelivery(raw);
    assert.throws(() => admitDelivery(delivery), (error) => error.reason === item.expected.reason);
    if (item.looseCandidate) {
      assert.equal(admitDelivery(await fixtureDelivery(item.looseCandidate)).admitted, true);
    }
  });
}

test('explicit empty assets admits self-contained HTML', async () => {
  const input = await fixtureDelivery(byId('ordinary-exact-body').delivery);
  const result = admitDelivery(input);
  assert.equal(result.admitted, true);
  assert.equal(result.entryPath, 'result.html');
  assert.equal(result.assets.size, 0);
  assert.equal(result.htmlBytes.toString(), input.html);
});

test('nested explicit bundle preserves entry and asset identities', async () => {
  const input = await fixtureDelivery(byId('nested-relative-resolution').left);
  const result = admitDelivery(input);
  assert.equal(result.entryPath, 'reports/proof.html');
  assert.equal(result.url, 'http://artifact.invalid/reports/proof.html');
  assert.deepEqual([...result.assets.keys()], ['assets/red.png', 'reports/styles/proof.css']);
});

test('unsafe manifest paths refuse rather than normalize', () => {
  for (const path of ['', '/absolute', '../escape', 'a/../b', './a', 'a//b', 'a\\b', 'a\0b']) {
    assert.throws(() => normalizeBundlePath(path), (error) => error.reason === 'path-traversal');
  }
});

test('entry URL encodes original path components without renaming', () => {
  assert.equal(entryUrl('reports/my proof.html'), 'http://artifact.invalid/reports/my%20proof.html');
  assert.equal(entryUrl('reports/π.html'), 'http://artifact.invalid/reports/%CF%80.html');
});

test('native URL resolution retains valid parent segments and refuses escape', () => {
  assert.equal(resolveLocalReference('../assets/red.png', 'reports/proof.html'), 'assets/red.png');
  assert.equal(resolveLocalReference('../../assets/red.png', 'reports/styles/proof.css'), 'assets/red.png');
  assert.throws(() => resolveLocalReference('../outside.png', 'proof.html'), (error) => error.reason === 'path-traversal');
  assert.throws(() => resolveLocalReference('https://other.invalid/red.png', 'proof.html'), (error) => error.reason === 'external-dependency');
  for (const url of [
    'http://artifact.invalid/assets/red.png', '//artifact.invalid/assets/red.png',
    'http://artifact.invalid/reports/../assets/red.png', '//artifact.invalid/reports/%2e%2e/assets/red.png',
  ]) assert.equal(resolveLocalReference(url, 'reports/proof.html'), 'assets/red.png');
  const aboveRoot = [
    'http://artifact.invalid/../outside.png', '//artifact.invalid/%2e%2e/outside.png',
  ].map((url) => {
    try { return { url, accepted: resolveLocalReference(url, 'reports/proof.html') }; }
    catch (error) { return { url, reason: error.reason }; }
  });
  assert.deepEqual(aboveRoot, [
    { url: 'http://artifact.invalid/../outside.png', reason: 'path-traversal' },
    { url: '//artifact.invalid/%2e%2e/outside.png', reason: 'path-traversal' },
  ]);
  const controls = [
    ['http://artifact.invalid/..\n/outside.png', 'reports/proof.html'],
    ['/.\t./outside.png', 'proof.html'],
    ['..\r/outside.png', 'proof.html'],
  ];
  const controlOutcomes = controls.map(([url, owner]) => {
    try { return { url, owner, accepted: resolveLocalReference(url, owner) }; }
    catch (error) { return { url, owner, reason: error.reason }; }
  });
  assert.deepEqual(controlOutcomes, controls.map(([url, owner]) => ({ url, owner, reason: 'path-traversal' })));
  for (const [url, owner, expected] of [
    [' \nhttp://artifact.invalid/assets/red.png\n ', 'proof.html', 'assets/red.png'],
    ['\u0001../assets/red.png\u001f', 'reports/proof.html', 'assets/red.png'],
    ['assets/re\td.png', 'proof.html', 'assets/red.png'],
    ['http://artifact.invalid/assets/my%20proof.png', 'proof.html', 'assets/my proof.png'],
    ['//artifact.invalid/assets/π.png', 'proof.html', 'assets/π.png'],
  ]) assert.equal(resolveLocalReference(url, owner), expected);
  for (const url of ['http://artifact.invalid/assets%2Fred.png', 'http://artifact.invalid/assets%5Cred.png']) {
    assert.throws(() => resolveLocalReference(url, 'proof.html'), (error) => error.reason === 'path-traversal');
  }
});

test('actual byte/resource/duplicate budgets reject registered excess', async () => {
  for (const id of ['main-byte-limit', 'asset-byte-limit', 'aggregate-byte-limit', 'resource-count-limit']) {
    const item = byId(id);
    const input = await fixtureDelivery(item.delivery);
    assert.throws(() => admitDelivery(input), (error) => error.reason === item.expected.reason);
  }
  const input = await fixtureDelivery(byId('nested-relative-resolution').left);
  input.assets.push(input.assets[0]);
  assert.throws(() => admitDelivery(input), (error) => error.reason === 'duplicate-asset');
  const wrong = await fixtureDelivery(byId('wrong-image-sha').delivery);
  assert.throws(() => admitDelivery(wrong), (error) => error.reason === 'asset-sha');
});

test('bounded source tile arithmetic includes the native maximum once', () => {
  assert.deepEqual(tilePositions(2080), [0, 640, 1280]);
  assert.deepEqual(tilePositions(800), [0]);
  assert.deepEqual(tilePositions(801), [0, 1]);
  assert.throws(() => tilePositions(12801), (error) => error.reason === 'document-height-limit');
});
