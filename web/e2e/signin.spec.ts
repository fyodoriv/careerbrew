import { execFileSync } from "node:child_process";
import { once } from "node:events";
import { chmodSync, existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { type AddressInfo, connect, createServer } from "node:net";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { expect, test } from "@playwright/test";
import { NOW, OUT } from "../playwright.config";

// Issue #141: this file runs its own synthetic careerbrew home and web app (not the seeded server), started
// the way `careerbrew open` starts it, so the token comes from the encrypted day/web-token.json and outlives
// a restart. A BROWSER script stands in for the default browser: it keeps the one-time link it is handed.
const ROOT = fileURLToPath(new URL("../..", import.meta.url));
const DIR = mkdtempSync(join(tmpdir(), "careerbrew-signin-"));
const OPENED = join(DIR, "opened-url");
const ENV: Record<string, string | undefined> = {
  ...process.env,
  CAREERBREW_HOME: join(DIR, "data"), CAREERBREW_KEY_FILE: join(DIR, "key"), CAREERBREW_PAIR_KEY_FILE: join(DIR, "pair"),
  CAREERBREW_BACKUP_DIRS: join(DIR, "backups"), CAREERBREW_COMMANDS_DIR: join(DIR, "commands"),
  CAREERBREW_LLM: "off", CAREERBREW_NOTIFY: "off", CAREERBREW_EVENTS: "on", CAREERBREW_NOW: NOW, CAREERBREW_TEST_OWNER: "1",
  BROWSER: join(DIR, "browser.sh"),
};
delete ENV.CAREERBREW_SERVE_TOKEN;  // the real path: no pinned token
let PORT = 0;
let BASE = "";

const cb = (...args: string[]) => execFileSync(join(ROOT, ".venv/bin/careerbrew"), args, { env: ENV, encoding: "utf8" });
const tokenOf = (url: string) => url.split("token=")[1];
const serverPid = () => Number(execFileSync(join(ROOT, ".venv/bin/python"), ["-c", "from careerbrew import server, store; print(store.read_json(server.web_state_path(), {})['pid'])"], { env: ENV, encoding: "utf8" }).trim());

function freePort(): Promise<number> {
  return new Promise((resolve) => {
    const s = createServer();
    s.listen(0, "127.0.0.1", () => {
      const { port } = s.address() as AddressInfo;
      s.close(() => resolve(port));
    });
  });
}

function portClosed(): Promise<boolean> {
  return new Promise((resolve) => {
    const s = connect(PORT, "127.0.0.1");
    s.once("connect", () => (s.destroy(), resolve(false)));
    s.once("error", () => resolve(true));
  });
}

// The link the default browser got: from `careerbrew open`, or from the sign-in page's button (the server opens it).
async function opened(): Promise<string> {
  await expect.poll(() => existsSync(OPENED), { timeout: 15_000 }).toBe(true);
  const url = readFileSync(OPENED, "utf8").trim();
  rmSync(OPENED);
  return url;
}

async function openLink(): Promise<string> {
  rmSync(OPENED, { force: true });
  expect(cb("open", "--view", "today")).not.toContain("token=");  // the token goes to the browser only
  return opened();
}

test.beforeAll(async () => {
  PORT = await freePort();
  BASE = `http://127.0.0.1:${PORT}`;
  writeFileSync(join(DIR, "browser.sh"), '#!/bin/sh\nd="$(dirname "$0")"\nprintf \'%s\' "$1" > "$d/.opened" && mv "$d/.opened" "$d/opened-url"\n');
  chmodSync(join(DIR, "browser.sh"), 0o755);
  cb("init");
  cb("config", "set", "serve_port", String(PORT));
});

test.afterAll(async () => {
  try {
    process.kill(serverPid(), "SIGKILL");
  } catch {
    // never started, or already stopped
  }
  rmSync(DIR, { recursive: true, force: true });
});

test("a restart keeps the sign-in: the old tab reloads and a bookmark opens without a link (issue #141)", async ({ page, context }) => {
  const link = await openLink();
  await page.goto(link);
  await expect(page.getByRole("heading", { name: "Next step" })).toBeVisible();
  expect(page.url()).not.toContain("token=");
  // The page sets the cookie with its own request (POST /api/session), and its blocks show before that answers (#142).
  const cookieOf = async () => (await context.cookies(BASE)).find((c) => c.name === `careerbrew-${PORT}`);
  await expect.poll(cookieOf).toMatchObject({ domain: "127.0.0.1", path: "/", httpOnly: true, sameSite: "Strict", secure: false });
  const cookie = await cookieOf();
  expect(cookie!.expires * 1000).toBeGreaterThan(Date.now());
  expect(await page.evaluate(() => document.cookie)).toBe("");  // HttpOnly: no script on the page reads it

  // An update, a crash, or a reboot ends the server; the next `careerbrew open` (or `careerbrew`) starts it again.
  process.kill(serverPid(), "SIGKILL");
  await expect.poll(portClosed, { timeout: 15_000 }).toBe(true);
  expect(tokenOf(await openLink())).toBe(tokenOf(link));
  await page.reload();
  await expect(page.getByRole("heading", { name: "Next step" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Sign in to careerbrew" })).toHaveCount(0);

  // A bookmark: a new tab with no link and nothing in its storage signs in from the cookie.
  const tab = await context.newPage();
  await tab.goto(`${BASE}/#/week`);
  await expect(tab.getByRole("heading", { name: "Plan ahead" })).toBeVisible();
  await tab.screenshot({ path: `${OUT}/signin-bookmark.png`, fullPage: true });
});

test("a cross-site page never gets the sign-in cookie, and the server refuses it (issue #141)", async ({ page, context }) => {
  await page.goto(await openLink());
  await expect(page.getByRole("heading", { name: "Next step" })).toBeVisible();  // this browser holds the cookie now
  // Another web page on this machine, with no careerbrew policy of its own, tries to read the token.
  const foreign = createHttpServer((_req, res) => res.end("<!doctype html><title>another page</title>")).listen(0, "127.0.0.1");
  await once(foreign, "listening");
  const fport = (foreign.address() as AddressInfo).port;
  const other = await context.newPage();
  const steal = (url: string) => fetch(url, { credentials: "include", mode: "no-cors" }).then((r) => r.text()).then((t) => t || "nothing", () => "blocked");
  // The browser hides a refused response's headers from the test, so the server's content-free event says what came in.
  const restores = () => readFileSync(join(DIR, "data", "logs", "events.jsonl"), "utf8").trim().split("\n").map((l) => JSON.parse(l)).filter((e) => e.event === "web.session" && e.route === "restore");
  try {
    // SameSite=Strict keeps the cookie from another site (localhost is not 127.0.0.1). Cookies ignore the port, so a page on
    // another 127.0.0.1 port sends it; the server refuses both, because neither is careerbrew's own page (Sec-Fetch-Site).
    for (const [origin, cookie] of [[`http://localhost:${fport}`, false], [`http://127.0.0.1:${fport}`, true]] as const) {
      await other.goto(`${origin}/`);
      const before = restores().length;
      expect(await other.evaluate(steal, `${BASE}/api/session`), origin).toBe("blocked");  // and Cross-Origin-Resource-Policy hides the answer
      await expect.poll(() => restores().length, { timeout: 15_000 }).toBe(before + 1);
      const last = restores()[before];
      expect(last, origin).toMatchObject({ status: "refused", code: 403, cookie });
    }
  } finally {
    foreign.close();
  }
});

test("rotate-token signs out open tabs; the sign-in page names the command and opens a signed-in tab (issue #141)", async ({ page, context, browser }) => {
  const link = await openLink();
  await page.goto(link);
  await expect(page.getByRole("heading", { name: "Next step" })).toBeVisible();
  const out = cb("web", "rotate-token", "--yes");
  expect(out).toContain("careerbrew open");
  expect(out).not.toContain(tokenOf(link));
  // The open tab notices on its next live-refresh poll (every 5 s): no reload needed.
  await expect(page.getByRole("heading", { name: "Sign in to careerbrew" })).toBeVisible();
  await expect(page.getByText("careerbrew open", { exact: true })).toBeVisible();
  await page.screenshot({ path: `${OUT}/signin-page.png`, fullPage: true });

  await page.getByRole("button", { name: "Open a signed-in tab" }).click();
  const fresh = await opened();  // the server handed the default browser a new link, as `careerbrew open` does
  expect(tokenOf(fresh)).not.toBe(tokenOf(link));
  const tab = await context.newPage();
  await tab.goto(fresh);
  await expect(tab.getByRole("heading", { name: "Next step" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Next step" })).toBeVisible();  // the old tab signs in again from the new cookie

  // Another browser profile has no cookie: the sign-in page, never data.
  const stranger = await browser.newContext();
  const p = await stranger.newPage();
  await p.goto(`${BASE}/#/today`);
  await expect(p.getByRole("heading", { name: "Sign in to careerbrew" })).toBeVisible();
  await stranger.close();

  for (const log of ["events.jsonl", "web.log"]) {
    const text = readFileSync(join(DIR, "data", "logs", log), "utf8");
    for (const t of [tokenOf(link), tokenOf(fresh)]) expect(text, `${log} holds no token`).not.toContain(t);
  }
});
