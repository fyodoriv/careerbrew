import { expect, open, region, shot, test } from "./helpers";

// US-52: the day's log: done today and the plan for the next workday (each row opens its details),
// careerbrew's suggestions with what, why, and what each button changes, and its questions.
test("US-52: done today and tomorrow's plan, a row opened", async ({ page }) => {
  await open(page, "today");
  const log = page.locator("div.split").filter({ has: region(page, /^Done today/) });
  await log.getByText("feat: shipped order export").click();
  await log.getByText(/Migrate billing jobs/).click();  // the next workday's task: today's plan holds the two short ones
  await expect(log.getByText("History").first()).toBeVisible();
  await shot(log, "done-and-plan");
});

test("US-52: suggestions and questions", async ({ page }) => {
  await open(page, "today");
  await shot(region(page, /^careerbrew suggests/), "suggestions");
  await shot(region(page, /^Questions for you/), "questions");
});
