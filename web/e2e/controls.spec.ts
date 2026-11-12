import { mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { expect, test } from "@playwright/test";
import type { Page, Route } from "@playwright/test";
import { OUT, TOKEN } from "../playwright.config";
import { runOwnedVisit, trackPageErrors } from "./controls-route-owner";

// The control inventory (US-72): every input, select, textarea, checkbox, button, form, and button-styled link in
// every view, by accessible name, with each view's folded parts open, for a personal and a corporate machine.
// Each control needs an entry in controls.json that names the Playwright test exercising its real effect, and that
// test must exist. The inventory itself never changes data: a state the seed does not have (a teacher step kind,
// an edited PM draft, notifications turned off, a read that failed, a signed-out tab) is a GET answer patched here,
// and the few clicks it makes to show more controls reach only patched POST answers or a refused clipboard.

const DIR = new URL(".", import.meta.url);
interface Generated { match: string; test: string; why: string }
interface Coverage { controls: Record<string, string>; generated: Generated[] }
const COVERAGE = JSON.parse(readFileSync(new URL("controls.json", DIR), "utf8")) as Coverage;

// Every test in web/e2e as "<file> › <title>", the way controls.json names them. A title made in a loop
// (`${v.name}: …`) matches each name it can take, so controls.json names the test Playwright runs ("Today: …").
function specTests(): { has: (name: string) => boolean } {
  const out = new Set<string>();
  const loops: RegExp[] = [];
  const quote = (t: string) => t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  for (const file of readdirSync(DIR).filter((f) => f.endsWith(".spec.ts"))) {
    for (const m of readFileSync(new URL(file, DIR), "utf8").matchAll(/^\s*test\((["'`])(.+?)\1\s*,/gm)) {
      const name = `${file} › ${m[2]}`;
      if (m[1] === "`" && name.includes("${")) loops.push(new RegExp(`^${name.split(/\$\{[^}]*\}/).map(quote).join("[^›]+")}$`));
      else out.add(name);
    }
  }
  return { has: (name) => out.has(name) || loops.some((re) => re.test(name)) };
}

// One control: "<view>: <role> <accessible name>". Roles follow ARIA (a date input is "date", a styled link "link").
// The name follows the accessible-name rules this app uses: aria-labelledby, aria-label, its <label>, its text, then
// title or placeholder. A form has no name of its own unless labelled, so it is named by its submit button.
async function controls(page: Page): Promise<string[]> {
  return page.evaluate(() => {
    const clean = (s: string | null | undefined) => (s ?? "").replace(/\s+/g, " ").trim();
    const shown = (el: Element) => el.tagName === "FORM" || el.getClientRects().length > 0;
    const labelText = (label: HTMLLabelElement) => {
      const copy = label.cloneNode(true) as HTMLElement;
      copy.querySelectorAll("input, select, textarea, button").forEach((c) => c.remove());
      return copy.textContent;
    };
    const nameOf = (el: Element): string => {
      const ids = el.getAttribute("aria-labelledby");
      if (ids) return clean(ids.split(/\s+/).map((id) => document.getElementById(id)?.textContent ?? "").join(" "));
      const aria = clean(el.getAttribute("aria-label"));
      if (aria) return aria;
      if (el instanceof HTMLFormElement) {
        const submit = el.querySelector("button[type=submit], button:not([type])");
        return submit ? `submits "${nameOf(submit)}"` : "";
      }
      const labels = "labels" in el ? Array.from((el as HTMLInputElement).labels ?? []) : [];
      if (labels.length) return clean(labels.map(labelText).join(" "));
      if (el instanceof HTMLButtonElement || el instanceof HTMLAnchorElement) return clean(el.textContent);
      return clean(el.getAttribute("title") || el.getAttribute("placeholder"));
    };
    const roleOf = (el: Element): string => {
      if (el instanceof HTMLButtonElement) return "button";
      if (el instanceof HTMLAnchorElement) return "link";
      if (el instanceof HTMLSelectElement) return "combobox";
      if (el instanceof HTMLTextAreaElement) return "textbox";
      if (el instanceof HTMLFormElement) return el.getAttribute("role") === "search" ? "search" : "form";
      const type = (el.getAttribute("type") ?? "text").toLowerCase();
      return ({ checkbox: "checkbox", radio: "radio", number: "spinbutton", date: "date", range: "slider" } as Record<string, string>)[type] ?? "textbox";
    };
    const view = clean(document.querySelector("main h1")?.textContent);
    const found = Array.from(document.querySelectorAll("input, select, textarea, button, form, a.button")).filter(shown);
    return [...new Set(found.map((el) => `${view}: ${roleOf(el)} ${nameOf(el) || "(no name)"}`))];
  });
}

type Patch = (body: Record<string, unknown>) => unknown;
type Url = string | RegExp | ((url: URL) => boolean);
interface Ctx {
  corporate: boolean;
  patch: (url: Url, fn: Patch) => Promise<void>;
  answer: (url: Url, body: unknown) => Promise<void>;
  fail: (url: Url) => Promise<void>;  // a GET answers 500, so the block shows its error and Retry
}
// `after` shows more controls (a revealed answer, an open form); it calls `snap` to record the ones a click hides.
// `as` names the controls after a page that shows inside any view (the sign-in page), not after the view's title.
interface Visit { view: string; state: string; hash: string; ready: (page: Page) => Promise<void>; personal?: boolean; as?: string; before?: (page: Page, ctx: Ctx) => Promise<void>; after?: (page: Page, snap: () => Promise<void>) => Promise<void> }

const RANKED = { projects: [{ name: "billing", rank: 1, match: ["*billing*"], open_tasks: 1, today: 0 }, { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 }] }; // the seed ranks nothing (US-81)
const heading = (name: string | RegExp) => async (page: Page) => { await expect(page.getByRole("heading", { name }).first()).toBeVisible(); };
const retry = async (page: Page) => { await expect(page.getByRole("button", { name: "Retry", exact: true }).first()).toBeVisible(); };
// Every view read fails except the ones the shell needs (sign-in, the role, the cache key, the live poll), as in blocks.spec.ts.
const VIEW_READ = (url: URL) => url.pathname.startsWith("/api/") && !/^\/api\/(session|overview|tab-key|ping|changes)(\/|$)/.test(url.pathname);
// The browser refuses the clipboard, so Copy update shows the text to copy by hand (issue #145).
const refuseCopy = (label: string) => async (page: Page) => {
  await page.evaluate(() => {
    navigator.clipboard.writeText = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
  });
  await page.getByRole("button", { name: label, exact: true }).click();
  await expect(page.getByLabel("Update to copy")).toBeVisible();
};
const STARTED = new Date().toISOString().slice(0, 19);

// Synthetic teacher steps, one per kind the seed does not reach on its own (the shape of teacher.next_step).
const card = { id: "inventory-card", question: "What does a hash set give you?", answer: "O(1) average membership tests.", explanation: "## How it works\nHashing.", explanation_id: "inventory-card", topic: "arrays", tags: [], deck: "mine", mode: "review" };
const exercise = { id: "inventory-ex", order: 1, pattern: "arrays", difficulty: "easy", title: "Contains duplicate", statement: "Return True if any value repeats.", signature: "def contains_duplicate(nums):", lesson: "", starter: "def contains_duplicate(nums):\n    pass\n", hints: ["Use a set."], tests: [{ args: [[1, 2, 1]], expected: true, note: "" }], watch_out: ["off by one"], worked_example: { title: "Two sum", statement: "Find two numbers.", solution: "def f(): pass" } };
const STEPS: Record<string, Record<string, unknown>> = {
  lesson: { kind: "lesson", title: "Lesson: arrays", why: "learn the idea first", pattern: "arrays", lesson: "Use a set.", prompt: "When would you use a set?", example: { title: "Example", statement: "Find a repeat.", solution: "set()" } },
  exercise: { kind: "exercise", title: "Contains duplicate", why: "next in the curriculum", exercise, attempts: 2 },
  card: { kind: "card", title: card.question, why: "due cards first", card },
  validate: { kind: "validate", title: `Check your card: ${card.question}`, why: "cards you wrote are checked once", card },
  approve: { kind: "approve", title: "Approve or reject: publish the notes", why: "an agent is waiting", approval: { id: "inventory-approval", title: "publish the notes", detail: "Publish the notes.", by: "agent", at: `${STARTED}`, status: "pending", note: "" } },
  resume: { kind: "resume", title: "Send your resume to Northwind", why: "you chose this job", job: { company: "Northwind", title: "Engineer", want: 5, p_offer: 0.2, ev: 1, url: "https://careers.example.com/1", posting_id: "inventory-job" }, checklist: ["Open the posting."] },
  mistake: { kind: "mistake", title: "This mistake repeated", why: "repeated mistakes first", mistake: { at: STARTED, source: "lesson:arrays", pattern: "arrays", kind: "concept", detail: "not yet confident", tip: "Explain it in your own words.", repeat: true } },
};

const VISITS: Visit[] = [
  { view: "Dashboard", state: "as seeded", hash: "dashboard", ready: heading("Right now") },
  { view: "Dashboard", state: "the browser refused the clipboard", hash: "dashboard", ready: heading("Right now"), after: refuseCopy("Copy update") },
  {
    view: "Dashboard", state: "an open promise (issue #268)", hash: "dashboard", ready: heading("Commitments (1)"),
    before: (_page, ctx) => ctx.patch("**/api/promises", () => ({ steps: { research: "research it" }, promises: [{ name: "promise-0000abcd", title: "Look into the timeout", to: "Sam", due: "", overdue: false, made_at: STARTED, url: "https://example.com/t", parent: "", next: "research", steps: {}, closed: "" }] })),
  },
  { view: "Teach me", state: "as seeded", hash: "teach", ready: heading("Help the teacher improve") },
  ...Object.entries(STEPS).map(([kind, step]): Visit => ({
    view: "Teach me", state: `${kind} step`, hash: "teach", ready: heading(kind === "card" ? "Recall this card" : String(step.title)),  // a card shows its question once, in the card
    before: (_page, ctx) => ctx.patch("**/api/teach", (t) => ({ ...t, step: { ...step, started: `${STARTED}Z` } })),
    after: kind === "card" ? async (page, snap) => { await snap(); await page.getByRole("button", { name: "Show answer (Space)" }).click(); } : undefined,
  })),
  { view: "Goals", state: "as seeded", hash: "goals", ready: heading("Upcoming goals") },
  {
    view: "Goals", state: "a goal and a long-term goal, adding another", hash: "goals", ready: heading("Upcoming goals"),
    before: (_page, ctx) => ctx.patch("**/api/goals", (g) => ({ ...g, queue: [{ id: "g1", kind: "interview", title: "Onsite", date: null, company: "Northwind", topics: [], minutes_per_day: 60 }], long_term: [{ id: "l1", text: "Staff engineer" }] })),
    after: (page) => page.getByRole("button", { name: "Add a goal" }).click(),
  },
  { view: "Today", state: "as seeded", hash: "today", ready: heading("Update for your PM") },
  { view: "Today", state: "two ranked projects", hash: "today", ready: heading("Active now"), before: (_page, ctx) => ctx.patch("**/api/priority", () => RANKED) },
  // Issue #275: more open asks than one answer holds shows "Showing N of M" and Show all.
  { view: "Today", state: "more open asks than one answer holds", hash: "today", ready: heading("Update for your PM"), before: (_page, ctx) => ctx.patch("**/api/feed/asks", (d) => ({ ...d, total: (d.asks as object[]).length + 10 })) },
  {
    view: "Today", state: "an edited PM draft, a goal suggestion, an ask just marked personal", hash: "today", ready: heading("Update for your PM"),
    before: async (_page, ctx) => {
      // The seed's goal suggestion is approved by an earlier test, so this state brings one of its own.
      const goal = { id: "inventory-goal", type: "proposal", kind: "goal", status: "open", text: "New goal", what: "New goal: close a gap", why: "your top gap", accept: "Adds it to your goals.", dismiss: "Nothing changes." };
      await ctx.patch("**/api/inbox", (d) => ({ ...d, inbox: [...(d.inbox as object[]), goal] }));
      await ctx.patch("**/api/daylog", (d) => ({ ...d, pm: { ...(d.pm as object), edited: true, draft: "the generated draft" } }));
      await ctx.answer("**/api/actions/work.feed.mark", { action: "work.feed.mark", argv: [], exit: 0, output: "" });
    },
    after: async (page) => {
      await page.getByRole("button", { name: /^Mark personal:/ }).first().click();
      await expect(page.getByRole("button", { name: "Undo" })).toBeVisible();
    },
  },
  {
    view: "Today", state: "the PM update goes by email", hash: "today", ready: heading("Update for your PM"),
    before: (_page, ctx) => ctx.patch("**/api/daylog", (d) => ({ ...d, pm: { ...(d.pm as object), via: "email", to: "pm@example.com" } })),
  },
  { view: "Today", state: "the browser refused the clipboard", hash: "today", ready: heading("Update for your PM"), after: refuseCopy("Copy update") },
  {
    // A list longer than its page (issue #146): the suggestions show Previous, Next, and one filter button per kind;
    // with projects and ranks, the project, priority, and sort selects (issue #147). A group of alike rows shows Accept
    // all, Review each, and Dismiss all, and each bulk button its confirm; a row done elsewhere shows Bring back. The
    // clicks here only open the confirm and the group's rows: nothing reaches the server.
    view: "Today", state: "more suggestions than a page, a group, a row done elsewhere", hash: "today", ready: heading("Update for your PM"),
    before: (_page, ctx) => ctx.patch("**/api/inbox", (d) => {
      const explained = { what: "Plan task", why: "an agent added it", accept: "Moves the task to open.", dismiss: "Sets the task to dismissed.", added_by: "an agent", links: [], lands: { label: "Week", url: "#/week" } };
      const group = { id: "grp-inventory1", kind: "agent-task", count: 3, label: "Plan 3 tasks an agent added", why: "an agent added them", accept: "Moves all 3 to open.", dismiss: "Sets all 3 to dismissed.", added_by: "an agent", links: [], lands: null, project: "", priority: null, created_at: "", ids: [] };
      return {
        ...d,
        inbox: [
          ...[1, 2, 3].map((n) => ({ id: `inventory-group-${n}`, type: "proposal", kind: "agent-task", text: `Synthetic chore ${n}`, status: "open", group: group.id, ...explained })),  // first: on page 1
          ...(d.inbox as object[]),
          ...Array.from({ length: 12 }, (_, i) => ({ id: `inventory-page-${i + 1}`, type: "proposal", kind: i < 10 ? "agent-task" : "goal", text: `Synthetic suggestion ${i + 1}`, status: "open", project: i < 4 ? "syn-platform" : "", priority: i < 4 ? 1 : null })),
        ],
        groups: [group],
        expired: [{ id: "inventory-expired", type: "proposal", kind: "agent-task", text: "Synthetic review", status: "expired", ...explained, expired: { what: "you gave a review (github)", url: "", at: "" } }],
      };
    }),
    after: async (page, snap) => {
      const group = page.locator("li[data-inbox-group]").first();
      await group.getByRole("button", { name: "Review each" }).click();
      await group.getByRole("button", { name: "Accept all" }).click();
      await expect(group.getByRole("button", { name: "Yes, accept all" })).toBeVisible();
      await snap();
      await group.getByRole("button", { name: "Dismiss all" }).click();
      await expect(group.getByRole("button", { name: "Yes, dismiss all" })).toBeVisible();
    },
  },
  { view: "Standup", state: "as seeded", hash: "standup", ready: heading("Order export") },
  { view: "Notifications", state: "as seeded", hash: "notifications", ready: heading(/^Open \(\d+\)$/) },
  {
    view: "Notifications", state: "turned off, one without a link", hash: "notifications", ready: heading(/^Open \(\d+\)$/),
    before: (_page, ctx) => ctx.patch("**/api/notifications", (n) => {
      const items = n.items as Record<string, unknown>[];
      return { ...n, status: { ...(n.status as object), enabled: false }, items: [...items, { ...items[0], id: "inventory-no-link", url: "", status: "new", actions: ["dismiss"] }] };
    }),
  },
  { view: "Week", state: "as seeded", hash: "week", ready: heading("Plan ahead") },
  { view: "Job hunt", state: "as seeded", hash: "jobs", ready: heading("Season strategy"), personal: true },
  {
    view: "Job hunt", state: "a chosen dream job", hash: "jobs", ready: heading("Season strategy"), personal: true,
    before: (_page, ctx) => ctx.patch("**/api/jobs", (j) => {
      const s = j.strategy as { suggested: Record<string, unknown[]>; tiers: Record<string, unknown[]> };
      return { ...j, strategy: { ...s, tiers: { ...s.tiers, dream: [s.suggested.dream[0]] } } };
    }),
  },
  { view: "Interviews", state: "as seeded", hash: "interviews", ready: heading(/Shared patterns across/), personal: true },
  {
    view: "Interviews", state: "recording, then a graded voice answer", hash: "interviews", ready: heading(/Shared patterns across/), personal: true,
    before: (_page, ctx) => ctx.answer("**/api/drill/voice", { transcript: "a test answer", source: "audio", row: { met: [], missed: [], score: 0, rating: "again" }, feedback: ["Score 0%"] }),
    after: async (page, snap) => {
      await page.getByRole("button", { name: "Record answer" }).click();
      await expect(page.getByRole("button", { name: "Stop and grade" })).toBeVisible();
      await snap();
      await page.getByRole("button", { name: "Stop and grade" }).click();
      await expect(page.getByRole("button", { name: "Next question" })).toBeVisible();
    },
  },
  { view: "Interviews", state: "a company page", hash: "interviews/Datadog", ready: heading("Interview loop: Datadog"), personal: true },
  {
    view: "Practice", state: "the answer shown", hash: "practice", ready: heading("Progress"), personal: true,
    after: async (page, snap) => { await snap(); await page.getByRole("button", { name: "Show answer (Space)" }).click(); },
  },
  { view: "Library", state: "a study card open", hash: "library", ready: heading(/Pages \(\d+\)/), personal: true,
    after: async (page) => {
      await page.getByLabel("Filter pages").fill("binary search");
      await page.getByRole("link", { name: "How does binary search work?" }).click();
      await expect(page.getByText("Report a problem with this explanation")).toBeVisible();
    } },
  { view: "Work", state: "as seeded", hash: "work", ready: heading(/Promotion case/) },
  { view: "Work", state: "two ranked projects", hash: "work", ready: heading("Active now"), before: (_page, ctx) => ctx.patch("**/api/priority", () => RANKED) },
  { view: "Review", state: "as seeded", hash: "review", ready: heading("Outcome metrics") },
  { view: "Reports", state: "as seeded", hash: "reports", ready: heading(/Run pages \(\d+\)/) },
  { view: "Actions", state: "as seeded", hash: "actions", ready: heading(/Actions \(\d+\)/) },
  // Each watcher state offers its own buttons; with it off, a setting is also being changed.
  ...(["off", "on", "paused"] as const).map((state): Visit => ({
    view: "Setup", state: state === "off" ? "watcher off, a setting being changed" : `watcher ${state}`, hash: "setup", ready: heading("Day watcher"),
    before: (_page, ctx) => ctx.patch("**/api/overview", (o) => ({ ...o, ...(ctx.corporate ? { role: "corporate" } : {}), watcher: { ...(o.watcher as object), state } })),
    after: state !== "off" ? undefined : async (page) => {
      await page.getByRole("region", { name: "Settings" }).getByRole("button", { name: /^Change/ }).first().click();
      await expect(page.getByRole("region", { name: "Settings" }).getByRole("button", { name: "Save" })).toBeVisible();
    },
  })),
  // Each block that fails to load shows its error with Retry (issue #142): every view in the nav, one after another.
  // The tab's copy of the last answers goes first (keys "careerbrew-cache…", web/src/cache.ts; the token stays), so
  // each view fails as on its first visit, the way blocks.spec.ts tests Retry.
  {
    view: "Every view", state: "each read failed", hash: "dashboard", ready: retry,
    before: async (page, ctx) => {
      await page.evaluate(() => Object.keys(sessionStorage).filter((k) => k.startsWith("careerbrew-cache")).forEach((k) => sessionStorage.removeItem(k)));
      await ctx.fail(VIEW_READ);
    },
    after: async (page, snap) => {
      const nav = page.getByRole("navigation", { name: "Views" });
      for (const name of await nav.getByRole("link").allTextContents()) {
        await nav.getByRole("link", { name, exact: true }).click();
        await expect(page.getByRole("heading", { level: 1, name, exact: true })).toBeVisible();
        await expect(page.locator(".skeleton")).toHaveCount(0);
        await retry(page);
        await snap();
      }
    },
  },
  // Last: a tab with no sign-in (no token, the server gives it none) shows the sign-in page in place of the view (issue #141).
  {
    view: "Sign-in page", state: "signed out", hash: "today", as: "Sign-in page", ready: heading("Sign in to careerbrew"),
    before: async (page, ctx) => {
      await page.evaluate(() => sessionStorage.clear());
      await ctx.fail((url) => url.pathname === "/api/session");
    },
  },
];

// Each action's generated form: open them one by one in the Actions view (no request: the list is loaded once).
async function actionForms(page: Page, add: (keys: string[], state: string) => void) {
  const buttons = page.getByRole("region", { name: /^Actions \(\d+\)$/ }).getByRole("listitem").getByRole("button");
  const names = await buttons.allTextContents();
  expect(names.length).toBeGreaterThan(100);
  for (const [i, name] of names.entries()) {
    await buttons.nth(i).click();
    await expect(page.getByRole("heading", { level: 2, name, exact: true })).toBeVisible();
    add(await controls(page), `the ${name} form`);
  }
}

for (const corporate of [false, true]) {
  const machine = corporate ? "corporate" : "personal";
  test(`inventory on a ${machine} machine: each control is in controls.json with a test that exists`, async ({ page }) => {
    test.setTimeout(240_000);
    const errors: string[] = [];
    if (corporate) {
      // As in app.spec.ts: the seeded server is personal, so the page plays a corporate machine.
      await page.context().route("**/api/overview", async (route) => {
        const res = await route.fetch();
        await route.fulfill({ response: res, json: { ...(await res.json()), role: "corporate" } });
      });
      await page.context().route(/\/api\/(jobs|interviews|cards|library|explanations|exercises|drill)(\/|$|\?)/, (route) => route.fulfill({ status: 403, json: { error: "job-hunt views are personal-only; this is a corporate machine" } }));
    }
    const found = new Map<string, Set<string>>();
    const add = (keys: string[], state: string) => keys.forEach((k) => found.set(k, (found.get(k) ?? new Set()).add(state)));
    const context = page.context();
    for (const v of VISITS) {
      await runOwnedVisit(context, async (page) => {
        trackPageErrors(page, errors);
        const snapshot = async () => (await controls(page)).map((k) => (v.as ? `${v.as}:${k.slice(k.indexOf(":") + 1)}` : k));
        const ctx: Ctx = {
          corporate,
          patch: async (url, fn) => {
            const handler = async (route: Route) => {
              if (route.request().method() !== "GET") return route.fallback();
              const res = await route.fetch();
              await route.fulfill({ response: res, json: fn(await res.json()) });
            };
            await page.route(url, handler);
          },
          answer: async (url, body) => {
            const handler = async (route: Route) => (route.request().method() === "POST" ? route.fulfill({ json: body }) : route.fallback());
            await page.route(url, handler);
          },
          fail: async (url) => {
            const handler = async (route: Route) => (route.request().method() === "GET" ? route.fulfill({ status: 500, json: { error: "synthetic failure for the control inventory" } }) : route.fallback());
            await page.route(url, handler);
          },
        };
        await page.goto(`/#token=${TOKEN}`);
        await v.before?.(page, ctx);
        await page.goto("about:blank");
        await page.goto(`/#/${v.hash}`);
        if (corporate && v.personal) {
          await expect(page.getByRole("status").filter({ hasText: "job-hunt views are personal-only" })).toBeVisible();
        } else {
          await v.ready(page);
          await expect(page.locator(".skeleton")).toHaveCount(0);
          await expect(page.getByText("Loading…", { exact: true })).toHaveCount(0);
          // A view first shows its last answer from the page's cache, then the fresh one (issue #142): wait for it.
          await expect(page.locator(".updated").filter({ hasText: "refreshing" })).toHaveCount(0);
          await v.after?.(page, async () => add(await snapshot(), `${v.view}: ${v.state}`));
          await page.evaluate(() => document.querySelectorAll("details").forEach((d) => (d.open = true)));
          if (v.view === "Actions") await actionForms(page, add);
        }
        add(await snapshot(), `${v.view}: ${v.state}`);
      });
    }

    const tests = specTests();
    const patterns = COVERAGE.generated.map((g) => ({ ...g, re: new RegExp(g.match) }));
    const covered = (key: string) => COVERAGE.controls[key] ?? patterns.find((p) => p.re.test(key))?.test;
    const where = (key: string) => [...(found.get(key) ?? [])].slice(0, 3).join("; ");
    const problems = [
      ...[...found.keys()].filter((k) => k.includes("(no name)")).map((k) => `${k}: no accessible name (in ${where(k)}). Fix: give it an aria-label or a <label>.`),
      ...[...found.keys()].filter((k) => !covered(k)).map((k) => `${k}: not in web/e2e/controls.json (in ${where(k)}). Fix: write a test that exercises it, then add "${k}": "<spec file> › <test title>".`),
      ...[...Object.values(COVERAGE.controls), ...patterns.map((p) => p.test)].filter((t) => !tests.has(t)).map((t) => `controls.json names a test that does not exist: ${t}`),
    ];
    if (!corporate) {
      problems.push(...Object.keys(COVERAGE.controls).filter((k) => !found.has(k)).map((k) => `controls.json lists ${k}, but no view shows it. Fix: remove the entry, or add a visit that shows it.`));
      problems.push(...patterns.filter((p) => ![...found.keys()].some((k) => p.re.test(k))).map((p) => `controls.json pattern ${p.match} matches no control. Fix: remove it.`));
    }
    mkdirSync(OUT, { recursive: true });
    writeFileSync(`${OUT}/controls-${machine}.json`, JSON.stringify(Object.fromEntries([...found].sort().map(([k, s]) => [k, { test: covered(k) ?? null, seen: [...s] }])), null, 2));
    expect(found.size, "controls found").toBeGreaterThan(corporate ? 50 : 80);
    expect(problems, `${problems.length} control problem(s); the full list is in ${OUT}/controls-${machine}.json`).toEqual([]);
    expect(errors).toEqual([]);
  });
}
