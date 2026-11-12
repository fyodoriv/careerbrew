import { expect, test } from "@playwright/test";
import type { Page, Request, Route } from "@playwright/test";
import { NOW, OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import { friendlyDay } from "../src/dates";
import { disconnectedSlackServer } from "./pm-slack-fixture";

// US-72: every control does what it says. Each test drives controls the other files do not reach and checks the
// real effect: the server's state (read back through the API), the view, or an error that names the fix.
// controls.json maps every control to its test; controls.spec.ts fails on a control with no test.
// Tests share one seeded server, so each one sets up what it needs and puts shared settings back.

const TODAY = NOW.slice(0, 10);
function addDays(day: string, n: number): string {
  const d = new Date(`${day}T00:00:00Z`);
  d.setUTCDate(d.getUTCDate() + n);
  return d.toISOString().slice(0, 10);
}
const HEADERS = { "X-Careerbrew-Token": TOKEN };

async function get<T>(page: Page, path: string): Promise<T> {
  const res = await page.request.get(path, { headers: HEADERS });
  expect(res.ok(), `GET ${path}`).toBe(true);
  return (await res.json()) as T;
}
async function post<T>(page: Page, path: string, data: unknown): Promise<T> {
  const res = await page.request.post(path, { headers: HEADERS, data });
  expect(res.ok(), `POST ${path}: ${await res.text()}`).toBe(true);
  return (await res.json()) as T;
}
// The same handler as the Actions view and the CLI: `careerbrew <action>`.
async function action(page: Page, name: string, values: Record<string, unknown>, confirm = false): Promise<{ exit: number; output: string }> {
  return post(page, `/api/actions/${name}`, { values, confirm });
}
const setConfig = (page: Page, key: string, value: string) => post(page, "/api/config", { key, value });
const unsetConfig = (page: Page, key: string) => action(page, "config.unset", { key });
const setGoals = (page: Page, queue: unknown[]) => post(page, "/api/goals", { queue });

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

const go = (page: Page, name: string) => page.getByRole("navigation", { name: "Views" }).getByRole("link", { name, exact: true }).click();
const body = (r: Request) => r.postDataJSON() as Record<string, unknown>;

// ---------- Week ----------

test("Week: a new task's hours and due date reach the planner, and Add task waits for a title", async ({ page }) => {
  await go(page, "Week");
  const add = page.getByRole("button", { name: "Add task" });
  await expect(add).toBeDisabled();  // a click with no title did nothing and said nothing
  const due = addDays(TODAY, 3);
  await page.getByLabel("New task").fill("Write the inventory notes");
  await expect(add).toBeEnabled();
  await page.getByLabel("Hours").fill("2.5");
  await page.getByLabel("Due date").fill(due);
  const sent = page.waitForRequest((r) => r.url().endsWith("/api/tasks") && r.method() === "POST");
  await add.click();
  expect(body(await sent)).toMatchObject({ title: "Write the inventory notes", hours: 2.5, due });
  await expect(page.getByLabel("New task")).toHaveValue("");
  const work = await get<{ tasks: { title: string; estimate_h: number; due?: string }[] }>(page, "/api/work");
  expect(work.tasks.find((t) => t.title === "Write the inventory notes")).toMatchObject({ estimate_h: 2.5, due });
  await go(page, "Work");
  await expect(page.getByRole("region", { name: "Tasks by priority" }).getByRole("listitem").filter({ hasText: "Write the inventory notes" })).toContainText(`2.5h · due ${friendlyDay(due)}`);  // shown as a day in words (US-76)
});

// ---------- Goals ----------

test("Goals: an interview goal keeps its company; Cancel, Remove, and a long-term Remove change the lists", async ({ page }) => {
  await setGoals(page, []);
  await go(page, "Goals");
  await page.getByLabel("Goal kind").selectOption("interview");
  await page.getByLabel("Goal title").fill("Onsite at Northwind");
  await page.getByLabel("Company").fill("Northwind");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText('now follow "Onsite at Northwind"');
  expect((await get<{ upcoming: { company: string } }>(page, "/api/goals")).upcoming.company).toBe("Northwind");

  await page.getByRole("button", { name: "Add a goal" }).click();
  await expect(page.getByRole("heading", { name: "New goal" })).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("heading", { name: "New goal" })).toHaveCount(0);
  expect((await get<{ queue: unknown[] }>(page, "/api/goals")).queue).toHaveLength(1);

  await page.getByRole("button", { name: "Remove Onsite at Northwind" }).click();
  await expect(page.getByRole("button", { name: "Remove Onsite at Northwind" })).toHaveCount(0);
  expect((await get<{ queue: unknown[] }>(page, "/api/goals")).queue).toEqual([]);

  const longTerm = page.getByRole("region", { name: "Long-term goals" });
  const add = longTerm.getByRole("button", { name: "Add", exact: true });
  await expect(add).toBeDisabled();  // a click with nothing typed did nothing and said nothing
  await page.getByLabel("New long-term goal").fill("Lead the platform team");
  await add.click();
  const row = longTerm.getByRole("listitem").filter({ hasText: "Lead the platform team" });
  await expect(row).toBeVisible();
  await row.getByRole("button", { name: "Remove" }).click();
  await expect(row).toHaveCount(0);
  const goals = await get<{ long_term: { text: string }[] }>(page, "/api/goals");
  expect(goals.long_term.map((g) => g.text)).not.toContain("Lead the platform team");
});

