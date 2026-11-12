import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";

// Issue #142, in every view: each block shows a visible loading label at once, fails on its own with Retry and
// the fix (the page never goes blank), shows its last data at once after a reload while it refreshes, and keeps
// a gap from the next card. The view's reads are held or failed here; the server never sees a failed one.

const FAILURE = "synthetic failure for the blocks test";
const VIEWS = [
  { id: "dashboard", name: "Dashboard", ready: /Why:/ },
  { id: "teach", name: "Teach me", ready: "Help the teacher improve" },
  { id: "today", name: "Today", ready: "Results of the day" },
  { id: "standup", name: "Standup", ready: "Order export" },
  { id: "notifications", name: "Notifications", ready: /PagerDuty: a page is assigned to you/ },
  { id: "goals", name: "Goals", ready: /The nearest goal still ahead/ },
  { id: "week", name: "Week", ready: /Mon/ },
  { id: "jobs", name: "Job hunt", ready: /Postings that pass your criteria/ },
  { id: "interviews", name: "Interviews", ready: /Typical order/ },
  { id: "practice", name: "Practice", ready: /Due now/ },
  { id: "library", name: "Library", ready: /How to/ },
  { id: "work", name: "Work", ready: /What is missing/ },
  { id: "review", name: "Review", ready: /estimate_ratio/ },
  { id: "reports", name: "Reports", ready: / done, / },
  { id: "actions", name: "Actions", ready: /config\.set/ },
  { id: "setup", name: "Setup", ready: /Day watcher/ },
];
// Reads the shell needs (the machine role, the page's cache key) always answer.
const NEVER_HELD = /\/api\/(overview|tab-key|ping)$/;

// Holds every view read (or only the read of `only`) until release(): then it fails them (500) or lets them
// through. Reads made after a release go through. /api/changes answers one version until bump(), so no live
// refetch races the checks.
async function holdReads(page: Page, only?: string) {
  let held = false;
  let outcome: "fail" | "pass" = "pass";
  let open = () => {};
  let gate = Promise.resolve();
  let version = "blocks-test";
  await page.route("**/api/**", async (route) => {
    const req = route.request();
    const path = new URL(req.url()).pathname;
    if (path === "/api/changes") return route.fulfill({ contentType: "application/json", body: JSON.stringify({ version }) });
    if (req.method() !== "GET" || NEVER_HELD.test(path) || !held || (only && path !== only)) return route.fallback();
    await gate;
    if (outcome === "fail") return route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: FAILURE }) });
    return route.fallback();
  });
  return {
    hold() {
      held = true;
      gate = new Promise<void>((r) => (open = r));
    },
    release(how: "fail" | "pass") {
      outcome = how;
      held = false;
      open();
    },
    // The server's data moved: the next /api/changes answers a new version.
    bump() {
      version += "+";
    },
  };
}

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

// Gaps between neighbouring cards in `main`, in px: below one another, and side by side.
function cardGaps(page: Page): Promise<number[]> {
  return page.evaluate(() => {
    const cards = [...document.querySelectorAll("main .card")].filter((c) => (c as HTMLElement).offsetParent !== null);
    const boxes = cards.map((c) => c.getBoundingClientRect());
    const gaps: number[] = [];
    cards.forEach((a, i) => {
      let below = Infinity;
      let right = Infinity;
      cards.forEach((b, j) => {
        if (i === j || a.contains(b) || b.contains(a)) return;
        const [ra, rb] = [boxes[i], boxes[j]];
        const xOverlap = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left) > 1;
        const yOverlap = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top) > 1;
        if (xOverlap && rb.top > ra.top + 1) below = Math.min(below, rb.top - ra.bottom);
        if (yOverlap && rb.left > ra.left + 1) right = Math.min(right, rb.left - ra.right);
      });
      for (const g of [below, right]) if (g !== Infinity) gaps.push(Math.round(g));
    });
    return gaps;
  });
}

