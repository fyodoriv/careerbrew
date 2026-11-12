import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type * as Timing from "./timing";

// test-setup.ts mocks ./timing for the view tests; this file tests the real module.
const { timeDone, useOpened } = await vi.importActual<typeof Timing>("./timing");

function Shows({ saves }: { saves: Timing.Saves }) {
  useOpened(saves, true);
  return null;
}

function posts(): { saves: string; step: string; seconds: number }[] {
  const spy = vi.mocked(fetch);
  return spy.mock.calls.filter(([url]) => String(url) === "/api/timing").map(([, init]) => JSON.parse(String(init?.body)));
}

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe("time saved: the owner's own seconds (US-86)", () => {
  it("sends the open, then the seconds up to the first Copy or Send only, and never any text", async () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-27T09:00:00Z"));
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ logged: true })))));
    const page = render(<><Shows saves="pm_update" /><Shows saves="pm_update" /></>);
    vi.setSystemTime(new Date("2026-10-27T09:02:30Z"));
    timeDone("pm_update", "copy");
    timeDone("pm_update", "send");  // the same open: not a second run
    expect(posts()).toEqual([{ saves: "pm_update", step: "open", seconds: 0 }, { saves: "pm_update", step: "copy", seconds: 150 }]);
    page.unmount();
    timeDone("pm_update", "copy");  // no open result: nothing to time
    expect(posts()).toHaveLength(2);
  });

  it("times each Accept of the inbox from the previous one", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-27T09:00:00Z"));
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ logged: true })))));
    const page = render(<Shows saves="inbox" />);
    vi.setSystemTime(new Date("2026-10-27T09:00:20Z"));
    timeDone("inbox", "accept");
    vi.setSystemTime(new Date("2026-10-27T09:00:50Z"));
    timeDone("inbox", "accept");
    expect(posts().map((p) => p.seconds)).toEqual([0, 20, 30]);
    page.unmount();
  });

  it("does not count the time the tab was hidden", () => {
    vi.useFakeTimers({ toFake: ["Date"] });
    vi.setSystemTime(new Date("2026-10-27T09:00:00Z"));
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ logged: true })))));
    const page = render(<Shows saves="standup" />);
    const hide = (hidden: boolean) => {
      Object.defineProperty(document, "hidden", { value: hidden, configurable: true });
      document.dispatchEvent(new Event("visibilitychange"));
    };
    vi.setSystemTime(new Date("2026-10-27T09:00:10Z"));
    hide(true);
    vi.setSystemTime(new Date("2026-10-27T10:00:10Z"));
    hide(false);
    vi.setSystemTime(new Date("2026-10-27T10:00:15Z"));
    timeDone("standup", "copy");
    expect(posts().at(-1)).toEqual({ saves: "standup", step: "copy", seconds: 15 });
    page.unmount();
    Reflect.deleteProperty(document, "hidden");
  });

  it("a failed send changes nothing on the page", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.reject(new TypeError("offline"))));
    const page = render(<Shows saves="standup" />);
    expect(() => timeDone("standup", "copy")).not.toThrow();
    page.unmount();
  });
});
