import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { NOW, OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import { shortDay } from "../src/dates";

// Dates come from the server's pinned clock, never a literal, and a check that only holds on a workday
// asks isWorkday first, so the suite passes on any day, weekends included.
const TODAY = NOW.slice(0, 10);
function addDays(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}
function isWorkday(day: string): boolean {  // Monday to Friday: the seed keeps the default work_days
  return ![0, 6].includes(new Date(`${day}T00:00:00Z`).getUTCDay());
}
function nextWorkday(day: string): string {
  let d = addDays(day, 1);
  while (!isWorkday(d)) d = addDays(d, 1);
  return d;
}

const views = [
  { name: "Dashboard", heading: "Right now", text: /Why:/ },
  { name: "Teach me", heading: "Help the teacher improve", text: /one step at a time|learn the idea first|Due cards|follow your upcoming goal/i },
  { name: "Goals", heading: "Upcoming goals", text: /Only you set these/ },
  { name: "Today", heading: "Next step", text: `Plan for ${shortDay(TODAY)}` },
  { name: "Standup", heading: "Order export", text: /^Since \w{3} \w{3} \d{1,2}, 10:00 \(\w{3} \w{3} \d{1,2} standup\) until / },
  { name: "Notifications", heading: /^Open \(\d+\)$/, text: /PagerDuty: a page is assigned to you/ },
  { name: "Week", heading: "Plan ahead", text: /Mon/ },
  { name: "Job hunt", heading: "Season strategy", text: /Postings that pass your criteria/ },
  { name: "Interviews", heading: /Shared patterns across \d+ companies/, text: /Typical order/ },
  { name: "Practice", heading: "Progress", text: /Due now/ },
  { name: "Library", heading: /Pages \(\d+\)/, text: /How to/ },
  { name: "Work", heading: /Promotion case/, text: /What is missing/ },
  { name: "Review", heading: "Outcome metrics", text: /estimate_ratio/ },
  { name: "Reports", heading: /Run pages \(\d+\)/, text: / done, / },
  { name: "Setup", heading: "This machine", text: /Day watcher/ },
];

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  await page.goto(`/#token=${TOKEN}`);
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

test("every view opens from the navigation and shows its data", async ({ page }) => {
  for (const v of views) {
    await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: v.name, exact: true }).click();
    await expect(page.getByRole("heading", { level: 1, name: v.name })).toBeVisible();
    await expect(page.getByRole("heading", { name: v.heading })).toBeVisible();
    await expect(page.getByText(v.text).first()).toBeVisible();
    await page.screenshot({ path: `${OUT}/view-${v.name.toLowerCase().replace(/\s+/g, "-")}.png`, fullPage: true });
  }
});

