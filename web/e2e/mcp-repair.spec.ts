import { existsSync, lstatSync, mkdirSync, readFileSync, realpathSync, statSync } from "node:fs";
import { expect, test } from "@playwright/test";
import { NOW, OUT, TRACE_IN_SANDBOX } from "../playwright.config";
import { REPAIR_CANARY, REPAIR_TOKEN, repairServer } from "./mcp-repair-fixture";

// Each test owns the synthetic server group and removes its client folder in afterEach.
let fixture: Awaited<ReturnType<typeof repairServer>>;
test.beforeEach(async () => { test.setTimeout(180_000); fixture = await repairServer(undefined, NOW); });
test.afterEach(async () => { await fixture?.stop(); });

test("Actions: GitHub repair honors confirmation and preserves configuration", async ({ page }) => {
  test.setTimeout(120_000);
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("console", (message) => { if (message.type() === "error" && !TRACE_IN_SANDBOX.test(message.text())) errors.push(message.text()); });
  const posts: string[] = [];
  page.on("request", (request) => { if (request.method() === "POST" && request.url().endsWith("/api/actions/day.start")) posts.push(request.url()); });
  const proof = `${OUT}/mcp-repair`;
  mkdirSync(proof, { recursive: true });
  for (const mode of [{ name: "phone", width: 390, height: 664 }, { name: "desktop", width: 1280, height: 900 }]) {
    fixture.reset();
    const initialTime = statSync(fixture.target, { bigint: true }).mtimeNs;
    const beforePosts = posts.length;
    await page.setViewportSize({ width: mode.width, height: mode.height });
    await page.emulateMedia({ reducedMotion: "reduce", colorScheme: "light" });
    await page.goto(`${fixture.base}/#/actions/day.start&token=${REPAIR_TOKEN}`);
    await page.reload();
    const form = page.getByRole("form", { name: "Run day.start" });
    await expect(form).toBeVisible();
    await expect(form.getByRole("checkbox", { name: "fix", exact: true })).toBeVisible();
    await expect(form.getByRole("checkbox", { name: "yes", exact: true })).toHaveCount(0);
    await page.getByLabel("Filter actions").fill("day.start");
    await form.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${proof}/form-${mode.name}.png` });
    const run = form.getByRole("button", { name: "Run", exact: true });
    const confirmation = form.getByRole("checkbox", { name: /^I understand: it / });
    await form.getByRole("checkbox", { name: "fix", exact: true }).check();
    await form.getByRole("checkbox", { name: "json", exact: true }).check();
    await expect(run).toBeDisabled();
    await confirmation.check();
    await expect(run).toBeEnabled();
    await confirmation.uncheck();
    await expect(run).toBeDisabled();
    expect(posts).toHaveLength(beforePosts);
    expect(readFileSync(fixture.target, "utf8")).toBe(fixture.original);
    expect(statSync(fixture.target, { bigint: true }).mtimeNs).toBe(initialTime);

    await confirmation.check();
    await run.click();
    const result = form.getByLabel("Result");
    await expect(result).toContainText('"changed": 1');
    await expect(result).toContainText("exit 0");
    expect(posts).toHaveLength(beforePosts + 1);
    expect(lstatSync(fixture.link).isSymbolicLink()).toBe(true);
    expect(realpathSync(fixture.link)).toBe(fixture.target);
    expect(statSync(fixture.target).mode & 0o777).toBe(0o640);
    const first = readFileSync(fixture.target, "utf8");
    const firstTime = statSync(fixture.target, { bigint: true }).mtimeNs;
    const settings = JSON.parse(first);
    expect(settings.keep).toEqual({ fixture: true });
    expect(settings.mcpServers.fixture.headers).toEqual({ "X-Test": "keep" });
    expect(settings.mcpServers.fixture.headersHelper).toEqual(expect.any(String));
    expect(first).not.toContain(REPAIR_CANARY);
    expect(existsSync(fixture.ghCalls)).toBe(false);
    await run.click();
    await expect(result).toContainText('"changed": 0');
    expect(posts).toHaveLength(beforePosts + 2);
    expect(readFileSync(fixture.target, "utf8")).toBe(first);
    expect(statSync(fixture.target, { bigint: true }).mtimeNs).toBe(firstTime);
    expect(await page.locator("body").innerText()).not.toContain(REPAIR_CANARY);
    await result.scrollIntoViewIfNeeded();
    await page.screenshot({ path: `${proof}/repeat-${mode.name}.png` });
  }
  expect(errors).toEqual([]);
});
