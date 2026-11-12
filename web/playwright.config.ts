import { defineConfig, devices } from "@playwright/test";

// CAREERBREW_E2E_PORT lets two checkouts run the suite at once (default 8799).
const PORT = Number(process.env.CAREERBREW_E2E_PORT ?? 8799);
// CAREERBREW_E2E_OUT gives each run its own output folder (traces and screenshots), so the
// story scripts can run Playwright in parallel (scripts/e2e.sh). Playwright empties it first.
export const OUT = process.env.CAREERBREW_E2E_OUT ?? "test-results";
export const TOKEN = "e2e-token-not-secret";
// The seeded server's pinned clock. Tests derive every date from it, so the suite passes on any CAREERBREW_NOW.
export const NOW = process.env.CAREERBREW_NOW ?? "2026-10-19T09:00";
// Playwright's trace recorder injects a snapshot script into every frame; Chrome blocks it in the Reports view's
// sandboxed run-page frame and logs this. The run page itself has no script (tests/unit/test_run_pages.py).
export const TRACE_IN_SANDBOX = /^Blocked script execution in 'about:srcdoc' because the document's frame is sandboxed/;

// The suite drives the built app served by `careerbrew serve` over a synthetic data root.
export default defineConfig({
  testDir: "./e2e",
  // These crowded-state cases run through US-59 with their owned real-app fixture.
  testIgnore: ["**/comfort-targets.spec.ts", "**/comfort-tag.spec.ts"],
  outputDir: OUT,
  fullyParallel: false,
  workers: 1,
  retries: 0,
  // Gates often run beside other heavy jobs (load 100+). A 5 s wait failed correct pages there;
  // 15 s still fails a missing element, only later.
  expect: { timeout: 15_000 },
  reporter: [["list"]],
  use: { baseURL: `http://127.0.0.1:${PORT}`, trace: "retain-on-failure", ...devices["Desktop Chrome"],
    // a synthetic microphone (a test tone) for voice practice; no real device, no permission prompt
    permissions: ["microphone"], launchOptions: { args: ["--use-fake-device-for-media-stream", "--use-fake-ui-for-media-stream"] } },
  webServer: [{
    command: `bash ../tests/e2e/web-seed-and-serve.sh ${PORT}`,
    // Ready when this run's own server prints its address, which it does only after it holds the port. A `url` check
    // also passed on another run's server on the same port, so two runs shared one data root and failed each other's
    // counts (task open-asks-mark-flake). Now a taken port stops the run with "Address already in use".
    wait: { stdout: new RegExp(`careerbrew web app: http://127\\.0\\.0\\.1:${PORT}/`) },
    env: { CAREERBREW_SERVE_TOKEN: TOKEN, CAREERBREW_NOW: NOW },
    reuseExistingServer: false,
    // Only stops a hang: readiness is the address line above. The seed's ~30 CLI processes took 50 s alone at load
    // ~100 and passed 60 s in every browser story of a gate at load ~250 (2026-10-25); one process now takes ~1 s.
    timeout: 180_000,
  }, {
    command: 'exec /usr/bin/env -i PATH=/usr/bin:/bin LANG=C LC_ALL=C CAREERBREW_NEXT_STEP_DAY="$CAREERBREW_NEXT_STEP_DAY" ../.venv/bin/python -I -B ../tests/e2e/web_seed.py --next-step',
    wait: { stdout: /careerbrew next-step fixture: http:\/\/127\.0\.0\.1:(?<careerbrew_next_step_port>[0-9]+)\// },
    env: { CAREERBREW_NEXT_STEP_DAY: NOW.slice(0, 10) },
    reuseExistingServer: false,
    timeout: 180_000,
    gracefulShutdown: { signal: "SIGINT", timeout: 3_000 },
  }, {
    command: 'exec /usr/bin/env -i PATH=/usr/bin:/bin LANG=C LC_ALL=C ../.venv/bin/python -I -B ../tests/e2e/live_health_seed.py',
    wait: { stdout: /careerbrew live health fixture: http:\/\/127\.0\.0\.1:(?<careerbrew_live_health_port>[0-9]+)\/ root=(?<careerbrew_live_health_root>[^\r\n]+)/ },
    reuseExistingServer: false,
    timeout: 180_000,
    gracefulShutdown: { signal: "SIGINT", timeout: 3_000 },
  }],
});