for (const v of VIEWS) {
  test(`${v.name}: loading label at once, an error with Retry, the last data on a reload, gaps between cards`, async ({ page }) => {
    const reads = await holdReads(page);
    const start = v.id === "goals" ? "review" : "goals";  // another view first: this one is not in the page's memory yet
    await page.goto(`/#/${start}&token=${TOKEN}`);
    await expect(page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: v.name, exact: true })).toBeVisible();
    await expect(page.locator("main .skeleton")).toHaveCount(0);

    // 1. Loading: a visible "Loading …" label within 100 ms of opening the view.
    reads.hold();
    await page.evaluate(() => {
      const w = window as unknown as { __loadingAt: number };
      w.__loadingAt = 0;
      new MutationObserver((_, obs) => {
        if ([...document.querySelectorAll("main .skeleton-label")].some((e) => /^Loading .+…/.test(e.textContent ?? ""))) {
          w.__loadingAt = performance.now();
          obs.disconnect();
        }
      }).observe(document.querySelector("main")!, { childList: true, subtree: true, characterData: true });
    });
    const opened = await page.evaluate((id) => {
      const t = performance.now();
      document.querySelector<HTMLAnchorElement>(`nav a[href="#/${id}"]`)!.click();
      return t;
    }, v.id);
    await expect(page.locator("main .skeleton-label").first()).toBeVisible();
    await expect(page.locator("main .skeleton-label").first()).toHaveText(/^Loading .+…/);
    const loadingAt = await page.evaluate(() => (window as unknown as { __loadingAt: number }).__loadingAt);
    expect(loadingAt - opened, "ms from opening the view to a visible loading label").toBeLessThan(100);

    // 2. Errors: each block that failed says why, with the fix and Retry; the page keeps its title.
    reads.release("fail");
    const alert = page.getByRole("alert").filter({ hasText: FAILURE }).first();
    await expect(alert).toBeVisible();
    await expect(alert).toContainText("Fix:");
    await expect(alert.getByRole("button", { name: "Retry" })).toBeVisible();
    await expect(page.getByRole("heading", { level: 1, name: v.name })).toBeVisible();
    // Every Retry in each try: with one click per try, 7 failed blocks needed 7 tries and a busy machine ran out of time.
    const retry = page.getByRole("button", { name: "Retry" });
    let missed = 0;
    try {
      await expect(async () => {
        // first() each time: `all()` gives index locators, and each click shifts the rest, so later ones miss.
        for (let i = 0; i < 20 && (await retry.count()); i++) await retry.first().click({ timeout: 2_000 }).catch(() => missed++);
        await expect(retry).toHaveCount(0, { timeout: 2_000 });
      }).toPass({ timeout: 30_000 });
    } catch (e) {
      const left = await page.getByRole("alert").filter({ has: retry }).allTextContents();
      throw new Error(`${left.length} block(s) still show Retry after 30 s (${missed} click(s) missed): ${left.join(" | ")}`, { cause: e });
    }
    await expect(page.getByText(v.ready).first()).toBeVisible();
    await expect(page.getByRole("alert").filter({ hasText: FAILURE })).toHaveCount(0);
    await expect(page.locator("main .skeleton")).toHaveCount(0);

    // 3. Spacing: no two cards touch.
    const gaps = await cardGaps(page);
    if (v.id === "dashboard") expect(gaps.length, "the Dashboard stacks cards").toBeGreaterThan(0);
    for (const g of gaps) expect(g, `gap between cards in ${v.name}: ${gaps.join(", ")}`).toBeGreaterThanOrEqual(8);

    // 4. Cache: a reload shows the last data at once, before any held read answers, then refreshes.
    reads.hold();
    await page.reload();
    await expect(page.getByText(v.ready).first()).toBeVisible();
    await expect(page.locator("main .updated").first()).toHaveText(/^Updated \d{2}:\d{2} · refreshing…$/);
    reads.release("pass");
    await expect(page.locator("main .updated")).toHaveCount(0);
    await expect(page.getByText(v.ready).first()).toBeVisible();
  });
}

