import { open, region, shot, test } from "./helpers";

// US-04: the season plan in phases, the current one marked, with readiness and the skill gaps to close first.
test("US-04: the season strategy", async ({ page }) => {
  await open(page, "jobs");
  await shot(region(page, "Season strategy"), "season-strategy");
});
