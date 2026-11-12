import { expect, test } from "@playwright/test";
import type { Page, Route } from "@playwright/test";
import { NOW, OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import type { BoardJob, DayLogView, FeedAsk, FeedAsks, FocusBlock, InboxItem, Jobs, Posting, ProjectRow, PromiseRow, Promises, ResultRow, RunSummary, StaleSource, Standup, StandupAction, StandupRow, Task, TierRow, Today, Work } from "../src/api";
import { PAGE_SIZE, VIEW_BLOCKS } from "../src/order";
import { VIEWS } from "../src/router";

// US-73, issue #146: every view puts the block you act on first at the top, in the order docs/web.md lists
// (VIEW_BLOCKS), and a list of live data longer than its page shows one page, "Showing 1–10 of N", and Previous
// and Next, with the page and the filter in the URL hash so a reload keeps the place. Folded blocks show a count.
// Long lists are synthetic rows answered in the browser (page.route): the shared seeded server never changes here.

// Blocks the seed may not have: Getting started (gone once every step is done or hidden), two days of logs, other
// work in the standup, closed notifications, chosen jobs, and Active now on Today (shown once a project is ranked).
const OPTIONAL: Partial<Record<string, string[]>> = { dashboard: ["first-run", "commitments"], today: ["active-now", "earlier-days"], standup: ["groups"], notifications: ["closed"], jobs: ["tiers", "job-suggestions"] };

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

// The view's top-level blocks in page order (a block inside another block does not count).
const blocksOnPage = (page: Page) =>
  page.locator("main [data-block]").evaluateAll((els) => els.filter((e) => !e.parentElement?.closest("[data-block]")).map((e) => e.getAttribute("data-block")));

const hash = (page: Page) => new URL(page.url()).hash;

for (const v of VIEWS) {
  const expected = VIEW_BLOCKS[v.id];
  test(`${v.label}: the first block is ${expected[0]}, and the blocks follow the order in docs/web.md`, async ({ page }) => {
    await page.goto(`/#/${v.id}&token=${TOKEN}`);
    await expect(page.getByRole("heading", { level: 1, name: v.label })).toBeVisible();
    await expect(page.locator(`main [data-block="${expected[0]}"]`)).toBeVisible();
    await expect(page.locator("main .skeleton")).toHaveCount(0);
    const found = await blocksOnPage(page);
    expect(found[0], "the first block").toBe(expected[0]);
    expect(found, "every block on the page is in VIEW_BLOCKS, in its order").toEqual(expected.filter((id) => found.includes(id)));
    for (const id of expected) if (!OPTIONAL[v.id]?.includes(id)) expect(found, `${v.label} shows ${id}`).toContain(id);
    if (v.id === "today" || v.id === "dashboard") await page.screenshot({ path: `${OUT}/order-${v.id}.png`, fullPage: true });
  });
}

const suggestion = (i: number, kind: string) => ({ id: `synthetic-${i}`, type: "proposal", kind, text: `Synthetic suggestion ${i}`, status: "open" });

test("a list longer than its page shows one page and Showing 1–10 of N, and the URL keeps its page and filter across a reload", async ({ page }) => {
  const rows = [...Array.from({ length: 19 }, (_, i) => suggestion(i + 1, "agent-task")), ...Array.from({ length: 4 }, (_, i) => suggestion(20 + i, "goal"))];
  await page.route("**/api/inbox", (route) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ inbox: rows }) }));
  await page.goto(`/#/today&token=${TOKEN}`);
  const block = page.locator('main [data-block="suggestions"]');
  await expect(block.getByRole("heading", { name: "careerbrew suggests (23)" })).toBeVisible();
  const items = block.locator("li[data-inbox]");
  await expect(items).toHaveCount(PAGE_SIZE.suggestions);
  await expect(items.first()).toHaveText(/^Synthetic suggestion 1(?!\d)/);
  const pager = block.getByRole("group", { name: "Pages of suggestions" });
  await expect(pager).toContainText("Showing 1–10 of 23");
  await expect(pager.getByRole("button", { name: "Previous" })).toBeDisabled();

  // Next: the second page, and the hash says so.
  await pager.getByRole("button", { name: "Next" }).click();
  await expect(pager).toContainText("Showing 11–20 of 23");
  await expect(items).toHaveCount(PAGE_SIZE.suggestions);
  await expect(items.first()).toHaveText(/^Synthetic suggestion 11(?!\d)/);
  expect(hash(page)).toBe("#/today?suggestions=2");
  await page.screenshot({ path: `${OUT}/paging.png`, fullPage: true });

  // A reload keeps the place.
  await page.reload();
  await expect(pager).toContainText("Showing 11–20 of 23");
  await expect(items.first()).toHaveText(/^Synthetic suggestion 11(?!\d)/);

  // The filter: one button per kind with its count; it starts at page 1 and stays in the hash.
  const filter = block.getByRole("group", { name: "Filter suggestions" });
  await expect(filter.getByRole("button")).toHaveText(["All (23)", "agent-task (19)", "goal (4)"]);
  await filter.getByRole("button", { name: "goal (4)" }).click();
  await expect(items).toHaveCount(4);
  await expect(pager).toHaveCount(0);
  expect(hash(page)).toBe("#/today?suggestions-filter=goal");
  await page.reload();
  await expect(items).toHaveCount(4);
  await expect(filter.getByRole("button", { name: "goal (4)" })).toHaveAttribute("aria-pressed", "true");

  // A link to the view starts on page 1 with no filter.
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Week", exact: true }).click();
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
  await expect(pager).toContainText("Showing 1–10 of 23");

  // All clears a chosen filter: every kind again, from page 1, and the hash drops the filter.
  await filter.getByRole("button", { name: "goal (4)" }).click();
  await expect(items).toHaveCount(4);
  await filter.getByRole("button", { name: "All (23)" }).click();
  await expect(pager).toContainText("Showing 1–10 of 23");
  await expect(items).toHaveCount(PAGE_SIZE.suggestions);
  expect(hash(page)).toBe("#/today");
});

