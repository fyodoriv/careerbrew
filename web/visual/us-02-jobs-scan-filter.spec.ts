import { open, region, shot, test } from "./helpers";

// US-02: postings from public company boards, filtered by my hard criteria, with why the rest were left out.
test("US-02: the postings that pass your criteria", async ({ page }) => {
  await open(page, "jobs");
  await shot(region(page, /^Postings that pass your criteria/), "postings");
});
