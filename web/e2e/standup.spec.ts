import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import type { Standup } from "../src/api";
import { NOW, OUT, TOKEN } from "../playwright.config";
import { finishStandup } from "./standup-teardown";

// Issue #110: the standup report since the last standup. The seed (tests/e2e/web-seed-and-serve.sh) keeps the
// default standup (Monday to Friday, 10:00, 15 minutes) and writes its notes at the pinned clock NOW. It passes three
// rows: one yours, one of Sam's that a note of yours matches by a whole word, and one of Ana's that nothing matches.

// The window text, derived from NOW so the test passes on any pinned day: since the last standup that has ended.
// NOW is a wall time in the standup zone; UTC arithmetic keeps it a wall time here. Format: "Mon Oct 19, 09:00".
function label(d: Date): string {
  const [wd, mon] = [{ weekday: "short" }, { month: "short" }].map((o) => d.toLocaleDateString("en-US", { ...o, timeZone: "UTC" } as Intl.DateTimeFormatOptions));
  return `${wd} ${mon} ${String(d.getUTCDate()).padStart(2, "0")}, ${d.toISOString().slice(11, 16)}`;
}
function standupWindow(): { since: string; until: string } {
  const now = new Date(`${NOW.slice(0, 16)}:00Z`);
  const start = new Date(now);
  start.setUTCHours(10, 0, 0, 0);
  start.setUTCDate(start.getUTCDate() - 1); // issue #200: the previous standup day, never today's own
  const ended = (d: Date) => ![0, 6].includes(d.getUTCDay()) && d.getTime() + 15 * 60_000 <= now.getTime();
  while (!ended(start)) start.setUTCDate(start.getUTCDate() - 1);
  return { since: label(start), until: label(now) };
}

test.use({ timezoneId: "Etc/GMT+4", permissions: ["clipboard-read", "clipboard-write"] });

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  await page.goto(`/#token=${TOKEN}`);
});

test.afterEach(async ({ page }) => {
  await finishStandup(page, (page as unknown as { __errors: string[] }).__errors);
});

const go = (page: Page, name: string) => page.getByRole("navigation", { name: "Views" }).getByRole("link", { name, exact: true }).click();
const parts = (page: Page, row: string) => page.getByRole("region", { name: row }).locator(":scope > p, :scope > ul").evaluateAll((els) => els.map((e) => e.className.split(" ").find((c) => c.startsWith("standup-"))));

test("each row: a talking point first, then its status, then the evidence; other people's rows only when your work matched", async ({ page }) => {
  await go(page, "Standup");
  const window = page.locator(".standup-window");
  await expect(window).toContainText(`Since ${standupWindow().since}`);
  await expect(window).toContainText(`until ${standupWindow().until}`);
  const mine = page.getByRole("region", { name: "Order export" });
  await expect(mine.locator(".standup-talk")).toHaveText("Shipped order export (1 note).");
  await expect(mine.locator(".standup-status")).toHaveText("Status: In review · Owner: you");
  await expect(mine.locator(".standup-action")).toContainText("feat: shipped order export");
  await expect(mine.locator(".standup-action")).toContainText('via word "order export"');
  expect(await parts(page, "Order export")).toEqual(["standup-talk", "standup-status", "standup-evidence"]);
  const sam = page.getByRole("region", { name: "Checkout reliability" });
  await expect(sam.locator(".standup-talk")).toHaveText("Led the checkout incident review (1 note).");
  await expect(sam.locator(".standup-status")).toHaveText("Status: In progress · Owner: Sam Lee");
  await expect(page.getByRole("region", { name: "Search ranking" })).toHaveCount(0);
  await expect(page.getByText("1 row with no update hidden.")).toBeVisible();
  // issue #271: when the rows were passed (the seed passes them at the pinned clock)
  await expect(page.locator(".standup-updated")).toHaveText(`updated ${NOW.slice(11, 16)}`);
  await page.screenshot({ path: `${OUT}/standup.png`, fullPage: true });
});

// Issue #229: the seed passes an agent's talking point and a Slack thread for "Rollout plan", a row of yours with no
// other work. The talking point replaces the generated line and says who wrote it; the thread joins the evidence.
test("an agent's talking point replaces the row's line, and an action an agent passed joins its evidence", async ({ page }) => {
  await go(page, "Standup");
  const row = page.getByRole("region", { name: "Rollout plan" });
  await expect(row.locator(".standup-talk")).toHaveText("Agreed the rollout order with support in Slack. Next: the first region goes live on Wednesday. Nothing blocks it. written by an agent");
  await expect(row.locator(".standup-by")).toHaveText("written by an agent");
  await expect(row.getByText("No actions of yours since the last standup.")).toHaveCount(0);
  const thread = row.locator(".standup-action");
  await expect(thread).toHaveCount(1);
  await expect(thread).toHaveAttribute("data-kinds", "slack");
  await expect(thread.locator(".tag").first()).toHaveText("thread");
  await expect(thread.getByRole("link", { name: "Agreed the rollout order with support" })).toHaveAttribute("href", "https://slack.example.com/archives/C0ROLLOUT/p1");
  await expect(thread).toContainText("via an agent");
  expect(await parts(page, "Rollout plan")).toEqual(["standup-talk", "standup-status", "standup-evidence"]);
  // a row whose line careerbrew wrote says nothing about an agent
  await expect(page.getByRole("region", { name: "Order export" }).locator(".standup-by")).toHaveCount(0);
});

