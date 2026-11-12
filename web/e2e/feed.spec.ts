import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { NOW, OUT, TOKEN } from "../playwright.config";

// US-63: Today lists the work feed's open asks (seeded from tests/fixtures/feed/web-asks.jsonl, moved to the pinned
// day: each ask keeps its local time and its distance in workdays), and Mark personal runs the same `work feed mark`
// action as the CLI. So at the pinned 09:00 the waits below hold on any day, weekends included.

const DANA = "slack:D0DANA:1792410900.000400";
const BUDGET = "Mail from Finance Team: Budget numbers for the Q1 platform roadmap";
const FAILURE = "synthetic failure for the open asks test";
// The seed's notifications block (issue #74) adds a DM and a page: both are open asks too.
const SEED_DM = "DM from Sam: can you look at the rollout?";
const SEED_PAGE = "Page: checkout latency over SLO";
const NEXT_STEP_CASE = "Next step links the selected open ask";
const NEXT_STEP_TITLE = "Synthetic ask: confirm the rollout note";
const NEXT_STEP_URL = "https://example.com/next-step/rollout";

test.beforeEach(async ({ page }, info) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  if (info.title === NEXT_STEP_CASE) {
    const port = process.env.CAREERBREW_NEXT_STEP_PORT;
    if (!port || !/^[0-9]+$/.test(port) || Number(port) < 1 || Number(port) > 65535) {
      throw new Error("Owned Next step server did not announce its port");
    }
    const origin = `http://127.0.0.1:${port}`;
    await page.route("**/*", (route) => new URL(route.request().url()).origin === origin ? route.continue() : route.abort());
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(`${origin}/#token=${TOKEN}`);
  } else {
    await page.goto(`/#token=${TOKEN}`);
  }
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

const asks = (page: Page) => page.getByRole("region", { name: /^Open asks/ });
const rows = (page: Page) => asks(page).locator("li[data-ask]");

// Click a Mark button and wait for the action's answer before the next check. The run's own time on a busy machine
// is then not part of that check's 15 s wait, and a failed run shows its output instead of an old count.
async function mark(page: Page, button: Locator) {
  const answer = page.waitForResponse((r) => r.url().endsWith("/api/actions/work.feed.mark") && r.request().method() === "POST");
  await button.click();
  const res = await answer;
  expect(res.status()).toBe(200);
  const out = (await res.json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
  return res.request();
}

test("Today lists open asks with their source link, and Mark personal runs work feed mark", async ({ page }) => {
  await expect(rows(page)).toHaveCount(4);
  // newest first (issue #275): the seed's page (08:40) and DM (08:30), then dana's DM from 08:15, before the workday,
  // so it waits 0m; last the mail from 16:30 two workdays ago, which waits 9h 30m of work time (1h that day + 8h 30m
  // the next; a weekend between does not count) and is over 8 work hours
  await expect(rows(page).nth(0)).toContainText(SEED_PAGE);
  await expect(rows(page).nth(1)).toContainText(SEED_DM);
  await expect(rows(page).nth(2)).toContainText("DM from dana");
  await expect(rows(page).nth(2)).toContainText("waiting 0m in work hours");
  await expect(rows(page).nth(2)).not.toContainText("over 8");
  await expect(rows(page).nth(3)).toContainText(BUDGET);
  await expect(rows(page).nth(3)).toContainText("over 8 work hours");
  await expect(rows(page).nth(3)).toContainText("waiting 9h 30m in work hours");
  await expect(asks(page).getByRole("link", { name: "DM from dana" })).toHaveAttribute("href", "https://example.slack.com/archives/D0DANA/p1792410900000400");
  await expect(asks(page)).not.toContainText("Mention from sam"); // answered in its thread
  await expect(asks(page)).not.toContainText("DM from robin"); // personal
  await page.screenshot({ path: `${OUT}/today-open-asks.png`, fullPage: true });

  const sent = await mark(page, asks(page).getByRole("button", { name: "Mark personal: DM from dana" }));
  expect(sent.postDataJSON()).toEqual({ values: { id: DANA, value: "personal" }, confirm: false });
  await expect(rows(page)).toHaveCount(3);
  await expect(asks(page).getByRole("status")).toContainText("Marked personal: DM from dana");
  await expect(page.getByRole("heading", { name: "Open asks (3)" })).toBeVisible();

  // a reload reads the stored mark; Undo marks an ask work again
  await page.reload();
  await expect(rows(page)).toHaveCount(3);
  await mark(page, asks(page).getByRole("button", { name: `Mark personal: ${BUDGET}` }));
  await expect(rows(page)).toHaveCount(2);
  await expect(rows(page).filter({ hasText: BUDGET })).toHaveCount(0);
  await mark(page, asks(page).getByRole("button", { name: "Undo" }));
  await expect(rows(page)).toHaveCount(3);
  await expect(rows(page).last()).toContainText(BUDGET);
});

test("Not an ask and Done close an ask, and Undo opens it again (issue #185)", async ({ page }, info) => {
  await page.emulateMedia({ reducedMotion: "reduce" });
  await expect(rows(page).filter({ hasText: SEED_PAGE })).toHaveCount(1);
  const before = await rows(page).count();
  const card = asks(page);
  const closed = card.getByText(/^Closed today:/);
  const initial = await closed.allTextContents();
  const sent = await mark(page, asks(page).getByRole("button", { name: `Not an ask: ${SEED_PAGE}` }));
  expect(sent.postDataJSON()).toEqual({ values: { id: expect.any(String), value: "not-ask" }, confirm: false });
  await expect(rows(page)).toHaveCount(before - 1);
  await expect(asks(page).getByRole("status")).toContainText(`Marked not an ask: ${SEED_PAGE}`);
  const notAnAsk = await closed.allTextContents();
  await mark(page, asks(page).getByRole("button", { name: "Undo" }));
  await expect(rows(page)).toHaveCount(before);
  const restoredAsk = await closed.allTextContents();
  await mark(page, asks(page).getByRole("button", { name: `Done: ${SEED_DM}` }));
  await expect(rows(page)).toHaveCount(before - 1);
  await expect(asks(page).getByRole("status")).toContainText(`Marked done elsewhere: ${SEED_DM}`);
  const done = await closed.allTextContents();
  await mark(page, asks(page).getByRole("button", { name: "Undo" })); // leave the seed as it was for the next specs
  await expect(rows(page)).toHaveCount(before);
  const undone = await closed.allTextContents();

  const observed = { initial, notAnAsk, restoredAsk, done, undone };
  await info.attach("issue377-closed-today-observed", {
    body: JSON.stringify(observed),
    contentType: "application/json",
  });
  await card.screenshot({ path: info.outputPath("issue377-open-asks.png"), animations: "disabled" });
  expect(observed).toEqual({
    initial: ["Closed today: 1"],
    notAnAsk: ["Closed today: 1"],
    restoredAsk: ["Closed today: 1"],
    done: ["Closed today: 2"],
    undone: ["Closed today: 1"],
  });
});

test("Past 50 open asks, Today lists the newest first, says Showing 50 of N, and Show all loads the rest (issue #275)", async ({ page }) => {
  // 60 synthetic asks answered in the browser the way the server answers: the `limit` newest (50 by default) and the total.
  const all = Array.from({ length: 60 }, (_, i) => ({ id: `synthetic:${60 - i}`, source: "acme", kind: "dm", at: `${NOW}:00Z`, title: `Synthetic ask ${60 - i}`, url: "", waiting_minutes: 0, work_minutes: 0, overdue: false }));
  const limits: string[] = [];
  await page.route((u) => u.pathname === "/api/feed/asks", (route) => {
    const limit = new URL(route.request().url()).searchParams.get("limit");
    limits.push(limit ?? "default");
    return route.fulfill({ contentType: "application/json", body: JSON.stringify({ asks: all.slice(0, Number(limit ?? 50)), total: 60, limit: 1000, events: 60, overdue_hours: 8 }) });
  });
  await page.reload();
  await expect(page.getByRole("heading", { name: "Open asks (60)" })).toBeVisible();
  await expect(rows(page).first()).toContainText("Synthetic ask 60"); // the newest ask is on top, never hidden
  const pager = asks(page).getByRole("group", { name: "Pages of open asks" });
  await expect(pager).toContainText("of 50");
  await expect(asks(page)).toContainText("Showing 50 of 60 open asks, newest first.");
  await asks(page).getByRole("button", { name: "Show all 60" }).click();
  await expect(pager).toContainText("of 60");
  await expect(asks(page).getByRole("button", { name: /^Show all/ })).toHaveCount(0);
  await expect(asks(page)).not.toContainText("Showing 50 of 60");
  expect(limits).toContain("60");
  await page.screenshot({ path: `${OUT}/today-open-asks-show-all.png`, fullPage: true });
});

test("Mark personal shows marking, then the error and its fix", async ({ page }) => {
  let release = () => {};
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/actions/work.feed.mark", async (route) => {
    await gate;
    await route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: FAILURE }) });
  });
  const button = asks(page).getByRole("button", { name: /^Mark personal:/ }).first();
  await button.click();
  await expect(asks(page).locator(".working").filter({ hasText: "Marking…" })).toBeVisible();
  await expect(button).toBeDisabled();
  release();
  await expect(asks(page).getByRole("alert").filter({ hasText: FAILURE })).toContainText("Fix:");
  await expect(rows(page)).not.toHaveCount(0); // the server never saw the request: nothing changed
});


test("Next step links the selected open ask", async ({ page }, info) => {
  await expect(rows(page)).toHaveCount(1);
  await expect(rows(page).first()).toContainText(NEXT_STEP_TITLE);
  const card = page.getByRole("region", { name: "Next step", exact: true });
  await expect(card.locator(".next")).toHaveText(/^(14 min free and nothing fits: review a PR or clear your inbox\.|Synthetic ask: confirm the rollout note)$/);
  await card.screenshot({ path: info.outputPath("next-step.png"), animations: "disabled" });

  await expect(card.locator(".next")).toHaveText(NEXT_STEP_TITLE);
  await expect(card).toContainText("Read and respond to this ask (5 min).");
  const link = card.getByRole("link", { name: "Open source", exact: true });
  await expect(link).toHaveAttribute("href", NEXT_STEP_URL);
  await expect(link).toHaveAttribute("target", "_blank");
  await expect(link).toHaveAttribute("rel", /(?:^|\s)noopener(?:\s|$)/);
  await expect(link).toHaveAttribute("rel", /(?:^|\s)noreferrer(?:\s|$)/);
});
