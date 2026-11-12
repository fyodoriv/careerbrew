import { expect, open, settle, shot, splitDates, test } from "./helpers";

// US-36: the days ahead, each with its meetings, free time, and the tasks the planner placed, for 5, 7, or 14 days.
test("US-36: two weeks ahead", async ({ page }) => {
  await open(page, "week");
  await page.getByLabel("Days to show").selectOption("14");
  await expect(page.getByRole("article")).toHaveCount(14);
  await settle(page);
  expect(await splitDates(page.locator(".week")), "a due date stays on one line in a narrow day column").toEqual([]);
  await shot(page.locator("main .view"), "fourteen-days");
});
