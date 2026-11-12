import { expect, test } from "@playwright/test";
import { trackInitialDayLog, waitForInitialDayLogs, waitForTodayDayLogs } from "./health-setup";
import { TOKEN } from "../playwright.config";

test("source-health baseline waits for the initial Day log response", async ({ page }) => {
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let requested = false;
  await page.route("**/api/daylog", async route => {
    requested = true;
    await held;
    await route.fulfill({ status: 200, contentType: "application/json", body: "{}" });
  });
  const tracker = trackInitialDayLog(page);
  await page.goto("/");
  await page.evaluate(() => { void fetch("/api/daylog"); });
  await expect.poll(() => requested).toBe(true);
  let baselineReady = false;
  const ready = tracker.wait().then(() => { baselineReady = true; });
  try {
    await page.waitForTimeout(100);
    expect(baselineReady).toBe(false);
  } finally {
    release();
    await ready;
  }
  expect(baselineReady).toBe(true);
});

test("source-health baseline rechecks both pages while either is pending", async () => {
  let releaseA!: () => void, releaseB!: () => void;
  const firstA = new Promise<void>(resolve => { releaseA = resolve; });
  const firstB = new Promise<void>(resolve => { releaseB = resolve; });
  let readyA = false, readyB = false;
  let observedBusy = false;
  const a = { wait: () => firstA, isReady: () => { if (!readyA && readyB) observedBusy = true; return readyA; } };
  const b = { wait: () => firstB, isReady: () => readyB };
  let baselineReady = false;
  const ready = waitForInitialDayLogs([a, b]).then(() => { baselineReady = true; });
  readyA = true;
  releaseA();
  await firstA;
  await Promise.resolve();
  readyA = false; // A new initial request starts while B is still pending.
  readyB = true;
  releaseB();
  await firstB;
  await expect.poll(() => observedBusy).toBe(true);
  expect(baselineReady).toBe(false);
  readyA = true;
  await ready;
  expect(baselineReady).toBe(true);
});

test("cached Today data stays unready while its own fresh request is held", async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`);
  const copy = page.locator('[data-block="copy-update"]');
  await expect(copy).toHaveAttribute("data-daylog-fresh", "true");
  let release!: () => void;
  const held = new Promise<void>(resolve => { release = resolve; });
  let requests = 0;
  await page.route("**/api/daylog", async route => {
    requests++;
    if (route.request().headers()["x-careerbrew-test-probe"] !== "1") await held;
    await route.continue();
  });
  const tracker = trackInitialDayLog(page);
  let baselineReady = false;
  let ready: Promise<void> | undefined;
  try {
    await page.goto("/#/today");
    await expect.poll(() => requests).toBeGreaterThanOrEqual(2);
    ready = waitForTodayDayLogs(page, [tracker]).then(() => { baselineReady = true; });
    await page.evaluate(async token => { const response = await fetch("/api/daylog", { headers: { "X-Careerbrew-Token": token, "X-Careerbrew-Test-Probe": "1" } }); await response.json(); }, TOKEN);
    await expect.poll(() => requests).toBeGreaterThanOrEqual(3);
    const markers = page.locator('[data-block="copy-update"], [data-block="pm-update"]');
    await expect(markers).toHaveCount(2);
    expect(await markers.evaluateAll(nodes => nodes.some(node => node.getAttribute("data-daylog-fresh") !== "true"))).toBe(true);
    expect(baselineReady).toBe(false);
  } finally {
    release();
  }
  await ready;
  expect(baselineReady).toBe(true);
});
