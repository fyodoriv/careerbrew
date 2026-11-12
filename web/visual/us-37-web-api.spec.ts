import { expect, open, patch, shot, test } from "./helpers";

// US-37: on a corporate-role machine the API refuses job-hunt views: the sidebar has no Job search group,
// and a direct link to one says why. (The seeded server is personal; the page plays a corporate machine.)
test("US-37: a job-hunt view on a corporate machine", async ({ page }) => {
  await patch(page, "**/api/overview", (o) => ({ ...o, role: "corporate" }));
  await page.route(/\/api\/(jobs|interviews|cards|library|explanations|exercises|drill)(\/|$|\?)/, (route) => route.fulfill({ status: 403, json: { error: "job-hunt views are personal-only; this is a corporate machine" } }));
  await open(page, "jobs");
  await expect(page.getByRole("status").filter({ hasText: "job-hunt views are personal-only" })).toBeVisible();
  await shot(page, "corporate-job-hunt");
});