test("error case: a page past the end shows the last page, and typed text never lands in the URL", async ({ page }) => {
  const rows = Array.from({ length: 23 }, (_, i) => suggestion(i + 1, "agent-task"));
  await page.route("**/api/inbox", (route) => route.fulfill({ contentType: "application/json", body: JSON.stringify({ inbox: rows }) }));
  // A hand-edited link: a page that does not exist, and text that is not a bare token.
  await page.goto(`/#/today?suggestions=99&q=my%20private%20note&token=${TOKEN}`);
  const block = page.locator('main [data-block="suggestions"]');
  const pager = block.getByRole("group", { name: "Pages of suggestions" });
  await expect(pager).toContainText("Showing 21–23 of 23");
  await expect(block.locator("li[data-inbox]")).toHaveCount(3);
  await pager.getByRole("button", { name: "Previous" }).click();
  await expect(pager).toContainText("Showing 11–20 of 23");
  expect(hash(page), "the app writes bare tokens only").toBe("#/today?suggestions=2");

  // A typed filter (Library) stays out of the URL.
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Library", exact: true }).click();
  await page.getByLabel("Filter pages").fill("binary search");
  await expect(page.getByRole("link", { name: "How does binary search work?" })).toBeVisible();
  expect(hash(page)).toBe("#/library");
});

// A synthetic notification, open ("new") or closed ("dismissed").
const notification = (id: string, headline: string, status: string, at: string) => ({
  id, kind: "dm", source: "slack", headline, title: "", why: "a synthetic row for the paging test", next: "nothing",
  priority: 3, priority_why: "a test row", count: 1, at, status, snooze_until: "", resolved: status === "dismissed" ? "dismissed" : "",
  url: "", link_label: "", accept_label: "", actions: status === "new" ? ["dismiss"] : [], popup: "held",
});

