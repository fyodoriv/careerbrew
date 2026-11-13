// Owner measurement tooling assigned to rust-27. No server or agent is started.
import assert from 'node:assert/strict';
import { createHash } from 'node:crypto';
import { lstat, mkdir, readFile, readdir, writeFile } from 'node:fs/promises';
import { createRequire } from 'node:module';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const sourcePath = fileURLToPath(import.meta.url);
const sourceDirectory = dirname(sourcePath);
const require = createRequire(new URL('../../../web/package.json', import.meta.url));
const { chromium } = require('playwright');
const playwrightVersion = require('playwright/package.json').version;
const METHOD = 'V2.2';
const CASES_SHA256 = '692f0bf3c4dbb3ef8a1fedc4cd4000415eb0b3977acc21116fd82c381a1444c1';
const POLICY = Object.freeze({
  viewport: { width: 1280, height: 800 }, colorScheme: 'light', reducedMotion: 'reduce',
  locale: 'en-US', timezoneId: 'UTC', javaScriptEnabled: false, offline: true,
});
const IMAGE_POLICY = 'Literal decoded author alt surrogate only; no image bytes or pixels read. Original IMG stays in place. Fixed markers are substituted after native text extraction.';
const MAIN_URL = 'http://projection.invalid/frozen-artifact';
const CSP = "default-src 'none'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
const hash = (bytes) => createHash('sha256').update(bytes).digest('hex');
const utf8 = (bytes) => new TextDecoder('utf-8', { fatal: true }).decode(bytes);

async function controlledContext(browser, htmlBytes = null) {
  // Request records belong to this disposable browser context, never another run.
  const aborted = [];
  const loader = { mainFrame: null, fulfilled: [] };
  const lifecycle = { contextClosed: false };
  const context = await browser.newContext(POLICY);
  await context.route('**/*', async (route) => {
    const request = route.request();
    if (htmlBytes !== null && loader.fulfilled.length === 0 && request.url() === MAIN_URL && request.isNavigationRequest() && request.frame() === loader.mainFrame) {
      loader.fulfilled.push({ url: MAIN_URL, status: 200, sourceSha256: hash(htmlBytes), csp: CSP, contentType: 'text/html; charset=utf-8', outboundNetwork: false });
      await route.fulfill({ status: 200, body: htmlBytes, headers: { 'content-type': 'text/html; charset=utf-8', 'content-security-policy': CSP, 'cache-control': 'no-store' } });
      return;
    }
    aborted.push({ url: request.url(), type: request.resourceType(), navigation: request.isNavigationRequest() });
    await route.abort('blockedbyclient');
  });
  return { context, aborted, loader, lifecycle };
}

