import type { ReactElement } from "react";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Goal, Goals, Step, Teach } from "./api";
import { GoalsView, TeachView } from "./teach";

// Task goals-form-keeps-typed-text: a form in a block that shows the page's cached answer first (issue #142) keeps
// what the owner typed when the fresh answer replaces the cached one, and still takes the fresh lists.

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

const json = (body: unknown) => new Response(JSON.stringify(body));
const goals = (queue: Goal[], long_term: Goals["long_term"] = []): Goals => ({
  queue, long_term, upcoming: queue[0] ?? null, history: [], focus: { why: "synthetic focus", activities: [] }, days_left: null, kinds: ["interview", "learn", "custom"],
});
const goal = (title: string): Goal => ({ id: title.toLowerCase().replace(/\W+/g, "-"), kind: "learn", title, date: null, company: null, topics: [], minutes_per_day: 30 });

// Serves GET answers by path; a path in `held` waits until the returned release runs. Records every POST body.
function server(answers: Record<string, unknown>, held: string[] = []) {
  let release = () => {};
  const gate = new Promise<void>((r) => (release = r));
  const posts: Record<string, unknown>[] = [];
  vi.stubGlobal("fetch", vi.fn(async (url: string, init?: RequestInit) => {
    const path = String(url);
    if (path.endsWith("/api/tab-key")) return json({ key: null });
    if (init?.method === "POST") {
      const body = JSON.parse(String(init.body)) as { queue?: Goal[] };
      posts.push(body);
      const queue = (body.queue ?? []).map((g) => ({ ...g, id: g.id ?? goal(g.title).id }));
      return json(goals(queue));
    }
    if (held.includes(path)) await gate;
    return json(answers[path]);
  }));
  return { posts, release: () => release() };
}

// The page saw `cached` before (a view switch keeps it in memory); now it shows it while `fresh` is held.
async function showCachedThenHoldFresh(view: () => ReactElement, path: string, cached: unknown, fresh: unknown, ready: string, others: Record<string, unknown> = {}) {
  server({ ...others, [path]: cached });
  const first = render(view());
  await screen.findByText(ready);
  first.unmount();
  const s = server({ ...others, [path]: fresh }, [path]);
  render(view());
  expect(screen.getAllByText(/^Updated \d{2}:\d{2} · refreshing…$/).length).toBeGreaterThan(0);  // the cached answer, at once
  return s;
}
const freshArrived = () => waitFor(() => expect(screen.queryByText(/^Updated /)).toBeNull());

