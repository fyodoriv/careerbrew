import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import App from "./App";

beforeEach(() => sessionStorage.setItem("careerbrew-token", "synthetic-token"));  // a tab opened with a careerbrew link
afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  window.location.hash = "";
  sessionStorage.clear();
});

const OVERVIEW = { plugins: [], watcher: { state: "off", running: false, reloads: 0, today: {} } };

// Answers /api/overview when `answer` is called; every other endpoint fails, so views show their error and never crash.
function stubServer() {
  const calls: string[] = [];
  let answer: (role: string) => void = () => {};
  const role = new Promise<string>((r) => (answer = r));
  vi.stubGlobal("fetch", vi.fn((url: string) => {
    calls.push(String(url));
    if (String(url).endsWith("/api/overview")) return role.then((r) => new Response(JSON.stringify({ ...OVERVIEW, role: r }), { status: 200 }));
    return Promise.resolve(new Response(JSON.stringify({ error: "synthetic" }), { status: 500 }));
  }));
  return { calls, answer };
}

const navLabels = () => [...document.querySelectorAll(".nav-label")].map((e) => e.textContent);

describe("app shell on a corporate machine (issue #52)", () => {
  it("hides job-hunt views and never fetches their endpoints", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      calls.push(String(url));
      const body = String(url).endsWith("/api/overview") ? { role: "corporate", plugins: [], watcher: { state: "off", running: false, reloads: 0, today: {} } } : {};
      return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }));
    }));
    window.location.hash = "#/jobs";
    render(<App />);
    expect(await screen.findByText(/job-hunt views are personal-only/)).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Job hunt" })).toBeNull();
    expect(screen.queryByRole("link", { name: "Practice" })).toBeNull();
    expect(calls.some((c) => /\/api\/(jobs|interviews|cards|library)/.test(c))).toBe(false);
  });
});

describe("sidebar groups follow the blocked views (issue #65)", () => {
  it("never shows a blocked link while the machine role loads, then drops the group whose every view is blocked", async () => {
    const { calls, answer } = stubServer();
    window.location.hash = "#/jobs";
    render(<App />);
    expect(await screen.findByRole("heading", { level: 1, name: "Job hunt" })).toBeInTheDocument();
    expect(screen.queryByRole("link", { name: "Job hunt" })).toBeNull();  // the role is not known yet
    expect(navLabels()).not.toContain("Job search");
    answer("corporate");
    expect(await screen.findByRole("link", { name: "Today" })).toBeInTheDocument();
    expect(navLabels()).toEqual(["Plan", "System"]);
    for (const name of ["Job hunt", "Interviews", "Practice", "Library"]) expect(screen.queryByRole("link", { name })).toBeNull();
    expect(screen.getByText(/job-hunt views are personal-only/)).toBeInTheDocument();
    expect(calls.some((c) => /\/api\/(jobs|interviews|cards|library|explanations|exercises|drill)/.test(c))).toBe(false);
  });

  it("shows every group on a personal machine", async () => {
    const { answer } = stubServer();
    window.location.hash = "#/review";
    render(<App />);
    answer("personal");
    expect(await screen.findByRole("link", { name: "Job hunt" })).toBeInTheDocument();
    expect(navLabels()).toEqual(["Plan", "Job search", "System"]);
  });
});

describe("sign-in across restarts (issue #141)", () => {
  // A synthetic server: /api/session answers from `cookie` (the browser sends it), /api/overview needs `token` in the header.
  function stubSession(opts: { cookie: string | null; token: string; open?: () => void }) {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      const path = String(url);
      const method = init?.method ?? "GET";
      calls.push(`${method} ${path}`);
      const json = (status: number, body: unknown) => Promise.resolve(new Response(JSON.stringify(body), { status }));
      const sent = (init?.headers as Record<string, string> | undefined)?.["X-Careerbrew-Token"];
      if (path === "/api/session" && method === "GET") return opts.cookie === opts.token ? json(200, { token: opts.cookie }) : json(401, { error: "missing or wrong token" });
      if (path === "/api/session/open") return (opts.open?.(), json(200, { opened: true }));
      if (sent !== opts.token) return json(401, { error: "missing or wrong token (fix: run `careerbrew open` in a terminal)" });
      if (path === "/api/session") return json(200, { signed_in: true, cookie: true });
      if (path === "/api/overview") return json(200, { ...OVERVIEW, role: "personal" });
      return json(500, { error: "synthetic" });
    }));
    return calls;
  }

  it("a bookmark with no link signs in from the cookie and keeps the token for this tab", async () => {
    sessionStorage.clear();
    const calls = stubSession({ cookie: "fresh-token", token: "fresh-token" });
    window.location.hash = "#/review";
    render(<App />);
    expect(await screen.findByRole("link", { name: "Job hunt" })).toBeInTheDocument();
    expect(sessionStorage.getItem("careerbrew-token")).toBe("fresh-token");
    expect(calls[0]).toBe("GET /api/session");
    expect(screen.queryByRole("heading", { name: "Sign in to careerbrew" })).toBeNull();
  });

  it("a tab with a link asks for the cookie once and renders at once", async () => {
    const calls = stubSession({ cookie: null, token: "synthetic-token" });
    window.location.hash = "#/review";
    render(<App />);
    expect(await screen.findByRole("link", { name: "Job hunt" })).toBeInTheDocument();
    expect(calls.filter((c) => c === "POST /api/session")).toHaveLength(1);
    expect(calls).not.toContain("GET /api/session");
  });

  it("a rotated token shows the sign-in page with the one command and a button that opens a signed-in tab", async () => {
    let cookie: string | null = "old-token";
    const opened = vi.fn(() => (cookie = "new-token"));
    sessionStorage.setItem("careerbrew-token", "old-token");  // the tab and its cookie hold the token from before the rotation
    stubSession({ get cookie() { return cookie; }, token: "new-token", open: opened });
    window.location.hash = "#/review";
    render(<App />);
    expect(await screen.findByRole("heading", { name: "Sign in to careerbrew" })).toBeInTheDocument();
    expect(screen.getByText("careerbrew open")).toBeInTheDocument();
    expect(sessionStorage.getItem("careerbrew-token")).toBeNull();  // the old token is dropped
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      fireEvent.click(screen.getByRole("button", { name: "Open a signed-in tab" }));
      expect(await screen.findByText(/A signed-in tab opened/)).toBeInTheDocument();
      expect(opened).toHaveBeenCalledTimes(1);
      await act(async () => { await vi.advanceTimersByTimeAsync(1100); });
      expect(await screen.findByRole("link", { name: "Job hunt" })).toBeInTheDocument();
      expect(screen.queryByRole("heading", { name: "Sign in to careerbrew" })).toBeNull();
      expect(sessionStorage.getItem("careerbrew-token")).toBe("new-token");
    } finally {
      vi.useRealTimers();
    }
  });
});
