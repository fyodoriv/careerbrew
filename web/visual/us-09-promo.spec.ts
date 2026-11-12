import { open, region, shot, test } from "./helpers";

// US-09: how much of the next level's bar my evidence covers, and what is missing, highest priority first.
test("US-09: the promotion case and what is missing", async ({ page }) => {
  await open(page, "work");
  await shot(region(page, /^Promotion case: /), "promotion-case");
  await shot(region(page, "What is missing (highest priority first)"), "what-is-missing");
});
