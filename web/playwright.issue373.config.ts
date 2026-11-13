import { defineConfig } from "@playwright/test";
import { resolve } from "node:path";

const run = process.env.ISSUE373_RUN;
const mode = process.env.ISSUE373_MODE;
const browser = process.env.ISSUE373_BROWSER;
if (!run || !browser || !mode || !["primary", "dates", "comfort", "geometry", "proof-before", "proof-after"].includes(mode)) throw new Error("BLOCKED: exact373 mode/root/browser required");
const root = resolve(import.meta.dirname, "..");
const scenarios = mode === "dates"
  ? ["ascii", "unicode", "legacy-missing", "legacy-invalid", "legacy-naive", "legacy-unavailable"]
  : [mode === "primary" ? "ascii" : "crowded"];
export default defineConfig({
  testDir: mode === "geometry" ? "./e2e" : "./issue373",
  testMatch: mode === "geometry" ? ["comfort-targets.spec.ts", "comfort-tag.spec.ts"] : "pm-cut.spec.ts",
  outputDir: `${run}/playwright`, preserveOutput: "always",
  workers: 1, fullyParallel: false, retries: 0,
  timeout: 90_000, globalTimeout: 600_000, expect: { timeout: 15_000 },
  reporter: [["list"], ["json", { outputFile: `${run}/pytest-equivalent-playwright.json` }]],
  use: {
    browserName: "chromium", headless: true, reducedMotion: "reduce", timezoneId: "America/New_York",
    serviceWorkers: "block", trace: "retain-on-failure", screenshot: "only-on-failure",
    launchOptions: {
      executablePath: browser,
      args: ["--use-mock-keychain", "--password-store=basic", "--disable-background-networking", "--disable-sync", "--host-resolver-rules=MAP * ~NOTFOUND, EXCLUDE 127.0.0.1"],
    },
  },
  webServer: scenarios.map((scenario) => ({
    command: `exec /bin/bash --noprofile --norc '${root}/tests/safety/issue373_web_server.sh' '${scenario}' '${run}'`,
    cwd: root, reuseExistingServer: false, timeout: 180_000,
    wait: { stdout: new RegExp(`issue373-ready ${scenario} http://127\\.0\\.0\\.1:[0-9]+/`) },
    gracefulShutdown: { signal: "SIGINT" as const, timeout: 3_000 },
    stdout: "pipe", stderr: "pipe",
  })),
});
