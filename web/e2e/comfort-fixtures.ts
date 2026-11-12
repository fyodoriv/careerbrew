import { test as base } from "@playwright/test";
import type { BrowserContextOptions } from "@playwright/test";
import { withComfortScreens } from "./comfort-screen";
import type { ComfortScreen } from "./comfort-screen";

export const test = base.extend<{ screen: ComfortScreen }>({
  screen: async ({ browser }, use, testInfo) => {
    await withComfortScreens(browser, testInfo.project.use as BrowserContextOptions, use);
  },
});
