import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { NotificationItem, Notifications } from "./api";
import { NotificationsView } from "./notifications";

const status = { enabled: true, min_priority: 4, quiet_hours: "19:00-08:00", daily_cap: 6, batch_minutes: 10, snooze_minutes: 60, backend_kind: "terminal-notifier", backend_note: "terminal-notifier: a click opens the notification in the web app", quiet_now: false, sent_today: 1, open: 2, unread: 2 };
const base: Omit<NotificationItem, "id" | "kind" | "headline" | "actions"> = { source: "slack", title: "", why: "it was sent to you directly", next: "reply in Slack", priority: 4, priority_why: "4 for a direct message", count: 1, at: "2026-10-21T13:30:00Z", status: "new", snooze_until: "", resolved: "", url: "", link_label: "", accept_label: "", popup: "shown as a pop-up" };
const view: Notifications = {
  status,
  items: [
    { ...base, id: "n-dm", kind: "dm", headline: "Slack: a direct message asks you", title: "DM from a teammate", url: "https://slack.example.com/archives/D1/p1", link_label: "Open in Slack", actions: ["snooze", "dismiss"] },
    { ...base, id: "n-sug", kind: "suggestion", source: "careerbrew", headline: "careerbrew: an agent proposes a task", title: "Plan task t005", url: "#/today", link_label: "See it in Today", accept_label: "Accept", actions: ["accept", "snooze", "dismiss"], priority: 3 },
    { ...base, id: "n-old", kind: "mention", headline: "Slack: someone mentioned you", status: "done", resolved: "closed", actions: [] },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function stub(answer: (url: string, init?: RequestInit) => unknown = () => view) {
  const posts: string[] = [];
  vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") posts.push(String(url));
    return Promise.resolve(new Response(JSON.stringify(answer(String(url), init)), { status: 200 }));
  }));
  return posts;
}

describe("Notifications view (issue #74)", () => {
  it("shows what, why, and next for each, the same actions as a pop-up, and closed ones apart", async () => {
    stub();
    render(<NotificationsView />);
    const dm = (await screen.findByText("DM from a teammate")).closest("li")!;
    expect(within(dm).getByText("Slack: a direct message asks you")).toBeInTheDocument();
    expect(within(dm).getByText("reply in Slack")).toBeInTheDocument();
    expect(within(dm).getByRole("link", { name: "Open in Slack" })).toHaveAttribute("target", "_blank");
    expect(within(dm).queryByRole("button", { name: "Accept" })).toBeNull();
    const sug = screen.getByText("Plan task t005").closest("li")!;
    for (const name of ["Accept", "Dismiss", "Snooze"]) expect(within(sug).getByRole("button", { name })).toBeInTheDocument();
    expect(within(sug).getByRole("link", { name: "See it in Today" })).toHaveAttribute("href", "#/today");
    expect(screen.getByRole("region", { name: "Closed (1)" })).toHaveTextContent("no longer asks you");
    expect(screen.getByText(/at most 6 pop-ups a day \(1 today\)/)).toBeInTheDocument();
  });

  it("marks the notification a pop-up click opened", async () => {
    stub();
    render(<NotificationsView focus="n-sug" />);
    const sug = (await screen.findByText("Plan task t005")).closest("li")!;
    expect(sug).toHaveAttribute("aria-current", "true");
    expect(screen.getByText("DM from a teammate").closest("li")).not.toHaveAttribute("aria-current");
  });

  it("snoozes, accepts, and marks read through the API", async () => {
    const posts = stub((url) => (url.endsWith("/snooze") ? { ...view, items: [{ ...view.items[0], status: "snoozed", snooze_until: "2026-10-21T11:00:00-04:00", actions: ["snooze", "dismiss"] }, view.items[1]] } : view));
    render(<NotificationsView />);
    const dm = (await screen.findByText("DM from a teammate")).closest("li")!;
    fireEvent.click(within(dm).getByRole("button", { name: "Snooze" }));
    expect(await screen.findByText("snoozed until 11:00")).toBeInTheDocument();
    fireEvent.click(within(screen.getByText("Plan task t005").closest("li")!).getByRole("button", { name: "Accept" }));
    await screen.findByText("Plan task t005");
    fireEvent.click(within(screen.getByText("Plan task t005").closest("li")!).getByRole("link", { name: "See it in Today" }));
    await vi.waitFor(() => expect(posts).toEqual(["/api/notifications/n-dm/snooze", "/api/notifications/n-sug/accept", "/api/notifications/n-sug/read"]));
  });

  it("offers to turn notifications on when they are off", async () => {
    const posts = stub((url) => (url.endsWith("/api/config") ? { settings: [] } : { status: { ...status, enabled: false }, items: [] }));
    render(<NotificationsView />);
    fireEvent.click(await screen.findByRole("button", { name: "Turn on" }));
    await vi.waitFor(() => expect(posts).toEqual(["/api/config"]));
    expect(screen.getByText(/Nothing needs you right now/)).toBeInTheDocument();
  });
});
