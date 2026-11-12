import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { NOW, OUT, TOKEN } from "../playwright.config";

// US-99 (issue #268): promises you made show on Dashboard's Commitments block until you close them. The seed has
// none (the block would change every Dashboard picture), so each test tracks its own through the `work promise add`
// action and closes every open one after.

const HEADERS = { "X-Careerbrew-Token": TOKEN };
interface Row { name: string; title: string; to: string; due: string; url: string; next: string }

async function promises(page: Page): Promise<Row[]> {
  return ((await (await page.request.get("/api/promises", { headers: HEADERS })).json()) as { promises: Row[] }).promises;
}

async function track(page: Page, text: string) {
  const values = { text, to: "Sam", due: NOW.slice(0, 10), link: "https://example.com/threads/timeout" };
  const res = await page.request.post("/api/actions/work.promise.add", { headers: HEADERS, data: { values, confirm: false } });
  const out = (await res.json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
}

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
});

test.afterEach(async ({ page }) => {
  for (const p of await promises(page)) await page.request.post(`/api/promises/${p.name}/done`, { headers: HEADERS, data: {} });
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

const block = (page: Page) => page.getByRole("region", { name: /^Commitments/ });

test("Commitments lists a promise with its link, who it was made to, and the next step; Done closes it", async ({ page }) => {
  await track(page, "Look into the synthetic timeout");
  await page.goto(`/#/dashboard&token=${TOKEN}`);
  await expect(block(page)).toHaveAttribute("aria-label", "Commitments (1)");
  const link = block(page).getByRole("link", { name: "Look into the synthetic timeout" });
  await expect(link).toHaveAttribute("href", "https://example.com/threads/timeout");
  await expect(block(page)).toContainText("to Sam");
  await expect(block(page)).toContainText("next: research what the promise needs");
  await page.screenshot({ path: `${OUT}/commitments.png`, fullPage: true });
  const answer = page.waitForResponse((r) => /\/api\/promises\/promise-[0-9a-f]{8}\/done$/.test(r.url()) && r.request().method() === "POST");
  await block(page).getByRole("button", { name: "Done: Look into the synthetic timeout" }).click();
  expect((await answer).status()).toBe(200);
  await expect(block(page)).toHaveCount(0);  // its only promise closed: the block goes
  expect(await promises(page)).toEqual([]);
});

test("Not a promise drops a wrong detection and keeps the others", async ({ page }) => {
  await track(page, "Send the synthetic rollout notes");
  await track(page, "Check the synthetic dashboards");
  await page.goto(`/#/dashboard&token=${TOKEN}`);
  await expect(block(page)).toHaveAttribute("aria-label", "Commitments (2)");
  await block(page).getByRole("button", { name: "Not a promise: Check the synthetic dashboards" }).click();
  await expect(block(page)).toHaveAttribute("aria-label", "Commitments (1)");
  await expect(block(page).getByRole("link", { name: "Send the synthetic rollout notes" })).toBeVisible();
  expect((await promises(page)).map((p) => p.title)).toEqual(["Send the synthetic rollout notes"]);
});