// 14 open notifications and 3 closed ones, on top of the seed's real status (the rules line).
async function manyNotifications(route: Route) {
  const res = await route.fetch();
  const body = await res.json();
  const item = (i: number, status: string) => notification(`synthetic-${i}`, `Synthetic notification ${i}`, status, `${body.items[0]?.at ?? ""}`);
  body.items = [...Array.from({ length: 14 }, (_, i) => item(i + 1, "new")), ...Array.from({ length: 3 }, (_, i) => item(15 + i, "dismissed"))];
  await route.fulfill({ response: res, json: body });
}

test("Notifications: Open pages 10 at a time, Closed is folded with its count, and a link opens the page and the fold that hold it", async ({ page }) => {
  await page.route("**/api/notifications", manyNotifications);
  await page.goto(`/#/notifications&token=${TOKEN}`);
  const open = page.locator('main [data-block="open"]');
  await expect(open.locator("[data-notification]")).toHaveCount(PAGE_SIZE.open);
  await expect(open.getByRole("group", { name: "Pages of open notifications" })).toContainText("Showing 1–10 of 14");

  // Folded: the header shows the count, the rows are hidden until it opens.
  const closed = page.getByRole("region", { name: "Closed (3)" });
  await expect(closed.getByRole("heading", { name: "Closed (3)" })).toBeVisible();
  await expect(closed.locator("[data-notification]").first()).toBeHidden();
  await closed.getByText("Closed (3)").click();
  await expect(closed.locator("[data-notification]")).toHaveCount(3);
  await expect(closed.locator("[data-notification]").first()).toBeVisible();

  // A pop-up's link to a row on page 2 opens page 2; a link to a closed one opens the fold.
  await page.goto(`/#/notifications/synthetic-13`);
  await expect(open.locator('[data-notification="synthetic-13"]')).toHaveAttribute("aria-current", "true");
  await expect(open.getByRole("group", { name: "Pages of open notifications" })).toContainText("Showing 11–14 of 14");
  await page.goto(`/#/notifications/synthetic-16`);
  await expect(page.locator('[data-notification="synthetic-16"]')).toBeVisible();
  await expect(page.locator('[data-notification="synthetic-16"]')).toHaveAttribute("aria-current", "true");
});

test("Today: Housekeeping starts folded and shows its count; Week keeps its day count in the URL", async ({ page }) => {
  await page.goto(`/#/today&token=${TOKEN}`);
  const housekeeping = page.getByRole("heading", { name: /^Housekeeping \(\d+\)$/ });
  await expect(housekeeping).toBeVisible();
  expect(await housekeeping.evaluate((h) => (h.closest("details") as HTMLDetailsElement).open)).toBe(false);

  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Week", exact: true }).click();
  await page.getByLabel("Days to show").selectOption("14");
  await expect(page.getByRole("article")).toHaveCount(14);
  expect(hash(page)).toBe("#/week?days=14");
  await page.reload();
  await expect(page.getByLabel("Days to show")).toHaveValue("14");
  await expect(page.getByRole("article")).toHaveCount(14);
});

// Every other list of live data, view by view: each is answered longer than its page (page.route over the seeded
// answer), shows one page and "Showing 1–N of M", and Next shows the rest and puts the page in the hash. A list that
// renders without `Paged` shows no pager and every row, so its view's test fails.
// `fold`: the header of the folded block that holds the list, opened first.
interface LongList { block: string; noun: string; key: string; size: number; text: string; fold?: string }
const longList = (block: string, key: string, size: number, noun: string, text: string, fold?: string): LongList => ({ block, key, size, noun, text, fold });
const more = 3;  // rows past the first page
const n = (size: number) => size + more;
const synthetic = <T>(size: number, row: (i: number) => T): T[] => Array.from({ length: n(size) }, (_, i) => row(i + 1));
const at = (i: number) => `${NOW.slice(0, 10)}T08:${String(59 - i).padStart(2, "0")}:00`;  // row 1 is the newest

// Answers `path` with the seeded server's answer, changed by `edit`.
const patch = <T>(page: Page, path: string, edit: (body: T) => void) => page.route(`**${path}`, async (route) => {
  const res = await route.fetch();
  const body = (await res.json()) as T;
  edit(body);
  await route.fulfill({ response: res, json: body });
});