async function project(browser, html, htmlBytes = Buffer.from(html, 'utf8')) {
  const { context, aborted, loader, lifecycle } = await controlledContext(browser, htmlBytes);
  const attach = (result) => ({ ...result, aborted, virtualMain: loader.fulfilled, lifecycle });
  try {
    const page = await context.newPage();
    loader.mainFrame = page.mainFrame();
    page.setDefaultTimeout(30000);
    let response;
    try { response = await page.goto(MAIN_URL, { waitUntil: 'load' }); }
    catch (error) {
      if (loader.fulfilled.length === 1 && aborted.some((request) => request.navigation)) return attach({ eligible: false, reason: 'Unsupported artifact navigation' });
      throw error;
    }
    assert.equal(loader.fulfilled.length, 1, 'Owned virtual main response is required');
    assert.equal(response?.status(), 200, 'Owned virtual main response failed');
    const responseHeaders = await response.allHeaders();
    assert.equal(responseHeaders['content-security-policy'], CSP, 'CSP response header differs');
    const responseHash = hash(await response.body());
    assert.equal(responseHash, hash(htmlBytes), 'Virtual response did not preserve original HTML bytes');
    loader.fulfilled[0].observedHeaders = responseHeaders;
    loader.fulfilled[0].observedBodySha256 = responseHash;
    if (page.url() !== MAIN_URL) return attach({ eligible: false, reason: 'Artifact changed the owned document' });
    const prepared = await page.evaluate(() => {
      const doc = document;
      const reject = (reason) => ({ eligible: false, reason });
      if (doc.querySelector('base,iframe,frame,frameset,object,embed,canvas,svg,math,video,audio,form,input,button,select,textarea,details,dialog')) return reject('Unsupported active or non-text surface');
      if (doc.querySelector('link[rel~="stylesheet"],link[rel~="preload"],link[rel~="modulepreload"],meta[http-equiv="refresh" i]')) return reject('Unsupported external or dynamic dependency');
      for (const script of doc.querySelectorAll('script')) {
        if (script.hasAttribute('src') || !['application/json', 'application/ld+json'].includes((script.getAttribute('type') || '').trim().toLowerCase())) return reject('Executable script dependency');
      }
      for (const element of doc.querySelectorAll('*')) {
        if ([...element.attributes].some((attribute) => attribute.name.toLowerCase().startsWith('on'))) return reject('Active event-handler attribute');
      }
      return { eligible: true };
    });
    if (!prepared.eligible) return attach(prepared);
    const result = await page.evaluate(() => {
      if (typeof Element.prototype.checkVisibility !== 'function') return { eligible: false, reason: 'Required browser visibility API unavailable' };
      const reject = (reason) => ({ eligible: false, reason });
      const checkRules = (rules) => {
        for (const rule of rules) {
          if ([3, 5, 7].includes(rule.type)) return 'External font/import or animated CSS';
          if (rule.constructor.name === 'CSSContainerRule') return 'Unsupported container query';
          if (rule.selectorText && (/::?first-(letter|line)\b/i.test(rule.selectorText) || rule.selectorText.includes('\\'))) return 'Unsupported text-context pseudo or escaped selector';
          if (rule.style && /\burl\s*\(/i.test(rule.style.cssText)) return 'CSS resource dependency';
          if (rule.cssRules) { const reason = checkRules(rule.cssRules); if (reason) return reason; }
        }
        return null;
      };
      for (const sheet of document.styleSheets) {
        let reason;
        try { reason = checkRules(sheet.cssRules); } catch { return reject('Stylesheet could not be inspected'); }
        if (reason) return reject(reason);
      }
      const visible = (element) => element.checkVisibility({ checkOpacity: true, checkVisibilityCSS: true, contentVisibilityAuto: true });
      for (const metadata of document.body.querySelectorAll('script,style,template')) {
        if (visible(metadata)) return reject('Unsupported rendered metadata');
      }
      for (const element of [document.documentElement, document.body, ...document.body.querySelectorAll('*')]) {
        const style = getComputedStyle(element);
        if (style.display === 'contents' || !['visible', ''].includes(style.contentVisibility)) return reject('Unsupported deferred or boxless visibility');
        if (['HTML', 'BODY'].includes(element.tagName) && [style.overflowX, style.overflowY].some((value) => ['hidden', 'clip'].includes(value))) return reject('Unsupported root or body clipping');
        if (!visible(element) && ![...element.querySelectorAll('*')].some(visible)) continue;
        if (style.position !== 'static' || style.transform !== 'none' || style.float !== 'none' || ['flex', 'inline-flex', 'grid', 'inline-grid', 'contents'].includes(style.display) || style.order !== '0') return reject('Unsupported positioned or reordered content');
        if (style.clip !== 'auto' || style.clipPath !== 'none' || style.maskImage !== 'none' || style.textOverflow === 'ellipsis' || (style.webkitLineClamp && style.webkitLineClamp !== 'none') || !['visible', ''].includes(style.contentVisibility)) return reject('Unsupported clipped or deferred content');
        // Native IMG clipping affects pixels; this method emits author alt text only.
        const imagePixelClip = element.tagName === 'IMG' && style.overflowX === 'clip' && style.overflowY === 'clip';
        if (!['HTML', 'BODY'].includes(element.tagName) && !imagePixelClip && (style.overflowX !== 'visible' || style.overflowY !== 'visible')) return reject(`Unsupported clipped or scrolling element: ${element.tagName} overflowX=${style.overflowX} overflowY=${style.overflowY}`);
        if (style.animationName !== 'none' || style.transitionDuration.split(',').some((value) => parseFloat(value) !== 0)) return reject('Dynamic CSS dependency');
        if (style.direction !== 'ltr' || style.writingMode !== 'horizontal-tb') return reject('Unsupported text reading order');
        for (const pseudo of ['::before', '::after']) {
          const content = getComputedStyle(element, pseudo).content;
          if (!['none', 'normal', '""', "''"].includes(content)) return reject('Generated CSS text');
        }
      }
      if (document.querySelector('table table,td[rowspan]:not([rowspan="1"]),th[rowspan]:not([rowspan="1"]),td[colspan]:not([colspan="1"]),th[colspan]:not([colspan="1"])')) return reject('Unsupported complex table');
      const excluded = new Set(['HEAD', 'STYLE', 'SCRIPT', 'TEMPLATE']);
      const blockTags = new Set(['ADDRESS', 'ARTICLE', 'ASIDE', 'BLOCKQUOTE', 'DIV', 'DL', 'DT', 'DD', 'FIGURE', 'FIGCAPTION', 'FOOTER', 'HEADER', 'MAIN', 'NAV', 'SECTION', 'P', 'PRE', 'UL', 'OL', 'TABLE', 'H1', 'H2', 'H3', 'H4', 'H5', 'H6']);
      const textVisible = (node) => {
        const parent = node.parentElement;
        if (!parent || !visible(parent)) return false;
        const color = getComputedStyle(parent).color;
        if (color === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(color)) return false;
        const range = document.createRange();
        range.selectNodeContents(node);
        return [...range.getClientRects()].some((rect) => rect.width > 0 && rect.height > 0);
      };
      // These snapshots belong to this extraction page and disappear with its context.
      const originalText = new WeakMap();
      const tokenNodes = new WeakSet();
      const tokens = [];
      const originalElements = [...document.querySelectorAll('*')];
      const attributes = originalElements.map((element) => JSON.stringify([...element.attributes].map((attribute) => [attribute.name, attribute.value])));
      const format = (element) => {
        const style = getComputedStyle(element);
        return JSON.stringify([...style].filter((name) => /^(font|text-|white-space|line-height|letter-spacing|word-spacing|writing-mode|direction|tab-size|color|opacity|visibility|display|content-visibility|-webkit-text-)/.test(name)).sort().map((name) => [name, style.getPropertyValue(name)]));
      };
      const allText = [];
      const allWalker = document.createTreeWalker(document, NodeFilter.SHOW_TEXT);
      while (allWalker.nextNode()) {
        const node = allWalker.currentNode;
        allText.push({ node, value: node.nodeValue, parent: node.parentElement, visible: visible(node.parentElement), format: format(node.parentElement) });
      }
      const nativeParents = new Map();
      const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
      while (walker.nextNode()) {
        const node = walker.currentNode;
        if (node.parentElement.closest('head,style,script,template')) continue;
        let opacityHidden = false;
        for (let parent = node.parentElement; parent; parent = parent.parentElement) {
          if (Number(getComputedStyle(parent).opacity) === 0) opacityHidden = true;
        }
        const color = getComputedStyle(node.parentElement).color;
        const transparent = color === 'transparent' || /rgba\([^)]*,\s*0\)$/.test(color);
        const range = document.createRange();
        range.selectNodeContents(node);
        const hasBox = [...range.getClientRects()].some((rect) => rect.width > 0 && rect.height > 0);
        originalText.set(node, { visible: textVisible(node), hiddenPaint: hasBox && (opacityHidden || transparent) });
        if (node.nodeValue.trim() && !nativeParents.has(node.parentElement)) nativeParents.set(node.parentElement, node.parentElement.innerText);
      }
      const images = [...document.body.querySelectorAll('img')].map((image) => ({
        image,
        visible: visible(image),
        alt: image.hasAttribute('alt') ? image.getAttribute('alt') : 'no alternative text supplied',
      }));
      if (document.documentElement.outerHTML.includes('\uE000')) return reject('Reserved image-token collision');
      for (const [index, { image, visible: imageVisible, alt }] of images.entries()) {
        if (!imageVisible) continue;
        const token = '\uE000' + index.toString(2).replaceAll('0', '\uE010').replaceAll('1', '\uE011') + '\uE001';
        const node = document.createTextNode(token);
        image.after(node);
        tokenNodes.add(node);
        tokens.push({ token, replacement: `[Image: ${alt}]` });
      }
      const stripTokens = (text) => tokens.reduce((value, token) => value.replaceAll(token.token, ''), text);
      const afterElements = [...document.querySelectorAll('*')];
      if (afterElements.length !== originalElements.length || afterElements.some((element, index) => element !== originalElements[index] || JSON.stringify([...element.attributes].map((attribute) => [attribute.name, attribute.value])) !== attributes[index])) return reject('Original element or attribute topology changed');
      const afterText = [];
      const afterWalker = document.createTreeWalker(document, NodeFilter.SHOW_TEXT);
      while (afterWalker.nextNode()) { if (!tokenNodes.has(afterWalker.currentNode)) afterText.push(afterWalker.currentNode); }
      if (afterText.length !== allText.length || afterText.some((node, index) => node !== allText[index].node)) return reject('Original text order changed');
      for (const state of allText) {
        if (state.node.nodeValue !== state.value || state.node.parentElement !== state.parent || visible(state.parent) !== state.visible || format(state.parent) !== state.format) return reject('Original text visibility or formatting changed');
      }
      for (const [parent, before] of nativeParents) {
        if (stripTokens(parent.innerText) !== before) return reject('Original native text context changed');
      }
      let rejection = null;
      const nativeText = (element) => {
        if (excluded.has(element.tagName)) return '';
        let anyVisible = false;
        let hiddenPaint = false;
        const textWalker = document.createTreeWalker(element, NodeFilter.SHOW_TEXT);
        while (textWalker.nextNode()) {
          const node = textWalker.currentNode;
          if (!node.nodeValue.trim() || node.parentElement.closest('head,style,script,template')) continue;
          const state = originalText.get(node);
          anyVisible ||= state?.visible === true || tokenNodes.has(node);
          hiddenPaint ||= state?.hiddenPaint === true;
        }
        if (!anyVisible) return '';
        if (hiddenPaint) { rejection = 'Unsupported mixed transparent text'; return ''; }
        return element.innerText;
      };
      const units = [];
      const emit = (text) => { if (text.length) units.push(text); };
      const collect = (container) => {
        const hasBlocks = [...container.children].some((child) => blockTags.has(child.tagName));
        if (!hasBlocks) { emit(nativeText(container)); return true; }
        let pending = '';
        const flush = () => { emit(pending.replace(/[\t\f\v ]+/g, ' ').trim()); pending = ''; };
        for (const child of container.childNodes) {
          if (child.nodeType !== Node.ELEMENT_NODE || !blockTags.has(child.tagName)) {
            if (child.nodeType === Node.TEXT_NODE) {
              if (tokenNodes.has(child)) pending += child.nodeValue;
              else if (originalText.get(child)?.visible && child.nodeValue.trim()) {
                const style = getComputedStyle(container);
                if (!['normal', 'nowrap'].includes(style.whiteSpace) || style.textTransform !== 'none') return false;
                pending += child.nodeValue.replace(/\s+/g, ' ');
              } else if (originalText.get(child)?.visible) pending += ' ';
            } else if (child.nodeType === Node.ELEMENT_NODE && !excluded.has(child.tagName)) {
              const text = nativeText(child);
              if (text) {
                const style = getComputedStyle(child);
                if (!['normal', 'nowrap'].includes(style.whiteSpace) || style.textTransform !== 'none' || text.includes('\n')) return false;
                pending += text;
              }
            }
            continue;
          }
          flush();
          if (excluded.has(child.tagName)) continue;
          if (/^H[1-6]$/.test(child.tagName)) { const text = nativeText(child); if (text) emit('#'.repeat(Number(child.tagName[1])) + ' ' + text); }
          else if (['P', 'PRE'].includes(child.tagName)) emit(nativeText(child));
          else if (['UL', 'OL'].includes(child.tagName)) {
            if (child.querySelector('li ul,li ol')) return false;
            const items = [...child.children].filter((item) => item.tagName === 'LI');
            emit(items.map((item, index) => ({ item, index, text: nativeText(item) })).filter((entry) => entry.text).map((entry) => (child.tagName === 'UL' ? '- ' : `${child.start + entry.index}. `) + entry.text).join('\n'));
          } else if (child.tagName === 'TABLE') {
            const rows = [];
            for (const row of child.rows) {
              const cells = [];
              for (const cell of row.cells) {
                if ([...cell.querySelectorAll('*')].some((nested) => blockTags.has(nested.tagName))) return false;
                const text = nativeText(cell);
                if (text.includes('\n')) return false;
                cells.push(text);
              }
              if (cells.some((text) => text)) rows.push(cells.join(' | '));
            }
            emit(rows.join('\n'));
          } else if (collect(child) === false) return false;
        }
        flush();
        return true;
      };
      if (collect(document.body) === false) return reject('Unsupported mixed formatting, nested list or complex cell');
      if (rejection) return reject(rejection);
      let text = units.join('\n\n').replace(/\n+$/, '') + '\n';
      if (!text.trim()) return reject('No supported visible body content');
      for (const token of tokens) {
        if (text.split(token.token).length !== 2) return reject('Owned image token was lost or duplicated');
        text = text.replace(token.token, token.replacement);
      }
      return { eligible: true, text, invariants: { originalElements: originalElements.length, originalTextNodes: allText.length, originalNativeContexts: nativeParents.size, addedTextTokens: tokens.length, elementAttributesAndTextPreserved: true, visibilityAndFormatPreserved: true, nativeContextPreserved: true } };
    });
    if (aborted.some((request) => request.type !== 'image')) return attach({ eligible: false, reason: 'Unexpected resource dependency' });
    return attach(result);
  } finally { await context.close(); lifecycle.contextClosed = true; }
}

