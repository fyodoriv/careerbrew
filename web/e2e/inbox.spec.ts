import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { NOW, OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import type { InboxGroup, InboxItem } from "../src/api";
import { ABOUT, VIEW_BLOCKS } from "../src/order";
import { VIEWS } from "../src/router";

// US-74, issue #147: the suggestions inbox is clear to a new user. Each row shows where it came from, a link, and
// what Accept and Dismiss will do; alike rows form one group with Accept all (asks first), Review each, and Dismiss
// all; an ask its source shows done moves to Done elsewhere; every block says what it is; Dashboard has a
// Getting started checklist. Tests share one seeded server: each one makes its own synthetic rows through the API
// (ticket keys SYN-*, repo acme/shop) and decides every row it made, so later files see the seed's inbox.

const HEADERS = { "X-Careerbrew-Token": TOKEN };
async function get<T>(page: Page, path: string): Promise<T> {
  const res = await page.request.get(path, { headers: HEADERS });
  expect(res.ok(), `GET ${path}`).toBe(true);
  return (await res.json()) as T;
}
async function action(page: Page, name: string, values: Record<string, unknown>, confirm = false): Promise<{ exit: number; output: string }> {
  const res = await page.request.post(`/api/actions/${name}`, { headers: HEADERS, data: { values, confirm } });
  expect(res.ok(), `action ${name}: ${await res.text()}`).toBe(true);
  return (await res.json()) as { exit: number; output: string };
}
const agentTask = (page: Page, title: string, why: string, link: string) => action(page, "work.task", { title, source: "agent", why, link: [link] });
const openTitles = async (page: Page) => (await get<{ tasks: { title: string }[] }>(page, "/api/work")).tasks.map((t) => t.title);
const go = (page: Page, name: string) => page.getByRole("navigation", { name: "Views" }).getByRole("link", { name, exact: true }).click();
const suggests = (page: Page) => page.locator('main [data-block="suggestions"]');

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  await page.goto(`/#token=${TOKEN}`);
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

// A row explains itself: its source, at least one link, and what Accept (or Approve goal) and Dismiss will do.
async function explainsItself(row: Locator, accept = /^(Accept|Approve goal|Accept all) will$/) {
  const terms = row.locator(":scope > dl.explain > dt");
  await expect(terms.filter({ hasText: /^Source$/ })).toHaveCount(1);
  await expect(row.locator(":scope > dl.explain > dt:text-is('Source') + dd")).not.toBeEmpty();
  await expect(terms.filter({ hasText: accept })).toHaveCount(1);
  await expect(terms.filter({ hasText: /^Dismiss( all)? will$/ })).toHaveCount(1);
  expect(await row.locator(":scope > dl.explain").getByRole("link").count(), "a link").toBeGreaterThan(0);
}

test("Today: each suggestion shows its source, a link, and what Accept and Dismiss do; Accept all accepts a group at once", async ({ page }) => {
  const titles = ["Draft the synthetic onboarding page", "Record the synthetic onboarding demo", "Write the synthetic onboarding quiz"];
  for (const [i, title] of titles.entries()) expect((await agentTask(page, title, "the synthetic onboarding sprint needs them", `https://tickets.example.com/browse/SYN-4${i + 1}`)).output).toContain("proposed");
  await go(page, "Today");
  const block = suggests(page);
  await expect(block.locator(".explainer")).toContainText("Nothing changes until you press Accept");
  await expect(block.locator(".explainer")).toContainText("plugins and agents add their own");

  // Every row on the page explains itself before the click.
  const rows = block.locator(":scope > ul.rows > li");
  await expect(rows.first()).toBeVisible();
  for (const row of await rows.all()) await explainsItself(row);

  // The three alike rows are one group: what Accept all and Dismiss all do, and Review each.
  const group = block.locator('li[data-inbox-group="agent-task"]').filter({ hasText: "Plan 3 tasks an agent added" });
  await expect(group).toContainText("an agent added them: the synthetic onboarding sprint needs them");
  await expect(group).toContainText("Moves all 3 tasks from proposed to open");
  await group.getByRole("button", { name: "Review each" }).click();
  const each = group.locator("li[data-inbox]");
  await expect(each).toHaveCount(3);
  for (const [i, title] of titles.entries()) {
    const row = each.filter({ hasText: title });
    await expect(row.getByRole("link", { name: /^tickets\.example\.com/ })).toHaveAttribute("href", `https://tickets.example.com/browse/SYN-4${i + 1}`);
    await expect(row.getByRole("link", { name: /^tickets\.example\.com/ })).toHaveAttribute("target", "_blank");
    await expect(row.getByRole("link", { name: "Week" })).toHaveAttribute("href", "#/week");
  }
  await page.screenshot({ path: `${OUT}/inbox.png`, fullPage: true });

  // Accept all asks first; Yes accepts every row at once.
  await group.getByRole("button", { name: "Accept all" }).click();
  await expect(group.getByRole("group", { name: "Confirm Accept all" })).toContainText("Accept all 3? Moves all 3 tasks from proposed to open");
  expect(await openTitles(page)).not.toContain(titles[0]);  // asked, not done
  await group.getByRole("button", { name: "Yes, accept all" }).click();
  await expect(group).toHaveCount(0);
  const open = await openTitles(page);
  for (const title of titles) expect(open, `${title} is planned`).toContain(title);
  const inbox = await get<{ inbox: InboxItem[]; groups: InboxGroup[] }>(page, "/api/inbox");
  expect(inbox.groups).toEqual([]);
  expect(inbox.inbox.map((i) => i.text).filter((t) => titles.some((x) => t.includes(x)))).toEqual([]);
});

test("Today: Dismiss all asks first, Cancel changes nothing, and Yes dismisses each row of the group", async ({ page }) => {
  const titles = ["Tidy the synthetic fixtures A", "Tidy the synthetic fixtures B", "Tidy the synthetic fixtures C"];
  for (const [i, title] of titles.entries()) await agentTask(page, title, "the synthetic cleanup week", `https://tickets.example.com/browse/SYN-5${i + 1}`);
  await go(page, "Today");
  const group = suggests(page).locator("li[data-inbox-group]").filter({ hasText: "the synthetic cleanup week" });
  await group.getByRole("button", { name: "Dismiss all" }).click();
  const confirm = group.getByRole("group", { name: "Confirm Dismiss all" });
  await expect(confirm).toContainText("Dismiss all 3? Sets all 3 tasks to dismissed; they are never planned. This is hard to undo.");
  await confirm.getByRole("button", { name: "Cancel" }).click();
  await expect(confirm).toHaveCount(0);
  expect((await get<{ groups: InboxGroup[] }>(page, "/api/inbox")).groups.map((g) => g.count)).toEqual([3]);  // Cancel: nothing changed
  await group.getByRole("button", { name: "Dismiss all" }).click();
  await group.getByRole("button", { name: "Yes, dismiss all" }).click();
  await expect(group).toHaveCount(0);
  const open = await openTitles(page);
  for (const title of titles) expect(open).not.toContain(title);
});

test("Today: an ask its source shows done moves to Done elsewhere, and Bring back returns it", async ({ page }) => {
  const title = "Review acme/shop#77 before the synthetic release";
  await agentTask(page, title, "a synthetic review request", "https://github.com/acme/shop/pull/77");
  // A review you gave on that PR, after the ask: data careerbrew already reads (the work feed). The watcher's next cycle
  // (`day inbox` here) sees it.
  const file = join(mkdtempSync(join(tmpdir(), "careerbrew-e2e-")), "review.jsonl");
  writeFileSync(file, JSON.stringify({ id: "github:review:e2e-77", source: "github", kind: "review", actor: "me", at: `${NOW}:30`, title: "Reviewed the synthetic PR", url: "https://github.com/acme/shop/pull/77#pullrequestreview-1" }) + "\n");
  await action(page, "work.feed.add", { file });
  await action(page, "day.inbox", {});
  await go(page, "Today");
  const block = suggests(page);
  await expect(block.getByRole("heading", { name: /^careerbrew suggests/ })).toBeVisible();
  await expect(block.locator(":scope > ul.rows").getByText(title)).toHaveCount(0);
  const fold = block.locator("details[data-done-elsewhere]");
  await fold.locator("summary").click();
  const gone = fold.locator("li[data-expired]").filter({ hasText: title });
  await expect(gone.getByRole("link", { name: "you gave a review (github)" })).toHaveAttribute("href", "https://github.com/acme/shop/pull/77#pullrequestreview-1");
  const kept = await get<{ expired: InboxItem[] }>(page, "/api/inbox");
  expect(kept.expired.some((i) => i.what?.includes(title)), "kept, never deleted").toBe(true);
  await gone.getByRole("button", { name: "Bring back" }).click();
  const back = block.locator(":scope > ul.rows > li").filter({ hasText: title });
  await expect(back).toBeVisible();
  await action(page, "day.inbox", {});  // the next cycle does not hide it again: the owner's choice wins
  await page.reload();
  await expect(back).toBeVisible();
  await back.getByRole("button", { name: "Dismiss" }).click();
  await expect(back).toHaveCount(0);
});

test("Today: filter suggestions by project and priority, sort them, and the URL keeps the choice", async ({ page }) => {
  const row = (i: number, project: string, priority: number | null): InboxItem => ({ id: `synthetic-${i}`, type: "proposal", kind: i % 3 ? "agent-task" : "source", text: `Synthetic suggestion ${i}`, status: "open", project, priority, created_at: `2026-10-${String(i).padStart(2, "0")}T09:00:00Z` });
  const rows = Array.from({ length: 14 }, (_, n) => row(n + 1, n < 5 ? "syn-platform" : n < 8 ? "syn-billing" : "", n < 5 ? 1 : n < 8 ? 2 : null));
  await page.route("**/api/inbox", (route) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ inbox: rows, groups: [], expired: [] }) }));
  await go(page, "Today");
  const block = suggests(page);
  const items = block.locator("li[data-inbox]");
  const project = block.getByRole("combobox", { name: "Filter suggestions by project", exact: true });
  await expect(project.locator("option")).toHaveText(["All (14)", "no project (6)", "syn-platform (5)", "syn-billing (3)"]);
  await project.selectOption("syn-billing");
  await expect(items).toHaveCount(3);
  expect(new URL(page.url()).hash).toBe("#/today?suggestions-project=syn-billing");
  await project.selectOption("");
  const priority = block.getByRole("combobox", { name: "Filter suggestions by priority", exact: true });
  await expect(priority.locator("option")).toHaveText(["All (14)", "no ranked project (6)", "rank 1 project (5)", "rank 2 project (3)"]);
  await priority.selectOption("p1");
  await expect(items).toHaveCount(5);
  await priority.selectOption("");
  const sort = block.getByRole("combobox", { name: "Sort suggestions", exact: true });
  await sort.selectOption("newest");
  await expect(items.first()).toHaveText(/^Synthetic suggestion 14(?!\d)/);
  await page.reload();
  await expect(block.getByRole("combobox", { name: "Sort suggestions", exact: true })).toHaveValue("newest");
  await expect(items.first()).toHaveText(/^Synthetic suggestion 14(?!\d)/);
  await sort.selectOption("priority");
  await expect(items.first()).toHaveText(/^Synthetic suggestion 1(?!\d)/);
  await expect(items.nth(5)).toHaveText(/^Synthetic suggestion 6(?!\d)/);
});

