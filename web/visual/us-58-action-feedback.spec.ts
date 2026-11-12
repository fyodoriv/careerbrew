import { expect, hold, open, region, shot, test } from "./helpers";
import { TOKEN } from "../playwright.visual.config";

// US-58: every block says what it loads at once; a block that fails shows its error, the fix, and Retry while the
// others stay; a failed action shows the server's message and the fix, and keeps what I typed.
test("US-58: every block of Today says what it loads", async ({ page }) => {
  for (const read of ["**/api/today", "**/api/feed/asks", "**/api/daylog", "**/api/inbox"]) await hold(page, read);
  await page.goto(`/#/today&token=${TOKEN}`);
  await expect(page.getByText("Loading the next step…")).toBeVisible();
  await expect(page.getByText("Loading open asks…")).toBeVisible();
  await shot(page, "today-loading");
});

test("US-58: one Dashboard block fails alone", async ({ page }) => {
  await page.route("**/api/focus/today", (route) => route.fulfill({ status: 500, json: { error: "synthetic failure: the plan could not be built" } }));
  await open(page, "dashboard");
  await expect(region(page, "Today").getByRole("button", { name: "Retry" })).toBeVisible();
  await expect(region(page, "Right now")).toContainText("Why:");  // the other blocks stay
  await shot(region(page, "Today"), "dashboard-one-block-failed");
});

test("US-58: a failed action names the fix", async ({ page }) => {
  await page.route("**/api/tasks", (route) => (route.request().method() === "POST" ? route.fulfill({ status: 500, json: { error: "synthetic failure: tasks.json is locked" } }) : route.fallback()));
  await open(page, "week");
  await page.getByLabel("New task").fill("Prepare the roadmap review");
  await page.getByRole("button", { name: "Add task" }).click();
  await expect(page.getByRole("alert")).toContainText("Fix:");
  await shot(region(page, "Plan ahead"), "week-add-task-failed");
});