test("Goals: goals added elsewhere while the view showed its last answer show up, and a save keeps them", async ({ page }) => {
  const before = await get<{ queue: object[]; long_term: object[] }>(page, "/api/goals");
  const soon = { kind: "interview", title: "Onsite at Northwind", date: addDays(TODAY, 10), company: "Northwind", topics: [], minutes_per_day: 60 };
  const first = await post<{ queue: object[] }>(page, "/api/goals", { queue: [soon], long_term: [] });
  try {
    await goalsAddedElsewhere(page, first.queue);
  } finally {
    await post(page, "/api/goals", { queue: before.queue, long_term: before.long_term });
  }
});

test("Goals: adding a long-term goal keeps the form as it is", async ({ page }) => {
  // A long-term save reloads the goals: the form must take the fresh lists without dropping a half-typed upcoming
  // goal (a review finding of goal 2026-10-22, against a form keyed on its goals).
  const before = await get<{ queue: object[]; long_term: object[] }>(page, "/api/goals");
  try {
    await go(page, "Goals");
    await page.getByRole("button", { name: "Add a goal" }).click();
    await page.getByLabel("Goal title").fill("Half typed");
    const longTerm = page.getByRole("region", { name: "Long-term goals" });
    await page.getByLabel("New long-term goal").fill("Write a design doc a month");
    await longTerm.getByRole("button", { name: "Add", exact: true }).click();
    await expect(longTerm.getByText("Write a design doc a month")).toBeVisible();
    await expect(page.getByLabel("New long-term goal")).toHaveValue("");
    await expect(page.getByLabel("Goal title")).toHaveValue("Half typed");
  } finally {
    await post(page, "/api/goals", { queue: before.queue, long_term: before.long_term });
  }
});

async function goalsAddedElsewhere(page: Page, queue: object[]) {
  await go(page, "Goals");
  await expect(page.getByRole("button", { name: "Edit Onsite at Northwind" })).toBeVisible();
  await go(page, "Today");  // the page keeps the Goals answer, and shows it first on the way back (issue #142)

  // The CLI or an agent adds a later goal and a long-term goal; the nearest goal, unchanged, still drives today.
  const later = { kind: "learn", title: "Cloud exam", date: addDays(TODAY, 30), topics: [], minutes_per_day: 30 };
  const added = await post<{ upcoming: object }>(page, "/api/goals", { queue: [...queue, later], long_term: [{ text: "Lead the platform team" }] });
  expect(added.upcoming).toEqual(queue[0]);
  await go(page, "Goals");
  const longTerm = page.getByRole("region", { name: "Long-term goals" });
  await expect(page.getByRole("button", { name: "Edit Cloud exam" })).toBeVisible();
  await expect(longTerm.getByText("Lead the platform team")).toBeVisible();

  // A save from the view starts from the fresh answer, so it keeps what was added elsewhere.
  await page.getByLabel("New long-term goal").fill("Mentor two engineers");
  await longTerm.getByRole("button", { name: "Add", exact: true }).click();
  await expect(longTerm.getByText("Mentor two engineers")).toBeVisible();
  const goals = await get<{ queue: { title: string }[]; long_term: { text: string }[] }>(page, "/api/goals");
  expect(goals.queue.map((g) => g.title)).toEqual(["Onsite at Northwind", "Cloud exam"]);
  expect(goals.long_term.map((g) => g.text)).toEqual(["Lead the platform team", "Mentor two engineers"]);
}

// Task goals-form-keeps-typed-text: the e2e "Teach me: confirm a lesson" once saved an empty title (400), because
// the form mounted again when the fresh answer replaced the cached one and dropped what was typed.
test("Goals: a title typed while the view shows its last answer stays when the fresh answer arrives, and the save keeps goals added elsewhere", async ({ page }) => {
  const before = await get<{ queue: object[]; long_term: object[] }>(page, "/api/goals");
  const soon = { kind: "interview", title: "Onsite at Northwind", date: addDays(TODAY, 10), company: "Northwind", topics: [], minutes_per_day: 60 };
  const first = await post<{ queue: object[] }>(page, "/api/goals", { queue: [soon], long_term: [] });
  try {
    await typedTitleSurvives(page, first.queue);
  } finally {
    await page.unrouteAll({ behavior: "ignoreErrors" });
    await post(page, "/api/goals", { queue: before.queue, long_term: before.long_term });
  }
});

async function typedTitleSurvives(page: Page, queue: object[]) {
  await go(page, "Goals");
  await expect(page.getByRole("button", { name: "Edit Onsite at Northwind" })).toBeVisible();
  await go(page, "Today");  // the page keeps the Goals answer, and shows it first on the way back (issue #142)

  // Hold the fresh read, so the form shows the cached answer while the owner types. The CLI or an agent adds a goal.
  let release = () => {};
  const held = new Promise<void>((r) => (release = r));
  await page.route("**/api/goals", async (route) => {
    if (route.request().method() === "GET") await held;
    await route.fallback();
  });
  const later = { kind: "learn", title: "Cloud exam", date: addDays(TODAY, 30), topics: [], minutes_per_day: 30 };
  await post(page, "/api/goals", { queue: [...queue, later] });
  await go(page, "Goals");
  await expect(page.locator("main .updated").first()).toHaveText(/^Updated \d{2}:\d{2} · (new data, )?refreshing…$/);
  await expect(page.getByRole("button", { name: "Edit Cloud exam" })).toHaveCount(0);  // still the cached answer
  await page.getByRole("button", { name: "Add a goal" }).click();
  await page.getByLabel("Goal title").fill("Mock interview");

  release();
  await expect(page.getByRole("button", { name: "Edit Cloud exam" })).toBeVisible();  // the fresh answer arrived
  await expect(page.locator("main .updated")).toHaveCount(0);
  await expect(page.getByLabel("Goal title")).toHaveValue("Mock interview");
  await page.getByRole("button", { name: "Save and re-plan" }).click();
  await expect(page.getByRole("status")).toContainText('now follow "Onsite at Northwind"');
  const goals = await get<{ queue: { title: string }[] }>(page, "/api/goals");
  expect(goals.queue.map((g) => g.title)).toEqual(["Onsite at Northwind", "Cloud exam", "Mock interview"]);
}

