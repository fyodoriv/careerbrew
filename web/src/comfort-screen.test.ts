import { expect, test, vi } from "vitest";
import type { Browser, BrowserContext, Page } from "@playwright/test";
import { withComfortScreens } from "../e2e/comfort-screen";

function ownedContext() {
  const page = {} as Page;
  const newPage = vi.fn().mockResolvedValue(page);
  const close = vi.fn().mockResolvedValue(undefined);
  return { context: { newPage, close } as unknown as BrowserContext, page, newPage, close };
}

function borrowedBrowser(contexts: BrowserContext[]) {
  const foreignClose = vi.fn();
  const enumerate = vi.fn(() => [{ close: foreignClose }]);
  const close = vi.fn();
  const newContext = vi.fn();
  for (const context of contexts) newContext.mockResolvedValueOnce(context);
  return { browser: { newContext, contexts: enumerate, close } as unknown as Browser, newContext, enumerate, close, foreignClose };
}

test("comfort scope closes only its own contexts and preserves options", async () => {
  const first = ownedContext(), second = ownedContext();
  const owner = borrowedBrowser([first.context, second.context]);
  const project = { baseURL: "http://127.0.0.1:1", permissions: ["microphone"], reducedMotion: "reduce" as const };
  const options = { viewport: { width: 375, height: 812 }, permissions: ["microphone", "clipboard-read", "clipboard-write"] };
  await withComfortScreens(owner.browser, project, async (screen) => {
    expect(await screen(options)).toBe(first.page);
    expect(await screen({ hasTouch: true })).toBe(second.page);
    expect(first.close).not.toHaveBeenCalled();
  });
  expect(owner.newContext.mock.calls).toEqual([[{ ...project, ...options }], [{ ...project, hasTouch: true }]]);
  expect(first.close).toHaveBeenCalledTimes(1);
  expect(second.close).toHaveBeenCalledTimes(1);
  expect(owner.enumerate).not.toHaveBeenCalled();
  expect(owner.foreignClose).not.toHaveBeenCalled();
  expect(owner.close).not.toHaveBeenCalled();
});

test("comfort scope closes its context when the body fails", async () => {
  const context = ownedContext();
  const owner = borrowedBrowser([context.context]);
  const primary = new Error("synthetic body failure");
  await expect(withComfortScreens(owner.browser, {}, async (screen) => {
    await screen({});
    throw primary;
  })).rejects.toBe(primary);
  expect(context.close).toHaveBeenCalledTimes(1);
});

test("comfort scope closes a context whose newPage fails", async () => {
  const context = ownedContext();
  const owner = borrowedBrowser([context.context]);
  const primary = new Error("synthetic newPage failure");
  context.newPage.mockRejectedValueOnce(primary);
  await expect(withComfortScreens(owner.browser, {}, async (screen) => {
    await screen({});
  })).rejects.toBe(primary);
  expect(context.close).toHaveBeenCalledTimes(1);
});

test("comfort scope attempts all closes and retains cleanup and body errors", async () => {
  const first = ownedContext(), second = ownedContext();
  const owner = borrowedBrowser([first.context, second.context]);
  const firstError = new Error("synthetic first close failure"), secondError = new Error("synthetic second close failure");
  first.close.mockRejectedValueOnce(firstError);
  second.close.mockRejectedValueOnce(secondError);
  const failure = await withComfortScreens(owner.browser, {}, async (screen) => {
    await screen({});
    await screen({});
  }).catch((error: unknown) => error);
  expect(failure).toBeInstanceOf(AggregateError);
  expect((failure as AggregateError).errors).toEqual([secondError, firstError]);
  expect(first.close).toHaveBeenCalledTimes(1);
  expect(second.close).toHaveBeenCalledTimes(1);
  const body = new Error("synthetic primary body failure");
  const third = ownedContext();
  third.close.mockRejectedValueOnce(firstError);
  const combined = await withComfortScreens(borrowedBrowser([third.context]).browser, {}, async (screen) => {
    await screen({});
    throw body;
  }).catch((error: unknown) => error);
  expect(combined).toBeInstanceOf(AggregateError);
  expect((combined as AggregateError).errors).toEqual([body, firstError]);
});
