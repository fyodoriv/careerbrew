import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { OUT, TOKEN } from "../playwright.config";

// US-91 (issue #272): a background agent reports each step with `work agent event` while it works, and Work's
// Agent activity shows it within seconds, with no reload: the event lands under work/, `/api/changes` moves, and
// the live block refetches on the next poll (5 s). The test posts through the same action the CLI runs, from
// outside the page, as a background agent would. Each run has a fresh agent name, so a rerun never sees old rows.

const HEADERS = { "X-Careerbrew-Token": TOKEN };
const LIVE_MS = 10_000; // one 5 s poll plus the refetch: the issue's "within seconds"

async function report(page: Page, values: Record<string, unknown>) {
  const res = await page.request.post("/api/actions/work.agent.event", { headers: HEADERS, data: { values, confirm: false } });
  const out = (await res.json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
}

test("Work: an agent's event shows in Agent activity within seconds with no reload, grouped by project with its link; done ends the run", async ({ page }) => {
  const agent = `review-bot-${Date.now().toString(36)}`;
  await page.goto(`/#token=${TOKEN}`);
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Work", exact: true }).click();
  const block = page.getByRole("region", { name: "Agent activity" });
  await expect(block).toBeVisible();
  await expect(block.locator(`li[data-agent="${agent}"]`)).toHaveCount(0);
  let navigations = 0;
  page.on("framenavigated", (f) => { if (f === page.mainFrame()) navigations += 1; });

  const started = Date.now();
  await report(page, { agent, title: "Reviewing PR 7", project: "billing", url: "https://github.example.com/org/shop/pull/7" });
  const row = block.locator(`li[data-agent="${agent}"]`);
  await expect(row).toBeVisible({ timeout: LIVE_MS });
  const seconds = (Date.now() - started) / 1000;
  console.log(`agent event shown after ${seconds.toFixed(1)} s`);
  expect(seconds).toBeLessThan(LIVE_MS / 1000);
  expect(navigations, "the page never reloaded").toBe(0);
  await expect(row).toHaveAttribute("data-status", "running");
  await expect(block.getByRole("region", { name: "Agents: billing" }).locator(`li[data-agent="${agent}"]`)).toBeVisible();
  await expect(row.getByRole("link", { name: "Reviewing PR 7" })).toHaveAttribute("href", "https://github.example.com/org/shop/pull/7");
  await page.screenshot({ path: `${OUT}/work-agent-activity.png`, fullPage: true });

  await report(page, { agent, title: "Left 3 comments on PR 7", status: "done", project: "billing", url: "https://github.example.com/org/shop/pull/7" });
  await expect(row).toHaveAttribute("data-status", "done", { timeout: LIVE_MS });
  await expect(row).toContainText("Left 3 comments on PR 7");
  await expect(row).toContainText("2 events since");
  expect(navigations, "the page never reloaded").toBe(0);
});
