import { answer, expect, open, region, shot, test } from "./helpers";
import type { Route } from "@playwright/test";

// US-84: Not an ask takes an ask off the list, says what three of them would do, and offers Undo (issue #185).
test("US-84: an ask just marked not an ask", async ({ page }) => {
  let marked = "";
  await page.route("**/api/feed/asks", async (route: Route) => {
    const res = await route.fetch();
    const d = (await res.json()) as { asks: { id: string }[]; total: number };
    const asks = d.asks.filter((a) => a.id !== marked);
    await route.fulfill({ response: res, json: { ...d, asks, total: asks.length } });
  });
  await answer(page, "**/api/actions/work.feed.mark", { action: "work.feed.mark", argv: [], exit: 0, output: "not an ask" });
  await open(page, "today");
  const asks = region(page, /^Open asks/);
  const dm = asks.locator("li[data-ask]", { hasText: "DM from dana" });
  marked = (await dm.getAttribute("data-ask")) ?? "";
  await dm.getByRole("button", { name: /^Not an ask/ }).click();
  await expect(asks.getByRole("status")).toContainText("Marked not an ask");
  await shot(asks, "marked-not-an-ask");
});
