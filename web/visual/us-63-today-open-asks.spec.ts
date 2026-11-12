import { answer, expect, open, region, shot, test } from "./helpers";
import type { Route } from "@playwright/test";

// US-63: Mark personal takes an ask off the list (and out of evidence, the day log, and the PM update), with Undo.
test("US-63: an ask just marked personal", async ({ page }) => {
  let marked = "";
  await page.route("**/api/feed/asks", async (route: Route) => {
    const res = await route.fetch();
    const d = (await res.json()) as { asks: { id: string }[]; total: number };
    const asks = d.asks.filter((a) => a.id !== marked);
    await route.fulfill({ response: res, json: { ...d, asks, total: asks.length } });
  });
  await answer(page, "**/api/actions/work.feed.mark", { action: "work.feed.mark", argv: [], exit: 0, output: "marked personal" });
  await open(page, "today");
  const asks = region(page, /^Open asks/);
  const dm = asks.locator("li[data-ask]", { hasText: "DM from dana" });
  marked = (await dm.getAttribute("data-ask")) ?? "";
  await dm.getByRole("button", { name: /^Mark personal/ }).click();
  await expect(asks.getByRole("button", { name: "Undo" })).toBeVisible();
  await expect(asks.locator("li[data-ask]")).toHaveCount(3);
  await shot(asks, "marked-personal");
});
