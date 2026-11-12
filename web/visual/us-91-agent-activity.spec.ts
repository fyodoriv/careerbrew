import { open, patch, region, shot, test, TODAY } from "./helpers";

// US-91: Agent activity on Work, background agents' runs by project with each run's state and last event. The seed
// has no agent events (a row would show in every Work picture), so the answers are patched here.
const run = (agent: string, project: string, status: string, title: string, url = "", extra: Record<string, unknown> = {}) => ({
  agent, run: agent, project, status, title, url, at: `${TODAY}T13:10:00+00:00`, started_at: `${TODAY}T12:40:00+00:00`, events: 3, ...extra,
});
const AGENTS = {
  counts: { running: 2, quiet: 1, done: 1, failed: 1 },
  quiet_minutes: 60,
  groups: [
    { project: "billing", running: 1, last_at: `${TODAY}T13:10:00+00:00`, runs: [run("review-bot", "billing", "running", "Reviewing PR 7: 2 of 5 files read", "https://github.example.com/org/shop/pull/7"), run("fix-bot", "billing", "failed", "The flaky test failed again after the retry", "https://github.example.com/org/shop/actions/runs/42")] },
    { project: "ledger", running: 1, last_at: `${TODAY}T13:05:00+00:00`, runs: [run("docs-bot", "ledger", "running", "Writing the close checklist", "", { run: "pr-9" })] },
    { project: "", running: 0, last_at: `${TODAY}T11:00:00+00:00`, runs: [run("standup-bot", "", "quiet", "Reading the board", "", { events: 1 }), run("triage-bot", "", "done", "Triaged 12 feed events")] },
  ],
};

test("US-91: Agent activity on Work", async ({ page }) => {
  await patch(page, "**/api/agents", () => AGENTS);
  await open(page, "work");
  await shot(region(page, "Agent activity"), "agent-activity");
});

test("US-91: Agent activity with no agent yet", async ({ page }) => {
  await patch(page, "**/api/agents", () => ({ counts: { running: 0, quiet: 0, done: 0, failed: 0 }, quiet_minutes: 60, groups: [] }));
  await open(page, "work");
  await shot(region(page, "Agent activity"), "agent-activity-empty");
});
