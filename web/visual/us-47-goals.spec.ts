import { TODAY, addDays, expect, open, patch, region, shot, splitDates, test } from "./helpers";

// US-47: my upcoming goals (the nearest one drives today) and my long-term goals; only I set them.
test("US-47: upcoming and long-term goals", async ({ page }) => {
  const onsite = { id: "g1", kind: "interview", title: "Onsite at Northwind", date: addDays(TODAY, 14), company: "Northwind", topics: ["system design"], minutes_per_day: 60 };
  const graphs = { id: "g2", kind: "learn", title: "Learn graphs", date: addDays(TODAY, 40), company: null, topics: ["graphs"], minutes_per_day: 30 };
  await patch(page, "**/api/goals", (g) => ({ ...g, queue: [onsite, graphs], upcoming: onsite, days_left: 14, long_term: [{ id: "l1", text: "Staff engineer at a calm product company by 2028" }] }));
  await open(page, "goals");
  expect(await splitDates(region(page, "Upcoming goals")), "a goal's date stays on one line").toEqual([]);
  await shot(page.locator("main .split").first(), "goals");
});
