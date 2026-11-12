import { expect, open, patch, region, shot, test } from "./helpers";

// US-53: every CLI action runs from the web app with a generated form; terminal-only ones say why and give the
// command; the PM update goes to Slack.
test("US-53: an action's generated form", async ({ page }) => {
  await open(page, "actions");
  const filter = page.getByLabel("Filter actions");
  // The filter spans its card, so its hint is readable (US-75 visual review: it was cut at "backu").
  const [box, card] = await Promise.all([filter.boundingBox(), region(page, /^Actions \(\d+\)$/).boundingBox()]);
  expect(box!.width).toBeGreaterThan(card!.width - 40);
  await filter.fill("config.set");
  await page.getByRole("button", { name: "config.set", exact: true }).click();
  await shot(page.locator("main .split"), "config-set-form");
});

test("US-53: a terminal-only action says why", async ({ page }) => {
  await open(page, "actions");
  await page.getByLabel("Filter actions").fill("secret.set");
  await page.getByRole("button", { name: "secret.set", exact: true }).click();
  await expect(page.getByText(/Terminal only/)).toBeVisible();
  await shot(region(page, "secret.set"), "terminal-only");
});

test("US-53: profile update form", async ({ page }) => {
  await open(page, "actions");
  await page.getByLabel("Filter actions").fill("data.set");
  await page.getByRole("button", { name: "data.set", exact: true }).click();
  await shot(page.locator("main .split"), "profile-set-form");
});

test("US-53: project quiet form", async ({ page }) => {
  await open(page, "actions");
  await page.getByLabel("Filter actions").fill("work.project.set");
  await page.getByRole("button", { name: "work.project.set", exact: true }).click();
  await shot(page.locator("main .split"), "project-quiet-form");
});

test("US-53: the PM update with Send to Slack", async ({ page }) => {
  await patch<{ pm: object }>(page, "**/api/daylog", (d) => ({ ...d, pm: { ...d.pm, to: "#team-updates", via: "slack" } }));
  await open(page, "today");
  await expect(page.getByRole("button", { name: "Send to Slack #team-updates" })).toBeEnabled();
  await shot(region(page, "Update for your PM"), "send-to-slack");
});
