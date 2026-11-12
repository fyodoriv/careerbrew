import { expect, open, patch, region, shot, test, TODAY } from "./helpers";

// US-74: the suggestions inbox explains itself (what a suggestion is, each row's source, links, and what Accept and
// Dismiss do), alike rows form one group, rows done elsewhere are kept apart, and Dashboard lists the first steps.
// The group and the row done elsewhere are synthetic rows patched in the page; the seeded server never changes.
const GROUP = "agent-task:synthetic-onboarding";
const titles = ["Draft the synthetic onboarding page", "Record the synthetic onboarding demo", "Write the synthetic onboarding quiz"];
const why = "an agent added them: the synthetic onboarding sprint needs them";
const accept = "Moves the task from proposed to open, so it is planned in Week.";
const dismiss = "Hides the suggestion for good; the task stays proposed.";
const row = (title: string, i: number) => ({
  id: `synthetic-${i + 1}`, type: "proposal", kind: "agent-task", text: title, status: "open", created_at: `${TODAY}T08:0${i}:00`,
  what: title, why, accept, dismiss, added_by: "an agent", links: [{ label: "tickets.example.com/browse/SYN-4" + (i + 1), url: `https://tickets.example.com/browse/SYN-4${i + 1}` }],
  lands: { label: "Week", url: "#/week" }, project: "", priority: null, group: GROUP,
});

test("US-74: alike suggestions as one group, and one done elsewhere", async ({ page }) => {
  await patch<{ inbox: unknown[] }>(page, "**/api/inbox", (d) => ({
    ...d,
    inbox: [...titles.map(row), ...d.inbox],
    groups: [{
      id: GROUP, kind: "agent-task", count: 3, label: "Plan 3 tasks an agent added", why, accept: "Moves all 3 tasks from proposed to open, so they are planned in Week.",
      dismiss: "Hides all 3 suggestions for good.", added_by: "an agent", links: [{ label: "tickets.example.com/browse/SYN-41", url: "https://tickets.example.com/browse/SYN-41" }],
      lands: { label: "Week", url: "#/week" }, project: "", priority: null, created_at: `${TODAY}T08:00:00`, ids: titles.map((_, i) => `synthetic-${i + 1}`),
    }],
    expired: [{
      id: "synthetic-done", type: "proposal", kind: "agent-task", text: "Review the synthetic checkout PR", status: "expired", what: "Review the synthetic checkout PR",
      expired: { what: "you reviewed acme/shop#12", url: "https://code.example.com/acme/shop/pulls/12", at: `${TODAY}T08:30:00` },
    }],
  }));
  await open(page, "today");
  const block = region(page, /^careerbrew suggests/);
  const group = block.locator(`li[data-group="${GROUP}"]`);
  await group.getByRole("button", { name: "Review each" }).click();
  await expect(group.locator("li[data-inbox]")).toHaveCount(3);
  await block.getByText(/^Done elsewhere \(1\)$/).click();
  await expect(block.getByRole("button", { name: "Bring back" })).toBeVisible();
  await shot(block, "suggestions-grouped");
});

test("US-74: Getting started on Dashboard", async ({ page }) => {
  await open(page, "dashboard");
  await shot(region(page, /^Getting started/), "getting-started");
});
