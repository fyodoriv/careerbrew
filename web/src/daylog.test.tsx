import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { DayLog, EarlierDays, InboxBlock, PmCopyBar, PmUpdate } from "./daylog";
import { useDayLog } from "./useDayLog";
import { clearCache } from "./cache";

const view = {
  log: { day: "2026-10-19", tomorrow: "2026-10-20", done: { Shipped: ["Merged the invoice export"] }, planned: ["t002 Backfill old invoices"], blockers: [], questions: ["What was the impact of: invoice export?"], advice: [] },
  markdown: "",
  pm: { subject: "Daily update 2026-10-19", body: "Done today:\n- Merged the invoice export", to: "pm@example.com", via: "email" },
  inbox: [
    { id: "goal-1", type: "proposal", kind: "goal", text: "New goal: close the Technical design gap", status: "open" },
    { id: "impact-1", type: "question", kind: "impact", text: "What was the impact of: invoice export?", status: "open" },
  ],
  history: ["2026-10-19", "2026-10-18"],
};

it("marks each Day log loader fresh only after its current request succeeds", async () => {
  clearCache();
  const releases: ((response: Response) => void)[] = [];
  vi.stubGlobal("fetch", vi.fn((url: string) => String(url).endsWith("/api/daylog")
    ? new Promise<Response>(resolve => { releases.push(resolve); })
    : Promise.resolve(new Response(JSON.stringify(view), { status: 200 }))));
  const { container } = render(<><PmCopyBar /><DayLogPanel /></>);
  const markers = () => [container.querySelector('[data-block="copy-update"]'), container.querySelector('[data-block="pm-update"]')];
  await waitFor(() => expect(releases).toHaveLength(2));
  expect(markers().map(node => node?.getAttribute("data-daylog-fresh"))).toEqual(["false", "false"]);
  act(() => { window.dispatchEvent(new Event("careerbrew:changed")); });
  await waitFor(() => expect(releases).toHaveLength(4));
  await act(async () => { releases[0](new Response(JSON.stringify(view), { status: 200 })); releases[1](new Response(JSON.stringify(view), { status: 200 })); });
  expect(markers().map(node => node?.getAttribute("data-daylog-fresh"))).toEqual(["false", "false"]);
  await act(async () => { releases[2](new Response(JSON.stringify(view), { status: 200 })); });
  await waitFor(() => expect(markers().filter(node => node?.getAttribute("data-daylog-fresh") === "true")).toHaveLength(1));
  await act(async () => { releases[3](new Response(JSON.stringify(view), { status: 200 })); });
  await waitFor(() => expect(markers().map(node => node?.getAttribute("data-daylog-fresh"))).toEqual(["true", "true"]));
  clearCache();
});

it("does not mark a failed Day log load fresh", () => {
  const data = view as unknown as Parameters<typeof PmUpdate>[0]["log"]["data"];
  const { container } = render(<PmUpdate log={{ data, error: "Synthetic failure", loading: false, cached: null, reload: () => {} }} />);
  expect(container.querySelector('[data-block="pm-update"]')).toHaveAttribute("data-daylog-fresh", "false");
});

// Today's day-log blocks (issue #146), in Today's order, on their shared reads.
function DayLogPanel() {
  const day = useDayLog();
  return (
    <>
      <PmUpdate log={day.log} />
      <InboxBlock type="question" items={day.items} onDone={day.onDone} />
      <InboxBlock type="proposal" items={day.items} onDone={day.onDone} />
      <DayLog log={day.log} />
      <EarlierDays log={day.log} />
    </>
  );
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
  Reflect.deleteProperty(navigator, "clipboard");
});

function clipboard(writeText: (text: string) => Promise<void>) {
  const spy = vi.fn(writeText);
  Object.defineProperty(navigator, "clipboard", { value: { writeText: spy }, configurable: true });
  return spy;
}

