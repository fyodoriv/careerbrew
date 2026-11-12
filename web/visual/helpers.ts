import { homedir } from "node:os";
import { fileURLToPath } from "node:url";
import { expect, test as base } from "@playwright/test";
import type { Locator, Page, Route } from "@playwright/test";
import { TOKEN, TRACE_IN_SANDBOX, VISUAL_INSTANT, VISUAL_NOW } from "../playwright.visual.config";

// Shared steps for the visual tests (US-75). Each spec is one user story: it opens the story's key screens on the
// seeded server and compares each with its approved picture, at desktop and at phone width (the two projects).
// A state the seed does not have is a GET answer patched in the page, never a write to the server, so the tests
// run side by side and in any order (the same rule as web/e2e/controls.spec.ts).

export { expect };
export const TODAY = VISUAL_NOW.slice(0, 10);
export function addDays(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}

// The pictures pin the system font every Mac has, so a font installed on one Mac never moves a picture.
const FONTS = ':root { font-family: -apple-system, BlinkMacSystemFont, "Helvetica Neue", sans-serif !important; }';

// The Time saved block's answer in every picture: this week and the week before, from the pinned day (US-86).
const MONDAY = addDays(TODAY, -((new Date(`${TODAY}T00:00:00Z`).getUTCDay() + 6) % 7));
const cell = (runs: number, web: number, spent: number, saved: number) => ({ runs, runs_web: web, runs_cli: runs - web, opened: web, minutes_spent: spent, minutes_saved: saved, time: web === runs ? "web" : web ? "web+cli" : "cli" });
export const TIME_SAVED = {
  computed: TODAY, this_week: "this-week", measured_baselines: 1, minutes_saved_total: 81.5, runs_total: 9,
  baselines: Object.fromEntries(["pm_update", "standup", "day_log", "day_plan", "evidence", "promo_doc", "job_scan", "inbox"].map((a, i) => [a, { minutes: 10, kind: i ? "estimate, careerbrew default, no published source" : "measured" }])),
  weeks: [
    { week: "last-week", start: addDays(MONDAY, -7), runs: 4, minutes_saved: 33.5, actions: { pm_update: cell(2, 1, 6.5, 23.5), day_plan: cell(2, 0, 0, 20) } },
    { week: "this-week", start: MONDAY, runs: 5, minutes_saved: 48, actions: { pm_update: cell(2, 2, 7, 23), standup: cell(1, 1, 2, 8), inbox: cell(2, 2, 1, 5), day_plan: cell(0, 0, 0, 0) } },
  ],
};

export const test = base.extend<{ visual: void }>({
  visual: [async ({ page }, use) => {
    const errors: string[] = [];
    page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
    page.on("pageerror", (e) => errors.push(e.message));
    // The browser's clock stands still at the server's pinned time, so "Updated hh:mm" and timers read the same.
    await page.clock.setFixedTime(new Date(VISUAL_INSTANT));
    await page.addInitScript((css) => {
      document.addEventListener("DOMContentLoaded", () => {
        const style = document.createElement("style");
        style.textContent = css;
        document.head.append(style);
      });
    }, FONTS);
    // The live poll answers one version, so no refetch starts while a picture is taken.
    await page.route("**/api/changes", (route) => route.fulfill({ json: { version: "visual" } }));
    // Four values differ on every run or machine, whatever the data, so the pictures read them at the pinned time:
    // watcher_cpu_ms is the CPU time this machine spent on the seed's watcher cycle (a measurement, not data);
    // the PM update's "updated hh:mm" is the day log file's write time, the wall clock when the seed ran;
    // the standup page's path lies in this run's temporary data folder;
    // the stt_command setting holds the checkout's path to the fake speech-to-text script (Settings, on Setup).
    await patch<{ metrics: { metric: string }[] }>(page, "**/api/review", (r) => ({ ...r, metrics: r.metrics.map((m) => (m.metric === "watcher_cpu_ms" ? { ...m, value: 120, status: "KEEP" } : m)) }));
    await patch<{ pm: { updated_at?: string } }>(page, "**/api/daylog", (d) => ({ ...d, pm: { ...d.pm, updated_at: d.pm.updated_at ? VISUAL_NOW : d.pm.updated_at } }));
    await patch(page, "**/api/standup", (s) => ({ ...s, page: "private/out/standup.html" }));
    await patch<{ settings: { key: string; value: unknown }[] }>(page, "**/api/config", (c) => ({ ...c, settings: c.settings.map((s) => (s.key === "stt_command" ? { ...s, value: "tests/fixtures/voice/fake-stt.sh {audio}" } : s)) }));
    // Time saved counts real run times of the seed's CLI runs, so the pictures show one synthetic answer; a timing
    // post (a measurement, not data) is answered here, so the visual tests never write to the server.
    await page.route("**/api/time-saved", (route) => route.fulfill({ json: TIME_SAVED }));
    await page.route("**/api/timing", (route) => route.fulfill({ json: { logged: true } }));
    await use();
    await page.unrouteAll({ behavior: "ignoreErrors" });  // a held read (hold below) never answers
    expect(errors, "browser console errors").toEqual([]);
  }, { auto: true }],
});

// Opens a view (`today`, `interviews/Datadog`) in a fresh page load, signed in, and waits until it settles.
export async function open(page: Page, hash: string): Promise<void> {
  await page.goto(`/#/${hash}&token=${TOKEN}`);
  await expect(page.locator("main h1")).toBeVisible();
  await settle(page);
}

