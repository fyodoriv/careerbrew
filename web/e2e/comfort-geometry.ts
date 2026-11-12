import { expect, test as base } from "@playwright/test";
import type { Page, Request, TestInfo } from "@playwright/test";
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, readdirSync, statSync, writeFileSync } from "node:fs";
import { join, relative } from "node:path";
import { isHealthRead } from "./comfort-health-boundary";

function required(key: string): string {
  const value = process.env[key];
  if (!value) throw new Error("BLOCKED: owned373 invocation required");
  return value;
}
const RUN = required("ISSUE373_RUN");
const TOKEN = "issue373-synthetic-not-a-secret";
export const MODES = [
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

export function record(info: TestInfo, name: string, value: unknown): void {
  mkdirSync(info.outputDir, { recursive: true });
  writeFileSync(info.outputPath(name), JSON.stringify(value, null, 2) + "\n", { flag: "wx" });
}

export const test = base.extend<{ proof: Proof }>({
  contextOptions: async ({ contextOptions }, runContext, info) => {
    const mode = MODES.find((m) => info.title.endsWith(`(${m.id})`));
    if (!mode) throw new Error("BLOCKED: fixed geometry mode required");
    await runContext({ ...contextOptions, viewport: mode.viewport, colorScheme: mode.colorScheme, hasTouch: mode.hasTouch, reducedMotion: "reduce" });
  },
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
      const healthRead = proof.ready !== null && isHealthRead(request.method(), url, `http://127.0.0.1:${proof.ready.port}`);
      const apiRead = request.method() === "GET" && (readPaths.has(url.pathname) || (url.pathname === "/api/tab-key" && url.search === "") || healthRead);
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

export async function open(page: Page, proof: Proof, scenario: string): Promise<Ready> {
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

export function state(scenario: string): Record<string, unknown> {
  return bytes(join(RUN, scenario, "world"));
}


// Keep the original whole-main geometry predicates, including disabled and offscreen controls.
export async function geometry(page: Page) {
  return page.evaluate(() => {
    const small: string[] = [], targets: string[] = [], spills: string[] = [];
    const controls: { path: string; text: string; width: number; height: number; disabled: boolean }[] = [];
    const path = (e: Element): string => {
      const parts: string[] = [];
      for (let a: Element | null = e; a; a = a.parentElement) {
        const peers = a.parentElement ? [...a.parentElement.children].filter((p) => p.tagName === a!.tagName) : [a];
        parts.unshift(`${a.tagName.toLowerCase()}:nth-of-type(${peers.indexOf(a) + 1})`);
      }
      return parts.join(" > ");
    };
    for (const e of document.querySelectorAll<HTMLElement>("main *")) {
      if (!e.getClientRects().length || e.closest(".sr-only,[hidden],[aria-hidden=true]")) continue;
      const css = getComputedStyle(e), r = e.getBoundingClientRect();
      const ownText = [...e.childNodes].some((n) => n.nodeType === Node.TEXT_NODE && n.textContent?.trim());
      if ((ownText || e.matches("textarea,input,select")) && parseFloat(css.fontSize) < 14) small.push(path(e));
      if (e.matches("button,a[href],textarea,input:not([type=hidden]),select,summary,[role=button]")) {
        const identity = path(e);
        controls.push({ path: identity, text: e.getAttribute("aria-label") ?? e.textContent?.trim() ?? "", width: r.width, height: r.height, disabled: e.matches(":disabled") });
        if (r.width < 44 || r.height < 44) targets.push(identity);
      }
      if (!e.matches("textarea,code,input") && css.display !== "inline" && e.scrollWidth > e.clientWidth + 1 && css.textOverflow !== "ellipsis") spills.push(path(e));
      if (ownText && e.closest("[data-block=pm-update]") && !e.closest("textarea,code") && ((css.overflowX !== "visible" && e.scrollWidth > e.clientWidth + 1) || (css.overflowY !== "visible" && e.scrollHeight > e.clientHeight + 1))) spills.push("clipped-PM-text");
    }
    const tags = [...document.querySelectorAll<HTMLElement>(".agenda .block-title .tag")].map((e) => {
      const r = e.getBoundingClientRect(), parent = e.parentElement!.getBoundingClientRect(), css = getComputedStyle(e);
      const range = document.createRange(); range.selectNodeContents(e);
      return { text: e.textContent, innerText: e.innerText, rect: r.toJSON(), parent: parent.toJSON(), fontSize: css.fontSize, overflowX: css.overflowX, overflowY: css.overflowY, textOverflow: css.textOverflow, whiteSpace: css.whiteSpace, lines: [...range.getClientRects()].map((v) => v.toJSON()) };
    });
    return { small, targets, spills, controls, tags, sideways: document.documentElement.scrollWidth > innerWidth + 1, reducedMotion: matchMedia("(prefers-reduced-motion: reduce)").matches };
  });
}

export async function preserveFold(page: Page) {
  const summary = page.locator("main summary").first();
  const details = summary.locator("..");
  await expect(details).not.toHaveAttribute("open");
  await summary.click();
  await expect(details).toHaveAttribute("open", "");
  await summary.press("Space");
  await expect(details).not.toHaveAttribute("open");
}
