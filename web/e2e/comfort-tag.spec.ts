import { expect } from "@playwright/test";
import { MODES, geometry, open, preserveFold, record, state, test } from "./comfort-geometry";

for (const mode of MODES) {
    test(`comfort tag preserves complete and unknown PM geometry (${mode.id})`, async ({ page, proof }, info) => {
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
        await page.locator(".agenda .block:last-child .tag").scrollIntoViewIfNeeded();
        await page.screenshot({ path: info.outputPath(`${mode.id}-${phase}.png`) });
        expect.soft(measured.reducedMotion).toBe(true);
        expect.soft(measured.small).toEqual([]);
        expect.soft(measured.sideways).toBe(false);
        expect.soft(measured.spills).toEqual([]);
        const long = measured.tags.find((t) => t.text === ready.labels.at(-1));
        expect(long, "complete long project tag is present").toBeDefined();
        if (long) {
          expect.soft(long.innerText).toBe(ready.labels.at(-1));
          expect.soft(long.rect.left).toBeGreaterThanOrEqual(long.parent.left - 1);
          expect.soft(long.rect.right).toBeLessThanOrEqual(long.parent.right + 1);
          expect.soft(long.overflowX).toBe("visible");
          expect.soft(long.overflowY).toBe("visible");
          expect.soft(long.textOverflow).not.toBe("ellipsis");
          expect.soft(parseFloat(long.fontSize)).toBeGreaterThanOrEqual(14);
          for (const line of long.lines) {
            expect.soft(line.left).toBeGreaterThanOrEqual(long.rect.left - 1);
            expect.soft(line.right).toBeLessThanOrEqual(long.rect.right + 1);
          }
        }
        expect(state("crowded"), "read-only geometry changed synthetic state").toEqual(before);
      }
    });
}
