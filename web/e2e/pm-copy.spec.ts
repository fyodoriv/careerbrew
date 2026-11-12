import { expect, test } from "@playwright/test";
import type { Page } from "@playwright/test";
import { NOW, OUT, TOKEN } from "../playwright.config";
import { longDay } from "../src/dates";

// US-59, issue #145: a Copy update button next to the day's heading, at the top of Today and Dashboard, copies the
// PM update in one click: the body only when pm_send_via is slack, the subject line first when it is email.
// Bullets and indentation stay, with delimited URLs in the plain fallback.

const TODAY = NOW.slice(0, 10);
const HEADERS = { "X-Careerbrew-Token": TOKEN };

test.use({ permissions: ["microphone", "clipboard-read", "clipboard-write"] });

test.beforeEach(async ({ page }) => {
  const errors: string[] = [];
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
  (page as unknown as { __errors: string[] }).__errors = errors;
  // Start each test from the generated draft. The suite shares one seeded server, and app.spec.ts runs first and
  // leaves its own PM edit saved for the day; an empty text resets the day to the generated draft.
  const day = ((await (await page.request.get("/api/daylog", { headers: HEADERS })).json()) as { log: { day: string } }).log.day;
  const reset = await page.request.post("/api/pm/draft", { headers: HEADERS, data: { day, text: "" } });
  expect(reset.status()).toBe(200);
});

test.afterEach(async ({ page }) => {
  expect((page as unknown as { __errors: string[] }).__errors).toEqual([]);
});

async function setVia(page: Page, via: "slack" | "email") {
  const r = await page.request.post("/api/config", { headers: HEADERS, data: { key: "pm_send_via", value: via } });
  expect(r.status()).toBe(200);
}

async function open(page: Page, view: "Today" | "Dashboard") {
  await page.goto(`/#token=${TOKEN}`);
  await page.getByRole("navigation", { name: "Views" }).getByRole("link", { name: view, exact: true }).click();
  await expect(page.getByRole("heading", { level: 2, name: longDay(TODAY) })).toBeVisible();
}

const clipboard = (page: Page) => page.evaluate(() => navigator.clipboard.readText());
const copyButton = (page: Page) => page.getByRole("button", { name: "Copy update", exact: true });

test("Copy update on Today copies the body without the subject for Slack, as Slack-ready text", async ({ page }) => {
  await setVia(page, "slack");
  await open(page, "Today");
  await expect(page.locator("[data-pm-source]")).toHaveText(/^generated draft · updated \d\d:\d\d$/);
  const body = await page.getByLabel("PM update").inputValue();
  expect(body).toMatch(/^Other: next: /);  // the seeded day's generated draft
  await copyButton(page).click();  // one click
  await expect(page.getByRole("status").filter({ hasText: "Copied, paste it in Slack" })).toBeVisible();
  const copied = await clipboard(page);
  expect(copied).toBe(body.replace(/\s+$/, ""));
  expect(copied).not.toContain("Daily update");
  await page.screenshot({ path: `${OUT}/copy-update.png` });

  // An edit with bullets, indentation, and a markdown link is copied as Slack-ready text, even before its save lands.
  const edit = "• billing: [close the ledger](https://jira.example.com/browse/BILL-7)\n    ◦ next: backfill old invoices";
  await page.getByLabel("PM update").fill(edit);
  await copyButton(page).click();
  await expect.poll(() => clipboard(page)).toBe("• billing: close the ledger <https://jira.example.com/browse/BILL-7>\n    ◦ next: backfill old invoices");
  await expect(page.locator("[data-pm-source]")).toHaveText(/^your edit · updated \d\d:\d\d$/);  // saved on blur
  await page.getByRole("button", { name: "Reset to draft" }).click();
  await expect(page.locator("[data-pm-source]")).toHaveText(/^generated draft · /);
});

test("Copy update on Dashboard puts the subject first for email", async ({ page }) => {
  await setVia(page, "email");
  try {
    await open(page, "Dashboard");
    const pm = (await (await page.request.get("/api/daylog", { headers: HEADERS })).json()).pm as { subject: string; body: string };
    expect(pm.subject).toBe(`Daily update ${TODAY}`);
    await copyButton(page).click();
    await expect(page.getByRole("status").filter({ hasText: "Copied, paste it in your email" })).toBeVisible();
    expect(await clipboard(page)).toBe(`Daily update ${TODAY}\n\n${pm.body.replace(/\s+$/, "")}`);
  } finally {
    await setVia(page, "slack");
  }
});

