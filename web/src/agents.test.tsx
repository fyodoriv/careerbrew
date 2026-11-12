import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AgentActivity } from "./agents";
import type { AgentRun, Agents } from "./api";

const run = (agent: string, extra: Partial<AgentRun> = {}): AgentRun => ({ agent, run: agent, project: "", status: "running", title: `${agent} works`, url: "", at: "2026-10-21T14:10:00+00:00", started_at: "2026-10-21T14:00:00+00:00", events: 2, ...extra });
const data: Agents = {
  counts: { running: 1, quiet: 1, done: 1, failed: 1 },
  quiet_minutes: 60,
  groups: [
    { project: "billing", running: 1, last_at: "2026-10-21T14:10:00+00:00", runs: [run("review-bot", { project: "billing", title: "Reviewing PR 7", url: "https://github.example.com/org/shop/pull/7" }), run("fix-bot", { project: "billing", status: "failed", title: "Bad link", url: "javascript:alert(1)" })] },
    { project: "", running: 0, last_at: "2026-10-21T13:00:00+00:00", runs: [run("standup-bot", { status: "quiet", events: 1 }), run("docs-bot", { run: "pr-9", status: "done" })] },
  ],
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function stub(body: unknown) {
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(body), { status: 200 }))));
}

describe("Agent activity (issue #272)", () => {
  it("groups runs by project, each with its state, its last event as a link, and the counts", async () => {
    stub(data);
    render(<AgentActivity />);
    const billing = await screen.findByRole("region", { name: "Agents: billing" });
    expect(within(billing).getByRole("link", { name: "Reviewing PR 7" })).toHaveAttribute("href", "https://github.example.com/org/shop/pull/7");
    expect(within(billing).getByText("Bad link").closest("a"), "a javascript: link is never a link").toBeNull();
    expect(within(billing).getByText("failed")).toHaveClass("tag-no");
    const none = screen.getByRole("region", { name: "Agents: no project" });
    expect(within(none).getByText(/no event for over 60 min/)).toBeInTheDocument();
    expect(within(none).getByText("· pr-9")).toBeInTheDocument();
    expect(screen.getByText("1 running")).toBeInTheDocument();
  });

  it("with no agent yet, says the one command an agent runs per step", async () => {
    stub({ counts: { running: 0, quiet: 0, done: 0, failed: 0 }, quiet_minutes: 60, groups: [] });
    render(<AgentActivity />);
    expect(await screen.findByText(/never in a batch at the end/)).toBeInTheDocument();
    expect(screen.getByText(/careerbrew work agent event/)).toBeInTheDocument();
  });
});
