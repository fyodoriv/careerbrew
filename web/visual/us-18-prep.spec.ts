import { open, region, shot, test } from "./helpers";

// US-18: the readiness score of my last prep check shows beside the company (Prep column).
test("US-18: a checked company shows its prep score", async ({ page }) => {
  await open(page, "interviews");
  await page.getByLabel("Any company").fill("Stripe");
  await shot(region(page, "Companies"), "stripe-prep-score");
});
