import { expect, open, overflowing, shot, test } from "./helpers";

// US-39: recall first (the answer stays hidden), then the answer, its explanation one click away, and the four grades.
test("US-39: a flashcard before and after the answer shows", async ({ page }) => {
  await open(page, "practice");
  const card = page.locator("section.study");
  await shot(card, "card-question");
  await page.keyboard.press("Space");
  await expect(page.locator(".flashcard .answer")).toBeVisible();
  await card.getByText(/^Explanation/).click();
  // Nothing in the card is wider than the card (US-75 visual review: at phone width the text and grades were cut off).
  expect(await overflowing(card)).toEqual([]);
  await shot(card, "card-answer");
});
