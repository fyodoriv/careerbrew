import type { BrowserContext, Page } from "@playwright/test";
import { TRACE_IN_SANDBOX } from "../playwright.config";

export function trackPageErrors(page: Page, errors: string[]): void {
  page.on("console", (m) => m.type() === "error" && !/Failed to load resource/.test(m.text()) && !TRACE_IN_SANDBOX.test(m.text()) && errors.push(m.text()));
  page.on("pageerror", (e) => errors.push(e.message));
}

/** Wait for this visit's page routes before closing its page. */
export async function finishVisit(page: Page): Promise<void> {
  await page.unrouteAll({ behavior: "wait" });
}

/** Own one visit page, including cleanup when its callback fails. */
export async function runOwnedVisit(context: BrowserContext, visit: (page: Page) => Promise<void>): Promise<void> {
  const page = await context.newPage();
  const errors: unknown[] = [];
  try {
    await visit(page);
  } catch (error) {
    errors.push(error);
  }
  try {
    await finishVisit(page);
  } catch (error) {
    errors.push(error);
  } finally {
    try {
      await page.close();
    } catch (error) {
      errors.push(error);
    }
  }
  if (errors.length === 1) throw errors[0];
  if (errors.length > 1) throw new AggregateError(errors, "Visit and cleanup failed");
}
