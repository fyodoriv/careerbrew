import { expect, open, shot, test } from "./helpers";

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
