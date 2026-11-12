import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import AxeBuilder from "@axe-core/playwright";
import { expect, test } from "@playwright/test";
import { NOW } from "../playwright.config";
import { REPAIR_TOKEN, repairServer } from "./mcp-repair-fixture";

test("Actions: profile update hides values and changes encrypted data", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  try {
    const profile = join(dirname(dirname(fixture.target)), "data", "profile.yaml");
    const before = readFileSync(profile);
    const canary = "SYNTHETIC-PROFILE-CANARY";
    await page.goto(`${fixture.base}/#/actions/data.set&token=${REPAIR_TOKEN}`);
    const form = page.getByRole("form", { name: "Run data.set" });
    await expect(form).toBeVisible();
    await form.getByLabel("file", { exact: true }).fill("profile.yaml");
    await form.getByLabel("name", { exact: true }).fill(canary);
    await form.getByLabel("employer", { exact: true }).fill("Synthetic Employer");
    await form.getByRole("button", { name: "Run", exact: true }).click();
    const result = form.getByLabel("Result");
    await expect(result).toContainText("exit 0");
    await expect(result).toContainText("[arguments redacted]");
    await expect(result).not.toContainText(canary);
    await expect(result).not.toContainText("Synthetic Employer");
    const after = readFileSync(profile);
    expect(after.equals(before)).toBe(false);
    expect(after.subarray(0, 5).toString()).toBe("CBV1\n");
    expect(after.includes(canary)).toBe(false);
  } finally {
    await fixture.stop();
  }
});

test("Actions: profile form meets the comfort budget", async ({ page }) => {
  test.setTimeout(180_000);
  const fixture = await repairServer(undefined, NOW);
  try {
    for (const width of [390, 1280]) {
      for (const colorScheme of ["light", "dark"] as const) {
        await page.setViewportSize({ width, height: 800 });
        await page.emulateMedia({ colorScheme, reducedMotion: "reduce" });
        await page.goto(`${fixture.base}/#/actions/data.set&token=${REPAIR_TOKEN}`);
        const form = page.getByRole("form", { name: "Run data.set" });
        await expect(form).toBeVisible();
        const axe = await new AxeBuilder({ page }).include("form[aria-label='Run data.set']").analyze();
        expect(axe.violations.filter((v) => v.impact === "serious" || v.impact === "critical")).toEqual([]);
        const geometry = await form.evaluate((element) => ({
          overflow: document.documentElement.scrollWidth > window.innerWidth,
          controls: [...element.querySelectorAll<HTMLElement>("input, button")].map((control) => {
            const rect = control.getBoundingClientRect();
            return { name: control.getAttribute("aria-label") || control.textContent, width: rect.width, height: rect.height, font: parseFloat(getComputedStyle(control).fontSize) };
          }),
        }));
        expect(geometry.overflow).toBe(false);
        expect(geometry.controls.filter((control) => control.width < 44 || control.height < 44 || control.font < 14), `${width}px ${colorScheme}`).toEqual([]);
      }
    }
  } finally {
    await fixture.stop();
  }
});
