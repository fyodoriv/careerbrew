import { expect, test } from "@playwright/test";
import type { APIRequestContext, Page, Request } from "@playwright/test";
import { OUT, TOKEN, TRACE_IN_SANDBOX } from "../playwright.config";
import type { TimeSaved } from "../src/api";

// US-86, time-saved-user-time: Copy update, Copy standup, and Accept each log the owner's own seconds from the
// result's open (`POST /api/timing`, ids and seconds only), `careerbrew metrics time-saved` counts them as web runs,
// and Dashboard's Time saved block shows them. The accepted suggestion is synthetic and answered in the page, so
// the shared seeded server keeps every suggestion the other specs act on.

const HEADERS = { "X-Careerbrew-Token": TOKEN };

test.use({ permissions: ["clipboard-read", "clipboard-write"] });

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

// Web runs per action over all weeks (the event stamps are wall-clock UTC; the server's clock is pinned).
async function webRuns(request: APIRequestContext): Promise<Record<string, number>> {
  const d = (await (await request.get("/api/time-saved", { headers: HEADERS })).json()) as TimeSaved;
  const out: Record<string, number> = { pm_update: 0, standup: 0, inbox: 0 };
  for (const w of d.weeks) for (const [a, c] of Object.entries(w.actions)) if (a in out) out[a] += c.runs_web;
  return out;
}

const timing = (page: Page, step: string) => page.waitForRequest((r: Request) => r.url().endsWith("/api/timing") && r.method() === "POST" && r.postDataJSON().step === step);

test("Copy update, Copy standup, and Accept log your own seconds, and Dashboard shows the minutes saved", async ({ page }) => {
  const before = await webRuns(page.request);

  // PM update: the open, then Copy update; only the action id, the step, and the seconds leave the page.
  const opened = timing(page, "open");
  await page.goto(`/#/today&token=${TOKEN}`);
  expect((await opened).postDataJSON()).toMatchObject({ step: "open" });
  const copied = timing(page, "copy");
  await page.getByRole("button", { name: "Copy update", exact: true }).click();
  const pm = (await copied).postDataJSON() as Record<string, unknown>;
  expect(Object.keys(pm).sort()).toEqual(["saves", "seconds", "step"]);
  expect(pm.saves).toBe("pm_update");
  expect(typeof pm.seconds).toBe("number");

  // Standup: Copy standup copies each item's talking point, and its time is logged.
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Standup", exact: true }).click();
  const standupCopy = timing(page, "copy");
  await page.getByRole("button", { name: "Copy standup" }).click();
  expect((await standupCopy).postDataJSON()).toMatchObject({ saves: "standup", step: "copy" });
  await expect(page.getByRole("status").filter({ hasText: "Copied" })).toBeVisible();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toMatch(/^• Order export\n {2}– /);

  // Inbox: Accept on a synthetic suggestion, answered in the page.
  await page.route("**/api/inbox", async (route) => {
    const res = await route.fetch();
    const d = (await res.json()) as { inbox: unknown[] };
    await route.fulfill({ response: res, json: { ...d, inbox: [{ id: "synthetic-time", type: "proposal", kind: "agent-task", text: "Synthetic timed suggestion", status: "open" }, ...d.inbox] } });
  });
  await page.route("**/api/inbox/synthetic-time/accept", async (route) => {
    const res = await route.fetch({ url: route.request().url().replace(/\/api\/inbox\/.*$/, "/api/daylog"), method: "GET", postData: undefined });
    await route.fulfill({ response: res });
  });
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Today", exact: true }).click();
  const row = page.locator('main [data-block="suggestions"] li[data-inbox]', { hasText: "Synthetic timed suggestion" });
  const accepted = timing(page, "accept");
  await row.getByRole("button", { name: "Accept" }).click();
  expect((await accepted).postDataJSON()).toMatchObject({ saves: "inbox", step: "accept" });

  // The real effect: each action has one more run timed in the web app.
  await expect.poll(() => webRuns(page.request)).toEqual({ pm_update: before.pm_update + 1, standup: before.standup + 1, inbox: before.inbox + 1 });

  // Dashboard: the Time saved block shows the week's minutes, and which runs used your time.
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: "Dashboard", exact: true }).click();
  const block = page.getByRole("region", { name: "Time saved" });
  await expect(block.locator("[data-this-week]")).toContainText(/min saved this week/);
  await expect(block.getByRole("table")).toContainText(/\d+ of \d+ timed in this app/);
  await block.scrollIntoViewIfNeeded();
  await page.screenshot({ path: `${OUT}/time-saved.png`, fullPage: true });
});

test("the timing endpoint refuses anything but an action id, a step, and seconds", async ({ page }) => {
  for (const data of [{ saves: "standup", step: "copy", seconds: "Shipped the ledger" }, { saves: "lunch", step: "copy", seconds: 1 }, { saves: "standup", step: "paste", seconds: 1 }]) {
    const r = await page.request.post("/api/timing", { headers: HEADERS, data });
    expect(r.status()).toBe(400);
  }
});
