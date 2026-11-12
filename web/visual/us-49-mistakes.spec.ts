import { TODAY, open, region, shot, teachStep, test } from "./helpers";

// US-49: a repeated mistake becomes the next step, named, explained, with how to prevent it.
test("US-49: a repeated mistake comes first", async ({ page }) => {
  await teachStep(page, {
    kind: "mistake", title: "This mistake repeated: edge case in two pointers", why: "Repeated mistakes get fixed first, so they stop.",
    mistake: { at: `${TODAY}T08:30:00+00:00`, source: "exercise:two-sum-ii", pattern: "two pointers", kind: "edge-case", detail: "failed on an empty list: IndexError: list index out of range (line 3)", tip: "Write the edge cases first: empty input, one element, duplicates, negatives, and the largest values.", repeat: true },
  });
  await open(page, "teach");
  await shot(region(page, "This mistake repeated: edge case in two pointers"), "mistake-step");
});