describe("daily log panel", () => {
  it("an empty plan for tomorrow says today's plan holds every open task, not that no task fits", async () => {
    const held = { ...view, log: { ...view.log, planned: [], today_holds: 2 } };
    vi.stubGlobal("fetch", vi.fn((url: string) => Promise.resolve(new Response(JSON.stringify(String(url).includes("/api/inbox") ? { inbox: [] } : held), { status: 200 }))));
    render(<DayLogPanel />);
    expect(await screen.findByText("Nothing left for Tue Oct 20: today's plan holds every open task.")).toBeInTheDocument();
    expect(screen.queryByText(/No task fits yet/)).toBeNull();
  });

  it("opens a Plan row's details: link, project, why, related, history", async () => {
    const detailed = { ...view, log: { ...view.log, details: { done: { Shipped: [] }, planned: [{ task: "t002", title: "Backfill old invoices", url: "https://jira.example.com/browse/PROJ-9", project: "billing", why: "Placed on the plan by score.", related: ["PROJ-9"], history: [{ at: "2026-10-18T09:00:00", event: "added by user" }] }] } } };
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(detailed), { status: 200 }))));
    render(<DayLogPanel />);
    fireEvent.click(await screen.findByText("t002 Backfill old invoices"));
    expect(screen.getByRole("link", { name: "https://jira.example.com/browse/PROJ-9" })).toBeInTheDocument();
    expect(screen.getByText("billing")).toBeInTheDocument();
    expect(screen.getByText("PROJ-9")).toBeInTheDocument();
    expect(screen.getByText("added by user", { exact: false }).closest("li")).toHaveTextContent("Sun Oct 18 09:00 added by user");
  });

  it("shows done and planned work, a PM draft to copy or send, and asks for approval", async () => {
    const calls: string[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string) => { calls.push(String(url)); return Promise.resolve(new Response(JSON.stringify(view), { status: 200 })); }));
    render(<DayLogPanel />);
    expect(await screen.findByText("Merged the invoice export")).toBeInTheDocument();
    expect(screen.getByText("t002 Backfill old invoices")).toBeInTheDocument();
    const send = screen.getByRole("link", { name: "Email pm@example.com" });
    expect(send.getAttribute("href")).toMatch(/^mailto:pm%40example\.com\?subject=Daily%20update/);
    fireEvent.change(screen.getByLabelText("PM update"), { target: { value: "Edited update" } });
    expect(screen.getByRole("link", { name: "Email pm@example.com" }).getAttribute("href")).toContain("Edited%20update");
    fireEvent.click(screen.getByRole("button", { name: "Approve goal" }));
    await screen.findByText("Merged the invoice export");
    expect(calls.some((c) => c.endsWith("/api/inbox/goal-1/accept"))).toBe(true);
  });

  it("sends the edited update to Slack by default", async () => {
    const posts: { url: string; body: string }[] = [];
    const slackView = { ...view, pm: { ...view.pm, via: "slack", to: "#billing-pm", slack_configured: true } };
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") posts.push({ url: String(url), body: String(init.body) });
      const out = String(url).endsWith("/api/pm/send") ? { sent: { via: "slack", to: "#billing-pm", ts: "1" } } : slackView;
      return Promise.resolve(new Response(JSON.stringify(out), { status: 200 }));
    }));
    render(<DayLogPanel />);
    await screen.findByRole("button", { name: "Send to Slack #billing-pm" });  // the slack view has loaded
    fireEvent.change(screen.getByLabelText("PM update"), { target: { value: "Shipped the export" } });
    fireEvent.click(screen.getByRole("button", { name: "Send to Slack #billing-pm" }));
    expect(await screen.findByRole("button", { name: "Sent to #billing-pm" })).toBeDisabled();
    expect(posts[0].url).toMatch(/\/api\/pm\/send$/);
    expect(posts[0].body).toContain("Shipped the export");
  });

  it("saves the edited PM update for the day when the box loses focus", async () => {
    const posts: { url: string; body: string }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") posts.push({ url: String(url), body: String(init.body) });
      const out = String(url).endsWith("/api/pm/draft") ? { saved: true, edited: true } : view;
      return Promise.resolve(new Response(JSON.stringify(out), { status: 200 }));
    }));
    render(<DayLogPanel />);
    const box = await screen.findByLabelText("PM update");
    fireEvent.change(box, { target: { value: "My own words" } });
    fireEvent.blur(box);
    expect(await screen.findByText("Your edits are saved for this day.")).toBeInTheDocument();
    expect(posts[0].url).toMatch(/\/api\/pm\/draft$/);
    expect(JSON.parse(posts[0].body)).toEqual({ day: "2026-10-19", text: "My own words" });
  });

  // Task goals-form-keeps-typed-text: a new answer with a new draft (the fresh one after the page's cached one,
  // issue #142, or a live refresh after the watcher found new work) never drops what the owner is typing.
  async function cachedThenNewDraft() {
    const newer = { ...view, pm: { ...view.pm, body: "Done today:\n- Merged the invoice export\n- Fixed the login bug" } };
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(view)))));
    const first = render(<DayLogPanel />);
    expect(await screen.findByLabelText("PM update")).toHaveValue(view.pm.body);
    first.unmount();
    let release = () => {};
    const gate = new Promise<void>((r) => (release = r));
    vi.stubGlobal("fetch", vi.fn(async () => { await gate; return new Response(JSON.stringify(newer)); }));
    render(<DayLogPanel />);
    return { newer, release: async () => { release(); await waitFor(() => expect(screen.queryByText(/^Updated /)).toBeNull()); } };
  }

  it("keeps unsaved typing in the PM box when a new answer changes the draft", async () => {
    const s = await cachedThenNewDraft();
    fireEvent.change(screen.getByLabelText("PM update"), { target: { value: "My own words" } });
    await s.release();
    expect(screen.getByLabelText("PM update")).toHaveValue("My own words");
  });

  it("shows the new draft when the PM box holds no typing", async () => {
    const s = await cachedThenNewDraft();
    await s.release();
    expect(screen.getByLabelText("PM update")).toHaveValue(s.newer.pm.body);
  });
});

