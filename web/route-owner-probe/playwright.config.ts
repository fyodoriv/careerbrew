import { defineConfig } from "@playwright/test";

export default defineConfig({
  // Never let Playwright run git in this checkout: in a pull request on GitHub Actions it fetches the base
  // with --depth=1, which makes the checkout shallow (tests/unit/test_hosted_ci.py, docs/ci.md).
  captureGitInfo: { commit: false, diff: false },
  testDir: ".",
  testMatch: "*.spec.ts",
  workers: 1,
  outputDir: `${process.env.CAREERBREW_E2E_OUT ?? "test-results"}/route-error-probe`,
  reporter: [["json"]],
  use: { headless: true },
});
