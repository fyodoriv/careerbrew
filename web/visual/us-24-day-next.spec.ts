import { open, region, shot, test } from "./helpers";

// US-24: the next step for this free slot, and why.
test("US-24: the next step", async ({ page }) => {
  await open(page, "today");
  await shot(region(page, "Next step"), "next-step");
});
