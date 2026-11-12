import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";

// Issue #57: every input and action shows a loading state while it runs and an error with the fix when it fails.
// Each case holds the request open (to see "working"), then answers 500 (to see the error and its fix).
// The server never sees these requests, so the shared seeded data does not change.

const FAILURE = "synthetic failure for the feedback test";

async function slowFailure(page: Page, url: string, method: "GET" | "POST" = "POST") {
  let release = () => {};
  const gate = new Promise<void>((r) => (release = r));
  await page.route(url, async (route) => {
    if (route.request().method() !== method) return route.fallback();
    await gate;
    await route.fulfill({ status: 500, contentType: "application/json", body: JSON.stringify({ error: FAILURE }) });
  });
  return release;
}

async function expectLoadingThenError(page: Page, release: () => void, working: RegExp, scope = page.locator("main")) {
  await expect(scope.locator(".working").filter({ hasText: working })).toBeVisible();
  release();
  const alert = scope.getByRole("alert").filter({ hasText: FAILURE });
  await expect(alert).toBeVisible();
  await expect(alert).toContainText("Fix:");
  await expect(scope.locator(".working").filter({ hasText: working })).toHaveCount(0);
}

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

test("Dashboard: a failed load shows the error and its fix", async ({ page }) => {
  const release = await slowFailure(page, "**/api/focus/*", "GET");  // each block has its own endpoint (issue #142)
  await page.goto(`/#/review`);
  // The Dashboard opened first: drop the page's cache of it (issue #142), so it loads again.
  await page.evaluate(() => Object.keys(sessionStorage).filter((k) => k.startsWith("careerbrew-cache")).forEach((k) => sessionStorage.removeItem(k)));
  await page.reload();
  await go(page, "Dashboard");
  await expect(page.locator(".skeleton[aria-busy=true]").first()).toBeVisible();
  release();
  await expect(page.getByRole("alert").filter({ hasText: FAILURE }).first()).toContainText("Fix:");
});

test("Standup: a failed load shows the error and its fix", async ({ page }) => {
  const release = await slowFailure(page, "**/api/standup", "GET");
  await go(page, "Standup");
  await expect(page.locator(".skeleton[aria-busy=true]").first()).toBeVisible();
  release();
  await expect(page.getByRole("alert").filter({ hasText: FAILURE })).toContainText("Fix:");
});

test("Today: saving the PM draft shows saving, then the error and its fix", async ({ page }) => {
  await go(page, "Today");
  const release = await slowFailure(page, "**/api/pm/draft");
  await page.getByLabel("PM update").fill("Done today:\n- feedback test");
  await page.getByLabel("PM update").blur();
  await expectLoadingThenError(page, release, /Saving…/);
});

test("Week: adding a task shows adding, then the error and its fix", async ({ page }) => {
  await go(page, "Week");
  const release = await slowFailure(page, "**/api/tasks");
  await page.getByLabel("New task").fill("feedback test task");
  await page.getByRole("button", { name: "Add task" }).click();
  await expect(page.getByRole("button", { name: "Add task" })).toBeDisabled();
  await expectLoadingThenError(page, release, /Adding…/);
  await expect(page.getByLabel("New task")).toHaveValue("feedback test task"); // nothing typed is lost
});

test("Goals: saving a goal shows saving, then the error and its fix", async ({ page }) => {
  await go(page, "Goals");
  const release = await slowFailure(page, "**/api/goals");
  await page.getByLabel("New long-term goal").fill("feedback test goal");
  await page.getByRole("button", { name: "Add", exact: true }).click();
  await expectLoadingThenError(page, release, /Saving…/);
  await expect(page.getByLabel("New long-term goal")).toHaveValue("feedback test goal");
});

test("Teach me: sending feedback shows sending, then the error and its fix", async ({ page }) => {
  await go(page, "Teach me");
  const release = await slowFailure(page, "**/api/teach/feedback");
  await page.getByLabel("Feedback for the teacher").fill("feedback test");
  await page.getByRole("button", { name: "Send feedback" }).click();
  await expectLoadingThenError(page, release, /Sending…/);
});

