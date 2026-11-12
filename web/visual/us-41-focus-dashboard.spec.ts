import { open, region, shot, test } from "./helpers";

// US-41: the best action for every horizon, each with the numbers behind its rank, and tomorrow's proposed plan.
test("US-41: the seven horizons and tomorrow", async ({ page }) => {
  await open(page, "dashboard");
  await shot(page.locator("main .grid").first(), "horizons");
  await shot(region(page, "Tomorrow"), "tomorrow");
});
