import { open, region, shot, test } from "./helpers";

// US-86: the minutes careerbrew saved me this week and the weeks before, and how many runs used my own time.
test("US-86: the Time saved block on Dashboard", async ({ page }) => {
  await open(page, "dashboard");
  await shot(region(page, "Time saved"), "time-saved");
});
