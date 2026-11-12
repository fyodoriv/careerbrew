import type { Standup } from "../src/api";
import { open, patch, shot, test } from "./helpers";

// US-65: for each item I report on, a talking point, its status and owner, then the evidence since the last standup.
test("US-65: the standup report", async ({ page }) => {
  await open(page, "standup");
  await shot(page.locator("main .view"), "standup");
});


test("US-65: three speaking bullets below the compact header", async ({ page }, info) => {
  const points = ["Shipped the API (2 PRs)", "Next: the CSV format", "Ask: a review from the data team"];
  await patch<Standup>(page, "**/api/standup", (d) => ({ ...d, rows: [{ ...d.rows[0], talking_point: points.join(". "), talking_points: points, agent_written: true }], groups: [] }));
  await open(page, "standup");
  await page.locator("main").screenshot({ path: info.outputPath("standup-speaking.png") });
  await page.locator("main .page-head").screenshot({ path: info.outputPath("standup-header.png") });
  await page.getByRole("region", { name: "Order export" }).screenshot({ path: info.outputPath("standup-row.png") });
  await shot(page.locator("main"), "speaking-bullets");
});
