import { TODAY, open, region, shot, teachStep, test } from "./helpers";

// US-45: one step at a time: with no goal, only "Set a goal first"; with one, a lesson; an agent's request comes first.
test("US-45: with no goal, the teacher asks for one", async ({ page }) => {
  await open(page, "teach");
  await shot(region(page, "Set a goal first"), "set-a-goal-first");
});

test("US-45: a lesson step", async ({ page }) => {
  await teachStep(page, {
    kind: "lesson", title: "Lesson: arrays and hashing", why: "learn the idea first: the next exercise uses it", pattern: "arrays and hashing",
    lesson: "A hash set answers \"have I seen this value?\" in O(1) on average. Walk the array once and keep what you saw.",
    prompt: "When would you use a set instead of sorting first?",
    example: { title: "Contains duplicate", statement: "Return True when any value appears twice.", solution: "def contains_duplicate(nums):\n    return len(set(nums)) < len(nums)" },
  });
  await open(page, "teach");
  await shot(region(page, "Lesson: arrays and hashing"), "lesson-step");
});

test("US-45: an agent's request to approve comes first", async ({ page }) => {
  await teachStep(page, {
    kind: "approve", title: "Approve or reject: send the resume to Northwind", why: "An agent is waiting for your decision.",
    approval: { id: "a1b2c3d4", title: "send the resume to Northwind", detail: "Send the tailored resume for **Staff Software Engineer, Platform**.", by: "agent", at: `${TODAY}T08:45:00+00:00`, status: "pending", note: "" },
  });
  await open(page, "teach");
  await shot(region(page, "Approve or reject: send the resume to Northwind"), "approve-step");
});
