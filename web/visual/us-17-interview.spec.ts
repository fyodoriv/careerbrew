import { open, region, shot, test } from "./helpers";

// US-17: a company's whole interview loop, each step with what it evaluates and the company's own notes.
test("US-17: a company's interview loop", async ({ page }) => {
  await open(page, "interviews/Datadog");
  await shot(region(page, "Interview loop: Datadog"), "datadog-loop");
});
