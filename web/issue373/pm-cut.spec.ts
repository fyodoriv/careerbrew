import AxeBuilder from "@axe-core/playwright";
import { expect, test as base } from "@playwright/test";
import type { Page, Request, TestInfo } from "@playwright/test";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";

function required(key: string): string {
  const value = process.env[key];
  if (!value) throw new Error("BLOCKED: owned373 invocation required");
  return value;
}
const RUN = required("ISSUE373_RUN");
const MODE = required("ISSUE373_MODE");
const TOKEN = "issue373-synthetic-not-a-secret";
const UNKNOWN = "Cannot verify which previously cut labels are absent from this text.";
const MODES = [
  { id: "phone-light", viewport: { width: 390, height: 664 }, colorScheme: "light" as const, hasTouch: true },
  { id: "phone-dark", viewport: { width: 390, height: 664 }, colorScheme: "dark" as const, hasTouch: true },
  { id: "desktop-light", viewport: { width: 1440, height: 900 }, colorScheme: "light" as const, hasTouch: false },
  { id: "desktop-dark", viewport: { width: 1440, height: 900 }, colorScheme: "dark" as const, hasTouch: false },
];
type Ready = { port: number; raw: string; kept: string; labels: string[]; identity: { pid: number; pgid: number; sid: number } };
type Proof = { attempts: string[]; pending: Set<Request>; records: unknown[]; measured: boolean; ready: Ready | null; scenario: string | null };

function bytes(root: string): Record<string, unknown> {
  const result: Record<string, unknown> = {};
  const walk = (folder: string) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isSymbolicLink()) throw new Error("BLOCKED: unexpected synthetic state symlink");
      if (entry.isDirectory()) walk(path);
      else if (entry.isFile()) {
        const s = statSync(path, { bigint: true });
        result[relative(root, path)] = { sha256: createHash("sha256").update(readFileSync(path)).digest("hex"), mode: Number(s.mode & 0o777n), mtimeNs: String(s.mtimeNs) };
      }
    }
  };
  walk(root);
  return result;
}

function record(info: TestInfo, name: string, value: unknown): void {
  mkdirSync(info.outputDir, { recursive: true });
  writeFileSync(info.outputPath(name), JSON.stringify(value, null, 2) + "\n", { flag: "wx" });
}

const test = base.extend<{ proof: Proof }>({
  proof: async ({ page, context }, runTest, info) => {
    const proof: Proof = { attempts: [], pending: new Set(), records: [], measured: false, ready: null, scenario: null };
    const errors: string[] = [];
    const observe = (p: Page) => {
      p.on("console", (m) => { if (m.type() === "error") errors.push(m.text()); });
      p.on("pageerror", (error) => errors.push(error.message));
      p.on("request", (r) => proof.pending.add(r));
      p.on("requestfinished", (r) => proof.pending.delete(r));
      p.on("requestfailed", (r) => proof.pending.delete(r));
    };
    observe(page);
    context.on("page", observe);
    await context.route("**/*", async (route) => {
      const request = route.request();
      const url = new URL(request.url());
      const same = proof.ready && url.origin === `http://127.0.0.1:${proof.ready.port}`;
      const readPaths = new Set(["/api/session", "/api/config", "/api/overview", "/api/changes", "/api/daylog", "/api/today", "/api/inbox", "/api/priority", "/api/feed/asks"]);
      const staticRead = request.method() === "GET" && (url.pathname === "/" || url.pathname === "/favicon.ico" || url.pathname.startsWith("/assets/"));
      const apiRead = request.method() === "GET" && (readPaths.has(url.pathname) || (url.pathname === "/api/tab-key" && url.search === ""));
      const timingOpen = !proof.measured && request.method() === "POST" && url.pathname === "/api/timing"
        && request.postDataJSON()?.saves === "pm_update" && request.postDataJSON()?.step === "open";
      const sessionSetup = !proof.measured && request.method() === "POST" && url.pathname === "/api/session" && request.postData() === "{}";
      proof.records.push({ method: request.method(), path: url.pathname, phase: proof.measured ? "measured" : "settling", allowed: Boolean(same && (staticRead || apiRead || timingOpen || sessionSetup)) });
      if (!same || !(staticRead || apiRead || timingOpen || sessionSetup)) {
        proof.attempts.push(`${request.method()} ${url.origin}${url.pathname}`);
        await route.abort("blockedbyclient");
        return;
      }
      await route.continue();
    });
    try {
      await runTest(proof);
    } finally {
      // The deny/record route stays installed through page shutdown. No blur,
      // Copy, Send or navigation is used to satisfy the display oracle.
      for (const p of context.pages()) await p.close({ runBeforeUnload: false });
      record(info, "boundary-terminal.json", { records: proof.records, attempts: proof.attempts, errors, scenario: proof.scenario, realServer: proof.ready?.identity, browserVersion: context.browser()?.version(), ownership: "maintained Playwright page/context/browser and webServer lifecycle; no external PID signal" });
      expect(proof.attempts, "BLOCKED: forbidden request through teardown").toEqual([]);
      expect(errors, "BLOCKED: actual app console/page errors").toEqual([]);
    }
  },
});