async function environmentProbe(browser, html) {
  // This private probe receives only the hash-pinned synthetic self-test input.
  const { context, aborted } = await controlledContext(browser);
  try {
    const page = await context.newPage();
    const responses = [];
    page.on('response', (response) => responses.push(response.url()));
    await page.setContent(html, { waitUntil: 'load' });
    return { marker: await page.evaluate(() => document.documentElement.getAttribute('data-artifact-ran')), aborted: aborted.length, responses: responses.length };
  } finally { await context.close(); }
}

async function selfTest(output) {
  await mkdir(output, { recursive: true });
  assert.equal((await readdir(output)).length, 0, 'Use a fresh self-test output directory');
  const bytes = await readFile(join(sourceDirectory, 'projector-v2-cases.json'));
  assert.equal(hash(bytes), CASES_SHA256, 'Registered self-test bytes changed');
  const cases = JSON.parse(utf8(bytes));
  assert.equal(cases.groups.length, 7);
  const executions = [];
  for (let iteration = 1; iteration <= 2; iteration++) {
    const browser = await chromium.launch();
    const results = [];
    try {
      for (const group of cases.groups) {
        if (group.html) {
          const result = await project(browser, group.html);
          assert.equal(result.lifecycle.contextClosed, true, 'Projection context was not closed');
          assert.equal(result.virtualMain.length, 1, 'One virtual main response is required');
          assert.equal(result.virtualMain[0].observedBodySha256, hash(Buffer.from(group.html, 'utf8')));
          assert.equal(result.eligible, true, `Group ${group.id}: ${result.reason}`);
          assert.equal(result.text, group.expected, `Group ${group.id} output`);
          results.push({ group: group.id, text: result.text, pass: true });
        } else if (group.environment_probe_html) {
          const controls = await environmentProbe(browser, group.environment_probe_html);
          assert.equal(controls.marker, group.expected_script_marker);
          assert.equal(controls.aborted, group.expected_aborted_requests);
          assert.equal(controls.responses, group.expected_resource_responses);
          assert.equal((await project(browser, group.environment_probe_html)).eligible, false);
          results.push({ group: group.id, controls, pass: true });
        } else {
          for (const html of group.reject_html) assert.equal((await project(browser, html)).eligible, false, `Group ${group.id} unsupported input`);
          results.push({ group: group.id, rejected: group.reject_html.length, pass: true });
        }
      }
    } finally {
      const version = browser.version();
      await browser.close();
      const record = { iteration, browser: version, browserClosed: !browser.isConnected(), results };
      executions.push(record);
      await mkdir(output, { recursive: true });
      await writeFile(join(output, `run-${iteration}.json`), JSON.stringify(record, null, 2) + '\n');
    }
  }
  assert.deepEqual(executions[0].results, executions[1].results, 'Two fresh browser runs differ');
  assert.equal(executions[0].browser, executions[1].browser, 'Browser version changed between runs');
  await writeFile(join(output, 'selftest-receipt.json'), JSON.stringify({ method: METHOD, policy: POLICY, loaderPolicy: { virtualMain: MAIN_URL, cspHeader: CSP, originalHtmlBytes: true, subrequestsAborted: true }, imagePolicy: IMAGE_POLICY, codeSha256: hash(await readFile(sourcePath)), casesSha256: CASES_SHA256, playwrightVersion, groups: 7, freshBrowserRuns: 2, byteIdentity: true, pass: true }, null, 2) + '\n');
  console.log('V2 projector: 7/7 fixed groups pass in two fresh browser runs; output identity passes');
}

