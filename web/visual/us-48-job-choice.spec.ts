import { open, patch, shot, test } from "./helpers";

// US-48: careerbrew suggests; a job enters a tier only when I choose it, and I can remove it again.
test("US-48: a chosen dream job sits in its tier", async ({ page }) => {
  await patch<{ strategy: { suggested: Record<string, object[]>; tiers: Record<string, object[]> } }>(page, "**/api/jobs", (j) => ({
    ...j, strategy: { ...j.strategy, tiers: { ...j.strategy.tiers, dream: [{ ...j.strategy.suggested.dream[0], chosen: true }] } },
  }));
  await open(page, "jobs");
  await shot(page.locator("main .columns").first(), "tiers-with-a-chosen-job");
});
