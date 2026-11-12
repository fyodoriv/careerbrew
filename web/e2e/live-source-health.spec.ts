import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { TOKEN } from "../playwright.config";
import { trackInitialDayLog, waitForTodayDayLogs } from "./health-setup";

test("US-61 US-65 US-97 both views expire without data writes and refresh after outside direct intake", async ({ browser, request }) => {
  test.setTimeout(150000);
  const port = process.env.CAREERBREW_LIVE_HEALTH_PORT;
  const root = process.env.CAREERBREW_LIVE_HEALTH_ROOT;
  if (!port || !/^[0-9]+$/.test(port) || !root) throw new Error("Owned source-health fixture did not announce its bindings");
  const origin = `http://127.0.0.1:${port}`;
  const headers = { "X-Careerbrew-Token": TOKEN };
  const complete = async (file: string) => {
    const add = await request.post(`${origin}/api/actions/work.feed.add`, { headers, data: { values: { file: `${root}/${file}.jsonl`, source: "pagerduty" } } });
    expect(add.ok()).toBeTruthy();
    expect((await add.json()).exit).toBe(0);
    const cycle = await request.post(`${origin}/api/actions/work.feed.cycle`, { headers, data: { values: {} } });
    expect(cycle.ok()).toBeTruthy();
    expect((await cycle.json()).exit).toBe(0);
  };
  await complete("initial");
  const context = await browser.newContext({ reducedMotion: "reduce", viewport: { width: 1280, height: 900 } });
  try {
    const today = await context.newPage(), standup = await context.newPage();
    const initialDayLogs = [trackInitialDayLog(today), trackInitialDayLog(standup)];
    for (const [page, view] of [[today, "today"], [standup, "standup"]] as const) {
      await page.goto(`${origin}/#token=${TOKEN}`);
      await expect(page.locator('[data-block="copy-update"]')).toHaveAttribute("data-daylog-fresh", "true");
      await page.goto(`${origin}/#/${view}`);
      await expect(page.getByRole("region", { name: "Work source health" })).toContainText("Work sources are current");
      await expect(page.getByRole("region", { name: "Work source health" })).toContainText("review is due");
      await expect(page.getByRole("region", { name: "Work source health" })).toContainText("Adapters stopped");
    }
    await waitForTodayDayLogs(today, initialDayLogs);
    const before = await request.get(`${origin}/api/changes`, { headers }).then((r) => r.json());
    await expect(today.getByRole("region", { name: "Work source health" })).toContainText("need a fresh pull", { timeout: 85000 });
    await expect(standup.getByRole("region", { name: "Work source health" })).toContainText("need a fresh pull", { timeout: 65000 });
    expect(await request.get(`${origin}/api/changes`, { headers }).then((r) => r.json())).toEqual(before);
    await complete("update");
    expect(await request.get(`${origin}/api/changes`, { headers }).then((r) => r.json())).not.toEqual(before);
    await expect(today.getByRole("region", { name: "Work source health" })).toContainText("Work sources are current");
    await expect(standup.getByRole("region", { name: "Work source health" })).toContainText("Work sources are current");
    await expect(today.getByText("Resolved second synthetic incident", { exact: true })).toBeVisible();
    await expect(standup.getByText("Resolved second synthetic incident", { exact: true })).toBeVisible();
    await expect(standup.getByRole("region", { name: "Work source health" })).toContainText("review is due");
  } finally {
    await context.close();
  }
});

for (const width of [390, 1280]) for (const colorScheme of ["light", "dark"] as const) {
  test(`US-61 US-65 source notice comfort ${width} ${colorScheme}`, async ({ page }) => {
    await page.setViewportSize({ width, height: 900 });
    await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
    await page.goto(`/#token=${TOKEN}`);
    for (const view of ["today", "standup"]) {
      await page.goto(`/#/${view}`);
      const notice = page.getByRole("region", { name: "Work source health" });
      await expect(notice).toContainText("Recent work may be missing from Today and Standup.");
      await expect(notice).toContainText("Set up my Careerbrew work feed.");
      await expect(notice).not.toContainText("<file.yaml>");
      const violations = await new AxeBuilder({ page }).include('[aria-label="Work source health"]').withTags(["wcag2a", "wcag2aa", "wcag21aa"]).analyze();
      expect(violations.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
      expect(await notice.evaluate((el) => [...el.querySelectorAll("*")].filter((node) => node.textContent?.trim() && parseFloat(getComputedStyle(node).fontSize) < 14).length)).toBe(0);
      expect(await page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1)).toBe(false);
      expect(await notice.evaluate((el) => [...el.querySelectorAll("a,button,input,select")].filter((node) => { const box = node.getBoundingClientRect(); return box.width < 44 || box.height < 44; }).length)).toBe(0);
    }
  });
}
