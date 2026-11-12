import { expect, hold, open, settle, shot, test } from "./helpers";

// US-64: a view opened before shows its last data at once, marked "Updated hh:mm · refreshing…", while it reloads.
test("US-64: Today again, from the page's cache", async ({ page }) => {
  await open(page, "today");
  await page.evaluate(() => (window.location.hash = "#/week"));
  await expect(page.getByRole("heading", { level: 1, name: "Week" })).toBeVisible();
  await settle(page);
  for (const read of ["**/api/today", "**/api/feed/asks", "**/api/daylog", "**/api/inbox"]) await hold(page, read);
  await page.evaluate(() => (window.location.hash = "#/today"));
  await expect(page.locator(".updated").first()).toHaveText(/^Updated \d{2}:\d{2} · refreshing…$/);
  await shot(page, "today-from-cache");
});
