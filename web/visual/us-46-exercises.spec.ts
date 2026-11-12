import { answer, expect, open, region, shot, teachStep, test } from "./helpers";

// US-46: an exercise in the curriculum order, run locally, graded on tests, complexity, and readability.
const EXERCISE = {
  kind: "exercise", title: "Contains duplicate", why: "next in the curriculum: arrays and hashing", attempts: 0,
  exercise: {
    id: "contains-duplicate", order: 1, pattern: "arrays and hashing", difficulty: "easy", title: "Contains duplicate",
    statement: "Given an integer array `nums`, return `True` if any value appears at least twice, and `False` if every value is distinct.",
    signature: "def contains_duplicate(nums: list[int]) -> bool:", lesson: "", starter: "def contains_duplicate(nums):\n    pass\n", hints: [],
    tests: [{ args: [[1, 2, 3, 1]], expected: true, note: "a repeat" }, { args: [[1, 2, 3, 4]], expected: false, note: "all distinct" }],
    watch_out: ["off by one in the loop bound"],
  },
};

test("US-46: an exercise step", async ({ page }) => {
  await teachStep(page, EXERCISE);
  await open(page, "teach");
  await shot(region(page, "Contains duplicate"), "exercise-step");
});

test("US-46: a graded attempt", async ({ page }) => {
  await teachStep(page, EXERCISE);
  await answer(page, "**/api/teach/complete", {
    message: "All tests pass in O(n). Next: two pointers.",
    next: { kind: "done", title: "All done for today", why: "you finished today's steps", started: "", summary: { steps: 3, minutes: 25, by_kind: {}, mistake_repeat_rate: null } },
    result: { grade: { passed: 2, total: 2, correct: true, complexity: { expected: "O(n)", ratio: 1.1, ok: true }, style: [], score: 1 }, error: null, failing: [], mistakes: [], solution: "def contains_duplicate(nums):\n    return len(set(nums)) < len(nums)" },
  });
  await open(page, "teach");
  await page.getByLabel("Your solution").fill("def contains_duplicate(nums):\n    return len(set(nums)) < len(nums)\n");
  await page.getByRole("button", { name: "Run and submit" }).click();
  await expect(page.getByRole("status")).toContainText("All tests pass");
  await shot(region(page, "Your last attempt"), "graded-attempt");
});
