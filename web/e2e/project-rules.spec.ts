import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { NOW } from "../playwright.config";
import { REPAIR_TOKEN, repairServer } from "./mcp-repair-fixture";

const headers = { "X-Careerbrew-Token": REPAIR_TOKEN };
test.use({ actionTimeout: 15_000 });

test("Actions: project rules require confirmation and read the saved value", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  try {
    const created = await page.request.post(`${fixture.base}/api/actions/work.project.add`, { headers, data: { values: { name: "rules-test" }, confirm: false } });
    expect((await created.json()).exit).toBe(0);
    await page.goto(`${fixture.base}/#/actions/work.project.rules-set&token=${REPAIR_TOKEN}`);
    const form = page.getByRole("form", { name: "Run work.project.rules-set", exact: true });
    await form.getByLabel("name", { exact: true }).fill("rules-test");
    await form.getByLabel("text", { exact: true }).fill("Synthetic standing rule");
    const run = form.getByRole("button", { name: "Run", exact: true });
    const confirmation = form.getByRole("checkbox", { name: /^I understand: it / });
    await expect(form.getByRole("checkbox", { name: "yes", exact: true })).toHaveCount(0);
    await expect(run).toBeDisabled();
    const before = await page.request.post(`${fixture.base}/api/actions/work.project.rules`, { headers, data: { values: { name: "rules-test", json: true }, confirm: false } });
    expect(JSON.parse((await before.json()).output).rules).toBe("");
    await confirmation.check();
    const [saved] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/api/actions/work.project.rules-set") && r.request().method() === "POST"), run.click()]);
    expect((await saved.json()).exit).toBe(0);
    await page.getByRole("button", { name: "work.project.rules", exact: true }).click();
    const reader = page.getByRole("form", { name: "Run work.project.rules", exact: true });
    await reader.getByLabel("name", { exact: true }).fill("rules-test");
    await reader.getByRole("checkbox", { name: "json", exact: true }).check();
    const [read] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/api/actions/work.project.rules") && r.request().method() === "POST"), reader.getByRole("button", { name: "Run", exact: true }).click()]);
    expect(JSON.parse((await read.json()).output).rules).toBe("Synthetic standing rule");
    await page.getByRole("button", { name: "work.project.rules-set", exact: true }).click();
    await form.getByLabel("name", { exact: true }).fill("rules-test");
    await form.getByRole("checkbox", { name: "clear", exact: true }).check();
    await confirmation.check();
    const [cleared] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/api/actions/work.project.rules-set") && r.request().method() === "POST"), run.click()]);
    expect((await cleared.json()).exit).toBe(0);
    const after = await page.request.post(`${fixture.base}/api/actions/work.project.rules`, { headers, data: { values: { name: "rules-test", json: true }, confirm: false } });
    expect(JSON.parse((await after.json()).output).rules).toBe("");
  } finally {
    await fixture.stop();
  }
});

test("Actions: standing rules forms meet the comfort budget", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  try {
    for (const action of ["work.project.rules", "work.project.rules-set"]) {
      for (const width of [390, 1280]) {
        for (const colorScheme of ["light", "dark"] as const) {
          await page.setViewportSize({ width, height: 800 });
          await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
          await page.goto(`${fixture.base}/#/actions/${action}&token=${REPAIR_TOKEN}`);
          await page.reload();
          const form = page.getByRole("form", { name: `Run ${action}`, exact: true });
          await expect(form).toBeVisible();
          const axe = await new AxeBuilder({ page }).include(`form[aria-label='Run ${action}']`).analyze();
          expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
          const geometry = await form.evaluate((element) => ({
            overflow: document.documentElement.scrollWidth > window.innerWidth,
            controls: [...element.querySelectorAll<HTMLElement>("input, select, button")].map((control) => {
              const target = control instanceof HTMLInputElement && control.type === "checkbox" ? control.closest("label") ?? control : control;
              const rect = target.getBoundingClientRect();
              return { name: target.getAttribute("aria-label") || target.textContent, width: rect.width, height: rect.height, font: parseFloat(getComputedStyle(target).fontSize) };
            }),
          }));
          expect(geometry.overflow).toBe(false);
          expect(geometry.controls.filter((control) => control.width < 44 || control.height < 44 || control.font < 14), `${action} ${width}px ${colorScheme}`).toEqual([]);
          if (action === "work.project.rules-set") {
            const hint = await form.getByLabel("text", { exact: true }).evaluate((input: HTMLInputElement) => {
              const style = getComputedStyle(input);
              const context = document.createElement("canvas").getContext("2d")!;
              context.font = style.font;
              return { width: context.measureText(input.placeholder).width, available: input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) };
            });
            expect(hint.width, `${width}px ${colorScheme}: rule hint fits`).toBeLessThanOrEqual(hint.available);
          }
        }
      }
    }
  } finally {
    await fixture.stop();
  }
});
