import { open, region, shot, test } from "./helpers";

// US-03: a proposed Want score for each job; a job I rated takes my rating (5 of 5 is Want 100), and says so.
test("US-03: suggestions ranked by Want, with your own rating", async ({ page }) => {
  await open(page, "jobs");
  await shot(region(page, "Suggestions: you choose"), "suggestions");
});