async function open(page: Page, proof: Proof, scenario: string): Promise<Ready> {
  const ready = JSON.parse(readFileSync(join(RUN, scenario, "ready.json"), "utf8")) as Ready;
  proof.ready = ready;
  proof.scenario = scenario;
  const origin = `http://127.0.0.1:${ready.port}`;
  const session = await page.context().request.post(`${origin}/api/session`, { headers: { "X-Careerbrew-Token": TOKEN } });
  expect(session.status(), "BLOCKED: real loopback owner session refused").toBe(200);
  await page.goto(`${origin}/#/today`);
  await expect(page.getByLabel("PM update", { exact: true })).toHaveValue(ready.kept);
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await page.evaluate(() => document.fonts.ready);
  await expect.poll(() => proof.pending.size).toBe(0);
  expect(proof.attempts).toEqual([]);
  return ready;
}

function state(scenario: string): Record<string, unknown> {
  return bytes(join(RUN, scenario, "world"));
}

if (MODE === "primary") {
  test("P03 unsaved restoration removes the visible absence flag without saving", async ({ page, proof }, info) => {
    const ready = await open(page, proof, "ascii");
    expect(ready.raw).toBe("billing: next: reconcile amber invoices.\nOther: reviewed 4 PRs.");
    await expect(page.locator("[data-pm-left-out]")).toContainText("Other");
    const before = state("ascii");
    proof.measured = true;
    const box = page.getByLabel("PM update", { exact: true });
    await box.fill(ready.raw);
    await expect(box).toHaveValue(ready.raw);
    await expect(box).toBeFocused();
    await expect.poll(() => proof.pending.size).toBe(0);
    expect(state("ascii"), "BLOCKED: typing changed encrypted/data/client inputs").toEqual(before);
    expect(proof.attempts).toEqual([]);
    record(info, "before-final-oracle.json", { currentText: await box.inputValue(), focused: await box.evaluate((e) => document.activeElement === e), before, after: state("ascii"), records: proof.records, existingBanner: await page.locator("[data-pm-left-out]").allTextContents() });
    await expect(page.locator("[data-pm-left-out]")).toHaveCount(0);
  });
}

