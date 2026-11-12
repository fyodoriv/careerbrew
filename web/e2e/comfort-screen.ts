import type { Browser, BrowserContext, BrowserContextOptions, Page } from "@playwright/test";

export type ComfortScreen = (options: BrowserContextOptions) => Promise<Page>;

// One test fixture invocation owns only the contexts its screen calls create.
export async function withComfortScreens(browser: Browser, project: BrowserContextOptions, run: (screen: ComfortScreen) => Promise<void>): Promise<void> {
  const contexts: BrowserContext[] = [];
  const cleanup: unknown[] = [];
  let bodyFailed = false;
  let bodyError: unknown;
  try {
    await run(async (options) => {
      const context = await browser.newContext({ ...project, ...options });
      contexts.push(context);
      return context.newPage();
    });
  } catch (error) {
    bodyFailed = true;
    bodyError = error;
  } finally {
    for (const context of contexts.reverse()) {
      try {
        await context.close();
      } catch (error) {
        cleanup.push(error);
      }
    }
  }
  if (bodyFailed) {
    if (cleanup.length) throw new AggregateError([bodyError, ...cleanup], "comfort test and context cleanup failed");
    throw bodyError;
  }
  if (cleanup.length) throw new AggregateError(cleanup, "comfort context cleanup failed");
}
