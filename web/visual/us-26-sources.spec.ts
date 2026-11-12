import { expect, open, region, shot, splitDates, test } from "./helpers";

// US-26: every company fact cites a public source, with the date it was checked.
test("US-26: a company page lists its sources", async ({ page }) => {
  await open(page, "interviews/Datadog");
  await page.getByText(/^Sources \(\d+\)/).click();
  const sources = region(page, "Sources");
  expect(await splitDates(sources), "each checked date on one line").toEqual([]);
  await shot(sources, "datadog-sources");
});
