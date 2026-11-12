import { TODAY, answer, expect, open, region, shot, test } from "./helpers";

const REVIEW = new Date(Date.parse(`${TODAY}T00:00:00Z`) + 3 * 86_400_000).toISOString().slice(0, 10);  // the engine's next review, 3 days on

// US-57: hear the question, answer by voice or by typing, and get it graded by the rubric.
test("US-57: a practice question and its graded answer", async ({ page }) => {
  await answer(page, "**/api/drill/voice", {
    transcript: "I would ask whether the array can be empty, then use two pointers from both ends.", source: "text",
    row: { met: ["clarify", "approach", "complexity"], missed: ["correct", "test"], score: 0.6, rating: "hard" },
    // the engine's own shape (tutor/engine.py feedback): the score line, then one "Practice:" line per missed criterion
    feedback: [`Score 60% -> hard; next review ${REVIEW}.`, "Practice: Final code is correct on normal and edge cases", "Practice: Walked through at least one test case by hand"],
  });
  await open(page, "interviews");
  const voice = region(page, "Practice by voice");
  await shot(voice, "question");
  await voice.getByLabel("Typed answer").fill("I would ask whether the array can be empty, then use two pointers from both ends.");
  await voice.getByRole("button", { name: "Grade typed answer" }).click();
  await expect(voice.getByText(/^Met: /)).toBeVisible();
  await shot(voice, "graded");
});
