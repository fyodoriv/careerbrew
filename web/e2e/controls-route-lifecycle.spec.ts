import { expect, test, type Route } from "@playwright/test";
import { TOKEN } from "../playwright.config";
import { finishVisit, runOwnedVisit } from "./controls-route-owner";
import { finishStandup } from "./standup-teardown";

test("a delayed visit response completes before its route is removed", async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`);
  let release!: () => void, started!: () => void, finished!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  const active = new Promise<void>((resolve) => { started = resolve; });
  const done = new Promise<void>((resolve) => { finished = resolve; });
  const errors: string[] = [];
  const handler = async (route: Route) => {
    try {
      const response = await route.fetch();
      started();
      await barrier;
      await route.fulfill({ response, json: await response.json() });
    } catch (error) {
      errors.push(String(error));
    } finally {
      finished();
    }
  };
  const url = "**/api/feed/asks";
  await page.route(url, handler);
  await page.evaluate(() => { void fetch("/api/feed/asks").catch(() => {}); });
  await active;
  let cleaned = false;
  const cleanup = finishVisit(page).then(() => { cleaned = true; });
  try {
    await page.evaluate(() => document.title);
    expect(cleaned).toBe(false);
  } finally {
    release();
    await cleanup;
    await done;
  }
  await page.goto("about:blank");
  expect(errors).toEqual([]);
});

test("early response, next visit, and context fallback survive cleanup", async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`);
  const url = "**/synthetic-route-control";
  await page.context().route(url, (route) => route.fulfill({ json: { source: "context" } }));
  await page.route(url, (route) => route.request().method() === "GET"
    ? route.fulfill({ json: { source: "visit" } })
    : route.fallback());
  const read = async (method: "GET" | "POST") => page.evaluate(async (m) => {
    const response = await fetch("/synthetic-route-control", { method: m });
    return response.json();
  }, method);
  expect(await read("GET")).toEqual({ source: "visit" });
  expect(await read("POST")).toEqual({ source: "context" });
  await finishVisit(page);
  expect(await read("GET")).toEqual({ source: "context" });
  await page.route(url, (route) => route.fulfill({ json: { source: "next" } }));
  expect(await read("GET")).toEqual({ source: "next" });
  await finishVisit(page);
  await page.context().unroute(url);
});


test("failed visit closes only its page after its held route settles", async ({ page }) => {
  await page.goto(`/#token=${TOKEN}`);
  const context = page.context();
  const url = "**/synthetic-owned-visit";
  await page.route(url, (route) => route.fulfill({ json: { source: "foreign" } }));
  let release!: () => void, started!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  const active = new Promise<void>((resolve) => { started = resolve; });
  let owned: import("@playwright/test").Page | undefined;
  const visit = runOwnedVisit(context, async (current) => {
    owned = current;
    await current.goto(`/#token=${TOKEN}`);
    await current.route(url, async (route) => {
      started();
      await barrier;
      await route.fulfill({ json: { source: "owned" } });
    });
    await current.evaluate(() => { void fetch("/synthetic-owned-visit").catch(() => {}); });
    await active;
    throw new Error("synthetic visit failure");
  });
  try {
    await active;
    expect(owned?.isClosed()).toBe(false);
  } finally {
    release();
    await expect(visit).rejects.toThrow("synthetic visit failure");
  }
  expect(owned?.isClosed()).toBe(true);
  expect(await page.evaluate(async () => (await fetch("/synthetic-owned-visit")).json())).toEqual({ source: "foreign" });
  await page.unroute(url);
});

test("errors from the owned visit page remain visible to the inventory", async ({ page }) => {
  const { trackPageErrors } = await import("./controls-route-owner");
  const errors: string[] = [];
  await runOwnedVisit(page.context(), async (owned) => {
    trackPageErrors(owned, errors);
    await owned.goto(`/#token=${TOKEN}`);
    const raised = owned.waitForEvent("pageerror");
    await owned.evaluate(() => {
      console.error("synthetic visit console error");
      setTimeout(() => { throw new Error("synthetic visit page error"); }, 0);
    });
    await raised;
  });
  expect(errors).toContain("synthetic visit console error");
  expect(errors).toContain("synthetic visit page error");
});

test("the anchor page stays idle while an owned visit runs", async ({ page }) => {
  const requests: string[] = [];
  page.on("request", (request) => requests.push(request.url()));
  await runOwnedVisit(page.context(), async (owned) => {
    await owned.goto(`/#token=${TOKEN}`);
    await expect(owned.getByRole("navigation", { name: "Views" })).toBeVisible();
  });
  expect(page.url()).toBe("about:blank");
  expect(requests).toEqual([]);
});


test("standup teardown waits for a held page route", async ({ page }) => {
  test.setTimeout(10_000);
  await page.goto(`/#token=${TOKEN}`);
  let release!: () => void, started!: () => void, finished!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  const active = new Promise<void>((resolve) => { started = resolve; });
  const done = new Promise<void>((resolve) => { finished = resolve; });
  await page.route("**/synthetic-standup-teardown", async (route) => {
    try {
      started();
      await barrier;
      await route.fulfill({ json: { ok: true } });
    } finally {
      finished();
    }
  });
  await page.evaluate(() => { void fetch("/synthetic-standup-teardown").catch(() => {}); });
  await active;
  let cleaned = false;
  const cleanup = finishStandup(page, []).then(() => { cleaned = true; });
  try {
    await page.evaluate(() => document.title);
    expect(cleaned).toBe(false);
  } finally {
    release();
    await cleanup;
    await done;
  }
  expect(cleaned).toBe(true);
});


test("standup teardown reports a page error during its drain", async ({ page }) => {
  test.setTimeout(10_000);
  await page.goto(`/#token=${TOKEN}`);
  let release!: () => void, started!: () => void, finished!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  const active = new Promise<void>((resolve) => { started = resolve; });
  const done = new Promise<void>((resolve) => { finished = resolve; });
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.route("**/synthetic-standup-error", async (route) => {
    try {
      started();
      await barrier;
      await route.fulfill({ json: { ok: true } });
    } finally {
      finished();
    }
  });
  await page.evaluate(() => { void fetch("/synthetic-standup-error").catch(() => {}); });
  await active;
  const cleanup = finishStandup(page, errors);
  const failure = expect(cleanup).rejects.toThrow("synthetic-standup-page-error");
  try {
    const seen = page.waitForEvent("pageerror");
    await page.evaluate(() => { setTimeout(() => { throw new Error("synthetic-standup-page-error"); }, 0); });
    await seen;
  } finally {
    release();
    await failure;
    await done;
  }
});
