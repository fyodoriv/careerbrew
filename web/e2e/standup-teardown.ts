import { expect, type Page } from "@playwright/test";
import { finishVisit } from "./controls-route-owner";

/** Complete the standup page's error check at the end of a test. */
export async function finishStandup(page: Page, errors: string[]): Promise<void> {
  await finishVisit(page);
  expect(errors).toEqual([]);
}
