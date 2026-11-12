import { open, patch, region, shot, test } from "./helpers";

// US-82: the ranked projects at the top of Today and above Projects, with their open tasks and today's items, and the
// Projects list with ranked projects first and a rank badge. The seed ranks nothing, so the answers are patched here.
const RANKED = [
  { name: "billing", rank: 1, match: ["*billing*"], open_tasks: 1, today: 2 },
  { name: "ledger", rank: 2, match: ["ledger"], open_tasks: 0, today: 0 },
  { name: "invoices", rank: 3, match: ["BILL-7"], open_tasks: 3, today: 1 },
];
interface Project { name: string; depth: number; rank?: number | null }

test("US-82: Active now on Today", async ({ page }) => {
  await patch(page, "**/api/priority", () => ({ projects: RANKED }));
  await open(page, "today");
  await shot(region(page, "Active now"), "active-now");
});

test("US-82: ranked projects first in Projects", async ({ page }) => {
  await patch(page, "**/api/priority", () => ({ projects: RANKED }));
  await patch<{ projects: Project[] }>(page, "**/api/work", (d) => {
    const rank = new Map(RANKED.map((r) => [r.name, r.rank]));
    const ranked = d.projects.filter((p) => rank.has(p.name)).sort((a, b) => (rank.get(a.name) ?? 0) - (rank.get(b.name) ?? 0));
    return { ...d, projects: [...ranked.map((p) => ({ ...p, depth: 0, rank: rank.get(p.name) })), ...d.projects.filter((p) => !rank.has(p.name))] };
  });
  await open(page, "work");
  await shot(region(page, "Projects"), "ranked-projects");
});
