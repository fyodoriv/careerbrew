import { act, render, screen } from "@testing-library/react";
import { afterEach, expect, it, vi } from "vitest";
import { api } from "./api";
import type { FeedHealth } from "./api";
import { FeedHealthNotice, SourceAge } from "./feedHealth";
import { useFeedHealth } from "./useFeedHealth";

const current: FeedHealth = {
  version: 1, as_of: "2026-11-05T12:00:00Z", status: "healthy", reason: "current",
  running: false, mode: "direct", required: 1, healthy: 1,
  sources: [{ name: "tickets", required: true, health: "healthy", acquired_at: "2026-11-05T11:45:10Z", checked_at: "2026-11-05T12:00:00Z", age_seconds: 890, max_age_minutes: 15 }],
  next_expiry: "2026-11-05T12:00:11Z", commands: [],
  row_review: { status: "due", checked_at: "2026-11-05T10:00:00Z", age_seconds: 7200, max_age_minutes: 60 },
};

afterEach(() => { vi.restoreAllMocks(); vi.useRealTimers(); });

function LiveNotice() {
  const q = useFeedHealth();
  return <FeedHealthNotice data={q.data} />;
}

it.each([
  ["missing", "Work sources are not set up."], ["stopped", "Work source refresh is stopped."],
  ["stale", "Work sources need a fresh pull."], ["error", "The latest work source check failed."],
  ["unknown", "Work source freshness is unknown."],
] as const)("406-13 %s has an accessible truthful notice", (status, text) => {
  render(<FeedHealthNotice data={{ ...current, status, commands: ["careerbrew work live cycle"] }} />);
  expect(screen.getByRole("region", { name: "Work source health" })).toHaveTextContent(text);
  expect(screen.getByRole("region")).toHaveTextContent("Recent work may be missing from Today and Standup.");
  expect(screen.getByRole("region")).toHaveTextContent(status === "missing" ? "Set up my Careerbrew work feed." : status === "stopped" ? "Start refreshing my Careerbrew work feed." : "Check and refresh my Careerbrew work sources.");
  expect(screen.queryByText("careerbrew work live cycle")).not.toBeInTheDocument();
});

it("406-14/15 direct activity preserves old row-fact review and adapter state", () => {
  render(<FeedHealthNotice data={current} />);
  expect(screen.getByRole("region")).toHaveTextContent("Adapters stopped");
  expect(screen.getByRole("region")).toHaveTextContent("review is due");
});

it("406-16 an ambiguous id prefix is not source provenance", () => {
  render(<SourceAge row={{ id: "tickets:ambiguous", title: "safe", at: "now", kind: "note", source: "evidence" }} health={current} />);
  expect(screen.getByText(/Source freshness unknown/)).toBeVisible();
});

it("406-17 clock-only expiry refreshes a visible notice and cleans up", async () => {
  vi.useFakeTimers();
  const fetch = vi.spyOn(api, "feedHealth").mockResolvedValueOnce(current).mockResolvedValue({ ...current, status: "stale", next_expiry: null });
  const view = render(<LiveNotice />);
  await act(async () => { await Promise.resolve(); });
  expect(fetch).toHaveBeenCalledTimes(1);
  await act(async () => { await vi.advanceTimersByTimeAsync(12000); });
  expect(fetch).toHaveBeenCalledTimes(2);
  expect(screen.getByRole("region")).toHaveTextContent("need a fresh pull");
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("406-17 hidden views pause and recheck immediately on visibility return", async () => {
  vi.useFakeTimers();
  let visible = "hidden";
  vi.spyOn(document, "visibilityState", "get").mockImplementation(() => visible as DocumentVisibilityState);
  const fetch = vi.spyOn(api, "feedHealth").mockResolvedValue(current);
  const view = render(<LiveNotice />);
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(120000); });
  expect(fetch).toHaveBeenCalledTimes(1);
  visible = "visible";
  await act(async () => { document.dispatchEvent(new Event("visibilitychange")); await Promise.resolve(); });
  expect(fetch).toHaveBeenCalledTimes(2);
  view.unmount();
  expect(vi.getTimerCount()).toBe(0);
});

it("406-17 a failed refresh becomes unknown and retries without a write", async () => {
  vi.useFakeTimers();
  const fetch = vi.spyOn(api, "feedHealth").mockResolvedValueOnce(current).mockRejectedValueOnce(new Error("synthetic unavailable")).mockResolvedValue({ ...current, status: "stale", next_expiry: null });
  render(<LiveNotice />);
  await act(async () => { await Promise.resolve(); });
  await act(async () => { await vi.advanceTimersByTimeAsync(12000); });
  expect(screen.getByRole("region")).toHaveTextContent("freshness is unknown");
  await act(async () => { await vi.advanceTimersByTimeAsync(60000); });
  expect(fetch).toHaveBeenCalledTimes(3);
  expect(screen.getByRole("region")).toHaveTextContent("need a fresh pull");
});