// Issue #142: each block loads and fails on its own. One block's read is held, then failed. While it waits,
// every other block of the view already shows its data; when it fails, only that block shows the error, and
// the others keep their data. Retry loads it.
const ONE_BLOCK = [
  {
    id: "dashboard", name: "Dashboard", read: "/api/focus/week", block: 'main [data-horizon="week"]', label: "this week", loaded: /Why:/,
    others: [
      { in: 'main [data-horizon="now"]', text: /Why:/ },
      { in: 'main section[aria-label="Tomorrow"]', text: /Proposed plan for/ },
    ],
  },
  {
    id: "today", name: "Today", read: "/api/feed/asks", block: 'main section[aria-label^="Open asks"]', label: "open asks", loaded: /Mark personal/,
    others: [
      { in: 'main section[aria-label="Next step"] .next', text: /\S/ },
      { in: 'main section[aria-label="Results of the day"]', text: /Results of the day/ },
      { in: 'main section[aria-label^="Done today"]', text: /Done today/ },
      { in: 'main section[aria-label^="careerbrew suggests"]', text: /careerbrew suggests/ },
    ],
  },
];

for (const v of ONE_BLOCK) {
  test(`${v.name}: one block waits and fails alone, the other blocks show their data`, async ({ page }) => {
    const reads = await holdReads(page, v.read);
    reads.hold();
    await page.goto(`/#/${v.id}&token=${TOKEN}`);
    const block = page.locator(v.block);
    const othersShowTheirData = async () => {
      for (const o of v.others) await expect(page.locator(o.in).filter({ hasText: o.text }).first()).toBeVisible();
    };

    // 1. Parallel: the other blocks show their data while this one still loads.
    await expect(block.locator(".skeleton-label")).toHaveText(new RegExp(`^Loading ${v.label}…`));
    await othersShowTheirData();
    await expect(page.locator("main .skeleton"), "only the held block still loads").toHaveCount(1);
    await expect(block.locator(".skeleton")).toHaveCount(1);

    // 2. Errors: only this block fails, with its fix and Retry; the others keep their data.
    reads.release("fail");
    const alert = block.getByRole("alert");
    await expect(alert).toContainText(FAILURE);
    await expect(alert).toContainText("Fix:");
    await expect(page.locator("main").getByRole("alert"), "no other block shows an error").toHaveCount(1);
    await expect(page.locator("main .skeleton")).toHaveCount(0);
    await othersShowTheirData();

    // 3. Retry loads this block alone.
    await alert.getByRole("button", { name: "Retry" }).click();
    await expect(page.locator("main").getByRole("alert")).toHaveCount(0);
    await expect(block.locator(".skeleton")).toHaveCount(0);
    await expect(block.getByText(v.loaded).first()).toBeVisible();
    await othersShowTheirData();
  });
}

// Issue #142 and US-64: when /api/changes moved while a view was closed, switching back shows its last data at
// once and says that new data is coming; the fresh data then replaces it.
test("a view switch after /api/changes moved shows the last data and says new data is coming", async ({ page }) => {
  const reads = await holdReads(page);
  const nav = page.getByRole("navigation", { name: "Views" });
  await page.goto(`/#/goals&token=${TOKEN}`);
  await expect(page.getByText(/The nearest goal still ahead/).first()).toBeVisible();
  await expect(page.locator("main .skeleton")).toHaveCount(0);
  await nav.getByRole("link", { name: "Week", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Week" })).toBeVisible();
  await expect(page.locator("main .skeleton")).toHaveCount(0);

  // The server's data moves. The page checks /api/changes every 5 s and when the tab shows: ask now, until it moved.
  reads.bump();
  await page.evaluate(() => new Promise<void>((resolve) => {
    const poke = window.setInterval(() => document.dispatchEvent(new Event("visibilitychange")), 100);
    window.addEventListener("careerbrew:changed", () => {
      window.clearInterval(poke);
      resolve();
    }, { once: true });
  }));

  reads.hold();
  await nav.getByRole("link", { name: "Goals", exact: true }).click();
  await expect(page.getByText(/The nearest goal still ahead/).first()).toBeVisible();
  await expect(page.locator("main .updated").first()).toHaveText(/^Updated \d{2}:\d{2} · new data, refreshing…$/);
  reads.release("pass");
  await expect(page.locator("main .updated")).toHaveCount(0);
  await expect(page.getByText(/The nearest goal still ahead/).first()).toBeVisible();
});
