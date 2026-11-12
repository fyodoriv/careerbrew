import { addDays, open, patch, region, shot, test, TODAY } from "./helpers";

// US-93: a prep project's card in the Watch list shows its readiness gates; the open ones carry the command that
// closes them. The seed has no prep project, so the Work answer gets one (a GET patch, never a write).
test("US-93: a prep project's readiness gates", async ({ page }) => {
  const gate = (g: string, text: string, done: boolean, note: string, fix: string) => ({ gate: g, text, done, at: done ? `${TODAY}T09:00:00+00:00` : null, note, fix: done ? "" : fix });
  const prep = {
    event_on: addDays(TODAY, 1), done: 2, total: 4, ready: false, quizzes: 1, quiz: { at: `${TODAY}T09:00:00+00:00`, asked: 10, missed: 2, score: 0.8 },
    gates: [
      gate("tool", "try the tool hands-on (the app, the coding pad, the deck)", true, "", ""),
      gate("links", "read every source link in the pack", true, "2 of 2 read by you", ""),
      gate("questions", "write your own questions", false, "", "careerbrew work project gate panel-prep questions"),
      gate("quiz", "pass the agent quiz on the format, rubric, questions, and logistics", false, "last: 8 of 10, 2 miss(es)", "careerbrew work project quiz panel-prep --asked <n> --missed <n> (re-quiz the 2 miss(es))"),
    ],
  };
  const card = { name: "panel-prep", title: "Panel interview prep", group: "", parent: "", archived: false, stale_after_days: 7, stale: 0, last_checked_at: `${TODAY}T09:00:00+00:00`, latest_update: null,
    sources: [{ kind: "url", ref: "https://example.com/rubric", checked_at: `${TODAY}T09:00:00+00:00`, stale: false, reference: false, reader: "careerbrew" }], prep };
  await patch<{ watched?: unknown[] }>(page, "**/api/work", (w) => ({ ...w, watched: [card] }));
  await open(page, "work");
  await shot(region(page, "panel-prep"), "prep-gates");
});