test("Copy update shows the text selected to copy by hand when the browser refuses the clipboard", async ({ page }) => {
  await page.addInitScript(() => {
    navigator.clipboard.writeText = () => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError"));
  });
  await open(page, "Today");
  await copyButton(page).click();
  await expect(page.getByRole("alert").filter({ hasText: "The browser did not allow the copy" })).toBeVisible();
  const box = page.getByLabel("Update to copy");
  await expect(box).toHaveValue(/^Other: next: /);
  await expect.poll(() => box.evaluate((el: HTMLTextAreaElement) => [el.selectionStart, el.selectionEnd, el.value.length])).toEqual(
    await box.evaluate((el: HTMLTextAreaElement) => [0, el.value.length, el.value.length]),
  );
});

test("each line's why-it-matters note shows beside the draft and is never copied (task pm-why-it-matters)", async ({ page }) => {
  await setVia(page, "slack");
  await open(page, "Today");
  const notes = page.locator("[data-pm-why]");
  await expect(notes).toContainText("for you only: never copied or sent");
  await expect(notes.getByRole("listitem").first()).toHaveText("Other: Not sure: helps the platform team; expected: fewer queue questions in design review.");
  await copyButton(page).click();
  await expect(page.getByRole("status").filter({ hasText: "Copied, paste it in Slack" })).toBeVisible();
  const copied = await clipboard(page);
  expect(copied).toMatch(/^Other: next: /);
  expect(copied).not.toContain("platform team");
  expect(await page.getByLabel("PM update").inputValue()).not.toContain("platform team");
});

test("an edit with a bare PR number is not saved, and the error names the line (issue #227)", async ({ page }) => {
  await setVia(page, "slack");
  await open(page, "Today");
  const box = page.getByLabel("PM update");
  await box.fill("billing: closed the ledger (#12).\ntooling: faster sync ([#9](https://github.example.com/o/r/pull/9)).");
  await box.blur();
  const alert = page.getByRole("alert");
  await expect(alert).toContainText("1 line(s) name a PR or ticket with no link");
  await expect(alert).toContainText("billing: closed the ledger (#12).");
  await expect(alert).not.toContainText("tooling:");
  await expect(page.locator("[data-pm-source]")).toHaveText(/^generated draft · /);  // nothing was saved
  await box.fill("billing: closed the ledger ([#12](https://github.example.com/o/r/pull/12)).");
  await box.blur();
  await expect(page.locator("[data-pm-source]")).toHaveText(/^your edit · /);  // a linked ref saves
});

test("an edit with a time estimate shows a content check for the owner, and still saves (issue #226)", async ({ page }) => {
  await setVia(page, "slack");
  await open(page, "Today");
  const box = page.getByLabel("PM update");
  const completed: { day: string; text: string; status: number }[] = [];
  page.on("response", (response) => {
    const request = response.request();
    if (request.method() !== "POST" || !new URL(response.url()).pathname.endsWith("/api/pm/draft")) return;
    const body = request.postDataJSON() as { day: string; text: string };
    completed.push({ ...body, status: response.status() });
  });
  await box.fill("billing: closed the ledger; next: backfill old invoices, ETA Friday.");
  await box.blur();
  await expect(page.locator("[data-pm-source]")).toHaveText(/^your edit · /);  // a check never blocks a save
  await page.reload();
  const checks = page.locator("[data-pm-checks]");
  await expect(checks).toContainText("no time estimates");
  await expect(checks).toContainText("ETA Friday");
  const corrected = "billing: closed the ledger; next: backfill old invoices.";
  let held = 0;
  let forwarded = 0;
  await page.route("**/api/pm/draft", async (route) => {
    const request = route.request();
    if (request.method() !== "POST") return route.continue();
    const body = request.postDataJSON() as { day: string; text: string };
    if (body.day === TODAY && body.text === corrected) {
      held += 1;
      await new Promise((resolve) => setTimeout(resolve, 500));
      await route.continue();
      forwarded += 1;
      return;
    }
    await route.continue();
  });
  await box.fill(corrected);
  const saved = page.waitForResponse((response) => {
    const request = response.request();
    if (request.method() !== "POST" || !new URL(response.url()).pathname.endsWith("/api/pm/draft")) return false;
    const body = request.postDataJSON() as { day: string; text: string };
    return body.day === TODAY && body.text === corrected;
  });
  await box.blur();
  expect((await saved).ok()).toBe(true);
  await page.reload();
  await expect.poll(() => held).toBe(1);
  await expect(page.locator('[data-block="pm-update"]')).toHaveAttribute("data-daylog-fresh", "true");
  await expect(box).toHaveValue(corrected);
  await expect(page.locator("[data-pm-checks]")).toHaveCount(0);
  expect(forwarded).toBe(1);
  expect(completed).toEqual([
    { day: TODAY, text: "billing: closed the ledger; next: backfill old invoices, ETA Friday.", status: 200 },
    { day: TODAY, text: corrected, status: 200 },
  ]);
});