test("Job hunt: choosing a job shows saving, then the error and its fix", async ({ page }) => {
  await go(page, "Job hunt");
  const release = await slowFailure(page, "**/api/jobs/**");
  await page.getByRole("button", { name: /^(Choose .* as dream|Remove)$/ }).first().click();
  await expectLoadingThenError(page, release, /Saving your choice…/);
});

test("Interviews: grading a typed answer shows grading, then the error and its fix", async ({ page }) => {
  await go(page, "Interviews");
  const release = await slowFailure(page, "**/api/drill/voice");
  await page.getByLabel("Typed answer").fill("a typed answer");
  await page.getByRole("button", { name: "Grade typed answer" }).click();
  await expectLoadingThenError(page, release, /Grading…/);
});

test("Practice: grading a card shows saving, then the error and its fix", async ({ page }) => {
  await go(page, "Practice");
  const release = await slowFailure(page, "**/api/cards/*/review");
  await page.getByRole("button", { name: "Show answer (Space)" }).click();
  await page.getByRole("button", { name: "3 Good" }).click();
  await expectLoadingThenError(page, release, /Saving…/);
});

test("Library: reporting a problem shows sending, then the error and its fix", async ({ page }) => {
  await go(page, "Library");
  await page.getByLabel("Filter pages").fill("binary search");
  await page.getByRole("link", { name: "How does binary search work?" }).click();
  const release = await slowFailure(page, "**/api/explanations/*/flag");
  await page.getByText("Report a problem with this explanation").click();
  await page.getByLabel("What is wrong").fill("feedback test");
  await page.getByRole("button", { name: "Report", exact: true }).click();
  await expectLoadingThenError(page, release, /Sending…/);
});

test("Actions: running an action shows running, then the error and its fix", async ({ page }) => {
  await go(page, "Actions");
  await page.getByLabel("Filter actions").fill("config.set");
  await page.getByRole("button", { name: "config.set" }).click();
  await page.getByLabel("key").fill("questions_per_day");
  await page.getByLabel("value").fill("4");
  const release = await slowFailure(page, "**/api/actions/*");
  await page.getByRole("button", { name: "Run" }).click();
  await expectLoadingThenError(page, release, /Running…/);
});

test("Setup: a watcher action and a setting show working, then the error and its fix", async ({ page }) => {
  await go(page, "Setup");
  let release = await slowFailure(page, "**/api/watch/*");
  const watcher = page.getByRole("region", { name: "Day watcher" });
  await watcher.getByRole("button").first().click();
  await expectLoadingThenError(page, release, /Working…/, watcher);
  release = await slowFailure(page, "**/api/config");
  await page.getByRole("region", { name: "Settings" }).getByRole("button", { name: "Change" }).first().click();
  await page.getByRole("region", { name: "Settings" }).getByRole("button", { name: "Save" }).click();
  await expectLoadingThenError(page, release, /Saving…/, page.getByRole("region", { name: "Settings" }));
});

test("Reports: a failed load shows the error and its fix", async ({ page }) => {
  const release = await slowFailure(page, "**/api/reports", "GET");
  await go(page, "Reports");
  await expect(page.locator(".skeleton[aria-busy=true]").first()).toBeVisible();
  release();
  await expect(page.getByRole("alert").filter({ hasText: FAILURE })).toContainText("Fix:");
});

test("a stopped server names the command that starts it", async ({ page }) => {
  await go(page, "Week");
  await page.route("**/api/tasks", (route) => (route.request().method() === "POST" ? route.abort("connectionrefused") : route.fallback()));
  await page.getByLabel("New task").fill("offline task");
  await page.getByRole("button", { name: "Add task" }).click();
  await expect(page.getByRole("alert")).toContainText("careerbrew is not answering. Fix: start it with uv run careerbrew");
  await expect(page.getByRole("alert").locator("code").first()).toHaveText("uv run careerbrew");  // a command shows as code, not backticks (US-75)
});
