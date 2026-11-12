import { CHECKOUT, expect, machinePaths, open, patch, region, shot, test } from "./helpers";

// US-12: an installed company plugin shows in Setup with its version and what it provides.
test("US-12: an installed plugin", async ({ page }) => {
  await patch(page, "**/api/setup", (s) => ({ ...s, plugins: [{ name: "acme", version: "1.4.0", extends: null, exec: false, provides: ["ladder", "terms", "policy", "connectors"] }] }));
  await open(page, "setup");
  await shot(region(page, "Plugins"), "plugins");
});

// The guard that keeps the Plugins picture the same in every checkout: Settings above it showed the seed's
// stt_command, whose path is the checkout's (2026-10-22). Unpinned again, the page names that path before any picture.
test("US-12: a page that shows the checkout's path takes no picture", async ({ page }) => {
  const stt = `${CHECKOUT}/tests/fixtures/voice/fake-stt.sh {audio}`;
  await patch<{ settings: { key: string; value: unknown }[] }>(page, "**/api/config", (c) => ({ ...c, settings: c.settings.map((s) => (s.key === "stt_command" ? { ...s, value: stt } : s)) }));
  await open(page, "setup");
  await page.locator("summary", { hasText: "Voice practice" }).click();  // settings fold by topic (US-76)
  expect(await machinePaths(page)).toEqual([stt]);
});
