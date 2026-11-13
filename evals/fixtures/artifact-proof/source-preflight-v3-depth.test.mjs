// Permanent separate pair from fb727f1a; original source suite stays 133 cases.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { preflightSource, semanticSourceReceipt } from './source-preflight-v3.mjs';

const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const canonical = (value) => JSON.stringify(value, (_key, item) => item && typeof item === 'object' && !Array.isArray(item)
  ? Object.fromEntries(Object.keys(item).sort().map((key) => [key, item[key]])) : item);
const bytes = readFileSync(new URL('./source-preflight-v3-depth-inputs.json', import.meta.url));
assert.equal(hash(bytes), '0cc58c4293b07b504a97ed53d685788af220a1a41125638d16deb33f306e2d1d');
const fixture = JSON.parse(bytes);
assert.deepEqual(fixture.cases.map((item) => item.id), ['A', 'B']);

for (const item of fixture.cases) test(`${item.id}: cached stylesheet cannot hide import depth nine`, () => {
  assert.equal(hash(canonical(item.rawDelivery)), item.rawEnvelopeSha256);
  assert.equal(hash(Buffer.from(item.rawDelivery.html)), item.htmlSha256);
  assert.equal(Buffer.byteLength(item.rawDelivery.html), item.htmlBytes);
  const raw = { ...item.rawDelivery, assets: item.rawDelivery.assets.map((asset) => {
    const bytes = Buffer.from(asset.bytes);
    assert.equal(hash(bytes), asset.sha256);
    return { ...asset, bytes };
  }) };
  const before = hash(canonical(raw));
  const observed = semanticSourceReceipt(preflightSource(raw));
  assert.equal(hash(canonical(raw)), before);
  assert.equal(observed.originalSourceBytesUnchanged, true);
  assert.equal(observed.sourceEligible, false);
  assert.equal(observed.reason, 'unsupported-reference-syntax');
  assert.equal(observed.failedStage, 'source-import-depth');
  for (const field of ['browserLaunches', 'artifactContextsCreated', 'metadataContextsCreated', 'headerChildrenCreated', 'codecCalls']) assert.equal(observed[field], 0);
});