describe("Today's day-log blocks page their lists (issue #146)", () => {
  it("shows 10 suggestions at a time with a kind filter, and Housekeeping folded with its count", async () => {
    window.history.replaceState(null, "", "#/today");
    const suggestions = Array.from({ length: 23 }, (_, i) => ({ id: `s${i + 1}`, type: "proposal", kind: i < 20 ? "agent-task" : "goal", text: `Suggestion ${i + 1}`, status: "open" }));
    const busy = { ...view, log: { ...view.log, housekeeping: ["billing: check https://wiki.example.com/billing"] }, inbox: [...view.inbox, ...suggestions] };
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(busy), { status: 200 }))));
    render(<DayLogPanel />);
    const block = await screen.findByRole("region", { name: "careerbrew suggests (24)" });
    expect(block).toHaveAttribute("data-block", "suggestions");
    expect(within(block).getAllByRole("listitem")).toHaveLength(10);
    expect(within(block).getByRole("group", { name: "Pages of suggestions" })).toHaveTextContent("Showing 1–10 of 24");
    fireEvent.click(within(block).getByRole("button", { name: "goal (4)" }));
    expect(window.location.hash).toBe("#/today?suggestions-filter=goal");
    expect(within(block).getAllByRole("listitem")).toHaveLength(4);
    expect(within(block).queryByRole("group", { name: "Pages of suggestions" })).toBeNull();
    const housekeeping = screen.getByRole("heading", { name: "Housekeeping (1)" }).closest("details")!;
    expect(housekeeping.open).toBe(false);
    expect(screen.getByRole("region", { name: "Questions for you (1)" })).toHaveAttribute("data-block", "questions");
  });
});

