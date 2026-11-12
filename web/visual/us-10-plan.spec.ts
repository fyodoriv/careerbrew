import { open, region, shot, test } from "./helpers";

// US-10: my tasks ranked by deadline, impact, size, and promotion gaps.
test("US-10: tasks by priority", async ({ page }) => {
  await open(page, "work");
  await shot(region(page, "Tasks by priority"), "tasks-by-priority");
});
