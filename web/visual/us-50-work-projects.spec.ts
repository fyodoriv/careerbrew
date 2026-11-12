import { open, region, shot, test } from "./helpers";

// US-50: project sources I have not read within their limit come first, with the command that marks them read.
test("US-50: stale project sources on the Dashboard", async ({ page }) => {
  await open(page, "dashboard");
  await shot(region(page, "Stale project sources"), "stale-sources");
});
