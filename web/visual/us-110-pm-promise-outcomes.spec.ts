import { addDays, answer, expect, open, shot, test, TODAY } from "./helpers";

test("US-110: confirm a promise outcome", async ({ page }) => {
  const prior = addDays(TODAY, 15), current = addDays(TODAY, 17), separate = addDays(TODAY, 16), original = addDays(TODAY, 14);
  const readable = (day: string) => new Intl.DateTimeFormat("en-US", { weekday: "short", month: "short", day: "numeric", timeZone: "UTC", ...(day.slice(0, 4) === TODAY.slice(0, 4) ? {} : { year: "numeric" }) }).format(new Date(`${day}T00:00:00Z`)).replaceAll(",", "");
  await answer(page, "**/api/actions/work.promise.outcomes", {
    action: "work.promise.outcomes", argv: [], exit: 0,
    output: JSON.stringify({ outcomes: [{ promise: "promise-0000000a", state: "decision-needs-review", prior_state: "moved", current_state: "open", prior_due: prior, current_due: current, id: "", due: "", resolutions: [
      { choice: "keep", id: "synthetic-keep", due: prior, separate_due: separate, separate_existing: false },
      { choice: "reopen", id: "synthetic-reopen", due: original, separate_due: separate, separate_existing: false },
      { choice: "drop", id: "synthetic-drop", due: "", separate_due: separate, separate_existing: false },
    ] }] }),
  });
  await open(page, "actions");
  await page.getByLabel("Filter actions").fill("work.promise.accept");
  await page.getByRole("button", { name: "work.promise.accept", exact: true }).click();
  await expect(page.getByLabel("yes", { exact: true })).toHaveCount(0);
  await expect(page.getByText(`promise-0000000a: prior moved, due ${readable(prior)}; current open, due ${readable(current)}.`)).toBeVisible();
  await expect(page.getByText(`promise-0000000a: Keep the prior decision, due ${readable(prior)}; create a separate promise due ${readable(separate)}`)).toBeVisible();
  await expect(page.getByText(`promise-0000000a: Reopen, due ${readable(original)}; create a separate promise due ${readable(separate)}`)).toBeVisible();
  await expect(page.getByText("Mark dropped")).toBeVisible();
  for (const label of await page.locator('form[aria-label="Run work.promise.accept"] label.field').all()) {
    expect((await label.boundingBox())?.height).toBeGreaterThanOrEqual(44);
  }
  await expect(page.getByText("I understand: it records your decision on one current PM promise outcome")).toBeVisible();
  await shot(page.locator("main .split"), "promise-outcome-confirmation");
});
