import { expect } from "@playwright/test";
import { MODES, geometry, open, preserveFold, record, state, test } from "./comfort-geometry";

for (const mode of MODES) {
    test(`comfort targets preserves complete and unknown PM geometry (${mode.id})`, async ({ page, proof }, info) => {
      const ready = await open(page, proof, "crowded");
      expect(ready.labels).toHaveLength(15);
      const before = state("crowded");
      proof.measured = true;
      await preserveFold(page);
      expect(state("crowded"), "fold controls changed synthetic state").toEqual(before);
      for (const phase of ["complete", "unknown"]) {
        if (phase === "unknown") await page.getByLabel("PM update", { exact: true }).fill("an unlabelled nonblank paragraph");
        const measured = await geometry(page);
        record(info, `${phase}-geometry.json`, measured);
        await page.locator("main summary").first().scrollIntoViewIfNeeded();
        await page.screenshot({ path: info.outputPath(`${mode.id}-${phase}.png`) });
        expect.soft(measured.reducedMotion).toBe(true);
        expect.soft(measured.small).toEqual([]);
        expect.soft(measured.sideways).toBe(false);
        expect.soft(measured.targets).toEqual([]);
        expect(measured.controls.length).toBeGreaterThan(28);
        expect(measured.controls.some((c) => c.disabled)).toBe(true);
        expect(state("crowded"), "read-only geometry changed synthetic state").toEqual(before);
      }
    });
}