describe("the suggestions inbox explains itself (issue #147)", () => {
  const row = (id: string, extra: object = {}) => ({
    id, type: "proposal", kind: "source", status: "open", text: `Add tracker ${id} to project syn-onboarding`, what: `Add tracker ${id} to project syn-onboarding: Synthetic child ticket ${id}`,
    why: `${id} is a child of SYN-1, which project syn-onboarding already follows`, accept: "Adds it to the sources of project syn-onboarding.", dismiss: "Nothing changes; this suggestion is hidden and does not come back.",
    added_by: "careerbrew: research on project syn-onboarding", links: [{ label: id, url: `https://tickets.example.com/browse/${id}` }], lands: { label: "Work: Projects", url: "#/work" },
    project: "syn-onboarding", priority: 1, created_at: "2026-10-19T09:00:00-04:00", ...extra,
  });
  const group = {
    id: "grp-0123456789", kind: "source", count: 3, label: "Add 3 trackers to project syn-onboarding", why: "each is a child of SYN-1, which project syn-onboarding already follows",
    accept: "Adds all 3 to the sources of project syn-onboarding.", dismiss: "Hides all 3; they do not come back.", added_by: "careerbrew: research on project syn-onboarding",
    links: [{ label: "SYN-1 (parent)", url: "https://tickets.example.com/browse/SYN-1" }], lands: { label: "Work: Projects", url: "#/work" }, project: "syn-onboarding", priority: 1, created_at: "2026-10-19T09:00:00-04:00", ids: ["SYN-11", "SYN-12", "SYN-13"],
  };
  const lone = row("SYN-20", { kind: "agent-task", what: "Plan task t009 (Review acme/shop#12)", added_by: "an agent", project: "", priority: null, maybe_done: { what: "PR acme/shop#12 merged", url: "https://github.com/acme/shop/pull/12", at: "2026-10-19T10:00:00Z" } });
  const gone = row("SYN-30", { status: "expired", expired: { what: "you gave a review (github)", url: "https://github.com/acme/shop/pull/30", at: "2026-10-19T12:00:00Z" } });
  const inboxView = { ...view, inbox: [...["SYN-11", "SYN-12", "SYN-13"].map((id) => row(id, { group: group.id })), lone], groups: [group], expired: [gone] };
  const serve = (posts: { url: string; body: string }[]) => vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
    if (init?.method === "POST") posts.push({ url: String(url), body: String(init.body) });
    const decided = { ...view, inbox: [lone], groups: [], expired: [gone], result: { decided: 3, failed: [] } };
    return Promise.resolve(new Response(JSON.stringify(init?.method === "POST" ? decided : inboxView), { status: 200 }));
  }));
  const suggests = () => screen.findByRole("region", { name: "careerbrew suggests (4)" });

  it("says what a suggestion is, and each row shows its source, links, and what Accept and Dismiss will do", async () => {
    window.history.replaceState(null, "", "#/today");
    serve([]);
    render(<DayLogPanel />);
    const block = await suggests();
    expect(within(block).getByText(/^A suggestion is a change careerbrew proposes/)).toHaveTextContent("Nothing changes until you press Accept");
    const item = block.querySelector('li[data-inbox="agent-task"]') as HTMLElement;
    expect(within(item).getByText("an agent")).toBeInTheDocument();
    expect(within(item).getByRole("link", { name: "Work: Projects" })).toHaveAttribute("href", "#/work");  // where Accept lands it
    expect(within(item).getByRole("link", { name: "SYN-20" })).toHaveAttribute("href", "https://tickets.example.com/browse/SYN-20");
    expect(within(item).getByText("Accept will")).toBeInTheDocument();
    expect(within(item).getByText("Dismiss will")).toBeInTheDocument();
    expect(within(item).getByRole("note")).toHaveTextContent("Maybe done already: PR acme/shop#12 merged");
    const g = block.querySelector('li[data-group="grp-0123456789"]') as HTMLElement;
    expect(within(g).getByText("Add 3 trackers to project syn-onboarding")).toBeInTheDocument();
    expect(within(g).getByRole("link", { name: "SYN-1 (parent)" })).toBeInTheDocument();
    expect(within(g).getByText("Accept all will")).toBeInTheDocument();
    expect(block.querySelectorAll("li[data-inbox]")).toHaveLength(1);  // the 3 alike rows are one group row until Review each
  });

  it("Accept all asks first: Cancel changes nothing, Yes sends yes and the ids it showed once, and the group goes", async () => {
    const posts: { url: string; body: string }[] = [];
    serve(posts);
    render(<DayLogPanel />);
    const block = await suggests();
    const g = block.querySelector('li[data-group="grp-0123456789"]') as HTMLElement;
    fireEvent.click(within(g).getByRole("button", { name: "Accept all" }));
    const confirm = within(g).getByRole("group", { name: "Confirm Accept all" });
    expect(confirm).toHaveTextContent("Accept all 3? Adds all 3 to the sources of project syn-onboarding. This is hard to undo.");
    fireEvent.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(within(g).queryByRole("group", { name: "Confirm Accept all" })).toBeNull();
    expect(posts).toEqual([]);
    fireEvent.click(within(g).getByRole("button", { name: "Accept all" }));
    fireEvent.click(within(g).getByRole("button", { name: "Yes, accept all" }));
    await waitFor(() => expect(screen.queryByText("Add 3 trackers to project syn-onboarding")).toBeNull());
    expect(posts).toEqual([{ url: "/api/inbox-group/grp-0123456789/accept", body: JSON.stringify({ yes: true, ids: ["SYN-11", "SYN-12", "SYN-13"] }) }]);  // only the rows it counted
  });

  it("Review each shows every row of the group with its own buttons; Dismiss all asks first too", async () => {
    serve([]);
    render(<DayLogPanel />);
    const g = (await suggests()).querySelector('li[data-group="grp-0123456789"]') as HTMLElement;
    const review = within(g).getByRole("button", { name: "Review each" });
    expect(review).toHaveAttribute("aria-expanded", "false");
    fireEvent.click(review);
    expect(review).toHaveAttribute("aria-expanded", "true");
    expect(g.querySelectorAll('li[data-inbox="source"]')).toHaveLength(3);
    expect(within(g).getByText("Add tracker SYN-11 to project syn-onboarding: Synthetic child ticket SYN-11")).toBeInTheDocument();  // each row's ticket title
    expect(within(g).getAllByRole("button", { name: "Accept" })).toHaveLength(3);
    fireEvent.click(within(g).getByRole("button", { name: "Dismiss all" }));
    expect(within(g).getByRole("group", { name: "Confirm Dismiss all" })).toHaveTextContent("Dismiss all 3? Hides all 3; they do not come back.");
    expect(within(g).getByRole("button", { name: "Yes, dismiss all" })).toBeInTheDocument();
  });

  it("lists the suggestions done elsewhere, folded, with the signal and Bring back", async () => {
    const posts: { url: string; body: string }[] = [];
    serve(posts);
    render(<DayLogPanel />);
    const block = await suggests();
    const fold = within(block).getByRole("heading", { name: "Done elsewhere (1)" }).closest("details")!;
    expect(fold.open).toBe(false);
    expect(within(fold).getByRole("link", { name: "you gave a review (github)" })).toHaveAttribute("href", "https://github.com/acme/shop/pull/30");
    fireEvent.click(within(fold).getByRole("button", { name: "Bring back" }));
    await waitFor(() => expect(posts.map((x) => x.url)).toEqual(["/api/inbox/SYN-30/reopen"]));
  });
});

