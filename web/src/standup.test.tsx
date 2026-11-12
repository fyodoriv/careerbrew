import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Standup, StandupAction } from "./api";
import { StandupView } from "./standup";
import { timeDone } from "./timing";

const act = (title: string, extra: Partial<StandupAction> = {}): StandupAction => ({ title, at: "2026-10-16T14:30:00-04:00", when: "Fri Oct 16, 14:30", kinds: ["pr"], nouns: ["PR"], repo: "", url: "", keys: [], via: "", ...extra });
const report: Standup = {
  window: { since: "Fri Oct 16, 10:00", until: "Mon Oct 19, 09:00", tz: "Etc/GMT+4", start: "", end: "", days: [0, 1, 2, 3, 4], time: "10:00", minutes: 15, mode: "previous-day", compares: "Fri Oct 16" },
  rows: [
    { title: "Order export", status: "In review", owner: "me", mine: true, keys: ["PROJ-21"], url: "", talking_point: "Add the export endpoint (1 PR, 1 commit).", actions: [act("PROJ-21 add the export endpoint", { kinds: ["pr", "commit", "feed"], nouns: ["PR", "commit", "event"], repo: "acme/shop", url: "https://github.example.com/acme/shop/pull/12", keys: ["PROJ-21"], via: "ticket PROJ-21" })] },
    { title: "Billing ledger", status: "", owner: "me", mine: true, keys: [], url: "", talking_point: "Closed the ledger. Next: the audit.", agent_written: true, actions: [] },
  ],
  groups: [{ label: "Tooling", never_rows: true, talking_point: "Export skill for agents (1 commit).", actions: [act("feat: export skill for agents", { kinds: ["commit"], nouns: ["commit"], repo: "agentbrew", via: "repo agentbrew" })] }],
  hidden: 1, has_rows: true, saved_at: "2026-10-19T08:55:00-04:00", updated: "updated 08:55", page: "/data/out/standup.html",
  counts: { actions: 2, rows: 3, shown: 2, yours: 2, matched: 0, hidden: 1, on_rows: 1, by_group: [["Tooling", 1]] },
};

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

function stub(body: unknown) {
  vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(body), { status: 200 }))));
}

describe("Standup view (issue #110)", () => {
  it("Copy standup nests each item’s talking point on the clipboard, and times it (US-86)", async () => {
    stub(report);
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<StandupView />);
    fireEvent.click(await screen.findByRole("button", { name: "Copy standup" }));
    expect(writeText).toHaveBeenCalledWith("• Order export\n  – Add the export endpoint (1 PR, 1 commit).\n• Billing ledger\n  – Closed the ledger. Next: the audit.\n• Tooling\n  – Export skill for agents (1 commit).");
    expect(vi.mocked(timeDone)).toHaveBeenCalledWith("standup", "copy");
    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("Copied"));
    Reflect.deleteProperty(navigator, "clipboard");
  });

  it("Copy standup waits for a row or a group", async () => {
    stub({ ...report, rows: [], groups: [], has_rows: false });
    render(<StandupView />);
    expect(await screen.findByRole("button", { name: "Copy standup" })).toBeDisabled();
  });

  it("shows each row's talking point first, then its status, then the evidence", async () => {
    stub(report);
    render(<StandupView />);
    const row = await screen.findByRole("region", { name: "Order export" });
    const text = row.textContent ?? "";
    expect(text.indexOf("Add the export endpoint (1 PR, 1 commit).")).toBeLessThan(text.indexOf("Status:"));
    expect(text.indexOf("Status:")).toBeLessThan(text.indexOf("PROJ-21 add the export endpoint"));
    expect(within(row).getByText("In review", { exact: false })).toBeInTheDocument();
    expect(within(row).getByRole("link", { name: "PROJ-21 add the export endpoint" })).toHaveAttribute("target", "_blank");
    expect(within(row).getByText("PR · commit · event")).toBeInTheDocument();
    expect(within(row).getByText(/via ticket PROJ-21/)).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Billing ledger" })).toHaveTextContent("No actions of yours since the last standup.");
    expect(screen.getByRole("region", { name: "Tooling" })).toHaveTextContent("feat: export skill for agents");
    expect(screen.getByText("1 row with no update hidden.")).toBeInTheDocument();
    expect(screen.getByText(/^Since/)).toHaveTextContent("Since Fri Oct 16, 10:00 (Fri Oct 16 standup) until Mon Oct 19, 09:00");
    expect(screen.getByText(/^Since/)).toHaveTextContent("· 2 actions of yours · updated 08:55.");
  });

  it("issue #229: marks a talking point an agent wrote and shows an action an agent passed in the evidence", async () => {
    const said = "Shipped the export API and walked support through it. Next: the CSV format.";
    const meeting = act("Agreed the ledger schema", { kinds: ["meeting"], nouns: ["meeting"], via: "an agent", extra: true });
    stub({ ...report, rows: [{ ...report.rows[0], talking_point: said, agent_written: true }, { ...report.rows[1], talking_point: "Agreed the ledger schema (1 meeting).", agent_written: false, actions: [meeting] }] });
    render(<StandupView />);
    const mine = await screen.findByRole("region", { name: "Order export" });
    expect(mine.querySelector(".standup-talk")).toHaveTextContent(`${said} written by an agent`);
    const ledger = screen.getByRole("region", { name: "Billing ledger" });
    expect(ledger.querySelector(".standup-by")).toBeNull();
    expect(ledger).toHaveTextContent("Agreed the ledger schema");
    expect(within(ledger).getByText(/via an agent/)).toBeInTheDocument();
    expect(ledger).not.toHaveTextContent("No actions of yours since the last standup.");
  });

  it("issue #271: lists each proof a row passed under it, linked, in place of an empty evidence line", async () => {
    const proof = { source: "github", url: "https://github.example.com/acme/shop/pull/12", checked_at: "2026-10-19T09:30:00", quote: "Merged on Friday", when: "Mon Oct 19, 09:30" };
    stub({ ...report, rows: [{ ...report.rows[1], talking_point: "Checked live in 1 source: see the proof below.", agent_written: false, proofs: [proof] }], counts: { ...report.counts, gated: true, passed: 1, failed: 0 } });
    render(<StandupView />);
    const row = await screen.findByRole("region", { name: "Billing ledger" });
    expect(row.querySelector(".standup-proof")).toHaveTextContent("Checked Mon Oct 19, 09:30 in github: Merged on Friday");
    expect(within(row).getByRole("link", { name: "Merged on Friday" })).toHaveAttribute("href", proof.url);
    expect(row).not.toHaveTextContent("No actions of yours since the last standup.");
  });

  it("issue #271: says no row passed the proof check when every row failed or the rows were cleared", async () => {
    stub({ ...report, rows: [], groups: [], hidden: 0, counts: { ...report.counts, gated: true, passed: 0, failed: 2 } });
    render(<StandupView />);
    expect(await screen.findByRole("status")).toHaveTextContent("No row passed the proof check.");
    cleanup();
    stub({ ...report, rows: [], groups: [], hidden: 0, has_rows: false, cleared: true });
    render(<StandupView />);
    expect(await screen.findByRole("status")).toHaveTextContent("No row passed the proof check.");
    expect(screen.queryByText(/careerbrew work standup --rows/)).toBeNull();
  });

  it("says how a plugin or agent passes the rows when there are none yet", async () => {
    stub({ ...report, rows: [], groups: [], hidden: 0, has_rows: false, counts: { ...report.counts, actions: 0 } });
    render(<StandupView />);
    expect(await screen.findByRole("status")).toHaveTextContent("careerbrew work standup --rows");
    expect(screen.getByText("No work of yours since the last standup yet.")).toBeInTheDocument();
  });
});

