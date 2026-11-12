import { open, region, shot, test } from "./helpers";

// US-60: projects imported from a team's sheet, children nested under their parents, with links and stale counts.
test("US-60: the project tree", async ({ page }) => {
  await open(page, "work");
  await shot(region(page, "Projects"), "projects");
});