if (MODE === "dates") {
  test("D02 browser subhistories preserve exact presence, uncertainty and legacy date wording", async ({ page, proof }, info) => {
    const ready = await open(page, proof, "ascii");
    const candidates: [string, "present" | "absent" | "unknown"][] = [
      ["Other: reviewed 4 PRs.", "present"], ["- **Other**: reviewed 4 PRs.", "present"], ["• _Other_: reviewed 4 PRs.", "present"], ["oThEr: reviewed 4 PRs.", "present"],
      ["Different: reviewed 4 PRs.", "absent"], ["[Other](https://example.com): reviewed 4 PRs.", "unknown"], ["# Other: reviewed 4 PRs.", "unknown"], ["> Other: reviewed 4 PRs.", "unknown"],
      ["```text\nOther: reviewed 4 PRs.\n```", "unknown"], ["1. Other: reviewed 4 PRs.", "unknown"], ["an unlabelled nonblank paragraph", "unknown"],
    ];
    const before = state("ascii");
    proof.measured = true;
    for (const [text, expected] of candidates) {
      await page.getByLabel("PM update", { exact: true }).fill(ready.kept + "\n" + text);
      await expect(page.getByLabel("PM update", { exact: true })).toBeFocused();
      if (expected === "absent") await expect(page.locator("[data-pm-left-out]")).toContainText("Other");
      else await expect(page.locator("[data-pm-left-out]")).toHaveCount(0);
      if (expected === "unknown") await expect(page.getByText(UNKNOWN, { exact: true })).toBeVisible();
      else await expect(page.getByText(UNKNOWN, { exact: true })).toHaveCount(0);
      expect(state("ascii")).toEqual(before);
    }
    record(info, "D02-ascii.json", { contracts: candidates, preserved: state("ascii") });
    await page.close({ runBeforeUnload: false });
    proof.measured = false;
    const unicode = await page.context().newPage();
    const u = await open(unicode, proof, "unicode");
    expect(u.labels).toEqual(["Équipe"]);
    const unicodeBefore = state("unicode");
    proof.measured = true;
    for (const spelling of ["Équipe", "ÉQUIPE", "E\u0301quipe"]) {
      await unicode.getByLabel("PM update", { exact: true }).fill(u.raw.replace("Équipe:", spelling + ":"));
      await expect(unicode.getByLabel("PM update", { exact: true })).toBeFocused();
      await expect(unicode.locator("[data-pm-left-out]")).toHaveCount(0);
      if (spelling === "Équipe") await expect(unicode.getByText(UNKNOWN, { exact: true })).toHaveCount(0);
      else await expect(unicode.getByText(UNKNOWN, { exact: true })).toBeVisible();
      expect(state("unicode")).toEqual(unicodeBefore);
    }
    record(info, "D02-unicode.json", { actualBuilderLabels: u.labels, preserved: state("unicode"), source: "encrypted synthetic legacy project, no learned-dictionary or API injection" });
    await unicode.close({ runBeforeUnload: false });
    for (const scenario of ["legacy-missing", "legacy-invalid", "legacy-naive", "legacy-unavailable"]) {
      proof.measured = false;
      const legacy = await page.context().newPage();
      await open(legacy, proof, scenario);
      proof.measured = true;
      const block = legacy.locator("[data-block=pm-update]");
      if (scenario === "legacy-unavailable") await expect(block.getByText("Date unavailable", { exact: true })).toBeVisible();
      else {
        await expect(block).toContainText(/Cut in update for (?:Mon Oct 26|Tue Oct 27)(?:, 2026)? \(record time unavailable\)/);
        await expect(block).not.toContainText("Cut recorded");
      }
      record(info, `${scenario}.json`, { text: await block.innerText(), state: state(scenario) });
      await legacy.close({ runBeforeUnload: false });
    }
  });
}