async function expectPaged(page: Page, l: LongList) {
  const block = page.locator(`main [data-block="${l.block}"]`);
  if (l.fold) await block.getByText(l.fold, { exact: true }).click();
  const pager = block.getByRole("group", { name: `Pages of ${l.noun}` });
  const rows = block.getByText(new RegExp(`${l.text} \\d+`));
  await expect(pager, `${l.key}: the pager`).toContainText(`Showing 1–${l.size} of ${n(l.size)}`);
  await expect(rows, `${l.key}: one page of rows`).toHaveCount(l.size);
  await expect(rows.first()).toHaveText(new RegExp(`${l.text} 1(?!\\d)`));
  await expect(pager.getByRole("button", { name: "Previous" })).toBeDisabled();
  await pager.getByRole("button", { name: "Next" }).click();
  await expect(pager).toContainText(`Showing ${l.size + 1}–${n(l.size)} of ${n(l.size)}`);
  await expect(rows, `${l.key}: the rest on page 2`).toHaveCount(more);
  await expect(rows.first()).toHaveText(new RegExp(`${l.text} ${l.size + 1}(?!\\d)`));
  expect(new URLSearchParams(hash(page).split("?")[1] ?? "").get(l.key), `${l.key}: the page in the hash`).toBe("2");
}

const promise = (i: number): PromiseRow => ({ name: `promise-${String(i).padStart(8, "0")}`, title: `Synthetic promise ${i}`, to: "", due: "", overdue: false, made_at: at(i), url: "", parent: "", next: "research", steps: {}, closed: "" });
const stale = (i: number): StaleSource => ({ project: "billing", kind: `Synthetic stale ${i}`, ref: `ref-${i}`, age_days: null, limit: 7 });
const ask = (i: number): FeedAsk => ({ id: `ask-${i}`, source: "slack", kind: "dm", at: at(i), title: `Synthetic ask ${i}`, url: "", waiting_minutes: 60, work_minutes: 60, overdue: false });
const question = (i: number): InboxItem => ({ id: `q-${i}`, type: "question", kind: "context", text: `Synthetic question ${i}`, status: "open" });
const result = (i: number): ResultRow => ({ id: `r-${i}`, at: at(i), kind: "note", title: `Synthetic result ${i}`, source: "cli" });
const action = (i: number): StandupAction => ({ title: `Synthetic action ${i}`, at: at(i), when: "today", kinds: ["commit"], nouns: ["commit"], repo: "", url: "", keys: [], via: "" });
const standupRow = (i: number): StandupRow => ({ title: `Synthetic standup row ${i}`, status: "on track", owner: "", mine: true, keys: [], url: "", talking_point: "a synthetic row", actions: [] });
const tierRow = (i: number): TierRow => ({ company: `Company ${i}`, title: `Synthetic suggestion ${i}`, want: 50 - i, p_offer: 0.1, ev: 1, url: "", posting_id: `sug-${i}` });
const boardJob = (i: number): BoardJob => ({ id: `b-${i}`, company: `Company ${i}`, role: `Synthetic board job ${i}`, stage: null, want: 50 - i, salary: null, location: null, remote: null, url: null, reason_no: null, last_their_contact: null, created: null });
const posting = (i: number): Posting => ({ id: `p-${i}`, company: `Company ${i}`, title: `Synthetic posting ${i}`, location: "", region: "remote", want: 50 - i, rating: null, url: "", source: "synthetic", has_loop: false });
const task = (i: number): Task => ({ id: `t${i}`, title: `Synthetic task ${i}`, impact: 3, urgency: 3, estimate_h: 1, competencies: [], due: null, score: 50 - i, promo_bonus: 0, project: null });
const project = (i: number): ProjectRow => ({ name: `Synthetic project ${i}`, title: "", group: "", parent: "", links: [], children: 0, depth: 0, sources: 1, stale: 0 });
const evidence = (i: number) => ({ id: `e-${i}`, kind: "note", title: `Synthetic evidence ${i}`, at: at(i), tags: [] });
const run = (i: number): RunSummary => ({ id: `synthetic-${i}`, run: "checkup", title: `Synthetic run ${i}`, at: at(i), status: "ok", personal: true, counts: {}, commands: [], done: [], blockers: [], next: [] });

