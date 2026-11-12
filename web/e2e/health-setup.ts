import { expect } from "@playwright/test";
import type { Page, Request } from "@playwright/test";

export function trackInitialDayLog(page: Page): { isReady: () => boolean; wait: () => Promise<void> } {
  const pending = new Set<Request>();
  let completed = 0;
  let failed = false;
  page.on("request", request => {
    if (request.method() === "GET" && new URL(request.url()).pathname === "/api/daylog") pending.add(request);
  });
  page.on("response", response => {
    if (pending.has(response.request()) && !response.ok()) failed = true;
  });
  page.on("requestfinished", request => {
    if (pending.delete(request)) completed++;
  });
  page.on("requestfailed", request => {
    if (pending.delete(request)) failed = true;
  });
  const isReady = () => completed > 0 && pending.size === 0 && !failed;
  return { isReady, wait: async () => {
    await expect.poll(isReady, { message: "initial Day log requests must finish before the baseline" }).toBe(true);
  } };
}

export async function waitForInitialDayLogs(trackers: { isReady: () => boolean }[]): Promise<void> {
  await expect.poll(() => trackers.every(tracker => tracker.isReady()), { message: "both pages' initial Day log requests must finish before the baseline" }).toBe(true);
}

export async function waitForTodayDayLogs(page: Page, trackers: { isReady: () => boolean }[]): Promise<void> {
  await expect(page.locator('[data-block="copy-update"]')).toHaveAttribute("data-daylog-fresh", "true");
  await expect(page.locator('[data-block="pm-update"]')).toHaveAttribute("data-daylog-fresh", "true");
  await waitForInitialDayLogs(trackers);
}
