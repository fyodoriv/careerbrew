import { expect, test } from "@playwright/test";

const token = "e2e-token-not-secret";

test("owner reviews and reopens a promise through the real app", async ({ page }) => {
  const errors: string[] = [];
  page.on("pageerror", (error) => errors.push(error.message));
  await page.goto(`/#/actions&token=${token}`);
  await page.getByLabel("Filter actions").fill("work.promise.accept");
  await page.getByRole("button", { name: "work.promise.accept", exact: true }).click();
  const form = page.getByRole("form", { name: "Run work.promise.accept" });
  await expect(form.getByText(/prior moved, due Wed Nov 4; current open, due Wed Nov 4/)).toBeVisible();
  await expect(form.getByRole("radio", { name: /Keep the prior decision, due Wed Nov 4/ })).toBeVisible();
  await expect(form.getByRole("radio", { name: /Reopen, due Mon Nov 2; create a separate promise due Wed Nov 4/ })).toBeVisible();
  await expect(form.getByRole("radio", { name: /Reopen, due Mon Nov 2/ })).toBeVisible();
  await expect(form.getByRole("button", { name: "Run" })).toBeDisabled();
  await form.getByRole("radio", { name: /Reopen, due Mon Nov 2/ }).check();
  await expect(form.getByRole("button", { name: "Run" })).toBeDisabled();
  await form.getByRole("checkbox", { name: /I understand/ }).check();
  await form.getByRole("button", { name: "Run" }).click();
  await expect(form.getByLabel("Result")).toContainText("exit 0");
  const response = await page.request.get("/api/promises", { headers: { "X-Careerbrew-Token": token } });
  const data = await response.json() as { promises: { title: string; due: string }[] };
  expect(data.promises.some((row) => row.title === "Ship the gap sample" && row.due === "2026-11-02")).toBe(true);
  expect(data.promises.some((row) => row.title === "Ship the gap sample" && row.due === "2026-11-04")).toBe(true);
  expect(errors).toEqual([]);
});
