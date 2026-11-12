import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { startLiveRefresh } from "./live";
import { useData } from "./useData";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

function Probe({ live }: { live: boolean }) {
  const q = useData(() => fetch("/api/thing").then((r) => r.json() as Promise<{ n: number }>), [], { live });
  return <p>n={q.data?.n ?? "-"}</p>;
}

describe("live refresh (issue #55)", () => {
  it("refetches live views when the change version moves, and only them", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let version = "a";
    let n = 0;
    vi.stubGlobal("fetch", vi.fn((url: string) => Promise.resolve(new Response(JSON.stringify(String(url).endsWith("/api/changes") ? { version } : { n: ++n }), { status: 200 }))));
    const stop = startLiveRefresh(1000);
    render(<><Probe live /><div data-testid="static"><Probe live={false} /></div></>);
    expect(await screen.findByText("n=1")).toBeInTheDocument();
    expect(await screen.findByText("n=2")).toBeInTheDocument();
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(await screen.findByText("n=3")).toBeInTheDocument();
    expect(screen.getByTestId("static").textContent).toBe("n=2");
    expect(n).toBe(3);
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(n).toBe(3);
    version = "b";
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(await screen.findByText("n=4")).toBeInTheDocument();
    expect(screen.getByTestId("static").textContent).toBe("n=2");
    stop();
  });
  it("refreshes a live view when a write precedes the first version response", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    let releaseFirst: ((response: Response) => void) | undefined;
    let first = true;
    let n = 0;
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      if (!String(url).endsWith("/api/changes")) return Promise.resolve(new Response(JSON.stringify({ n }), { status: 200 }));
      if (first) {
        first = false;
        return new Promise<Response>((resolve) => { releaseFirst = resolve; });
      }
      return Promise.resolve(new Response(JSON.stringify({ version: "after" }), { status: 200 }));
    }));
    const stop = startLiveRefresh(1000);
    render(<Probe live />);
    expect(await screen.findByText("n=0")).toBeInTheDocument();
    n = 1;
    expect(releaseFirst).toBeDefined();
    await act(async () => {
      releaseFirst?.(new Response(JSON.stringify({ version: "after" }), { status: 200 }));
      await vi.advanceTimersByTimeAsync(2100);
    });
    expect(screen.getByText("n=1")).toBeInTheDocument();
    stop();
  });

  it("ignores the first version response after stop", async () => {
    let release: ((response: Response) => void) | undefined;
    const listener = vi.fn();
    window.addEventListener("careerbrew:changed", listener);
    vi.stubGlobal("fetch", vi.fn(() => new Promise<Response>((resolve) => { release = resolve; })));
    const stop = startLiveRefresh(1000);
    stop();
    await act(async () => { release?.(new Response(JSON.stringify({ version: "after" }), { status: 200 })); });
    expect(listener).not.toHaveBeenCalled();
    window.removeEventListener("careerbrew:changed", listener);
  });

  it("does not let an older in-flight read replace the first refresh", async () => {
    let releaseVersion: ((response: Response) => void) | undefined;
    let releaseOld: ((response: Response) => void) | undefined;
    let loads = 0;
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      if (String(url).endsWith("/api/changes")) return new Promise<Response>((resolve) => { releaseVersion = resolve; });
      loads += 1;
      if (loads === 1) return new Promise<Response>((resolve) => { releaseOld = resolve; });
      return Promise.resolve(new Response(JSON.stringify({ n: 1 }), { status: 200 }));
    }));
    const stop = startLiveRefresh(1000);
    render(<Probe live />);
    await act(async () => { releaseVersion?.(new Response(JSON.stringify({ version: "after" }), { status: 200 })); });
    expect(await screen.findByText("n=1")).toBeInTheDocument();
    await act(async () => { releaseOld?.(new Response(JSON.stringify({ n: 0 }), { status: 200 })); });
    expect(screen.getByText("n=1")).toBeInTheDocument();
    stop();
  });

  it("retries a failed initial poll and refreshes on its first success", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const listener = vi.fn();
    window.addEventListener("careerbrew:changed", listener);
    vi.stubGlobal("fetch", vi.fn().mockRejectedValueOnce(new Error("offline")).mockResolvedValue(new Response(JSON.stringify({ version: "after" }), { status: 200 })));
    const stop = startLiveRefresh(1000);
    await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
    expect(listener).toHaveBeenCalledTimes(1);
    stop();
    window.removeEventListener("careerbrew:changed", listener);
  });

});
