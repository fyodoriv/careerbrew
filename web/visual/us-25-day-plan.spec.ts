import { shortDay } from "../src/dates";
import { TODAY, open, shot, test } from "./helpers";

// US-25: today's meetings and planned work in one time-ordered agenda, with free and focus time.
test("US-25: today's plan", async ({ page }) => {
  await open(page, "today");
  const plan = page.locator("div.split").filter({ has: page.getByRole("region", { name: `Plan for ${shortDay(TODAY)}`, exact: true }) });
  await shot(plan, "todays-plan");
});
