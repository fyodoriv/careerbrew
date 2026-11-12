import { open, shot, test } from "./helpers";

// US-35: every view opens from the navigation with real data, and nothing scrolls sideways at any width.
// One picture per view, of the screen as it opens. Each story's own spec shows the blocks further down.
const VIEWS = ["dashboard", "teach", "today", "standup", "notifications", "goals", "week", "jobs", "interviews", "practice", "library", "work", "review", "reports", "actions", "setup"];

for (const view of VIEWS) {
  test(`US-35: the ${view} view as it opens`, async ({ page }) => {
    await open(page, view);
    await shot(page, `view-${view}`);
  });
}
