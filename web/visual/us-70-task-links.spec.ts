import { TODAY, open, region, shot, test } from "./helpers";

// US-70: a task's links (file, ticket, channel) and its own why on every row: only http(s) links are anchors.
test("US-70: task links on the Dashboard", async ({ page }) => {
  await open(page, "dashboard");
  await shot(region(page, "Right now"), "dashboard-right-now");
});

test("US-70: task links in the Week", async ({ page }) => {
  await open(page, "week");
  const monday = page.getByRole("article", { name: new RegExp(` ${TODAY}$`) });
  await shot(monday, "week-today");
});
