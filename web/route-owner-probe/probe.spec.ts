import { expect, test } from "@playwright/test";
import { runOwnedVisit } from "../e2e/controls-route-owner";
import { finishStandup } from "../e2e/standup-teardown";

test("route callback error is reported", async ({ context }) => {
  test.fail();
  await runOwnedVisit(context, async (page) => {
    await page.route("**/*", () => { throw new Error("synthetic-route-error-sentinel"); });
    await page.goto("http://127.0.0.1:1/");
  });
});

test("a clean visit completes", async ({ context }) => {
  await runOwnedVisit(context, async (page) => {
    await page.route("**/*", (route) => route.fulfill({ body: "synthetic response" }));
    await page.goto("http://127.0.0.1:1/");
    await expect(page.locator("body")).toHaveText("synthetic response");
  });
});


test("standup route callback error is reported during drain", async ({ context }) => {
  test.fail();
  const page = await context.newPage();
  await page.goto("about:blank");
  let release!: () => void, started!: () => void;
  const barrier = new Promise<void>((resolve) => { release = resolve; });
  const active = new Promise<void>((resolve) => { started = resolve; });
  await page.route("**/*", async () => {
    started();
    await barrier;
    throw new Error("synthetic-standup-route-error-sentinel");
  });
  await page.evaluate(() => { void fetch("http://127.0.0.1:1/synthetic").catch(() => {}); });
  await active;
  const cleanup = finishStandup(page, []);
  release();
  await cleanup;
  await page.close();
});
