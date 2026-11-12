import { TODAY, open, patch, shot, test } from "./helpers";

// US-31: when `careerbrew changes` finds that a company's process changed, its loop says so before I prepare.
test("US-31: a loop whose process changed says so", async ({ page }) => {
  await patch(page, "**/api/interviews/Datadog", (d) => ({ ...d, stale: { reason: "its careers page changed", at: TODAY } }));
  await open(page, "interviews/Datadog");
  await shot(page.locator(".callout-error", { hasText: "Process changed" }), "process-changed");
});
