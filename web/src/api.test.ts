import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { api, ApiError, captureToken, duration, hhmm, keepSignIn, openSignedInTab, pct, pmCopyText, SIGNED_OUT_EVENT, signIn } from "./api";

describe("token handling", () => {
  beforeEach(() => sessionStorage.clear());
  it("moves the token from the fragment to sessionStorage and strips it from the URL", () => {
    window.history.replaceState(null, "", "/#token=abc_123");
    expect(captureToken()).toBe("abc_123");
    expect(window.location.hash).not.toContain("abc_123");
    expect(sessionStorage.getItem("careerbrew-token")).toBe("abc_123");
  });
  it("sends the token header and raises ApiError with the server message", async () => {
    sessionStorage.setItem("careerbrew-token", "t1");
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify({ error: "job-hunt views are personal-only" }), { status: 403 }));
    vi.stubGlobal("fetch", fetchMock);
    await expect(api.jobs()).rejects.toBeInstanceOf(ApiError);
    expect(fetchMock.mock.calls[0][1].headers["X-Careerbrew-Token"]).toBe("t1");
  });
});

describe("sign-in across restarts (issue #141)", () => {
  beforeEach(() => sessionStorage.clear());
  afterEach(() => vi.unstubAllGlobals());
  const reply = (status: number, body: unknown) => vi.fn().mockResolvedValue(new Response(JSON.stringify(body), { status }));

  it("a 401 from any view tells the app it is signed out", async () => {
    sessionStorage.setItem("careerbrew-token", "rotated");
    vi.stubGlobal("fetch", reply(401, { error: "missing or wrong token" }));
    const seen = vi.fn();
    window.addEventListener(SIGNED_OUT_EVENT, seen);
    await expect(api.today()).rejects.toBeInstanceOf(ApiError);
    window.removeEventListener(SIGNED_OUT_EVENT, seen);
    expect(seen).toHaveBeenCalledTimes(1);
  });

  it("a tab with the token asks for the sign-in cookie with the token header", async () => {
    sessionStorage.setItem("careerbrew-token", "t1");
    const fetchMock = reply(200, { signed_in: true, cookie: true });
    vi.stubGlobal("fetch", fetchMock);
    await keepSignIn();
    expect(fetchMock.mock.calls[0][0]).toBe("/api/session");
    expect(fetchMock.mock.calls[0][1]).toMatchObject({ method: "POST", headers: { "X-Careerbrew-Token": "t1" } });
  });

  it("signs in from the cookie, drops a token the server refuses, and says when the app does not answer", async () => {
    vi.stubGlobal("fetch", reply(200, { token: "from-cookie" }));
    expect(await signIn()).toBe("ok");
    expect(sessionStorage.getItem("careerbrew-token")).toBe("from-cookie");
    vi.stubGlobal("fetch", reply(401, { error: "missing or wrong token" }));
    expect(await signIn()).toBe("signed-out");
    expect(sessionStorage.getItem("careerbrew-token")).toBeNull();
    vi.stubGlobal("fetch", vi.fn().mockRejectedValue(new TypeError("Failed to fetch")));
    expect(await signIn()).toBe("offline");
  });

  it("the sign-in page's button asks the server to open a tab and never sees the token", async () => {
    const fetchMock = reply(200, { opened: true });
    vi.stubGlobal("fetch", fetchMock);
    expect(await openSignedInTab("week")).toBeNull();
    expect(fetchMock.mock.calls[0][0]).toBe("/api/session/open");
    expect(JSON.parse(fetchMock.mock.calls[0][1].body)).toEqual({ view: "week" });
    vi.stubGlobal("fetch", reply(409, { error: "could not open a browser from the web app here (fix: run `careerbrew open` in a terminal)" }));
    expect(await openSignedInTab("week")).toMatch(/careerbrew open/);
  });
});

describe("formatting", () => {
  it("formats times and percentages", () => {
    expect(hhmm("2026-10-19T09:30:00-04:00")).toBe("09:30");
    expect(pct(0.456)).toBe("46%");
    expect(pct(null)).toBe("—");
  });
  it("formats minutes as hours and minutes", () => {
    expect(duration(45)).toBe("45m");
    expect(duration(120)).toBe("2h");
    expect(duration(465)).toBe("7h 45m");
    expect(duration(-5)).toBe("0m");
  });
});

describe("PM update copy text (issue #145)", () => {
  const body = "Done today:\n• billing: [close the ledger](https://jira.example.com/browse/BILL-7)\n    ◦ see <https://docs.example.com/ledger>\n• tooling: [https://github.example.com/o/r/pull/3](https://github.example.com/o/r/pull/3)\n\n";
  it("copies the body only for Slack, with bullets, indentation, and delimited fallback URLs", () => {
    expect(pmCopyText({ subject: "Daily update 2026-10-19", via: "slack" }, body)).toBe(
      "Done today:\n• billing: close the ledger <https://jira.example.com/browse/BILL-7>\n    ◦ see <https://docs.example.com/ledger>\n• tooling: <https://github.example.com/o/r/pull/3>",
    );
  });
  it("puts the subject line first for email", () => {
    expect(pmCopyText({ subject: "Daily update 2026-10-19", via: "email" }, "billing: 3 commits.")).toBe("Daily update 2026-10-19\n\nbilling: 3 commits.");
  });
});