test("Dashboard shows the best action and why for all 7 horizons", async ({ page }) => {
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  const cards = page.locator("[data-horizon]");
  await expect(cards).toHaveCount(7);
  const ids = await cards.evaluateAll((els) => els.map((e) => e.getAttribute("data-horizon")));
  expect(ids).toEqual(["now", "today", "week", "month", "quarter", "year", "company"]);
  for (const id of ids) {
    const why = page.locator(`[data-horizon="${id}"] .why`).first();
    await expect(why, `horizon ${id} has a why`).toHaveText(/Why: \S+/);
  }
  await expect(page.locator('[data-horizon="month"]')).toContainText("Plan the Q1 platform roadmap");
  await expect(page.locator('[data-horizon="quarter"]')).toContainText("/ effort");
  await page.screenshot({ path: `${OUT}/dashboard.png`, fullPage: true });
  const tomorrow = page.getByRole("region", { name: "Tomorrow" });
  await expect(tomorrow).toContainText(/Proposed plan for \w{3} \w{3} \d{1,2}/);
  await expect(tomorrow.locator("li").first()).toContainText(/\d{2}:\d{2}–\d{2}:\d{2}/);
  if (isWorkday(TODAY)) {
    // US-75 pictures: Tomorrow repeated today's tasks at today's times. It is Week's next workday, after today's plan.
    for (const title of ["Fix the flaky checkout test", "Write the queue design doc"]) await expect(tomorrow).not.toContainText(title);
    await expect(tomorrow).toContainText("Migrate billing jobs");
  }
  await tomorrow.getByRole("link", { name: "Today's plan" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
});

test("Today shows the next step, meetings from the calendar, and the results list", async ({ page }) => {
  await page.getByRole("link", { name: "Today", exact: true }).click();
  await expect(page.getByText(/Work on it now|this task fits|Meeting in|in a meeting/)).toBeVisible();
  // The fixture's standup repeats Monday to Friday (tests/fixtures/calendar/work.ics), so a weekend day has no meeting.
  const plan = page.getByRole("region", { name: `Plan for ${shortDay(TODAY)}`, exact: true });
  await expect(plan).toBeVisible();
  if (isWorkday(TODAY)) {
    await expect(plan).toContainText("Team standup");
  } else {
    await expect(plan).not.toContainText("Team standup");
    await expect(page.getByRole("group", { name: "Meetings" })).toHaveText(/^Meetings\s*0$/);
  }
  await expect(page.getByRole("region", { name: "Results of the day" })).toContainText("shipped order export");
});

test("Week plans ahead: a new task lands in the schedule", async ({ page }) => {
  await page.getByRole("link", { name: "Week", exact: true }).click();
  await expect(page.getByRole("article").first()).toBeVisible();
  await page.getByLabel("New task").fill("Prepare the Q4 roadmap review");
  await page.getByLabel("Hours").fill("1");
  await page.getByRole("button", { name: "Add task" }).click();
  await expect(page.getByText("Prepare the Q4 roadmap review").first()).toBeVisible();
  await page.getByLabel("Days to show").selectOption("14");
  await expect(page.getByRole("article")).toHaveCount(14);
  // US-75 pictures: a 12h task is too big for any one day; the days still have room, so it is not "No room".
  await expect(page.getByText(/^Too big for one day: Plan the Q1 platform roadmap \(12h\)\. A day here has at most \d+h( \d+m)? of focus time: split it/)).toBeVisible();
  await expect(page.getByText(/^No room in these 14 days/)).toHaveCount(0);
});

test("Interviews: a company page shows its loop, AI policy, tips, and sources", async ({ page }) => {
  await page.getByRole("link", { name: "Interviews", exact: true }).click();
  await page.getByRole("link", { name: "Datadog", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Interview loop: Datadog" })).toBeVisible();
  await expect(page.getByText(/AI tools:/)).toBeVisible();
  await page.getByText("Tips and traps").first().click();
  await expect(page.getByText(/^Tip:/).first()).toBeVisible();
  await page.getByText(/^Sources \(\d+\)/).click();
  await expect(page.getByRole("link", { name: /Datadog/ }).first()).toBeVisible();
});

test("Interviews: any company opens a derived loop with shared AI guidance", async ({ page }) => {
  await page.getByRole("link", { name: "Interviews", exact: true }).click();
  await page.getByLabel("Any company").fill("data");
  await expect(page.getByRole("link", { name: "Datadog", exact: true })).toBeVisible();
  await expect(page.getByRole("link", { name: "Stripe", exact: true })).toHaveCount(0);
  await page.getByLabel("Any company").fill("Zzunknownco");
  await expect(page.getByText(/No sourced loop for “Zzunknownco”/)).toBeVisible();
  await page.getByRole("button", { name: "Open loop" }).click();
  await expect(page.getByRole("heading", { name: "Interview loop: Zzunknownco" })).toBeVisible();
  await expect(page.getByText(/No sourced loop yet/)).toBeVisible();
  await expect(page.getByText(/no sourced rule for Zzunknownco/)).toBeVisible();
  await expect(page.getByText(/^Sources \(\d+\)/)).toBeVisible();
});

test("Practice: recall, reveal, grade with keys, and the next card appears", async ({ page }) => {
  await page.getByRole("link", { name: "Practice", exact: true }).click();
  const first = await page.locator(".flashcard .question").innerText();
  await expect(page.getByText("Answer from memory first")).toBeVisible();
  await expect(page.locator(".flashcard .answer")).toHaveCount(0);
  await page.keyboard.press("Space");
  await expect(page.locator(".flashcard .answer")).toBeVisible();
  await page.keyboard.press("3");
  await page.keyboard.press("3"); // a double press must not grade twice
  await expect(page.getByRole("status")).toContainText("Next review");
  await expect(page.locator(".flashcard .question")).not.toHaveText(first);
  await page.getByRole("button", { name: "Show answer (Space)" }).click();
  await page.getByRole("button", { name: "1 Again" }).click();
  await expect(page.getByRole("group", { name: "Reviewed today" })).toContainText("2");
});

test("Library: pages are grouped, filterable, and open", async ({ page }) => {
  await page.getByRole("link", { name: "Library", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Job search plan" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Interview stories" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Job notes" })).toBeVisible();
  await expect(page.getByRole("heading", { name: /^Interview knowledge · Computer science/ })).toBeVisible();
  await page.getByLabel("Filter pages").fill("binary search");
  await page.getByRole("link", { name: "How does binary search work?" }).click();
  await expect(page.locator(".reader .page")).toContainText("Short answer.");
  await expect(page.locator(".reader .page pre code")).toContainText("def binary_search");
  await page.getByText("Report a problem with this explanation").click();
  await page.getByLabel("What is wrong").fill("the loop bound is unclear");
  await page.getByRole("button", { name: "Report", exact: true }).click();
  await expect(page.getByRole("status")).toContainText("rewritten in the next card-explainer run");
  await page.getByLabel("Filter pages").fill("system design");
  await page.getByRole("link", { name: "How to pass a system design interview?" }).click();
  await expect(page.getByRole("heading", { name: "How to pass a system design interview?" })).toBeVisible();
  await expect(page.locator(".reader .page")).toContainText("Start with requirements");
});

test("corporate machine: the Job search group never shows and its endpoints are never called (issue #65)", async ({ page }) => {
  // The seeded server is personal. The page plays a corporate machine: /api/overview says so (held open to
  // check the sidebar while the role loads), and job-hunt endpoints answer 403 like the real server does.
  let release = () => {};
  const gate = new Promise<void>((r) => (release = r));
  await page.route("**/api/overview", async (route) => {
    const res = await route.fetch();
    const body = { ...(await res.json()), role: "corporate" };
    await gate;
    await route.fulfill({ response: res, json: body });
  });
  const jobCalls: string[] = [];
  await page.route(/\/api\/(jobs|interviews|cards|library|explanations|exercises|drill)(\/|$|\?)/, (route) => {
    jobCalls.push(route.request().url());
    return route.fulfill({ status: 403, json: { error: "job-hunt views are personal-only; this is a corporate machine" } });
  });
  await page.reload();
  const nav = page.getByRole("navigation", { name: "Views" });
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  await expect(nav.getByRole("link", { name: "Job hunt", exact: true })).toHaveCount(0);  // role still loading
  release();
  await expect(nav.getByRole("link", { name: "Today", exact: true })).toBeVisible();
  await expect(nav.locator(".nav-label")).toHaveText(["Plan", "System"]);
  for (const name of ["Job hunt", "Interviews", "Practice", "Library"]) await expect(nav.getByRole("link", { name, exact: true })).toHaveCount(0);
  await page.screenshot({ path: `${OUT}/corporate-sidebar.png`, fullPage: true });
  for (const [id, label] of [["jobs", "Job hunt"], ["interviews", "Interviews"], ["practice", "Practice"], ["library", "Library"]]) {
    await page.evaluate((h) => (window.location.hash = h), `#/${id}`);
    await expect(page.getByRole("heading", { level: 1, name: label })).toBeVisible();
    await expect(page.getByRole("status").filter({ hasText: "job-hunt views are personal-only" })).toBeVisible();
  }
  expect(jobCalls).toEqual([]);
});

test("sidebar groups: Work under Plan, Review under System, and Actions says what it does (issue #66)", async ({ page }) => {
  const nav = page.getByRole("navigation", { name: "Views" });
  await expect(nav.locator(".nav-label")).toHaveText(["Plan", "Job search", "System"]);
  const links = (group: string) => nav.locator(".nav-group").filter({ has: page.locator(".nav-label", { hasText: group }) }).getByRole("link");
  await expect(links("Plan")).toHaveText(["Dashboard", "Teach me", "Today", "Standup", "Notifications", "Goals", "Week", "Work"]);
  await expect(links("Job search")).toHaveText(["Job hunt", "Interviews", "Practice", "Library"]);
  await expect(links("System")).toHaveText(["Review", "Reports", "Actions", "Setup"]);
  await page.screenshot({ path: `${OUT}/sidebar-groups.png` });
  await nav.getByRole("link", { name: "Actions", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Actions" })).toBeVisible();
  await expect(page.locator(".page-head .muted")).toHaveText(/^Run any careerbrew command from the browser/);
});

test("Work shows the projects imported from a sheet as a tree, with parents and links (issue #69)", async ({ page }) => {
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Work", exact: true }).click();
  const projects = page.getByRole("region", { name: "Projects" });
  const rows = projects.locator("li");
  await expect(rows).toHaveCount(6);
  expect(await rows.evaluateAll((els) => els.map((e) => `${e.querySelector("strong")?.textContent}:${e.getAttribute("data-depth")}`))).toEqual(["payments:0", "billing:1", "invoices:2", "ledger:1", "checkout:0", "checkout-analytics:1"]);
  const billing = rows.filter({ hasText: "Billing revamp" });
  await expect(billing).toContainText("q4-cutline");
  await expect(billing).toContainText("part of payments · linked to ledger, invoices");
  await expect(billing).toContainText("2 sources · 2 stale");
  await page.screenshot({ path: `${OUT}/work-projects.png`, fullPage: true });
  await projects.getByRole("link", { name: "Import a sheet" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Actions" })).toBeVisible();
  await expect(page.getByText(/adds or updates many project records at once/)).toBeVisible();
});

test("Work: the watch list shows one section per active project, 5 per page (issue #172)", async ({ page }) => {
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Work", exact: true }).click();
  const watch = page.getByRole("region", { name: "Watch list", exact: true });
  const sections = watch.locator("section.watched-project");
  await expect(sections).toHaveCount(5);
  await expect(sections.first()).toHaveAttribute("aria-label", "billing");
  await expect(sections.first()).toContainText("Latest change (team");
  await watch.getByRole("button", { name: "Next" }).click();
  await expect(sections).toHaveCount(1);
  await expect(sections.first()).toHaveAttribute("aria-label", "payments");
  await watch.getByRole("button", { name: "Previous" }).click();
  await expect(sections).toHaveCount(5);
});

test("Reports: the newest run page shows Done, Blockers, and Next in a sandboxed frame (run pages)", async ({ page }) => {
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Reports", exact: true }).click();
  const list = page.getByRole("region", { name: /Run pages \(\d+\)/ });
  await expect(list.getByRole("link").first()).toHaveAttribute("aria-current", "page");
  const frame = page.locator("iframe.report-frame");
  await expect(frame).toHaveAttribute("sandbox", "");
  const run = page.frameLocator("iframe.report-frame");
  for (const name of [/^Done/, /^Blockers/, /^Next/]) await expect(run.getByRole("heading", { level: 2, name })).toBeVisible();
  await expect(run.locator("ol.items li").first()).toContainText("Run:");
  await page.screenshot({ path: `${OUT}/reports.png`, fullPage: true });
  // A direct link to an older page opens that page.
  const older = list.getByRole("link").nth(1);
  const title = await older.textContent();
  await older.click();
  await expect(older).toHaveAttribute("aria-current", "page");
  await expect(run.getByRole("heading", { level: 1 })).toHaveText(title!);
});

test("Job hunt shows the Notion jobs board with stages", async ({ page }) => {
  await page.getByRole("link", { name: "Job hunt", exact: true }).click();
  const board = page.getByRole("region", { name: /Jobs board from Notion/ });
  await expect(board).toBeVisible();
  await expect(board.getByText("No: Onsite")).toBeVisible();
});

// Issue #67: nothing scrolls sideways at phone, tablet, or desktop width: not the page, and not any part of it
// (a table, the sidebar, a long URL, a code block). Every <details> is opened first, so folded rows count too.
async function sideways(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    for (const d of document.querySelectorAll("details")) d.open = true;
    const what = (el: Element) => `${el.tagName.toLowerCase()}${typeof el.className === "string" && el.className ? `.${el.className.trim().split(/\s+/).join(".")}` : ""}`;
    const root = document.documentElement;
    const out = root.scrollWidth > root.clientWidth ? [`the page (${root.scrollWidth} > ${root.clientWidth} px)`] : [];
    for (const el of document.querySelectorAll("body *")) {
      const x = getComputedStyle(el).overflowX;
      if ((x === "auto" || x === "scroll") && el.scrollWidth > el.clientWidth + 1) out.push(`${what(el)} (${el.scrollWidth} > ${el.clientWidth} px)`);
    }
    return out;
  });
}

// On a phone (760 px or less) the views sit behind the Menu button; on a wider screen the sidebar shows them.
async function navTo(page: Page, name: string) {
  if (page.viewportSize()!.width <= 760) await page.getByRole("button", { name: "Menu" }).click();
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name, exact: true }).click();
}

const fitPages: { name: string; open: (page: Page) => Promise<void> }[] = [
  ...[...views, { name: "Actions", heading: /Actions \(\d{3}\)/ }].map((v) => ({
    name: v.name,
    open: async (page: Page) => {
      await navTo(page, v.name);
      await expect(page.getByRole("heading", { name: v.heading })).toBeVisible();
    },
  })),
  { name: "Actions: a read-only action's output", open: async (page) => {
    await page.getByLabel("Filter actions").fill("config.show");
    await page.getByRole("button", { name: "config.show", exact: true }).click();
    await page.getByRole("button", { name: "Run" }).click();
    await expect(page.getByLabel("Result")).toContainText("exit 0");
  } },
  { name: "Interviews: a company page", open: async (page) => {
    await page.evaluate(() => (window.location.hash = "#/interviews/Datadog"));
    await expect(page.getByRole("heading", { name: "Interview loop: Datadog" })).toBeVisible();
  } },
  { name: "Library: a page with code", open: async (page) => {
    await navTo(page, "Library");
    await page.getByLabel("Filter pages").fill("binary search");
    await page.getByRole("link", { name: "How does binary search work?" }).click();
    await expect(page.locator(".reader .page pre code")).toContainText("def binary_search");
  } },
];

for (const [width, height] of [[375, 812], [768, 1024], [1280, 800]]) {
  test(`at ${width} px every view fits its width without sideways scrolling (issue #67)`, async ({ page }) => {
    await page.setViewportSize({ width, height });
    const found: string[] = [];
    for (const p of fitPages) {
      await p.open(page);
      await expect(page.locator(".skeleton")).toHaveCount(0);  // every part of the view has loaded
      for (const s of await sideways(page)) found.push(`${p.name}: ${s}`);
    }
    await page.screenshot({ path: `${OUT}/fit-${width}.png`, fullPage: true });
    expect(found, `parts that scroll sideways at ${width} px`).toEqual([]);
  });
}

test("Teach me: confirm a lesson, then solve an exercise with tests, time, and a graded result", async ({ page }) => {
  await page.getByRole("link", { name: "Teach me", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Set a goal first" })).toBeVisible();  // #54: no lesson without a goal
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Goals", exact: true }).click();  // the callout links Goals too
  await page.getByLabel("Goal title").fill("Practice");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText("Practice");
  await page.getByRole("link", { name: "Teach me", exact: true }).click();
  await expect(page.getByRole("heading", { name: /^Lesson: / })).toBeVisible();
  await page.getByLabel("Explain it back").fill("Use a hash set to check if I have seen a value in O(1).");
  await page.getByRole("button", { name: "I understand it" }).click();
  await expect(page.getByRole("status")).toContainText("Confirmed");
  await expect(page.getByLabel("Your solution")).toBeVisible();
  await expect(page.getByLabel("Time on this step")).toBeVisible();
  await page.getByLabel("Your solution").fill("def contains_duplicate(nums):\n    return nums[0] in nums[1:]\n");
  await page.getByRole("button", { name: "Run and submit" }).click();
  await expect(page.getByRole("status")).toContainText("hint is unlocked");
  await expect(page.getByText("Hints unlocked")).toBeVisible();
  await expect(page.getByLabel("Your solution")).toHaveValue(/nums\[0\] in nums\[1:\]/); // your code survives a failed attempt
  await expect(page.getByRole("group", { name: "Steps" })).not.toContainText(/^Steps0$/);
  await page.getByLabel("Your solution").fill("def contains_duplicate(nums):\n    seen = set()\n    for x in nums:\n        if x in seen:\n            return True\n        seen.add(x)\n    return False\n");
  await page.getByRole("button", { name: "Run and submit" }).click();
  await expect(page.getByRole("status")).toContainText("All tests pass");
  await expect(page.getByRole("group", { name: "Tests" })).toContainText("5/5");
});

// Tests share one seeded server; a test that needs an empty goal queue clears it first.
async function clearGoals(page: import("@playwright/test").Page) {
  const res = await page.request.post("/api/goals", { headers: { "X-Careerbrew-Token": TOKEN }, data: { queue: [] } });
  expect(res.ok()).toBe(true);
}

test("Teach me: runs a whole flashcard session, one card per step", async ({ page }) => {
  await clearGoals(page);
  await page.getByRole("link", { name: "Goals", exact: true }).click();
  await page.getByLabel("Goal kind").selectOption("promotion");
  await page.getByLabel("Goal title").fill("Promotion packet");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText("Promotion packet");
  await page.getByRole("link", { name: "Teach me", exact: true }).click();
  const before = Number((await page.getByRole("group", { name: "Steps" }).innerText()).replace(/\D/g, ""));  // earlier tests also log steps today
  const seen = new Set<string>();
  for (let i = 0; i < 3; i++) {
    const q = page.locator(".flashcard .question");
    await expect(q).toBeVisible();
    const text = await q.innerText();
    expect(seen.has(text)).toBe(false); // each step is a new card
    seen.add(text);
    await page.keyboard.press("Space");
    await expect(page.locator(".flashcard .answer")).toBeVisible();
    if (i === 0) await page.keyboard.press("3");
    else await page.getByRole("button", { name: i === 1 ? "2 Hard" : "4 Easy" }).click();
    await expect(page.getByRole("status").first()).toContainText(/Keep going|come back soon/);
    await expect(page.locator(".flashcard .question")).not.toHaveText(text); // the next step is a new card
  }
  await expect(page.getByRole("group", { name: "Steps" })).toContainText(String(before + 3));
});

test("Goals: only the owner sets them, and the day re-plans at once", async ({ page }) => {
  await clearGoals(page);
  await page.getByRole("link", { name: "Goals", exact: true }).click();
  await page.getByLabel("Goal kind").selectOption("learn");
  await page.getByLabel("Goal title").fill("Learn hashing");
  await page.getByLabel("Topics").fill("arrays and hashing");
  await page.getByLabel("Minutes a day").fill("60");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText('now follow "Learn hashing"');
  await expect(page.getByText(/^Coding: /).first()).toBeVisible();
  await page.getByRole("button", { name: "Add a goal" }).click();
  await page.getByLabel("Goal kind").selectOption("promotion");
  await page.getByLabel("Goal title").fill("Promotion packet");
  await page.getByLabel("Goal date").fill(addDays(TODAY, 1));  // still ahead, so it drives the day
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText('now follow "Promotion packet"');
  await expect(page.getByText("Promotion: close one gap").first()).toBeVisible();
  await page.getByRole("button", { name: "Edit Promotion packet" }).click();
  await page.getByLabel("Goal date").fill("");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText('now follow "Learn hashing"');
  await page.getByLabel("New long-term goal").fill("Staff engineer by 2027");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expect(page.getByText("Staff engineer by 2027")).toBeVisible();
});

test("Job hunt: careerbrew suggests, the owner chooses", async ({ page }) => {
  await page.getByRole("link", { name: "Job hunt", exact: true }).click();
  await expect(page.getByRole("heading", { name: "Suggestions: you choose" })).toBeVisible();
  await page.getByRole("button", { name: /^Choose Northwind as dream$/ }).first().click();
  const dream = page.getByRole("region", { name: "Dream companies" });
  await expect(dream.getByText("Northwind")).toBeVisible();
});

test("the API refuses requests without the token", async ({ request }) => {
  const res = await request.get("/api/today");
  expect(res.status()).toBe(401);
});

test("Actions: every CLI action runs from the web app with a generated form", async ({ page }) => {
  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await expect(page.getByRole("heading", { name: /Actions \(\d{3}\)/ })).toBeVisible();
  await page.getByLabel("Filter actions").fill("config.set");
  await page.getByRole("button", { name: "config.set" }).click();
  await page.getByLabel("key").fill("questions_per_day");
  await page.getByLabel("value").fill("4");
  await page.getByRole("button", { name: "Run" }).click();
  await expect(page.getByLabel("Result")).toContainText("questions_per_day = 4");
  await page.getByLabel("Filter actions").fill("secret");
  await page.getByRole("button", { name: "secret.set" }).click();
  await expect(page.getByText(/Terminal only/)).toBeVisible();
  await page.screenshot({ path: `${OUT}/actions.png`, fullPage: true });
});

// Last: approving a goal changes the shared seeded server (Teach and Goals read it).
test("Today keeps the daily log: done, plan, PM draft to copy or send, and goal approval in the UI", async ({ page }) => {
  await page.getByRole("link", { name: "Today", exact: true }).click();
  await expect(page.getByRole("region", { name: /Done today/ })).toContainText("shipped order export");
  await expect(page.getByRole("region", { name: `Plan for ${shortDay(nextWorkday(TODAY))}` })).toBeVisible();
  const draft = page.getByLabel("PM update");
  await expect(draft).toHaveValue(/^Other: next: /);  // issue #68: one line per project, no headers or task ids
  await expect(draft).not.toHaveValue(/\bt\d{3}\b/);
  await draft.fill("Done today:\n- Shipped the order export");
  await expect(page.getByRole("button", { name: "Send to Slack" })).toBeDisabled();  // Slack is the default; no destination set yet
  await expect(page.getByText(/config set pm_slack/)).toBeVisible();
  const goal = page.getByRole("button", { name: "Approve goal" });
  await expect(goal).toBeVisible();
  const suggests = page.getByRole("region", { name: /careerbrew suggests/ });  // issue #62: what, why, and what each button changes
  for (const label of ["What", "Why", "Approve goal", "Dismiss"]) await expect(suggests.locator("dt", { hasText: label }).first()).toBeVisible();
  await expect(suggests.getByText(/Adds it to your upcoming goals/)).toBeVisible();
  await expect(suggests.getByText(/top promotion gap/).first()).toBeVisible();
  await goal.click();
  await expect(page.getByRole("button", { name: "Approve goal" })).toHaveCount(0);
  await page.screenshot({ path: `${OUT}/daily-log.png`, fullPage: true });
  await page.getByRole("link", { name: "Setup", exact: true }).click();
  await expect(page.getByRole("region", { name: "Settings" })).toContainText("pm_email");
});

test("Today: click a Done row to see why it is there and its history (issue #61)", async ({ page }) => {
  await page.getByRole("link", { name: "Today", exact: true }).click();
  const done = page.getByRole("region", { name: /Done today/ });
  await done.getByText(/shipped order export/).first().click();
  await expect(done.getByText("Why", { exact: true }).first()).toBeVisible();
  await expect(done.getByText("History", { exact: true }).first()).toBeVisible();
  await page.screenshot({ path: `${OUT}/row-details.png`, fullPage: true });
});

test("Interviews: practice by voice records an answer, transcribes it locally, and grades it", async ({ page }) => {
  await page.getByRole("link", { name: "Interviews", exact: true }).click();
  const voice = page.getByRole("region", { name: "Practice by voice" });
  await expect(voice.getByRole("button", { name: "Read question aloud" })).toBeVisible();
  await voice.getByRole("button", { name: "Record answer" }).click();
  await page.waitForTimeout(500);
  await voice.getByRole("button", { name: "Stop and grade" }).click();
  await expect(voice.getByText(/You said: .*checkout page/)).toBeVisible({ timeout: 15_000 });
  await expect(voice.getByText(/^Met: /)).toBeVisible();
  await expect(voice.getByText(/^Score \d+%/)).toBeVisible();
  await voice.getByRole("button", { name: "Next question" }).click();
  await voice.getByLabel("Typed answer").fill("Let me clarify edge cases first");
  await voice.getByRole("button", { name: "Grade typed answer" }).click();
  await expect(voice.getByText(/^Met: /)).toBeVisible();
});

test("cross-host help names the published skill and supported transcript hosts", async ({ page }) => {
  await page.route("**/api/interviews/Synthetic", (route) => route.fulfill({ json: { company: "Synthetic", known: false, loop: [], values: [], source_list: {} } }));
  await page.goto("/#/interviews/Synthetic");
  await expect(page.getByText(/Add one with the careerbrew-browser-research skill/)).toBeVisible();
  await page.route("**/api/library", (route) => route.fulfill({ json: { pages: [] } }));
  await page.getByRole("link", { name: "Library", exact: true }).click();
  await expect(page.getByText("Nothing imported yet. Ask your agent to run the careerbrew-notion-import skill.")).toBeVisible();
  await page.getByRole("link", { name: "Actions", exact: true }).click();
  await page.getByLabel("Filter actions").fill("work.feed.transcript");
  await page.getByRole("button", { name: "work.feed.transcript", exact: true }).click();
  await expect(page.getByLabel("file", { exact: true })).toHaveAttribute("placeholder", "Claude/Codex JSONL");
});

for (const [viewport, width, height] of [["phone", 390, 844], ["desktop", 1280, 800]] as const) {
  for (const colorScheme of ["light", "dark"] as const) {
    test(`transcript hint fits ${viewport} ${colorScheme}`, async ({ page }) => {
      await page.setViewportSize({ width, height });
      await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
      await page.goto("/#/actions");
      await page.getByLabel("Filter actions").fill("work.feed.transcript");
      await page.getByRole("button", { name: "work.feed.transcript", exact: true }).click();
      const field = page.getByLabel("file", { exact: true });
      const metrics = await field.evaluate((element) => {
        const input = element as HTMLInputElement;
        const style = getComputedStyle(input);
        const context = document.createElement("canvas").getContext("2d")!;
        context.font = style.font;
        return { text: input.placeholder, textWidth: context.measureText(input.placeholder).width, availableWidth: input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) };
      });
      console.log("transcript hint", viewport, colorScheme, JSON.stringify(metrics));
      expect(metrics.textWidth).toBeLessThanOrEqual(metrics.availableWidth);
      expect(metrics.text).toBe("Claude/Codex JSONL");
    });
  }
}
