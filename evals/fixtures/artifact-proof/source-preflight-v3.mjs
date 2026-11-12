// Pure developer tooling. Graph buffers have one delivery-invocation owner.
// Module-instance parser cache holds maintained APIs only, never delivery state.
// Graph maps become collectible when the last returned graph reference is dropped.
import { readFileSync, lstatSync, realpathSync } from 'node:fs';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import { resolve, sep } from 'node:path';
import { createHash } from 'node:crypto';
import { admitDelivery, AdmissionError, resolveLocalReference, sha256 } from './capture-visual-v3-admission.mjs';

const policy = JSON.parse(readFileSync(new URL('./source-preflight-v3-registration/policy.json', import.meta.url)));
const fixedLimits = policy.limits;
const zeroOwners = Object.freeze({ browserLaunches: 0, artifactContextsCreated: 0, metadataContextsCreated: 0, headerChildrenCreated: 0, codecCalls: 0 });
const pairs = new Set(policy.refusalStagePairs.map(({ reason, failedStage }) => `${reason}:${failedStage}`));
const toolsRoot = resolve(fileURLToPath(new URL('./source-preflight-v3-tools/', import.meta.url)));
const requireTools = createRequire(new URL('./source-preflight-v3-tools/package.json', import.meta.url));
let parserApis;

function maintainedParsers() {
  if (parserApis) return parserApis;
  const [major, minor] = process.versions.node.split('.').map(Number);
  if (major !== 24 || minor < 15) throw new Error('tooling-unavailable');
  if (lstatSync(toolsRoot).isSymbolicLink() || lstatSync(resolve(toolsRoot, 'node_modules')).isSymbolicLink()) throw new Error('tooling-unavailable');
  const lock = readFileSync(resolve(toolsRoot, 'package-lock.json'));
  if (sha256(lock) !== 'de942aeed26ffd7b98780192cf89bc2c36addd1588533bbd1284c8e2041d501a') throw new Error('tooling-unavailable');
  const versions = { parse5: '8.1.0', 'css-tree': '3.2.1', 'data-urls': '8.0.0' };
  const loaded = {};
  for (const [name, version] of Object.entries(versions)) {
    const root = resolve(toolsRoot, 'node_modules', name);
    if (lstatSync(root).isSymbolicLink()) throw new Error('tooling-unavailable');
    const manifest = JSON.parse(readFileSync(resolve(root, 'package.json')));
    const modulePath = realpathSync(requireTools.resolve(name));
    if (manifest.version !== version || !modulePath.startsWith(`${realpathSync(root)}${sep}`)) throw new Error('tooling-unavailable');
    loaded[name] = requireTools(name);
  }
  parserApis = Object.freeze({ html: loaded.parse5, css: loaded['css-tree'], dataUrl: loaded['data-urls'].default ?? loaded['data-urls'] });
  return parserApis;
}

