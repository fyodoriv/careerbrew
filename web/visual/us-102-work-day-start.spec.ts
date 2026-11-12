import { expect, open, region, shot, test } from "./helpers";

test("US-102: GitHub repair form asks for owner confirmation", async ({ page }) => {
  await open(page, "actions/day.start");
  await page.getByLabel("Filter actions").fill("day.start");
  const form = page.getByRole("form", { name: "Run day.start" });
  await expect(form.getByRole("checkbox", { name: "fix", exact: true })).toBeVisible();
  await expect(form.getByRole("checkbox", { name: "yes", exact: true })).toHaveCount(0);
  await expect(form.getByRole("button", { name: "Run", exact: true })).toBeDisabled();
  await shot(region(page, "day.start"), "github-repair-form");
});