describe("Standup speaking layout (#306, #307)", () => {
  it("puts Copy beside the title and context in one subheading, without a window card or file path", async () => {
    stub(report);
    render(<StandupView />);
    const copy = await screen.findByRole("button", { name: "Copy standup" });
    const heading = await screen.findByRole("heading", { name: "Standup", level: 1 });
    expect(heading.closest("header")).toContainElement(copy);
    expect(screen.queryByRole("heading", { name: "Report window" })).not.toBeInTheDocument();
    expect(document.querySelector('[data-block="window"]')).toBeNull();
    expect(screen.queryByText("Also as a file")).not.toBeInTheDocument();
    expect(screen.queryByText(report.page)).not.toBeInTheDocument();
    expect(document.querySelectorAll(".standup-window")).toHaveLength(1);
    expect(document.querySelector('.standup-window')?.closest("section")).toBeNull();
  });

  it("shows three speaking bullets and copies them nested under the row title", async () => {
    const points = ["Shipped the API (2 PRs)", "Next: the CSV format", "Ask: a review from the data team"];
    stub({ ...report, rows: [{ ...report.rows[0], talking_point: points.join(" "), talking_points: points }], groups: [] });
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });
    render(<StandupView />);
    const row = await screen.findByRole("region", { name: "Order export" });
    expect(row.querySelectorAll("ul.standup-talk > li")).toHaveLength(3);
    fireEvent.click(screen.getByRole("button", { name: "Copy standup" }));
    expect(writeText).toHaveBeenCalledWith("• Order export\n  – Shipped the API (2 PRs)\n  – Next: the CSV format\n  – Ask: a review from the data team");
    Reflect.deleteProperty(navigator, "clipboard");
  });

  it("keeps the window readable for a zone accepted by Python but unsupported by the browser", async () => {
    const format = Intl.DateTimeFormat;
    const formatSpy = vi.spyOn(Intl, "DateTimeFormat").mockImplementation((locales, options) => {
      if (options?.timeZone === "Factory") throw new RangeError("unsupported report zone");
      return format(locales, options);
    });
    try {
      stub({ ...report, window: { ...report.window, tz: "Factory" } });
      render(<StandupView />);
      expect(await screen.findByText(/^Since/)).toHaveTextContent("(Factory)");
    } finally {
      formatSpy.mockRestore();
    }
  });

});