const PAGED_VIEWS: { view: string; label: string; setup: (page: Page) => Promise<unknown>; lists: LongList[] }[] = [
  {
    view: "dashboard", label: "Dashboard",
    setup: (page) => Promise.all([
      patch<FocusBlock>(page, "/api/focus/stale", (b) => { b.stale = synthetic(PAGE_SIZE["stale-sources"], stale); }),
      patch<Promises>(page, "/api/promises", (b) => { b.promises = synthetic(PAGE_SIZE.commitments, promise); }),
    ]),
    lists: [
      longList("commitments", "commitments", PAGE_SIZE.commitments, "commitments", "Synthetic promise"),
      longList("stale-sources", "stale-sources", PAGE_SIZE["stale-sources"], "stale sources", "Synthetic stale"),
    ],
  },
  {
    view: "today", label: "Today",
    setup: (page) => Promise.all([
      patch<FeedAsks>(page, "/api/feed/asks", (b) => { b.asks = synthetic(PAGE_SIZE["open-asks"], ask); b.total = b.asks.length; }),
      patch<{ inbox: InboxItem[] }>(page, "/api/inbox", (b) => { b.inbox = synthetic(PAGE_SIZE.questions, question); }),
      patch<Today>(page, "/api/today", (b) => { b.results = synthetic(PAGE_SIZE.results, result); }),
      patch<DayLogView>(page, "/api/daylog", (b) => {
        b.log.done = { "Synthetic work": synthetic(PAGE_SIZE.done, (i) => `Synthetic done ${i}`) };
        b.log.blockers = synthetic(PAGE_SIZE.blockers, (i) => `Synthetic blocker ${i}`);
        b.log.risks = synthetic(PAGE_SIZE.risks, (i) => `Synthetic risk ${i}`);
        b.log.housekeeping = synthetic(PAGE_SIZE.housekeeping, (i) => `Synthetic housekeeping ${i}`);
      }),
    ]),
    lists: [
      longList("open-asks", "open-asks", PAGE_SIZE["open-asks"], "open asks", "Synthetic ask"),
      longList("questions", "questions", PAGE_SIZE.questions, "questions", "Synthetic question"),
      longList("plan", "results", PAGE_SIZE.results, "results", "Synthetic result"),
      longList("day-log", "done-synthetic-work", PAGE_SIZE.done, "done rows in Synthetic work", "Synthetic done"),
      longList("day-log", "blockers", PAGE_SIZE.blockers, "blockers", "Synthetic blocker"),
      longList("day-log", "risks", PAGE_SIZE.risks, "risks", "Synthetic risk"),
      longList("day-log", "housekeeping", PAGE_SIZE.housekeeping, "housekeeping", "Synthetic housekeeping", `Housekeeping (${n(PAGE_SIZE.housekeeping)})`),
    ],
  },
  {
    view: "standup", label: "Standup",
    setup: (page) => patch<Standup>(page, "/api/standup", (b) => {
      b.rows = synthetic(PAGE_SIZE.rows, standupRow);
      b.has_rows = true;
      b.groups = [{ label: "Synthetic group", never_rows: true, talking_point: "synthetic evidence", actions: synthetic(PAGE_SIZE.group, action) }];
    }),
    lists: [
      longList("rows", "rows", PAGE_SIZE.rows, "rows you report on", "Synthetic standup row"),
      longList("groups", "group-synthetic-group", PAGE_SIZE.group, "Synthetic group evidence", "Synthetic action"),
    ],
  },
  {
    view: "notifications", label: "Notifications",
    setup: (page) => patch<{ items: unknown[] }>(page, "/api/notifications", (b) => {
      b.items = [
        ...synthetic(PAGE_SIZE.open, (i) => notification(`open-${i}`, `Synthetic open ${i}`, "new", at(i))),
        ...synthetic(PAGE_SIZE.closed, (i) => notification(`closed-${i}`, `Synthetic closed ${i}`, "dismissed", at(i))),
      ];
    }),
    lists: [
      longList("open", "open", PAGE_SIZE.open, "open notifications", "Synthetic open"),
      longList("closed", "closed", PAGE_SIZE.closed, "closed notifications", "Synthetic closed", `Closed (${n(PAGE_SIZE.closed)})`),
    ],
  },
  {
    view: "jobs", label: "Job hunt",
    // 3 dream, 5 target, and 5 practice suggestions: the list shows dream first, then target, then practice.
    setup: (page) => patch<Jobs>(page, "/api/jobs", (b) => {
      const s = synthetic(PAGE_SIZE["job-suggestions"], tierRow);
      b.strategy.suggested = { dream: s.slice(0, 3), target: s.slice(3, 8), practice: s.slice(8) };
      b.board = synthetic(PAGE_SIZE.board, boardJob);
      b.postings = synthetic(PAGE_SIZE.postings, posting);
    }),
    lists: [
      longList("job-suggestions", "job-suggestions", PAGE_SIZE["job-suggestions"], "job suggestions", "Synthetic suggestion"),
      longList("board", "board", PAGE_SIZE.board, "jobs on the board", "Synthetic board job"),
      longList("postings", "postings", PAGE_SIZE.postings, "postings", "Synthetic posting"),
    ],
  },
  {
    view: "work", label: "Work",
    setup: (page) => patch<Work>(page, "/api/work", (b) => {
      b.tasks = synthetic(PAGE_SIZE.tasks, task);
      b.projects = synthetic(PAGE_SIZE.projects, project);
      b.evidence = synthetic(PAGE_SIZE.evidence, evidence);
    }),
    lists: [
      longList("tasks", "tasks", PAGE_SIZE.tasks, "tasks", "Synthetic task"),
      longList("projects", "projects", PAGE_SIZE.projects, "projects", "Synthetic project"),
      longList("evidence", "evidence", PAGE_SIZE.evidence, "evidence", "Synthetic evidence"),
    ],
  },
  {
    view: "reports", label: "Reports",
    setup: (page) => Promise.all([
      page.route("**/api/reports", (route) => route.fulfill({ json: { reports: synthetic(PAGE_SIZE.runs, run) } })),
      page.route("**/api/reports/synthetic-*", (route) => route.fulfill({ json: { summary: run(1), html: "<p>A synthetic run page</p>" } })),
    ]),
    lists: [longList("runs", "runs", PAGE_SIZE.runs, "run pages", "Synthetic run")],
  },
];

