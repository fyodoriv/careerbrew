import { cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CHANGED_EVENT } from "./live";
import { keepDates } from "./text";
import { ActiveNow, DashboardView, DataTable, InterviewsView, JobsView, LibraryView, ReviewView, SetupView, TodayView, WeekView, WorkView } from "./views";

// The day log Today and Dashboard's Copy update bar (#145) load beside the view's own data.
const EMPTY_LOG = { log: { day: "2026-10-19", tomorrow: "2026-10-20", done: {}, planned: [], blockers: [], questions: [], advice: [] }, markdown: "", pm: { subject: "Daily update", body: "", to: "" }, inbox: [], history: [] };

function mockApi(body: unknown, status = 200) {
  // A fresh body per call. The Today view also loads the daily log and the open asks; give them empty ones unless the test is about errors.
  const side = (url: string) => (url.includes("/api/daylog") ? EMPTY_LOG : url.includes("/api/inbox") ? { inbox: [] } : url.includes("/api/feed/asks") ? NO_ASKS : null);
  vi.stubGlobal("fetch", vi.fn((url: string) => Promise.resolve(new Response(JSON.stringify((status === 200 && side(String(url))) || body), { status }))));
}

const NO_ASKS = { asks: [], total: 0, events: 0, overdue_hours: 8 };
const TODAY = {
  plan: { day: "2026-10-19", meetings: [], free_minutes: 420, focus_minutes: 336, calibration: 1, schedule: [], unplaced: [], calendar_errors: [] },
  next: { kind: "task", text: "Add a task." }, results: [], summary: {},
};

afterEach(() => { vi.unstubAllGlobals(); vi.useRealTimers(); });

