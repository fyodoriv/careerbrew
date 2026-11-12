import { open, region, shot, test } from "./helpers";

// US-61: every work tool's open asks in one list, oldest first, with the wait counted in work hours.
test("US-61: open asks from the work feed", async ({ page }) => {
  await open(page, "today");
  await shot(region(page, /^Open asks/), "open-asks");
});


test("US-61: source health is separate from saved work", async ({ page }) => {
  await open(page, "today");
  await shot(page.getByRole("region", { name: "Work source health" }), "source-health");
});