// Waits until every block has its data: no loading label, no cached answer still refreshing, fonts ready.
export async function settle(page: Page): Promise<void> {
  await expect(page.locator(".skeleton")).toHaveCount(0);
  await expect(page.getByText(/^Loading/)).toHaveCount(0);
  await expect(page.locator(".updated").filter({ hasText: "refreshing" })).toHaveCount(0);
  await expect(page.locator(".working")).toHaveCount(0);
  const health = page.getByRole("region", { name: "Work source health" });
  if (await health.count()) await expect(health).toContainText("Work sources are not set up.");
  await page.evaluate(() => document.fonts.ready.then(() => undefined));
}

export const isPhone = (page: Page): boolean => (page.viewportSize()?.width ?? 1280) < 760;
export const region = (page: Page, name: string | RegExp): Locator => page.getByRole("region", { name, exact: typeof name === "string" }).first();

// One approved picture: the screen as it opens (the page) or one block (a locator), at this project's width.
export async function shot(target: Page | Locator, name: string, opts: { mask?: Locator[] } = {}): Promise<void> {
  const page = "goto" in target ? target : target.page();
  const mask = opts.mask ?? [];  // a mask only for a part that changes on every run and that no patch above pins
  expect(await machinePaths(page), "text that differs per checkout or machine (pin it with patch() in the visual fixture)").toEqual([]);
  await page.mouse.move(0, 0);  // no hover state on the control a click left the pointer over
  if ("goto" in target) {
    await expect(page).toHaveScreenshot(`${name}.png`, { mask });
  } else {
    await target.scrollIntoViewIfNeeded();
    await expect(target).toHaveScreenshot(`${name}.png`, { mask });
  }
}

// The checkout folder and the seed's temporary data folder (web-seed-and-serve.sh) differ on every machine and
// worktree. A line that holds one wraps differently there, and the line height (20.25 px) is not a whole pixel, so
// every block below it moves by part of a pixel and its picture comes out 1 px taller or shorter (2026-10-22:
// stt_command's path in Settings moved Plugins). So no screen with a picture may show either, even off the picture.
export const CHECKOUT = fileURLToPath(new URL("../..", import.meta.url)).replace(/\/$/, "");
const MACHINE_PATHS = [CHECKOUT, CHECKOUT.replace(homedir(), "~"), /careerbrew-web\.\w{6}/];
export async function machinePaths(page: Page): Promise<string[]> {
  const lines = (await page.evaluate(() => document.body.innerText)).split("\n");
  return lines.filter((line) => MACHINE_PATHS.some((p) => (typeof p === "string" ? line.includes(p) : p.test(line)))).map((line) => line.trim());
}

// A teacher step the seed does not reach (the shape of teacher.next_step), started 5 minutes before the pinned time.
export async function teachStep(page: Page, step: Record<string, unknown>): Promise<void> {
  const started = new Date(new Date(VISUAL_INSTANT).getTime() - 5 * 60_000).toISOString().replace(/\.\d{3}Z$/, "+00:00");
  await patch(page, "**/api/teach", (t) => ({ ...t, step: { ...step, started } }));
}

// Dates (2026-10-21) in `scope` that the browser broke across two lines at a hyphen (US-75 visual review).
export async function splitDates(scope: Locator): Promise<string[]> {
  return scope.evaluateAll((els) => els.flatMap((el) => {
    const out: string[] = [];
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let node = walker.nextNode(); node; node = walker.nextNode()) {
      for (const m of (node.textContent ?? "").matchAll(/\d{4}-\d{2}-\d{2}/g)) {
        const range = document.createRange();
        range.setStart(node, m.index ?? 0);
        range.setEnd(node, (m.index ?? 0) + m[0].length);
        if (range.getClientRects().length > 1) out.push(m[0]);
      }
    }
    return out;
  }));
}

// Parts of `scope` that stick out past its right edge, where the card cuts them off (US-75 visual review).
export async function overflowing(scope: Locator): Promise<string[]> {
  return scope.evaluate((root) => {
    const edge = root.getBoundingClientRect().right + 1;
    const name = (el: Element) => `${el.tagName.toLowerCase()}${typeof el.className === "string" && el.className ? `.${el.className.trim().split(/\s+/).join(".")}` : ""}`;
    return [...root.querySelectorAll("*")].filter((el) => el.getClientRects().length > 0 && el.getBoundingClientRect().right > edge).map(name);
  });
}

// Holds every GET that matches until the test ends, so its block shows its loading label.
export async function hold(page: Page, url: string | RegExp): Promise<void> {
  await page.route(url, (route) => (route.request().method() === "GET" ? new Promise<void>(() => undefined) : route.fallback()));
}

// Patch a GET answer in this page only: the server never sees a change. Patches of one URL all apply, in the order
// they were made (route.fetch() skips the other route handlers, so one handler per URL runs them all).
// Lifecycle: page scope; the WeakMap entry goes with the page.
const patches = new WeakMap<Page, Map<string, ((body: unknown) => unknown)[]>>();
export async function patch<T = Record<string, unknown>>(page: Page, url: string | RegExp, fn: (body: T) => unknown): Promise<void> {
  const byUrl = patches.get(page) ?? new Map<string, ((body: unknown) => unknown)[]>();
  patches.set(page, byUrl);
  const key = String(url);
  const fns = byUrl.get(key);
  if (fns) return void fns.push(fn as (body: unknown) => unknown);
  const list = [fn as (body: unknown) => unknown];
  byUrl.set(key, list);
  await page.route(url, async (route: Route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    await route.fulfill({ response: res, json: list.reduce((body, f) => f(body), (await res.json()) as unknown) });
  });
}

// Answer a POST in this page only, with the body the server would send.
export async function answer(page: Page, url: string | RegExp, body: unknown, status = 200): Promise<void> {
  await page.route(url, (route) => (route.request().method() === "POST" ? route.fulfill({ status, json: body }) : route.fallback()));
}
