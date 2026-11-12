import { expect, open, patch, region, shot, test } from "./helpers";

// US-73: each view starts with the block I act on first, and a long list shows one page with "Showing" and
// Previous and Next. The long lists are synthetic rows patched in the page; the seeded server never changes.
const suggestion = (i: number, kind: string) => ({ id: `synthetic-${i}`, type: "proposal", kind, text: `Synthetic suggestion ${i}`, status: "open" });

test("US-73: a long list on its second page", async ({ page }) => {
  const rows = [...Array.from({ length: 19 }, (_, i) => suggestion(i + 1, "agent-task")), ...Array.from({ length: 4 }, (_, i) => suggestion(20 + i, "goal"))];
  await patch(page, "**/api/inbox", () => ({ inbox: rows }));
  await open(page, "today?suggestions=2");
  const block = region(page, "careerbrew suggests (23)");
  await expect(block.getByRole("group", { name: "Pages of suggestions" })).toContainText("Showing 11–20 of 23");
  await shot(block, "suggestions-page-2");
});

// A synthetic notification, open ("new") or closed ("dismissed").
const notification = (i: number, status: string, at: string) => ({
  id: `synthetic-${i}`, kind: "dm", source: "slack", headline: `Synthetic notification ${i}`, title: "", why: "a synthetic row for the paging picture", next: "nothing",
  priority: 3, priority_why: "a test row", count: 1, at, status, snooze_until: "", resolved: status === "dismissed" ? "dismissed" : "",
  url: "", link_label: "", accept_label: "", actions: status === "new" ? ["dismiss"] : [], popup: "held",
});

test("US-73: a folded block shows its count", async ({ page }) => {
  await patch<{ items: { at: string }[] }>(page, "**/api/notifications", (body) => {
    const at = body.items[0]?.at ?? "";
    return { ...body, items: [...Array.from({ length: 14 }, (_, i) => notification(i + 1, "new", at)), ...Array.from({ length: 3 }, (_, i) => notification(15 + i, "dismissed", at))] };
  });
  await open(page, "notifications");
  await expect(region(page, /^Open \(\d+\)$/).getByRole("group", { name: "Pages of open notifications" })).toContainText("Showing 1–10 of 14");
  await shot(region(page, "Closed (3)"), "closed-folded");
});