async function projectArtifact(outputs, expectedHash, output) {
  await mkdir(output, { recursive: true });
  assert.equal((await readdir(output)).length, 0, 'Use a fresh projection output directory');
  const selected = [];
  for (const name of ['result.html', 'dashboard-result.html']) {
    try { const stat = await lstat(join(outputs, name)); if (!stat.isFile() || stat.isSymbolicLink()) throw new Error('Canonical artifact must be a regular file'); selected.push(name); }
    catch (error) { if (error.code !== 'ENOENT') throw error; }
  }
  assert.equal(selected.length, 1, 'Exactly one canonical finished HTML is required');
  const artifactPath = join(outputs, selected[0]);
  const bytes = await readFile(artifactPath);
  assert.match(expectedHash, /^[a-f0-9]{64}$/, 'Frozen artifact SHA-256 is required');
  assert.equal(hash(bytes), expectedHash, 'Frozen artifact hash differs');
  const browser = await chromium.launch();
  let result;
  let browserVersion;
  let browserClosed = false;
  try { browserVersion = browser.version(); result = await project(browser, utf8(bytes), bytes); }
  finally { await browser.close(); browserClosed = !browser.isConnected(); }
  await mkdir(output, { recursive: true });
  const receipt = { method: METHOD, artifactPath, artifactSha256: hash(bytes), projectedInputSha256: result.eligible ? hash(Buffer.from(result.text, 'utf8')) : null, codeSha256: hash(await readFile(sourcePath)), playwrightVersion, browserVersion, policy: POLICY, loaderPolicy: { virtualMain: MAIN_URL, cspHeader: CSP, originalHtmlBytes: true, subrequestsAborted: true }, imagePolicy: IMAGE_POLICY, eligible: result.eligible, reason: result.reason || null, abortedRequests: result.aborted, virtualMainResponses: result.virtualMain, contextClosed: result.lifecycle.contextClosed, browserClosed, invariants: result.invariants || null };
  if (result.eligible) await writeFile(join(output, 'projected-input.txt'), result.text, { flag: 'wx' });
  await writeFile(join(output, 'projection-receipt.json'), JSON.stringify(receipt, null, 2) + '\n', { flag: 'wx' });
  if (!result.eligible) process.exitCode = 2;
}

const args = process.argv.slice(2);
if (args.length === 3 && args[0] === '--self-test' && args[1] === '--output') {
  await selfTest(args[2]);
} else if (args.length === 6 && args[0] === '--outputs-dir' && args[2] === '--artifact-sha256' && args[4] === '--output') {
  await projectArtifact(args[1], args[3], args[5]);
} else {
  throw new Error('Use --self-test --output <owner-dir>, or --outputs-dir <finished-dir> --artifact-sha256 <frozen-sha256> --output <new-owner-dir>');
}
