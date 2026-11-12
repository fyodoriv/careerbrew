import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { OUT, TOKEN } from "../playwright.config";

// US-82 (issue #198): Active now shows the ranked projects on Today and Work with the numbers of
// `work priority show`, and Up, Down, Remove, and Put back run the same `work priority set` and `remove` actions.
// The seed ranks nothing (a ranking changes the PM update, the plan, and Next step of every other spec), so each
// test ranks two seeded projects first and removes every ranking after.

const HEADERS = { "X-Careerbrew-Token": TOKEN };
interface Row { name: string; rank: number; match: string[]; open_tasks: number; today: number }

async function ranking(page: Page): Promise<Row[]> {
  return ((await (await page.request.get("/api/priority", { headers: HEADERS })).json()) as { projects: Row[] }).projects;
}

async function action(page: Page, name: string, values: Record<string, unknown>) {
  const res = await page.request.post(`/api/actions/${name}`, { headers: HEADERS, data: { values, confirm: false } });
  const out = (await res.json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
}

// Click a control and wait for its action's answer, so a slow run is not part of the next check's wait.
async function click(page: Page, button: Locator, name: string) {
  const answer = page.waitForResponse((r) => r.url().endsWith(`/api/actions/${name}`) && r.request().method() === "POST");
  await button.click();
  const out = (await (await answer).json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
}

const block = (page: Page) => page.getByRole("region", { name: "Active now" });
const rows = (page: Page) => block(page).locator("li[data-rank]");
const label = (r: Row) => `${r.open_tasks} open task${r.open_tasks === 1 ? "" : "s"} · ${r.today} item${r.today === 1 ? "" : "s"} today`;

test.beforeEach(async ({ page }) => {
  for (const r of await ranking(page)) await action(page, "work.priority.remove", { project: r.name });
  await action(page, "work.priority.set", { project: "billing", match: ["*billing*"] }); // the seed's "Migrate billing jobs"
  await action(page, "work.priority.set", { project: "ledger" });
  await page.goto(`/#token=${TOKEN}`);
});

test.afterEach(async ({ page }) => {
  for (const r of await ranking(page)) await action(page, "work.priority.remove", { project: r.name });
});

test("Today: Active now shows the ranked projects with work priority show's numbers; Up, Remove, and Put back change the ranking", async ({ page }) => {
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
  const before = await ranking(page);
  expect(before.map((r) => r.name)).toEqual(["billing", "ledger"]);
  expect(before[0].open_tasks).toBeGreaterThan(0);
  await expect(rows(page)).toHaveCount(2);
  for (const [i, r] of before.entries()) {
    await expect(rows(page).nth(i)).toContainText(`#${r.rank} ${r.name}`);
    await expect(rows(page).nth(i)).toContainText(label(r));
  }
  await expect(block(page).getByRole("button", { name: "Move up: billing" })).toBeDisabled();
  await page.screenshot({ path: `${OUT}/today-active-now.png`, fullPage: true });

  await click(page, block(page).getByRole("button", { name: "Move up: ledger" }), "work.priority.set");
  expect((await ranking(page)).map((r) => r.name)).toEqual(["ledger", "billing"]);
  await expect(rows(page).nth(0)).toContainText("#1 ledger");

  await click(page, block(page).getByRole("button", { name: "Remove from ranking: billing" }), "work.priority.remove");
  expect((await ranking(page)).map((r) => r.name)).toEqual(["ledger"]);
  await expect(rows(page)).toHaveCount(1);
  await click(page, block(page).getByRole("button", { name: "Put back: billing" }), "work.priority.set");
  const back = await ranking(page);
  expect(back.map((r) => [r.name, r.match])).toEqual([["ledger", ["ledger"]], ["billing", ["*billing*"]]]);
  await expect(rows(page)).toHaveCount(2);
});

test("Work: ranked projects come first in Projects with a rank badge, and Down there reorders both lists", async ({ page }) => {
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Work", exact: true }).click();
  const projects = page.getByRole("region", { name: "Projects" }).locator("li");
  const badged = projects.filter({ has: page.locator(".tag-rank") });
  await expect(projects.nth(0)).toContainText("#1 billing");
  await expect(projects.nth(1)).toContainText("↳ invoices"); // a ranked project moves with its children (review of #204)
  await expect(badged.nth(1)).toContainText("#2 ledger");
  await click(page, block(page).getByRole("button", { name: "Move down: billing" }), "work.priority.set");
  expect((await ranking(page)).map((r) => r.name)).toEqual(["ledger", "billing"]);
  await expect(rows(page).nth(0)).toContainText("#1 ledger");
  await expect(projects.nth(0)).toContainText("#1 ledger");
  await expect(badged.nth(1)).toContainText("#2 billing");
});
