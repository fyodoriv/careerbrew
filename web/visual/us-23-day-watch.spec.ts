import { open, patch, region, shot, test } from "./helpers";

// US-23: the watcher I can pause or turn off, and the results-of-the-day list it fills.
test("US-23: the day watcher, on", async ({ page }) => {
  await patch<{ watcher: object }>(page, "**/api/overview", (o) => ({ ...o, watcher: { ...o.watcher, state: "on", running: true } }));
  await open(page, "setup");
  await shot(region(page, "Day watcher"), "watcher-on");
});

test("US-23: the results of the day", async ({ page }) => {
  await open(page, "today");
  await shot(region(page, "Results of the day"), "results-of-the-day");
});
