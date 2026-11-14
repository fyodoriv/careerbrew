import { defineConfig, devices } from "@playwright/test";

// Visual tests (US-75): each user story with a web surface walks its key screens and compares them with approved
// pictures in web/visual/__screenshots__/ (toHaveScreenshot). Everything that could move a pixel is fixed here:
// - the clock: the server and the browser both read VISUAL_NOW in VISUAL_TZ, whatever CAREERBREW_NOW says;
// - the data: the synthetic e2e seed (tests/e2e/web-seed-and-serve.sh), on a server of this run's own;
// - the screen: a desktop and a phone viewport, light colours, en-US, reduced motion, no animations, no caret;
// - the fonts: visual/helpers.ts picks fonts every Mac has (styles.css starts with Inter, which only some have).
// A missing or different picture fails the run; nothing is written unless you pass --update-snapshots (docs/web.md).
const PORT = Number(process.env.CAREERBREW_E2E_PORT ?? 8798);
export const OUT = process.env.CAREERBREW_E2E_OUT ?? "test-results/visual";
export const TOKEN = "e2e-token-not-secret";
// A Monday at UTC-4: the seed's calendar has its standup, and every dated fixture lands on a workday.
export const VISUAL_NOW = "2026-10-19T09:00";
export const VISUAL_TZ = "Etc/GMT+4";
export const VISUAL_INSTANT = `${VISUAL_NOW}:00-04:00`;
export const TRACE_IN_SANDBOX = /^Blocked script execution in 'about:srcdoc' because the document's frame is sandboxed/;

export default defineConfig({
  // Never let Playwright run git in this checkout: in a pull request on GitHub Actions it fetches the base
  // with --depth=1, which makes the checkout shallow (tests/unit/test_hosted_ci.py, docs/ci.md).
  captureGitInfo: { commit: false, diff: false },
  testDir: "./visual",
  outputDir: OUT,
  // Pictures are per platform: font rendering differs between macOS and Linux (docs/web.md, "Visual tests").
  // VISUAL_SNAPSHOTS points a run at other pictures (tests/e2e/us-75-visual-stories.sh checks a picture that differs).
  snapshotPathTemplate: `${process.env.VISUAL_SNAPSHOTS ?? "{testDir}/__screenshots__"}/{testFileName}/{arg}-{projectName}-{platform}{ext}`,
  // A visual test never writes to the server (it patches GET answers instead), so tests run side by side.
  fullyParallel: true,
  workers: Number(process.env.VISUAL_WORKERS ?? 3),
  retries: 0,
  updateSnapshots: "none",
  timeout: 60_000,
  expect: {
    timeout: 15_000,
    toHaveScreenshot: { animations: "disabled", caret: "hide", scale: "css", maxDiffPixels: 0 },
  },
  reporter: [["list"]],
  use: {
    baseURL: `http://127.0.0.1:${PORT}`,
    trace: "retain-on-failure",
    locale: "en-US",
    timezoneId: VISUAL_TZ,
    colorScheme: "light",
    contextOptions: { reducedMotion: "reduce" },
    // a synthetic microphone for the voice practice screen; no real device, no permission prompt
    permissions: ["microphone"],
    launchOptions: { args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] },
  },
  projects: [
    { name: "desktop", use: { ...devices["Desktop Chrome"], viewport: { width: 1280, height: 800 }, deviceScaleFactor: 1 } },
    { name: "phone", use: { ...devices["Pixel 7"], viewport: { width: 390, height: 844 }, deviceScaleFactor: 1 } },
  ],
  webServer: {
    command: `bash ../tests/e2e/web-seed-and-serve.sh ${PORT}`,
    // As in playwright.config.ts: ready only when this run's own server prints its address.
    wait: { stdout: new RegExp(`careerbrew web app: http://127\\.0\\.0\\.1:${PORT}/`) },
    // Empty values make the seed use a fresh temporary data root and key, whatever your shell exports, and keep the
    // public study deck on: the pictures never come from (or write to) your own private/ folder.
    env: {
      CAREERBREW_SERVE_TOKEN: TOKEN, CAREERBREW_NOW: VISUAL_NOW, TZ: VISUAL_TZ,
      CAREERBREW_VISUAL_SEED: "1",
      CAREERBREW_HOME: "", CAREERBREW_KEY_FILE: "", CAREERBREW_PAIR_KEY_FILE: "", CAREERBREW_BACKUP_DIRS: "", CAREERBREW_PUBLIC_CARDS: "",
    },
    reuseExistingServer: false,
    timeout: 180_000, // a hang guard: the same seed as playwright.config.ts
  },
});