type CopyPolicy = "accept" | "missing-rich" | "reject-rich" | "reject-both";
type CopyAttempt = { method: "plain" | "rich"; accepted: boolean; plain?: string; html?: string };
type CopyRecorder = { policy: CopyPolicy; attempts: CopyAttempt[] };
type CopyWindow = Window & { issue376Clipboard: CopyRecorder };

const COPY_WORLD = process.env.ISSUE376_WORLD ?? "after";
if (COPY_WORLD !== "before" && COPY_WORLD !== "after") throw new Error("BLOCKED: unknown issue376 world");
const COPY_AFTER = COPY_WORLD === "after";
const BILL = "https://jira.example.com/browse/BILL-7";
const DESIGN = "https://docs.example.com/Review_(draft)";
const BILL_DRAFT = `• Billing ([BILL-7](${BILL})).`;
const BILL_PLAIN = `• Billing (BILL-7 <${BILL}>).`;
const BILL_BEFORE = `• Billing (BILL-7 (${BILL})).`;
const DESIGN_DRAFT = `• Search: [the design](${DESIGN}).\n    ◦ next: review`;
const DESIGN_PLAIN = `• Search: the design <${DESIGN}>.\n    ◦ next: review`;

async function recordClipboard(page: Page): Promise<string[]> {
  const denied: string[] = [];
  const origin = `http://127.0.0.1:${Number(process.env.CAREERBREW_E2E_PORT ?? 8799)}`;
  await page.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const allowedWrite = request.method() === "POST" && ["/api/session", "/api/timing", "/api/pm/draft"].includes(url.pathname);
    if (url.origin !== origin || (request.method() !== "GET" && !allowedWrite)) {
      denied.push(url.pathname);
      await route.abort();
      return;
    }
    await route.continue();
  });
  await page.addInitScript(() => {
    const recorder: CopyRecorder = { policy: "accept", attempts: [] };
    Object.defineProperty(window, "issue376Clipboard", { value: recorder });
    const denied = () => new DOMException("Synthetic clipboard refusal", "NotAllowedError");
    Object.defineProperty(navigator.clipboard, "writeText", { value: async (plain: string) => {
      const accepted = recorder.policy !== "reject-both";
      recorder.attempts.push({ method: "plain", accepted, plain });
      if (!accepted) throw denied();
    } });
    Object.defineProperty(navigator.clipboard, "write", { get: () => recorder.policy === "missing-rich" ? undefined : async (items: ClipboardItem[]) => {
      if (items.length !== 1 || !(items[0] instanceof ClipboardItem)) throw new Error("BLOCKED: expected one real clipboard item");
      const item = items[0];
      const plain = item.types.includes("text/plain") ? await (await item.getType("text/plain")).text() : undefined;
      const html = item.types.includes("text/html") ? await (await item.getType("text/html")).text() : undefined;
      const accepted = recorder.policy !== "reject-rich" && recorder.policy !== "reject-both";
      recorder.attempts.push({ method: "rich", accepted, plain, html });
      if (!accepted) throw denied();
    } });
  });
  return denied;
}

async function policy(page: Page, value: CopyPolicy) {
  await page.evaluate((mode) => {
    const recorder = (window as CopyWindow).issue376Clipboard;
    recorder.policy = mode;
    recorder.attempts.length = 0;
  }, value);
}

async function attempts(page: Page): Promise<CopyAttempt[]> {
  return page.evaluate(() => (window as CopyWindow).issue376Clipboard.attempts);
}

