import { open, patch, region, shot, test } from "./helpers";

// US-62: important new events, each with why it matters, the next step, its priority, and what I can do from here.
test("US-62: open notifications", async ({ page }) => {
  await open(page, "notifications");
  await shot(region(page, /^Open \(\d+\)$/), "open");
});

test("US-62: notifications turned off", async ({ page }) => {
  await patch<{ status: object }>(page, "**/api/notifications", (n) => ({ ...n, status: { ...n.status, enabled: false } }));
  await open(page, "notifications");
  await shot(page.getByRole("status").filter({ hasText: "Notifications are off" }), "turned-off");
});