class SourceRefusal extends Error {
  constructor(reason, failedStage) { super(reason); this.reason = reason; this.failedStage = failedStage; }
}
const refuse = (reason, stage) => { throw new SourceRefusal(reason, stage); };
const syntax = () => refuse('unsupported-reference-syntax', 'source-syntax');
const asciiLower = (value) => value.replace(/[A-Z]/g, (letter) => letter.toLowerCase());
const utf8 = (bytes) => {
  try { return new TextDecoder('utf-8', { fatal: true }).decode(bytes); }
  catch { return refuse('unsupported-reference-syntax', 'source-encoding'); }
};
function rawFingerprint(raw) {
  const hash = createHash('sha256');
  const put = (value) => {
    if (value instanceof Uint8Array) { hash.update('bytes:'); hash.update(value); }
    else hash.update(`${typeof value}:${JSON.stringify(value) ?? 'undefined'};`);
  };
  if (!raw || typeof raw !== 'object') { put(raw); return hash.digest('hex'); }
  for (const key of ['entryPath', 'htmlSha256', 'html']) { put(Object.hasOwn(raw, key)); put(raw[key]); }
  put(Object.hasOwn(raw, 'assets'));
  if (Array.isArray(raw.assets)) for (const asset of raw.assets) {
    for (const key of ['path', 'mime', 'sha256', 'bytes']) put(asset?.[key]);
  }
  return hash.digest('hex');
}
const phase = (stage) => ({ 'source-encoding': 0, 'source-active-script': 1, 'source-navigation': 2, 'source-active-animation': 3, 'source-syntax': 4 }[stage] ?? 5);
function emptyStatistics(admitted) {
  return { astNodes: 0, referenceEdges: 0, expandedBundleBytes: admitted?.bundleBytes ?? 0, resourceIdentities: admitted?.assets.size ?? 0 };
}
function resultRecord(eligible, failure, stats, unchanged, graph) {
  const resources = eligible ? graph.resources : null;
  const images = resources?.filter((row) => row.role === 'image');
  const dataImages = images?.filter((row) => row.identity.startsWith('data:'));
  return Object.freeze({
    sourceEligible: eligible,
    sourceDisposition: eligible ? (images.length ? 'defer-later-metadata' : 'source-preflight-eligible') : 'expected-source-refusal',
    reason: failure?.reason ?? null, failedStage: failure?.failedStage ?? null,
    imageCandidates: eligible ? images.length : null,
    stylesheetCandidates: eligible ? resources.filter((row) => row.role === 'css').length : null,
    fontCandidates: eligible ? resources.filter((row) => row.role === 'font').length : null,
    ...zeroOwners, originalSourceBytesUnchanged: unchanged, ...stats,
    laterHeaderDecision: 'deferred', decodedImageBodySha256: eligible && dataImages.length === 1 ? dataImages[0].sha256 : null,
    ...(eligible ? { boundResourceSet: resources, graph } : {}),
  });
}
function admissionFailure(error) {
  if (!(error instanceof AdmissionError)) throw error;
  if (['delivery-record-unavailable', 'asset-manifest-missing', 'entry-path-missing', 'main-hash-missing', 'main-hash-mismatch'].includes(error.reason)) return new SourceRefusal(error.reason, 'raw-admission');
  if (['main-byte-limit', 'asset-byte-limit', 'asset-sha', 'aggregate-byte-limit', 'resource-count-limit'].includes(error.reason)) return new SourceRefusal(error.reason, 'declared-admission');
  if (error.reason === 'unsupported-main-encoding') return new SourceRefusal('unsupported-reference-syntax', 'source-encoding');
  if (error.reason === 'path-traversal') return new SourceRefusal(error.reason, 'source-reference-binding');
  throw error;
}
function admittedInput(raw) {
  try { return { admitted: admitDelivery(raw) }; }
  catch (error) { return { failure: admissionFailure(error) }; }
}

