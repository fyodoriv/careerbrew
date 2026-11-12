import { open, region, shot, test } from "./helpers";

// US-11: every outcome metric labeled KEEP, WATCH, PIVOT, or no data (never zero), with the pivot action.
test("US-11: the outcome metrics", async ({ page }) => {
  await open(page, "review");
  await shot(region(page, "Outcome metrics"), "outcome-metrics");
});