for (const v of PAGED_VIEWS) {
  test(`${v.label}: each list longer than its page shows one page, Showing 1–N of M, and Next`, async ({ page }) => {
    await v.setup(page);
    await page.goto(`/#/${v.view}&token=${TOKEN}`);
    await expect(page.getByRole("heading", { level: 1, name: v.label })).toBeVisible();
    for (const l of v.lists) await expectPaged(page, l);
    if (v.view === "jobs") {
      // The suggestions filter by tier, with counts; a tier starts at page 1 and stays in the hash.
      const block = page.locator('main [data-block="job-suggestions"]');
      const filter = block.getByRole("group", { name: "Filter job suggestions" });
      await expect(filter.getByRole("button")).toHaveText(["All (13)", "practice (5)", "target (5)", "dream (3)"]);
      await filter.getByRole("button", { name: "dream (3)" }).click();
      await expect(block.getByText(/Synthetic suggestion \d+/)).toHaveText(["Synthetic suggestion 1", "Synthetic suggestion 2", "Synthetic suggestion 3"]);
      await expect(block.getByRole("group", { name: "Pages of job suggestions" })).toHaveCount(0);
      expect(new URLSearchParams(hash(page).split("?")[1]).get("job-suggestions-filter")).toBe("dream");
    }
  });
}