// ---------- Today ----------

test("Today: answer or skip a question, accept or dismiss a suggestion", async ({ page }) => {
  for (const title of ["Draft the e2e rollout note", "Clean up the e2e fixtures"]) {
    expect((await action(page, "work.task", { title, source: "agent", why: "a test proposes it" })).output).toContain("proposed");
  }
  await go(page, "Today");
  const suggests = page.getByRole("region", { name: /careerbrew suggests/ });
  const accepted = suggests.getByRole("listitem").filter({ hasText: "Draft the e2e rollout note" });
  await accepted.getByRole("button", { name: "Accept" }).click();
  await expect(accepted).toHaveCount(0);
  const dismissed = suggests.getByRole("listitem").filter({ hasText: "Clean up the e2e fixtures" });
  await dismissed.getByRole("button", { name: "Dismiss" }).click();
  await expect(dismissed).toHaveCount(0);
  const titles = (await get<{ tasks: { title: string }[] }>(page, "/api/work")).tasks.map((t) => t.title);
  expect(titles).toContain("Draft the e2e rollout note");  // accepted: an open task the planner sees
  expect(titles).not.toContain("Clean up the e2e fixtures");  // dismissed: never planned

  const questions = page.getByRole("region", { name: /^Questions for you/ });
  const competency = questions.locator('li[data-inbox="competency"]').first();
  const task = (await competency.innerText()).match(/\b(t\d{3})\b/)![1];
  const answer = competency.getByRole("textbox", { name: /^Answer: / });
  await expect(competency.getByRole("button", { name: "Answer" })).toBeDisabled();
  await answer.fill("not-a-competency");
  await competency.getByRole("button", { name: "Answer" }).click();
  await expect(competency.getByRole("alert")).toContainText("unknown competencies");
  await expect(competency.getByRole("alert")).toContainText("Fix:");
  await expect(answer).toHaveValue("not-a-competency");  // nothing typed is lost
  await answer.fill("quality");
  await competency.getByRole("button", { name: "Answer" }).click();
  await expect(questions.getByText(new RegExp(`competency does ${task} `))).toHaveCount(0);
  const tasks = (await get<{ tasks: { id: string; competencies: string[] }[] }>(page, "/api/work")).tasks;
  expect(tasks.find((t) => t.id === task)?.competencies).toEqual(["quality"]);

  const skipped = questions.getByRole("listitem").first();
  const text = await skipped.locator("div").first().innerText();
  await skipped.getByRole("button", { name: "Skip" }).click();
  await expect(questions.getByText(text, { exact: true })).toHaveCount(0);
  const inbox = (await get<{ inbox: { text: string }[] }>(page, "/api/daylog")).inbox;
  expect(inbox.map((i) => i.text)).not.toContain(text);
});

async function freshDraft(page: Page): Promise<{ day: string; draft: string; subject: string; via: string }> {
  const log = await get<{ log: { day: string }; pm: { draft?: string; body: string; subject: string } }>(page, "/api/daylog");
  await post(page, "/api/pm/draft", { day: log.log.day, text: "" });  // earlier tests may have edited it
  const pm = (await get<{ pm: { body: string; subject: string; via: string } }>(page, "/api/daylog")).pm;
  return { day: log.log.day, draft: pm.body, subject: pm.subject, via: pm.via };
}
// What Copy puts on the clipboard (issue #145): the body for Slack, the subject line first for email.
const copyText = (pm: { subject: string; via: string }, text: string) => (pm.via === "email" ? `${pm.subject}\n\n${text}` : text).replace(/\s+$/, "");

