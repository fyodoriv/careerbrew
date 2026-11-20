import { defineConfig, devices } from "@playwright/test";

const port = Number(process.env.CAREERBREW_E2E_PORT);
if (!port || !process.env.CAREERBREW_PROMISE_E2E_HOME || !process.env.CAREERBREW_CLIENT_HOME) {
  throw new Error("US-110 requires its own synthetic data root and port");
}

export default defineConfig({
  // Never let Playwright run git in this checkout: in a pull request on GitHub Actions it fetches the base
  // with --depth=1, which makes the checkout shallow (tests/unit/test_hosted_ci.py, docs/ci.md).
  captureGitInfo: { commit: false, diff: false },
  testDir: "./promise-e2e",
  workers: 1,
  retries: 0,
  reporter: [["list"]],
  outputDir: process.env.CAREERBREW_E2E_OUT,
  expect: { timeout: 15_000 },
  use: { baseURL: `http://127.0.0.1:${port}`, trace: "retain-on-failure", ...devices["Desktop Chrome"] },
  webServer: {
    command: `../.venv/bin/careerbrew serve --port ${port}`,
    wait: { stdout: new RegExp(`careerbrew web app: http://127\\.0\\.0\\.1:${port}/`) },
    env: { HOME: process.env.CAREERBREW_CLIENT_HOME, CAREERBREW_HOME: process.env.CAREERBREW_PROMISE_E2E_HOME, CAREERBREW_SERVE_TOKEN: "e2e-token-not-secret", CAREERBREW_TEST_OWNER: "" },
    reuseExistingServer: false,
    timeout: 180_000,
  },
});