if (MODE === "comfort") {
  for (const mode of MODES) {
    test.describe(mode.id, () => {
      test.use({ viewport: mode.viewport, colorScheme: mode.colorScheme, hasTouch: mode.hasTouch, reducedMotion: "reduce" });
      test(`comfort ${mode.id} complete and unknown`, async ({ page, proof }, info) => {
        const ready = await open(page, proof, "crowded");
        expect(ready.labels).toHaveLength(15);
        proof.measured = true;
        for (const phase of ["complete", "unknown"]) {
          if (phase === "unknown") await page.getByLabel("PM update", { exact: true }).fill("an unlabelled nonblank paragraph");
          const result = await new AxeBuilder({ page }).withTags(["wcag2a", "wcag2aa", "wcag21aa", "wcag22aa"]).analyze();
          const geometry = await page.evaluate(() => {
            const small: string[] = [], targets: string[] = [], spills: string[] = [], contrast: string[] = [];
            const rgba = (s: string) => {
              const v = (s.match(/[\d.]+/g) ?? []).map(Number);
              return [v[0] ?? 0, v[1] ?? 0, v[2] ?? 0, v[3] ?? 1];
            };
            const blend = (fg: number[], bg: number[]) => fg.slice(0, 3).map((v, i) => v * fg[3] + bg[i] * (1 - fg[3]));
            const luminance = (rgb: number[]) => rgb.map((n) => { const x = n / 255; return x <= 0.04045 ? x / 12.92 : ((x + 0.055) / 1.055) ** 2.4; }).reduce((sum, n, i) => sum + n * [0.2126, 0.7152, 0.0722][i], 0);
            for (const e of document.querySelectorAll<HTMLElement>("main *")) {
              if (!e.getClientRects().length || e.closest(".sr-only,[hidden],[aria-hidden=true]")) continue;
              const css = getComputedStyle(e), r = e.getBoundingClientRect();
              const ownText = [...e.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim());
              if ((ownText || e.matches("textarea,input,select")) && parseFloat(css.fontSize) < 14) small.push(e.tagName);
              if (e.matches("button,a[href],textarea,input:not([type=hidden]),select,summary,[role=button]") && (r.width < 44 || r.height < 44)) targets.push(e.tagName + ":" + (e.getAttribute("aria-label") ?? e.textContent?.trim().slice(0, 30)));
              if (!e.matches("textarea,code,input") && css.display !== "inline" && e.scrollWidth > e.clientWidth + 1 && css.textOverflow !== "ellipsis") spills.push(e.tagName);
              if (ownText && e.closest("[data-block=pm-update]") && !e.closest("textarea,code")) {
                const ancestry: HTMLElement[] = [];
                for (let a: HTMLElement | null = e; a; a = a.parentElement) ancestry.unshift(a);
                let bg = [255, 255, 255];
                for (const a of ancestry) bg = blend(rgba(getComputedStyle(a).backgroundColor), bg);
                const light = luminance(blend(rgba(css.color), bg)), dark = luminance(bg);
                const ratio = (Math.max(light, dark) + 0.05) / (Math.min(light, dark) + 0.05);
                if (ratio < 4.5) contrast.push(`${e.tagName} ${ratio.toFixed(3)}`);
                if ((css.overflowX !== "visible" && e.scrollWidth > e.clientWidth + 1) || (css.overflowY !== "visible" && e.scrollHeight > e.clientHeight + 1)) spills.push("clipped-PM-text");
              }
            }
            const pm = document.querySelector<HTMLElement>("[data-block=pm-update]");
            const bannerText = pm?.innerText ?? "";
            return { small, targets, spills, contrast, sideways: document.documentElement.scrollWidth > innerWidth + 1, rawDates: /\b\d{4}-\d{2}-\d{2}\b/.test(bannerText) };
          });
          record(info, `${phase}-comfort.json`, { axe: result, geometry, tapsAddedByDisplay: 0 });
          expect(result.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
          expect(geometry.small).toEqual([]); expect(geometry.targets).toEqual([]); expect(geometry.spills).toEqual([]);
          expect(geometry.contrast).toEqual([]);
          expect(geometry.sideways).toBe(false); expect(geometry.rawDates).toBe(false);
          if (phase === "unknown") await expect(page.getByText(UNKNOWN, { exact: true })).toBeVisible();
          await page.locator("[data-block=pm-update]").scrollIntoViewIfNeeded();
          await page.screenshot({ path: info.outputPath(`${mode.id}-${phase}.png`) });
        }
        await page.locator("[data-block=pm-update]").scrollIntoViewIfNeeded();
        await page.screenshot({ path: info.outputPath(`${mode.id}.png`) });
      });
    });
  }
}

if (MODE === "proof-before" || MODE === "proof-after") {
  for (const [modeId, part] of [["phone-light", "banner"], ["desktop-dark", "banner"], ["phone-dark", "date"], ["desktop-light", "date"]]) {
    const mode = MODES.find((m) => m.id === modeId);
    if (!mode) throw new Error("BLOCKED: undeclared capture mode");
    test.describe(`${part}-${modeId}`, () => {
      test.use({ viewport: mode.viewport, colorScheme: mode.colorScheme, hasTouch: mode.hasTouch, reducedMotion: "reduce" });
      test(`${MODE} ${part} ${modeId}`, async ({ page, proof }, info) => {
        const ready = await open(page, proof, "crowded");
        proof.measured = true;
        const before = state("crowded");
        if (part === "banner") await page.getByLabel("PM update", { exact: true }).fill(ready.raw);
        await page.locator("[data-block=pm-update]").scrollIntoViewIfNeeded();
        expect(state("crowded")).toEqual(before);
        record(info, "capture-state.json", { part, mode: modeId, sourceWorld: MODE, currentText: await page.getByLabel("PM update", { exact: true }).inputValue(), labels: ready.labels, frozenState: before, claimedSemanticResult: null });
        await page.screenshot({ path: info.outputPath(`${part}-${modeId}.png`) });
      });
    });
  }
}