function buildGraph(admitted, state) {
  const { html: htmlParser, css, dataUrl } = maintainedParsers();
  const issues = [];
  const actions = [];
  const bound = new Map();
  const declared = new Map();
  const payloads = new Map();
  const edges = [];
  const visitedCss = new Set();
  const activeCss = new Set();
  const cssImports = new Map();
  const checkedImportDepths = new Set();
  const inline = new Set();
  const mainIdentity = `main:${admitted.entryPath}`;
  const main = Object.freeze({ role: 'html', identity: mainIdentity, mime: 'text/html', bytes: admitted.htmlBytes.length, sha256: sha256(admitted.htmlBytes) });
  payloads.set(mainIdentity, Buffer.from(admitted.htmlBytes));
  for (const asset of admitted.assets.values()) {
    const identity = `local:${asset.path}`;
    declared.set(identity, Object.freeze({ identity, mime: asset.mime, bytes: asset.bytes.length, sha256: sha256(asset.bytes) }));
    payloads.set(identity, Buffer.from(asset.bytes));
  }
  const issue = (error) => {
    if (!(error instanceof SourceRefusal)) throw error;
    issues.push(error);
  };
  const attempt = (fn) => { try { return fn(); } catch (error) { issue(error); return undefined; } };
  function count(depth) {
    state.astNodes += 1;
    if (state.astNodes > fixedLimits.astNodes || depth > fixedLimits.astDepth) refuse('unsupported-reference-syntax', 'source-ast-budget');
  }
  // Parse/account each source once; validate its cached imports at every depth.
  function validateImportDepth(key, depth, path) {
    if (path.has(key)) refuse('unsupported-reference-syntax', 'source-import-cycle');
    if (depth > fixedLimits.importDepth) refuse('unsupported-reference-syntax', 'source-import-depth');
    const context = `${key}:${depth}`;
    if (checkedImportDepths.has(context)) return;
    path.add(key);
    for (const child of cssImports.get(key) ?? []) validateImportDepth(child, depth + 1, path);
    path.delete(key);
    checkedImportDepths.add(context);
  }
  function bind(reference, role, source, offset, importDepth) {
    state.referenceEdges += 1;
    if (state.referenceEdges > fixedLimits.referenceEdges) refuse('resource-count-limit', 'source-edge-budget');
    let identity, mime, bytes;
    if (/^data:/i.test(reference)) {
      if (role !== 'image') syntax();
      if (Buffer.byteLength(reference) > fixedLimits.dataInputUtf8Bytes) refuse('asset-byte-limit', 'declared-admission');
      const parsed = dataUrl(reference);
      if (!parsed) refuse('image-decode', 'source-data-url');
      mime = parsed.mimeType.essence;
      if (!policy.dataUrls.allowedMimes.includes(mime)) refuse('unsupported-image-format', 'source-data-url');
      bytes = Buffer.from(parsed.body);
      if (bytes.length > fixedLimits.dataDecodedBytes) refuse('asset-byte-limit', 'declared-admission');
      identity = `data:${mime}:${sha256(bytes)}`;
      if (!inline.has(identity)) {
        inline.add(identity);
        state.expandedBundleBytes += bytes.length;
        state.resourceIdentities += 1;
        if (state.resourceIdentities > fixedLimits.resources) refuse('resource-count-limit', 'source-resource-budget');
        if (state.expandedBundleBytes > fixedLimits.expandedBundleBytes) refuse('aggregate-byte-limit', 'source-expanded-byte-budget');
      }
    } else {
      let path;
      try { path = resolveLocalReference(reference, source.path); }
      catch (error) {
        if (!(error instanceof AdmissionError)) throw error;
        if (error.reason === 'unsupported-resource-url') syntax();
        refuse(error.reason, 'source-reference-binding');
      }
      const asset = admitted.assets.get(path);
      if (!asset) refuse('missing-asset', 'source-reference-binding');
      identity = `local:${path}`; mime = asset.mime; bytes = asset.bytes;
      if ((role === 'css' && mime !== 'text/css') || (role === 'font' && !mime.startsWith('font/')) || (role === 'image' && !mime.startsWith('image/'))) syntax();
    }
    const descriptor = Object.freeze({ role, identity, mime, bytes: bytes.length, sha256: sha256(bytes) });
    if (bound.has(identity) && bound.get(identity).role !== role) syntax();
    bound.set(identity, descriptor);
    if (!payloads.has(identity)) payloads.set(identity, Buffer.from(bytes));
    edges.push(Object.freeze({ sourceSha256: source.sha256, containerCodeUnitOffset: source.containerOffset ?? 0, astCodeUnitOffset: offset, role, identity }));
    if (role === 'css') {
      const key = `${identity}:${descriptor.sha256}`;
      if (source.stylesheetKey) cssImports.get(source.stylesheetKey).push(key);
      if (activeCss.has(key)) refuse('unsupported-reference-syntax', 'source-import-cycle');
      if (importDepth > fixedLimits.importDepth) refuse('unsupported-reference-syntax', 'source-import-depth');
      if (!visitedCss.has(key)) {
        visitedCss.add(key); activeCss.add(key);
        cssImports.set(key, []);
        try { parseCss(utf8(bytes), 'stylesheet', { path: identity.slice(6), sha256: descriptor.sha256, stylesheetKey: key }, importDepth); }
        finally { activeCss.delete(key); }
      }
      validateImportDepth(key, importDepth, new Set(activeCss));
    }
  }
  function parseCss(text, context, source, importDepth) {
    let tree;
    try {
      tree = css.parse(text, { context, ...policy.css.parseOptions, onParseError() { issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-syntax')); } });
    } catch { issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-syntax')); return; }
    let depth = -1;
    css.walk(tree, { enter(node) {
      depth += 1; count(depth);
      const name = (value) => asciiLower(css.ident.decode(value));
      if (node.type === 'Atrule' && name(node.name) === 'charset') issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-encoding'));
      if ((node.type === 'Atrule' && policy.css.earlyActiveAtRules.includes(name(node.name))) || (node.type === 'Declaration' && policy.css.activePropertiesAnyValue.includes(name(node.property)))) issues.push(new SourceRefusal('active-animation', 'source-active-animation'));
    }, leave() { depth -= 1; } });
    const references = [];
    css.walk(tree, { enter(node) {
      attempt(() => {
        const name = (value) => asciiLower(css.ident.decode(value));
        const atrule = this.atrule && name(this.atrule.name);
        if (node.type === 'Raw') syntax();
        if (node.type === 'Atrule') {
          const key = name(node.name);
          if (key === 'charset' || policy.css.earlyActiveAtRules.includes(key)) return;
          if (!Object.hasOwn(policy.css.atRules, key)) syntax();
          if (key === 'import') {
            const parts = node.prelude?.children?.toArray() ?? [];
            if (node.block || parts.length !== 1 || !['String', 'Url'].includes(parts[0].type)) syntax();
            references.push({ reference: parts[0].value, role: 'css', offset: parts[0].loc?.start.offset ?? 0, depth: importDepth + 1 });
          } else if (key === 'font-face') {
            if (node.prelude || !node.block) syntax();
          } else {
            const matched = css.lexer.matchAtrulePrelude(key, node.prelude);
            if (matched.error || !matched.matched || !node.block) syntax();
          }
        }
        if (node.type === 'Declaration') {
          const property = name(node.property);
          if (policy.css.activePropertiesAnyValue.includes(property)) return;
          if (property.startsWith('--')) syntax();
          const fontFace = atrule === 'font-face';
          if (!(fontFace ? policy.css.fontFaceDescriptors : policy.css.properties).includes(property)) syntax();
          const matched = fontFace ? css.lexer.matchAtruleDescriptor('font-face', property, node.value) : css.lexer.matchProperty(property, node.value);
          if (matched.error || !matched.matched) syntax();
        }
        if (node.type === 'Function') {
          const key = name(node.name);
          if (!Object.hasOwn(policy.css.functions, key)) syntax();
          if (key === 'format') {
            const values = node.children.toArray();
            if (atrule !== 'font-face' || name(this.declaration?.property ?? '') !== 'src' || values.length !== 1 || values[0].type !== 'String' || values[0].value !== 'truetype') syntax();
          }
        }
        if (node.type === 'Url' && atrule !== 'import') {
          references.push({ reference: node.value, role: atrule === 'font-face' ? 'font' : 'image', offset: node.loc?.start.offset ?? 0, depth: importDepth });
        }
      });
    } });
    for (const ref of references) attempt(() => bind(ref.reference, ref.role, source, ref.offset, ref.depth));
  }

  const document = htmlParser.parse(utf8(admitted.htmlBytes), {
    scriptingEnabled: false, sourceCodeLocationInfo: true,
    onParseError() { issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-syntax')); },
  });
  if (document.mode !== 'no-quirks') issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-syntax'));
  const source = { path: admitted.entryPath, sha256: main.sha256 };
  const pending = [{ node: document, depth: 0 }];
  while (pending.length) {
    const { node, depth } = pending.pop(); count(depth);
    const children = [...(node.childNodes ?? []), ...(node.content ? [node.content] : [])];
    for (let i = children.length - 1; i >= 0; i -= 1) pending.push({ node: children[i], depth: depth + 1 });
    if (!node.tagName) continue;
    const attrs = Object.fromEntries(node.attrs.map((attr) => [attr.name, attr.value]));
    const offset = node.sourceCodeLocation?.startOffset ?? 0;
    if (node.attrs.some((attr) => asciiLower(attr.name).startsWith('on')) || (node.tagName === 'script' && asciiLower(attrs.type ?? '') !== 'application/json')) issues.push(new SourceRefusal('active-script', 'source-active-script'));
    if (node.tagName === 'meta' && asciiLower(attrs['http-equiv'] ?? '') === 'refresh') issues.push(new SourceRefusal('navigation', 'source-navigation'));
    if (node.tagName === 'meta' && attrs.charset && asciiLower(attrs.charset) !== 'utf-8') issues.push(new SourceRefusal('unsupported-reference-syntax', 'source-encoding'));
    attempt(() => {
      if (node.namespaceURI !== policy.html.namespace || !policy.html.tags.includes(node.tagName)) syntax();
      const allowed = [...policy.html.attributes['*'], ...(policy.html.attributes[node.tagName] ?? [])];
      if (node.attrs.some((attr) => attr.namespace || !allowed.includes(attr.name))) syntax();
      if (node.tagName === 'meta' && Object.hasOwn(attrs, 'http-equiv') && asciiLower(attrs['http-equiv']) !== 'refresh') syntax();
      if (node.tagName === 'link' && (asciiLower(attrs.rel ?? '') !== 'stylesheet' || !attrs.href)) syntax();
    });
    if (attrs.style) actions.push(() => parseCss(attrs.style, 'declarationList', { ...source, containerOffset: node.sourceCodeLocation?.attrs?.style?.startOffset ?? offset }, 0));
    if (node.tagName === 'style') actions.push(() => parseCss((node.childNodes ?? []).map((child) => child.value ?? '').join(''), 'stylesheet', { ...source, containerOffset: node.childNodes?.[0]?.sourceCodeLocation?.startOffset ?? offset }, 0));
    if (node.tagName === 'link' && asciiLower(attrs.rel ?? '') === 'stylesheet' && attrs.href) actions.push(() => bind(attrs.href, 'css', source, offset, 0));
    if (node.tagName === 'img' && attrs.src) actions.push(() => bind(attrs.src, 'image', source, offset, 0));
  }
  for (const action of actions) attempt(action);
  if (issues.length) throw issues.reduce((first, next) => phase(next.failedStage) < phase(first.failedStage) ? next : first);
  const resources = Object.freeze([...bound.values()].sort((a, b) => a.identity < b.identity ? -1 : a.identity > b.identity ? 1 : 0));
  return Object.freeze({ main, resources, declaredAssets: Object.freeze([...declared.values()]), edges: Object.freeze(edges), entryPath: admitted.entryPath,
    copyPayload(identity) {
      const descriptor = identity === mainIdentity ? main : bound.get(identity) ?? declared.get(identity);
      const retained = payloads.get(identity);
      if (!descriptor || !retained || sha256(retained) !== descriptor.sha256) throw new Error('graph-integrity');
      return Buffer.from(retained);
    },
  });
}
function parsedResult(raw, admitted, before) {
  const stats = emptyStatistics(admitted);
  try {
    const graph = buildGraph(admitted, stats);
    return resultRecord(true, null, stats, before === rawFingerprint(raw), graph);
  }
  catch (error) {
    const failure = error instanceof SourceRefusal && pairs.has(`${error.reason}:${error.failedStage}`) ? error : { reason: 'adapter-internal-error', failedStage: 'adapter-internal' };
    return resultRecord(false, failure, stats, before === rawFingerprint(raw));
  }
}
export function preflightSource(rawDelivery) {
  let before;
  try {
    const input = admittedInput(rawDelivery);
    before = rawFingerprint(rawDelivery);
    if (input.failure) return resultRecord(false, input.failure, emptyStatistics(), before === rawFingerprint(rawDelivery));
    return parsedResult(rawDelivery, input.admitted, before);
  } catch { return resultRecord(false, { reason: 'adapter-internal-error', failedStage: 'adapter-internal' }, emptyStatistics(), before !== undefined && before === rawFingerprint(rawDelivery)); }
}
export function semanticSourceReceipt(result) {
  const fields = ['sourceEligible', 'sourceDisposition', 'reason', 'failedStage', 'imageCandidates', 'stylesheetCandidates', 'fontCandidates', ...Object.keys(zeroOwners), 'originalSourceBytesUnchanged', 'astNodes', 'referenceEdges', 'expandedBundleBytes', 'resourceIdentities', 'laterHeaderDecision', 'decodedImageBodySha256', 'boundResourceSet', 'primaryReason', 'sourceOwnerTerminal', 'lateResultCanRestoreEligibility', 'ownerEvidenceKind', 'actualProcessEvidence'];
  const semantic = Object.fromEntries(fields.filter((field) => Object.hasOwn(result, field)).map((field) => [field, result[field]]));
  if (semantic.boundResourceSet) for (const row of semantic.boundResourceSet) {
    if (Object.keys(row).sort().join(',') !== 'bytes,identity,mime,role,sha256') throw new Error('invalid-source-metadata');
  }
  return JSON.parse(JSON.stringify(semantic));
}
// Synchronous injected decisions only. An actual asynchronous worker remains held.
export function decideSourceOwner(rawDelivery, ports) {
  const input = admittedInput(rawDelivery);
  const before = rawFingerprint(rawDelivery);
  if (input.failure) return resultRecord(false, input.failure, emptyStatistics(), before === rawFingerprint(rawDelivery));
  let token, built, primary, terminal = 'port-reports-never-created';
  const thunk = () => { built ??= parsedResult(rawDelivery, input.admitted, before); return built; };
  try { token = ports.create(thunk); }
  catch { primary = new SourceRefusal('source-preflight-unavailable', 'source-owner-create'); }
  if (!primary) {
    try {
      const event = ports.result(token);
      if (event.kind === 'deadline' || event.elapsedMs >= fixedLimits.sourceDeadlineMs) primary = new SourceRefusal('source-preflight-unavailable', 'source-deadline');
      else if (event.kind === 'exit' || !Number.isFinite(event.elapsedMs)) primary = new SourceRefusal('source-preflight-unavailable', 'source-owner');
      else if (event.outputBytes > policy.sourceWorker.maxOutputBytes) primary = new SourceRefusal('source-preflight-unavailable', 'source-output-bound');
      else if (event.kind !== 'result' || !built || event.value !== built) primary = new SourceRefusal('source-preflight-unavailable', 'source-owner');
    } catch { primary = new SourceRefusal('source-preflight-unavailable', 'source-owner'); }
    try { terminal = ports.close(token).terminal === 'reaped' ? 'port-reports-reaped' : 'unknown'; }
    catch { terminal = 'unknown'; }
  }
  if (terminal === 'unknown') {
    const failed = resultRecord(false, new SourceRefusal('cleanup-failed', 'source-cleanup'), emptyStatistics(input.admitted), before === rawFingerprint(rawDelivery));
    return Object.freeze({ ...failed, primaryReason: primary?.reason ?? built?.reason ?? 'source-preflight-unavailable', sourceOwnerTerminal: terminal, lateResultCanRestoreEligibility: false, ownerEvidenceKind: 'injected-unit-port', actualProcessEvidence: false });
  }
  const result = primary ? resultRecord(false, primary, emptyStatistics(input.admitted), before === rawFingerprint(rawDelivery)) : built;
  return Object.freeze({ ...result, primaryReason: primary?.reason ?? result.reason, sourceOwnerTerminal: terminal, lateResultCanRestoreEligibility: false, ownerEvidenceKind: 'injected-unit-port', actualProcessEvidence: false });
}
