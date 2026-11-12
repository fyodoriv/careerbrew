import { expect, open, shot, test } from "./helpers";

// US-44: a public study card's explanation reads in the Library, with a way to report a problem.
test("US-44: a study card's explanation in the Library", async ({ page }) => {
  await open(page, "library");
  await page.getByLabel("Filter pages").fill("binary search");
  await page.getByRole("link", { name: "How does binary search work?" }).click();
  const reader = page.locator("article.reader");
  await expect(reader).toContainText("def binary_search");
  await shot(reader, "binary-search-card");
});