test("every view: each block says What is this?, and opens one or two sentences about it", async ({ page }) => {
  for (const v of VIEWS) {
    await page.goto(`/#/${v.id}`);
    await expect(page.getByRole("heading", { level: 1, name: v.label })).toBeVisible();
    await expect(page.locator(`main [data-block="${VIEW_BLOCKS[v.id][0]}"]`)).toBeVisible();
    await expect(page.locator("main .skeleton")).toHaveCount(0);
    const blocks = await page.locator("main [data-block]").evaluateAll((els) => els.filter((e) => !e.parentElement?.closest("[data-block]")).map((e) => e.getAttribute("data-block") ?? ""));
    expect(blocks.length, `${v.label} shows blocks`).toBeGreaterThan(0);
    for (const id of blocks) {
      const about = page.locator(`main [data-block="${id}"] details.about[data-about="${id}"]`).first();
      await expect(about.locator("summary"), `${v.label}: ${id}`).toHaveText("What is this?");
      await about.locator("summary").click();
      await expect(about.locator("p"), `${v.label}: ${id}`).toHaveText(ABOUT[id]);
    }
  }
});

test("Dashboard: Getting started ticks each step from data, links to where to do it, and Hide checklist hides it", async ({ page }) => {
  const c = await get<{ steps: { id: string; title: string; view: string; done: boolean }[]; done: number; total: number; show: boolean }>(page, "/api/checklist");
  expect(c.show, "the seed is a new user: not every step is done").toBe(true);
  await go(page, "Dashboard");
  const first = page.locator('main [data-block="first-run"]');
  await expect(first.getByRole("heading", { name: `Getting started (${c.done} of ${c.total} done)` })).toBeVisible();
  for (const s of c.steps) {
    const step = first.locator(`li[data-step="${s.id}"]`);
    await expect(step.getByRole("link", { name: s.title })).toHaveAttribute("href", s.view);
    await expect(step).toHaveAttribute("data-done", s.done ? "yes" : "no");
  }
  await first.getByRole("button", { name: "Hide checklist" }).click();
  await expect(first).toHaveCount(0);
  const settings = (await get<{ settings: { key: string; value: unknown }[] }>(page, "/api/config")).settings;
  expect(settings.find((s) => s.key === "first_run_checklist")?.value).toBe(false);
  await page.reload();
  await expect(page.getByRole("heading", { name: "Right now" })).toBeVisible();
  await expect(first).toHaveCount(0);
  await action(page, "config.unset", { key: "first_run_checklist" });  // shared seed: put it back
});
