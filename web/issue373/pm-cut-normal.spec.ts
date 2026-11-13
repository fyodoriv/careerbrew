import { expect, test } from "@playwright/test";
import type { Request } from "@playwright/test";
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, realpathSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { TOKEN } from "../playwright.config";

function snapshot(root: string) {
  const result: Record<string, { bytes: string; mtime: string; mode: string }> = {};
  const visit = (folder: string) => {
    for (const entry of readdirSync(folder, { withFileTypes: true })) {
      const path = join(folder, entry.name);
      if (entry.isSymbolicLink()) throw new Error("US-59 fixture contains a symlink");
      if (entry.isDirectory()) visit(path);
      else if (entry.isFile()) {
        const stat = statSync(path, { bigint: true });
        result[relative(root, path)] = {
          bytes: createHash("sha256").update(readFileSync(path)).digest("hex"),
          mtime: String(stat.mtimeNs), mode: String(stat.mode),
        };
      } else throw new Error("US-59 fixture contains a nonregular entry");
    }
  };
  visit(root);
  return result;
}

test("restoring a PM line clears its cut flag without a save", async ({ page, context, baseURL }) => {
  const world = process.env.CAREERBREW_PM_CUT_WORLD;
  if (!world || realpathSync(world) !== world || !baseURL) throw new Error("US-59 owned world missing");
  expect(readFileSync(join(world, ".owner"), "utf8")).toBe("US-59 isolated PM cut fixture\n");
  const { raw, kept } = JSON.parse(readFileSync(join(world, "seed.json"), "utf8")) as { raw: string; kept: string };
  const root = join(world, "data");
  const errors: string[] = [];
  const forbidden: string[] = [];
  const pending = new Set<Request>();
  const reads = new Set(["/api/session", "/api/config", "/api/overview", "/api/changes", "/api/daylog", "/api/today", "/api/inbox", "/api/priority", "/api/feed/asks", "/api/tab-key"]);
  let settled = false;
  page.on("console", (message) => { if (message.type() === "error") errors.push(message.text()); });
  page.on("pageerror", (error) => errors.push(error.message));
  page.on("request", (request) => pending.add(request));
  page.on("requestfinished", (request) => pending.delete(request));
  page.on("requestfailed", (request) => pending.delete(request));
  await context.route("**/*", async (route) => {
    const request = route.request();
    const url = new URL(request.url());
    const read = request.method() === "GET" && (reads.has(url.pathname) || url.pathname === "/" || url.pathname === "/favicon.ico" || url.pathname.startsWith("/assets/"));
    const setup = !settled && request.method() === "POST" && (
      (url.pathname === "/api/session" && request.postData() === "{}")
      || (url.pathname === "/api/timing" && request.postDataJSON()?.saves === "pm_update" && request.postDataJSON()?.step === "open")
    );
    if (url.origin !== new URL(baseURL).origin || (!read && !setup)) {
      forbidden.push(request.method() + " " + url.pathname);
      await route.abort("blockedbyclient");
      return;
    }
    await route.continue();
  });
  const session = await context.request.post("/api/session", { headers: { "X-Careerbrew-Token": TOKEN } });
  expect(session.status()).toBe(200);
  let before: ReturnType<typeof snapshot> | undefined;
  try {
    await page.goto("/#/today");
    const editor = page.getByLabel("PM update", { exact: true });
    const flag = page.locator("[data-pm-left-out]");
    await expect(editor).toHaveValue(kept);
    await expect(flag).toContainText("Other");
    await expect(page.locator(".skeleton")).toHaveCount(0);
    await expect.poll(() => pending.size).toBe(0);
    before = snapshot(root);
    settled = true;
    await editor.fill(raw);
    await expect(editor).toHaveValue(raw);
    await expect(editor).toBeFocused();
    await expect.poll(() => pending.size).toBe(0);
    expect(snapshot(root)).toEqual(before);
    await expect(flag).toHaveCount(0);
  } finally {
    await page.close({ runBeforeUnload: false });
    if (before) expect(snapshot(root)).toEqual(before);
    expect(forbidden).toEqual([]);
    expect(errors).toEqual([]);
  }
});