describe("Copy update at the top (issue #145)", () => {
  const slackView = { ...view, pm: { ...view.pm, via: "slack", to: "#billing-pm", slack_configured: true, edited: false, updated_at: "2026-10-19T15:45-04:00" } };
  const serve = (v: object) => vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify(v), { status: 200 }))));

  it("copies the body without the subject for Slack in one click, and says so in a toast", async () => {
    serve(slackView);
    const writes = clipboard(() => Promise.resolve());
    render(<PmCopyBar />);
    expect(await screen.findByRole("heading", { name: "Monday, Oct 19" })).toBeInTheDocument();
    expect(screen.getByText("generated draft · updated 15:45")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy update" }));
    expect(writes).toHaveBeenCalledWith("Done today:\n- Merged the invoice export");
    expect(await screen.findByRole("status")).toHaveTextContent("Copied, paste it in Slack");
  });

  it("puts the subject first for email, and names the owner's edit", async () => {
    serve({ ...view, pm: { ...view.pm, edited: true, updated_at: "2026-10-19T09:05-04:00" } });
    const writes = clipboard(() => Promise.resolve());
    render(<PmCopyBar />);
    expect(await screen.findByText("your edit · updated 09:05")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy update" }));
    expect(writes).toHaveBeenCalledWith("Daily update 2026-10-19\n\nDone today:\n- Merged the invoice export");
    expect(await screen.findByRole("status")).toHaveTextContent("Copied, paste it in your email");
  });

  it("copies the PM box's unsaved edit, not the text the server last sent", async () => {
    serve(slackView);
    const writes = clipboard(() => Promise.resolve());
    render(<><PmCopyBar /><DayLogPanel /></>);
    fireEvent.change(await screen.findByLabelText("PM update"), { target: { value: "• billing: [the ledger](https://jira.example.com/browse/BILL-7)" } });
    fireEvent.click(screen.getByRole("button", { name: "Copy update" }));
    expect(writes).toHaveBeenCalledWith("• billing: the ledger <https://jira.example.com/browse/BILL-7>");
  });

  it("shows the text selected to copy by hand when the browser refuses the clipboard", async () => {
    serve(slackView);
    clipboard(() => Promise.reject(new DOMException("Write permission denied.", "NotAllowedError")));
    render(<PmCopyBar />);
    fireEvent.click(await screen.findByRole("button", { name: "Copy update" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("The browser did not allow the copy");
    const box = screen.getByLabelText("Update to copy") as HTMLTextAreaElement;
    expect(box.value).toBe("Done today:\n- Merged the invoice export");
    await waitFor(() => expect([box.selectionStart, box.selectionEnd]).toEqual([0, box.value.length]));
    fireEvent.click(screen.getByRole("button", { name: "Close" }));
    expect(screen.queryByLabelText("Update to copy")).not.toBeInTheDocument();
  });

  it("falls back the same way when there is no clipboard API at all", async () => {
    serve(slackView);
    render(<PmCopyBar />);
    fireEvent.click(await screen.findByRole("button", { name: "Copy update" }));
    expect(await screen.findByLabelText("Update to copy")).toHaveValue("Done today:\n- Merged the invoice export");
  });

  // Issue #142: the bar is a block like any other: a visible loading label, its own error with the fix and Retry.
  it("says it loads, fails on its own with the fix and Retry, and Retry loads it", async () => {
    let fail = true;
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(fail
      ? new Response(JSON.stringify({ error: "synthetic failure" }), { status: 500 })
      : new Response(JSON.stringify(slackView), { status: 200 }))));
    render(<PmCopyBar />);
    expect(screen.getByText("Loading the PM update…")).toBeVisible();
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Could not load the PM update: synthetic failure");
    expect(alert).toHaveTextContent("Fix:");
    fail = false;
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(await screen.findByText("generated draft · updated 15:45")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).toBeNull();
  });
});

describe("Why it matters notes (issue #214)", () => {
  const why = [{ label: "billing", note: "Helps the support team; expected: fewer failed exports." }];
  const withWhy = { ...view, pm: { ...view.pm, via: "slack", to: "#billing-pm", slack_configured: true, why } };

  it("shows one owner-only note per line, and Copy and Send never carry it", async () => {
    const posts: { url: string; body: string }[] = [];
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (init?.method === "POST") posts.push({ url, body: String(init.body) });
      const answer = url.includes("/api/pm/send") ? { sent: { via: "slack", to: "#billing-pm" } } : withWhy;
      return Promise.resolve(new Response(JSON.stringify(answer), { status: 200 }));
    }));
    const writes = clipboard(() => Promise.resolve());
    render(<DayLogPanel />);
    const notes = await screen.findByText(/for you only: never copied or sent/);
    expect(notes.closest("[data-pm-why]")).toHaveTextContent("billing: Helps the support team; expected: fewer failed exports.");
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    expect(writes).toHaveBeenCalledWith("Done today:\n- Merged the invoice export");
    fireEvent.click(screen.getByRole("button", { name: "Send to Slack #billing-pm" }));
    await waitFor(() => expect(posts.some((p) => p.url.includes("/api/pm/send"))).toBe(true));
    expect(posts.every((p) => !p.body.includes("support team"))).toBe(true);
  });

  it("shows nothing when no line has a note", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ ...view, pm: { ...view.pm, why: [] } }), { status: 200 }))));
    render(<DayLogPanel />);
    await screen.findByLabelText("PM update");
    expect(document.querySelector("[data-pm-why]")).toBeNull();
  });
});

