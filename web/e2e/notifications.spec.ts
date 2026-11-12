import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { NOW, OUT, TOKEN } from "../playwright.config";

// Issue #74: every notification is actionable in the web app, with the same actions as a clickable
// pop-up (Open, Accept or Dismiss for a suggestion, Snooze), and a pop-up click's deep link lands on it.
// The seed (tests/e2e/web-seed-and-serve.sh) opts in and adds a page, a DM, and an agent's task suggestion.
// This file runs after app.spec.ts and changes the shared seeded data.

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  await page.goto(`/#token=${TOKEN}`);
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

const go = (page: Page, name: string) => page.getByRole("navigation", { name: "Views" }).getByRole("link", { name, exact: true }).click();
const agentTask = (page: Page) => page.locator('[data-kind="suggestion"]').filter({ hasText: "an agent proposes a task" });

test("a pop-up click's deep link opens that notification, and the token leaves the address bar", async ({ page }) => {
  await go(page, "Notifications");
  const pageAlert = page.locator('[data-kind="page"]');
  await expect(pageAlert).toContainText("PagerDuty: a page is assigned to you");
  await expect(pageAlert).toContainText("Page: checkout latency over SLO");  // the title shows only here, in your local app
  await expect(pageAlert).toContainText("acknowledge it in PagerDuty, then fix it or hand it off");
  const id = await pageAlert.getAttribute("data-notification");
  // What `careerbrew notify open <id>` (a pop-up click) opens: a fresh tab with the link from `careerbrew open`.
  await page.goto("about:blank");
  await page.goto(`/#/notifications/${id}&token=${TOKEN}`);
  await expect(page.getByRole("heading", { level: 1, name: "Notifications" })).toBeVisible();
  await expect(page.locator(`[data-notification="${id}"]`)).toHaveAttribute("aria-current", "true");
  expect(page.url()).not.toContain("token=");
  await page.screenshot({ path: `${OUT}/notifications.png`, fullPage: true });
});

const HEADERS = { "X-Careerbrew-Token": TOKEN };
const statusOf = async (page: Page, id: string) => {
  const res = await page.request.get("/api/notifications", { headers: HEADERS });
  return ((await res.json()) as { items: { id: string; status: string }[] }).items.find((n) => n.id === id)?.status;
};
async function action(page: Page, name: string, values: Record<string, unknown>) {
  const res = await page.request.post(`/api/actions/${name}`, { headers: HEADERS, data: { values, confirm: false } });
  const out = (await res.json()) as { exit: number; output: string };
  expect(out.exit, out.output).toBe(0);
}

test("Open in a tool, or See it in Today, marks a notification read (US-72)", async ({ page }) => {
  await page.context().route("https://pagerduty.example.com/**", (route) => route.fulfill({ contentType: "text/html", body: "<title>PagerDuty</title>" }));
  await go(page, "Notifications");
  const pageAlert = page.locator('[data-kind="page"]');
  const id = (await pageAlert.getAttribute("data-notification"))!;
  const popup = page.waitForEvent("popup");
  await pageAlert.getByRole("link", { name: "Open in PagerDuty" }).click();
  expect((await popup).url()).toBe("https://pagerduty.example.com/incidents/PSEED1");
  await expect(pageAlert.locator(".notification-head .muted")).toHaveText("read");
  expect(await statusOf(page, id)).toBe("read");

  const taskId = (await agentTask(page).getAttribute("data-notification"))!;
  await agentTask(page).getByRole("link", { name: "See it in Today" }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
  await expect.poll(() => statusOf(page, taskId)).toBe("read");
});

test("accept a suggestion, snooze a DM, dismiss a page: every notification is actionable here", async ({ page }) => {
  await go(page, "Notifications");
  await expect(page.getByText(/^On: priority 3 or more/)).toBeVisible();
  const dm = page.locator('[data-kind="dm"]');
  await expect(dm.getByRole("link", { name: "Open in Slack" })).toHaveAttribute("target", "_blank");
  await dm.getByRole("button", { name: "Snooze" }).click();
  await expect(dm).toContainText(/snoozed until \d\d:\d\d/);

  await expect(agentTask(page)).toContainText("Review the rollout checklist");
  await agentTask(page).getByRole("button", { name: "Accept" }).click();
  const closed = page.getByRole("region", { name: /^Closed/ });
  await expect(closed).toContainText("accepted");

  await page.locator('[data-kind="page"]').getByRole("button", { name: "Dismiss" }).click();
  await expect(closed).toContainText("dismissed");
  await expect(page.getByRole("region", { name: /^Open/ }).locator('[data-kind="page"]')).toHaveCount(0);

  // Accept decided the suggestion in the inbox too: Today no longer offers it.
  await go(page, "Today");
  await expect(page.getByRole("region", { name: /careerbrew suggests/ })).toBeVisible();
  await expect(page.getByRole("region", { name: /careerbrew suggests/ })).not.toContainText("Review the rollout checklist");
});

test("Turn on switches notifications back on from the view (US-72)", async ({ page }) => {
  await page.request.post("/api/config", { headers: HEADERS, data: { key: "notify_events", value: "false" } });
  await go(page, "Notifications");
  await expect(page.getByText(/^Notifications are off/)).toBeVisible();
  await page.getByRole("button", { name: "Turn on" }).click();
  await expect(page.getByText(/^On: priority 3 or more/)).toBeVisible();
  const res = await page.request.get("/api/config", { headers: HEADERS });
  expect(((await res.json()) as { settings: { key: string; value: unknown }[] }).settings.find((s) => s.key === "notify_events")?.value).toBe(true);
});

test("Mark read closes a notification that has no link (US-72)", async ({ page }) => {
  // A direct message with no link to open: careerbrew notifies it, and Mark read is the only way to mark it read.
  const file = join(mkdtempSync(join(tmpdir(), "careerbrew-e2e-")), "dm.jsonl");
  writeFileSync(file, JSON.stringify({ id: "slack:D9:e2e-no-link", source: "slack", kind: "dm", at: `${NOW.slice(0, 10)}T08:50:00`, title: "DM from Lee: no link here", actor: "other", to_me: true, thread: "slack:D9:e2e" }) + "\n");
  await action(page, "work.feed.add", { file });
  await action(page, "notify.check", {});
  await go(page, "Notifications");
  const dm = page.locator("[data-notification]").filter({ hasText: "DM from Lee: no link here" });
  await expect(dm.getByRole("link")).toHaveCount(0);
  await dm.getByRole("button", { name: "Mark read" }).click();
  await expect(dm.getByRole("button", { name: "Mark read" })).toHaveCount(0);
  await expect(dm.locator(".notification-head .muted")).toHaveText("read");
  expect(await statusOf(page, (await dm.getAttribute("data-notification"))!)).toBe("read");
});
