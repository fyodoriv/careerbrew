import { expect, open, region, shot, test } from "./helpers";

// US-72: a button with nothing to act on yet is disabled, not silent: Add task with no title, Open loop with no company.
test("US-72: Add task waits for a title", async ({ page }) => {
  await open(page, "week");
  await expect(page.getByRole("button", { name: "Add task" })).toBeDisabled();
  await shot(region(page, "Plan ahead"), "add-task-disabled");
});

test("US-72: Open loop waits for a company", async ({ page }) => {
  await open(page, "interviews");
  await expect(page.getByRole("button", { name: "Open loop" })).toBeDisabled();
  await shot(page.getByRole("search"), "open-loop-disabled");
});
