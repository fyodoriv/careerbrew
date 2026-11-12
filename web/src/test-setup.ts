import "@testing-library/jest-dom/vitest";
import { cleanup } from "@testing-library/react";
import { afterEach, vi } from "vitest";
import { clearCache } from "./cache";

// The page's cache of each view (issue #142) lives as long as the page: start every test without it.
// Testing Library unmounts on its own only with vitest `globals`, which this repo does not use: without this, every
// view a test renders stays mounted and keeps fetching through the next tests' stubs (views.test flake, 2026-10-25).
afterEach(() => {
  cleanup();
  clearCache();
  sessionStorage.clear();
});

// Time saved (US-86): the page's timing posts are measurements, not the owner's actions, so the view tests that count
// every POST never see them. timing.test.ts tests the real module.
vi.mock("./timing", () => ({ useOpened: vi.fn(), timeDone: vi.fn() }));
