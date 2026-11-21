// Owner-side admission and geometry only; assigned to rust-27. No browser import.
import { createHash } from 'node:crypto';

export const ORIGIN = 'http://artifact.invalid';
export const LIMITS = Object.freeze({
  mainBytes: 2 * 1024 * 1024, assetBytes: 5 * 1024 * 1024,
  bundleBytes: 20 * 1024 * 1024, resources: 64, imagePixels: 16777216,
  viewportWidth: 1280, viewportHeight: 800, stepY: 640,
  documentHeight: 12800, aggregateTiles: 20, timeoutMs: 30000,
});

export class AdmissionError extends Error {
  constructor(reason) {
    super(`V3 delivery unavailable: ${reason}`);
    this.name = 'AdmissionError';
    this.reason = reason;
  }
}

const refuse = (reason) => { throw new AdmissionError(reason); };
const record = (value) => value !== null && typeof value === 'object' && !Array.isArray(value);
const own = (value, field) => Object.hasOwn(value, field);
const validHash = (value) => typeof value === 'string' && /^[a-f0-9]{64}$/i.test(value);
export const sha256 = (bytes) => createHash('sha256').update(bytes).digest('hex');

function bytesOf(value, reason) {
  if (typeof value === 'string') return Buffer.from(value, 'utf8');
  if (value instanceof Uint8Array) return Buffer.from(value);
  return refuse(reason);
}

export function normalizeBundlePath(value) {
  if (typeof value !== 'string' || !value || value.startsWith('/') || /[\\\0]/.test(value)) refuse('path-traversal');
  const segments = value.split('/');
  if (segments.some((part) => !part || part === '.' || part === '..')) refuse('path-traversal');
  return value;
}

export function entryUrl(path) {
  const original = normalizeBundlePath(path);
  return `${ORIGIN}/${original.split('/').map(encodeURIComponent).join('/')}`;
}

function preprocessUrlReference(reference) {
  return reference.replace(/^[\u0000-\u0020]+|[\u0000-\u0020]+$/g, '').replace(/[\t\n\r]/g, '');
}

export function resolveLocalReference(reference, ownerPath) {
  if (typeof reference !== 'string') refuse('path-traversal');
  const raw = preprocessUrlReference(reference);
  if (!raw || raw.includes('\\')) refuse('path-traversal');
  const base = entryUrl(ownerPath);
  let parsed;
  try { parsed = new URL(raw, base); } catch { return refuse('external-dependency'); }
  if (parsed.origin !== ORIGIN || parsed.protocol !== 'http:') refuse('external-dependency');
  // A frozen file bundle has no query-dependent response implementation.
  if (parsed.search) refuse('unsupported-resource-url');
  const withoutScheme = raw.replace(/^[a-z][a-z\d+.-]*:/i, '');
  const hasAuthority = withoutScheme.startsWith('//');
  const rawPath = (hasAuthority ? withoutScheme.replace(/^\/{2,}[^/]*/, '') : withoutScheme).split(/[?#]/, 1)[0];
  const stack = hasAuthority || rawPath.startsWith('/') ? [] : normalizeBundlePath(ownerPath).split('/').slice(0, -1);
  for (const encoded of rawPath.split('/')) {
    let part;
    try { part = decodeURIComponent(encoded); } catch { return refuse('path-traversal'); }
    if (part.includes('/') || part.includes('\\') || part.includes('\0')) refuse('path-traversal');
    if (!part || part === '.') continue;
    if (part === '..') {
      if (!stack.length) refuse('path-traversal');
      stack.pop();
    } else stack.push(part);
  }
  let path;
  try {
    const parts = parsed.pathname.slice(1).split('/').map(decodeURIComponent);
    if (parts.some((part) => /[/\\\0]/.test(part))) refuse('path-traversal');
    path = parts.join('/');
  }
  catch { return refuse('path-traversal'); }
  return normalizeBundlePath(path);
}

export function admitDelivery(raw) {
  if (!record(raw)) refuse('delivery-record-unavailable');
  if (!own(raw, 'assets') || !Array.isArray(raw.assets)) refuse('asset-manifest-missing');
  if (!own(raw, 'entryPath') || typeof raw.entryPath !== 'string' || !raw.entryPath) refuse('entry-path-missing');
  if (!own(raw, 'htmlSha256') || !validHash(raw.htmlSha256)) refuse('main-hash-missing');
  const entryPath = normalizeBundlePath(raw.entryPath);
  if (!own(raw, 'html')) refuse('main-bytes-unavailable');
  const htmlBytes = bytesOf(raw.html, 'main-bytes-unavailable');
  if (htmlBytes.length > LIMITS.mainBytes) refuse('main-byte-limit');
  if (sha256(htmlBytes) !== raw.htmlSha256.toLowerCase()) refuse('main-hash-mismatch');
  try { new TextDecoder('utf-8', { fatal: true }).decode(htmlBytes); }
  catch { return refuse('unsupported-main-encoding'); }
  if (raw.assets.length > LIMITS.resources) refuse('resource-count-limit');
  const assets = new Map();
  let bundleBytes = htmlBytes.length;
  for (const source of raw.assets) {
    if (!record(source) || !own(source, 'path')) refuse('asset-manifest-missing');
    const path = normalizeBundlePath(source.path);
    if (assets.has(path) || path === entryPath) refuse('duplicate-asset');
    if (!own(source, 'sha256') || !validHash(source.sha256)) refuse('asset-sha');
    if (typeof source.mime !== 'string' || !source.mime || /[\r\n]/.test(source.mime)) refuse('asset-mime');
    if (!own(source, 'bytes')) refuse('missing-asset');
    const bytes = bytesOf(source.bytes, 'missing-asset');
    if (bytes.length > LIMITS.assetBytes) refuse('asset-byte-limit');
    if (sha256(bytes) !== source.sha256.toLowerCase()) refuse('asset-sha');
    bundleBytes += bytes.length;
    if (bundleBytes > LIMITS.bundleBytes) refuse('aggregate-byte-limit');
    assets.set(path, Object.freeze({ path, mime: source.mime, sha256: source.sha256, bytes }));
  }
  return Object.freeze({ admitted: true, entryPath, url: entryUrl(entryPath), htmlBytes, htmlSha256: raw.htmlSha256, assets, bundleBytes });
}

export function tilePositions(documentHeight) {
  if (!Number.isSafeInteger(documentHeight) || documentHeight < 0) refuse('unstable-geometry');
  if (documentHeight > LIMITS.documentHeight) refuse('document-height-limit');
  const maximum = Math.max(0, documentHeight - LIMITS.viewportHeight);
  const result = [0];
  for (let position = LIMITS.stepY; position < maximum; position += LIMITS.stepY) result.push(position);
  if (maximum) result.push(maximum);
  if (result.length > LIMITS.aggregateTiles) refuse('aggregate-tile-limit');
  return result;
}