describe("Content checks (issue #226)", () => {
  const checks = [{ rule: "time-estimate", line: "billing: next: backfill, ETA Friday.", fix: "no time estimates: say what is done and what is next" }];

  it("lists each line that breaks a rule, with its fix, and Copy never carries it", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ ...view, pm: { ...view.pm, via: "slack", to: "#billing-pm", slack_configured: true, checks } }), { status: 200 }))));
    const writes = clipboard(() => Promise.resolve());
    render(<DayLogPanel />);
    const box = (await screen.findByText(/Content checks/)).closest("[data-pm-checks]");
    expect(box).toHaveTextContent("no time estimates: say what is done and what is next: billing: next: backfill, ETA Friday.");
    fireEvent.click(screen.getByRole("button", { name: "Copy" }));
    expect(writes).toHaveBeenCalledWith("Done today:\n- Merged the invoice export");
  });

  it("shows nothing when every line passes", async () => {
    vi.stubGlobal("fetch", vi.fn(() => Promise.resolve(new Response(JSON.stringify({ ...view, pm: { ...view.pm, checks: [] } }), { status: 200 }))));
    render(<DayLogPanel />);
    await screen.findByLabelText("PM update");
    expect(document.querySelector("[data-pm-checks]")).toBeNull();
  });
});


