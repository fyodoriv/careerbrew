import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: ".",
  testMatch: "*.spec.ts",
  workers: 1,
  outputDir: `${process.env.CAREERBREW_E2E_OUT ?? "test-results"}/route-error-probe`,
  reporter: [["json"]],
  use: { headless: true },
});
