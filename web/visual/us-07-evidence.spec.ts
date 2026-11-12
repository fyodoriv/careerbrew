import { open, region, shot, test } from "./helpers";

// US-07: my work evidence in one place, newest first.
test("US-07: recent evidence", async ({ page }) => {
  await open(page, "work");
  await shot(region(page, "Recent evidence"), "recent-evidence");
});