async function inspectHtml(page: Page, html: string) {
  return page.evaluate((source) => {
    const template = document.createElement("template");
    template.innerHTML = source;
    const text = template.content.textContent ?? "";
    const anchors = [...template.content.querySelectorAll("a")].map((a) => ({
      label: a.textContent, href: a.getAttribute("href"),
      before: a.previousSibling?.textContent ?? "", after: a.nextSibling?.textContent ?? "",
    }));
    const active = template.content.querySelectorAll("script, img, iframe, svg, object, embed, [onerror], [onclick]").length;
    return { anchors, text, active };
  }, html);
}

async function settledCopy(page: Page): Promise<CopyAttempt> {
  await expect.poll(async () => (await attempts(page)).filter((attempt) => attempt.accepted).length, "BLOCKED: no single accepted clipboard payload").toBe(1);
  const accepted = (await attempts(page)).filter((attempt) => attempt.accepted);
  return accepted[0];
}

async function saveFixtureDraft(page: Page, text: string) {
  const response = await page.request.post("/api/pm/draft", { headers: HEADERS, data: { day: TODAY, text } });
  expect(response.status(), "BLOCKED: fixture draft could not be saved").toBe(200);
}

test("PM rich Copy #376 P01 Today inline Copy preserves the parenthesized link", async ({ page }, info) => {
  const denied = await recordClipboard(page);
  await setVia(page, "slack");
  await open(page, "Today");
  await page.getByLabel("PM update").fill(BILL_DRAFT);
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  const copied = await settledCopy(page);
  const html = copied.html ? await inspectHtml(page, copied.html) : null;
  expect(denied, "BLOCKED: unexpected request").toEqual([]);
  await info.attach("P01-clipboard-before-final-oracle", { body: JSON.stringify({ copied, html, denied }), contentType: "application/json" });
  await page.screenshot({ path: info.outputPath("issue376-inline-copy.png") });
  expect({ method: copied.method, plain: copied.plain, html }).toMatchObject({
    method: "rich", plain: BILL_PLAIN,
    html: { anchors: [{ label: "BILL-7", href: BILL, before: "• Billing (", after: ")." }], text: "• Billing (BILL-7).", active: 0 },
  });
});

test("PM rich Copy #376 P02 Dashboard header Copy preserves balanced destination parentheses", async ({ page }, info) => {
  const denied = await recordClipboard(page);
  await setVia(page, "slack");
  await saveFixtureDraft(page, DESIGN_DRAFT);
  await open(page, "Dashboard");
  await expect(page.locator("[data-pm-source]")).toHaveAttribute("data-pm-source", "edit");
  await copyButton(page).click();
  const copied = await settledCopy(page);
  const html = copied.html ? await inspectHtml(page, copied.html) : null;
  expect(denied, "BLOCKED: unexpected request").toEqual([]);
  await info.attach("P02-clipboard-before-final-oracle", { body: JSON.stringify({ copied, html, denied }), contentType: "application/json" });
  await page.screenshot({ path: info.outputPath("issue376-header-copy.png") });
  expect({ method: copied.method, plain: copied.plain, html }).toMatchObject({
    method: "rich", plain: DESIGN_PLAIN,
    html: { anchors: [{ label: "the design", href: DESIGN, before: "• Search: ", after: "." }], active: 0 },
  });
  expect(html?.text).toContain("◦ next: review");
});

test("PM rich Copy #376 C01 no-link Slack copy keeps bullets and indentation", async ({ page }) => {
  const denied = await recordClipboard(page);
  await setVia(page, "slack");
  await open(page, "Today");
  const plain = "• Billing: draft\n    ◦ next: review";
  await page.getByLabel("PM update").fill(plain);
  await page.getByRole("button", { name: "Copy", exact: true }).click();
  expect((await settledCopy(page)).plain).toBe(plain);
  expect(denied).toEqual([]);
});

test("PM rich Copy #376 C02 email stays on the existing subject and plain-body path", async ({ page }) => {
  const denied = await recordClipboard(page);
  await setVia(page, "email");
  try {
    await open(page, "Today");
    await page.getByLabel("PM update").fill(BILL_DRAFT);
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    expect(await settledCopy(page)).toEqual({ method: "plain", accepted: true, plain: `Daily update ${TODAY}\n\n${BILL_BEFORE}` });
    expect(denied).toEqual([]);
  } finally {
    await setVia(page, "slack");
  }
});