describe("Slack setup for the PM update (issue #375)", () => {
  function renderPm(via: "slack" | "email", to: string, configured: boolean, refusal?: string) {
    const posts: string[] = [];
    const data = { ...view, pm: { ...view.pm, via, to, slack_configured: configured } };
    vi.stubGlobal("fetch", vi.fn((url: string, init?: RequestInit) => {
      if (String(url).endsWith("/api/pm/send") && init?.method === "POST") {
        posts.push(String(init.body));
        return Promise.resolve(new Response(JSON.stringify({ error: refusal ?? "synthetic send must stay local" }), { status: 400 }));
      }
      return Promise.resolve(new Response(JSON.stringify(data), { status: 200 }));
    }));
    render(<DayLogPanel />);
    return posts;
  }

  it("P01 shows Slack setup before a disconnected send can be attempted", async () => {
    const posts = renderPm("slack", "#billing-pm", false, "Slack is not connected");
    const button = await screen.findByRole("button", { name: "Send to Slack #billing-pm" });
    if (!(button instanceof HTMLButtonElement)) throw new Error("Expected a native Send button");
    const enabled = !button.disabled;
    const setupHint = screen.queryByText("careerbrew connect slack") !== null;
    if (enabled) {
      fireEvent.click(button);
      await screen.findByRole("alert");
    }
    const observed = { enabled, setupHint, postCount: posts.length };
    expect(observed).toEqual({ enabled: false, setupHint: true, postCount: 0 });
  });

  it("C02 keeps the destination hint when Slack is configured without a destination", async () => {
    const posts = renderPm("slack", "", true);
    expect(await screen.findByRole("button", { name: "Send to Slack" })).toBeDisabled();
    expect(screen.getByText("careerbrew config set pm_slack #channel-or-member-id")).toBeInTheDocument();
    expect(posts).toEqual([]);
  });

  it("C03 opens the existing mail draft without a Slack request", async () => {
    const posts = renderPm("email", "pm@example.com", false);
    const mail = await screen.findByRole("link", { name: "Email pm@example.com" });
    expect(mail).toHaveAttribute("href", "mailto:pm%40example.com?subject=Daily%20update%202026-10-19&body=Done%20today%3A%0A-%20Merged%20the%20invoice%20export");
    expect(posts).toEqual([]);
  });

  it("C05 keeps the configured provider refusal and does not report success", async () => {
    const posts = renderPm("slack", "#billing-pm", true, "Slack refused: channel_not_found");
    const button = await screen.findByRole("button", { name: "Send to Slack #billing-pm" });
    fireEvent.click(button);
    expect(await screen.findByRole("alert")).toHaveTextContent("channel_not_found");
    expect(button).toBeEnabled();
    expect(screen.queryByRole("button", { name: /Sent to/ })).toBeNull();
    expect(posts).toHaveLength(1);
  });
});
