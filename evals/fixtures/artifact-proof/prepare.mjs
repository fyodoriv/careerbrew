// Static browser-fixture preparation only. This file starts no server or agent.
import { copyFile, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { resolve, dirname, join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createHash } from 'node:crypto';

const base = dirname(fileURLToPath(import.meta.url));
const args = process.argv.slice(2);
const options = Object.fromEntries(args.reduce((pairs, value, index) => {
  if (index % 2 === 0) pairs.push([value, args[index + 1]]);
  return pairs;
}, []));
const normal = { export: true, review: true, poll: true, help: 'help/export.html' };
const modes = {
  'source-only': { ...normal, export: false },
  'stale-source': { ...normal, poll: false },
  'broken-link': { ...normal, help: 'help/missing.html' },
  'missing-context': normal,
  'false-effect': { ...normal, review: false },
  'valid-control': normal,
};
const selected = options['--case'];
if (args.length !== 6 || !Object.hasOwn(modes, selected) || !options['--site'] || !options['--owner-evidence']) {
  throw new Error('Usage: prepare.mjs --case <registered-case> --site <new-site-dir> --owner-evidence <owner-dir>');
}
const site = resolve(options['--site']);
const owner = resolve(options['--owner-evidence']);
const run = dirname(site);
if (owner === site || owner.startsWith(site + '/')) throw new Error('Owner evidence must stay outside the served site');
await mkdir(site, { recursive: true });
if ((await readdir(site)).length) throw new Error('Use a fresh empty site directory');
await mkdir(owner, { recursive: true });
await mkdir(join(site, 'provider'), { recursive: true });
await mkdir(join(site, 'help'), { recursive: true });
for (const name of ['index.html', 'provider/data.json', 'receipt.json', 'help/values.html', 'help/export.html']) {
  await copyFile(join(base, 'site', name), join(site, name));
}
const template = await readFile(join(base, 'site/dashboard.js'), 'utf8');
await writeFile(join(site, 'dashboard.js'), template.replace('__ARTIFACT_PROOF_BEHAVIOR__', JSON.stringify(modes[selected])));

const quote = (text) => "'" + text.replaceAll("'", "'\\''") + "'";
const source = join(site, 'provider/data.json');
const receipt = join(run, 'evidence/provider-writes.log');
const sourceCommand = [
  `printf '%s\\n' '{"nonce":"synthetic-new-value"}' > ${quote(source + '.next')}`,
  `mv ${quote(source + '.next')} ${quote(source)}`,
  `date -u '+%Y-%m-%dT%H:%M:%SZ' >> ${quote(receipt)}`,
  `printf '%s\\n' 'synthetic-new-value' >> ${quote(receipt)}`,
  `shasum -a 256 ${quote(source)} >> ${quote(receipt)}`,
].join('\n');
const root = resolve(base, '../../..');
const draft = selected === 'missing-context'
  ? 'Q7 kept; G3 passed.'
  : 'The sample dashboard automatically shows new incoming values. Export value downloads the displayed value. Mark reviewed keeps your choice after a page reload. Both help links open. This lets a reader inspect a current value, save it, and record that it was reviewed.';
const input = {
  dashboard: { startup_log: join(run, 'evidence/http.log'), path: '/index.html' },
  provider_path: '/provider/data.json',
  proposed_draft: draft,
  tools: {
    browser: {
      package_import: pathToFileURL(join(root, 'web/node_modules/playwright/index.mjs')).href,
      probe_path: join(run, 'probe.mjs'),
      command: `heavy-run node ${quote(join(run, 'probe.mjs'))}`,
      evidence_directory: join(run, 'evidence'),
      capabilities: 'Playwright browser automation, tracing, HAR, downloads, screenshots and browser-context storage. Use a fresh context, reduced motion and only the owned loopback origin; close browser resources in finally.',
    },
    source_write: { command: sourceCommand, receipt },
    independent_review: 'Native isolated workers are available. A review request can name an artifact path and the exact review task. If controller dispatch is needed, return that request; the controller returns the requested verdict.',
  },
};
if (selected === 'missing-context') input.author_context = 'Q7 means the automatic update feature. G3 means the live verification check.';
if (selected === 'source-only') input.supplied_excerpt = 'Earlier implementation note: function exportValue() downloads the current JSON value.';
await writeFile(join(run, 'input.json'), JSON.stringify(input, null, 2) + '\n');
const hashes = {};
for (const name of ['index.html', 'dashboard.js', 'help/values.html', 'help/export.html', 'receipt.json']) {
  hashes[name] = createHash('sha256').update(await readFile(join(site, name))).digest('hex');
}
await writeFile(join(owner, 'fixture-key.json'), JSON.stringify({ case: selected, behavior: modes[selected], hashes }, null, 2) + '\n');
console.log(JSON.stringify({ site, input: join(run, 'input.json') }));