// Issue #271: with live proofs passed (`work standup --proof`), each row lists the proofs it passed, linked; a row that
// failed never reaches the view, and when none passed the view says so. The answer is patched: the seed passes no proofs.
test("a row lists the live proofs it passed, and the view says when no row passed the proof check", async ({ page }) => {
  const proof = { source: "github", url: "https://github.example.com/acme/shop/pull/12", checked_at: `${NOW.slice(0, 10)}T08:50:00`, quote: "Merged: add the order export", when: "08:50" };
  let mode: "pass" | "none" = "pass";
  await page.route((u) => u.pathname === "/api/standup", async (route) => {
    const body = await (await route.fetch()).json();
    const gated = { ...body.counts, gated: true, passed: mode === "pass" ? 1 : 0, failed: mode === "pass" ? 0 : 3 };
    const rows = mode === "pass" ? [{ ...body.rows.find((r: { title: string }) => r.title === "Order export"), proofs: [proof] }] : [];
    await route.fulfill({ json: { ...body, rows, counts: gated } });
  });
  await go(page, "Standup");
  const row = page.getByRole("region", { name: "Order export" });
  await expect(row.locator(".standup-proof")).toHaveText("Checked 08:50 in github: Merged: add the order export");
  await expect(row.getByRole("link", { name: "Merged: add the order export" })).toHaveAttribute("href", proof.url);
  await expect(page.getByRole("region", { name: "Checkout reliability" })).toHaveCount(0);
  mode = "none";
  await page.reload();
  await go(page, "Standup");
  await expect(page.getByRole("status").filter({ hasText: "No row passed the proof check." })).toBeVisible();
  await expect(page.getByRole("region", { name: "Order export" })).toHaveCount(0);
});


test("Standup has one context subheading and Copy beside its title, with three nested speaking bullets", async ({ page }) => {
  const points = ["Shipped the API (2 PRs)", "Next: the CSV format", "Ask: a review from the data team"];
  await page.route("**/api/standup", async (route) => {
    const response = await route.fetch();
    const body = await response.json() as Standup;
    await route.fulfill({ response, json: { ...body, rows: [{ ...body.rows[0], talking_point: points.join(" "), talking_points: points }], groups: [] } });
  });
  await go(page, "Standup");
  const title = page.getByRole("heading", { name: "Standup", level: 1 });
  const header = page.locator("main .page-head");
  await expect(header).toContainText("Standup");
  await expect(header.getByRole("button", { name: "Copy standup" })).toBeVisible();
  await expect(title).toHaveCount(1);
  await expect(page.locator(".standup-window")).toHaveCount(1);
  await expect(page.locator(".standup-window")).not.toContainText("Etc/GMT+4");
  await expect(page.getByRole("heading", { name: "Report window" })).toHaveCount(0);
  await expect(page.getByText("Also as a file")).toHaveCount(0);
  await expect(page.locator("main")).not.toContainText("out/standup.html");
  await expect(page.locator("main [data-block]").first()).toHaveAttribute("data-block", "rows");
  const row = page.getByRole("region", { name: "Order export" });
  await expect(row.locator("ul.standup-talk > li")).toHaveText(points);
  await page.getByRole("button", { name: "Copy standup" }).click();
  await expect(page.getByRole("status").filter({ hasText: "Copied" })).toBeVisible();
  await expect.poll(() => page.evaluate(() => navigator.clipboard.readText())).toBe("• Order export\n  – Shipped the API (2 PRs)\n  – Next: the CSV format\n  – Ask: a review from the data team");
});

test("the subheading names the report zone when the browser differs", async ({ browser, page }) => {
  const utc = await browser.newContext({ timezoneId: "UTC", baseURL: new URL(page.url()).origin });
  const other = await utc.newPage();
  const errors: string[] = [];
  other.on("console", (m) => m.type() === "error" && errors.push(m.text()));
  other.on("pageerror", (e) => errors.push(e.message));
  try {
    await other.goto(`/#token=${TOKEN}`);
    await go(other, "Standup");
    await expect(other.locator(".standup-window")).toContainText("Etc/GMT+4");
    expect(errors).toEqual([]);
  } finally {
    await utc.close();
  }
});
