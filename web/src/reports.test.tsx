import { render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ReportsView } from "./reports";

const summary = (id: string, status: "ok" | "blocked") => ({
  id, run: id.split("-20")[0], title: `Page ${id}`, at: "2026-10-19T09:00:00-04:00", status, personal: true, counts: {}, commands: [],
  done: [{ id: "a", label: "did a", command: "x" }], blockers: status === "blocked" ? [{ id: "b", label: "b", why: "w", command: "y" }] : [], next: [],
});

function mockApi(reports: unknown[]) {
  const calls: string[] = [];
  vi.stubGlobal("fetch", vi.fn((url: string) => {
    calls.push(String(url));
    const body = String(url).endsWith("/api/reports")
      ? { reports }
      : { summary: summary(decodeURIComponent(String(url).split("/").pop()!), "ok"), html: "<!doctype html><h1>Synthetic run page</h1>" };
    return Promise.resolve(new Response(JSON.stringify(body)));
  }));
  return calls;
}

afterEach(() => vi.unstubAllGlobals());

describe("Reports view (run pages)", () => {
  it("lists run pages newest first and shows the newest in a sandboxed frame", async () => {
    const calls = mockApi([summary("review-20261019-090000-2", "blocked"), summary("day-plan-20261019-090000", "ok")]);
    render(<ReportsView />);
    const frame = await screen.findByTitle("Run page: Page review-20261019-090000-2");
    expect(frame).toHaveAttribute("sandbox", "");
    expect(frame.getAttribute("srcdoc")).toContain("Synthetic run page");
    expect(screen.getByRole("link", { name: "Page review-20261019-090000-2" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByText("blocked")).toBeInTheDocument();
    expect(screen.getByText(/1 blocker,/)).toBeInTheDocument();
    expect(calls).toContain("/api/reports/review-20261019-090000-2");
  });

  it("opens the page a link names", async () => {
    const calls = mockApi([summary("review-20261019-090000-2", "ok"), summary("day-plan-20261019-090000", "ok")]);
    render(<ReportsView id="day-plan-20261019-090000" />);
    expect(await screen.findByTitle("Run page: Page day-plan-20261019-090000")).toBeInTheDocument();
    expect(calls).toContain("/api/reports/day-plan-20261019-090000");
  });

  it("says which runs make a page when there is none", async () => {
    const calls = mockApi([]);
    render(<ReportsView />);
    expect(await screen.findByText(/No run pages yet/)).toBeInTheDocument();
    expect(calls.filter((c) => c.startsWith("/api/reports/"))).toEqual([]);
  });
});