test("Today: the PM draft keeps your edits, Reset to draft brings the draft back, Copy copies it", async ({ page, context }) => {
  const { draft, subject, via } = await freshDraft(page);
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await go(page, "Today");
  const box = page.getByLabel("PM update");
  await expect(box).toHaveValue(draft);
  await box.fill("Shipped the inventory.\nNext: the coverage map.");
  await box.blur();
  await expect(page.getByText("Your edits are saved for this day.")).toBeVisible();
  await page.reload();
  await expect(box).toHaveValue("Shipped the inventory.\nNext: the coverage map.");

  await page.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(page.getByRole("button", { name: "Copied", exact: true })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(copyText({ subject, via }, "Shipped the inventory.\nNext: the coverage map."));

  await page.getByRole("button", { name: "Reset to draft" }).click();
  await expect(box).toHaveValue(draft);
  await expect(page.getByText("Back to the generated draft.")).toBeVisible();
  await page.reload();
  await expect(box).toHaveValue(draft);
  await expect(page.getByRole("button", { name: "Reset to draft" })).toHaveCount(0);
});

test("Today and Dashboard: a refused clipboard shows the update to copy by hand, and Close hides it", async ({ page }) => {
  const pm = await freshDraft(page);
  const text = copyText(pm, pm.draft);
  await page.addInitScript(() => {
    navigator.clipboard.writeText = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
  });
  await page.reload();
  const refused = page.getByRole("alert").filter({ hasText: "The browser did not allow the copy" });
  for (const view of ["Dashboard", "Today"]) {
    await go(page, view);
    await page.getByRole("button", { name: "Copy update", exact: true }).click();
    await expect(refused).toBeVisible();
    await expect(page.getByLabel("Update to copy")).toHaveValue(text);
    await page.getByRole("button", { name: "Close", exact: true }).click();
    await expect(page.getByLabel("Update to copy")).toHaveCount(0);
    await expect(refused).toHaveCount(0);
  }
  // The Copy button under Today's draft falls back the same way, inside its section.
  const section = page.getByRole("region", { name: "Update for your PM" });
  await section.getByRole("button", { name: "Copy", exact: true }).click();
  await expect(section.getByLabel("Update to copy")).toHaveValue(text);
  await section.getByRole("button", { name: "Close", exact: true }).click();
  await expect(section.getByLabel("Update to copy")).toHaveCount(0);
  await expect(section.getByRole("button", { name: "Copied", exact: true })).toHaveCount(0);
});

test("Today: Send to Slack posts the update, and the email choice opens a mail draft", async ({ page }) => {
  const { subject } = await freshDraft(page);
  await setConfig(page, "pm_slack", "#e2e-updates");
  try {
    await go(page, "Today");
    const sent = page.waitForRequest((r) => r.url().endsWith("/api/pm/send"));
    await page.getByRole("button", { name: "Send to Slack #e2e-updates" }).click();
    expect(String(body(await sent).text)).toContain(subject);
    await expect(page.getByRole("button", { name: "Sent to #e2e-updates" })).toBeDisabled();  // the server posted it (recorded Slack answer)
  } finally {
    await unsetConfig(page, "pm_slack");
  }
  await setConfig(page, "pm_send_via", "email");
  await setConfig(page, "pm_email", "pm@example.com");
  try {
    await page.reload();
    const draft = await page.getByLabel("PM update").inputValue();
    const mail = page.getByRole("link", { name: "Email pm@example.com" });
    await expect(mail).toHaveAttribute("href", `mailto:pm%40example.com?subject=${encodeURIComponent(subject)}&body=${encodeURIComponent(draft)}`);
  } finally {
    await unsetConfig(page, "pm_send_via");
    await unsetConfig(page, "pm_email");
  }
});

test("Today: Slack setup blocks sending without a connector", async ({ page }, info) => {
  const fixture = await disconnectedSlackServer(NOW);
  let attempts = 0;
  try {
    await page.route("**/*", async (route) => {
      const url = new URL(route.request().url());
      if (url.origin !== fixture.origin) return route.abort();
      if (url.pathname === "/api/pm/send") {
        attempts++;
        return route.abort();
      }
      return route.continue();
    });
    await page.emulateMedia({ reducedMotion: "reduce" });
    await page.goto(`${fixture.origin}/#token=${TOKEN}`);
    await go(page, "Today");
    const pm = page.getByRole("region", { name: "Update for your PM" });
    const send = pm.getByRole("button", { name: "Send to Slack #e2e-disconnected" });
    await expect(send).toBeVisible();
    await pm.screenshot({ path: info.outputPath("pm-slack-availability.png"), animations: "disabled" });
    await expect(send).toBeDisabled();
    await expect(pm.getByText("careerbrew connect slack", { exact: true })).toBeVisible();
    expect(attempts).toBe(0);
  } finally {
    try { await page.close(); } finally { await fixture.stop(); }
  }
});

test("Today: Earlier day shows the log kept for that day", async ({ page }) => {
  const days = (await get<{ history: string[] }>(page, "/api/daylog")).history;
  expect(days.length).toBeGreaterThan(1);  // the seed keeps the day before too
  await go(page, "Today");
  const earlier = page.getByRole("region", { name: "Earlier days" });
  await earlier.getByLabel("Earlier day").selectOption(days[1]);
  const kept = await get<{ markdown: string }>(page, `/api/daylog/${days[1]}`);
  await expect(earlier.locator("pre")).toHaveText(kept.markdown.trim());
  await expect(earlier.locator("pre")).toContainText(days[1]);
});

// ---------- Job hunt ----------

test("Job hunt: choose a target and a practice job, then remove them", async ({ page }) => {
  await go(page, "Job hunt");
  const tier = (name: string) => page.getByRole("region", { name: `${name} companies` });
  await page.getByRole("button", { name: "Choose Fabrikam as target" }).click();
  await expect(tier("Target").getByText("Fabrikam")).toBeVisible();
  await page.getByRole("button", { name: "Choose Contoso as practice" }).click();
  await expect(tier("Practice").getByText("Contoso")).toBeVisible();
  let jobs = await get<{ strategy: { tiers: Record<string, { company: string }[]> } }>(page, "/api/jobs");
  expect(jobs.strategy.tiers.target.map((r) => r.company)).toContain("Fabrikam");
  expect(jobs.strategy.tiers.practice.map((r) => r.company)).toContain("Contoso");
  for (const [name, company] of [["Target", "Fabrikam"], ["Practice", "Contoso"]]) {
    await tier(name).getByRole("listitem").filter({ hasText: company }).getByRole("button", { name: "Remove" }).click();
    await expect(tier(name).getByText(company)).toHaveCount(0);
    await expect(page.getByRole("button", { name: `Choose ${company} as ${name.toLowerCase()}` })).toBeVisible();  // a suggestion again
  }
  jobs = await get(page, "/api/jobs");
  expect([...jobs.strategy.tiers.target, ...jobs.strategy.tiers.practice].map((r) => r.company)).toEqual([]);
});

// ---------- Interviews ----------

test("Interviews: Open loop waits for a name, Read question aloud speaks the prompt, Next question clears the graded answer", async ({ page }) => {
  await page.addInitScript(() => {
    const said: string[] = [];
    (window as unknown as { __said: string[] }).__said = said;
    window.speechSynthesis.speak = (u: SpeechSynthesisUtterance) => void said.push(u.text);  // the browser's voice, without the sound
  });
  await page.reload();
  await go(page, "Interviews");
  await expect(page.getByRole("button", { name: "Open loop" })).toBeDisabled();  // a click with no name did nothing and said nothing
  const prompt = (await get<{ item: { prompt: string } }>(page, "/api/drill")).item.prompt;
  const voice = page.getByRole("region", { name: "Practice by voice" });
  await voice.getByRole("button", { name: "Read question aloud" }).click();
  expect(await page.evaluate(() => (window as unknown as { __said: string[] }).__said)).toEqual([prompt]);
  await voice.getByLabel("Typed answer").fill("I would clarify the edge cases first");
  await voice.getByRole("button", { name: "Grade typed answer" }).click();
  await expect(voice.getByLabel("Voice result")).toContainText("Met:");
  await expect(voice.getByLabel("Typed answer")).toHaveValue("");
  await voice.getByRole("button", { name: "Next question" }).click();
  await expect(voice.getByLabel("Voice result")).toHaveCount(0);
  await expect(voice.getByRole("button", { name: "Grade typed answer" })).toBeDisabled();
});

// ---------- Practice ----------

test("Practice: Hard, Good, and Easy each grade the card, and its explanation takes a report", async ({ page }) => {
  await go(page, "Practice");
  const reviewed = page.getByRole("group", { name: "Reviewed today" });
  for (const [label, rating] of [["2 Hard", "hard"], ["3 Good", "good"], ["4 Easy", "easy"]]) {
    const before = Number((await reviewed.innerText()).replace(/\D/g, ""));
    await page.getByRole("button", { name: "Show answer (Space)" }).click();
    const sent = page.waitForRequest((r) => /\/api\/cards\/[^/]+\/review$/.test(r.url()));
    await page.getByRole("button", { name: label }).click();
    expect(body(await sent).rating).toBe(rating);
    await expect(reviewed).toHaveText(new RegExp(`^Reviewed today\\s*${before + 1}$`));
  }
  await page.getByRole("button", { name: "Show answer (Space)" }).click();
  await page.getByText(/^Explanation: how it works/).click();
  await page.getByText("Report a problem with this explanation").click();
  await page.getByLabel("What is wrong").fill("the example skips the edge case");
  await page.getByRole("button", { name: "Report", exact: true }).click();
  await expect(page.getByText("rewritten in the next card-explainer run")).toBeVisible();
});

// ---------- Teach me ----------

test("Teach me: Not yet brings the lesson back, and the repeated mistake asks Got it", async ({ page }) => {
  await setGoals(page, [{ kind: "interview", title: "Two pointers onsite", topics: ["two pointers"], minutes_per_day: 90 }]);
  await go(page, "Teach me");
  const teacher = page.locator(".teacher-says");
  await expect(page.getByRole("heading", { name: "Lesson: two pointers" })).toBeVisible();
  await page.getByRole("button", { name: "Not yet" }).click();
  await expect(teacher).toContainText("That is fine. Read it once more");
  await expect(page.getByRole("heading", { name: "Lesson: two pointers" })).toBeVisible();
  await expect(page.getByText(/^Example: /)).toBeVisible();  // the second time, the lesson comes with a worked example
  await page.getByRole("button", { name: "Not yet" }).click();
  await expect(page.getByRole("heading", { name: /^This mistake repeated: concept in two pointers/ })).toBeVisible();
  await page.getByRole("button", { name: "Got it" }).click();
  await expect(teacher).toContainText("Good. Remember:");
  await expect(page.getByRole("heading", { name: /^This mistake repeated/ })).toHaveCount(0);
});

test("Teach me: an agent's request is approved with a note, or rejected", async ({ page }) => {
  await setGoals(page, [{ kind: "promotion", title: "Promotion packet", minutes_per_day: 90 }]);
  for (const [title, button, status] of [["Publish the e2e notes", "Approve", "approved"], ["Delete the e2e drafts", "Reject", "rejected"]]) {
    const out = (await action(page, "approvals.request", { title, by: "agent" })).output;
    const id = out.match(/approval (\w+) is waiting/)![1];
    await page.goto("about:blank");
    await page.goto("/#/teach");
    await expect(page.getByRole("heading", { name: `Approve or reject: ${title}` })).toBeVisible();
    await page.getByLabel("Note for the agent").fill(`${status} by the e2e test`);
    await page.getByRole("button", { name: button, exact: true }).click();
    await expect(page.locator(".teacher-says")).toContainText(`Recorded: ${status}. The agent will see it.`);
    const shown = await action(page, "approvals.list", {});
    expect(shown.output).toContain(id);
    expect(shown.output).toMatch(new RegExp(`${id}.*${status}`));
  }
});

test("Teach me: a card step: Show answer, report its explanation, grade Again and Good", async ({ page }) => {
  await setGoals(page, [{ kind: "promotion", title: "Promotion packet", minutes_per_day: 90 }]);
  await go(page, "Teach me");
  const teacher = page.locator(".teacher-says");
  await expect(page.locator(".flashcard .question")).toBeVisible();
  await page.getByRole("button", { name: "Show answer (Space)" }).click();
  await page.getByText("Explanation", { exact: true }).click();
  await page.getByText("Report a problem with this explanation").click();
  await page.getByLabel("What is wrong").fill("the answer is too long");
  await page.getByRole("button", { name: "Report", exact: true }).click();
  await expect(page.getByText("rewritten in the next card-explainer run")).toBeVisible();
  for (const [label, rating, said] of [["1 Again", "again", "It will come back soon."], ["3 Good", "good", "Keep going."]]) {
    if (label !== "1 Again") await page.getByRole("button", { name: "Show answer (Space)" }).click();
    const question = await page.locator(".flashcard .question").innerText();
    const sent = page.waitForRequest((r) => r.url().endsWith("/api/teach/complete"));
    await page.getByRole("button", { name: label }).click();
    expect(body(await sent).result).toEqual({ rating });
    await expect(teacher).toContainText(said);
    await expect(page.locator(".flashcard .question")).not.toHaveText(question);
  }
});

// Steps the shared seed does not reach by itself (a card you wrote, a chosen job's resume) come from a patched
// GET /api/teach; each button's POST goes to the real server, which records it and answers with the next step.
async function showStep(page: Page, step: Record<string, unknown>) {
  await page.route("**/api/teach", async (route) => {
    if (route.request().method() !== "GET") return route.fallback();
    const res = await route.fetch();
    await route.fulfill({ response: res, json: { ...(await res.json()), step: { ...step, started: new Date().toISOString() } } });
  });
  // The page's cache shows the last answer first (issue #142), maybe this same step from the last call: wait for
  // the patched fresh answer, so the real one never replaces it after the unroute.
  const fresh = page.waitForResponse((r) => new URL(r.url()).pathname === "/api/teach" && r.request().method() === "GET");
  await page.goto("about:blank");
  await page.goto("/#/teach");
  await fresh;
  await expect(page.getByRole("heading", { name: String(step.title) })).toBeVisible();
  await expect(page.locator(".updated")).toHaveCount(0);
  await page.unroute("**/api/teach");
}

test("Teach me: your own card: keep it, fix it, or drop it", async ({ page }) => {
  await setGoals(page, [{ kind: "promotion", title: "Promotion packet", minutes_per_day: 90 }]);
  const card = { id: "e2e-mine-1", question: "What does an index cost on writes?", answer: "Nothing", topic: "databases", tags: ["databases"], deck: "mine", mode: "new" };
  const step = { kind: "validate", title: `Check your card: ${card.question}`, why: "cards you wrote are checked once", card };
  const total = async () => (await get<{ stats: { total: number } }>(page, "/api/cards")).stats.total;
  const before = await total();
  for (const [button, said] of [["It is right: keep", "Validated."], ["Save my fix", "Fixed and validated."], ["Drop it", "Dropped from your review."]]) {
    await showStep(page, step);
    if (button === "Save my fix") {
      await expect(page.getByRole("button", { name: button })).toBeDisabled();
      await page.getByLabel("Corrected answer").fill("Every write also updates the index");
    }
    const sent = page.waitForRequest((r) => r.url().endsWith("/api/teach/complete"));
    await page.getByRole("button", { name: button }).click();
    expect((body(await sent).step as { kind: string }).kind).toBe("validate");
    await expect(page.locator(".teacher-says")).toContainText(said);
  }
  expect(await total()).toBe(before + 1);  // the fix is a new card with your corrected answer
});

test("Teach me: a chosen job's resume: I sent it, or Skip for now", async ({ page }) => {
  const job = { company: "Fabrikam", title: "Platform engineer", want: 4, p_offer: 0.2, ev: 1, url: "https://careers.example.com/f-1", posting_id: "ashby:Fabrikam:f-1", skills: ["python"] };
  const step = { kind: "resume", title: "Send your resume to Fabrikam", why: "you chose this target job", job, checklist: ["Open the posting and re-read the requirements."] };
  for (const [button, said] of [["I sent it", "Logged: applied to Fabrikam. Good luck."], ["Skip for now", "Skipped for now."]]) {
    await showStep(page, step);
    await page.getByRole("button", { name: button }).click();
    await expect(page.locator(".teacher-says")).toContainText(said);
  }
});

test("Teach me: written feedback reaches the teacher's list", async ({ page }) => {
  await go(page, "Teach me");
  const send = page.getByRole("button", { name: "Send feedback" });
  await expect(send).toBeDisabled();
  await page.getByLabel("Feedback for the teacher").fill("Shorter lessons before the e2e exercises, please");
  await send.click();
  await expect(page.getByRole("status").filter({ hasText: "Thanks." })).toBeVisible();
  const list = await get<{ improvements: { proposal: string }[] }>(page, "/api/teach/improvements");
  expect(list.improvements.map((i) => i.proposal)).toContain("You wrote: Shorter lessons before the e2e exercises, please");
});

// ---------- Sign-in page ----------

// "Open a signed-in tab" opens the default browser, so it runs in signin.spec.ts, on a server with a stand-in browser.
test("Sign-in page: Try again checks the sign-in again, and signs the tab in once the browser has the cookie", async ({ page }) => {
  const offline = (route: Route) => route.abort("connectionrefused");
  await page.route("**/api/session", offline);
  await page.evaluate(() => sessionStorage.clear());  // no token in this tab
  await page.goto("about:blank");
  await page.context().clearCookies();  // after the page that asked for the cookie is gone
  expect(await page.context().cookies()).toEqual([]);
  await page.goto("/#/today");
  await expect(page.getByRole("heading", { name: "careerbrew does not answer" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open a signed-in tab" })).toHaveCount(0);

  // The server answers again, but this browser has no sign-in yet: Try again says so and offers the fix.
  await page.unroute("**/api/session", offline);
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByRole("heading", { name: "Sign in to careerbrew" })).toBeVisible();
  await expect(page.getByRole("button", { name: "Open a signed-in tab" })).toBeVisible();
  await expect(page.getByText("careerbrew open", { exact: true })).toBeVisible();

  // Another tab signed in (it set the cookie): Try again signs this tab in, and the view loads its data.
  const res = await page.request.post("/api/session", { headers: HEADERS, data: {} });
  expect(await res.json()).toMatchObject({ signed_in: true, cookie: true });
  await page.getByRole("button", { name: "Try again" }).click();
  await expect(page.getByRole("heading", { name: "Update for your PM" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sign in to careerbrew" })).toHaveCount(0);
  expect(await page.evaluate(() => sessionStorage.getItem("careerbrew-token"))).toBe(TOKEN);
});

// ---------- Setup ----------

test("Setup: Change and Save a setting, Enter saves too, and a bad value names the fix", async ({ page }) => {
  await go(page, "Setup");
  const settings = page.getByRole("region", { name: "Settings" });
  const row = (key: string) => settings.getByRole("row").filter({ has: page.locator("code", { hasText: new RegExp(`^${key}$`) }) });
  const value = async (key: string) => (await get<{ settings: { key: string; value: unknown }[] }>(page, "/api/config")).settings.find((s) => s.key === key)?.value;
  try {
    await settings.locator("summary", { hasText: "PM update" }).click();  // settings fold by topic; Work day starts open
    await row("pm_report_title").getByRole("button", { name: "Change" }).click();
    await settings.getByLabel("New value for pm_report_title").fill("Daily notes");
    await settings.getByRole("button", { name: "Save" }).click();
    await expect(row("pm_report_title")).toContainText("Daily notes");
    expect(await value("pm_report_title")).toBe("Daily notes");

    await row("pm_report_title").getByRole("button", { name: "Change" }).click();
    await settings.getByLabel("New value for pm_report_title").fill("Daily update");
    await settings.getByLabel("New value for pm_report_title").press("Enter");
    await expect(settings.getByLabel("New value for pm_report_title")).toHaveCount(0);
    expect(await value("pm_report_title")).toBe("Daily update");

    await row("questions_per_day").getByRole("button", { name: "Change" }).click();
    await settings.getByLabel("New value for questions_per_day").fill("many");
    await settings.getByRole("button", { name: "Save" }).click();
    await expect(settings.getByRole("alert")).toContainText("questions_per_day");
    await expect(settings.getByRole("alert")).toContainText("Fix:");
    await expect(settings.getByLabel("New value for questions_per_day")).toHaveValue("many");  // nothing typed is lost
  } finally {
    await unsetConfig(page, "pm_report_title");
  }
});

test("Setup: the day watcher turns on, pauses, resumes, and turns off", async ({ page }) => {
  // A real watcher process starts and stops. Its first cycle would read sources and add notifications the files
  // after this one do not expect, so for this test the workday starts after the pinned clock (no cycle runs).
  const later = Number(NOW.slice(11, 13)) < 18 ? ["20:00", "21:00"] : ["04:00", "05:00"];
  await setConfig(page, "work_start", later[0]);
  await setConfig(page, "work_end", later[1]);
  try {
    await go(page, "Setup");
    const watcher = page.getByRole("region", { name: "Day watcher" });
    const state = watcher.locator(".card-head .badge");
    const buttons = watcher.locator(".buttons button");
    await expect(state).toHaveText("off");
    for (const [click, after, offered] of [
      ["Turn on", "on", ["Pause", "Turn off"]],
      ["Pause", "paused", ["Resume", "Turn off"]],
      ["Resume", "on", ["Pause", "Turn off"]],
      ["Turn off", "off", ["Turn on"]],
    ] as const) {
      await watcher.getByRole("button", { name: click }).click();
      await expect(state).toHaveText(after);
      await expect(buttons).toHaveText([...offered]);
      expect((await get<{ watcher: { state: string } }>(page, "/api/overview")).watcher.state).toBe(after);
    }
    await expect(watcher).toContainText("Running: false");
  } finally {
    await post(page, "/api/watch/off", {});
    await unsetConfig(page, "work_start");
    await unsetConfig(page, "work_end");
  }
});

// ---------- Actions ----------

test("Actions: the filter ignores case, and each action in the list opens its generated form", async ({ page }) => {
  await go(page, "Actions");
  const list = page.getByRole("region", { name: /^Actions \(\d+\)$/ });
  await page.getByLabel("Filter actions").fill("CONFIG.SET");
  await expect(list.getByRole("button", { name: "config.set", exact: true })).toBeVisible();
  await page.getByLabel("Filter actions").fill("");
  const all = (await get<{ actions: { name: string; terminal_only: string | null }[] }>(page, "/api/actions")).actions;
  const buttons = list.getByRole("listitem").getByRole("button");
  await expect(buttons).toHaveCount(all.length);
  for (const [i, a] of all.entries()) {
    await buttons.nth(i).click();
    await expect(page.getByRole("heading", { level: 2, name: a.name, exact: true })).toBeVisible();
    if (a.terminal_only) await expect(page.getByText(`Terminal only: ${a.terminal_only}`)).toBeVisible();
    else await expect(page.getByRole("form", { name: `Run ${a.name}` })).toBeVisible();
  }
  await page.screenshot({ path: `${OUT}/actions-forms.png`, fullPage: true });
});

test("Actions: generated fields of every kind reach the command: text, list, number, whole number, flag, choice, confirm", async ({ page }) => {
  await go(page, "Actions");
  const pick = async (name: string) => {
    await page.getByLabel("Filter actions").fill(name);
    await page.getByRole("button", { name, exact: true }).click();
    return page.getByRole("form", { name: `Run ${name}` });
  };
  const run = async (form: ReturnType<Page["getByRole"]>) => {
    await form.getByRole("button", { name: "Run" }).click();
    const result = form.getByLabel("Result");
    await expect(result).toContainText("exit 0");
    return { argv: await result.locator("code").innerText(), output: await result.locator("pre").innerText() };
  };

  // whole number and flag
  let form = await pick("cards.next");
  await form.getByLabel("count", { exact: true }).fill("2");
  await form.getByLabel("json", { exact: true }).check();
  let r = await run(form);
  expect(r.argv).toBe("careerbrew cards next --count=2 --json");
  expect(JSON.parse(r.output)).toHaveLength(2);

  // text and a comma-separated list
  form = await pick("work.project.add");
  await form.getByLabel("name", { exact: true }).fill("e2e-linked");
  await form.getByLabel("link", { exact: true }).fill("payments, checkout");
  r = await run(form);
  expect(r.argv).toBe("careerbrew work project add --link=payments --link=checkout -- e2e-linked");
  const projects = (await get<{ projects: { name: string; links: string[] }[] }>(page, "/api/work")).projects;
  expect(projects.find((p) => p.name === "e2e-linked")?.links).toEqual(["payments", "checkout"]);

  // number and choice
  form = await pick("work.task");
  await form.getByLabel("title", { exact: true }).fill("Size the e2e work");
  await form.getByLabel("hours", { exact: true }).fill("2.5");
  await form.getByLabel("source", { exact: true }).selectOption("agent");
  await form.getByLabel("why", { exact: true }).fill("a generated form test");
  r = await run(form);
  expect(r.argv).toContain("--hours=2.5");
  expect(r.argv).toContain("--source=agent");
  expect(r.output).toContain("proposed");
  const inbox = (await get<{ inbox: { text: string }[] }>(page, "/api/daylog")).inbox;
  expect(inbox.some((i) => i.text.includes("Size the e2e work"))).toBe(true);

  // confirm: Run waits for the box; the choice and the note reach the decision
  form = await pick("approvals.request");
  await form.getByLabel("title", { exact: true }).fill("Archive the e2e board");
  const id = (await run(form)).output.match(/approval (\w+) is waiting/)![1];
  form = await pick("approvals.decide");
  await form.getByLabel("id", { exact: true }).fill(id);
  await form.getByLabel("decision", { exact: true }).selectOption("rejected");
  await form.getByLabel("note", { exact: true }).fill("not now");
  await expect(form.getByRole("button", { name: "Run" })).toBeDisabled();
  await form.getByRole("checkbox", { name: /^I understand: it records your decision/ }).check();
  r = await run(form);
  expect(r.output).toContain(`${id} rejected`);
  expect((await action(page, "approvals.list", {})).output).toMatch(new RegExp(`${id}.*rejected`));
});

test("Today: impact notes follow the saved PM lines and reset with the draft", async ({ page }, info) => {
  const { day, draft } = await freshDraft(page);
  const original = (await get<{ pm: { why: { label: string }[] } }>(page, "/api/daylog")).pm.why.map((row) => row.label);
  const labels = ["Verify the synthetic invoice guide", "Review the synthetic fallback path"];
  const edit = `Today:\n• ${labels[0]}\n\nTomorrow:\n• ${labels[1]}`;
  try {
    await page.emulateMedia({ reducedMotion: "reduce" });
    await go(page, "Today");
    const box = page.getByLabel("PM update");
    await expect(box).toHaveValue(draft);
    await box.fill(edit);
    await box.blur();
    await expect(page.getByText("Your edits are saved for this day.")).toBeVisible();
    await page.reload();
    await expect(box).toHaveValue(edit);
    const notes = page.locator("[data-pm-why]");
    await expect(notes).toBeVisible();
    await notes.screenshot({ path: info.outputPath("pm-impact-notes.png"), animations: "disabled" });
    await expect(notes.locator("li strong")).toHaveText(labels);
    await expect(notes.getByRole("listitem").filter({ hasText: "Not sure:" })).toHaveCount(2);
    await page.getByRole("button", { name: "Reset to draft" }).click();
    await expect(box).toHaveValue(draft);
    await page.reload();
    await expect(notes.locator("li strong")).toHaveText(original);
  } finally {
    await post(page, "/api/pm/draft", { day, text: "" });
  }
});
