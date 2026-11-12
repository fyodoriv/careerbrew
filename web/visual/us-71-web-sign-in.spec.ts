import { expect, shot, test } from "./helpers";

// US-71: a tab that cannot sign in (a new browser, a rotated or expired token) names the one command that fixes it.
test("US-71: the sign-in page", async ({ page }) => {
  await page.route("**/api/session", (route) => (route.request().method() === "GET" ? route.fulfill({ json: {} }) : route.fallback()));
  await page.goto("/#/today");
  await expect(page.getByRole("heading", { name: "Sign in to careerbrew" })).toBeVisible();
  await shot(page, "sign-in");
});
