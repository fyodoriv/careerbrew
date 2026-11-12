import { open, region, shot, test } from "./helpers";

// US-38: the Notion jobs board lands in Job hunt, its stages tinted by outcome.
test("US-38: the jobs board imported from Notion", async ({ page }) => {
  await open(page, "jobs");
  await shot(region(page, /^Jobs board from Notion/), "notion-jobs-board");
});
