import { expect, test } from "@playwright/test";
import { NOW, TOKEN } from "../playwright.config";

for (const order of ["normal", "default-first", "expanded-first"] as const) {
  test(`open asks expansion survives refresh: ${order}`, async ({ page }) => {
    const all = Array.from({ length: 60 }, (_, i) => ({ id: `synthetic:${i}`, source: "acme", kind: "dm", at: `${NOW}:00Z`, title: `Synthetic ask ${i}`, url: "", waiting_minutes: 0, work_minutes: 0, overdue: false }));
    const limits: string[] = [];
    let holdDefault = false;
    let defaultSeen = () => {}, expandedSeen = () => {}, releaseDefault = () => {}, releaseExpanded = () => {};
    const defaultRequest = new Promise<void>((resolve) => { defaultSeen = resolve; });
    const expandedRequest = new Promise<void>((resolve) => { expandedSeen = resolve; });
    const defaultGate = new Promise<void>((resolve) => { releaseDefault = resolve; });
    const expandedGate = new Promise<void>((resolve) => { releaseExpanded = resolve; });
    let defaultDone = () => {};
    const defaultFinished = new Promise<void>((resolve) => { defaultDone = resolve; });
    await page.route((u) => u.pathname === "/api/feed/asks", async (route) => {
      const limit = new URL(route.request().url()).searchParams.get("limit");
      limits.push(limit ?? "default");
      const heldDefault = holdDefault && !limit;
      if (heldDefault) { holdDefault = false; defaultSeen(); await defaultGate; }
      if (limit === "60" && order !== "normal") { expandedSeen(); await expandedGate; }
      try {
        await route.fulfill({ contentType: "application/json", body: JSON.stringify({ asks: all.slice(0, Number(limit ?? 50)), total: 60, limit: 1000, events: 60, overdue_hours: 8 }) });
      } finally {
        if (heldDefault) defaultDone();
      }
    });
    try {
      const firstChange = page.waitForResponse((response) => new URL(response.url()).pathname === "/api/changes");
      await page.goto(`/#token=${TOKEN}`);
      await firstChange;
      await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
      const asks = page.getByRole("region", { name: /^Open asks/ });
      const button = asks.getByRole("button", { name: "Show all 60" });
      const pager = asks.getByRole("group", { name: "Pages of open asks" });
      await expect(button).toBeEnabled();
      if (order === "normal") {
        await button.click();
      } else {
        await button.scrollIntoViewIfNeeded();
        const box = await button.boundingBox();
        if (!box) throw new Error("Show all button has no bounding box");
        const x = box.x + box.width / 2, y = box.y + box.height / 2;
        expect(await button.evaluate((node, point) => document.elementFromPoint(point.x, point.y) === node, { x, y })).toBe(true);
        await page.mouse.move(x, y);
        await page.mouse.down();
        holdDefault = true;
        await page.evaluate(() => window.dispatchEvent(new Event("careerbrew:changed")));
        await defaultRequest;
        await expect(button).toBeEnabled();
        await page.mouse.up();
        await expandedRequest;
        if (order === "default-first") { releaseDefault(); await defaultFinished; releaseExpanded(); }
        else { releaseExpanded(); await expect(pager).toContainText("of 60"); releaseDefault(); await defaultFinished; }
      }
      await expect(pager).toContainText("of 60");
      expect(limits.filter((n) => n === "60")).toHaveLength(1);
    } finally {
      releaseDefault();
      releaseExpanded();
    }
  });
}
