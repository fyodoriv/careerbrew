import { open, region, shot, test } from "./helpers";

// US-33: what the researched loops share: the typical order, how often each step appears, and AI-policy counts.
test("US-33: the patterns all loops share", async ({ page }) => {
  await open(page, "interviews");
  await shot(region(page, /^Shared patterns across \d+ companies$/), "shared-patterns");
});