test("PM rich Copy #376 C03 clipboard fallback preserves selected manual text", async ({ page }, info) => {
  const denied = await recordClipboard(page);
  await setVia(page, "slack");
  await saveFixtureDraft(page, BILL_DRAFT);
  for (const mode of COPY_AFTER ? ["missing-rich", "reject-rich", "reject-both"] as const : ["reject-both"] as const) {
    await open(page, "Dashboard");
    await policy(page, mode);
    await copyButton(page).click();
    if (mode === "reject-both") {
      const box = page.getByLabel("Update to copy");
      await expect(box).toHaveValue(COPY_AFTER ? BILL_PLAIN : BILL_BEFORE);
      await expect.poll(() => box.evaluate((el: HTMLTextAreaElement) => [el.selectionStart, el.selectionEnd])).toEqual([0, (COPY_AFTER ? BILL_PLAIN : BILL_BEFORE).length]);
      expect((await attempts(page)).every((attempt) => !attempt.accepted)).toBe(true);
      await page.screenshot({ path: info.outputPath("issue376-manual-fallback.png") });
      await page.getByRole("button", { name: "Close", exact: true }).click();
      await expect(box).toHaveCount(0);
    } else {
      expect(await settledCopy(page)).toEqual({ method: "plain", accepted: true, plain: BILL_PLAIN });
      expect((await attempts(page)).map((attempt) => attempt.method)).toEqual(mode === "missing-rich" ? ["plain"] : ["rich", "plain"]);
    }
  }
  expect(denied).toEqual([]);
});

if (COPY_AFTER) {
  test("PM rich Copy #376 C04 rich content does not activate unsafe draft markup", async ({ page }) => {
    const denied = await recordClipboard(page);
    await setVia(page, "slack");
    await open(page, "Today");
    const inert = '[bad](javascript:alert(1)) [credentials](https://name:sample@example.com/path) <img src="https://example.com/pixel"> ![image](https://example.com/image) `[literal](https://example.com/code)`';
    await page.getByLabel("PM update").fill(`[safe](${BILL}) ${inert}`);
    await page.getByRole("button", { name: "Copy", exact: true }).click();
    const copied = await settledCopy(page);
    expect(copied.plain).toBe(`safe <${BILL}> ${inert}`);
    const html = copied.html ? await inspectHtml(page, copied.html) : null;
    expect(html).toMatchObject({ anchors: [{ label: "safe", href: BILL }], active: 0, text: `safe ${inert}` });
    expect(denied).toEqual([]);
  });
}

test("PM rich Copy #376 C05 current text copies before its existing blur save settles", async ({ page }, info) => {
  const denied = await recordClipboard(page);
  await setVia(page, "slack");
  await open(page, "Today");
  const notes = page.locator("[data-pm-why]");
  await expect(notes).toContainText("never copied or sent");
  await expect(notes).toContainText("platform team");
  const before = await page.request.get("/api/daylog", { headers: HEADERS });
  expect(before.ok()).toBe(true);
  const log = (await before.json()).log;
  let release = () => {};
  const settle = new Promise<void>((resolve) => { release = resolve; });
  const saves: string[] = [];
  await page.route("**/api/pm/draft", async (route) => {
    saves.push(route.request().postDataJSON().text);
    await settle;
    await route.continue();
  });
  const draft = "• Synthetic clipboard draft\n    ◦ next: preserve indentation";
  try {
    await page.getByLabel("PM update").fill(draft);
    await copyButton(page).click();
    const copied = await settledCopy(page);
    await expect.poll(() => saves).toEqual([draft]);
    expect(copied.plain).toBe(draft);
    expect(copied.plain).not.toContain("platform team");
    expect(denied).toEqual([]);
    await info.attach("C05-copy-before-blur-save-settled", { body: JSON.stringify({ draft, copied, saves, denied }), contentType: "application/json" });
  } finally {
    release();
  }
  await expect(page.locator("[data-pm-source]")).toHaveAttribute("data-pm-source", "edit");
  const after = await page.request.get("/api/daylog", { headers: HEADERS });
  expect(after.ok()).toBe(true);
  expect((await after.json()).log).toEqual(log);
  expect(denied).toEqual([]);
});