describe("Goals keeps typed text across the cached-to-fresh swap", () => {
  it("keeps a title typed into the cached form, and the save sends it", async () => {
    const s = await showCachedThenHoldFresh(() => <GoalsView />, "/api/goals", goals([]), goals([]), "New goal");
    fireEvent.change(screen.getByLabelText("Goal title"), { target: { value: "Practice" } });
    s.release();
    await freshArrived();
    expect(screen.getByLabelText("Goal title")).toHaveValue("Practice");
    fireEvent.click(screen.getByRole("button", { name: "Save and re-plan" }));
    await screen.findByText(/now follow "Practice"/);
    expect((s.posts[0].queue as Goal[]).map((g) => g.title)).toEqual(["Practice"]);
  });

  it("takes goals added elsewhere from the fresh answer, so the save keeps them and the typed goal", async () => {
    const soon = goal("Onsite at Northwind");
    const later = goal("Cloud exam");  // the CLI or an agent added it after the page cached its answer
    const s = await showCachedThenHoldFresh(() => <GoalsView />, "/api/goals", goals([soon]), goals([soon, later], [{ text: "Lead the platform team" }]), "Onsite at Northwind");
    fireEvent.click(screen.getByRole("button", { name: "Add a goal" }));
    fireEvent.change(screen.getByLabelText("Goal title"), { target: { value: "Mock interview" } });
    s.release();
    expect(await screen.findByRole("button", { name: "Edit Cloud exam" })).toBeInTheDocument();
    expect(screen.getByText("Lead the platform team")).toBeInTheDocument();
    expect(screen.getByLabelText("Goal title")).toHaveValue("Mock interview");
    fireEvent.click(screen.getByRole("button", { name: "Save and re-plan" }));
    await screen.findByText(/^Saved\./);
    expect((s.posts[0].queue as Goal[]).map((g) => g.title)).toEqual(["Onsite at Northwind", "Cloud exam", "Mock interview"]);
  });

  it("keeps an edit of a goal that moved in the fresh answer on that same goal", async () => {
    const a = goal("Onsite at Northwind");
    const b = goal("Cloud exam");
    const first = goal("Phone screen at Contoso");  // added elsewhere, ahead of the goal being edited
    const s = await showCachedThenHoldFresh(() => <GoalsView />, "/api/goals", goals([a, b]), goals([first, a, b]), "Cloud exam");
    fireEvent.click(screen.getByRole("button", { name: "Edit Cloud exam" }));
    fireEvent.change(screen.getByLabelText("Goal title"), { target: { value: "Cloud exam, part 2" } });
    s.release();
    expect(await screen.findByRole("button", { name: "Edit Phone screen at Contoso" })).toBeInTheDocument();
    expect(screen.getByLabelText("Goal title")).toHaveValue("Cloud exam, part 2");
    fireEvent.click(screen.getByRole("button", { name: "Save and re-plan" }));
    await screen.findByText(/^Saved\./);
    expect((s.posts[0].queue as Goal[]).map((g) => g.title)).toEqual(["Phone screen at Contoso", "Onsite at Northwind", "Cloud exam, part 2"]);
  });

  it("keeps an edit of a goal removed elsewhere as a new goal, so the typed text is not lost", async () => {
    const a = goal("Onsite at Northwind");
    const b = goal("Cloud exam");
    const s = await showCachedThenHoldFresh(() => <GoalsView />, "/api/goals", goals([a, b]), goals([a]), "Cloud exam");
    fireEvent.click(screen.getByRole("button", { name: "Edit Cloud exam" }));
    fireEvent.change(screen.getByLabelText("Goal title"), { target: { value: "Cloud exam, part 2" } });
    s.release();
    await freshArrived();
    expect(screen.queryByRole("button", { name: "Edit Cloud exam" })).toBeNull();
    expect(screen.getByRole("heading", { name: "New goal" })).toBeInTheDocument();
    expect(screen.getByLabelText("Goal title")).toHaveValue("Cloud exam, part 2");
    fireEvent.click(screen.getByRole("button", { name: "Save and re-plan" }));
    await screen.findByText(/^Saved\./);
    expect((s.posts[0].queue as Goal[]).map((g) => g.title)).toEqual(["Onsite at Northwind", "Cloud exam, part 2"]);
  });
});

describe("Teach me keeps typed text across the cached-to-fresh swap", () => {
  // The server stamps `started` on every read of the step, so the cached and the fresh answer differ only there.
  const lesson = (started: string): Teach => ({
    step: { kind: "lesson", title: "Lesson: hashing", why: "learn the idea first", started, pattern: "hashing", lesson: "Use a set.", prompt: "In one sentence, when would you use hashing?" } as Step,
    summary: { steps: 0, minutes: 0, by_kind: {}, mistake_repeat_rate: null },
    rules_version: "1",
  });
  const others = { "/api/goals": goals([goal("Practice")]), "/api/teach/improvements": { improvements: [] } };

  it("keeps an explanation typed into the cached lesson step", async () => {
    const s = await showCachedThenHoldFresh(() => <TeachView />, "/api/teach", lesson("2026-10-19T09:00:00+00:00"), lesson("2026-10-19T09:00:07+00:00"), "Lesson: hashing", others);
    fireEvent.change(screen.getByLabelText("Explain it back"), { target: { value: "To check membership in O(1)." } });
    s.release();
    await freshArrived();
    expect(screen.getByLabelText("Explain it back")).toHaveValue("To check membership in O(1).");
    expect(screen.getByRole("button", { name: "I understand it" })).toBeEnabled();
  });
});
