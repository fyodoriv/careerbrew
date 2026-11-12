import { expect, test } from "@playwright/test";
import type { Locator, Page } from "@playwright/test";
import { NOW, OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import { shortDay } from "../src/dates";

// US-70 (issue #143): every task row shows the task's links and its own why. The seed gives its top task
// "Fix the flaky checkout test" a file, a labeled ticket, a channel to post in, and a javascript: value
// (tests/e2e/web-seed-and-serve.sh). Only http(s) links become anchors, and each opens in a new tab.

const TASK = "Fix the flaky checkout test";
const FILE = "https://code.example.com/acme/shop/src/branch/main/tests/test_checkout.py";
const TICKET = "https://jira.example.com/browse/SHOP-12";
const CHANNEL = "https://slack.example.com/archives/C0CHECKOUT";
const CI = "https://ci.example.com/builds/812";
const TODAY = NOW.slice(0, 10);
const isWorkday = (day: string) => ![0, 6].includes(new Date(`${day}T00:00:00Z`).getUTCDay());  // the seed keeps the default work_days

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

async function expectTaskLinks(row: Locator, why = true) {
  const links = row.getByRole("list", { name: "Links" });
  for (const [name, href] of [["code.example.com/…/test_checkout.py", FILE], ["ticket", TICKET], ["post in #checkout", CHANNEL]]) {
    const a = links.getByRole("link", { name, exact: true });
    await expect(a).toHaveAttribute("href", href);
    await expect(a).toHaveAttribute("target", "_blank");
    await expect(a).toHaveAttribute("rel", "noreferrer noopener");
  }
  await expect(links.getByText("javascript:alert(1)")).toBeVisible();  // shown, but as text
  await expect(links.getByRole("link")).toHaveCount(3);
  if (why) {
    await expect(row).toContainText("Context: CI failed twice:");
    await expect(row.getByRole("link", { name: CI })).toHaveAttribute("target", "_blank");
  }
}

async function noScriptLinks(page: Page) {
  const hrefs = await page.locator("a[href]").evaluateAll((as) => as.map((a) => a.getAttribute("href") ?? ""));
  expect(hrefs.filter((h) => /^\s*(javascript|data|file|vbscript):/i.test(h))).toEqual([]);
}

test("a task's links and its own why open in a new tab on Dashboard, Today, and Week; a javascript: value stays text", async ({ page }) => {
  await expect(page.getByRole("heading", { level: 1, name: "Dashboard" })).toBeVisible();
  if (isWorkday(TODAY)) {
    const today = page.locator('[data-horizon="today"] li').filter({ hasText: TASK });
    await expectTaskLinks(today);
    await expect(today.locator(".why")).toContainText("score");  // the computed score line stays beside it
  }
  const tomorrow = page.getByRole("region", { name: "Tomorrow" }).locator("li").filter({ hasText: TASK });
  // On a workday today's plan holds the task, so Tomorrow does not list it again (US-75 pictures); on a day off it is tomorrow's.
  if (isWorkday(TODAY)) await expect(tomorrow).toHaveCount(0);
  else await expectTaskLinks(tomorrow, false);
  await noScriptLinks(page);
  await page.screenshot({ path: `${OUT}/task-links-dashboard.png`, fullPage: true });

  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
  await expect(page.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
  if (isWorkday(TODAY)) {
    await expectTaskLinks(page.getByRole("region", { name: `Plan for ${shortDay(TODAY)}`, exact: true }).locator("li.block-task").filter({ hasText: TASK }));
    const next = page.getByRole("region", { name: "Next step" });
    await expect(next).toContainText(TASK);
    await expectTaskLinks(next);
  }
  await noScriptLinks(page);
  await page.screenshot({ path: `${OUT}/task-links-today.png`, fullPage: true });

  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Week", exact: true }).click();
  await expectTaskLinks(page.locator("article li.block-task").filter({ hasText: TASK }).first());
  await noScriptLinks(page);
  const ticket = page.locator("article li.block-task").filter({ hasText: TASK }).first().getByRole("link", { name: "ticket", exact: true });
  await page.context().route("https://jira.example.com/**", (r) => r.fulfill({ contentType: "text/html", body: "<title>SHOP-12</title>" }));  // no real network
  const [tab] = await Promise.all([page.context().waitForEvent("page"), ticket.click()]);  // a new tab; the app stays open
  await tab.waitForLoadState();
  expect(tab.url()).toBe(TICKET);
  await tab.close();
  await expect(page.getByRole("heading", { level: 1, name: "Week" })).toBeVisible();
});
