import { expect } from "@playwright/test";
import { MODES, open, record, state, test } from "../e2e/comfort-geometry";
import { measurePaint } from "../e2e/comfort-paint";

const CONTROLS = [
  { text: "fixed simple pass", verdict: "pass", min: 20.9, max: 21.1 },
  { text: "fixed gradient pass", verdict: "pass", min: 4.5, max: 5 },
  { text: "fixed gradient fail", verdict: "fail", min: 1, max: 1.01 },
  { text: "fixed group opacity pass", verdict: "pass", min: 5, max: 6 },
  { text: "fixed group opacity fail", verdict: "fail", min: 3.8, max: 4.1 },
] as const;

async function calibrate(page: Parameters<typeof measurePaint>[0]) {
  await page.locator("main").evaluate((main) => {
    const fixture = document.createElement("section");
    fixture.setAttribute("data-paint-controls", "");
    fixture.innerHTML = `
      <div style="background:#fff;color:#000">fixed simple pass</div>
      <div style="width:max-content;padding:0 2px;background:linear-gradient(90deg,#fff 0%,#767676 calc(100% - 4px));color:#000">fixed gradient pass</div>
      <div style="width:max-content;padding:0 2px;background:linear-gradient(90deg,#fff 0%,#000 calc(100% - 4px));color:#000">fixed gradient fail</div>
      <div style="background:#000"><div style="opacity:.5;background:#000;color:#fff">fixed group opacity pass</div></div>
      <div style="background:#fff"><div style="opacity:.5;background:#fff;color:#000">fixed group opacity fail</div></div>`;
    main.append(fixture);
  });
  try {
    const measured = await measurePaint(page, "[data-paint-controls]");
    expect(measured).toHaveLength(CONTROLS.length);
    for (const control of CONTROLS) {
      const actual = measured.find((item) => item.text === control.text);
      expect(actual, control.text).toBeDefined();
      expect(actual?.verdict, `${control.text}: ${JSON.stringify(actual)}`).toBe(control.verdict);
      expect(actual?.samples ?? 0, JSON.stringify(actual)).toBeGreaterThan(0);
      expect(actual?.ratio ?? 0, JSON.stringify(actual)).toBeGreaterThanOrEqual(control.min);
      expect(actual?.ratio ?? Infinity, JSON.stringify(actual)).toBeLessThanOrEqual(control.max);
    }
    return measured;
  } finally {
    await page.locator("[data-paint-controls]").evaluate((fixture) => fixture.remove());
  }
}

for (const mode of MODES) {
  test(`paint resolves actual and fixed compositions (${mode.id})`, async ({ page, proof }, info) => {
    await open(page, proof, "crowded");
    const before = state("crowded");
    proof.measured = true;
    for (const phase of ["complete", "unknown"]) {
      if (phase === "unknown") await page.getByLabel("PM update", { exact: true }).fill("an unlabelled nonblank paragraph");
      const controls = await calibrate(page);
      const measured = await measurePaint(page);
      record(info, `${phase}-paint.json`, { controls, measured });
      expect(measured.length).toBeGreaterThan(20);
      expect(measured.some((item) => item.text.startsWith("Send to Slack")), "disabled Send is in the denominator").toBe(true);
      expect(measured.filter((item) => item.verdict === "unknown"), "no unsupported composition becomes a pass").toEqual([]);
      expect(measured.filter((item) => item.verdict === "fail"), "every measured text composition meets 4.5:1").toEqual([]);
      expect(state("crowded"), "read-only paint measurement changed synthetic state").toEqual(before);
      await page.screenshot({ path: info.outputPath(`${mode.id}-${phase}-paint.png`), fullPage: true });
    }
  });
}
