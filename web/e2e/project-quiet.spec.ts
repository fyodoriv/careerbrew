import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { NOW } from "../playwright.config";
import { REPAIR_TOKEN, repairServer } from "./mcp-repair-fixture";

const headers = { "X-Careerbrew-Token": REPAIR_TOKEN };

test("Actions: set and clear a project's quiet date", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  const tomorrow = new Date(`${NOW.slice(0, 10)}T00:00:00Z`);
  tomorrow.setUTCDate(tomorrow.getUTCDate() + 1);
  const quietDate = tomorrow.toISOString().slice(0, 10);
  try {
    const created = await page.request.post(`${fixture.base}/api/actions/work.project.add`, { headers, data: { values: { name: "quiet-test" }, confirm: false } });
    expect((await created.json()).exit).toBe(0);
    await page.goto(`${fixture.base}/#/actions/work.project.set&token=${REPAIR_TOKEN}`);
    const form = page.getByRole("form", { name: "Run work.project.set" });
    await form.getByLabel("name", { exact: true }).fill("quiet-test");
    await form.getByLabel("quiet_until", { exact: true }).fill(quietDate);
    const [saved] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/api/actions/work.project.set") && r.request().method() === "POST"), form.getByRole("button", { name: "Run", exact: true }).click()]);
    expect((await saved.json()).exit).toBe(0);
    const list = await page.request.post(`${fixture.base}/api/actions/work.project.list`, { headers, data: { values: { json: true }, confirm: false } });
    expect((await list.json()).output).toContain(`"quiet_until": "${quietDate}"`);
    await form.getByLabel("quiet_until", { exact: true }).fill("none");
    const [removed] = await Promise.all([page.waitForResponse((r) => r.url().endsWith("/api/actions/work.project.set") && r.request().method() === "POST"), form.getByRole("button", { name: "Run", exact: true }).click()]);
    expect((await removed.json()).exit).toBe(0);
    const cleared = await page.request.post(`${fixture.base}/api/actions/work.project.list`, { headers, data: { values: { json: true }, confirm: false } });
    expect((await cleared.json()).output).toContain('"quiet_until": ""');
  } finally {
    await fixture.stop();
  }
});

test("Actions: project form meets the comfort budget", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  try {
    for (const width of [390, 1280]) {
      for (const colorScheme of ["light", "dark"] as const) {
        await page.setViewportSize({ width, height: 800 });
        await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
        await page.goto(`${fixture.base}/#/actions/work.project.set&token=${REPAIR_TOKEN}`);
        const form = page.getByRole("form", { name: "Run work.project.set" });
        await expect(form).toBeVisible();
        const axe = await new AxeBuilder({ page }).include("form[aria-label='Run work.project.set']").analyze();
        expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
        const geometry = await form.evaluate((element) => ({
          overflow: document.documentElement.scrollWidth > window.innerWidth,
          controls: [...element.querySelectorAll<HTMLElement>("input, select, button")].map((control) => {
            const rect = control.getBoundingClientRect();
            return { name: control.getAttribute("aria-label") || control.textContent, width: rect.width, height: rect.height, font: parseFloat(getComputedStyle(control).fontSize) };
          }),
        }));
        expect(geometry.overflow).toBe(false);
        expect(geometry.controls.filter((control) => control.width < 44 || control.height < 44 || control.font < 14), `${width}px ${colorScheme}`).toEqual([]);
        const hint = await form.getByLabel("quiet_until", { exact: true }).evaluate((input: HTMLInputElement) => {
          const style = getComputedStyle(input);
          const canvas = document.createElement("canvas");
          const context = canvas.getContext("2d")!;
          context.font = style.font;
          return { text: input.placeholder, width: context.measureText(input.placeholder).width, available: input.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight) };
        });
        expect(hint.text).toContain("YYYY-MM-DD");
        expect(hint.text).toContain("none");
        expect(hint.width, `${width}px ${colorScheme}: date hint fits`).toBeLessThanOrEqual(hint.available);
      }
    }
  } finally {
    await fixture.stop();
  }
});