describe("views", () => {
  it("Library points at the published Notion skill", async () => {
    mockApi({ pages: [] });
    render(<LibraryView />);
    expect(await screen.findByText(/careerbrew-notion-import/)).toBeInTheDocument();
  });

  it("Today shows the next step, the plan, and results", async () => {
    mockApi({
      plan: { day: "2026-10-19", meetings: [{ start: "2026-10-19T09:30", end: "2026-10-19T09:45", title: "Team standup" }], free_minutes: 420, focus_minutes: 336, calibration: 1.5, schedule: [{ task: "t001", title: "write design doc", start: "2026-10-19T10:00", end: "2026-10-19T11:30", carry_over: false, reason: "impact 3", project: "checkout" }], unplaced: ["t002"], unplaced_titles: ["migrate billing"], calendar_errors: [] },
      next: { kind: "task", task: "t001", title: "write design doc", text: "Work on t001 now (120 min free).", plain: "Work on it now (120 min free).", reason: "closes promotion gap design" },
      results: [{ id: "a", at: "x", kind: "note", title: "shipped search", source: "evidence" }],
      summary: { note: 1 },
    });
    render(<TodayView />);
    expect(await screen.findByText(/Work on it now/)).toBeInTheDocument();
    // Issue #59: titles and projects, not raw ids; the id stays in a closed details view.
    expect(screen.getByText("checkout")).toBeInTheDocument();
    expect(screen.getByText(/Not today: migrate billing/)).toBeInTheDocument();
    expect(screen.queryByText(/t00\d/)?.closest("details")).toBeTruthy();
    expect(screen.queryByText(/Work on t001/)).toBeNull();
    expect(screen.getByText("Team standup")).toBeInTheDocument();
    expect(screen.getByText(/closes promotion gap design/)).toBeInTheDocument();
    expect(screen.getByText("shipped search")).toBeInTheDocument();
  });

  it("Today lists open asks with their source link, and Mark personal runs `work feed mark`", async () => {
    const ask = { id: "slack:D0DANA:1.2", source: "slack", kind: "dm", at: "2026-10-19T12:15:00Z", title: "DM from dana", url: "https://example.slack.com/archives/D0DANA/p1", waiting_minutes: 45, work_minutes: 0, overdue: false };
    const old = { ...ask, id: "outlook:m1", source: "outlook", title: "Mail from Finance: budget", url: "", waiting_minutes: 5310, work_minutes: 570, overdue: true };
    let asks = [old, ask];
    const posts: { url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      const u = String(url);
      if (init?.method === "POST") {
        posts.push({ url: u, body: JSON.parse(String(init.body)) });
        asks = [old];
        return Promise.resolve(new Response(JSON.stringify({ action: "work.feed.mark", argv: [], exit: 0, output: "marked" })));
      }
      const body = u.includes("/api/feed/asks") ? { asks, total: asks.length, events: 6, overdue_hours: 8 } : u.includes("/api/inbox") ? { inbox: [] } : u.includes("/api/daylog") ? { log: { day: "2026-10-19", tomorrow: "2026-10-20", done: {}, planned: [], blockers: [], questions: [], advice: [] }, markdown: "", pm: { subject: "s", body: "", to: "" }, inbox: [], history: [] } : TODAY;
      return Promise.resolve(new Response(JSON.stringify(body)));
    }));
    render(<TodayView />);
    const region = await screen.findByRole("region", { name: "Open asks (2)" });
    expect(screen.getByRole("link", { name: "DM from dana" })).toHaveAttribute("href", "https://example.slack.com/archives/D0DANA/p1");
    expect(region).toHaveTextContent("Mail from Finance: budget");
    expect(region).toHaveTextContent("over 8 work hours");
    expect(region).toHaveTextContent("waiting 9h 30m in work hours");
    expect(region).not.toHaveTextContent("88h"); // the wall-clock wait (nights, the weekend) is not shown as the wait
    screen.getByRole("button", { name: "Mark personal: DM from dana" }).click();
    expect(await screen.findByRole("region", { name: "Open asks (1)" })).toBeInTheDocument();
    expect(posts).toEqual([{ url: "/api/actions/work.feed.mark", body: { values: { id: "slack:D0DANA:1.2", value: "personal" }, confirm: false } }]);
    expect(screen.getByRole("status")).toHaveTextContent("Marked personal: DM from dana");
    expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
  });

  it("past the answer's limit, Show names how many it loads, and the hint's terminal command lists the oldest asks (issue #275)", async () => {
    cleanup();
    const ask = (i: number) => ({ id: `acme:${i}`, source: "acme", kind: "dm", at: "2026-10-19T12:15:00Z", title: `Ask ${i}`, url: "", waiting_minutes: 0, work_minutes: 0, overdue: false });
    const asked: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      const u = String(url);
      if (u.includes("/api/feed/asks")) {
        asked.push(u);
        const n = Number(new URL(u, "http://x").searchParams.get("limit") ?? 50);
        return Promise.resolve(new Response(JSON.stringify({ asks: Array.from({ length: n }, (_, i) => ask(1500 - i)), total: 1500, limit: 1000, events: 1500, overdue_hours: 8 })));
      }
      const body = u.includes("/api/inbox") ? { inbox: [] } : u.includes("/api/daylog") ? EMPTY_LOG : TODAY;
      return Promise.resolve(new Response(JSON.stringify(body)));
    }));
    render(<TodayView />);
    const region = await screen.findByRole("region", { name: "Open asks (1500)" });
    expect(region).toHaveTextContent("Showing 50 of 1500 open asks, newest first.");
    const show = within(region).getByRole("button", { name: "Show 1000" }); // never "Show all 1500": one answer holds 1000
    await waitFor(() => expect(show).toBeEnabled());
    fireEvent.click(show);
    await waitFor(() => expect(region).toHaveTextContent("Showing 1000 of 1500 open asks, newest first."));
    expect(asked).toContain("/api/feed/asks?limit=1000");
    expect(region).toHaveTextContent("The 500 oldest are only in your terminal: careerbrew work feed show --show --open --top 1500");
    expect(within(region).queryByRole("button", { name: /^Show/ })).toBeNull();
  });

  it("Not an ask runs `work feed mark <id> not-ask`, and Undo opens it again (issue #185)", async () => {
    cleanup();
    const ask = { id: "slack:C1:7", source: "slack", kind: "mention", at: "2026-10-19T12:15:00Z", title: "release-bot mentions you", url: "", waiting_minutes: 45, work_minutes: 0, overdue: false };
    let asks = [ask];
    const posts: unknown[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      const u = String(url);
      if (init?.method === "POST") {
        const body = JSON.parse(String(init.body));
        posts.push(body.values);
        asks = body.values.value === "ask" ? [ask] : [];
        return Promise.resolve(new Response(JSON.stringify({ action: "work.feed.mark", argv: [], exit: 0, output: "marked" })));
      }
      const body = u.includes("/api/feed/asks") ? { asks, total: asks.length, events: 6, overdue_hours: 8 } : u.includes("/api/inbox") ? { inbox: [] } : u.includes("/api/daylog") ? { log: { day: "2026-10-19", tomorrow: "2026-10-20", done: {}, planned: [], blockers: [], questions: [], advice: [] }, markdown: "", pm: { subject: "s", body: "", to: "" }, inbox: [], history: [] } : TODAY;
      return Promise.resolve(new Response(JSON.stringify(body)));
    }));
    render(<TodayView />);
    (await screen.findByRole("button", { name: "Not an ask: release-bot mentions you" })).click();
    expect(await screen.findByRole("region", { name: "Open asks (0)" })).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Marked not an ask: release-bot mentions you");
    screen.getByRole("button", { name: "Undo" }).click();
    expect(await screen.findByRole("region", { name: "Open asks (1)" })).toBeInTheDocument();
    expect(posts).toEqual([{ id: "slack:C1:7", value: "not-ask" }, { id: "slack:C1:7", value: "ask" }]);
    cleanup();
  });

  it("Active now lists the ranked projects with their counts; Up, Remove, and Put back run work priority set and remove (issue #198)", async () => {
    let rows = [
      { name: "billing", rank: 1, match: ["BILL-*"], open_tasks: 2, today: 1 },
      { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 },
    ];
    const posts: { url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        const body = JSON.parse(String(init.body)) as { values: { project: string; rank?: number } };
        posts.push({ url: String(url), body });
        const p = rows.find((r) => r.name === body.values.project) ?? { name: body.values.project, rank: 1, match: ["BILL-*"], open_tasks: 2, today: 1 };
        const rest = rows.filter((r) => r.name !== p.name);
        rows = (String(url).endsWith("remove") ? rest : [...rest.slice(0, (body.values.rank ?? 1) - 1), p, ...rest.slice((body.values.rank ?? 1) - 1)]).map((r, i) => ({ ...r, rank: i + 1 }));
        return Promise.resolve(new Response(JSON.stringify({ action: "x", argv: [], exit: 0, output: "saved" })));
      }
      return Promise.resolve(new Response(JSON.stringify({ projects: rows })));
    }));
    const changed = vi.fn();
    render(<ActiveNow onChange={changed} />);
    const region = await screen.findByRole("region", { name: "Active now" });
    expect(region).toHaveTextContent("#1 billing2 open tasks · 1 item today");
    expect(screen.getByRole("button", { name: "Move up: billing" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Move down: ledger" })).toBeDisabled();
    fireEvent.click(screen.getByRole("button", { name: "Move up: ledger" }));
    await waitFor(() => expect(region).toHaveTextContent("#1 ledger"));
    await waitFor(() => expect(screen.getByRole("button", { name: "Move up: billing" })).toBeEnabled());
    expect(changed).toHaveBeenCalled();
    fireEvent.click(screen.getByRole("button", { name: "Remove from ranking: billing" }));
    fireEvent.click(await screen.findByRole("button", { name: "Put back: billing" }));
    await waitFor(() => expect(screen.queryByRole("button", { name: "Put back: billing" })).toBeNull());
    expect(posts).toEqual([
      { url: "/api/actions/work.priority.set", body: { values: { project: "ledger", rank: 1 }, confirm: false } },
      { url: "/api/actions/work.priority.remove", body: { values: { project: "billing" }, confirm: false } },
      { url: "/api/actions/work.priority.set", body: { values: { project: "billing", rank: 2, match: ["BILL-*"] }, confirm: false } },
    ]);
    await waitFor(() => expect(within(region).getAllByRole("listitem").map((li) => li.getAttribute("data-rank"))).toEqual(["1", "2"]));
  });

  it("Active now drops Put back when the ranking changed elsewhere after a Remove (review of #204)", async () => {
    let rows = [
      { name: "billing", rank: 1, match: ["BILL-*"], open_tasks: 2, today: 1 },
      { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 },
    ];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        rows = rows.filter((r) => r.name !== "billing").map((r, i) => ({ ...r, rank: i + 1 }));
        return Promise.resolve(new Response(JSON.stringify({ action: "x", argv: [], exit: 0, output: "saved" })));
      }
      return Promise.resolve(new Response(JSON.stringify({ projects: rows })));
    }));
    render(<ActiveNow />);
    const region = await screen.findByRole("region", { name: "Active now" });
    await within(region).findByText("billing");
    fireEvent.click(within(region).getByRole("button", { name: "Remove from ranking: billing" }));
    expect(await within(region).findByRole("button", { name: "Put back: billing" })).toBeInTheDocument();
    // The fresh answer after the Remove keeps the offer.
    await waitFor(() => expect(within(region).getAllByRole("listitem")).toHaveLength(1));
    expect(within(region).getByRole("button", { name: "Put back: billing" })).toBeInTheDocument();
    // `careerbrew work priority set other --rank 1` in a terminal, then the server reports new data.
    rows = [{ name: "other", rank: 1, match: ["o-*"], open_tasks: 0, today: 0 }, { ...rows[0], rank: 2 }];
    window.dispatchEvent(new Event(CHANGED_EVENT));
    await waitFor(() => expect(within(region).queryByRole("button", { name: "Put back: billing" })).toBeNull());
  });

  it("Active now keeps Put back when a stale answer from before the Remove lands after it (review of #204)", async () => {
    const before = [
      { name: "billing", rank: 1, match: ["BILL-*"], open_tasks: 2, today: 1 },
      { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 },
    ];
    let removed = false;
    let stale = 0; // GETs that still answer with the ranking from before the Remove
    vi.stubGlobal("fetch", vi.fn((_url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        removed = true;
        stale = 1;
        return Promise.resolve(new Response(JSON.stringify({ action: "x", argv: [], exit: 0, output: "saved" })));
      }
      const rows = removed && stale-- <= 0 ? [{ ...before[1], rank: 1 }] : before;
      return Promise.resolve(new Response(JSON.stringify({ projects: rows })));
    }));
    render(<ActiveNow />);
    const region = await screen.findByRole("region", { name: "Active now" });
    await within(region).findByText("billing");
    fireEvent.click(within(region).getByRole("button", { name: "Remove from ranking: billing" }));
    expect(await within(region).findByRole("button", { name: "Put back: billing" })).toBeInTheDocument();
    await new Promise((r) => setTimeout(r, 50));
    expect(within(region).getByRole("button", { name: "Put back: billing" })).toBeInTheDocument();
    window.dispatchEvent(new Event(CHANGED_EVENT)); // the answer with the ranking the Remove left
    await waitFor(() => expect(within(region).getAllByRole("listitem")).toHaveLength(1));
    expect(within(region).getByRole("button", { name: "Put back: billing" })).toBeInTheDocument();
  });

  it("Active now drops Put back when the first answer after a Remove already shows a change made elsewhere (review of #204)", async () => {
    let rows = [
      { name: "billing", rank: 1, match: ["BILL-*"], open_tasks: 2, today: 1 },
      { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 },
    ];
    vi.stubGlobal("fetch", vi.fn((_url: string, init?: RequestInit) => {
      if (init?.method === "POST") {
        // The Remove, and `work priority set other --rank 1` in a terminal before the reload answers.
        rows = [{ name: "other", rank: 1, match: ["o-*"], open_tasks: 0, today: 0 }, { ...rows[1], rank: 2 }];
        return Promise.resolve(new Response(JSON.stringify({ action: "x", argv: [], exit: 0, output: "saved" })));
      }
      return Promise.resolve(new Response(JSON.stringify({ projects: rows })));
    }));
    render(<ActiveNow />);
    const region = await screen.findByRole("region", { name: "Active now" });
    await within(region).findByText("billing");
    fireEvent.click(within(region).getByRole("button", { name: "Remove from ranking: billing" }));
    await within(region).findByText("other");
    await waitFor(() => expect(within(region).queryByRole("button", { name: "Put back: billing" })).toBeNull());
  });

  it("Today says how to start the work feed when it is empty", async () => {
    mockApi(TODAY);
    render(<TodayView />);
    expect(await screen.findByRole("region", { name: "Open asks (0)" })).toHaveTextContent("careerbrew connect slack");
  });

  it("Today says every ask is answered when the feed has events but no open asks", async () => {
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      const u = String(url);
      const body = u.includes("/api/feed/asks") ? { ...NO_ASKS, events: 6 } : u.includes("/api/inbox") ? { inbox: [] } : u.includes("/api/daylog") ? { log: { day: "2026-10-19", tomorrow: "2026-10-20", done: {}, planned: [], blockers: [], questions: [], advice: [] }, markdown: "", pm: { subject: "s", body: "", to: "" }, inbox: [], history: [] } : TODAY;
      return Promise.resolve(new Response(JSON.stringify(body)));
    }));
    render(<TodayView />);
    expect(await screen.findByText("No open asks: you answered everything.")).toBeInTheDocument(); // this file keeps earlier renders mounted
  });

  // Issue #142: each Dashboard block asks for its own data (`/api/focus/<id>`).
  function mockBlocks(blocks: Record<string, unknown>, fail: string[] = []) {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string) => {
      const id = String(url).split("/api/focus/")[1] ?? "";
      if (id) calls.push(id);
      if (String(url).includes("/api/daylog")) return Promise.resolve(new Response(JSON.stringify(EMPTY_LOG), { status: 200 }));
      if (fail.includes(id)) return Promise.resolve(new Response(JSON.stringify({ error: "synthetic failure" }), { status: 500 }));
      return Promise.resolve(new Response(JSON.stringify({ id, at: "2026-10-19T10:00", ...(blocks[id] as object ?? {}) }), { status: 200 }));
    }));
    return calls;
  }
  const IDS = ["now", "today", "week", "month", "quarter", "year", "company"];

  it("Dashboard shows every horizon with a why, or its empty state", async () => {
    const calls = mockBlocks(Object.fromEntries(IDS.map((id) => [id, {
      horizon: id === "month" ? { id, label: "This month", items: [], empty: "No task is due within 31 days." } : { id, label: `H ${id}`, items: [{ title: `do ${id}`, why: `because ${id}` }], empty: "" },
      ...(id === "company" ? { ladder: "generic-swe", company_ladder: false } : {}),
    }])));
    render(<DashboardView />);
    expect(await screen.findByText("because quarter")).toBeInTheDocument();
    for (const id of IDS.filter((i) => i !== "month")) expect(await screen.findByText(`do ${id}`)).toBeInTheDocument();
    expect(screen.getByText("No task is due within 31 days.")).toBeInTheDocument();
    expect(screen.getByText(/\(generic\)/)).toBeInTheDocument();
    expect(new Set(calls)).toEqual(new Set([...IDS, "stale", "tomorrow"]));  // one request per block, all at once
  });

  it("Dashboard, Today's next step, and the plan show a task's links and its own why (issue #143)", async () => {
    const ctx = { links: [{ label: "ticket", url: "https://jira.example.com/browse/SHOP-12" }, { label: "javascript:alert(1)", url: "javascript:alert(1)" }], task_why: "asked in https://slack.example.com/archives/C0CHECKOUT" };
    mockBlocks({
      today: { horizon: { id: "today", label: "Today", items: [{ title: "Tell the owner", why: "score 9.9 = ...", ref: "t001", ...ctx }], empty: "" } },
      tomorrow: { tomorrow: { day: "2026-10-20", items: [{ title: "Tell the owner", start: "09:00", end: "09:30", ref: "t001", ...ctx }], empty: "" } },
    });
    const dash = render(<DashboardView />);
    const horizon = await within(dash.container).findByRole("region", { name: "Today" });
    expect(await within(horizon).findByText("score 9.9 = ...")).toBeInTheDocument();  // each block loads on its own (issue #142)
    expect(within(horizon).getByText(/Context:/)).toBeInTheDocument();
    expect(within(horizon).getByRole("link", { name: "ticket" })).toHaveAttribute("rel", "noreferrer noopener");
    expect(within(horizon).getByRole("link", { name: "https://slack.example.com/archives/C0CHECKOUT" })).toHaveAttribute("target", "_blank");
    expect(within(horizon).getAllByRole("link")).toHaveLength(2);  // the javascript: value stays text
    expect(await within(within(dash.container).getByRole("region", { name: "Tomorrow" })).findByRole("link", { name: "ticket" })).toHaveAttribute("target", "_blank");
    dash.unmount();

    mockApi({ ...TODAY,
      plan: { ...TODAY.plan, schedule: [{ task: "t001", title: "Tell the owner", start: "2026-10-19T10:00", end: "2026-10-19T10:30", carry_over: false, reason: "impact 5", ...ctx }] },
      next: { kind: "task", task: "t001", title: "Tell the owner", text: "Work on t001 now.", plain: "Work on it now.", reason: "impact 5", ...ctx },
    });
    const today = render(<TodayView />);
    const next = await within(today.container).findByRole("region", { name: "Next step" });
    expect(within(next).getByRole("link", { name: "ticket" })).toHaveAttribute("href", "https://jira.example.com/browse/SHOP-12");
    const plan = within(today.container).getByRole("region", { name: "Plan for Mon Oct 19" });
    expect(within(plan).getByRole("link", { name: "ticket" })).toHaveAttribute("target", "_blank");
    expect(within(plan).getByText(/Context:/)).toBeInTheDocument();
    today.unmount();
  });

  it("Dashboard shows Getting started for a new user, each step with its link and command, and Hide checklist hides it (issue #147)", async () => {
    const steps = [
      { id: "connect", title: "Connect a work tool", why: "careerbrew reads your work from it", view: "#/setup", command: "careerbrew connect", done: true },
      { id: "watch", title: "Turn on the day watcher", why: "it keeps Today fresh", view: "#/setup", command: "careerbrew day watch on", done: false },
    ];
    const posts: { url: string; body: unknown }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      const u = String(url);
      if (init?.method === "POST") posts.push({ url: u, body: JSON.parse(String(init.body)) });
      const body = u.endsWith("/api/checklist") ? { steps, done: 1, total: 2, hidden: false, show: true } : u.includes("/api/config") ? { settings: [] } : u.includes("/api/daylog") ? EMPTY_LOG : { id: "", at: "2026-10-19T10:00" };
      return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }));
    }));
    const { container } = render(<DashboardView />);
    const first = await within(container).findByRole("region", { name: "Getting started (1 of 2 done)" });
    expect(first).toHaveAttribute("data-block", "first-run");
    expect(within(first).getByRole("link", { name: "Turn on the day watcher" })).toHaveAttribute("href", "#/setup");
    expect(within(first).getByText("careerbrew day watch on")).toBeInTheDocument();
    expect(within(first).queryByText("careerbrew connect")).toBeNull();  // a done step needs no command
    fireEvent.click(within(first).getByRole("button", { name: "Hide checklist" }));
    await waitFor(() => expect(within(container).queryByRole("region", { name: /^Getting started/ })).toBeNull());
    expect(posts).toEqual([{ url: "/api/config", body: { key: "first_run_checklist", value: "false" } }]);
    cleanup();
  });

  it("Dashboard shows Commitments only while a promise is open, overdue marked, with its link and next step (issue #268)", async () => {
    const row = { name: "promise-0000000a", title: "Send the rollout notes", to: "Kim", due: "2026-10-18", overdue: true, made_at: "2026-10-17T08:00:00Z", url: "https://example.com/t/1", parent: "", next: "draft", steps: {}, closed: "" };
    const posts: string[] = [];
    let open = [row];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      const u = String(url);
      if (init?.method === "POST") {
        posts.push(u);
        open = [];
      }
      const body = u.includes("/api/promises") ? { steps: { draft: "draft the follow-up" }, promises: open } : u.includes("/api/daylog") ? EMPTY_LOG : { id: "", at: "2026-10-19T10:00" };
      return Promise.resolve(new Response(JSON.stringify(body), { status: 200 }));
    }));
    const { container } = render(<DashboardView />);
    const block = await within(container).findByRole("region", { name: "Commitments (1)" });
    expect(block).toHaveAttribute("data-block", "commitments");
    expect(within(block).getByRole("link", { name: "Send the rollout notes" })).toHaveAttribute("href", "https://example.com/t/1");
    expect(within(block).getByText("overdue")).toBeInTheDocument();
    expect(within(block).getByText(/to Kim · due .* · next: draft the follow-up/)).toBeInTheDocument();
    fireEvent.click(within(block).getByRole("button", { name: "Done: Send the rollout notes" }));
    await waitFor(() => expect(within(container).queryByRole("region", { name: /^Commitments/ })).toBeNull());
    expect(posts).toEqual(["/api/promises/promise-0000000a/done"]);
    cleanup();
  });

  it("Dashboard lists stale project sources first", async () => {
    mockBlocks({ stale: { stale: [{ project: "billing", kind: "tracker", ref: "PROJ-1", age_days: 8, limit: 7 }] } });
    render(<DashboardView />);
    expect(await screen.findByRole("region", { name: "Stale project sources" })).toHaveTextContent("PROJ-1");
    expect(screen.getByText(/careerbrew work project check billing PROJ-1/)).toBeInTheDocument();
  });

  it("Dashboard: each block says what it loads, and a failed block shows its error and Retry while the others show", async () => {
    let release = () => {};
    const gate = new Promise<void>((r) => (release = r));
    vi.stubGlobal("fetch", vi.fn(async (url: string) => {
      const id = String(url).split("/api/focus/")[1] ?? "";
      await gate;
      if (String(url).includes("/api/daylog")) return new Response(JSON.stringify(EMPTY_LOG), { status: 200 });
      if (id === "week") return new Response(JSON.stringify({ error: "synthetic failure" }), { status: 500 });
      return new Response(JSON.stringify({ id, at: "2026-10-19T10:00", horizon: { id, label: id, items: [{ title: `do ${id}`, why: "w" }], empty: "" } }), { status: 200 });
    }));
    const { container } = render(<DashboardView />);
    const page = within(container);
    expect(page.getByText("Loading right now…")).toBeVisible();
    expect(page.getByText("Loading this week…")).toBeVisible();
    release();
    expect(await page.findByText("do quarter")).toBeInTheDocument();
    const week = page.getByRole("region", { name: "This week" });
    expect(await within(week).findByRole("alert")).toHaveTextContent("Could not load this week: synthetic failure. Fix:");
    expect(within(week).getByRole("button", { name: "Retry" })).toBeInTheDocument();
    expect(page.getByText("do now")).toBeInTheDocument();  // the other blocks are on the page
    cleanup();  // the next test counts every alert on the page
  });

  it("Setup lists the steps for a connector that is not ready", async () => {
    vi.stubGlobal("fetch", vi.fn((url: string) => Promise.resolve(new Response(JSON.stringify(url.includes("/api/setup")
      ? { role: "corporate", plugins: [], policy: {}, connectors: [{ kind: "jira", origin: "plugin:acme", secret: "keychain", secret_ok: false, missing: ["email"], ready: false, steps: ["Create a token at https://id.atlassian.com/x", "Run in your terminal: careerbrew connect jira --base-url <base_url> --email <email>"] }] }
      : { role: "corporate", plugins: [], watcher: { state: "off", running: false, reloads: 0, today: {} } }), { status: 200 }))));
    render(<SetupView />);
    expect(await screen.findByText("needs setup")).toBeInTheDocument();
    expect(screen.getByText(/careerbrew connect jira --base-url/)).toBeInTheDocument();
  });

  it("Review shows statuses with direction-aware thresholds", async () => {
    mockApi({ metrics: [{ metric: "estimate_ratio", value: 3, status: "PIVOT", success: 1.3, pivot: 2, question: "q", action: "Split tasks", higher_is_better: false }] });
    render(<ReviewView />);
    expect(await screen.findByText("PIVOT")).toBeInTheDocument();
    expect(screen.getByText(/keep ≤ 1.3 · pivot > 2/)).toBeInTheDocument();
  });

  it("Review lists the metrics that need action first", async () => {
    const m = (metric: string, status: string) => ({ metric, value: 1, status, success: 1, pivot: 0, question: "q", action: "", higher_is_better: true });
    mockApi({ metrics: [m("kept", "KEEP"), m("empty", "no-data"), m("broken", "PIVOT"), m("slipping", "WATCH")] });
    const { container } = render(<ReviewView />);
    await screen.findByText("broken");
    const order = [...container.querySelectorAll("tbody tr code")].map((c) => c.textContent);
    expect(order).toEqual(["broken", "slipping", "kept", "empty"]);
    expect(screen.getByText(/of 4 metrics in KEEP/)).toBeInTheDocument();
  });

  it("DataTable names each cell's column, so a row stacked on a narrow screen still says what each value is (issue #67)", () => {
    const table = (companies: string[]) => (
      <DataTable>
        <thead><tr><th>Company</th><th className="num">Want</th><th></th></tr></thead>
        <tbody>{companies.map((c) => <tr key={c}><td>{c}</td><td className="num">5</td><td><button type="button">Choose</button></td></tr>)}</tbody>
      </DataTable>
    );
    const { container, rerender } = render(table(["Northwind"]));
    expect(screen.getByText("Northwind")).toHaveAttribute("data-label", "Company");
    rerender(table(["Northwind", "Contoso"]));  // rows added later get their labels too
    expect(screen.getByText("Contoso")).toHaveAttribute("data-label", "Company");
    expect([...container.querySelectorAll("tbody td")].map((td) => td.getAttribute("data-label"))).toEqual(["Company", "Want", "", "Company", "Want", ""]);
    expect(container.querySelector(".table-wrap > table")).not.toBeNull();
  });

  it("Work lists projects as a tree with group, title, parent, and links (issue #69)", async () => {
    const row = (name: string, depth: number, extra: object) => ({ name, title: "", group: "", parent: "", links: [], children: 0, depth, sources: 1, stale: 0, ...extra });
    mockApi({ promo: { competencies: [] }, gaps: [], tasks: [], evidence: [], projects: [
      row("payments", 0, { title: "Payments platform", group: "q4-cutline", children: 1 }),
      row("billing", 1, { title: "Billing revamp", group: "q4-cutline", parent: "payments", links: ["ledger"], stale: 2, sources: 2 }),
    ] });
    render(<WorkView />);
    const section = await screen.findByRole("region", { name: "Projects" });
    const items = [...section.querySelectorAll("li")];
    expect(items.map((li) => li.getAttribute("data-depth"))).toEqual(["0", "1"]);
    expect(items[1]).toHaveTextContent("billing · Billing revamp");
    expect(items[1]).toHaveTextContent("part of payments · linked to ledger");
    expect(items[1]).toHaveTextContent("2 sources · 2 stale");
    expect(screen.getByRole("link", { name: "Import a sheet" })).toHaveAttribute("href", "#/actions/work.project.import");
  });

  it("Work shows one section per watched project: stale sources first, last check, and the latest change (issue #172)", async () => {
    cleanup();  // the tree test's Work view is still on the page
    vi.setSystemTime(new Date("2026-10-23T12:00:00Z"));
    const src = (ref: string, extra: object) => ({ kind: "doc", ref, checked_at: null, stale: false, reference: false, reader: "careerbrew", ...extra });
    const watched = [
      { name: "billing", title: "Billing revamp", group: "q4", parent: "", archived: false, stale_after_days: 7, stale: 1, last_checked_at: "2026-10-21T09:00:00+00:00", latest_update: { at: "2026-10-22T09:00:00+00:00", kind: "team", text: "new tech lead" },
        sources: [src("doc-1", { checked_at: "2026-10-21T09:00:00+00:00", status: "spec signed off" }), src("#billing", { kind: "chat", stale: true }), ...["a", "b", "c", "d"].map((r) => src(r, {}))] },
      { name: "ledger", group: "", parent: "", archived: false, stale_after_days: 7, stale: 0, last_checked_at: null, latest_update: null, sources: [] },
    ];
    mockApi({ promo: { competencies: [] }, gaps: [], tasks: [], evidence: [], projects: [], watched });
    render(<WorkView />);
    const billing = await screen.findByRole("region", { name: "billing" });  // the view's cache may show an earlier answer first
    const block = screen.getByRole("region", { name: "Watch list" });
    expect(within(billing).getByRole("heading", { level: 3 })).toHaveTextContent("billing · Billing revamp");
    expect(billing).toHaveTextContent("6 sources · 1 stale · last checked Wed Oct 21");
    expect(billing).toHaveTextContent("Latest change (team, Thu Oct 22): new tech lead");
    expect(billing.querySelector("li")).toHaveTextContent("#billing");  // the stale source comes first
    expect(billing).toHaveTextContent("spec signed off");
    expect(within(billing).getByText("Show 1 more source")).toBeInTheDocument();
    const ledger = within(block).getByRole("region", { name: "ledger" });
    expect(ledger).toHaveTextContent("last checked never");
    expect(ledger).toHaveTextContent("No change noted yet.");
    expect(ledger).toHaveTextContent("No sources yet.");
    expect(block).toHaveTextContent("Note a change: careerbrew work project update <name>");  // once per block, not per project
    cleanup();  // the next tests look for one Work view
  });

  it("Work shows a prep project's readiness gates, the open ones with the command that closes them (issue #276)", async () => {
    cleanup();
    vi.setSystemTime(new Date("2026-11-02T12:00:00Z"));
    const gate = (g: string, done: boolean, note = "") => ({ gate: g, text: `${g} text`, done, at: done ? "2026-11-02T09:00:00+00:00" : null, note, fix: done ? "" : `careerbrew work project gate demo ${g}` });
    const prep = { event_on: "2026-11-03", done: 2, total: 4, ready: false, quizzes: 1, quiz: { at: "2026-11-02T09:00:00+00:00", asked: 4, missed: 1, score: 0.75 },
      gates: [gate("tool", true), gate("links", true, "2 of 2 read"), gate("questions", false), gate("quiz", false, "last: 3 of 4, 1 miss(es)")] };
    const watched = [{ name: "demo", group: "", parent: "", archived: false, stale_after_days: 7, stale: 0, last_checked_at: null, latest_update: null, sources: [], prep }];
    mockApi({ promo: { competencies: [] }, gaps: [], tasks: [], evidence: [], projects: [], watched });
    render(<WorkView />);
    const demo = await screen.findByRole("region", { name: "demo" });
    expect(demo).toHaveTextContent("Not ready: 2 of 4 gates pass · event Tue Nov 3");
    const rows = within(within(demo).getByRole("list", { name: "Readiness gates" })).getAllByRole("listitem");
    expect(rows.map((r) => r.textContent?.includes("open"))).toEqual([false, false, true, true]);
    expect(rows[3]).toHaveTextContent("last: 3 of 4, 1 miss(es)");
    expect(rows[2]).toHaveTextContent("careerbrew work project gate demo questions");
    expect(rows[0]).not.toHaveTextContent("careerbrew work project");  // a passed gate needs no command
    cleanup();
  });

  it("Work names the agent's list command when no project is watched (issue #172)", async () => {
    mockApi({ promo: { competencies: [] }, gaps: [], tasks: [], evidence: [], projects: [], watched: [] });
    render(<WorkView />);
    expect(await screen.findByText(/careerbrew work project list --active --json/)).toBeInTheDocument();
    cleanup();
  });

  it("Work says how to add or import projects when there are none", async () => {
    mockApi({ promo: { competencies: [] }, gaps: [], tasks: [], evidence: [], projects: [] });
    render(<WorkView />);
    expect(await screen.findByText(/careerbrew work project import/)).toBeInTheDocument();
  });

  it("Job hunt shows your rating beside the proposed Want, where a rating sets the Want (US-75 visual review)", async () => {
    // Rated 5 of 5: the strategy's Want is the rating (100); the postings table showed only the model's 66.7, so one
    // job read as Want 100 and Want 66.7 on one page. The CLI's `jobs list` prints `rated=5` beside it.
    const posting = { id: "gh:Northwind:101", company: "Northwind", title: "Staff Software Engineer, Platform", location: "Remote", region: "us", want: 66.7, rating: 5, url: "", source: "Greenhouse", has_loop: false };
    const other = { ...posting, id: "lever:Contoso:c-1", company: "Contoso", title: "Senior Backend Engineer", want: 62.4, rating: null };
    const row = (p: typeof posting, want: number) => ({ company: p.company, title: p.title, want, p_offer: 0.1, ev: 1, url: "", posting_id: p.id });
    mockApi({ strategy: { phase: "learn", steps: [], tiers: { dream: [], target: [], practice: [] }, suggested: { dream: [row(posting, 100)], target: [], practice: [row(other, 62.4)] } }, postings: [posting, other], rejected: {}, board: [] });
    render(<JobsView />);
    const postings = await screen.findByRole("region", { name: /Postings that pass your criteria/ });
    const rated = within(postings).getByText("Staff Software Engineer, Platform").closest("tr")!;
    expect(within(rated).getByText("5/5")).toHaveAttribute("data-label", "Your rating");
    expect(within(within(postings).getByText("Senior Backend Engineer").closest("tr")!).getByText("—")).toHaveAttribute("data-label", "Your rating");
    const suggested = within(screen.getByRole("region", { name: "Suggestions: you choose" })).getByText("Northwind").closest("tr")!;
    expect(suggested).toHaveTextContent("100 (you rated 5/5)");
  });

  it("Library writes a study topic that is an acronym in capitals (US-75 visual review)", async () => {
    mockApi({ pages: [{ id: "k1", title: "What is AutoML?", section: "knowledge:ai", file: "k1" }, { id: "k2", title: "What is a B-tree?", section: "knowledge:computer science", file: "k2" }] });
    render(<LibraryView />);
    expect(await screen.findByRole("heading", { name: /^Interview knowledge · AI/ })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: /^Interview knowledge · Computer science/ })).toBeInTheDocument();
  });

  it("keeps each date in a plan row's reason whole, so a narrow day column never splits it (US-75 visual review)", () => {
    vi.setSystemTime(new Date("2026-10-23T12:00:00Z"));  // the year decides "Wed Oct 21" or "Wed Oct 21, 2026"
    const { container } = render(<p>{keepDates("due 2026-10-21; impact 5")}</p>);
    expect(container.querySelector(".nowrap")).toHaveTextContent(/^Wed Oct 21$/);  // a friendly day, kept whole (US-76)
    expect(container).toHaveTextContent("due Wed Oct 21; impact 5");
    expect(keepDates("impact 3")).toBe("impact 3");
  });

  it("says a day once and keeps another year's (US-76: no ISO date in the UI)", () => {
    vi.setSystemTime(new Date("2026-10-23T12:00:00Z"));  // the year decides "Wed Oct 21" or "Wed Oct 21, 2026"
    const { container } = render(<p>{keepDates("planned for Tue 2026-10-20, then by 2099-01-17; at 2026-10-19T09:00")}</p>);
    expect(container).toHaveTextContent("planned for Tue Oct 20, then by Sat Jan 17, 2099; at 2026-10-19T09:00");
  });

  it("keeps a title's time range whole, so a phone never splits (09:00– from 09:30) (US-76 visual review)", () => {
    const { container } = render(<p>{keepDates("Fix the flaky checkout test (09:00–09:30)")}</p>);
    expect(container.querySelector(".nowrap")).toHaveTextContent(/^\(09:00–09:30\)$/);
    expect(container).toHaveTextContent("Fix the flaky checkout test (09:00–09:30)");
  });

  it("Week says a task is too big for one day instead of no room (US-75 visual review)", async () => {
    const day = (d: string, extra = {}) => ({ day: d, weekday: "Mon", workday: true, meetings: [], schedule: [], free_minutes: 495, unplaced: [], calendar_connected: true, ...extra });
    mockApi({ days: [day("2026-10-19"), day("2026-10-20", { unplaced: ["t005"], unplaced_titles: ["Review the rollout"], too_big: [{ task: "t004", title: "Plan the Q1 platform roadmap", minutes: 720 }], day_focus_minutes: 396 })] });
    render(<WeekView />);
    expect(await screen.findByText(/Too big for one day: Plan the Q1 platform roadmap \(12h\)/)).toBeInTheDocument();
    expect(screen.getByText(/at most 6h 36m of focus time/)).toBeInTheDocument();
    expect(screen.getByText(/split it into tasks of a day or less/i)).toBeInTheDocument();
    expect(screen.getByText("No room in these 2 days for: Review the rollout")).toBeInTheDocument();
  });

  it("shows a clear message when a view is blocked on this machine", async () => {
    mockApi({ error: "job-hunt views are personal-only; this is a corporate machine" }, 403);
    render(<TodayView />);
    const alerts = await screen.findAllByRole("alert");  // Today and its daily-log panel both say why
    expect(alerts.every((a) => a.textContent?.includes("not available here"))).toBe(true);
  });
});


it("an unsourced interview loop names the published browser research skill", async () => {
  mockApi({ company: "Synthetic", known: false, loop: [], values: [], source_list: {} });
  render(<InterviewsView company="Synthetic" />);
  expect(await screen.findByText(/Add one with the careerbrew-browser-research skill/)).toBeVisible();
});
