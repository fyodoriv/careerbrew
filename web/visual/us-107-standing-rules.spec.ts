import { expect, open, shot, test } from "./helpers";

for (const action of ["work.project.rules", "work.project.rules-set"]) {
  test(`US-107: ${action} form`, async ({ page }) => {
    await open(page, `actions/${action}`);
    const form = page.getByRole("form", { name: `Run ${action}`, exact: true });
    await expect(form).toBeVisible();
    await shot(form, action.endsWith("rules-set") ? "rules-write" : "rules-read");
  });
}
