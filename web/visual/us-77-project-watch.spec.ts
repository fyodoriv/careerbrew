import { open, region, shot, test } from "./helpers";

// US-77: one section per active project, with its sources (stale first), last check, and latest change.
test("US-77: watched projects", async ({ page }) => {
  await open(page, "work");
  await shot(region(page, "Watch list"), "watched");
});
