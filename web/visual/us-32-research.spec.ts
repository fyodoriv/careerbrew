import { open, region, shot, test } from "./helpers";

// US-32: each company's rule on AI tools in interviews, with the date it was checked.
test("US-32: a company's AI policy with its checked date", async ({ page }) => {
  await open(page, "interviews/Datadog");
  await shot(region(page, "AI tools in this loop"), "datadog-ai-policy");
});
