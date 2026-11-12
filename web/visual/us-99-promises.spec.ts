import { addDays, open, patch, region, shot, test, TODAY } from "./helpers";

// US-99: Dashboard's Commitments block lists the promises you made, overdue first, each linked to its message with
// who it was made to, its due day, and the next step. The seed has none, so the answer gets two (a GET patch, never a write).
test("US-99: the Commitments block", async ({ page }) => {
  const steps = { research: "research what the promise needs (the thread, the links, the code)", draft: "draft the follow-up message (a draft only; the owner sends it)", status: "report status to the owner" };
  const row = (name: string, title: string, to: string, due: string, overdue: boolean, next: string) => ({ name, title, to, due, overdue, made_at: `${TODAY}T08:05:00Z`, url: "https://example.com/threads/1", parent: "", next, steps: {}, closed: "" });
  await patch(page, "**/api/promises", () => ({
    steps,
    promises: [
      row("promise-0000000a", "Send the rollout notes", "Kim", addDays(TODAY, -1), true, "draft"),
      row("promise-0000000b", "Look into the timeout", "Sam", addDays(TODAY, 1), false, "research"),
    ],
  }));
  await open(page, "dashboard");
  await shot(region(page, "Commitments (2)"), "commitments");
});
