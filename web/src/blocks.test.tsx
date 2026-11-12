import { webcrypto } from "node:crypto";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ApiError } from "./api";
import { CHANGED_EVENT, startLiveRefresh } from "./live";
import { useData } from "./useData";
import { State } from "./views";

// Issue #142: every block says what it loads, fails on its own with Retry, and shows its last answer at once.

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.resetModules();
});

const thing = Object.assign(() => fetch("/api/thing").then(async (r) => {
  if (!r.ok) throw new ApiError(r.status, "synthetic failure");
  return r.json() as Promise<{ n: number }>;
}), { key: "/api/thing" });

function Thing() {
  const q = useData(thing);
  return <State q={q} label="the thing">{(d) => <p>n={d.n}</p>}</State>;
}

// Answers /api/thing in order from `answers` (a number, or an HTTP status to fail with); each waits for `gate`.
function serve(answers: (number | { status: number })[], gate: Promise<void> = Promise.resolve()) {
  vi.stubGlobal("fetch", vi.fn(async (url: string) => {
    if (String(url).endsWith("/api/tab-key")) return new Response(JSON.stringify({ key: null }));
    await gate;
    const a = answers.shift();
    if (typeof a === "object") return new Response(JSON.stringify({ error: "synthetic failure" }), { status: a.status });
    return new Response(JSON.stringify({ n: a }));
  }));
}

describe("a block (issue #142)", () => {
  it("says what it loads at once, and how long once it takes 3 s", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    serve([1], new Promise(() => {}));
    render(<Thing />);
    expect(screen.getByText("Loading the thing…")).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(3100); });
    expect(screen.getByText("Loading the thing… 3 s")).toBeVisible();
  });

  it("fails on its own with the fix and Retry, and Retry loads it", async () => {
    serve([{ status: 500 }, 2]);
    render(<Thing />);
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not load the thing: synthetic failure. Fix:");
    expect(alert).toHaveTextContent("scripts/doctor.sh");
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("n=2")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows its last answer at once on a view switch, marked Updated, then the fresh one", async () => {
    serve([1]);
    const first = render(<Thing />);
    expect(await screen.findByText("n=1")).toBeInTheDocument();
    first.unmount();
    let release = () => {};
    serve([2], new Promise<void>((r) => (release = r)));
    render(<Thing />);
    expect(screen.getByText("n=1")).toBeInTheDocument();  // no skeleton: the last answer, from memory
    expect(screen.getByText(/^Updated \d{2}:\d{2} · refreshing…$/)).toBeInTheDocument();
    release();
    expect(await screen.findByText("n=2")).toBeInTheDocument();
    expect(screen.queryByText(/^Updated/)).toBeNull();
  });

  it("says new data is coming when /api/changes moved since its last answer", async () => {
    serve([1]);
    const first = render(<Thing />);
    await screen.findByText("n=1");
    first.unmount();
    // The page's live refresh sees the server's change version move from v1 to v2.
    const versions = ["v1", "v2"];
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ version: versions.length > 1 ? versions.shift() : versions[0] }))));
    const changed = new Promise<void>((r) => window.addEventListener(CHANGED_EVENT, () => r(), { once: true }));
    const stop = startLiveRefresh(10);
    await changed;
    stop();
    let release = () => {};
    serve([2], new Promise<void>((r) => (release = r)));
    render(<Thing />);
    expect(screen.getByText("n=1")).toBeInTheDocument();
    expect(screen.getByText(/^Updated \d{2}:\d{2} · new data, refreshing…$/)).toBeInTheDocument();
    release();
    expect(await screen.findByText("n=2")).toBeInTheDocument();
    expect(screen.queryByText(/^Updated/)).toBeNull();
  });

  it("keeps its last answer when the refresh fails, with the error and Retry", async () => {
    serve([1]);
    const first = render(<Thing />);
    await screen.findByText("n=1");
    first.unmount();
    serve([{ status: 500 }]);
    render(<Thing />);
    expect(await screen.findByRole("alert")).toHaveTextContent("Could not load the thing");
    expect(screen.getByText("n=1")).toBeInTheDocument();
    expect(screen.getByText(/· not refreshed$/)).toBeInTheDocument();
  });
});

describe("the tab's copy (issue #142)", () => {
  const stored = () => Array.from({ length: sessionStorage.length }, (_, i) => sessionStorage.key(i) ?? "").filter((k) => k.startsWith("careerbrew-cache:"));

  function server(key: string | null) {
    vi.stubGlobal("crypto", webcrypto);
    vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ key }))));
  }
  const newKey = () => Buffer.from(webcrypto.getRandomValues(new Uint8Array(32))).toString("base64");

  it("is encrypted with the server's key, names no endpoint, and comes back after a reload", async () => {
    const key = newKey();
    server(key);
    const page = await import("./cache");
    page.save("/api/interviews/Northwind", { note: "canary business text" });
    await vi.waitFor(() => expect(stored()).toHaveLength(1));
    const raw = stored().map((k) => k + sessionStorage.getItem(k)).join("");
    expect(raw).not.toContain("canary business text");
    expect(raw).not.toContain("Northwind");  // the entry's name is a hash
    expect(raw).not.toContain(key);
    vi.resetModules();
    const reloaded = await import("./cache");  // a reload: memory is gone, the tab keeps its copy
    expect(reloaded.peek("/api/interviews/Northwind")).toBeUndefined();
    expect((await reloaded.restore("/api/interviews/Northwind"))?.data).toEqual({ note: "canary business text" });
  });

  it("cannot be read by another server, and is dropped", async () => {
    server(newKey());
    const page = await import("./cache");
    page.save("/api/today", { n: 1 });
    await vi.waitFor(() => expect(stored()).toHaveLength(1));
    vi.resetModules();
    server(newKey());  // careerbrew restarted: a new key
    const reloaded = await import("./cache");
    expect(await reloaded.restore("/api/today")).toBeUndefined();
    expect(stored()).toHaveLength(0);
  });

  it("is never written with web_tab_cache off", async () => {
    server(null);
    const page = await import("./cache");
    page.save("/api/today", { n: 1 });
    await new Promise((r) => setTimeout(r, 50));
    expect(stored()).toHaveLength(0);
    expect(page.peek("/api/today")?.data).toEqual({ n: 1 });  // memory still serves view switches
  });
});
