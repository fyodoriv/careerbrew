import { expect, open, region, shot, test } from "./helpers";

// US-67: one page per long run, newest first; the chosen page shows Done, Blockers, and Next in a sandboxed frame.
test("US-67: a run page in Reports", async ({ page }) => {
  await open(page, "reports");
  const list = region(page, /^Run pages \(\d+\)$/);
  await list.getByRole("link", { name: /^Plan for / }).click();
  await expect(page.frameLocator("iframe.report-frame").getByRole("heading", { level: 1 })).toHaveText(/^Plan for /);
  await shot(page, "plan-run-page");
});
