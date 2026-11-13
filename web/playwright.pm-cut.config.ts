import { defineConfig } from "@playwright/test";
import normal from "./playwright.config";

const port = Number(process.env.CAREERBREW_E2E_PORT);
const world = process.env.CAREERBREW_PM_CUT_WORLD;
if (!world || !Number.isInteger(port) || port < 1 || port > 65535) throw new Error("US-59 requires its isolated world and port");
// The shared suite may also start feature fixtures. Its first server owns the app port.
const appServer = Array.isArray(normal.webServer) ? normal.webServer[0] : normal.webServer;
if (appServer?.command !== `bash ../tests/e2e/web-seed-and-serve.sh ${port}` || appServer.reuseExistingServer !== false) {
  throw new Error("US-59 requires the ordinary app server template");
}

export default defineConfig({
  ...normal,
  testDir: "./issue373",
  testMatch: "pm-cut-normal.spec.ts",
  use: { ...normal.use, serviceWorkers: "block" },
  webServer: {
    ...appServer,
    command: `bash ../tests/e2e/pm-cut-seed-and-serve.sh ${port}`,
    env: { ...appServer.env, CAREERBREW_PM_CUT_WORLD: world },
  },
});
