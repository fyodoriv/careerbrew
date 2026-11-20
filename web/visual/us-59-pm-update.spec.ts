import type { DayLogView } from "../src/api";
import { addDays, expect, open, patch, shot, test, TODAY } from "./helpers";

// US-59: Copy update next to the day's heading; when the browser refuses the clipboard, the text shows selected to copy.
test("US-59: Copy update when the browser refuses the clipboard", async ({ page }) => {
  await open(page, "today");
  await page.evaluate(() => {
    navigator.clipboard.writeText = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
  });
  await page.getByRole("button", { name: "Copy update", exact: true }).click();
  await expect(page.getByLabel("Update to copy")).toBeVisible();
  await shot(page.locator(".day-head"), "copy-by-hand");
});

test("US-59: a restored PM line clears its cut note before saving", async ({ page }) => {
  const kept = "billing: next: reconcile amber invoices.";
  const raw = `${kept}\nOther: reviewed 4 PRs.`;
  const cutDay = addDays(TODAY, -1);
  await patch<DayLogView>(page, "**/api/daylog", (data) => ({
    ...data,
    pm: {
      ...data.pm,
      body: kept,
      draft: raw,
      left_out: ["Other"],
      cut_context: {
        schema: 1,
        candidates: [{
          label: "Other",
          aliases: [{ spelling: "Other", norm: "other" }],
          recorded_at: `${cutDay}T14:30:00-04:00`,
          report_day: cutDay,
          date_basis: "saved",
        }],
      },
    },
  }));
  await open(page, "today");
  const block = page.locator("[data-block=pm-update]");
  await expect(block.locator("[data-pm-left-out]")).toContainText("Other");
  await shot(block, "cut-note");
  await page.getByLabel("PM update", { exact: true }).fill(raw);
  await expect(block.locator("[data-pm-left-out]")).toHaveCount(0);
  await expect(page.getByLabel("PM update", { exact: true })).toBeFocused();
  await shot(block, "cut-restored");
});
